//! Bluetooth device inventory + battery levels, via `system_profiler` JSON.

#![allow(dead_code)]

use crate::engine::{BtDevice, BtInfo};
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Deserialize, Default)]
struct SPBluetooth {
    #[serde(default)]
    bluetooth_state: Option<String>,
    #[serde(default)]
    device_connected: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    device_not_connected: Option<Vec<serde_json::Value>>,
}

static CACHE: OnceLock<Mutex<(Instant, BtInfo)>> = OnceLock::new();

fn classify(name: &str) -> &'static str {
    use crate::locale::text as t;
    let l = name.to_lowercase();
    if l.contains("airpods") || l.contains("beats") { t("Headphones", "Hodetelefoner") }
    else if l.contains("mouse") { t("Mouse", "Mus") }
    else if l.contains("keyboard") { t("Keyboard", "Tastatur") }
    else if l.contains("trackpad") { t("Trackpad", "Styreflate") }
    else if l.contains("pencil") { t("Pencil", "Penn") }
    else if l.contains("watch") { t("Watch", "Klokke") }
    else if l.contains("iphone") || l.contains("ipad") { t("Phone", "Telefon") }
    else if l.contains("controller") || l.contains("dualsense") { t("Controller", "Kontroller") }
    else if l.contains("speaker") || l.contains("homepod") { t("Speaker", "Høyttaler") }
    else { t("Device", "Enhet") }
}

/// A line icon for a device, from the same name hints as its kind.
pub fn device_icon(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.contains("airpods") || l.contains("beats") { "icons/headphones.svg" }
    else if l.contains("mouse") { "icons/mouse.svg" }
    else if l.contains("keyboard") { "icons/keyboard.svg" }
    else if l.contains("trackpad") { "icons/mouse-pointer-2.svg" }
    else if l.contains("pencil") { "icons/pencil.svg" }
    else if l.contains("watch") { "icons/watch.svg" }
    else if l.contains("ipad") { "icons/tablet.svg" }
    else if l.contains("iphone") { "icons/smartphone.svg" }
    else if l.contains("controller") || l.contains("dualsense") { "icons/gamepad-2.svg" }
    else if l.contains("speaker") || l.contains("homepod") { "icons/speaker.svg" }
    else { "icons/bluetooth.svg" }
}

fn battery_of(v: &serde_json::Value) -> Option<f64> {
    v.get("device_batteryLevel")
        .or_else(|| v.get("device_battery_level"))
        .and_then(|x| {
            x.as_str()
                .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                .or_else(|| x.as_f64().map(|n| if n <= 1.0 { n * 100.0 } else { n }))
        })
}

fn inner_battery(v: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(x) = v.get(*k) {
            let b = x
                .as_str()
                .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                .or_else(|| x.as_f64().map(|n| if n <= 1.0 { n * 100.0 } else { n }));
            if b.is_some() {
                return b;
            }
        }
    }
    None
}

pub fn bt_info() -> BtInfo {
    let cache = CACHE.get_or_init(|| Mutex::new((Instant::now() - Duration::from_secs(60), BtInfo::default())));
    {
        let g = cache.lock().unwrap();
        if g.0.elapsed() < Duration::from_secs(20) {
            return g.1.clone();
        }
    }
    let mut info = BtInfo::default();
    if let Ok(out) = std::process::Command::new("system_profiler")
        .args(["-json", "SPBluetoothDataType"])
        .output()
    {
        if let Ok(root) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            if let Some(arr) = root.get("SPBluetoothDataType").and_then(|x| x.as_array()) {
                if let Some(first) = arr.first() {
                    let state = first
                        .get("bluetooth_state")
                        .or_else(|| first.get("local_device_title").and_then(|t| t.get("general_state")))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    info.enabled = !state.to_lowercase().contains("off");
                    let mut devs = Vec::new();
                    if let Some(list) = first.get("device_connected").and_then(|x| x.as_array()) {
                        for entry in list {
                            if let Some(obj) = entry.as_object() {
                                for (name, v) in obj {
                                    let b = battery_of(v);
                                    devs.push(BtDevice {
                                        name: name.clone(),
                                        kind: classify(name).into(),
                                        battery: b.or_else(|| inner_battery(v, &["device_batteryLevelMain", "device_batteryLevelLeft"])),
                                        second_battery: inner_battery(v, &["device_batteryLevelRight"]),
                                        case_battery: inner_battery(v, &["device_caseBatteryLevel", "device_batteryLevelCase"]),
                                        connected: true,
                                        charging: v.get("device_ischarging").and_then(|x| x.as_bool()).unwrap_or(false),
                                    });
                                }
                            }
                        }
                    }
                    devs.sort_by(|a, b| a.battery.unwrap_or(101.0).partial_cmp(&b.battery.unwrap_or(101.0)).unwrap());
                    info.devices = devs;
                }
            }
        }
    }
    let mut g = cache.lock().unwrap();
    *g = (Instant::now(), info.clone());
    info
}
