//! GPU power from macOS's IOReport energy counters ("Energy Model" group,
//! "GPU Energy" channel), read without special permissions. The counter is
//! cumulative energy; power is its change over the time between samples.
//!
//! The Neural Engine is not read: on a Mac17,7 (M5 Max, macOS 27.0.1) its
//! channels stay at zero under load, and `powermetrics` reports no Neural
//! Engine power either, even as root.

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
        (!subscription.is_null() && !subscribed.is_null()).then_some(Reader { subscription, channels: subscribed, last: None })
    }
}

/// GPU power since the previous call on this thread, in watts. The first
/// call only takes a baseline; a Mac without the counter gives none.
pub fn sample() -> Option<f64> {
    READER.with(|slot| {
        let mut slot = slot.borrow_mut();
        read(slot.get_or_insert_with(open).as_mut()?)
    })
}

fn read(reader: &mut Reader) -> Option<f64> {
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
            let mut power = None;
            if !items.is_null() {
                for i in 0..CFArrayGetCount(items) {
                    let channel = CFArrayGetValueAtIndex(items, i);
                    if crate::metrics::cf_to_string(IOReportChannelGetChannelName(channel)).as_deref() != Some("GPU Energy") {
                        continue;
                    }
                    let unit = crate::metrics::cf_to_string(IOReportChannelGetUnitLabel(channel)).as_deref().and_then(joules_per_unit);
                    power = unit.and_then(|u| watts(IOReportSimpleGetIntegerValue(channel, 0), u, seconds));
                    break;
                }
            }
            CFRelease(delta);
            power
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{joules_per_unit, watts};

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
        assert_eq!(super::sample(), None, "the first call is a baseline");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let w = super::sample();
        eprintln!("GPU power {w:?} W");
        assert!(w.is_some_and(|w| w >= 0.));
    }
}
