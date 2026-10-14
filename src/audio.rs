//! CoreAudio: output device list/volume + per-app audio processes.

#![allow(dead_code)]

use crate::engine::AudioApp;
use crate::ffi::*;
use std::ffi::c_void;

#[derive(Clone, Default)]
pub struct OutputState {
    pub id: u32,
    pub name: String,
    pub volume: Option<f32>,
    pub muted: Option<bool>,
    pub can_volume: bool,
    pub can_mute: bool,
}

fn addr(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    }
}

unsafe fn get<T: Copy>(obj: AudioObjectID, a: &AudioObjectPropertyAddress, out: &mut T) -> bool {
    let mut size = std::mem::size_of::<T>() as u32;
    AudioObjectGetPropertyData(
        obj,
        a,
        0,
        std::ptr::null(),
        &mut size,
        out as *mut T as *mut c_void,
    ) == 0
}

unsafe fn set<T: Copy>(obj: AudioObjectID, a: &AudioObjectPropertyAddress, v: &T) -> bool {
    AudioObjectSetPropertyData(
        obj,
        a,
        0,
        std::ptr::null(),
        std::mem::size_of::<T>() as u32,
        v as *const T as *const c_void,
    ) == 0
}

pub fn object_list(sel: u32) -> Vec<AudioObjectID> {
    unsafe {
        let a = addr(sel, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL);
        let mut size = 0u32;
        if AudioObjectGetPropertyDataSize(
            K_AUDIO_OBJECT_SYSTEM_OBJECT,
            &a,
            0,
            std::ptr::null(),
            &mut size,
        ) != 0
            || size == 0
        {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / 4];
        if AudioObjectGetPropertyData(
            K_AUDIO_OBJECT_SYSTEM_OBJECT,
            &a,
            0,
            std::ptr::null(),
            &mut size,
            ids.as_mut_ptr() as *mut c_void,
        ) != 0
        {
            return Vec::new();
        }
        ids
    }
}

fn cfstr_value(obj: AudioObjectID, sel: u32, scope: u32) -> Option<String> {
    unsafe {
        let a = addr(sel, scope);
        let mut cf: CFTypeRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFTypeRef>() as u32;
        if AudioObjectGetPropertyData(
            obj,
            &a,
            0,
            std::ptr::null(),
            &mut size,
            &mut cf as *mut CFTypeRef as *mut c_void,
        ) != 0
            || cf.is_null()
        {
            return None;
        }
        let s = crate::metrics::cf_to_string(cf);
        CFRelease(cf);
        s
    }
}

pub fn default_output_device() -> AudioObjectID {
    unsafe {
        let mut id: AudioObjectID = 0;
        get(
            K_AUDIO_OBJECT_SYSTEM_OBJECT,
            &addr(
                K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
                K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            ),
            &mut id,
        );
        id
    }
}

pub fn device_name(id: AudioObjectID) -> String {
    cfstr_value(
        id,
        K_AUDIO_DEVICE_PROPERTY_DEVICE_NAME_CFSTRING,
        K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
    )
    .unwrap_or_else(|| crate::locale::text("Output", "Utgang").into())
}

pub fn default_output_name() -> String {
    let d = default_output_device();
    if d == 0 {
        crate::locale::text("Default output", "Standardutgang").into()
    } else {
        device_name(d)
    }
}

fn output_property(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
        mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    }
}

unsafe fn property_settable(
    id: AudioObjectID,
    address: &AudioObjectPropertyAddress,
) -> Result<bool, OSStatus> {
    if AudioObjectHasProperty(id, address) == 0 {
        return Ok(false);
    }
    let mut settable: Boolean = 0;
    let status = AudioObjectIsPropertySettable(id, address, &mut settable);
    readonly_settable_result(status, settable)
}

pub fn output_state() -> OutputState {
    let id = default_output_device();
    if id == 0 {
        return OutputState {
            name: crate::locale::text("Default output", "Standardutgang").into(),
            ..Default::default()
        };
    }
    let volume_address = output_property(K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR);
    let mute_address = output_property(K_AUDIO_DEVICE_PROPERTY_MUTE);
    unsafe {
        let mut volume = 0.0f32;
        let mut muted = 0u32;
        let volume = get(id, &volume_address, &mut volume)
            .then_some(volume)
            .filter(|volume| valid_volume(*volume));
        let muted = get(id, &mute_address, &mut muted).then_some(muted != 0);
        let can_volume = property_settable(id, &volume_address).unwrap_or(false);
        let can_mute = property_settable(id, &mute_address).unwrap_or(false);
        OutputState {
            id,
            name: device_name(id),
            volume,
            muted,
            can_volume,
            can_mute,
        }
    }
}

fn stale_output_error() -> String {
    crate::locale::text(
        "The selected audio output is no longer active.",
        "Den valgte lydutgangen er ikke lenger aktiv.",
    )
    .into()
}

fn unsupported_control_error(control: &str) -> String {
    match control {
        "volume" => crate::locale::text(
            "This audio output does not support volume control.",
            "Denne lydutgangen støtter ikke volumkontroll.",
        )
        .into(),
        _ => crate::locale::text(
            "This audio output does not support mute control.",
            "Denne lydutgangen støtter ikke demping.",
        )
        .into(),
    }
}

fn os_status_error(control: &str, status: OSStatus) -> String {
    let message = match control {
        "volume" => crate::locale::text(
            "Could not access volume control.",
            "Kunne ikke åpne volumkontrollen.",
        ),
        _ => crate::locale::text(
            "Could not access mute control.",
            "Kunne ikke åpne dempekontrollen.",
        ),
    };
    format!("{message} (OSStatus {status})")
}

pub fn adjust_output_volume(expected_id: u32, delta: f32) -> Result<(), String> {
    let value = adjusted_volume(&output_state(), expected_id, delta)?;
    set_output_volume(expected_id, value)
}

fn adjusted_volume(state: &OutputState, expected_id: u32, delta: f32) -> Result<f32, String> {
    if !target_is_current(expected_id, state.id) {
        return Err(stale_output_error());
    }
    if !state.can_volume || !valid_volume(delta.abs()) {
        return Err(unsupported_control_error("volume"));
    }
    let volume = state
        .volume
        .filter(|value| valid_volume(*value))
        .ok_or_else(|| unsupported_control_error("volume"))?;
    Ok((volume + delta).clamp(0., 1.))
}

pub fn set_output_volume(expected_id: u32, value: f32) -> Result<(), String> {
    if !valid_volume(value) {
        return Err(crate::locale::text(
            "Volume must be a finite value from 0 to 1.",
            "Volum må være en endelig verdi fra 0 til 1.",
        )
        .into());
    }
    let state = output_state();
    if !target_is_current(expected_id, state.id) {
        return Err(stale_output_error());
    }
    let address = output_property(K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR);
    unsafe {
        match property_settable(state.id, &address) {
            Ok(true) => {}
            Ok(false) => return Err(unsupported_control_error("volume")),
            Err(status) => return Err(os_status_error("volume", status)),
        }
        let status = AudioObjectSetPropertyData(
            state.id,
            &address,
            0,
            std::ptr::null(),
            std::mem::size_of::<f32>() as u32,
            &value as *const f32 as *const c_void,
        );
        if status == 0 {
            Ok(())
        } else {
            Err(os_status_error("volume", status))
        }
    }
}

pub fn set_output_mute(expected_id: u32, value: bool) -> Result<(), String> {
    let state = output_state();
    if !target_is_current(expected_id, state.id) {
        return Err(stale_output_error());
    }
    let address = output_property(K_AUDIO_DEVICE_PROPERTY_MUTE);
    let muted: u32 = u32::from(value);
    unsafe {
        match property_settable(state.id, &address) {
            Ok(true) => {}
            Ok(false) => return Err(unsupported_control_error("mute")),
            Err(status) => return Err(os_status_error("mute", status)),
        }
        let status = AudioObjectSetPropertyData(
            state.id,
            &address,
            0,
            std::ptr::null(),
            std::mem::size_of::<u32>() as u32,
            &muted as *const u32 as *const c_void,
        );
        if status == 0 {
            Ok(())
        } else {
            Err(os_status_error("mute", status))
        }
    }
}

pub fn toggle_output_mute(expected_id: u32) -> Result<(), String> {
    let value = toggled_mute_value(&output_state(), expected_id)?;
    set_output_mute(expected_id, value)
}

fn toggled_mute_value(state: &OutputState, expected_id: u32) -> Result<bool, String> {
    if !target_is_current(expected_id, state.id) {
        return Err(stale_output_error());
    }
    if !state.can_mute {
        return Err(unsupported_control_error("mute"));
    }
    state
        .muted
        .map(|muted| !muted)
        .ok_or_else(|| unsupported_control_error("mute"))
}

fn valid_volume(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn target_is_current(expected_id: u32, current_id: u32) -> bool {
    expected_id != 0 && expected_id == current_id
}

fn readonly_settable_result(status: OSStatus, settable: Boolean) -> Result<bool, OSStatus> {
    if status == 0 {
        Ok(settable != 0)
    } else {
        Err(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_range_rejects_nonfinite_and_out_of_range_values() {
        assert!(!valid_volume(f32::NAN));
        assert!(!valid_volume(-0.01));
        assert!(valid_volume(0.0));
        assert!(valid_volume(1.0));
        assert!(!valid_volume(1.01));
    }

    #[test]
    fn stale_output_identity_is_rejected() {
        assert!(!target_is_current(0, 42));
        assert!(!target_is_current(42, 43));
        assert!(target_is_current(42, 42));
    }

    #[test]
    fn adjustments_use_current_volume_and_reject_unavailable_targets() {
        let mut state = OutputState {
            id: 42,
            volume: Some(0.5),
            can_volume: true,
            ..OutputState::default()
        };
        state.volume = Some(adjusted_volume(&state, 42, 0.1).unwrap());
        assert!((adjusted_volume(&state, 42, 0.1).unwrap() - 0.7).abs() < 0.0001);
        assert_eq!(adjusted_volume(&state, 42, 0.8).unwrap(), 1.);
        assert_eq!(adjusted_volume(&state, 42, -0.8).unwrap(), 0.);
        assert!(adjusted_volume(&state, 43, 0.1).is_err());
        assert!(adjusted_volume(&state, 42, f32::NAN).is_err());
        state.can_volume = false;
        assert!(adjusted_volume(&state, 42, 0.1).is_err());
        state.can_volume = true;
        state.volume = None;
        assert!(adjusted_volume(&state, 42, 0.1).is_err());
    }

    #[test]
    fn mute_toggle_uses_fresh_state_and_rejects_unavailable_targets() {
        let mut state = OutputState {
            id: 42,
            muted: Some(true),
            can_mute: true,
            ..OutputState::default()
        };
        assert!(!toggled_mute_value(&state, 42).unwrap());
        state.muted = Some(false);
        assert!(toggled_mute_value(&state, 42).unwrap());
        assert!(toggled_mute_value(&state, 43).is_err());
        state.can_mute = false;
        assert!(toggled_mute_value(&state, 42).is_err());
        state.can_mute = true;
        state.muted = None;
        assert!(toggled_mute_value(&state, 42).is_err());
    }

    #[test]
    fn readonly_property_probe_preserves_osstatus() {
        assert_eq!(readonly_settable_result(0, 1), Ok(true));
        assert_eq!(readonly_settable_result(0, 0), Ok(false));
        assert_eq!(readonly_settable_result(-50, 1), Err(-50));
    }

    #[test]
    #[ignore = "requires a live CoreAudio output device; read-only probe"]
    fn native_output_state_probe() {
        let state = output_state();
        assert!(!state.name.is_empty());
    }

    struct RestoreOutput {
        id: u32,
        volume: f32,
        muted: bool,
    }
    impl Drop for RestoreOutput {
        fn drop(&mut self) {
            let _ = set_output_volume(self.id, self.volume);
            let _ = set_output_mute(self.id, self.muted);
        }
    }

    fn speak(text: &str) {
        // The lines are English; the system voice may not be.
        let say = |voice: &[&str]| {
            std::process::Command::new("/usr/bin/say")
                .args(voice)
                .arg(text)
                .status()
                .unwrap()
                .success()
        };
        assert!(say(&["-v", "Samantha"]) || say(&[]));
    }

    fn observed(label: &str) -> OutputState {
        let state = output_state();
        println!(
            "{label}: id={} name={:?} volume={:?} muted={:?} can_volume={} can_mute={}",
            state.id, state.name, state.volume, state.muted, state.can_volume, state.can_mute
        );
        state
    }

    #[test]
    #[ignore = "changes this Mac's output volume and mute, speaks aloud, then restores; needs a listener"]
    fn native_output_controls_are_audible_and_restored() {
        let before = observed("before");
        assert!(before.id != 0 && before.can_volume && before.can_mute);
        let volume = before.volume.unwrap();
        let muted = before.muted.unwrap();
        let _restore = RestoreOutput {
            id: before.id,
            volume,
            muted,
        };

        assert!(set_output_volume(before.id + 1, 0.).is_err());
        assert!(toggle_output_mute(before.id + 1).is_err());
        let untouched = observed("after stale-target requests");
        assert_eq!(untouched.volume, before.volume);
        assert_eq!(untouched.muted, before.muted);

        let reference = volume.clamp(0.3, 0.5);
        set_output_mute(before.id, false).unwrap();
        set_output_volume(before.id, reference).unwrap();
        observed("reference");
        speak("One. This is the reference volume.");

        set_output_volume(before.id, reference * 0.3).unwrap();
        let quiet = observed("quieter").volume.unwrap();
        assert!((quiet - reference * 0.3).abs() < 0.03);
        speak("Two. This should be much quieter.");

        adjust_output_volume(before.id, reference + 0.2 - quiet).unwrap();
        let loud = observed("louder").volume.unwrap();
        assert!((loud - (reference + 0.2)).abs() < 0.03);
        speak("Three. This should be louder than one.");

        toggle_output_mute(before.id).unwrap();
        assert_eq!(observed("muted").muted, Some(true));
        speak("Four. You should not hear this sentence at all.");

        toggle_output_mute(before.id).unwrap();
        assert_eq!(observed("unmuted").muted, Some(false));
        speak("Five. The sound is back.");

        set_output_volume(before.id, volume).unwrap();
        set_output_mute(before.id, muted).unwrap();
        let after = observed("restored");
        assert_eq!(after.id, before.id);
        assert_eq!(after.muted, before.muted);
        assert!((after.volume.unwrap() - volume).abs() < 0.005);
        speak("Six. The starting volume is restored.");
    }

    struct RestoreDefaultOutput(u32);
    impl Drop for RestoreDefaultOutput {
        fn drop(&mut self) {
            set_default_output(self.0);
        }
    }

    fn set_default_output(id: u32) -> bool {
        unsafe {
            set(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &addr(
                    K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
                    K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
                ),
                &id,
            )
        }
    }

    fn device_volume_and_mute(id: u32) -> (Option<f32>, Option<bool>) {
        unsafe {
            let (mut volume, mut muted) = (0f32, 0u32);
            (
                get(id, &output_property(K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR), &mut volume)
                    .then_some(volume),
                get(id, &output_property(K_AUDIO_DEVICE_PROPERTY_MUTE), &mut muted)
                    .then_some(muted != 0),
            )
        }
    }

    #[test]
    #[ignore = "switches the default output to TIDEGAUGE_SWITCH_OUTPUT, then restores it"]
    fn native_output_switch_rejects_stale_target() {
        let name = std::env::var("TIDEGAUGE_SWITCH_OUTPUT")
            .expect("set TIDEGAUGE_SWITCH_OUTPUT to another output device's name");
        let before = observed("before");
        let target = object_list(K_AUDIO_HARDWARE_PROPERTY_DEVICES)
            .into_iter()
            .find(|id| *id != before.id && device_name(*id) == name)
            .expect("no other device with that name");
        let original = device_volume_and_mute(before.id);
        let _restore = RestoreDefaultOutput(before.id);

        assert!(set_default_output(target));
        let switched = observed("switched");
        assert_eq!(switched.id, target);

        assert!(set_output_volume(before.id, 0.).is_err());
        assert!(adjust_output_volume(before.id, -0.1).is_err());
        assert!(toggle_output_mute(before.id).is_err());
        assert_eq!(device_volume_and_mute(before.id), original);

        if !switched.can_volume {
            assert!(set_output_volume(target, 0.3).is_err());
            assert!(adjust_output_volume(target, -0.1).is_err());
        }
        if !switched.can_mute {
            assert!(toggle_output_mute(target).is_err());
        }
        assert_eq!(observed("after unsupported requests").volume, switched.volume);
        assert_eq!(output_state().muted, switched.muted);

        assert!(set_default_output(before.id));
        let after = observed("restored");
        assert_eq!(after.id, before.id);
        assert_eq!((after.volume, after.muted), (before.volume, before.muted));
    }
}

pub fn output_device_names() -> Vec<String> {
    let mut out = Vec::new();
    for id in object_list(K_AUDIO_HARDWARE_PROPERTY_DEVICES) {
        unsafe {
            // has output streams?
            let a = addr(
                K_AUDIO_DEVICE_PROPERTY_STREAMS,
                K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            );
            let mut size = 0u32;
            if AudioObjectGetPropertyDataSize(id, &a, 0, std::ptr::null(), &mut size) == 0
                && size > 0
            {
                out.push(device_name(id));
            }
        }
    }
    if out.is_empty() {
        out.push(crate::locale::text("Default output", "Standardutgang").into());
    }
    out
}

// ---- per-app audio processes ----

pub fn audio_processes() -> Vec<AudioApp> {
    let mut out = Vec::new();
    for id in object_list(K_AUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST) {
        unsafe {
            let mut pid: i32 = 0;
            if !get(
                id,
                &addr(
                    K_AUDIO_PROCESS_PROPERTY_PID,
                    K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
                ),
                &mut pid,
            ) {
                continue;
            }
            let mut run_out: u32 = 0;
            let mut run_in: u32 = 0;
            get(
                id,
                &addr(
                    K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_OUTPUT,
                    K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
                ),
                &mut run_out,
            );
            get(
                id,
                &addr(
                    K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_INPUT,
                    K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
                ),
                &mut run_in,
            );
            let bundle = cfstr_value(
                id,
                K_AUDIO_PROCESS_PROPERTY_BUNDLE_ID,
                K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            );
            let name = crate::processes::proc_path(pid);
            let (app_name, icon) = if let Some(root) = crate::processes::bundle_root(&name) {
                (
                    std::path::Path::new(&root)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| name.clone()),
                    Some(root),
                )
            } else {
                (
                    std::path::Path::new(&name)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .or(bundle.clone())
                        .unwrap_or_else(|| format!("PID {}", pid)),
                    None,
                )
            };
            out.push(AudioApp {
                pid,
                name: app_name,
                icon_path: icon,
                running_output: run_out != 0,
                running_input: run_in != 0,
            });
        }
    }
    out.sort_by(|a, b| b.running_output.cmp(&a.running_output));
    out
}
