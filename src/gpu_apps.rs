//! GPU time per process, read from the GPU's user clients in the I/O
//! Registry without special permissions. Each process that talks to the GPU
//! holds one or more clients under the GPU node; each client reports the
//! process as `IOUserClientCreator` ("pid 443, WindowServer") and a cumulative
//! `accumulatedGPUTime` in nanoseconds for each API it used (`AppUsage`).
//! Activity Monitor's GPU column reads the same counters.
//!
//! Checked on a Mac17,7 (M5 Max, macOS 27): the per-process changes over a
//! few seconds add up to roughly the GPU's own "Device Utilization %". Work
//! macOS does on an app's behalf, such as compositing its windows, is counted
//! under WindowServer, not under the app.
//!
//! Some GPU users, WindowServer among them, run as another user, so Tidegauge
//! cannot read their other counters and they are not in its process list.
//! `Unlisted` gives those their own rows, named from the client's creator.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;

use crate::ffi::*;
use crate::metrics::{cf_to_i64, cf_to_string, dict_get, entry_prop, matching_services};

/// One process's GPU use: cumulative nanoseconds over all its GPU clients,
/// and the process name its clients report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuClient {
    pub ns: u64,
    pub name: String,
}

/// GPU use per process id. Empty when the GPU exposes no clients (virtual Macs).
pub fn gpu_time_by_pid() -> HashMap<i32, GpuClient> {
    let mut out = HashMap::new();
    let plane = CString::new("IOService").unwrap();
    for gpu in matching_services("IOAccelerator") {
        let mut children: io_iterator_t = 0;
        if unsafe { IORegistryEntryGetChildIterator(gpu, plane.as_ptr(), &mut children) } == KERN_SUCCESS {
            let children = crate::metrics::IoIterator(children);
            for client in children {
                if let Some((pid, name, ns)) = client_usage(client) {
                    let entry = out.entry(pid).or_insert_with(|| GpuClient { ns: 0, name });
                    entry.ns = entry.ns.saturating_add(ns);
                }
                unsafe { IOObjectRelease(client) };
            }
        }
        unsafe { IOObjectRelease(gpu) };
    }
    out
}

fn client_usage(client: io_registry_entry_t) -> Option<(i32, String, u64)> {
    let creator = entry_prop(client, "IOUserClientCreator");
    let parts = cf_to_string(creator).and_then(|s| creator_parts(&s));
    if !creator.is_null() {
        unsafe { CFRelease(creator) };
    }
    let (pid, name) = parts?;
    let usage = entry_prop(client, "AppUsage");
    if usage.is_null() {
        return None;
    }
    let mut ns = 0u64;
    unsafe {
        if CFGetTypeID(usage) == CFArrayGetTypeID() {
            for i in 0..CFArrayGetCount(usage as CFArrayRef) {
                let item = CFArrayGetValueAtIndex(usage as CFArrayRef, i);
                if !item.is_null() && CFGetTypeID(item) == CFDictionaryGetTypeID() {
                    let t = cf_to_i64(dict_get(item as CFDictionaryRef, "accumulatedGPUTime")).unwrap_or(0);
                    ns = ns.saturating_add(t.max(0) as u64);
                }
            }
        }
        CFRelease(usage);
    }
    Some((pid, name, ns))
}

/// The process id and name in an `IOUserClientCreator` value such as
/// "pid 443, WindowServer".
pub fn creator_parts(creator: &str) -> Option<(i32, String)> {
    let rest = creator.trim().strip_prefix("pid ")?;
    let (digits, name) = rest.split_once(',').unwrap_or((rest, ""));
    let pid = digits.trim().parse().ok().filter(|p: &i32| *p > 0)?;
    Some((pid, name.trim().to_string()))
}

/// GPU use of processes that are not in Tidegauge's process list, as one row
/// per process name.
#[derive(Default)]
pub struct Unlisted {
    prev: HashMap<i32, GpuClient>,
}

impl Unlisted {
    pub fn sample(
        &mut self,
        times: &HashMap<i32, GpuClient>,
        listed: &HashSet<i32>,
        seconds: f64,
    ) -> Vec<crate::processes::AppGroup> {
        let mut rows: HashMap<String, crate::processes::AppGroup> = HashMap::new();
        let mut next = HashMap::new();
        for (pid, now) in times {
            if listed.contains(pid) || now.name.is_empty() {
                continue;
            }
            // A first sight, or a different process on a reused pid, has no
            // interval to measure yet.
            let pct = match self.prev.get(pid) {
                Some(before) if before.name == now.name => gpu_pct(before.ns, now.ns, seconds),
                _ => 0.0,
            };
            next.insert(*pid, now.clone());
            let row = rows.entry(now.name.clone()).or_insert_with(|| crate::processes::AppGroup {
                key: format!("gpu:{}", now.name),
                name: now.name.clone(),
                is_system: true,
                ..Default::default()
            });
            row.pids.push(*pid);
            row.gpu_pct += pct;
        }
        self.prev = next;
        // Name order keeps tied rows from swapping places between readings.
        let mut rows: Vec<_> = rows.into_values().collect();
        for row in &mut rows {
            row.pids.sort_unstable();
            row.gpu_pct = row.gpu_pct.min(100.0);
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        rows
    }
}

/// Share of one GPU's time a process used between two readings, in percent.
/// A counter that went backwards (a new process on a reused pid) reads as 0.
pub fn gpu_pct(prev_ns: u64, now_ns: u64, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return 0.0;
    }
    (now_ns.saturating_sub(prev_ns) as f64 / seconds / 1e9 * 100.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_parts_reads_the_process_id_and_name() {
        assert_eq!(creator_parts("pid 443, WindowServer"), Some((443, "WindowServer".into())));
        assert_eq!(creator_parts("pid 72227, com.apple.Safari"), Some((72227, "com.apple.Safari".into())));
        assert_eq!(creator_parts("pid 12"), Some((12, String::new())));
        assert_eq!(creator_parts("pid 0, kernel_task"), None);
        assert_eq!(creator_parts("WindowServer"), None);
        assert_eq!(creator_parts(""), None);
    }

    fn client(ns: u64, name: &str) -> GpuClient {
        GpuClient { ns, name: name.into() }
    }

    #[test]
    fn unlisted_processes_get_rows_from_the_second_reading() {
        let mut unlisted = Unlisted::default();
        let listed: HashSet<i32> = [10].into();
        let first: HashMap<i32, GpuClient> =
            [(443, client(1_000_000_000, "WindowServer")), (10, client(5, "Safari"))].into();
        let rows = unlisted.sample(&first, &listed, 2.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "WindowServer");
        assert_eq!(rows[0].gpu_pct, 0.0);

        // 0.5 s of GPU time over 2 s is a quarter; the listed process stays out.
        let second: HashMap<i32, GpuClient> =
            [(443, client(1_500_000_000, "WindowServer")), (10, client(9, "Safari"))].into();
        let rows = unlisted.sample(&second, &listed, 2.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "gpu:WindowServer");
        assert_eq!(rows[0].pids, vec![443]);
        assert_eq!(rows[0].gpu_pct, 25.0);

        // A different process on the same pid starts over.
        let reused: HashMap<i32, GpuClient> = [(443, client(9_000_000_000, "other"))].into();
        assert_eq!(unlisted.sample(&reused, &listed, 2.0)[0].gpu_pct, 0.0);
    }

    #[test]
    fn gpu_pct_is_the_share_of_elapsed_time() {
        // 1.5 s of GPU time over 3 s is half the GPU.
        assert_eq!(gpu_pct(1_000_000_000, 2_500_000_000, 3.0), 50.0);
        assert_eq!(gpu_pct(5, 5, 2.0), 0.0);
        // A counter that went backwards is a new process, not negative use.
        assert_eq!(gpu_pct(9_000, 1_000, 2.0), 0.0);
        assert_eq!(gpu_pct(0, 1_000_000_000, 0.0), 0.0);
        assert_eq!(gpu_pct(0, 10_000_000_000, 1.0), 100.0);
    }

    /// Needs a physical Mac: virtual Macs expose no GPU clients.
    #[test]
    #[ignore]
    fn native_gpu_clients_report_time_per_process() {
        let times = gpu_time_by_pid();
        assert!(!times.is_empty(), "no GPU clients found");
        assert!(times.values().any(|c| c.ns > 0), "no process has GPU time");
        assert!(times.values().any(|c| c.name == "WindowServer"), "WindowServer has no GPU client");
    }
}
