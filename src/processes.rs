//! Process enumeration and grouping into apps:
//! every process is attributed to the app bundle it lives in, or to its
//! nearest app ancestor, so an app includes all of its helper processes.

#![allow(dead_code)]

use crate::ffi::*;
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};
use std::mem::size_of;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct ProcInfo {
    pub pid: i32,
    pub ppid: i32,
    pub uid: u32,
    pub comm: String,
    pub exe_path: String,
    pub start_unix: f64,
    // rusage cumulative counters
    pub cpu_ns: u64,
    pub mem_bytes: u64,
    pub disk_read: u64,
    pub disk_write: u64,
    pub energy_nj: u64,
    pub interrupt_wakeups: Option<u64>,
    // per-tick computed
    pub cpu_pct: f64,
    pub read_rate: f64,
    pub write_rate: f64,
    pub power_w: f64,
}

#[derive(Clone, Debug, Default)]
pub struct AppGroup {
    pub key: String,          // bundle path or pseudo key
    pub name: String,
    pub bundle_id: Option<String>,
    pub icon_path: Option<String>, // bundle dir for icon lookup
    pub pids: Vec<i32>,
    pub cpu_pct: f64,
    pub mem_bytes: u64,
    pub disk_read_rate: f64,
    pub disk_write_rate: f64,
    pub net_in_rate: f64,
    pub net_out_rate: f64,
    pub power_w: f64,
    pub is_system: bool,
}

pub struct ProcSnapshot {
    pub procs: Vec<ProcInfo>,
    pub apps: Vec<AppGroup>,
    pub total_count: usize,
}

pub fn list_pids() -> Vec<i32> {
    unsafe {
        let mut n = proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0);
        if n <= 0 {
            return Vec::new();
        }
        n = (n / size_of::<i32>() as i32) + 64;
        let mut buf = vec![0i32; n as usize];
        let got = proc_listpids(PROC_ALL_PIDS, 0, buf.as_mut_ptr() as *mut c_void, n * 4);
        buf.truncate((got / 4).max(0) as usize);
        buf.retain(|p| *p > 0);
        buf
    }
}

pub fn proc_bsd(pid: i32) -> Option<proc_bsdinfo> {
    unsafe {
        let mut b: proc_bsdinfo = Default::default();
        let r = proc_pidinfo(
            pid,
            PROC_PIDTBSDINFO,
            0,
            &mut b as *mut proc_bsdinfo as *mut c_void,
            size_of::<proc_bsdinfo>() as i32,
        );
        if r > 0 { Some(b) } else { None }
    }
}

pub fn proc_rusage(pid: i32) -> Option<rusage_info_v4> {
    unsafe {
        let mut r: rusage_info_v4 = Default::default();
        if proc_pid_rusage(pid, RUSAGE_INFO_V4, &mut r) >= 0 {
            Some(r)
        } else {
            None
        }
    }
}

pub fn proc_path(pid: i32) -> String {
    unsafe {
        let mut buf = [0i8; 4096];
        if proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) > 0 {
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().to_string()
        } else {
            String::new()
        }
    }
}

pub fn proc_cwd(pid: i32) -> String {
    unsafe {
        let mut info: proc_vnodepathinfo_full = std::mem::zeroed();
        let size = size_of::<proc_vnodepathinfo_full>();
        let r = proc_pidinfo(
            pid,
            PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut proc_vnodepathinfo_full as *mut c_void,
            size as i32,
        );
        if r != size as i32 {
            return String::new();
        }
        CStr::from_ptr(info.pvi_cdir.vi_path.as_ptr())
            .to_string_lossy()
            .to_string()
    }
}

fn comm_str(b: &proc_bsdinfo) -> String {
    unsafe { CStr::from_ptr(b.pbi_comm.as_ptr()).to_string_lossy().to_string() }
}

/// Extract the ".app" bundle root from an executable path.
/// /Applications/Foo.app/Contents/MacOS/foo -> /Applications/Foo.app
/// Also handles Foo.app/Contents/Frameworks/Bar Helper.app/… (nested helper app)
/// — we keep the OUTER .app so helpers roll up to the real app.
pub fn bundle_root(path: &str) -> Option<String> {
    let idx = path.find(".app/")?;
    Some(path[..idx + 4].to_string())
}

/// Bundle display name: folder stem, cleaned of common helper suffixes.
fn app_name(bundle_path: &str, fallback: &str) -> String {
    std::path::Path::new(bundle_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// Group processes into apps. Returns (app_key -> pids) by walking each
/// process's bundle path first, then its parent chain.
pub fn group_procs(procs: &[ProcInfo]) -> Vec<AppGroup> {
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    // pid -> app key
    let mut belong: HashMap<i32, String> = HashMap::new();
    let mut app_name_of: HashMap<String, String> = HashMap::new();
    let mut app_path_of: HashMap<String, String> = HashMap::new();

    let resolve = |pid: i32, belong: &HashMap<i32, String>, by_pid: &HashMap<i32, &ProcInfo>| -> Option<String> {
        let mut cur = pid;
        for _ in 0..64 {
            if let Some(k) = belong.get(&cur) {
                return Some(k.clone());
            }
            let p = by_pid.get(&cur)?;
            if let Some(root) = bundle_root(&p.exe_path) {
                return Some(root);
            }
            if p.ppid <= 1 {
                return None;
            }
            cur = p.ppid;
        }
        None
    };

    for p in procs {
        let key = if let Some(root) = bundle_root(&p.exe_path) {
            root
        } else if let Some(k) = resolve(p.pid, &belong, &by_pid) {
            k
        } else {
            // standalone daemon/tool: group by executable stem
            let n = std::path::Path::new(&p.exe_path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| p.comm.clone());
            format!("proc:{}", n)
        };
        belong.insert(p.pid, key.clone());
        if key.ends_with(".app") {
            app_path_of.insert(key.clone(), key.clone());
            app_name_of
                .entry(key.clone())
                .or_insert_with(|| app_name(&key, &p.comm));
        } else {
            let n = key.trim_start_matches("proc:").to_string();
            app_name_of.entry(key.clone()).or_insert(n);
        }
    }

    let mut groups: HashMap<String, AppGroup> = HashMap::new();
    for p in procs {
        let key = match belong.get(&p.pid) {
            Some(k) => k.clone(),
            None => continue,
        };
        let g = groups.entry(key.clone()).or_insert_with(|| AppGroup {
            key: key.clone(),
            name: app_name_of.get(&key).cloned().unwrap_or_else(|| key.clone()),
            icon_path: app_path_of.get(&key).cloned(),
            is_system: key.starts_with("proc:"),
            ..Default::default()
        });
        g.pids.push(p.pid);
        g.cpu_pct += p.cpu_pct;
        g.mem_bytes += p.mem_bytes;
        g.disk_read_rate += p.read_rate;
        g.disk_write_rate += p.write_rate;
        g.power_w += p.power_w;
    }
    let mut v: Vec<AppGroup> = groups.into_values().collect();
    v.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    v
}


#[cfg(test)]
mod tests {
    #[test]
    fn own_working_directory_is_not_truncated() {
        let actual = super::proc_cwd(std::process::id() as i32);
        let expected = std::env::current_dir().unwrap();
        assert_eq!(std::path::PathBuf::from(actual), expected);
    }
}
