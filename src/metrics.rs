//! System metrics collectors: CPU, memory, disk, network, battery, GPU,
//! power, thermal — everything the UI snapshots each tick.

#![allow(non_snake_case, dead_code)]

use crate::ffi::*;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::mem::{size_of, zeroed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Process rusage CPU times are Mach ticks; normalize them before computing rates.
pub fn mach_ticks_to_ns(ticks: u64) -> u64 {
    static TIMEBASE: OnceLock<mach_timebase_info_data_t> = OnceLock::new();
    let timebase = TIMEBASE.get_or_init(|| {
        let mut info = mach_timebase_info_data_t::default();
        if unsafe { mach_timebase_info(&mut info) } == KERN_SUCCESS && info.denom > 0 {
            info
        } else {
            mach_timebase_info_data_t { numer: 1, denom: 1 }
        }
    });
    ticks_to_ns(ticks, timebase.numer, timebase.denom)
}

fn ticks_to_ns(ticks: u64, numer: u32, denom: u32) -> u64 {
    (ticks as u128 * numer as u128 / denom as u128).min(u64::MAX as u128) as u64
}

pub fn sysctl_u64(name: &str) -> Option<u64> {
    let n = CString::new(name).ok()?;
    unsafe {
        let mut size = 0usize;
        if sysctlbyname(n.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut v: u64 = 0;
        if size == 8 {
            if sysctlbyname(n.as_ptr(), &mut v as *mut u64 as *mut c_void, &mut size, std::ptr::null_mut(), 0) != 0 { return None; }
            Some(v)
        } else {
            let mut v4: u32 = 0;
            if sysctlbyname(n.as_ptr(), &mut v4 as *mut u32 as *mut c_void, &mut size, std::ptr::null_mut(), 0) != 0 { return None; }
            Some(v4 as u64)
        }
    }
}

pub fn sysctl_str(name: &str) -> Option<String> {
    let n = CString::new(name).ok()?;
    unsafe {
        let mut size = 0usize;
        if sysctlbyname(n.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0 || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        if sysctlbyname(n.as_ptr(), buf.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        buf.truncate(size);
        while buf.last() == Some(&0) { buf.pop(); }
        String::from_utf8(buf).ok()
    }
}

fn swap_usage() -> Option<xsw_usage> {
    unsafe {
        let mut x: xsw_usage = zeroed();
        let mut size = size_of::<xsw_usage>();
        let n = CString::new("vm.swapusage").ok()?;
        if sysctlbyname(n.as_ptr(), &mut x as *mut xsw_usage as *mut c_void, &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        Some(x)
    }
}

#[derive(Clone, Copy, Default)]
pub struct CpuTicks {
    pub user: u64,
    pub system: u64,
    pub idle: u64,
    pub nice: u64,
}

pub fn cpu_ticks() -> Option<CpuTicks> {
    let cores = core_ticks()?;
    let mut t = CpuTicks::default();
    for c in cores {
        t.user += c.user;
        t.system += c.system;
        t.idle += c.idle;
        t.nice += c.nice;
    }
    Some(t)
}

/// Busy share of one core between two tick readings, 0–100 %.
pub fn busy_pct(a: CpuTicks, b: CpuTicks) -> f64 {
    let busy = (b.user.saturating_sub(a.user) + b.nice.saturating_sub(a.nice) + b.system.saturating_sub(a.system)) as f64;
    let idle = b.idle.saturating_sub(a.idle) as f64;
    if busy + idle <= 0. { 0. } else { (busy / (busy + idle) * 100.).clamp(0., 100.) }
}

/// Each logical CPU's ticks, in the kernel's order.
pub fn core_ticks() -> Option<Vec<CpuTicks>> {
    unsafe {
        let host = mach_host_self();
        let mut count: natural_t = 0;
        let mut info: *mut c_int = std::ptr::null_mut();
        let mut info_count: natural_t = 0;
        if host_processor_info(
            host,
            PROCESSOR_CPU_LOAD_INFO,
            &mut count,
            &mut info,
            &mut info_count,
        ) != KERN_SUCCESS || info.is_null() {
            return None;
        }
        let ticks = std::slice::from_raw_parts(info as *const u32, info_count as usize);
        let cores = ticks
            .chunks_exact(4)
            .map(|c| CpuTicks {
                user: c[CPU_STATE_USER] as u64,
                system: c[CPU_STATE_SYSTEM] as u64,
                idle: c[CPU_STATE_IDLE] as u64,
                nice: c[CPU_STATE_NICE] as u64,
            })
            .collect();
        vm_deallocate(
            mach_task_self(),
            info as usize,
            info_count as usize * size_of::<c_int>(),
        );
        Some(cores)
    }
}

pub fn load_avg() -> [f64; 3] {
    unsafe {
        let mut a = [0f64; 3];
        getloadavg(a.as_mut_ptr(), 3);
        a
    }
}

#[derive(Clone, Copy, Default)]
pub struct MemStats {
    pub app: u64,
    pub wired: u64,
    pub compressed: u64,
    pub cached: u64,
    pub free: u64,
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    /// macOS's own memory-pressure level, the one Activity Monitor colours.
    pub pressure: Option<Pressure>,
    /// The share of memory macOS counts as available (`kern.memorystatus_level`).
    pub available_pct: Option<u8>,
}

/// The kernel's memory-pressure level. macOS reports 1, 2 or 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

impl Pressure {
    pub fn from_level(level: u64) -> Option<Self> {
        match level {
            1 => Some(Self::Normal),
            2 => Some(Self::Warning),
            4 => Some(Self::Critical),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        use crate::locale::text as l;
        match self {
            Self::Normal => l("Normal", "Normalt"),
            Self::Warning => l("Warning", "Advarsel"),
            Self::Critical => l("Critical", "Kritisk"),
        }
    }
}

fn available_pct(level: u64) -> Option<u8> {
    (level <= 100).then_some(level as u8)
}

pub fn mem_stats() -> MemStats {
    unsafe {
        let host = mach_host_self();
        let mut vms: vm_statistics64 = zeroed();
        let mut count = (size_of::<vm_statistics64>() / size_of::<natural_t>()) as natural_t;
        let mut out = MemStats::default();
        let mut page_size: natural_t = 4096;
        let _ = host_page_size(host, &mut page_size);
        if host_statistics64(host, HOST_VM_INFO64, &mut vms, &mut count) == KERN_SUCCESS {
            let ps = page_size as u64;
            let free = (vms.free_count as u64 + vms.speculative_count as u64) * ps;
            let purgeable = vms.purgeable_count as u64 * ps;
            let internal = vms.internal_page_count as u64 * ps;
            let external = vms.external_page_count as u64 * ps;
            let wired = vms.wire_count as u64 * ps;
            let compressed = vms.compressor_page_count as u64 * ps;
            let app = internal.saturating_sub(purgeable);
            let cached = purgeable + external;
            out = MemStats {
                app,
                wired,
                compressed,
                cached,
                free,
                used: app + wired + compressed,
                total: sysctl_u64("hw.memsize").unwrap_or(0),
                ..Default::default()
            };
        }
        out.total = out.total.max(1);
        if let Some(sw) = swap_usage() {
            out.swap_used = sw.xsu_used;
            out.swap_total = sw.xsu_total;
        }
        out.pressure = sysctl_u64("kern.memorystatus_vm_pressure_level").and_then(Pressure::from_level);
        out.available_pct = sysctl_u64("kern.memorystatus_level").and_then(available_pct);
        out
    }
}

#[derive(Clone, Default)]
pub struct DiskStats {
    pub total: u64,
    pub free: u64,
    pub used: u64,
    pub read_bytes_total: u64,
    pub write_bytes_total: u64,
    pub volume_count: usize,
    pub volume_names: Vec<String>,
}

impl DiskStats {
    pub fn volumes() -> (usize, Vec<String>) {
        // count mounted local volumes via getmntinfo would need more FFI;
        // keep it simple: read /Volumes plus root.
        let mut names = vec!["Macintosh HD".to_string()];
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n != "Macintosh HD" && !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        (names.len(), names)
    }
}

#[repr(C)]
struct statfs64 {
    f_bsize: u32,
    f_iosize: i32,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_fsid: u64,
    f_owner: u32,
    f_type: u32,
    f_flags: u32,
    f_fssubtype: u32,
    f_fstypename: [u8; 16],
    f_mntonname: [u8; 1024],
    f_mntfromname: [u8; 1024],
    f_flags_ext: u32,
    f_reserved: [u32; 7],
}

extern "C" {
    fn statfs(path: *const std::ffi::c_char, buf: *mut c_void) -> c_int;
}

fn statfs_free_total(path: &str) -> Option<(u64, u64)> {
    unsafe {
        let mut buf = vec![0u8; 2168];
        let c = CString::new(path).ok()?;
        if statfs(c.as_ptr(), buf.as_mut_ptr() as *mut c_void) != 0 {
            return None;
        }
        let bsize = u32::from_ne_bytes(buf[0..4].try_into().ok()?) as u64;
        let blocks = u64::from_ne_bytes(buf[8..16].try_into().ok()?);
        let bavail = u64::from_ne_bytes(buf[24..32].try_into().ok()?);
        Some((bavail * bsize, blocks * bsize))
    }
}

pub fn disk_stats() -> DiskStats {
    let (free, total) = statfs_free_total("/").unwrap_or((0, 0));
    let (count, names) = DiskStats::volumes();
    DiskStats {
        total,
        free,
        used: total.saturating_sub(free),
        volume_count: count,
        volume_names: names,
        ..Default::default()
    }
}

// ---- IOKit helpers ----

pub fn cf_str(s: &str) -> CFStringRef {
    unsafe {
        extern "C" {
            fn CFStringCreateWithCString(
                alloc: CFAllocatorRef,
                cstr: *const c_char,
                encoding: u32,
            ) -> CFStringRef;
        }
        let c = CString::new(s).unwrap();
        CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    }
}

pub fn cf_to_string(s: CFTypeRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(s) != CFStringGetTypeID() {
            return None;
        }
        let mut buf = [0i8; 256];
        if CFStringGetCString(s as CFStringRef, buf.as_mut_ptr(), 256, K_CF_STRING_ENCODING_UTF8) != 0 {
            let c = CStr::from_ptr(buf.as_ptr());
            Some(c.to_string_lossy().to_string())
        } else {
            None
        }
    }
}

pub fn cf_to_i64(v: CFTypeRef) -> Option<i64> {
    if v.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(v) == CFNumberGetTypeID() {
            let mut out: i64 = 0;
            if CFNumberGetValue(v as CFNumberRef, K_CF_NUMBER_SINT64, &mut out as *mut i64 as *mut c_void) != 0 {
                return Some(out);
            }
            let mut f: f64 = 0.0;
            if CFNumberGetValue(v as CFNumberRef, K_CF_NUMBER_FLOAT64, &mut f as *mut f64 as *mut c_void) != 0 {
                return Some(f as i64);
            }
        }
        None
    }
}

pub fn cf_to_bool(v: CFTypeRef) -> Option<bool> {
    if v.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(v) == CFBooleanGetTypeID() {
            return Some(CFBooleanGetValue(v) != 0);
        }
        if CFGetTypeID(v) == CFNumberGetTypeID() {
            return cf_to_i64(v).map(|n| n != 0);
        }
    }
    None
}

pub fn dict_get(dict: CFDictionaryRef, key: &str) -> CFTypeRef {
    if dict.is_null() {
        return std::ptr::null();
    }
    let k = cf_str(key);
    let v = unsafe { CFDictionaryGetValue(dict, k as CFTypeRef) };
    unsafe { CFRelease(k as CFTypeRef) };
    v
}

pub struct IoIterator(pub io_iterator_t);
impl Drop for IoIterator {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0) };
    }
}
impl Iterator for IoIterator {
    type Item = io_object_t;
    fn next(&mut self) -> Option<io_object_t> {
        let o = unsafe { IOIteratorNext(self.0) };
        if o != 0 { Some(o) } else { None }
    }
}

pub fn matching_services(name: &str) -> IoIterator {
    let c = CString::new(name).unwrap();
    unsafe {
        let dict = IOServiceMatching(c.as_ptr());
        let mut it: io_iterator_t = 0;
        if IOServiceGetMatchingServices(0, dict, &mut it) == KERN_SUCCESS {
            IoIterator(it)
        } else {
            IoIterator(0)
        }
    }
}

pub fn entry_prop(entry: io_registry_entry_t, key: &str) -> CFTypeRef {
    let k = cf_str(key);
    let v = unsafe { IORegistryEntryCreateCFProperty(entry, k, std::ptr::null(), 0) };
    unsafe { CFRelease(k as CFTypeRef) };
    v
}

// ---- Storage-driver disk I/O totals ----
pub fn disk_io_totals() -> (u64, u64) {
    let mut read = 0u64;
    let mut write = 0u64;
    for e in matching_services("IOBlockStorageDriver") {
        let stats = entry_prop(e, "Statistics");
        if !stats.is_null() {
            read += cf_to_i64(dict_get(stats as CFDictionaryRef, "Bytes (Read)"))
                .unwrap_or(0) as u64;
            write += cf_to_i64(dict_get(stats as CFDictionaryRef, "Bytes (Write)"))
                .unwrap_or(0) as u64;
            unsafe { CFRelease(stats) };
        }
        unsafe { IOObjectRelease(e) };
    }
    (read, write)
}

// ---- GPU ----
#[derive(Clone, Copy, Default)]
pub struct GpuStats {
    pub utilization: Option<f64>,
    pub mem_in_use: Option<u64>,
    pub supported: bool,
    /// Measured GPU power from macOS's energy counters, in watts.
    pub power_w: Option<f64>,
}

pub fn gpu_stats() -> GpuStats {
    for class in ["IOAccelerator", "IOGPU"] {
        for e in matching_services(class) {
            let stats = entry_prop(e, "PerformanceStatistics");
            if !stats.is_null() {
                let d = stats as CFDictionaryRef;
                let mut g = GpuStats { supported: true, ..Default::default() };
                for (key, dst) in [
                    ("Device Utilization %", "util"),
                    ("GPU activity(%)", "util"),
                    ("GPU Utilization %", "util"),
                    ("In use system memory", "mem"),
                    ("Alloc system memory", "mem"),
                ] {
                    if let Some(v) = cf_to_i64(dict_get(d, key)) {
                        if dst == "util" {
                            g.utilization = Some(v as f64);
                        } else {
                            g.mem_in_use = Some(v as u64);
                        }
                    }
                }
                unsafe { CFRelease(stats) };
                unsafe { IOObjectRelease(e) };
                return g;
            }
            unsafe { IOObjectRelease(e) };
        }
    }
    GpuStats::default()
}

pub use crate::battery::{battery_stats, BatteryStats};

// ---- Network (system counters) ----
#[derive(Clone, Default)]
pub struct NetStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub primary_iface: String,
    pub iface_kind: String,
}

pub fn net_counters() -> (u64, u64) {
    unsafe {
        let mut ifas: *mut ifaddrs = std::ptr::null_mut();
        if getifaddrs(&mut ifas) != 0 || ifas.is_null() {
            return (0, 0);
        }
        let mut rx = 0u64;
        let mut tx = 0u64;
        let mut cur = ifas;
        while !cur.is_null() {
            let a = &*cur;
            if !a.ifa_addr.is_null()
                && (*a.ifa_addr).sa_family == AF_LINK
                && !a.ifa_data.is_null()
                && a.ifa_flags & IFF_LOOPBACK == 0
            {
                let d = &*a.ifa_data;
                rx += d.ifi_ibytes as u64;
                tx += d.ifi_obytes as u64;
            }
            cur = a.ifa_next;
        }
        freeifaddrs(ifas);
        (rx, tx)
    }
}

static DEFAULT_IFACE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

pub fn default_interface() -> String {
    {
        let g = DEFAULT_IFACE.lock().unwrap();
        if let Some((t, name)) = g.as_ref() {
            if !name.is_empty() && t.elapsed() < Duration::from_secs(30) {
                return name.clone();
            }
        }
    }
    let mut iface = String::from("en0");
    if let Ok(out) = std::process::Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with("interface:") {
                iface = l.trim_start_matches("interface:").trim().to_string();
            }
        }
    }
    let mut g = DEFAULT_IFACE.lock().unwrap();
    *g = Some((Instant::now(), iface.clone()));
    iface
}

// ---- Chip / CPU info ----
#[derive(Clone, Default)]
pub struct ChipInfo {
    pub brand: String,
    pub cores: usize,
    pub p_cores: usize,
    pub e_cores: usize,
    /// Core groups in the kernel's CPU order, with macOS's own names
    /// ("Super", "Performance", "Efficiency").
    pub groups: Vec<CoreGroup>,
    /// The GPU's core count, where the GPU reports it.
    pub gpu_cores: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreGroup {
    pub name: String,
    /// The device tree's cluster type ("P", "M", "E"), which the clock
    /// readings are keyed by.
    pub kind: String,
    pub first: usize,
    pub count: usize,
}

/// Group consecutive CPUs of one cluster type. macOS lists the slowest
/// performance level's cores first, so group i in CPU order is perflevel
/// `levels.len() - 1 - i`; its name is used when the core counts agree.
pub fn core_groups(kinds: &[String], levels: &[(String, usize)]) -> Vec<CoreGroup> {
    let mut groups: Vec<CoreGroup> = Vec::new();
    for (cpu, kind) in kinds.iter().enumerate() {
        match groups.last_mut() {
            Some(g) if &g.kind == kind => g.count += 1,
            _ => groups.push(CoreGroup { name: String::new(), kind: kind.clone(), first: cpu, count: 1 }),
        }
    }
    // Clusters of one type can be split (two "M" clusters); a level counts
    // all cores of its type.
    let mut merged: Vec<CoreGroup> = Vec::new();
    for g in groups {
        match merged.iter_mut().find(|m| m.kind == g.kind && m.first + m.count == g.first) {
            Some(m) => m.count += g.count,
            None => merged.push(g),
        }
    }
    let n = merged.len();
    for (i, g) in merged.iter_mut().enumerate() {
        g.name = levels
            .get(n - 1 - i)
            .filter(|(_, count)| *count == g.count)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| g.kind.clone());
    }
    merged
}

pub fn chip_info() -> ChipInfo {
    let brand = sysctl_str("machdep.cpu.brand_string")
        .or_else(|| sysctl_str("hw.model"))
        .unwrap_or_else(|| "Apple Silicon".to_string());
    let cores = sysctl_u64("hw.ncpu").unwrap_or(0) as usize;
    let levels = sysctl_u64("hw.nperflevels").unwrap_or(1) as usize;
    let (mut p, mut e) = (cores, 0);
    if levels > 1 {
        p = sysctl_u64("hw.perflevel0.physicalcpu").unwrap_or(cores as u64) as usize;
        e = sysctl_u64("hw.perflevel1.physicalcpu").unwrap_or(0) as usize;
    }
    let level_info: Vec<(String, usize)> = (0..levels)
        .map(|i| {
            (
                sysctl_str(&format!("hw.perflevel{i}.name")).unwrap_or_default(),
                sysctl_u64(&format!("hw.perflevel{i}.logicalcpu")).unwrap_or(0) as usize,
            )
        })
        .collect();
    let groups = core_groups(&crate::clocks::cluster_types(), &level_info);
    let gpu_cores = matching_services("AGXAccelerator").find_map(|e| {
        let value = entry_prop(e, "gpu-core-count");
        let count = cf_to_i64(value).filter(|n| (1..1024).contains(n)).map(|n| n as u32);
        if !value.is_null() {
            unsafe { CFRelease(value) };
        }
        unsafe { IOObjectRelease(e) };
        count
    });
    ChipInfo {
        brand,
        cores,
        p_cores: p,
        e_cores: e,
        groups,
        gpu_cores,
    }
}

// ---- Thermal ----
/// macOS's thermal state: 0 nominal, 1 fair, 2 serious, 3 critical.
pub fn thermal_level() -> Option<u8> {
    let level = objc2_foundation::NSProcessInfo::processInfo().thermalState().0;
    (0..=3).contains(&level).then_some(level as u8)
}

pub fn thermal_state() -> Option<&'static str> {
    use crate::locale::text as l;
    match objc2_foundation::NSProcessInfo::processInfo().thermalState().0 {
        0 => Some(l("Nominal", "Normal")),
        1 => Some(l("Fair", "Moderat")),
        2 => Some(l("Serious", "Alvorlig")),
        3 => Some(l("Critical", "Kritisk")),
        _ => None,
    }
}


#[cfg(test)]
mod tests {
    use super::Pressure;

    #[test]
    fn pressure_levels_follow_the_kernel_values() {
        assert_eq!(Pressure::from_level(1), Some(Pressure::Normal));
        assert_eq!(Pressure::from_level(2), Some(Pressure::Warning));
        assert_eq!(Pressure::from_level(4), Some(Pressure::Critical));
        assert_eq!(Pressure::from_level(0), None, "an unknown level is not shown as normal");
        assert_eq!(Pressure::from_level(3), None);
        assert_eq!(super::available_pct(47), Some(47));
        assert_eq!(super::available_pct(101), None);
    }

    #[test]
    #[ignore = "requires macOS"]
    fn native_memory_pressure_reads() {
        let m = super::mem_stats();
        eprintln!("pressure {:?}, available {:?} %", m.pressure, m.available_pct);
        assert!(m.pressure.is_some() && m.available_pct.is_some());
    }

    #[test]
    fn cpu_time_conversion_uses_the_platform_timebase() {
        assert_eq!(super::ticks_to_ns(24_000_000, 125, 3), 1_000_000_000);
        assert_eq!(super::ticks_to_ns(1_000_000_000, 1, 1), 1_000_000_000);
    }

    #[test]
    #[ignore = "requires a Mac with IOBlockStorageDriver counters"]
    fn native_disk_counters_report_activity_since_boot() {
        let (read, write) = super::disk_io_totals();
        assert!(read > 0 || write > 0, "no disk bytes were collected");
    }
}
