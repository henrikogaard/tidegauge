//! CPU and GPU clock speeds without special permissions.
//!
//! IOReport's "CPU Stats" and "GPU Stats" groups count how long each core and
//! the GPU spend in each performance state. The power manager's device-tree
//! entry lists the frequency of each state. The average active frequency is
//! the residency-weighted mean over the states that are not idle or off.
//!
//! Checked on a Mac17,7 (M5 Max, macOS 27.0.1): Performance cores ("M"
//! clusters) use the 15-state table of 1.344–4.380 GHz, Super cores ("P"
//! cluster) the 20-state table of 1.308–4.608 GHz, and the GPU 338–1620 MHz,
//! the same steps `powermetrics` reports. A table is only used when its
//! number of states matches the counter's, so a chip with a different layout
//! gets no clock reading rather than a wrong one.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use crate::ffi::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFRelease, CFStringRef};

type Subscription = *mut c_void;
type Dict = *const c_void;
type MutableDict = *mut c_void;

#[link(name = "IOReport")]
extern "C" {
    fn IOReportCopyChannelsInGroup(group: CFStringRef, subgroup: CFStringRef, a: u64, b: u64, c: u64) -> Dict;
    fn IOReportMergeChannels(a: Dict, b: Dict, c: *const c_void);
    fn IOReportCreateSubscription(a: *const c_void, channels: MutableDict, out: *mut MutableDict, b: u64, c: *const c_void) -> Subscription;
    fn IOReportCreateSamples(subscription: Subscription, channels: MutableDict, a: *const c_void) -> Dict;
    fn IOReportCreateSamplesDelta(before: Dict, after: Dict, a: *const c_void) -> Dict;
    fn IOReportChannelGetChannelName(channel: Dict) -> CFStringRef;
    fn IOReportChannelGetSubGroup(channel: Dict) -> CFStringRef;
    fn IOReportStateGetCount(channel: Dict) -> i32;
    fn IOReportStateGetNameForIndex(channel: Dict, index: i32) -> CFStringRef;
    fn IOReportStateGetResidency(channel: Dict, index: i32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryCreateMutableCopy(allocator: *const c_void, capacity: isize, dict: Dict) -> MutableDict;
    fn CFDictionaryGetValue(dict: Dict, key: *const c_void) -> *const c_void;
    fn CFDataGetLength(data: *const c_void) -> isize;
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IORegistryEntryFromPath(port: u32, path: *const std::ffi::c_char) -> u32;
}

/// Clock speeds since the previous sample, in MHz.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clocks {
    /// Per core group, keyed by the device tree's cluster type ("P", "M", "E").
    pub cpu: HashMap<String, f64>,
    pub gpu: Option<f64>,
}

/// The first 32-bit value of each 8-byte (frequency, voltage) entry.
pub fn table(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(8).map(|e| u32::from_le_bytes([e[0], e[1], e[2], e[3]])).collect()
}

/// A CPU table with `states` entries in kHz, as MHz; `None` when no table or
/// more than one different table has that many states.
pub fn cpu_table(tables: &HashMap<String, Vec<u32>>, states: usize) -> Option<Vec<f64>> {
    let mut found: Option<&Vec<u32>> = None;
    for (name, t) in tables {
        let khz = t.iter().all(|f| (100_000..10_000_000).contains(f));
        if name.ends_with("-sram") && t.len() == states && khz {
            match found {
                Some(other) if other != t => return None,
                _ => found = Some(t),
            }
        }
    }
    found.map(|t| t.iter().map(|khz| *khz as f64 / 1000.).collect())
}

/// The GPU's frequencies in MHz: a table that starts with an off state of 0
/// followed by rising frequencies in Hz, with no more entries than the
/// counter has performance states. On the Mac17,7 the counter has 15 and the
/// table 13; the top two are never entered, and `weighted_mhz` refuses the
/// table if one ever is.
pub fn gpu_table(tables: &HashMap<String, Vec<u32>>, states: usize) -> Option<Vec<f64>> {
    let mut found: Option<&Vec<u32>> = None;
    for (name, t) in tables {
        let shape = !name.ends_with("-sram")
            && t.len() > 2
            && t.len() - 1 <= states
            && t[0] == 0
            && t[1..].iter().all(|f| (100_000_000..5_000_000_000).contains(&(*f as u64)))
            && t[1..].windows(2).all(|w| w[0] < w[1]);
        if shape {
            match found {
                Some(other) if other != t => return None,
                _ => found = Some(t),
            }
        }
    }
    found.map(|t| t[1..].iter().map(|hz| *hz as f64 / 1_000_000.).collect())
}

/// The residency-weighted frequency over active states. CPU states are named
/// "V{i}P{j}" (table index i); GPU states "P{n}" (table index n-1). Idle,
/// down and off states are not active. Time spent in a state the table has
/// no frequency for means the table does not fit: no clock rather than one
/// that leaves that time out.
pub fn weighted_mhz(states: &[(String, i64)], freqs: &[f64]) -> Option<f64> {
    let (mut sum, mut time) = (0f64, 0i64);
    for (name, residency) in states {
        let index = if let Some(rest) = name.strip_prefix('V') {
            rest.split('P').next().and_then(|i| i.parse::<usize>().ok())
        } else if let Some(n) = name.strip_prefix('P') {
            n.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
        } else {
            None
        };
        if let (Some(i), true) = (index, *residency > 0) {
            let f = freqs.get(i)?;
            sum += f * *residency as f64;
            time += residency;
        }
    }
    (time > 0).then(|| sum / time as f64)
}

/// The cluster type ("P", "M", "E") of each logical CPU, in order.
pub fn cluster_types() -> Vec<String> {
    let mut out = Vec::new();
    for cpu in 0..256 {
        let path = std::ffi::CString::new(format!("IODeviceTree:/cpus/cpu{cpu}")).unwrap();
        let entry = unsafe { IORegistryEntryFromPath(0, path.as_ptr()) };
        if entry == 0 {
            break;
        }
        let value = crate::metrics::entry_prop(entry, "cluster-type");
        let kind = data(value).and_then(|b| b.first().copied()).map(|c| (c as char).to_string());
        if !value.is_null() {
            unsafe { CFRelease(value) };
        }
        unsafe { crate::ffi::IOObjectRelease(entry) };
        out.push(kind.unwrap_or_default());
    }
    out
}

fn data(value: *const c_void) -> Option<Vec<u8>> {
    if value.is_null() {
        return None;
    }
    unsafe {
        let len = CFDataGetLength(value);
        let ptr = CFDataGetBytePtr(value);
        (len > 0 && !ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, len as usize).to_vec())
    }
}

fn frequency_tables() -> HashMap<String, Vec<u32>> {
    let mut tables = HashMap::new();
    let entry = unsafe { IORegistryEntryFromPath(0, c"IODeviceTree:/arm-io/pmgr".as_ptr()) };
    if entry == 0 {
        return tables;
    }
    for n in 0..64 {
        for suffix in ["", "-sram"] {
            let key = format!("voltage-states{n}{suffix}");
            let value = crate::metrics::entry_prop(entry, &key);
            if let Some(bytes) = data(value) {
                tables.insert(key, table(&bytes));
            }
            if !value.is_null() {
                unsafe { CFRelease(value) };
            }
        }
    }
    unsafe { crate::ffi::IOObjectRelease(entry) };
    tables
}

struct Reader {
    subscription: Subscription,
    channels: MutableDict,
    last: Option<Dict>,
    tables: HashMap<String, Vec<u32>>,
}

thread_local! {
    static READER: RefCell<Option<Option<Reader>>> = const { RefCell::new(None) };
}

fn open() -> Option<Reader> {
    unsafe {
        let cpu_group = crate::metrics::cf_str("CPU Stats");
        let gpu_group = crate::metrics::cf_str("GPU Stats");
        let cpu = IOReportCopyChannelsInGroup(cpu_group, std::ptr::null(), 0, 0, 0);
        let gpu = IOReportCopyChannelsInGroup(gpu_group, std::ptr::null(), 0, 0, 0);
        CFRelease(cpu_group);
        CFRelease(gpu_group);
        if cpu.is_null() {
            return None;
        }
        if !gpu.is_null() {
            IOReportMergeChannels(cpu, gpu, std::ptr::null());
            CFRelease(gpu);
        }
        let channels = CFDictionaryCreateMutableCopy(std::ptr::null(), 0, cpu);
        CFRelease(cpu);
        let mut subscribed: MutableDict = std::ptr::null_mut();
        let subscription = IOReportCreateSubscription(std::ptr::null(), channels, &mut subscribed, 0, std::ptr::null());
        CFRelease(channels);
        (!subscription.is_null() && !subscribed.is_null()).then(|| Reader { subscription, channels: subscribed, last: None, tables: frequency_tables() })
    }
}

/// Clock speeds since the previous call on this thread. The first call only
/// takes a baseline.
pub fn sample() -> Clocks {
    READER.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(open).as_mut().and_then(read).unwrap_or_default()
    })
}

fn read(reader: &mut Reader) -> Option<Clocks> {
    unsafe {
        let now = IOReportCreateSamples(reader.subscription, reader.channels, std::ptr::null());
        if now.is_null() {
            return None;
        }
        let before = reader.last.replace(now)?;
        let delta = IOReportCreateSamplesDelta(before, now, std::ptr::null());
        CFRelease(before);
        if delta.is_null() {
            return None;
        }
        let key = crate::metrics::cf_str("IOReportChannels");
        let items = CFDictionaryGetValue(delta, key) as CFArrayRef;
        CFRelease(key);
        // Each cluster type's state residencies, summed over its cores.
        let mut cpu: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        let mut gpu: Vec<(String, i64)> = Vec::new();
        if !items.is_null() {
            for i in 0..CFArrayGetCount(items) {
                let channel = CFArrayGetValueAtIndex(items, i);
                let sub = crate::metrics::cf_to_string(IOReportChannelGetSubGroup(channel)).unwrap_or_default();
                let name = crate::metrics::cf_to_string(IOReportChannelGetChannelName(channel)).unwrap_or_default();
                let states: Vec<(String, i64)> = (0..IOReportStateGetCount(channel))
                    .map(|k| {
                        let state = crate::metrics::cf_to_string(IOReportStateGetNameForIndex(channel, k)).unwrap_or_default();
                        (state.trim().to_string(), IOReportStateGetResidency(channel, k))
                    })
                    .collect();
                if sub == "CPU Core Performance States" {
                    // "MCPU03" → cluster type "M".
                    if let Some(kind) = name.trim_end_matches(|c: char| c.is_ascii_digit()).strip_suffix("CPU") {
                        let entry = cpu.entry(kind.to_string()).or_insert_with(|| states.iter().map(|(n, _)| (n.clone(), 0)).collect());
                        for (slot, (_, r)) in entry.iter_mut().zip(&states) {
                            slot.1 += r;
                        }
                    }
                } else if sub == "GPU Performance States" && name == "GPUPH" {
                    gpu = states;
                }
            }
        }
        CFRelease(delta);
        let mut clocks = Clocks::default();
        for (kind, states) in &cpu {
            let v_states = states.iter().filter(|(n, _)| n.starts_with('V')).count();
            if let Some(freqs) = cpu_table(&reader.tables, v_states) {
                if let Some(mhz) = weighted_mhz(states, &freqs) {
                    clocks.cpu.insert(kind.clone(), mhz);
                }
            }
        }
        let gpu_states = gpu.iter().filter(|(n, _)| n.starts_with('P')).count();
        if let Some(freqs) = gpu_table(&reader.tables, gpu_states) {
            clocks.gpu = weighted_mhz(&gpu, &freqs);
        }
        Some(clocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(freqs: &[u32]) -> Vec<u8> {
        freqs.iter().flat_map(|f| f.to_le_bytes().into_iter().chain(800u32.to_le_bytes())).collect()
    }

    #[test]
    fn tables_are_read_from_frequency_voltage_pairs() {
        assert_eq!(table(&entries(&[1_344_000, 4_380_000])), [1_344_000, 4_380_000]);
    }

    #[test]
    fn the_cpu_table_is_the_one_with_as_many_states() {
        let mut t = HashMap::new();
        t.insert("voltage-states22-sram".into(), vec![1_344_000, 2_000_000, 4_380_000]);
        t.insert("voltage-states23-sram".into(), vec![1_344_000, 2_000_000, 4_380_000]);
        t.insert("voltage-states5-sram".into(), vec![1_308_000, 2_000_000, 3_000_000, 4_608_000]);
        t.insert("voltage-states22".into(), vec![48_761, 39_863, 14_962]);
        assert_eq!(cpu_table(&t, 3), Some(vec![1344., 2000., 4380.]), "identical copies agree");
        assert_eq!(cpu_table(&t, 4), Some(vec![1308., 2000., 3000., 4608.]));
        assert_eq!(cpu_table(&t, 9), None, "no table of that size, no clock");
        t.insert("voltage-states40-sram".into(), vec![1_000_000, 2_000_000, 3_000_000]);
        assert_eq!(cpu_table(&t, 3), None, "two different candidates, no guess");
    }

    #[test]
    fn the_gpu_table_starts_with_off() {
        let mut t = HashMap::new();
        t.insert("voltage-states9".into(), vec![0, 338_000_000, 486_000_000, 1_620_000_000]);
        t.insert("voltage-states8".into(), vec![732_000_000, 816_000_000]);
        assert_eq!(gpu_table(&t, 15), Some(vec![338., 486., 1620.]));
        assert_eq!(gpu_table(&t, 2), None, "more speeds than the counter has steps");
    }

    #[test]
    fn active_states_are_weighted_by_time() {
        let cpu = [("IDLE".to_string(), 900), ("V0P2".into(), 50), ("V2P0".into(), 50)];
        assert_eq!(weighted_mhz(&cpu, &[1000., 2000., 4000.]), Some(2500.), "idle time is left out");
        let gpu = [("OFF".to_string(), 10), ("P1".into(), 30), ("P3".into(), 10)];
        assert_eq!(weighted_mhz(&gpu, &[338., 486., 636.]), Some((338. * 30. + 636. * 10.) / 40.));
        assert_eq!(weighted_mhz(&[("IDLE".to_string(), 5)], &[1000.]), None, "all idle, no clock");
        let beyond = [("P1".to_string(), 10), ("P5".into(), 10)];
        assert_eq!(weighted_mhz(&beyond, &[338., 486.]), None, "time in a step with no speed: no clock");
        let unused = [("P1".to_string(), 10), ("P5".into(), 0)];
        assert_eq!(weighted_mhz(&unused, &[338., 486.]), Some(338.), "an unused step beyond the table is fine");
    }

    #[test]
    #[ignore = "needs an Apple silicon Mac"]
    fn native_clocks_read() {
        eprintln!("cluster types {:?}", cluster_types());
        let _ = sample();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let clocks = sample();
        eprintln!("{clocks:?}");
        assert!(!clocks.cpu.is_empty(), "no CPU clocks");
    }
}
