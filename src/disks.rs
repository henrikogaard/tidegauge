//! Read-only device inventory, APFS space accounting, and device-scoped I/O.

use crate::ffi::*;
use crate::metrics::{
    cf_to_bool, cf_to_i64, cf_to_string, dict_get, entry_prop, matching_services,
};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Health {
    Passed,
    Failed,
    Unsupported,
    #[default]
    Unavailable,
}

#[derive(Clone, Debug, Default)]
pub struct Smart {
    pub health: Health,
    pub temperature_c: Option<f64>,
    pub lifetime_read: Option<u64>,
    pub lifetime_write: Option<u64>,
    pub power_on_hours: Option<u64>,
    pub power_cycles: Option<u64>,
    pub endurance_used: Option<u64>,
    pub extended: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Volume {
    pub id: String,
    pub name: String,
    pub mount: Option<String>,
    pub container: Option<String>,
    pub stores: Vec<String>,
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Device {
    pub id: String,
    pub registry_id: Option<u64>,
    pub name: String,
    pub protocol: Option<String>,
    pub internal: Option<bool>,
    pub solid_state: Option<bool>,
    pub size: Option<u64>,
    pub smart: Smart,
    pub volumes: Vec<Volume>,
}

impl Device {
    pub fn identity_matches(&self, io: &HashMap<String, IoSample>) -> bool {
        self.registry_id.is_none_or(|id| {
            io.get(&self.id)
                .is_some_and(|sample| sample.entry_id == id && sample.fresh())
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub devices: Vec<Device>,
    pub collected_at: Option<Instant>,
    pub available: bool,
    pub partial: bool,
}

impl Inventory {
    pub fn fresh(&self) -> bool {
        self.available
            && self
                .collected_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(90))
    }
}

#[derive(Clone, Debug)]
pub struct IoSample {
    pub entry_id: u64,
    pub read_total: u64,
    pub write_total: u64,
    pub read_rate: Option<f64>,
    pub write_rate: Option<f64>,
    pub read_history: Vec<f64>,
    pub write_history: Vec<f64>,
    pub at: Instant,
}

impl IoSample {
    pub fn fresh(&self) -> bool {
        self.at.elapsed() < Duration::from_secs(10)
    }
}

type SharedInventory = Arc<Mutex<Inventory>>;
type SharedIo = Arc<Mutex<HashMap<String, IoSample>>>;
static INVENTORY: OnceLock<SharedInventory> = OnceLock::new();
static IO: OnceLock<SharedIo> = OnceLock::new();

pub fn inventory() -> Inventory {
    INVENTORY
        .get()
        .map(|s| s.lock().unwrap().clone())
        .unwrap_or_default()
}
pub fn io() -> HashMap<String, IoSample> {
    IO.get()
        .map(|s| s.lock().unwrap().clone())
        .unwrap_or_default()
}

pub fn start() {
    INVENTORY.get_or_init(|| {
        let state = Arc::new(Mutex::new(Inventory::default()));
        let out = state.clone();
        std::thread::Builder::new()
            .name("tidegauge-disk-inventory".into())
            .spawn(move || loop {
                let sample = collect();
                *out.lock().unwrap() = sample;
                std::thread::sleep(Duration::from_secs(30));
            })
            .expect("disk inventory worker");
        state
    });
    IO.get_or_init(|| {
        let state = Arc::new(Mutex::new(HashMap::new()));
        let out = state.clone();
        std::thread::Builder::new()
            .name("tidegauge-disk-io".into())
            .spawn(move || {
                let mut previous = HashMap::new();
                loop {
                    let mut next = HashMap::new();
                    for (id, entry_id, read, write) in native_counters() {
                        let sample =
                            advance(previous.get(&id), entry_id, read, write, Instant::now());
                        next.insert(id, sample);
                    }
                    *out.lock().unwrap() = next.clone();
                    previous = next;
                    std::thread::sleep(Duration::from_secs(2));
                }
            })
            .expect("disk I/O worker");
        state
    });
}

fn advance(
    previous: Option<&IoSample>,
    entry_id: u64,
    read: u64,
    write: u64,
    at: Instant,
) -> IoSample {
    let previous = previous
        .filter(|p| p.entry_id == entry_id && read >= p.read_total && write >= p.write_total);
    let elapsed = previous
        .map(|p| at.saturating_duration_since(p.at).as_secs_f64())
        .filter(|dt| *dt > 0.0 && *dt < 10.0);
    let read_rate = previous
        .zip(elapsed)
        .map(|(p, dt)| (read - p.read_total) as f64 / dt);
    let write_rate = previous
        .zip(elapsed)
        .map(|(p, dt)| (write - p.write_total) as f64 / dt);
    let mut read_history = previous
        .filter(|_| elapsed.is_some())
        .map(|p| p.read_history.clone())
        .unwrap_or_default();
    let mut write_history = previous
        .filter(|_| elapsed.is_some())
        .map(|p| p.write_history.clone())
        .unwrap_or_default();
    if let (Some(r), Some(w)) = (read_rate, write_rate) {
        read_history.push(r);
        write_history.push(w);
    }
    if read_history.len() > 60 {
        read_history.remove(0);
        write_history.remove(0);
    }
    IoSample {
        entry_id,
        read_total: read,
        write_total: write,
        read_rate,
        write_rate,
        read_history,
        write_history,
        at,
    }
}

fn whole_disk(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("disk")?;
    let mut parts = rest.split('s');
    let number = parts.next()?;
    if number.is_empty()
        || !number.bytes().all(|b| b.is_ascii_digit())
        || !parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(&id[..4 + number.len()])
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}

// Drain both pipes while the parent enforces a deadline, including failed tools.
fn command(program: &str, args: &[&str], input: Option<Vec<u8>>) -> Option<(i32, Vec<u8>)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= 2 * 1024 * 1024).then_some(bytes)
    });
    let writer = input
        .zip(child.stdin.take())
        .map(|(bytes, mut stdin)| std::thread::spawn(move || stdin.write_all(&bytes)));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let bytes = reader.join().ok().flatten()?;
    Some((status?.code()?, bytes))
}

fn diskutil(args: &[&str]) -> Option<Value> {
    let (status, bytes) = command("/usr/sbin/diskutil", args, None)?;
    if status != 0 {
        return None;
    }
    let (status, json) = command(
        "/usr/bin/plutil",
        &["-convert", "json", "-o", "-", "-"],
        Some(bytes),
    )?;
    if status != 0 {
        return None;
    }
    serde_json::from_slice(&json).ok()
}

fn collect() -> Inventory {
    let started = Instant::now();
    let identities: HashMap<_, _> = native_counters()
        .into_iter()
        .map(|(id, entry, _, _)| (id, entry))
        .collect();
    let Some(list) = diskutil(&["list", "-plist"]) else {
        return Inventory {
            collected_at: Some(started),
            ..Default::default()
        };
    };
    if list
        .get("AllDisksAndPartitions")
        .and_then(Value::as_array)
        .is_none()
    {
        return Inventory {
            collected_at: Some(started),
            ..Default::default()
        };
    }
    let apfs = diskutil(&["apfs", "list", "-plist"]);
    let mut infos = HashMap::new();
    let mut partial = apfs.is_none();
    let roots: Vec<_> = array(&list, "AllDisksAndPartitions")
        .iter()
        .filter(|v| v["Content"] != "Apple_APFS_Container")
        .collect();
    partial |= roots.len() > 32;
    for root in roots.into_iter().take(32) {
        let Some(id) = root["DeviceIdentifier"]
            .as_str()
            .filter(|id| whole_disk(id) == Some(*id))
        else {
            partial = true;
            continue;
        };
        if started.elapsed() > Duration::from_secs(12) {
            partial = true;
            break;
        }
        if let Some(info) = diskutil(&["info", "-plist", id]) {
            infos.insert(id.into(), info);
        } else {
            partial = true;
        }
        for volume in array(root, "Partitions")
            .iter()
            .filter(|v| v.get("MountPoint").is_some())
        {
            let Some(id) = volume["DeviceIdentifier"]
                .as_str()
                .filter(|id| whole_disk(id).is_some())
            else {
                continue;
            };
            if started.elapsed() > Duration::from_secs(12) {
                partial = true;
                break;
            }
            if let Some(info) = diskutil(&["info", "-plist", id]) {
                infos.insert(id.into(), info);
            }
        }
    }
    let mut devices = parse_inventory(&list, apfs.as_ref().unwrap_or(&Value::Null), &infos);
    let helper = [
        "/opt/homebrew/sbin/smartctl",
        "/opt/homebrew/bin/smartctl",
        "/usr/local/sbin/smartctl",
        "/usr/local/bin/smartctl",
    ]
    .into_iter()
    .find(|p| std::path::Path::new(p).is_file());
    if let Some(helper) = helper {
        for device in &mut devices {
            if started.elapsed() > Duration::from_secs(20) {
                partial = true;
                break;
            }
            if let Some((_, data)) = command(
                helper,
                &[
                    "-j",
                    "-a",
                    "-n",
                    "standby,0",
                    &format!("/dev/{}", device.id),
                ],
                None,
            ) {
                if let Ok(json) = serde_json::from_slice(&data) {
                    device.smart = parse_smart(&json, device.smart.health);
                }
            }
        }
    }
    let current: HashMap<_, _> = native_counters()
        .into_iter()
        .map(|(id, entry, _, _)| (id, entry))
        .collect();
    devices.retain_mut(|device| {
        device.registry_id = identities.get(&device.id).copied();
        let unchanged = device.registry_id == current.get(&device.id).copied();
        partial |= !unchanged;
        unchanged
    });
    Inventory {
        devices,
        collected_at: Some(started),
        available: true,
        partial,
    }
}

fn parse_inventory(list: &Value, apfs: &Value, infos: &HashMap<String, Value>) -> Vec<Device> {
    let mut devices = Vec::new();
    for root in array(list, "AllDisksAndPartitions") {
        let Some(id) = root["DeviceIdentifier"]
            .as_str()
            .filter(|id| whole_disk(id) == Some(*id))
        else {
            continue;
        };
        if root["Content"] == "Apple_APFS_Container" {
            continue;
        }
        let Some(info) = infos.get(id) else {
            continue;
        };
        if info["BusProtocol"] == "Disk Image" || info["VirtualOrPhysical"] == "Virtual" {
            continue;
        }
        let health = match info["SMARTStatus"].as_str() {
            Some("Verified") => Health::Passed,
            Some("Failing") => Health::Failed,
            Some("Not Supported") => Health::Unsupported,
            _ => Health::Unavailable,
        };
        let mut volumes = Vec::new();
        for volume in std::iter::once(root).chain(array(root, "Partitions")) {
            if string(volume, "VolumeName").is_none() && string(volume, "MountPoint").is_none() {
                continue;
            }
            let Some(vid) = string(volume, "DeviceIdentifier") else {
                continue;
            };
            let detail = infos.get(&vid).unwrap_or(&Value::Null);
            let total = number(detail, "VolumeSize")
                .filter(|v| *v > 0)
                .or_else(|| number(volume, "Size"));
            let mount = string(detail, "MountPoint").or_else(|| string(volume, "MountPoint"));
            let free = mount
                .as_ref()
                .and_then(|_| number(detail, "FreeSpace"))
                .filter(|free| total.is_some_and(|total| *free <= total));
            let used = total.zip(free).map(|(total, free)| total - free);
            volumes.push(Volume {
                name: string(volume, "VolumeName").unwrap_or_else(|| vid.clone()),
                id: vid,
                mount,
                total,
                used,
                free,
                ..Default::default()
            });
        }
        for container in array(apfs, "Containers") {
            let stores: Vec<String> = array(container, "PhysicalStores")
                .iter()
                .filter_map(|s| string(s, "DeviceIdentifier"))
                .collect();
            if !stores.iter().any(|store| whole_disk(store) == Some(id)) {
                continue;
            }
            let container_id = string(container, "ContainerReference");
            let native_container = array(list, "AllDisksAndPartitions")
                .iter()
                .find(|v| string(v, "DeviceIdentifier") == container_id);
            let total = number(container, "CapacityCeiling");
            let free = number(container, "CapacityFree")
                .filter(|free| total.is_some_and(|total| *free <= total));
            for volume in array(container, "Volumes") {
                let Some(vid) = string(volume, "DeviceIdentifier") else {
                    continue;
                };
                let native = native_container.and_then(|c| {
                    array(c, "APFSVolumes")
                        .iter()
                        .find(|v| v["DeviceIdentifier"] == vid)
                });
                let mount = native.and_then(|v| {
                    string(v, "MountPoint").or_else(|| {
                        array(v, "MountedSnapshots")
                            .iter()
                            .find_map(|s| string(s, "SnapshotMountPoint"))
                    })
                });
                volumes.push(Volume {
                    id: vid.clone(),
                    name: string(volume, "Name").unwrap_or(vid),
                    mount,
                    container: container_id.clone(),
                    stores: stores.clone(),
                    total,
                    free,
                    used: number(volume, "CapacityInUse")
                        .filter(|used| total.is_some_and(|total| *used <= total)),
                });
            }
        }
        volumes.sort_by_key(|v| match v.mount.as_deref() {
            Some("/") => 0,
            Some("/System/Volumes/Data") => 1,
            Some(path) if path.starts_with("/Volumes/") => 2,
            Some(_) => 3,
            None => 4,
        });
        devices.push(Device {
            id: id.into(),
            registry_id: None,
            name: string(info, "MediaName")
                .or_else(|| string(info, "IORegistryEntryName"))
                .unwrap_or_else(|| id.into()),
            protocol: string(info, "BusProtocol"),
            internal: info["Internal"].as_bool(),
            solid_state: info["SolidState"].as_bool(),
            size: number(info, "TotalSize").or_else(|| number(root, "Size")),
            smart: Smart {
                health,
                ..Default::default()
            },
            volumes,
        });
    }
    devices.sort_by_key(|d| {
        (
            !d.volumes.iter().any(|v| v.mount.as_deref() == Some("/")),
            d.id.clone(),
        )
    });
    devices
}

fn exact_counter(value: &Value, key: &str) -> Option<u64> {
    match value.get(format!("{key}_s")) {
        Some(text) => text.as_str()?.parse().ok(),
        None => number(value, key),
    }
}

fn parse_smart(value: &Value, native_health: Health) -> Smart {
    let log = &value["nvme_smart_health_information_log"];
    let health = match value["smart_status"]["passed"].as_bool() {
        Some(false) => Health::Failed,
        Some(true) if native_health != Health::Failed => Health::Passed,
        _ => native_health,
    };
    Smart {
        health,
        temperature_c: value["temperature"]["current"]
            .as_f64()
            .filter(|t| t.is_finite() && (-40.0..=150.0).contains(t)),
        lifetime_read: exact_counter(log, "data_units_read")
            .and_then(|units| units.checked_mul(512_000)),
        lifetime_write: exact_counter(log, "data_units_written")
            .and_then(|units| units.checked_mul(512_000)),
        power_on_hours: number(&value["power_on_time"], "hours")
            .or_else(|| number(log, "power_on_hours")),
        power_cycles: number(value, "power_cycle_count").or_else(|| number(log, "power_cycles")),
        endurance_used: number(log, "percentage_used").filter(|v| *v <= 255),
        extended: value.get("smart_status").is_some()
            || value.get("temperature").is_some()
            || value.get("nvme_smart_health_information_log").is_some(),
    }
}

struct OwnedCf(CFTypeRef);
impl Drop for OwnedCf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
}
struct Entry(u32);
impl Drop for Entry {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}
extern "C" {
    fn CFDictionaryGetTypeID() -> CFTypeID;
    fn IORegistryEntryGetChildIterator(
        entry: u32,
        plane: *const std::ffi::c_char,
        iterator: *mut u32,
    ) -> i32;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
}

fn native_counters() -> Vec<(String, u64, u64, u64)> {
    let mut rows = Vec::new();
    for driver in matching_services("IOBlockStorageDriver")
        .into_iter()
        .map(Entry)
    {
        let stats = OwnedCf(entry_prop(driver.0, "Statistics"));
        if stats.0.is_null() || unsafe { CFGetTypeID(stats.0) != CFDictionaryGetTypeID() } {
            continue;
        }
        let read = cf_to_i64(dict_get(stats.0 as CFDictionaryRef, "Bytes (Read)"))
            .and_then(|n| u64::try_from(n).ok());
        let write = cf_to_i64(dict_get(stats.0 as CFDictionaryRef, "Bytes (Write)"))
            .and_then(|n| u64::try_from(n).ok());
        let (Some(read), Some(write)) = (read, write) else {
            continue;
        };
        let mut entry_id = 0;
        let mut iterator = 0;
        if unsafe {
            IORegistryEntryGetRegistryEntryID(driver.0, &mut entry_id) != 0
                || IORegistryEntryGetChildIterator(driver.0, c"IOService".as_ptr(), &mut iterator)
                    != 0
        } {
            continue;
        }
        let iterator = Entry(iterator);
        let mut ids = Vec::new();
        loop {
            let child = unsafe { IOIteratorNext(iterator.0) };
            if child == 0 {
                break;
            }
            let child = Entry(child);
            if cf_to_bool(OwnedCf(entry_prop(child.0, "Whole")).0) != Some(true) {
                continue;
            }
            if let Some(id) = cf_to_string(OwnedCf(entry_prop(child.0, "BSD Name")).0)
                .filter(|id| whole_disk(id) == Some(id.as_str()))
            {
                ids.push(id);
            }
        }
        if ids.len() == 1 {
            rows.push((ids.remove(0), entry_id, read, write));
        }
    }
    rows
}

pub fn preview() -> Option<(Inventory, HashMap<String, IoSample>)> {
    #[cfg(debug_assertions)]
    if std::env::var("TIDEGAUGE_DISK_PREVIEW").as_deref() == Ok("1") {
        let at = Instant::now();
        let first = Device {
            id: "disk0".into(),
            registry_id: Some(1),
            name: "Sample internal SSD".into(),
            protocol: Some("NVMe".into()),
            internal: Some(true),
            solid_state: Some(true),
            size: Some(1_000_000_000_000),
            smart: Smart {
                health: Health::Passed,
                temperature_c: Some(42.0),
                lifetime_read: Some(12_000_000_000_000),
                lifetime_write: Some(8_000_000_000_000),
                power_on_hours: Some(1234),
                power_cycles: Some(167),
                endurance_used: Some(3),
                extended: true,
            },
            volumes: vec![Volume {
                id: "disk2s1".into(),
                name: "Sample system".into(),
                mount: Some("/".into()),
                container: Some("disk2".into()),
                stores: vec!["disk0s2".into()],
                total: Some(990_000_000_000),
                used: Some(20_000_000_000),
                free: Some(400_000_000_000),
            }],
        };
        let second = Device {
            id: "disk3".into(),
            registry_id: None,
            name: "Sample external SSD".into(),
            internal: Some(false),
            protocol: Some("USB".into()),
            size: Some(500_000_000_000),
            solid_state: Some(true),
            smart: Smart {
                health: Health::Unsupported,
                ..Default::default()
            },
            volumes: vec![Volume {
                id: "disk3s1".into(),
                name: "Sample archive".into(),
                mount: Some("/Volumes/Sample archive".into()),
                total: Some(500_000_000_000),
                free: Some(200_000_000_000),
                used: Some(300_000_000_000),
                ..Default::default()
            }],
        };
        let io = IoSample {
            entry_id: 1,
            read_total: 500_000_000,
            write_total: 200_000_000,
            read_rate: Some(50_000_000.0),
            write_rate: Some(20_000_000.0),
            read_history: vec![0.0, 10_000_000.0, 40_000_000.0, 50_000_000.0],
            write_history: vec![0.0, 5_000_000.0, 30_000_000.0, 20_000_000.0],
            at,
        };
        return Some((
            Inventory {
                devices: vec![first, second],
                available: true,
                collected_at: Some(at),
                partial: false,
            },
            HashMap::from([("disk0".into(), io)]),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apfs_capacity_is_shared_not_volume_free_space() {
        let list = json!({"AllDisksAndPartitions": [ {"DeviceIdentifier":"disk0","Size":1000}, {"DeviceIdentifier":"disk2","Content":"Apple_APFS_Container","APFSVolumes":[{"DeviceIdentifier":"disk2s1","MountedSnapshots":[{"SnapshotMountPoint":"/"}]}]} ]});
        let apfs = json!({"Containers":[{"ContainerReference":"disk2","CapacityCeiling":900,"CapacityFree":300,"PhysicalStores":[{"DeviceIdentifier":"disk0s2"}],"Volumes":[{"DeviceIdentifier":"disk2s1","Name":"System","CapacityInUse":100},{"DeviceIdentifier":"disk2s2","Name":"Data","CapacityInUse":400}]}]});
        let infos = HashMap::from([(
            "disk0".into(),
            json!({"MediaName":"Native drive","Internal":true,"SMARTStatus":"Not Supported","TotalSize":1000}),
        )]);
        let devices = parse_inventory(&list, &apfs, &infos);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].volumes.len(), 2);
        assert_eq!(devices[0].volumes[0].mount.as_deref(), Some("/"));
        assert_eq!(devices[0].volumes[0].used, Some(100));
        assert_eq!(devices[0].volumes[1].free, Some(300));
        assert_eq!(devices[0].smart.health, Health::Unsupported);
        assert_eq!(whole_disk("disk12s3s1"), Some("disk12"));
        assert_eq!(whole_disk("disk12;echo"), None);
    }

    #[test]
    fn smart_units_missing_values_and_failures_are_preserved() {
        let value = json!({"smart_status":{"passed":false}, "temperature":{"current":42}, "nvme_smart_health_information_log":{"data_units_read":2,"data_units_written":3,"percentage_used":105}});
        let smart = parse_smart(&value, Health::Passed);
        assert_eq!(smart.health, Health::Failed);
        assert_eq!(smart.lifetime_read, Some(1_024_000));
        assert_eq!(smart.lifetime_write, Some(1_536_000));
        assert_eq!(smart.endurance_used, Some(105));
        assert_eq!(smart.power_on_hours, None);
        let empty = parse_smart(&json!({}), Health::Unavailable);
        assert_eq!(empty.temperature_c, None);
        assert_eq!(empty.lifetime_read, None);
        assert_eq!(parse_smart(&json!({"nvme_smart_health_information_log":{"data_units_read_s":"18446744073709551616"}}), Health::Unavailable).lifetime_read, None);
    }

    #[test]
    fn io_rates_reset_on_reconnect_counter_reset_or_stall() {
        let at = Instant::now();
        let first = advance(None, 10, 100, 200, at);
        assert_eq!(first.read_rate, None);
        let next = advance(Some(&first), 10, 300, 600, at + Duration::from_secs(2));
        let device = Device {
            id: "disk1".into(),
            registry_id: Some(10),
            ..Default::default()
        };
        assert!(device.identity_matches(&HashMap::from([("disk1".into(), first.clone())])));
        assert!(!device.identity_matches(&HashMap::new()));
        let replacement = advance(None, 11, 100, 200, at);
        assert!(!device.identity_matches(&HashMap::from([("disk1".into(), replacement)])));
        assert_eq!(
            (next.read_rate, next.write_rate),
            (Some(100.0), Some(200.0))
        );
        for (id, read, delay) in [(11, 300, 2), (10, 1, 2), (10, 300, 20)] {
            let reset = advance(Some(&first), id, read, 600, at + Duration::from_secs(delay));
            assert_eq!(reset.read_rate, None);
            assert!(reset.read_history.is_empty());
        }
    }

    #[test]
    #[ignore = "Reads the current Mac; compare against diskutil and IORegistry"]
    fn native_disk_inventory_probe() {
        println!("{:#?}", collect());
        println!("Device counters: {:#?}", native_counters());
    }
}
