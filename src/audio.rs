//! CoreAudio: output device list/volume + per-app audio processes.

#![allow(dead_code)]

use crate::engine::AudioApp;
use crate::ffi::*;
use std::ffi::c_void;

fn addr(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    }
}

unsafe fn get<T: Copy>(
    obj: AudioObjectID,
    a: &AudioObjectPropertyAddress,
    out: &mut T,
) -> bool {
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
                K_AUDIO_HARDWARE_PROPERTY_DEFAULT_SYSTEM_OUTPUT_DEVICE,
                K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            ),
            &mut id,
        );
        id
    }
}

pub fn device_name(id: AudioObjectID) -> String {
    cfstr_value(id, K_AUDIO_DEVICE_PROPERTY_DEVICE_NAME_CFSTRING, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL)
        .unwrap_or_else(|| "Output".into())
}

pub fn default_output_name() -> String {
    let d = default_output_device();
    if d == 0 { "Default output".into() } else { device_name(d) }
}

fn device_volume_ch(id: AudioObjectID, ch: u32) -> Option<f32> {
    unsafe {
        let a = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR,
            mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            mElement: ch,
        };
        let mut v: f32 = 0.0;
        if get(id, &a, &mut v) { Some(v) } else { None }
    }
}

pub fn default_output_volume() -> Option<f32> {
    let d = default_output_device();
    if d == 0 {
        return None;
    }
    let l = device_volume_ch(d, 1).or_else(|| device_volume_ch(d, 0));
    let r = device_volume_ch(d, 2).or_else(|| device_volume_ch(d, 0));
    match (l, r) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        (x, _) => x,
    }
}

pub fn set_default_output_volume(v: f32) {
    let d = default_output_device();
    if d == 0 {
        return;
    }
    for ch in [1u32, 2, 0] {
        let a = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR,
            mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            mElement: ch,
        };
        unsafe {
            if AudioObjectHasProperty(d, &a) != 0 {
                set(d, &a, &v);
            }
        }
    }
}

pub fn default_output_muted() -> Option<bool> {
    let d = default_output_device();
    if d == 0 {
        return None;
    }
    unsafe {
        let a = AudioObjectPropertyAddress {
            mSelector: K_AUDIO_DEVICE_PROPERTY_MUTE,
            mScope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            mElement: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut m: u32 = 0;
        if get(d, &a, &mut m) { Some(m != 0) } else { None }
    }
}

pub fn output_device_names() -> Vec<String> {
    let mut out = Vec::new();
    for id in object_list(K_AUDIO_HARDWARE_PROPERTY_DEVICES) {
        unsafe {
            // has output streams?
            let a = addr(K_AUDIO_DEVICE_PROPERTY_STREAMS, K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT);
            let mut size = 0u32;
            if AudioObjectGetPropertyDataSize(id, &a, 0, std::ptr::null(), &mut size) == 0 && size > 0
            {
                out.push(device_name(id));
            }
        }
    }
    if out.is_empty() {
        out.push("Default output".into());
    }
    out
}

// ---- per-app audio processes ----

pub fn audio_processes() -> Vec<AudioApp> {
    let mut out = Vec::new();
    for id in object_list(K_AUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST) {
        unsafe {
            let mut pid: i32 = 0;
            if !get(id, &addr(K_AUDIO_PROCESS_PROPERTY_PID, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &mut pid) {
                continue;
            }
            let mut run_out: u32 = 0;
            let mut run_in: u32 = 0;
            get(id, &addr(K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_OUTPUT, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &mut run_out);
            get(id, &addr(K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_INPUT, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &mut run_in);
            let mut audible: u32 = 0;
            get(id, &addr(K_AUDIO_HARDWARE_PROPERTY_PROCESS_IS_AUDIBLE, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &mut audible);
            let mut muted: u32 = 0;
            get(id, &addr(K_AUDIO_DEVICE_PROPERTY_PROCESS_MUTE, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &mut muted);
            let bundle = cfstr_value(id, K_AUDIO_PROCESS_PROPERTY_BUNDLE_ID, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL);
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
                volume: None,
                muted: muted != 0,
                level: if audible != 0 { 0.65 + (pid % 30) as f32 / 100.0 } else { 0.0 },
            });
        }
    }
    out.sort_by(|a, b| b.running_output.cmp(&a.running_output));
    out
}

/// Set an app's mute state on its audio process object.
pub fn set_app_mute(pid: i32, mute: bool) {
    for id in object_list(K_AUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST) {
        unsafe {
            let mut p: i32 = 0;
            if !get(id, &addr(K_AUDIO_PROCESS_PROPERTY_PID, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &mut p)
                || p != pid
            {
                continue;
            }
            let m: u32 = if mute { 1 } else { 0 };
            set(id, &addr(K_AUDIO_DEVICE_PROPERTY_PROCESS_MUTE, K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL), &m);
        }
    }
}
