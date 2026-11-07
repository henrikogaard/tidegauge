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

use std::collections::HashMap;
use std::ffi::CString;

use crate::ffi::*;
use crate::metrics::{cf_to_i64, cf_to_string, dict_get, entry_prop, matching_services};

/// Cumulative GPU nanoseconds per process id, summed over all of a process's
/// GPU clients. Empty when the GPU exposes no clients (virtual Macs).
pub fn gpu_time_by_pid() -> HashMap<i32, u64> {
    let mut out = HashMap::new();
    let plane = CString::new("IOService").unwrap();
    for gpu in matching_services("IOAccelerator") {
        let mut children: io_iterator_t = 0;
        if unsafe { IORegistryEntryGetChildIterator(gpu, plane.as_ptr(), &mut children) } == KERN_SUCCESS {
            let children = crate::metrics::IoIterator(children);
            for client in children {
                if let Some((pid, ns)) = client_usage(client) {
                    *out.entry(pid).or_insert(0) += ns;
                }
                unsafe { IOObjectRelease(client) };
            }
        }
        unsafe { IOObjectRelease(gpu) };
    }
    out
}

fn client_usage(client: io_registry_entry_t) -> Option<(i32, u64)> {
    let creator = entry_prop(client, "IOUserClientCreator");
    let pid = cf_to_string(creator).and_then(|s| creator_pid(&s));
    if !creator.is_null() {
        unsafe { CFRelease(creator) };
    }
    let pid = pid?;
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
    Some((pid, ns))
}

/// The process id in an `IOUserClientCreator` value such as "pid 443, WindowServer".
pub fn creator_pid(creator: &str) -> Option<i32> {
    let rest = creator.trim().strip_prefix("pid ")?;
    let digits = rest.split(',').next()?.trim();
    digits.parse().ok().filter(|p: &i32| *p > 0)
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
    fn creator_pid_reads_the_process_id() {
        assert_eq!(creator_pid("pid 443, WindowServer"), Some(443));
        assert_eq!(creator_pid("pid 72227, com.apple.Safari"), Some(72227));
        assert_eq!(creator_pid("pid 0, kernel_task"), None);
        assert_eq!(creator_pid("WindowServer"), None);
        assert_eq!(creator_pid(""), None);
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
        assert!(times.values().any(|ns| *ns > 0), "no process has GPU time");
    }
}
