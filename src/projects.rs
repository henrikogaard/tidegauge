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
    "node", "bun", "deno", "python", "python3", "ruby", "php", "java", "go",
    "cargo", "rustc", "vite", "next-server", "uvicorn", "gunicorn", "flask",
    "rails", "puma", "webpack-dev-server", "esbuild", "expo", "metro",
    "postgres", "postmaster", "redis-server", "mysqld", "mariadbd", "mongod",
    "docker", "com.docker", "nginx", "caddy", "http-server", "serve",
];

fn likely_server(comm: &str, exe: &str) -> bool {
    let n = comm.to_lowercase();
    if SERVER_NAMES.iter().any(|s| n.contains(s)) {
        return true;
    }
    let e = exe.to_lowercase();
    e.contains("node_modules") || e.contains(".bun") || e.contains("/bin/vite")
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
    if n.contains("node") { "Node".into() }
    else if n.contains("bun") { "Bun".into() }
    else if n.contains("deno") { "Deno".into() }
    else if n.contains("python") { "Python".into() }
    else if n.contains("ruby") { "Ruby".into() }
    else if n.contains("php") { "PHP".into() }
    else if n.contains("java") { "Java".into() }
    else if n.contains("postgres") || n.contains("postmaster") { "PostgreSQL".into() }
    else if n.contains("redis") { "Redis".into() }
    else if n.contains("mysql") || n.contains("mariadb") { "MySQL".into() }
    else if n.contains("mongo") { "MongoDB".into() }
    else if n.contains("docker") { "Docker".into() }
    else if n.contains("nginx") { "Nginx".into() }
    else { comm.to_string() }
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

    for (pid, ports) in &listeners {
        let Some(p) = procs.iter().find(|p| p.pid == *pid) else { continue };
        // keep only user-space dev processes (skip system daemons by uid/path)
        if !likely_server(&p.comm, &p.exe_path) && !p.exe_path.contains("/Users/") {
            continue;
        }
        seen_servers.insert(*pid);
        let cwd = proc_cwd(*pid);
        let dir = if cwd.is_empty() { "~".into() } else { cwd };
        let group_key = project_root(&dir);
        let mut im = idle_map.lock().unwrap();
        let e = im.entry(*pid).or_insert(IdleTrack { since: Instant::now(), cpu: p.cpu_pct });
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
    idle_map.lock().unwrap().retain(|pid, _| seen_servers.contains(pid));

    let mut groups: Vec<ProjectGroup> = dirs
        .into_iter()
        .map(|(dir, mut servers)| {
            servers.sort_by_key(|s| s.port);
            let name = std::path::Path::new(&dir)
                .file_name()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.clone());
            let idle = servers.iter().all(|s| s.status.starts_with("idle") || s.status.contains("barely"));
            ProjectGroup { name, dir, servers, idle }
        })
        .collect();
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    groups
}

/// Collapse a cwd to its project root (nearest dir containing .git or
/// package.json within 4 levels), else the dir itself.
fn project_root(dir: &str) -> String {
    let mut d = std::path::PathBuf::from(dir);
    for _ in 0..4 {
        if d.join(".git").exists() || d.join("package.json").exists() {
            return d.to_string_lossy().to_string();
        }
        if !d.pop() {
            break;
        }
    }
    dir.to_string()
}

pub fn kill_pid(pid: i32) {
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

/// Per-process network bytes via `nettop -x` deltas applied to app groups.
static NET_PREV: OnceLock<Mutex<HashMap<i32, (u64, u64, Instant)>>> = OnceLock::new();

pub fn apply_nettop(apps: &mut [AppGroup]) {
    let map = NET_PREV.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(out) = std::process::Command::new("nettop")
        .args(["-x", "-P", "-l", "1", "-J", "bytes_in,bytes_out"])
        .output()
    else {
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let now = Instant::now();
    let mut totals: HashMap<i32, (u64, u64)> = HashMap::new();
    for line in text.lines().skip(1) {
        // "process.pid,bytes_in,bytes_out"
        let mut it = line.split(',');
        let Some(first) = it.next() else { continue };
        let Some(pid) = first.rsplit('.').next().and_then(|s| s.parse::<i32>().ok()) else { continue };
        let bin: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let bout: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let e = totals.entry(pid).or_default();
        e.0 += bin;
        e.1 += bout;
    }
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
}
