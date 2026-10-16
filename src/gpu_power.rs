//! GPU power from macOS's IOReport energy counters ("Energy Model" group,
//! "GPU Energy" channel), read without special permissions. The counter is
//! cumulative energy; power is its change over the time between samples.
//!
//! The Neural Engine's channels ("ANE0", "ANE1"…) are read the same way, but
//! a Neural Engine power is only given once one of them has counted: on a
//! Mac17,7 (M5 Max, macOS 27.0.1) they stay at zero even under load, and a
//! counter that never moves is no reading, not an idle Neural Engine.

use std::cell::RefCell;
use std::ffi::c_void;
use std::time::Instant;

use crate::ffi::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFRelease, CFStringRef};

type Subscription = *mut c_void;
type Dict = *const c_void;
type MutableDict = *mut c_void;

#[link(name = "IOReport")]
extern "C" {
    fn IOReportCopyChannelsInGroup(group: CFStringRef, subgroup: CFStringRef, a: u64, b: u64, c: u64) -> Dict;
    fn IOReportCreateSubscription(a: *const c_void, channels: MutableDict, out: *mut MutableDict, b: u64, c: *const c_void) -> Subscription;
    fn IOReportCreateSamples(subscription: Subscription, channels: MutableDict, a: *const c_void) -> Dict;
    fn IOReportCreateSamplesDelta(before: Dict, after: Dict, a: *const c_void) -> Dict;
    fn IOReportChannelGetChannelName(channel: Dict) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(channel: Dict) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(channel: Dict, index: i32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryCreateMutableCopy(allocator: *const c_void, capacity: isize, dict: Dict) -> MutableDict;
    fn CFDictionaryGetValue(dict: Dict, key: *const c_void) -> *const c_void;
}

/// Joules per unit for the energy labels IOReport uses.
pub fn joules_per_unit(label: &str) -> Option<f64> {
    match label.trim() {
        "nJ" => Some(1e-9),
        "uJ" | "µJ" => Some(1e-6),
        "mJ" => Some(1e-3),
        "J" => Some(1.),
        _ => None,
    }
}

/// Watts from an energy change over `seconds`; nothing for a counter that
/// went backwards, an empty interval or an implausible result.
pub fn watts(energy: i64, unit: f64, seconds: f64) -> Option<f64> {
    if energy < 0 || seconds <= 0. {
        return None;
    }
    let w = energy as f64 * unit / seconds;
    (w.is_finite() && w < 1000.).then_some(w)
}

struct Reader {
    subscription: Subscription,
    channels: MutableDict,
    last: Option<(Dict, Instant)>,
    /// Whether a Neural Engine channel has ever counted on this Mac.
    ane_counts: bool,
}

/// Power since the previous sample, in watts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Power {
    pub gpu: Option<f64>,
    /// `None` until the Neural Engine counter has been seen to move.
    pub neural_engine: Option<f64>,
}

/// Whether a channel name is one of the Neural Engine's energy channels.
pub fn is_neural_engine(name: &str) -> bool {
    name.strip_prefix("ANE").is_some_and(|rest| rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) || rest == " Energy")
}

thread_local! {
    static READER: RefCell<Option<Option<Reader>>> = const { RefCell::new(None) };
}

fn open() -> Option<Reader> {
    unsafe {
        let group = crate::metrics::cf_str("Energy Model");
        let channels = IOReportCopyChannelsInGroup(group, std::ptr::null(), 0, 0, 0);
        CFRelease(group);
        if channels.is_null() {
            return None;
        }
        let mutable = CFDictionaryCreateMutableCopy(std::ptr::null(), 0, channels);
        CFRelease(channels);
        let mut subscribed: MutableDict = std::ptr::null_mut();
        let subscription = IOReportCreateSubscription(std::ptr::null(), mutable, &mut subscribed, 0, std::ptr::null());
        CFRelease(mutable);
        (!subscription.is_null() && !subscribed.is_null()).then_some(Reader { subscription, channels: subscribed, last: None, ane_counts: false })
    }
}

/// GPU and Neural Engine power since the previous call on this thread. The
/// first call only takes a baseline; a Mac without the counters gives none.
pub fn sample() -> Power {
    READER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(reader) = slot.get_or_insert_with(open).as_mut() else {
            return Power::default();
        };
        read(reader).unwrap_or_default()
    })
}

fn read(reader: &mut Reader) -> Option<Power> {
    {
        unsafe {
            let now = IOReportCreateSamples(reader.subscription, reader.channels, std::ptr::null());
            if now.is_null() {
                return None;
            }
            let at = Instant::now();
            let previous = reader.last.replace((now, at));
            let (before, then) = previous?;
            let delta = IOReportCreateSamplesDelta(before, now, std::ptr::null());
            CFRelease(before);
            if delta.is_null() {
                return None;
            }
            let key = crate::metrics::cf_str("IOReportChannels");
            let items = CFDictionaryGetValue(delta, key) as CFArrayRef;
            CFRelease(key);
            let seconds = at.duration_since(then).as_secs_f64();
            let mut power = Power::default();
            let mut ane: Option<f64> = None;
            if !items.is_null() {
                for i in 0..CFArrayGetCount(items) {
                    let channel = CFArrayGetValueAtIndex(items, i);
                    let Some(name) = crate::metrics::cf_to_string(IOReportChannelGetChannelName(channel)) else { continue };
                    let gpu = name == "GPU Energy";
                    if !gpu && !is_neural_engine(&name) {
                        continue;
                    }
                    let unit = crate::metrics::cf_to_string(IOReportChannelGetUnitLabel(channel)).as_deref().and_then(joules_per_unit);
                    let w = unit.and_then(|u| watts(IOReportSimpleGetIntegerValue(channel, 0), u, seconds));
                    if gpu {
                        power.gpu = w;
                    } else if let Some(w) = w {
                        ane = Some(ane.unwrap_or(0.) + w);
                    }
                }
            }
            CFRelease(delta);
            reader.ane_counts |= ane.is_some_and(|w| w > 0.);
            power.neural_engine = ane.filter(|_| reader.ane_counts);
            Some(power)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_neural_engine, joules_per_unit, watts};

    #[test]
    fn neural_engine_channels_are_recognised() {
        assert!(is_neural_engine("ANE0"));
        assert!(is_neural_engine("ANE12"));
        assert!(is_neural_engine("ANE"));
        assert!(is_neural_engine("ANE Energy"));
        assert!(!is_neural_engine("ANE_THROTTLE_SW_TRIG"));
        assert!(!is_neural_engine("GPU Energy"));
    }

    #[test]
    fn energy_changes_become_watts() {
        assert_eq!(joules_per_unit("nJ"), Some(1e-9));
        assert_eq!(joules_per_unit("mJ"), Some(1e-3));
        assert_eq!(joules_per_unit("events"), None, "not an energy channel");
        let w = watts(4_451_315_165, 1e-9, 3.).unwrap();
        assert!((w - 1.4838).abs() < 0.001, "{w}");
        assert_eq!(watts(-5, 1e-9, 1.), None, "a counter reset is no reading");
        assert_eq!(watts(5, 1e-9, 0.), None);
    }

    #[test]
    #[ignore = "needs an Apple silicon Mac"]
    fn native_gpu_power_reads() {
        assert_eq!(super::sample(), super::Power::default(), "the first call is a baseline");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let p = super::sample();
        eprintln!("{p:?}");
        assert!(p.gpu.is_some_and(|w| w >= 0.));
    }
}
