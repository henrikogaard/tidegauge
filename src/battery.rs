use crate::ffi::*;
use crate::locale::text as l;
use crate::metrics::{
    cf_to_bool, cf_to_i64, cf_to_string, dict_get, entry_prop, matching_services,
};

#[derive(Clone, Default, Debug)]
pub struct BatteryStats {
    pub available: bool,
    pub present: bool,
    pub on_battery: bool,
    pub percent: Option<f64>,
    pub minutes_remaining: Option<i64>,
    pub minutes_to_full: Option<i64>,
    pub is_charging: bool,
    pub is_charged: bool,
    pub cycles: Option<i64>,
    pub health: Option<f64>,
    pub condition: Option<String>,
    pub temperature_c: Option<f64>,
    /// Signed terminal power: positive into the battery, negative out of it.
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub current_ma: Option<i64>,
    pub full_capacity_mah: Option<i64>,
    pub design_capacity_mah: Option<i64>,
    pub adapter_rating_w: Option<i64>,
    pub ac_power: bool,
}

impl BatteryStats {
    pub fn status(&self) -> &'static str {
        if !self.available {
            l(
                "Battery information unavailable",
                "Batteriinformasjon utilgjengelig",
            )
        } else if !self.present {
            l("No battery", "Ingen batteri")
        } else if self.is_charging {
            l("Charging", "Lader")
        } else if self.on_battery {
            l("On battery", "På batteri")
        } else if self.ac_power && self.is_charged {
            l("Fully charged", "Fulladet")
        } else if self.ac_power {
            l("Connected, not charging", "Tilkoblet, lader ikke")
        } else {
            l("Charge status unavailable", "Ladestatus utilgjengelig")
        }
    }

    pub fn condition_label(&self) -> &'static str {
        match self.condition.as_deref() {
            Some("Good") => l("Good", "God"),
            Some("Fair") => l("Reduced capacity", "Redusert kapasitet"),
            Some("Poor" | "Check Battery") => l("Service recommended", "Service anbefales"),
            Some("Permanent Battery Failure") => l("Replacement required", "Må byttes"),
            _ => l("Unavailable", "Utilgjengelig"),
        }
    }
}

struct OwnedCf(CFTypeRef);
impl Drop for OwnedCf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

struct BatteryEntry(io_registry_entry_t);
impl Drop for BatteryEntry {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0) };
    }
}
impl BatteryEntry {
    fn number(&self, key: &str) -> Option<i64> {
        cf_to_i64(OwnedCf(entry_prop(self.0, key)).0)
    }
    fn boolean(&self, key: &str) -> Option<bool> {
        cf_to_bool(OwnedCf(entry_prop(self.0, key)).0)
    }
}

fn percentage(current: i64, maximum: i64) -> Option<f64> {
    (maximum > 0 && current >= 0 && current <= maximum)
        .then(|| current as f64 / maximum as f64 * 100.0)
}

fn capacity_mah(value: Option<i64>) -> Option<i64> {
    // Normalized 0–100 charge values are not physical pack capacities.
    value.filter(|value| (101..=200_000).contains(value))
}

fn capacity_health(full: Option<i64>, design: Option<i64>) -> Option<f64> {
    let full = capacity_mah(full)?;
    let design = capacity_mah(design)?;
    let ratio = full as f64 / design as f64 * 100.0;
    (ratio <= 150.0).then_some(ratio)
}

fn signed_current(value: i64) -> Option<i64> {
    let value = if (i32::MAX as i64 + 1..=u32::MAX as i64).contains(&value) {
        value as u32 as i32 as i64
    } else {
        value
    };
    (-100_000..=100_000).contains(&value).then_some(value)
}

fn terminal_power(current_ma: Option<i64>, voltage_mv: Option<i64>) -> Option<f64> {
    let amps = signed_current(current_ma?)?;
    let volts = voltage_mv.filter(|v| (1..=60_000).contains(v))?;
    let watts = amps as f64 * volts as f64 / 1e6;
    (watts.abs() <= 500.0).then_some(watts)
}

pub fn battery_stats() -> BatteryStats {
    let mut b = BatteryStats::default();
    unsafe {
        let info = OwnedCf(IOPSCopyPowerSourcesInfo());
        if !info.0.is_null() {
            let list = OwnedCf(IOPSCopyPowerSourcesList(info.0));
            if !list.0.is_null() {
                b.available = true;
                for i in 0..CFArrayGetCount(list.0) {
                    let ps = CFArrayGetValueAtIndex(list.0, i);
                    let d = IOPSGetPowerSourceDescription(info.0, ps);
                    if d.is_null()
                        || cf_to_string(dict_get(d, "Type")).as_deref() != Some("InternalBattery")
                    {
                        continue;
                    }
                    if cf_to_bool(dict_get(d, "Is Present")) == Some(false) {
                        continue;
                    }
                    b.present = true;
                    b.percent = cf_to_i64(dict_get(d, "Current Capacity"))
                        .zip(cf_to_i64(dict_get(d, "Max Capacity")))
                        .and_then(|(c, m)| percentage(c, m));
                    b.is_charging = cf_to_bool(dict_get(d, "Is Charging")).unwrap_or(false);
                    b.is_charged = cf_to_bool(dict_get(d, "Is Charged")).unwrap_or(false);
                    let state = cf_to_string(dict_get(d, "Power Source State"));
                    b.on_battery = state.as_deref() == Some("Battery Power");
                    b.ac_power = matches!(state.as_deref(), Some("AC Power" | "UPS Power"));
                    b.minutes_remaining =
                        cf_to_i64(dict_get(d, "Time to Empty")).filter(|t| *t > 0 && b.on_battery);
                    b.minutes_to_full = cf_to_i64(dict_get(d, "Time to Full Charge"))
                        .filter(|t| *t > 0 && b.is_charging);
                    b.condition = cf_to_string(dict_get(d, "BatteryHealthCondition"))
                        .or_else(|| cf_to_string(dict_get(d, "BatteryHealth")));
                    break;
                }
            }
        }
        if let Some(entry) = matching_services("AppleSmartBattery").next() {
            let e = BatteryEntry(entry);
            b.available = true;
            if e.boolean("BatteryInstalled") == Some(false) {
                return BatteryStats {
                    available: true,
                    ..BatteryStats::default()
                };
            }
            b.present = true;
            let full = capacity_mah(e.number("AppleRawMaxCapacity"))
                .or_else(|| capacity_mah(e.number("NominalChargeCapacity")))
                .or_else(|| {
                    if cfg!(target_arch = "x86_64") {
                        capacity_mah(e.number("MaxCapacity"))
                    } else {
                        None
                    }
                });
            b.full_capacity_mah = full;
            b.design_capacity_mah = capacity_mah(e.number("DesignCapacity"));
            b.health = capacity_health(full, b.design_capacity_mah);
            b.cycles = e.number("CycleCount").filter(|c| *c >= 0);
            if b.percent.is_none() {
                b.percent = e
                    .number("CurrentCapacity")
                    .zip(e.number("MaxCapacity"))
                    .and_then(|(c, m)| percentage(c, m));
            }
            b.temperature_c = e
                .number("Temperature")
                .map(|t| t as f64 / 100.0)
                .filter(|t| (-50.0..=150.0).contains(t));
            b.current_ma = e
                .number("InstantAmperage")
                .or_else(|| e.number("Amperage"))
                .and_then(signed_current);
            let volts = e.number("Voltage").filter(|v| (1..=60_000).contains(v));
            b.voltage_v = volts.map(|v| v as f64 / 1000.0);
            b.power_w = terminal_power(b.current_ma, volts);
            b.is_charging = e.boolean("IsCharging").unwrap_or(b.is_charging);
            b.is_charged = e.boolean("FullyCharged").unwrap_or(b.is_charged);
            if let Some(external) = e.boolean("ExternalConnected") {
                b.ac_power = external;
                b.on_battery = !external;
            }
        }
        if b.ac_power {
            extern "C" {
                fn IOPSCopyExternalPowerAdapterDetails() -> CFDictionaryRef;
            }
            let details = OwnedCf(IOPSCopyExternalPowerAdapterDetails());
            b.adapter_rating_w =
                cf_to_i64(dict_get(details.0, "Watts")).filter(|w| (1..=1000).contains(w));
        }
    }
    b.minutes_remaining = b.minutes_remaining.filter(|_| b.on_battery);
    b.minutes_to_full = b.minutes_to_full.filter(|_| b.is_charging);
    b
}

/// UI-only development fixture; never enters the sampler or history database.
pub fn preview() -> Option<BatteryStats> {
    #[cfg(debug_assertions)]
    {
        let mode = std::env::var("TIDEGAUGE_BATTERY_PREVIEW").ok()?;
        if !matches!(mode.as_str(), "charging" | "discharging") {
            return None;
        }
        let charging = mode == "charging";
        return Some(BatteryStats {
            available: true,
            present: true,
            percent: Some(72.0),
            on_battery: !charging,
            ac_power: charging,
            is_charging: charging,
            minutes_remaining: (!charging).then_some(240),
            minutes_to_full: charging.then_some(55),
            cycles: Some(167),
            health: Some(90.0),
            condition: Some("Good".into()),
            temperature_c: Some(32.5),
            full_capacity_mah: Some(5400),
            design_capacity_mah: Some(6000),
            power_w: Some(if charging { 12.0 } else { -12.0 }),
            voltage_v: Some(12.0),
            current_ma: Some(if charging { 1000 } else { -1000 }),
            adapter_rating_w: charging.then_some(96),
            ..BatteryStats::default()
        });
    }
    #[cfg(not(debug_assertions))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_health_uses_physical_capacity_not_charge_percent() {
        assert_eq!(capacity_health(Some(5400), Some(6000)), Some(90.0));
        assert_eq!(capacity_health(Some(100), Some(6000)), None);
        assert_eq!(capacity_health(Some(5400), Some(0)), None);
        assert_eq!(capacity_health(None, Some(6000)), None);
        assert_eq!(capacity_health(Some(6300), Some(6000)), Some(105.0));
    }

    #[test]
    fn battery_terminal_power_preserves_direction_and_real_zero() {
        assert_eq!(terminal_power(Some(-1000), Some(12000)), Some(-12.0));
        assert_eq!(terminal_power(Some(1000), Some(12000)), Some(12.0));
        assert_eq!(terminal_power(Some(0), Some(12000)), Some(0.0));
        assert_eq!(
            terminal_power(Some(4_294_966_296), Some(12000)),
            Some(-12.0)
        );
        assert_eq!(terminal_power(None, Some(12000)), None);
        assert_eq!(terminal_power(Some(1000), Some(0)), None);
    }

    #[test]
    fn battery_percent_rejects_unknown_and_out_of_range() {
        assert_eq!(percentage(80, 100), Some(80.0));
        assert_eq!(percentage(0, 100), Some(0.0));
        assert_eq!(percentage(-1, 100), None);
        assert_eq!(percentage(101, 100), None);
        assert_eq!(percentage(10, 0), None);
    }

    #[test]
    #[ignore = "Reads the current Mac's power sources; requires hardware comparison"]
    fn native_battery_probe() {
        let battery = battery_stats();
        println!("{battery:#?}");
        if !battery.present {
            assert!(battery.percent.is_none());
            assert!(battery.health.is_none());
            assert!(battery.power_w.is_none());
        }
    }
}
