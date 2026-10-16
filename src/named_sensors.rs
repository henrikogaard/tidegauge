//! Temperatures that macOS itself names: the chip's die sensors, the SSD and
//! the battery, read from the HID sensor service. These are the readings the
//! CPU page leads with. The SMC list stays available as raw channels, but
//! some SMC keys are not temperatures at all, so it is not a fair headline.
//!
//! Checked on a Mac17,7 (M5 Max, macOS 27.0.1): the battery sensors here
//! matched SMC `TB0T`/`TB1T`/`TB2T` to the tenth of a degree.

#![allow(dead_code)]

use crate::ffi::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFRelease, CFStringRef, CFTypeRef};
use std::ffi::c_void;

type Client = *mut c_void;
type Service = *const c_void;
type Event = *const c_void;

const EVENT_TEMPERATURE: i64 = 15;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreate(allocator: *const c_void) -> Client;
    fn IOHIDEventSystemClientSetMatching(client: Client, matching: *const c_void);
    fn IOHIDEventSystemClientCopyServices(client: Client) -> CFArrayRef;
    fn IOHIDServiceClientCopyEvent(service: Service, kind: i64, options: i32, timestamp: i64) -> Event;
    fn IOHIDServiceClientCopyProperty(service: Service, key: CFStringRef) -> CFTypeRef;
    fn IOHIDEventGetFloatValue(event: Event, field: i32) -> f64;
}

/// What a named sensor measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Chip,
    Ssd,
    Battery,
}

impl Kind {
    pub fn label(self) -> &'static str {
        use crate::locale::text as l;
        match self {
            Kind::Chip => l("Chip", "Brikke"),
            Kind::Ssd => "SSD",
            Kind::Battery => l("Battery", "Batteri"),
        }
    }
}

/// The kind a sensor's product name belongs to; `None` for sensors that are
/// not a temperature of one of these parts (calibration channels, say).
pub fn classify(product: &str) -> Option<Kind> {
    let name = product.to_lowercase();
    if name.contains("tdie") {
        Some(Kind::Chip)
    } else if name.contains("nand") {
        Some(Kind::Ssd)
    } else if name.contains("battery") {
        Some(Kind::Battery)
    } else {
        None
    }
}

/// One line per kind: the hottest sensor and the average, in °C.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub kind: Kind,
    pub hottest: f64,
    pub average: f64,
    pub sensors: usize,
}

pub fn summarise(readings: &[(Kind, f64)]) -> Vec<Summary> {
    [Kind::Chip, Kind::Ssd, Kind::Battery]
        .into_iter()
        .filter_map(|kind| {
            let values: Vec<f64> = readings.iter().filter(|(k, _)| *k == kind).map(|(_, v)| *v).collect();
            let hottest = values.iter().copied().reduce(f64::max)?;
            Some(Summary { kind, hottest, average: values.iter().sum::<f64>() / values.len() as f64, sensors: values.len() })
        })
        .collect()
}

/// The service client is made once per process and kept: creating one per
/// sample is slow and macOS keeps it alive anyway.
fn client() -> Option<usize> {
    static CLIENT: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *CLIENT.get_or_init(|| unsafe {
        let client = IOHIDEventSystemClientCreate(std::ptr::null());
        if client.is_null() {
            return None;
        }
        // Apple's vendor page 0xff00, usage 5: temperature sensors.
        use objc2_foundation::{NSDictionary, NSNumber, NSString};
        let keys = [NSString::from_str("PrimaryUsagePage"), NSString::from_str("PrimaryUsage")];
        let values = [NSNumber::new_i32(0xff00), NSNumber::new_i32(5)];
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let value_refs: Vec<&NSNumber> = values.iter().map(|v| &**v).collect();
        let matching = NSDictionary::from_slices(&key_refs, &value_refs);
        IOHIDEventSystemClientSetMatching(client, objc2::rc::Retained::as_ptr(&matching) as *const c_void);
        Some(client as usize)
    })
}

/// Every named temperature now, read-only. Empty where the service offers
/// none (a virtual Mac, an Intel Mac).
pub fn read() -> Vec<(Kind, f64)> {
    let Some(client) = client() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    unsafe {
        let services = IOHIDEventSystemClientCopyServices(client as Client);
        if services.is_null() {
            return out;
        }
        let product_key = crate::metrics::cf_str("Product");
        for i in 0..CFArrayGetCount(services) {
            let service = CFArrayGetValueAtIndex(services, i);
            let product = IOHIDServiceClientCopyProperty(service, product_key);
            let kind = crate::metrics::cf_to_string(product).as_deref().and_then(classify);
            if !product.is_null() {
                CFRelease(product);
            }
            let Some(kind) = kind else { continue };
            let event = IOHIDServiceClientCopyEvent(service, EVENT_TEMPERATURE, 0, 0);
            if event.is_null() {
                continue;
            }
            let value = IOHIDEventGetFloatValue(event, (EVENT_TEMPERATURE as i32) << 16);
            CFRelease(event);
            // Same plausibility bound as the SMC reader: no fabricated values.
            if value.is_finite() && value > 0. && value < 150. {
                out.push((kind, value));
            }
        }
        CFRelease(product_key);
        CFRelease(services);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{classify, summarise, Kind};

    #[test]
    fn product_names_map_to_parts() {
        assert_eq!(classify("PMU tdie7"), Some(Kind::Chip));
        assert_eq!(classify("NAND CH0 temp"), Some(Kind::Ssd));
        assert_eq!(classify("gas gauge battery"), Some(Kind::Battery));
        assert_eq!(classify("PMU tcal"), None, "calibration is not a part's temperature");
    }

    #[test]
    fn summaries_give_hottest_and_average_per_part() {
        let s = summarise(&[(Kind::Chip, 58.), (Kind::Chip, 60.), (Kind::Battery, 36.)]);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].kind, s[0].hottest, s[0].average, s[0].sensors), (Kind::Chip, 60., 59., 2));
        assert_eq!((s[1].kind, s[1].hottest), (Kind::Battery, 36.));
    }

    #[test]
    #[ignore = "needs a physical Mac"]
    fn native_named_sensors_read() {
        let readings = super::read();
        let summary = summarise(&readings);
        eprintln!("{summary:#?}");
        assert!(summary.iter().any(|s| s.kind == Kind::Chip), "no chip sensors: {readings:?}");
    }
}
