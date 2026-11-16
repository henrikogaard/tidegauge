//! Ports (design: docs/design/ports-and-leftovers.md): every listening TCP
//! port of the current user's processes, with its project folder, its
//! launcher and whether other devices can reach it.

pub mod cli;
pub mod launcher;

use crate::agents::Session;
use crate::locale::text as l;
use crate::processes::{bundle_root, proc_bsd, proc_cwd, proc_path, ProcInfo};
pub use launcher::Launcher;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

/// Where a listener accepts connections from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bind {
    /// This Mac only (127.0.0.1, ::1).
    Loopback(IpAddr),
    /// Every address (0.0.0.0, ::): reachable from the network.
    Any(IpAddr),
    /// One address of this Mac, such as its Wi-Fi address.
    Addr(IpAddr),
}

impl Bind {
    pub fn of(addr: IpAddr) -> Self {
        if addr.is_unspecified() {
            Bind::Any(addr)
        } else if addr.is_loopback() {
            Bind::Loopback(addr)
        } else {
            Bind::Addr(addr)
        }
    }

    pub fn exposed(self) -> bool {
        !matches!(self, Bind::Loopback(_))
    }

    pub fn addr(self) -> IpAddr {
        match self {
            Bind::Loopback(a) | Bind::Any(a) | Bind::Addr(a) => a,
        }
    }

    /// `*` for every address, otherwise the address.
    pub fn text(self) -> String {
        match self {
            Bind::Any(_) => "*".into(),
            other => other.addr().to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    pub binds: Vec<Bind>,
    pub process: ProcInfo,
    pub name: String,
    /// The project folder, when the process runs inside one.
    pub project: Option<String>,
    /// A recognised development server in `project` (what Stop may end
    /// through the dev-server path).
    pub dev_server: bool,
    pub launcher: Launcher,
}

impl Listener {
    pub fn exposed(&self) -> bool {
        self.binds.iter().any(|b| b.exposed())
    }

    /// A dev server (or something an agent started) open to the network:
    /// the case the warning is for (FR-8). Apps and system services that
    /// listen on all addresses on purpose are left out.
    pub fn warn_exposed(&self) -> bool {
        self.exposed() && (self.dev_server || self.launcher.is_agent())
    }

    pub fn binds_text(&self) -> String {
        self.binds.iter().map(|b| b.text()).collect::<Vec<_>>().join(", ")
    }
}

/// What the exposed warning says (FR-8).
pub fn exposed_warning() -> &'static str {
    l(
        "Open to your network: other devices on this Wi-Fi or LAN can connect to it, not just this Mac. That is fine at home, risky on shared or public Wi-Fi. To keep it on this Mac, start it bound to 127.0.0.1 (for example --host 127.0.0.1).",
        "Åpen mot nettverket: andre enheter på dette Wi-Fi-et eller lokalnettet kan koble til, ikke bare denne Macen. Det går fint hjemme, men er risikabelt på delt eller offentlig Wi-Fi. For å holde den på denne Macen, start den bundet til 127.0.0.1 (for eksempel --host 127.0.0.1).",
    )
}

fn display_name(p: &ProcInfo) -> String {
    std::path::Path::new(&p.exe_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| p.comm.clone())
}

/// Build listeners from sockets and processes. Pure apart from `cwd`.
pub fn build(
    sockets: &crate::projects::Sockets,
    procs: &[ProcInfo],
    cx: &launcher::Context,
    cwd: &dyn Fn(i32) -> String,
) -> Vec<Listener> {
    let uid = unsafe { libc::geteuid() };
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut out = Vec::new();
    for (pid, list) in sockets {
        let Some(p) = by_pid.get(pid) else { continue };
        let dir = cwd(*pid);
        let dev = crate::projects::development_project(p, &dir, uid);
        let project = dev.clone().or_else(|| {
            bundle_root(&p.exe_path).is_none().then(|| crate::projects::project_root(&dir)).flatten()
        });
        let launcher = launcher::launcher(p, &by_pid, cx);
        let mut by_port: HashMap<u16, Vec<Bind>> = HashMap::new();
        for (port, addr) in list {
            by_port.entry(*port).or_default().push(Bind::of(*addr));
        }
        for (port, mut binds) in by_port {
            binds.sort();
            binds.dedup();
            out.push(Listener {
                port,
                binds,
                process: (*p).clone(),
                name: display_name(p),
                project: project.clone(),
                dev_server: dev.is_some(),
                launcher: launcher.clone(),
            });
        }
    }
    out.sort_by_key(|l| (l.port, l.process.pid));
    out
}

/// A process list light enough for the command line: no resource counters.
pub fn basic_procs() -> Vec<ProcInfo> {
    crate::processes::list_pids()
        .into_iter()
        .filter_map(|pid| {
            let b = proc_bsd(pid)?;
            let comm = unsafe { std::ffi::CStr::from_ptr(b.pbi_comm.as_ptr()).to_string_lossy().into_owned() };
            Some(ProcInfo {
                pid,
                ppid: b.pbi_ppid as i32,
                uid: b.pbi_uid,
                comm,
                exe_path: proc_path(pid),
                start_unix: b.pbi_start_tvsec as f64 + b.pbi_start_tvusec as f64 / 1e6,
                ..Default::default()
            })
        })
        .collect()
}

/// Every listener now, with sockets at most `max_age` old.
pub fn listeners(procs: &[ProcInfo], sessions: &[Session], max_age: Duration) -> Vec<Listener> {
    let sockets = crate::projects::cached_sockets(max_age);
    let mut cx = launcher::Context::from_sessions(sessions);
    // launchctl is only worth running when something hangs off launchd.
    let orphan = sockets.keys().any(|pid| procs.iter().any(|p| p.pid == *pid && p.ppid == 1));
    if orphan {
        cx.launchd = launcher::launchd_jobs();
    }
    build(&sockets, procs, &cx, &proc_cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn binds_classify_loopback_any_and_lan() {
        assert!(!Bind::of(ip("127.0.0.1")).exposed());
        assert!(!Bind::of(ip("::1")).exposed());
        assert!(Bind::of(ip("0.0.0.0")).exposed());
        assert!(Bind::of(ip("::")).exposed());
        assert!(Bind::of(ip("192.168.1.20")).exposed());
        assert_eq!(Bind::of(ip("::")).text(), "*");
        assert_eq!(Bind::of(ip("127.0.0.1")).text(), "127.0.0.1");
    }

    #[test]
    fn listeners_merge_families_and_flag_only_dev_servers() {
        let dir = crate::cleanup::test_support::temp("ports-build");
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        let uid = unsafe { libc::geteuid() };
        let node = ProcInfo { pid: 500, ppid: 1, uid, comm: "node".into(), exe_path: "/opt/homebrew/bin/node".into(), start_unix: 1.0, ..Default::default() };
        let app = ProcInfo { pid: 600, ppid: 1, uid, comm: "ControlCenter".into(), exe_path: "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter".into(), start_unix: 1.0, ..Default::default() };
        let mut sockets = crate::projects::Sockets::new();
        sockets.insert(500, vec![(5173, ip("0.0.0.0")), (5173, ip("::"))]);
        sockets.insert(600, vec![(7000, ip("::"))]);
        let d = dir.to_string_lossy().into_owned();
        let cwd = move |pid: i32| if pid == 500 { d.clone() } else { "/".into() };
        let got = build(&sockets, &[node, app], &launcher::Context::default(), &cwd);
        assert_eq!(got.len(), 2);
        let vite = &got[0];
        assert_eq!((vite.port, vite.binds.len()), (5173, 2));
        assert!(vite.dev_server && vite.warn_exposed());
        assert_eq!(vite.project.as_deref(), Some(dir.to_string_lossy().as_ref()));
        let cc = &got[1];
        assert!(cc.exposed() && !cc.warn_exposed() && cc.project.is_none());
        assert_eq!(cc.launcher, Launcher::App("ControlCenter".into()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
