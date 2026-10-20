//! Dev-server detection ("Projects"): finds processes listening on TCP ports,
//! groups them by the project folder of their working directory, and computes
//! an idle/busy status from recent CPU + network activity.

#![allow(dead_code)]

use crate::engine::{DevServer, ProjectGroup, ServerState};
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
const PROCESS_STATUS_ZOMBIE: u32 = 5;

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

/// pid -> listening TCP ports, read from the kernel through libproc. Unlike
/// `lsof` this never stats a file, so a wedged volume cannot block it. Only
/// the current user's processes can be inspected, which is what `lsof` showed
/// a non-root user too.
pub fn listening_ports() -> HashMap<i32, Vec<u16>> {
    let mut out: HashMap<i32, Vec<u16>> = HashMap::new();
    for pid in list_pids() {
        let ports = listening_ports_of(pid);
        if !ports.is_empty() {
            out.insert(pid, ports);
        }
    }
    out
}

/// The listening TCP ports of one process, ascending and without repeats.
pub fn listening_ports_of(pid: i32) -> Vec<u16> {
    use crate::ffi::*;
    use std::ffi::c_void;
    let mut ports = Vec::new();
    unsafe {
        let size = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0);
        if size <= 0 {
            return ports;
        }
        // Room for descriptors opened between the two calls.
        let count = size as usize / size_of::<proc_fdinfo>() + 16;
        let mut fds = vec![proc_fdinfo::default(); count];
        let got = proc_pidinfo(
            pid,
            PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr() as *mut c_void,
            (count * size_of::<proc_fdinfo>()) as i32,
        );
        if got <= 0 {
            return ports;
        }
        fds.truncate(got as usize / size_of::<proc_fdinfo>());
        let mut info = socket_fdinfo::zeroed();
        for fd in fds.iter().filter(|fd| fd.proc_fdtype == PROX_FDTYPE_SOCKET) {
            let read = proc_pidfdinfo(
                pid,
                fd.proc_fd,
                PROC_PIDFDSOCKETINFO,
                &mut info as *mut socket_fdinfo as *mut c_void,
                SOCKET_FDINFO_SIZE as i32,
            );
            if read as usize == SOCKET_FDINFO_SIZE {
                if let Some(port) = info.listening_tcp_port() {
                    ports.push(port);
                }
            }
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
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

pub fn select_server_state(cpu_pct: f64, idle_secs: u64, up_for: f64) -> ServerState {
    if cpu_pct > 15.0 {
        ServerState::Working
    } else if idle_secs > 3600 {
        ServerState::Idle { seconds: idle_secs }
    } else if up_for > 86400.0 && cpu_pct < 2.0 {
        ServerState::BarelyUsed {
            uptime_seconds: up_for.max(0.) as u64,
        }
    } else {
        ServerState::Idle {
            seconds: idle_secs.max(60),
        }
    }
}

/// The last listening-port scan, reused while it is younger than a caller's limit.
type Listeners = HashMap<i32, Vec<u16>>;
static LISTENERS: Mutex<Option<(Instant, Listeners)>> = Mutex::new(None);

/// `fetch`'s result, or the one kept in `cache` if it is younger than `max_age`.
fn cached_within<T: Clone>(
    cache: &mut Option<(Instant, T)>,
    max_age: Duration,
    fetch: impl FnOnce() -> T,
) -> T {
    if let Some((at, value)) = cache.as_ref() {
        if at.elapsed() < max_age {
            return value.clone();
        }
    }
    let value = fetch();
    *cache = Some((Instant::now(), value.clone()));
    value
}

/// Dev servers among `procs`. Listening ports may be up to `max_age` old.
pub fn detect_projects(procs: &[ProcInfo], max_age: Duration) -> Vec<ProjectGroup> {
    let listeners = cached_within(&mut LISTENERS.lock().unwrap(), max_age, listening_ports);
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
        let group_key = canonical_project(&group_key)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or(group_key);
        let recorded_cwd = canonical_project(&cwd)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| cwd.clone());
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
        let state = select_server_state(p.cpu_pct, idle_secs, up_for);
        for port in ports {
            let mut s = DevServer {
                pid: *pid,
                name: std::path::Path::new(&p.exe_path)
                    .file_name()
                    .map(|x| x.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.comm.clone()),
                exe_path: p.exe_path.clone(),
                cwd: recorded_cwd.clone(),
                runtime: runtime_of(&p.comm, &p.exe_path),
                port: *port,
                state: state.clone(),
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
            let idle = servers.iter().all(|s| s.state.is_idle());
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

fn refusal(english: &'static str, norwegian: &'static str) -> StopOutcome {
    StopOutcome::Refused(crate::locale::text(english, norwegian).into())
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
        && left.server.exe_path == right.server.exe_path
        && left.server.cwd == right.server.cwd
        && left.server.up_since == right.server.up_since
        && canonical_project(&left.project_dir)
            .zip(canonical_project(&right.project_dir))
            .is_some_and(|(left, right)| left == right)
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

fn validate_process_identity(
    target: &StopTarget,
    info: &crate::ffi::proc_bsdinfo,
    project: &std::path::Path,
) -> Result<(), StopOutcome> {
    if !matches_server(&target.server, info, unsafe { libc::geteuid() }) {
        return Err(refusal(
            "the process identity no longer matches.",
            "prosessidentiteten samsvarer ikke lenger.",
        ));
    }
    let path = proc_path(target.server.pid);
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy());
    if path.is_empty()
        || path != target.server.exe_path
        || name.as_deref() != Some(target.server.name.as_str())
    {
        return Err(refusal(
            "the executable identity no longer matches.",
            "den kjørbare identiteten samsvarer ikke lenger.",
        ));
    }
    let cwd = canonical_project(&proc_cwd(target.server.pid));
    let recorded_cwd = canonical_project(&target.server.cwd);
    let Some(cwd) = cwd else {
        return Err(refusal(
            "the process working directory is unavailable.",
            "arbeidsmappen til prosessen er ikke tilgjengelig.",
        ));
    };
    if recorded_cwd.as_ref() != Some(&cwd) {
        return Err(refusal(
            "the process working directory no longer matches.",
            "arbeidsmappen til prosessen samsvarer ikke lenger.",
        ));
    }
    let process = process_info(target.server.pid, info, path);
    let discovered_project =
        development_project(&process, &cwd.to_string_lossy(), unsafe { libc::geteuid() })
            .and_then(|root| canonical_project(&root));
    if discovered_project.as_deref() != Some(project) {
        return Err(refusal(
            "the process is not a recognized development server in the selected project.",
            "prosessen er ikke en gjenkjent utviklingsserver i det valgte prosjektet.",
        ));
    }
    Ok(())
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
    validate_process_identity(target, &info, &project)?;
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
    if final_info.pbi_start_tvsec != identity.pbi_start_tvsec
        || final_info.pbi_start_tvusec != identity.pbi_start_tvusec
    {
        return refused(
            target,
            "the process changed before the stop request.",
            "prosessen endret seg før stoppforespørselen.",
        );
    }
    let Some(expected_project) = canonical_project(&target.project_dir) else {
        return result_for(
            target,
            refusal(
                "the project folder is unavailable.",
                "prosjektmappen er ikke tilgjengelig.",
            ),
        );
    };
    if let Err(outcome) = validate_process_identity(target, &final_info, &expected_project) {
        return result_for(target, outcome);
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
        if let Some(info) = current_info.as_ref() {
            let changed_start = info.pbi_start_tvsec > 0
                && (info.pbi_start_tvsec != identity.pbi_start_tvsec
                    || info.pbi_start_tvusec != identity.pbi_start_tvusec);
            if info.pbi_pid == target.server.pid as u32
                && (info.pbi_status == PROCESS_STATUS_ZOMBIE || changed_start)
            {
                return result_for(target, StopOutcome::Exited);
            }
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
    let mut conflicts = HashSet::new();
    let mut results = Vec::new();
    for target in targets {
        if conflicts.contains(&target.server.pid) {
            continue;
        }
        if let Some(index) = unique
            .iter()
            .position(|existing| existing.server.pid == target.server.pid)
        {
            if same_identity(&unique[index], &target) {
                unique[index].ports.extend(target_ports(&target));
            } else {
                let first = unique.swap_remove(index);
                conflicts.insert(target.server.pid);
                results.push(refused(
                    &first,
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
    fn native_stop_rejects_non_development_identity_before_signalling() {
        let project = std::env::temp_dir().join(format!(
            "tidegauge-stop-nondevelopment-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            b"[package]\nname=\"stop-test\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .current_dir(&project)
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let info = proc_bsd(pid).unwrap();
        let path = proc_path(pid);
        let server = DevServer {
            pid,
            name: "sleep".into(),
            exe_path: path,
            cwd: canonical_project(&project.to_string_lossy())
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            up_since: info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0,
            ..Default::default()
        };
        let target = StopTarget {
            server,
            project_dir: project.to_string_lossy().into_owned(),
            ports: vec![],
        };
        let canonical_project = canonical_project(&target.project_dir).unwrap();
        assert!(matches!(
            validate_process_identity(&target, &info, &canonical_project),
            Err(StopOutcome::Refused(_))
        ));
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    #[ignore = "launches and terminates a disposable owned project listener"]
    fn native_stop_owned_project_listener_and_rejects_stale_identity() {
        let project =
            std::env::temp_dir().join(format!("tidegauge-stop-project-{}", std::process::id()));
        let nested_cwd = project.join("nested");
        std::fs::create_dir_all(&nested_cwd).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            b"[package]\nname=\"stop-test\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let mut child = std::process::Command::new("python3")
            .args([
                "-u",
                "-c",
                "import socket,time\ns=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); t=socket.socket(); t.bind(('127.0.0.1',0)); t.listen(); print(s.getsockname()[1],t.getsockname()[1],flush=True)\ntime.sleep(30)",
            ])
            .current_dir(&nested_cwd)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut port_text = String::new();
        use std::io::BufRead;
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut port_text)
            .unwrap();
        let mut ports = port_text
            .split_whitespace()
            .map(|port| port.parse().unwrap());
        let port: u16 = ports.next().unwrap();
        let second_port: u16 = ports.next().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let pid = child.id() as i32;
        let info = proc_bsd(pid).unwrap();
        let path = proc_path(pid);
        let recorded_cwd = canonical_project(&nested_cwd.to_string_lossy())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let server = DevServer {
            pid,
            name: std::path::Path::new(&path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into(),
            exe_path: path,
            cwd: recorded_cwd,
            port,
            up_since: info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1_000_000.0,
            ..Default::default()
        };
        let target = StopTarget {
            server: server.clone(),
            project_dir: project.to_string_lossy().into(),
            ports: vec![port],
        };
        assert!(stop_targets(Vec::new()).is_empty());
        assert!(child.try_wait().unwrap().is_none());

        let stale_start = StopTarget {
            server: DevServer {
                up_since: server.up_since + 1.0,
                ..server.clone()
            },
            ..target.clone()
        };
        let stale_executable = StopTarget {
            server: DevServer {
                exe_path: format!("/unrelated/{}", server.name),
                ..server.clone()
            },
            ..target.clone()
        };
        let stale_cwd = StopTarget {
            server: DevServer {
                cwd: project.to_string_lossy().into(),
                ..server.clone()
            },
            ..target.clone()
        };
        for stale in [stale_start, stale_executable, stale_cwd] {
            assert!(matches!(
                stop_targets(vec![stale])
                    .first()
                    .map(|result| &result.outcome),
                Some(StopOutcome::Refused(_))
            ));
            assert!(child.try_wait().unwrap().is_none());
        }

        let conflicting = StopTarget {
            server: DevServer {
                cwd: project.to_string_lossy().into(),
                ..server.clone()
            },
            ..target.clone()
        };
        let results = stop_targets(vec![target.clone(), conflicting]);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].outcome, StopOutcome::Refused(_)));
        assert!(child.try_wait().unwrap().is_none());

        let compatible = StopTarget {
            ports: vec![second_port],
            ..target.clone()
        };
        let results = stop_targets(vec![target, compatible]);
        assert_eq!(results.len(), 1);
        assert!(results[0].ports.contains(&port));
        assert!(results[0].ports.contains(&second_port));
        if !matches!(
            &results[0].outcome,
            StopOutcome::Exited | StopOutcome::StillRunningAfterSigterm
        ) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("unexpected stop outcome: {:?}", results[0].outcome);
        }
        use std::os::unix::process::ExitStatusExt;
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("the owned project listener did not exit after SIGTERM");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(status.signal(), Some(libc::SIGTERM));
        let _ = std::fs::remove_dir_all(project);
    }
}

/// Per-process network rates, in bytes per second, from the latest `nettop`
/// reading. The nettop worker writes them; sampling only looks them up, so a
/// slow `nettop` never holds up a sample.
static NET_RATES: Mutex<Option<HashMap<i32, (f64, f64)>>> = Mutex::new(None);

pub fn apply_net_rates(apps: &mut [AppGroup]) {
    let rates = NET_RATES.lock().unwrap();
    for app in apps.iter_mut() {
        let (mut rin, mut rout) = (0.0, 0.0);
        for (r_in, r_out) in app.pids.iter().filter_map(|pid| rates.as_ref()?.get(pid)) {
            rin += r_in;
            rout += r_out;
        }
        app.net_in_rate = rin;
        app.net_out_rate = rout;
    }
}

/// Whether counters read `elapsed` apart still give a rate for a probe that
/// runs every `period`. A longer gap (the probe was resting at a slower pace)
/// only sets a new baseline instead of averaging over minutes.
fn net_window_usable(elapsed: Duration, period: Duration) -> bool {
    elapsed <= period * 2 + Duration::from_secs(5)
}

/// Fold one `nettop` reading into the previous counters and the rates. A
/// process seen for the first time has no rate yet.
fn advance_net(
    previous: &mut HashMap<i32, (u64, u64, Instant)>,
    rates: &mut HashMap<i32, (f64, f64)>,
    totals: &HashMap<i32, (u64, u64)>,
    now: Instant,
    period: Duration,
) {
    for (pid, cur) in totals {
        if let Some(pv) = previous.get(pid) {
            let elapsed = now.duration_since(pv.2);
            if net_window_usable(elapsed, period) {
                let dt = elapsed.as_secs_f64().max(0.1);
                rates.insert(
                    *pid,
                    (cur.0.saturating_sub(pv.0) as f64 / dt, cur.1.saturating_sub(pv.1) as f64 / dt),
                );
            }
        }
        previous.insert(*pid, (cur.0, cur.1, now));
    }
    previous.retain(|pid, _| totals.contains_key(pid));
    rates.retain(|pid, _| totals.contains_key(pid));
}

static NETTOP: crate::bounded::Probe = crate::bounded::Probe::new();

/// Reads `nettop -x` and publishes per-process rates.
#[derive(Default)]
pub struct NetProbe {
    previous: HashMap<i32, (u64, u64, Instant)>,
    rates: HashMap<i32, (f64, f64)>,
}

impl NetProbe {
    pub fn poll(&mut self, period: Duration) {
        let mut command = std::process::Command::new("nettop");
        command.args(["-x", "-P", "-L", "1", "-J", "bytes_in,bytes_out"]);
        let Some(stdout) = NETTOP.run(command, Duration::from_secs(20)) else {
            return;
        };
        let now = Instant::now();
        let Some(totals) = parse_nettop(&String::from_utf8_lossy(&stdout)) else {
            return;
        };
        advance_net(&mut self.previous, &mut self.rates, &totals, now, period);
        *NET_RATES.lock().unwrap() = Some(self.rates.clone());
    }
}

/// Run `nettop` on its own thread at the pace the screen and alerts ask for.
pub fn start_nettop() {
    std::thread::Builder::new()
        .name("tidegauge-nettop".into())
        .spawn(move || {
            crate::demand::utility_thread();
            let mut probe = NetProbe::default();
            let mut last: Option<Instant> = None;
            loop {
                let seen = crate::demand::generation();
                let period = crate::demand::current().nettop_period();
                if last.is_none_or(|at| at.elapsed() + Duration::from_millis(500) >= period) {
                    probe.poll(period);
                    last = Some(Instant::now());
                }
                // Rest until the next run is due, or until the demand changes.
                let rest = last.map_or(Duration::ZERO, |at| {
                    period.saturating_sub(at.elapsed() + Duration::from_millis(500))
                });
                crate::demand::wait_for_change(seen, rest.max(Duration::from_secs(1)));
            }
        })
        .expect("nettop worker");
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
    fn server_state_preserves_priority_and_thresholds() {
        assert_eq!(
            select_server_state(15.1, 10_000, 200_000.),
            ServerState::Working
        );
        assert_eq!(select_server_state(16., 0, 0.), ServerState::Working);
        assert_eq!(
            select_server_state(15., 0, 0.),
            ServerState::Idle { seconds: 60 }
        );
        assert_eq!(
            select_server_state(1., 3601, 200_000.),
            ServerState::Idle { seconds: 3601 }
        );
        assert_eq!(
            select_server_state(1., 3600, 200_000.),
            ServerState::BarelyUsed {
                uptime_seconds: 200_000
            }
        );
        assert_eq!(
            select_server_state(1.9, 10, 86401.),
            ServerState::BarelyUsed {
                uptime_seconds: 86401
            }
        );
        assert_eq!(
            select_server_state(2.0, 10, 86401.),
            ServerState::Idle { seconds: 60 }
        );
        assert_eq!(
            select_server_state(1.9, 10, 86400.),
            ServerState::Idle { seconds: 60 }
        );
        assert_eq!(
            select_server_state(0., 12, 60.),
            ServerState::Idle { seconds: 60 }
        );
    }

    #[test]
    fn server_state_labels_and_idleness() {
        let english = |e: &'static str, _: &'static str| e;
        let norwegian = |_: &'static str, n: &'static str| n;
        let up = |secs: f64| format!("{secs}s");
        assert_eq!(ServerState::Working.label_with(english, up), "working");
        assert_eq!(
            ServerState::Idle { seconds: 120 }.label_with(english, up),
            "idle 120s"
        );
        assert_eq!(
            ServerState::BarelyUsed {
                uptime_seconds: 90_000
            }
            .label_with(english, up),
            "up 90000s, barely used"
        );
        assert_eq!(ServerState::Working.label_with(norwegian, up), "arbeider");
        assert_eq!(
            ServerState::Idle { seconds: 120 }.label_with(norwegian, up),
            "inaktiv i 120s"
        );
        assert_eq!(
            ServerState::BarelyUsed {
                uptime_seconds: 90_000
            }
            .label_with(norwegian, up),
            "oppe i 90000s, nesten ikke brukt"
        );
        assert!(!ServerState::Working.is_idle());
        assert!(ServerState::Idle { seconds: 0 }.is_idle());
        assert!(ServerState::BarelyUsed { uptime_seconds: 0 }.is_idle());
        assert_eq!(ServerState::default(), ServerState::Idle { seconds: 0 });
    }

    #[test]
    fn listening_ports_are_reused_only_while_young() {
        let mut cache = None;
        let mut runs = 0;
        let mut fetch = |cache: &mut Option<(Instant, u32)>, max_age| {
            cached_within(cache, max_age, || {
                runs += 1;
                runs
            })
        };
        assert_eq!(fetch(&mut cache, Duration::from_secs(30)), 1);
        assert_eq!(fetch(&mut cache, Duration::from_secs(30)), 1);
        // A caller that wants fresh data (zero age) always fetches.
        assert_eq!(fetch(&mut cache, Duration::ZERO), 2);
        assert_eq!(fetch(&mut cache, Duration::from_secs(30)), 2);
        cache = cache.map(|(at, v)| (at - Duration::from_secs(31), v));
        assert_eq!(fetch(&mut cache, Duration::from_secs(30)), 3);
    }

    #[test]
    fn net_rates_need_two_readings_a_period_apart() {
        let period = Duration::from_secs(4);
        let t0 = Instant::now();
        let (mut previous, mut rates) = (HashMap::new(), HashMap::new());
        advance_net(&mut previous, &mut rates, &HashMap::from([(7, (1000, 0))]), t0, period);
        assert!(rates.is_empty(), "the first reading has no rate");
        let t1 = t0 + Duration::from_secs(4);
        advance_net(&mut previous, &mut rates, &HashMap::from([(7, (5000, 800))]), t1, period);
        assert_eq!(rates[&7], (1000.0, 200.0));
        // A reading long after the last one only sets a new baseline.
        let t2 = t1 + Duration::from_secs(60);
        advance_net(&mut previous, &mut rates, &HashMap::from([(7, (9000, 800))]), t2, period);
        assert_eq!(rates[&7], (1000.0, 200.0));
        assert_eq!(previous[&7].0, 9000);
        // At the resting pace the same gap is a normal window.
        let t3 = t2 + Duration::from_secs(60);
        advance_net(&mut previous, &mut rates, &HashMap::from([(7, (9000 + 6000, 800))]), t3, Duration::from_secs(60));
        assert_eq!(rates[&7], (100.0, 0.0));
        // A process that is gone loses its entries.
        advance_net(&mut previous, &mut rates, &HashMap::new(), t3, period);
        assert!(rates.is_empty() && previous.is_empty());
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
    #[ignore = "reads this process's sockets through libproc on macOS"]
    fn native_libproc_finds_a_listening_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let pid = std::process::id() as i32;
        assert!(listening_ports_of(pid).contains(&port));
        assert!(listening_ports().get(&pid).is_some_and(|p| p.contains(&port)));
        drop(listener);
        assert!(!listening_ports_of(pid).contains(&port));
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
        let mut probe = NetProbe::default();
        probe.poll(Duration::from_secs(4));
        let reader = std::thread::spawn(move || {
            let mut bytes = vec![0u8; 512 * 1024];
            server.read_exact(&mut bytes).unwrap();
            server
        });
        client.write_all(&vec![1u8; 512 * 1024]).unwrap();
        let _server = reader.join().unwrap();
        probe.poll(Duration::from_secs(4));
        apply_net_rates(&mut apps);
        assert!(
            apps[0].net_in_rate > 0.0 || apps[0].net_out_rate > 0.0,
            "transferred bytes were not measured"
        );
    }
}
