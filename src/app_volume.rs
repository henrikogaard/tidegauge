//! Per-app volume through Core Audio process taps (macOS 14.2+).
//!
//! For an app whose volume is changed, Tidegauge creates a private tap on
//! that app's audio process with "muted when tapped": while Tidegauge reads
//! the tap, macOS silences the app's own output. A private aggregate device
//! joins the tap to the current output device, and its I/O callback plays the
//! tapped sound to that output at the chosen volume. Nothing is recorded or
//! stored; the sound only passes through.
//!
//! Back at 100 %, the tap is removed and the app plays as before. If
//! Tidegauge quits or crashes, the tap ends with it and macOS un-mutes the
//! app ("when tapped" only applies while the tap is read).
//!
//! The first tap makes macOS ask for permission to capture audio
//! (`NSAudioCaptureUsageDescription` in Info.plist).

use crate::ffi::{AudioObjectID, CFRelease, OSStatus};
use crate::locale::text as l;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, AllocAnyThread};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

#[repr(C)]
struct AudioBuffer {
    channels: u32,
    bytes: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

type IoProc = extern "C" fn(
    AudioObjectID,
    *const c_void,
    *const AudioBufferList,
    *const c_void,
    *mut AudioBufferList,
    *const c_void,
    *mut c_void,
) -> OSStatus;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioHardwareCreateProcessTap(description: *const AnyObject, tap: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyProcessTap(tap: AudioObjectID) -> OSStatus;
    fn AudioHardwareCreateAggregateDevice(description: *const c_void, device: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyAggregateDevice(device: AudioObjectID) -> OSStatus;
    fn AudioDeviceCreateIOProcID(device: AudioObjectID, proc_: IoProc, data: *mut c_void, id: *mut *mut c_void) -> OSStatus;
    fn AudioDeviceDestroyIOProcID(device: AudioObjectID, id: *mut c_void) -> OSStatus;
    fn AudioDeviceStart(device: AudioObjectID, id: *mut c_void) -> OSStatus;
    fn AudioDeviceStop(device: AudioObjectID, id: *mut c_void) -> OSStatus;
}

/// What the real-time callback reads and writes: no locks, no allocation.
struct Shared {
    /// The volume as `f32` bits, 0.0–1.0.
    gain: AtomicU32,
    /// The loudest sample the app sent in the last callback, as `f32` bits.
    peak: AtomicU32,
    /// Callbacks run, so a check can see the tap is live.
    calls: AtomicU64,
}

struct Tap {
    tap: AudioObjectID,
    device: AudioObjectID,
    proc_id: *mut c_void,
    shared: Box<Shared>,
    output: AudioObjectID,
}

// The raw ids are only used under the manager's lock.
unsafe impl Send for Tap {}

impl Drop for Tap {
    fn drop(&mut self) {
        unsafe {
            AudioDeviceStop(self.device, self.proc_id);
            AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            AudioHardwareDestroyAggregateDevice(self.device);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

/// Live taps by audio process object.
static TAPS: Mutex<Option<HashMap<AudioObjectID, Tap>>> = Mutex::new(None);

/// Copy the tap's stereo mix into the output at `gain`. The tap is the last
/// input stream (after any inputs of the output device itself); each output
/// channel takes the tap channel with the same index, wrapping round.
pub fn render(input: &[&[f32]], input_channels: usize, output: &mut [&mut [f32]], output_channels: &[usize], gain: f32) -> f32 {
    let source: &[f32] = input.last().copied().unwrap_or(&[]);
    let mut peak = 0f32;
    for sample in source {
        peak = peak.max(sample.abs());
    }
    let frames_in = if input_channels == 0 { 0 } else { source.len() / input_channels };
    let mut channel_base = 0;
    for (buffer, &channels) in output.iter_mut().zip(output_channels) {
        if channels == 0 {
            continue;
        }
        let frames = buffer.len() / channels;
        for frame in 0..frames {
            for c in 0..channels {
                let value = if frame < frames_in && input_channels > 0 {
                    source[frame * input_channels + (channel_base + c) % input_channels] * gain
                } else {
                    0.
                };
                buffer[frame * channels + c] = value;
            }
        }
        channel_base += channels;
    }
    peak
}

extern "C" fn io_proc(
    _device: AudioObjectID,
    _now: *const c_void,
    input: *const AudioBufferList,
    _input_time: *const c_void,
    output: *mut AudioBufferList,
    _output_time: *const c_void,
    data: *mut c_void,
) -> OSStatus {
    unsafe {
        let shared = &*(data as *const Shared);
        shared.calls.fetch_add(1, Ordering::Relaxed);
        let gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
        if output.is_null() {
            return 0;
        }
        // The tap is a stereo mixdown: the last input buffer with sound.
        let (mut source, mut source_channels): (&[f32], usize) = (&[], 0);
        if !input.is_null() {
            let count = (*input).count as usize;
            let buffers = std::slice::from_raw_parts((*input).buffers.as_ptr(), count);
            if let Some(b) = buffers.iter().rev().find(|b| !b.data.is_null() && b.bytes > 0) {
                source = std::slice::from_raw_parts(b.data as *const f32, b.bytes as usize / 4);
                source_channels = b.channels as usize;
            }
        }
        let count = (*output).count as usize;
        let buffers = std::slice::from_raw_parts_mut((*output).buffers.as_mut_ptr(), count);
        // At most 16 output buffers, kept on the stack.
        let mut outs: [&mut [f32]; 16] = Default::default();
        let mut channels = [0usize; 16];
        let n = count.min(16);
        for (i, b) in buffers.iter_mut().take(n).enumerate() {
            if !b.data.is_null() {
                outs[i] = std::slice::from_raw_parts_mut(b.data as *mut f32, b.bytes as usize / 4);
                channels[i] = b.channels as usize;
            }
        }
        let peak = render(&[source], source_channels, &mut outs[..n], &channels[..n], gain);
        shared.peak.store(peak.to_bits(), Ordering::Relaxed);
    }
    0
}

fn any<T: objc2::Message>(object: Retained<T>) -> Retained<AnyObject> {
    unsafe { Retained::cast_unchecked(object) }
}

fn device_uid(device: AudioObjectID) -> Option<String> {
    unsafe {
        let address = crate::ffi::AudioObjectPropertyAddress {
            mSelector: crate::ffi::K_AUDIO_DEVICE_PROPERTY_UID,
            mScope: crate::ffi::K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            mElement: crate::ffi::K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut cf: crate::ffi::CFTypeRef = std::ptr::null();
        let mut size = std::mem::size_of::<crate::ffi::CFTypeRef>() as u32;
        if crate::ffi::AudioObjectGetPropertyData(device, &address, 0, std::ptr::null(), &mut size, &mut cf as *mut _ as *mut c_void) != 0 || cf.is_null() {
            return None;
        }
        let uid = crate::metrics::cf_to_string(cf);
        CFRelease(cf);
        uid
    }
}

fn failed(step: &str, status: OSStatus) -> String {
    format!("{} ({step} {status})", l("Per-app volume could not start.", "Volum per app kunne ikke starte."))
}

/// Start passing one audio process through Tidegauge at `gain`.
fn start(process: AudioObjectID, output: AudioObjectID, gain: f32) -> Result<Tap, String> {
    let output_uid = device_uid(output).ok_or_else(|| failed("output", -1))?;
    let class = AnyClass::get(c"CATapDescription").ok_or_else(|| l("Per-app volume needs macOS 14.2 or later.", "Volum per app krever macOS 14.2 eller nyere.").to_string())?;
    unsafe {
        let processes = NSArray::from_retained_slice(&[NSNumber::new_u32(process)]);
        let description: Retained<AnyObject> = {
            let allocated: objc2::rc::Allocated<AnyObject> = msg_send![class, alloc];
            let initialised: Option<Retained<AnyObject>> = msg_send![allocated, initStereoMixdownOfProcesses: &*processes];
            initialised.ok_or_else(|| failed("description", -1))?
        };
        // Muted only while the tap is read: if Tidegauge stops, the app is
        // heard again on its own.
        let _: () = msg_send![&*description, setMuteBehavior: 2isize];
        let _: () = msg_send![&*description, setPrivate: true];
        let _: () = msg_send![&*description, setName: &*NSString::from_str("Tidegauge")];
        let uuid: Retained<AnyObject> = msg_send![&*description, UUID];
        let tap_uid: Retained<NSString> = msg_send![&*uuid, UUIDString];

        let mut tap: AudioObjectID = 0;
        let status = AudioHardwareCreateProcessTap(Retained::as_ptr(&description), &mut tap);
        if status != 0 {
            return Err(failed("tap", status));
        }

        let key = |k: &str| NSString::from_str(k);
        let sub_device = NSDictionary::from_retained_objects(&[&*key("uid")], &[any(NSString::from_str(&output_uid))]);
        let sub_tap = NSDictionary::from_retained_objects(
            &[&*key("uid"), &*key("drift")],
            &[any(tap_uid.clone()), any(NSNumber::new_bool(true))],
        );
        let aggregate_uid = String::from("no.ogard.tidegauge.app-volume.") + &tap_uid.to_string();
        let description = NSDictionary::from_retained_objects(
            &[&*key("name"), &*key("uid"), &*key("master"), &*key("private"), &*key("stacked"), &*key("tapautostart"), &*key("subdevices"), &*key("taps")],
            &[
                any(NSString::from_str("Tidegauge")),
                any(NSString::from_str(&aggregate_uid)),
                any(NSString::from_str(&output_uid)),
                any(NSNumber::new_bool(true)),
                any(NSNumber::new_bool(false)),
                any(NSNumber::new_bool(true)),
                any(NSArray::from_retained_slice(&[sub_device])),
                any(NSArray::from_retained_slice(&[sub_tap])),
            ],
        );
        let mut device: AudioObjectID = 0;
        let status = AudioHardwareCreateAggregateDevice(Retained::as_ptr(&description) as *const c_void, &mut device);
        if status != 0 {
            AudioHardwareDestroyProcessTap(tap);
            return Err(failed("device", status));
        }
        let shared = Box::new(Shared {
            gain: AtomicU32::new(gain.to_bits()),
            peak: AtomicU32::new(0),
            calls: AtomicU64::new(0),
        });
        let mut proc_id: *mut c_void = std::ptr::null_mut();
        let status = AudioDeviceCreateIOProcID(device, io_proc, &*shared as *const Shared as *mut c_void, &mut proc_id);
        if status != 0 {
            AudioHardwareDestroyAggregateDevice(device);
            AudioHardwareDestroyProcessTap(tap);
            return Err(failed("callback", status));
        }
        let started = Tap { tap, device, proc_id, shared, output };
        let status = AudioDeviceStart(device, proc_id);
        if status != 0 {
            drop(started);
            return Err(failed("start", status));
        }
        Ok(started)
    }
}

/// One app's volume as Tidegauge holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub volume: f32,
    /// The loudest recent sample of the app's own sound, 0.0–1.0.
    pub peak: f32,
    pub live: bool,
}

/// Set an audio process's volume, 0.0–1.0. At 1.0 the tap is removed and
/// the app plays on its own again.
pub fn set_volume(process: AudioObjectID, volume: f32) -> Result<(), String> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(l("Volume must be from 0 to 100%.", "Volum må være fra 0 til 100 %.").into());
    }
    let mut taps = TAPS.lock().unwrap();
    let taps = taps.get_or_insert_with(HashMap::new);
    if volume >= 0.999 {
        taps.remove(&process);
        return Ok(());
    }
    let output = crate::audio::default_output_device();
    if let Some(tap) = taps.get(&process).filter(|t| t.output == output) {
        tap.shared.gain.store(volume.to_bits(), Ordering::Relaxed);
        return Ok(());
    }
    // A new tap, or the output changed: start again on the current output.
    taps.remove(&process);
    taps.insert(process, start(process, output, volume)?);
    Ok(())
}

/// The volume Tidegauge holds for each tapped process.
pub fn levels() -> HashMap<AudioObjectID, Level> {
    let taps = TAPS.lock().unwrap();
    taps.iter()
        .flatten()
        .map(|(process, tap)| {
            (
                *process,
                Level {
                    volume: f32::from_bits(tap.shared.gain.load(Ordering::Relaxed)),
                    peak: f32::from_bits(tap.shared.peak.load(Ordering::Relaxed)).min(1.),
                    live: tap.shared.calls.load(Ordering::Relaxed) > 0,
                },
            )
        })
        .collect()
}

/// Keep taps in step with the system: drop those whose process has gone,
/// and move the rest to a new default output.
pub fn follow(processes: &[AudioObjectID]) {
    let output = crate::audio::default_output_device();
    let mut taps = TAPS.lock().unwrap();
    let Some(taps) = taps.as_mut() else { return };
    taps.retain(|process, _| processes.contains(process));
    let moved: Vec<(AudioObjectID, f32)> = taps
        .iter()
        .filter(|(_, t)| t.output != output)
        .map(|(p, t)| (*p, f32::from_bits(t.shared.gain.load(Ordering::Relaxed))))
        .collect();
    for (process, volume) in moved {
        taps.remove(&process);
        if let Ok(tap) = start(process, output, volume) {
            taps.insert(process, tap);
        }
    }
}

/// End every tap, so each app plays on its own again.
pub fn stop_all() {
    if let Some(taps) = TAPS.lock().unwrap().as_mut() {
        taps.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn the_tap_is_played_at_the_chosen_volume() {
        let tap = [0.5f32, -1.0, 0.25, 0.5];
        let mut left_right = [9f32; 4];
        let peak = render(&[&tap], 2, &mut [&mut left_right], &[2], 0.5);
        assert_eq!(left_right, [0.25, -0.5, 0.125, 0.25]);
        assert_eq!(peak, 1.0, "the meter shows the app's own level");
    }

    #[test]
    fn split_and_wider_outputs_take_matching_channels() {
        let tap = [0.1f32, 0.2, 0.3, 0.4];
        let (mut left, mut right) = ([0f32; 2], [0f32; 2]);
        render(&[&tap], 2, &mut [&mut left, &mut right], &[1, 1], 1.0);
        assert_eq!((left, right), ([0.1, 0.3], [0.2, 0.4]));
        let mut four = [0f32; 8];
        render(&[&tap], 2, &mut [&mut four], &[4], 1.0);
        assert_eq!(four, [0.1, 0.2, 0.1, 0.2, 0.3, 0.4, 0.3, 0.4]);
    }

    #[test]
    fn silence_when_the_tap_has_nothing() {
        let mut out = [7f32; 4];
        assert_eq!(render(&[&[]], 0, &mut [&mut out], &[2], 1.0), 0.0);
        assert_eq!(out, [0.; 4]);
    }

    #[test]
    fn volumes_outside_the_range_are_refused() {
        assert!(super::set_volume(1, 1.5).is_err());
        assert!(super::set_volume(1, f32::NAN).is_err());
    }
}
