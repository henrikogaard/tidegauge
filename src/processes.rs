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

/// Command-line arguments of a process you may inspect, used only to tell
/// programs apart (for example a coding agent from its background host).
/// Arguments can hold secrets, so callers must never display or store them.
/// `None` when macOS refuses (other users' processes) or the data is malformed.
pub fn proc_args(pid: i32) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) } != 0
        || size < 4
        || size > 1 << 20
    {
        return None;
    }
    let mut buf = vec![0u8; size];
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    buf.truncate(size);
    parse_procargs(&buf)
}

/// KERN_PROCARGS2 layout: argc (i32), the executable path, NUL padding, then
/// argc NUL-terminated arguments (followed by the environment, ignored).
fn parse_procargs(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    if !(0..=4096).contains(&argc) {
        return None;
    }
    let rest = buf.get(4..)?;
    let path_end = rest.iter().position(|&b| b == 0)?;
    let rest = &rest[path_end..];
    let args_start = rest.iter().position(|&b| b != 0)?;
    let args: Vec<String> = rest[args_start..]
        .split(|&b| b == 0)
        .take(argc as usize)
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    (args.len() == argc as usize).then_some(args)
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

/// Programs whose files don't name them well, grouped under one name.
fn known_tool(path: &str) -> Option<&'static str> {
    // Claude Code runs as `…/claude/versions/2.1.300` from the installer and
    // as `…/Claude/claude-code/<version>/…/claude.app` inside the desktop app.
    (path.contains("/claude/versions/") || path.contains("/Claude/claude-code/")).then_some("Claude Code")
}

/// A readable name for a program outside an app bundle.
fn tool_name(file_name: &str) -> String {
    // Reverse-DNS names (com.apple.Virtualization.VirtualMachine): keep the
    // last part and space out its words.
    let looks_reverse_dns = file_name.matches('.').count() >= 2
        && file_name.split('.').all(|part| part.chars().next().is_some_and(|c| c.is_ascii_alphabetic()));
    if !looks_reverse_dns {
        return file_name.to_string();
    }
    let last = file_name.rsplit('.').next().unwrap_or(file_name);
    let mut name = String::new();
    let chars: Vec<char> = last.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
        if i > 0 && c.is_uppercase() && (chars[i - 1].is_lowercase() || next_lower) {
            name.push(' ');
        }
        name.push(c);
    }
    name
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
        let key = if let Some(tool) = known_tool(&p.exe_path) {
            format!("proc:{tool}")
        } else if let Some(root) = bundle_root(&p.exe_path) {
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
            let n = tool_name(key.trim_start_matches("proc:"));
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
    fn claude_code_copies_group_under_one_name() {
        let proc = |pid, path: &str| super::ProcInfo { pid, ppid: 1, exe_path: path.into(), ..Default::default() };
        let groups = super::group_procs(&[
            proc(10, "/Users/x/.local/share/claude/versions/2.1.300"),
            proc(11, "/Users/x/Library/Application Support/Claude/claude-code/2.1.295/9d64/claude.app/Contents/MacOS/claude"),
            proc(12, "/System/Library/Frameworks/Virtualization.framework/Versions/A/XPCServices/com.apple.Virtualization.VirtualMachine.xpc/Contents/MacOS/com.apple.Virtualization.VirtualMachine"),
        ]);
        let names: Vec<_> = groups.iter().map(|g| (g.name.as_str(), g.pids.len())).collect();
        assert!(names.contains(&("Claude Code", 2)), "{names:?}");
        assert!(names.contains(&("Virtual Machine", 1)), "{names:?}");
    }

    #[test]
    fn plain_names_stay_as_they_are() {
        assert_eq!(super::tool_name("iconservicesagent"), "iconservicesagent");
        assert_eq!(super::tool_name("node"), "node");
        assert_eq!(super::tool_name("2.1.300"), "2.1.300");
        assert_eq!(super::tool_name("com.apple.WebKit.WebContent"), "Web Content");
    }

    #[test]
    fn procargs_layout_is_parsed_and_bounded() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/usr/bin/node\0\0\0node\0/opt/gemini\0HOME=/x\0");
        assert_eq!(super::parse_procargs(&buf), Some(vec!["node".into(), "/opt/gemini".into()]));
        let mut short = 3i32.to_ne_bytes().to_vec();
        short.extend_from_slice(b"/bin/x\0x\0");
        assert_eq!(super::parse_procargs(&short), None);
        assert_eq!(super::parse_procargs(&[1, 0]), None);
    }

    #[test]
    fn own_arguments_are_readable() {
        let args = super::proc_args(std::process::id() as i32).expect("own argv");
        assert!(!args.is_empty());
    }

    #[test]
    fn own_working_directory_is_not_truncated() {
        let actual = super::proc_cwd(std::process::id() as i32);
        let expected = std::env::current_dir().unwrap();
        assert_eq!(std::path::PathBuf::from(actual), expected);
    }
}
