//! Dev-server detection ("Projects"): finds processes listening on TCP ports,
//! groups them by the project folder of their working directory, and computes
//! an idle/busy status from recent CPU + network activity.

#![allow(dead_code)]

use crate::engine::{DevServer, ProjectGroup};
use crate::processes::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SERVER_NAMES: &[&str] = &[
    "node",
    "bun",
    "deno",
    "python",
    "python3",
    "ruby",
    "php",
    "java",
    "go",
    "cargo",
    "rustc",
    "vite",
    "next-server",
    "uvicorn",
    "gunicorn",
    "flask",
    "rails",
    "puma",
    "webpack-dev-server",
    "esbuild",
    "expo",
    "metro",
    "postgres",
    "postmaster",
    "redis-server",
    "mysqld",
    "mariadbd",
    "mongod",
    "docker",
    "com.docker",
    "nginx",
    "caddy",
    "http-server",
    "serve",
];

fn likely_server(comm: &str, exe: &str) -> bool {
    let executable = std::path::Path::new(exe)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    [comm, executable].iter().any(|name| {
        let name = name.to_lowercase();
        SERVER_NAMES.contains(&name.as_str())
            || name.strip_prefix("python").is_some_and(|version| {
                !version.is_empty() && version.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            })
    })
}

fn development_project(process: &ProcInfo, cwd: &str, current_uid: u32) -> Option<String> {
    // macOS framework Python runs its CLI through an interpreter bundle.
    let framework_python = process
        .exe_path
        .ends_with("/Resources/Python.app/Contents/MacOS/Python")
        && std::path::Path::new(&process.exe_path)
            .components()
            .any(|part| {
                let name = part.as_os_str().to_string_lossy();
                name.starts_with("Python") && name.ends_with(".framework")
            });
    if process.uid != current_uid || (bundle_root(&process.exe_path).is_some() && !framework_python)
    {
        return None;
    }
    let root = project_root(cwd)?;
    // A custom server executable built inside the project is also eligible.
    if likely_server(&process.comm, &process.exe_path)
        || std::path::Path::new(&process.exe_path).starts_with(&root)
    {
        Some(root)
    } else {
        None
    }
}

/// pid -> set of listening TCP ports, via `lsof` (batch, cached per tick).
pub fn listening_ports() -> HashMap<i32, Vec<u16>> {
    let mut out: HashMap<i32, Vec<u16>> = HashMap::new();
    if let Ok(o) = std::process::Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pLn"])
        .output()
    {
        let text = String::from_utf8_lossy(&o.stdout);
        let mut pid = 0i32;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix('p') {
                pid = rest.parse().unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix('n') {
                if pid > 0 {
                    if let Some(port_str) = rest.rsplit(':').next() {
                        if let Ok(port) = port_str.parse::<u16>() {
                            out.entry(pid).or_default().push(port);
                        }
                    }
                }
            }
        }
    }
    out
}

fn runtime_of(comm: &str, exe: &str) -> String {
    let n = format!("{} {}", comm.to_lowercase(), exe.to_lowercase());
    if n.contains("node") {
        "Node".into()
    } else if n.contains("bun") {
        "Bun".into()
    } else if n.contains("deno") {
        "Deno".into()
    } else if n.contains("python") {
        "Python".into()
    } else if n.contains("ruby") {
        "Ruby".into()
    } else if n.contains("php") {
        "PHP".into()
    } else if n.contains("java") {
        "Java".into()
    } else if n.contains("postgres") || n.contains("postmaster") {
        "PostgreSQL".into()
    } else if n.contains("redis") {
        "Redis".into()
    } else if n.contains("mysql") || n.contains("mariadb") {
        "MySQL".into()
    } else if n.contains("mongo") {
        "MongoDB".into()
    } else if n.contains("docker") {
        "Docker".into()
    } else if n.contains("nginx") {
        "Nginx".into()
    } else {
        comm.to_string()
    }
}

struct IdleTrack {
    since: Instant,
    cpu: f64,
}
static IDLE: OnceLock<Mutex<HashMap<i32, IdleTrack>>> = OnceLock::new();

pub fn detect_projects(procs: &[ProcInfo]) -> Vec<ProjectGroup> {
    let listeners = listening_ports();
    let idle_map = IDLE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut seen_servers: HashSet<i32> = HashSet::new();
    let mut dirs: HashMap<String, Vec<DevServer>> = HashMap::new();
    let current_uid = unsafe { libc::geteuid() };

    for (pid, ports) in &listeners {
        let Some(p) = procs.iter().find(|p| p.pid == *pid) else {
            continue;
        };
        let cwd = proc_cwd(*pid);
        let Some(group_key) = development_project(p, &cwd, current_uid) else {
            continue;
        };
        seen_servers.insert(*pid);
        let mut im = idle_map.lock().unwrap();
        let e = im.entry(*pid).or_insert(IdleTrack {
            since: Instant::now(),
            cpu: p.cpu_pct,
        });
        if p.cpu_pct > 8.0 {
            e.since = Instant::now();
            e.cpu = p.cpu_pct;
        }
        let idle_secs = e.since.elapsed().as_secs();
        drop(im);
        let up_for = crate::metrics::now_unix() - p.start_unix;
        let status = if p.cpu_pct > 15.0 {
            "working".into()
        } else if idle_secs > 3600 {
            format!("idle {}", crate::engine::fmt_up(idle_secs as f64))
        } else if up_for > 86400.0 && p.cpu_pct < 2.0 {
            format!("up {}, barely used", crate::engine::fmt_up(up_for))
        } else {
            format!("idle {}", crate::engine::fmt_up(idle_secs.max(60) as f64))
        };
        for port in ports {
            let mut s = DevServer {
                pid: *pid,
                name: std::path::Path::new(&p.exe_path)
                    .file_name()
                    .map(|x| x.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.comm.clone()),
                runtime: runtime_of(&p.comm, &p.exe_path),
                port: *port,
                status: status.clone(),
                mem_bytes: p.mem_bytes,
                cpu_pct: p.cpu_pct,
                up_since: p.start_unix,
            };
            if s.name.is_empty() {
                s.name = p.comm.clone();
            }
            dirs.entry(group_key.clone()).or_default().push(s);
        }
    }

    // prune stale idle records
    idle_map
        .lock()
        .unwrap()
        .retain(|pid, _| seen_servers.contains(pid));

    let mut groups: Vec<ProjectGroup> = dirs
        .into_iter()
        .map(|(dir, mut servers)| {
            servers.sort_by_key(|s| s.port);
            let name = std::path::Path::new(&dir)
                .file_name()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.clone());
            let idle = servers
                .iter()
                .all(|s| s.status.starts_with("idle") || s.status.contains("barely"));
            ProjectGroup {
                name,
                dir,
                servers,
                idle,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    groups
}

/// Require a project marker near the cwd; arbitrary listeners are not projects.
fn project_root(dir: &str) -> Option<String> {
    let mut d = std::path::PathBuf::from(dir);
    if !d.is_absolute() {
        return None;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    for _ in 0..4 {
        if d.parent().is_none() || home.as_deref() == Some(d.as_path()) {
            break;
        }
        if [
            ".git",
            "package.json",
            "Cargo.toml",
            "pyproject.toml",
            "go.mod",
            "Gemfile",
            "composer.json",
        ]
        .iter()
        .any(|marker| d.join(marker).exists())
        {
            return Some(d.to_string_lossy().to_string());
        }
        if !d.pop() {
            break;
        }
    }
    None
}

fn matches_server(server: &DevServer, info: &crate::ffi::proc_bsdinfo, uid: u32) -> bool {
    let started = info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0;
    server.pid > 1
        && info.pbi_pid == server.pid as u32
        && info.pbi_uid == uid
        && server.up_since.is_finite()
        && server.up_since > 0.0
        && started == server.up_since
}

#[derive(Clone)]
pub struct StopTarget {
    pub server: DevServer,
    pub project_dir: String,
    pub ports: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopOutcome {
    Exited,
    AlreadyStopped,
    Refused(String),
    Permission,
    Error(i32),
    StillRunningAfterSigterm,
}

pub struct StopResult {
    pub pid: i32,
    pub name: String,
    pub project_dir: String,
    pub ports: Vec<u16>,
    pub outcome: StopOutcome,
}

impl StopResult {
    pub fn message(&self) -> String {
        let name = if self.name.is_empty() {
            "server"
        } else {
            &self.name
        };
        match &self.outcome {
            StopOutcome::Exited => format!(
                "{} {} (PID {}) {}",
                crate::locale::text("Stopped", "Stoppet"),
                name,
                self.pid,
                crate::locale::text("and it exited.", "og prosessen avsluttet.")
            ),
            StopOutcome::AlreadyStopped => format!(
                "{} {} (PID {}) {}",
                crate::locale::text("Server", "Serveren"),
                name,
                self.pid,
                crate::locale::text("was already stopped.", "var allerede stoppet.")
            ),
            StopOutcome::Refused(reason) => format!(
                "{} {} (PID {}): {}",
                crate::locale::text("Refused to stop", "Nektet å stoppe"),
                name,
                self.pid,
                reason
            ),
            StopOutcome::Permission => format!(
                "{} {} (PID {}): {}",
                crate::locale::text("Could not stop", "Kunne ikke stoppe"),
                name,
                self.pid,
                crate::locale::text("permission was denied.", "tilgang ble nektet.")
            ),
            StopOutcome::Error(status) => format!(
                "{} {} (PID {}): {} ({status})",
                crate::locale::text("Could not stop", "Kunne ikke stoppe"),
                name,
                self.pid,
                crate::locale::text("system error", "systemfeil")
            ),
            StopOutcome::StillRunningAfterSigterm => format!(
                "{} {} (PID {}): {}",
                crate::locale::text("Termination requested for", "Avslutning bedt om for"),
                name,
                self.pid,
                crate::locale::text(
                    "it is still running after 2 seconds.",
                    "prosessen kjører fortsatt etter 2 sekunder.",
                )
            ),
        }
    }
}

fn result_for(target: &StopTarget, outcome: StopOutcome) -> StopResult {
    let mut ports = target.ports.clone();
    if target.server.port != 0 && !ports.contains(&target.server.port) {
        ports.push(target.server.port);
    }
    ports.sort_unstable();
    ports.dedup();
    StopResult {
        pid: target.server.pid,
        name: target.server.name.clone(),
        project_dir: target.project_dir.clone(),
        ports,
        outcome,
    }
}

fn refused(target: &StopTarget, english: &'static str, norwegian: &'static str) -> StopResult {
    result_for(
        target,
        StopOutcome::Refused(crate::locale::text(english, norwegian).into()),
    )
}

fn process_comm(info: &crate::ffi::proc_bsdinfo) -> String {
    unsafe {
        std::ffi::CStr::from_ptr(info.pbi_comm.as_ptr())
            .to_string_lossy()
            .to_string()
    }
}

fn process_info(pid: i32, info: &crate::ffi::proc_bsdinfo, path: String) -> ProcInfo {
    ProcInfo {
        pid,
        uid: info.pbi_uid,
        comm: process_comm(info),
        exe_path: path,
        start_unix: info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0,
        ..Default::default()
    }
}

fn target_ports(target: &StopTarget) -> Vec<u16> {
    let mut ports = target.ports.clone();
    if target.server.port != 0 {
        ports.push(target.server.port);
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn same_identity(left: &StopTarget, right: &StopTarget) -> bool {
    left.server.pid == right.server.pid
        && left.server.name == right.server.name
        && left.server.up_since == right.server.up_since
        && canonical_project(&left.project_dir) == canonical_project(&right.project_dir)
}

fn canonical_project(project: &str) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(project).ok()
}

fn kill_probe(pid: i32) -> Result<bool, StopOutcome> {
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EINVAL)
    {
        libc::ESRCH => Ok(false),
        libc::EPERM => Err(StopOutcome::Permission),
        error => Err(StopOutcome::Error(error)),
    }
}

fn validate_target(target: &StopTarget) -> Result<crate::ffi::proc_bsdinfo, StopOutcome> {
    if target.server.pid <= 1 {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "the process identifier is not safe.",
                "prosess-ID-en er ikke trygg.",
            )
            .into(),
        ));
    }
    let project = canonical_project(&target.project_dir).ok_or_else(|| {
        StopOutcome::Refused(
            crate::locale::text(
                "the project folder is unavailable.",
                "prosjektmappen er ikke tilgjengelig.",
            )
            .into(),
        )
    })?;
    let Some(info) = proc_bsd(target.server.pid) else {
        return Err(match kill_probe(target.server.pid) {
            Ok(false) => StopOutcome::AlreadyStopped,
            Ok(true) => StopOutcome::Refused(
                crate::locale::text(
                    "the process identity is inaccessible.",
                    "prosessidentiteten er utilgjengelig.",
                )
                .into(),
            ),
            Err(outcome) => outcome,
        });
    };
    if !matches_server(&target.server, &info, unsafe { libc::geteuid() }) {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "the process identity no longer matches.",
                "prosessidentiteten samsvarer ikke lenger.",
            )
            .into(),
        ));
    }
    let path = proc_path(target.server.pid);
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy());
    if path.is_empty() || name.as_deref() != Some(target.server.name.as_str()) {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "the executable identity no longer matches.",
                "den kjørbare identiteten samsvarer ikke lenger.",
            )
            .into(),
        ));
    }
    let cwd = canonical_project(&proc_cwd(target.server.pid));
    let Some(cwd) = cwd else {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "the process working directory is unavailable.",
                "arbeidsmappen til prosessen er utilgjengelig.",
            )
            .into(),
        ));
    };
    if cwd != project {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "the process is no longer in the selected project.",
                "prosessen er ikke lenger i det valgte prosjektet.",
            )
            .into(),
        ));
    }
    let process = process_info(target.server.pid, &info, path);
    let expected_project = project.to_string_lossy().to_string();
    if development_project(&process, &cwd.to_string_lossy(), unsafe { libc::geteuid() }).as_deref()
        != Some(expected_project.as_str())
    {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "the process is not a recognized development server.",
                "prosessen er ikke en gjenkjent utviklingsserver.",
            )
            .into(),
        ));
    }
    let ports = listening_ports();
    if !target_ports(target).iter().any(|port| {
        ports
            .get(&target.server.pid)
            .is_some_and(|open| open.contains(port))
    }) {
        return Err(StopOutcome::Refused(
            crate::locale::text(
                "none of the originally confirmed ports is still listening.",
                "ingen av de opprinnelig bekreftede portene lytter fortsatt.",
            )
            .into(),
        ));
    }
    Ok(info)
}

fn stop_one(target: &StopTarget) -> StopResult {
    let identity = match validate_target(target) {
        Ok(info) => info,
        Err(outcome) => return result_for(target, outcome),
    };
    // Re-read the identity immediately before signalling, after all probes.
    let Some(final_info) = proc_bsd(target.server.pid) else {
        let outcome = match kill_probe(target.server.pid) {
            Ok(false) => StopOutcome::AlreadyStopped,
            Ok(true) => StopOutcome::Refused(
                crate::locale::text(
                    "the process identity became inaccessible.",
                    "prosessidentiteten ble utilgjengelig.",
                )
                .into(),
            ),
            Err(outcome) => outcome,
        };
        return result_for(target, outcome);
    };
    if !matches_server(&target.server, &final_info, unsafe { libc::geteuid() })
        || final_info.pbi_start_tvsec != identity.pbi_start_tvsec
        || final_info.pbi_start_tvusec != identity.pbi_start_tvusec
    {
        return refused(
            target,
            "the process changed before the stop request.",
            "prosessen endret seg før stoppforespørselen.",
        );
    }
    let final_path = proc_path(target.server.pid);
    let final_cwd = canonical_project(&proc_cwd(target.server.pid));
    let expected_project = canonical_project(&target.project_dir);
    let final_name = std::path::Path::new(&final_path)
        .file_name()
        .map(|name| name.to_string_lossy());
    if final_path.is_empty()
        || final_name.as_deref() != Some(target.server.name.as_str())
        || final_cwd != expected_project
    {
        return refused(
            target,
            "the executable or project identity changed before the stop request.",
            "den kjørbare filen eller prosjektidentiteten endret seg før stoppforespørselen.",
        );
    }
    let final_process = process_info(target.server.pid, &final_info, final_path);
    let Some(final_project) = final_cwd.as_ref() else {
        return refused(
            target,
            "the project identity became unavailable before the stop request.",
            "prosjektidentiteten ble utilgjengelig før stoppforespørselen.",
        );
    };
    let final_project_string = final_project.to_string_lossy().to_string();
    if development_project(&final_process, &final_project_string, unsafe {
        libc::geteuid()
    })
    .as_deref()
        != Some(final_project_string.as_str())
    {
        return refused(
            target,
            "the development-server identity changed before the stop request.",
            "utviklingsserverens identitet endret seg før stoppforespørselen.",
        );
    }
    if unsafe { libc::kill(target.server.pid, libc::SIGTERM) } != 0 {
        let outcome = match std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EINVAL)
        {
            libc::ESRCH => StopOutcome::AlreadyStopped,
            libc::EPERM => StopOutcome::Permission,
            error => StopOutcome::Error(error),
        };
        return result_for(target, outcome);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let current_info = proc_bsd(target.server.pid);
        if current_info
            .as_ref()
            .is_some_and(|info| info.pbi_status == 5)
        {
            return result_for(target, StopOutcome::Exited);
        }
        if current_info.is_none() && proc_path(target.server.pid).is_empty() {
            return result_for(target, StopOutcome::Exited);
        }
        match kill_probe(target.server.pid) {
            Ok(false) => return result_for(target, StopOutcome::Exited),
            Err(outcome) => return result_for(target, outcome),
            Ok(true) => {}
        }
        if Instant::now() >= deadline {
            return result_for(target, StopOutcome::StillRunningAfterSigterm);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn stop_targets(targets: Vec<StopTarget>) -> Vec<StopResult> {
    let mut unique: Vec<StopTarget> = Vec::new();
    let mut results = Vec::new();
    for target in targets {
        if let Some(index) = unique
            .iter()
            .position(|existing| existing.server.pid == target.server.pid)
        {
            let existing = &mut unique[index];
            if same_identity(existing, &target) {
                existing.ports.extend(target_ports(&target));
            } else {
                results.push(refused(
                    &target,
                    "conflicting stop requests for this PID were refused.",
                    "motstridende stoppforespørsler for denne PID-en ble avvist.",
                ));
            }
        } else {
            unique.push(target);
        }
    }
    for target in unique {
        results.push(stop_one(&target));
    }
    results
}

pub fn stop_server(server: &DevServer) -> bool {
    let target = StopTarget {
        server: server.clone(),
        project_dir: String::new(),
        ports: vec![server.port],
    };
    matches!(
        stop_targets(vec![target])
            .first()
            .map(|result| &result.outcome),
        Some(StopOutcome::Exited)
    )
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    #[test]
    fn stop_requires_same_owner_pid_and_start_time() {
        let pid = std::process::id() as i32;
        let info = proc_bsd(pid).unwrap();
        let uid = unsafe { libc::getuid() };
        let mut server = DevServer {
            pid,
            up_since: info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0,
            ..Default::default()
        };
        assert!(matches_server(&server, &info, uid));
        assert!(!matches_server(&server, &info, uid.wrapping_add(1)));
        server.up_since += 1.0;
        assert!(!matches_server(&server, &info, uid));
        server.up_since = f64::NAN;
        assert!(!matches_server(&server, &info, uid));
        server.pid = 0;
        assert!(!matches_server(&server, &info, uid));
    }

    #[test]
    #[ignore = "launches and terminates a disposable owned process"]
    fn native_stop_rejects_non_development_listener_before_signalling() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let info = proc_bsd(pid).unwrap();
        let started = info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0;
        let server = DevServer {
            pid,
            up_since: started + 1.0,
            ..Default::default()
        };
        assert!(!stop_server(&server));
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        let _ = child.wait();
    }

    #[test]
    #[ignore = "launches and terminates a disposable owned project listener"]
    fn native_stop_owned_project_listener_and_rejects_stale_identity() {
        let project =
            std::env::temp_dir().join(format!("tidegauge-stop-project-{}", std::process::id()));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            b"[package]\nname=\"stop-test\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let mut child = std::process::Command::new("python3")
            .args([
                "-u",
                "-c",
                "import socket,time\ns=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); print(s.getsockname()[1],flush=True)\ntime.sleep(30)",
            ])
            .current_dir(&project)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut port_text = String::new();
        use std::io::BufRead;
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut port_text)
            .unwrap();
        let port: u16 = port_text.trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let pid = child.id() as i32;
        let info = proc_bsd(pid).unwrap();
        let path = proc_path(pid);
        let server = DevServer {
            pid,
            name: std::path::Path::new(&path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into(),
            port,
            up_since: info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0,
            ..Default::default()
        };
        let stale = StopTarget {
            server: DevServer {
                up_since: server.up_since + 1.0,
                ..server.clone()
            },
            project_dir: project.to_string_lossy().into(),
            ports: vec![port],
        };
        assert!(matches!(
            stop_targets(vec![stale])
                .first()
                .map(|result| &result.outcome),
            Some(StopOutcome::Refused(_))
        ));
        assert!(child.try_wait().unwrap().is_none());
        let target = StopTarget {
            server,
            project_dir: project.to_string_lossy().into(),
            ports: vec![port],
        };
        assert_eq!(stop_targets(vec![target])[0].outcome, StopOutcome::Exited);
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(project);
    }
}

/// Per-process network bytes via `nettop -x` deltas applied to app groups.
static NET_PREV: OnceLock<Mutex<HashMap<i32, (u64, u64, Instant)>>> = OnceLock::new();

pub fn apply_nettop(apps: &mut [AppGroup]) {
    let map = NET_PREV.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(out) = std::process::Command::new("nettop")
        .args(["-x", "-P", "-L", "1", "-J", "bytes_in,bytes_out"])
        .output()
    else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let now = Instant::now();
    let Some(totals) = parse_nettop(&text) else {
        return;
    };
    let mut prev = map.lock().unwrap();
    for app in apps.iter_mut() {
        let mut rin = 0.0;
        let mut rout = 0.0;
        for pid in &app.pids {
            if let (Some(cur), Some(pv)) = (totals.get(pid), prev.get(pid)) {
                let dt = now.duration_since(pv.2).as_secs_f64().max(0.1);
                rin += cur.0.saturating_sub(pv.0) as f64 / dt;
                rout += cur.1.saturating_sub(pv.1) as f64 / dt;
            }
            if let Some(cur) = totals.get(pid) {
                prev.insert(*pid, (cur.0, cur.1, now));
            }
        }
        app.net_in_rate = rin;
        app.net_out_rate = rout;
    }
    prev.retain(|pid, _| totals.contains_key(pid));
}

fn parse_nettop(text: &str) -> Option<HashMap<i32, (u64, u64)>> {
    let mut lines = text.lines();
    let header = csv_fields(lines.next()?)?;
    let process_col = header.iter().position(|h| h.is_empty() || h == "process")?;
    let in_col = header.iter().position(|h| h == "bytes_in")?;
    let out_col = header.iter().position(|h| h == "bytes_out")?;
    let mut totals = HashMap::new();
    for line in lines {
        let Some(fields) = csv_fields(line) else {
            continue;
        };
        let Some(pid) = fields
            .get(process_col)
            .and_then(|label| label.rsplit('.').next())
            .and_then(|pid| pid.parse::<i32>().ok())
            .filter(|pid| *pid > 0)
        else {
            continue;
        };
        let Some(bytes_in) = fields.get(in_col).and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        let Some(bytes_out) = fields.get(out_col).and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        totals.insert(pid, (bytes_in, bytes_out));
    }
    Some(totals)
}

// Process labels can contain commas and escaped quotes in nettop's CSV output.
fn csv_fields(line: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(ch),
        }
    }
    if quoted {
        return None;
    }
    fields.push(field);
    Some(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_process_names_do_not_match_short_runtime_substrings() {
        assert!(!likely_server(
            "LogoSync",
            "/Applications/Example.app/Contents/MacOS/LogoSync"
        ));
        assert!(!likely_server(
            "JavaScriptEditor",
            "/Users/example/Applications/Editor"
        ));
        assert!(likely_server("node", "/opt/bin/node"));
        assert!(likely_server("python3.12", "/opt/bin/python3.12"));
    }

    #[test]
    fn projects_require_markers_and_exclude_desktop_bundles_and_other_users() {
        let temporary =
            std::env::temp_dir().join(format!("tidegauge-project-test-{}", std::process::id()));
        std::fs::create_dir_all(temporary.join("workspace/src")).unwrap();
        let root = temporary.join("workspace");
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let cwd = root.join("src").to_string_lossy().into_owned();
        let mut process = ProcInfo {
            uid: 501,
            comm: "node".into(),
            exe_path: "/opt/bin/node".into(),
            ..Default::default()
        };
        assert_eq!(
            development_project(&process, &cwd, 501),
            Some(root.to_string_lossy().into_owned())
        );
        assert_eq!(
            development_project(&process, temporary.to_str().unwrap(), 501),
            None
        );
        assert_eq!(development_project(&process, "/", 501), None);
        assert_eq!(development_project(&process, "", 501), None);
        assert_eq!(development_project(&process, "~", 501), None);
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                development_project(&process, &home.to_string_lossy(), 501),
                None
            );
        }
        assert_eq!(development_project(&process, &cwd, 502), None);
        process.exe_path = "/Users/example/Applications/Example.app/Contents/MacOS/node".into();
        assert_eq!(development_project(&process, &cwd, 501), None);
        process.comm = "Python".into();
        process.exe_path =
            "/opt/lib/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python"
                .into();
        assert!(development_project(&process, &cwd, 501).is_some());
        process.exe_path = "/Applications/Python.app/Contents/MacOS/Python".into();
        assert_eq!(development_project(&process, &cwd, 501), None);
        process.comm = "LogoSync".into();
        process.exe_path = "/Users/example/bin/LogoSync".into();
        assert_eq!(development_project(&process, &cwd, 501), None);
        process.exe_path = root
            .join("target/debug/custom-server")
            .to_string_lossy()
            .into_owned();
        assert!(development_project(&process, &cwd, 501).is_some());
        process.exe_path = format!("{}-other/server", root.display());
        assert_eq!(development_project(&process, &cwd, 501), None);
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn csv_parser_accepts_native_header_and_trailing_empty_column() {
        let csv = ",bytes_in,bytes_out,\nworker.42,4096,1024,\n";
        assert_eq!(parse_nettop(csv).unwrap().get(&42), Some(&(4096, 1024)));
        assert!(parse_nettop("plain text output").is_none());
    }

    #[test]
    fn csv_parser_uses_header_columns_and_quoted_process_labels() {
        let csv = "time,process,bytes_out,bytes_in,\n10:20:30,\"worker, helper.42\",1024,4096,\n10:20:30,server.43,256,512,\n";
        let totals = parse_nettop(csv).unwrap();
        assert_eq!(totals.get(&42), Some(&(4096, 1024)));
        assert_eq!(totals.get(&43), Some(&(512, 256)));
    }

    #[test]
    fn csv_parser_skips_malformed_counters_without_inventing_zeroes() {
        let csv = ",bytes_in,bytes_out,\nworker.42,invalid,1024,\nworker.43,512,256,\n";
        let totals = parse_nettop(csv).unwrap();
        assert!(!totals.contains_key(&42));
        assert_eq!(totals.get(&43), Some(&(512, 256)));
    }

    #[test]
    #[ignore = "runs nettop against a disposable loopback connection on macOS"]
    fn native_network_rates_report_transferred_bytes() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let mut apps = vec![AppGroup {
            key: "test-network".into(),
            pids: vec![std::process::id() as i32],
            ..Default::default()
        }];
        apply_nettop(&mut apps);
        let reader = std::thread::spawn(move || {
            let mut bytes = vec![0u8; 512 * 1024];
            server.read_exact(&mut bytes).unwrap();
            server
        });
        client.write_all(&vec![1u8; 512 * 1024]).unwrap();
        let _server = reader.join().unwrap();
        apply_nettop(&mut apps);
        assert!(
            apps[0].net_in_rate > 0.0 || apps[0].net_out_rate > 0.0,
            "transferred bytes were not measured"
        );
    }
}
