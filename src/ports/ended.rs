//! Agent leftovers (FR-4–FR-7, D-2): remember every process seen in a live
//! agent session's tree; when the session's root exits, what is still
//! running from that tree was left behind.
//!
//! One file, `ended-sessions.json` in the config dir, holds every tracked
//! session, live or ended, so the list survives a restart (FR-6) and the
//! SessionEnd hook can add a session's tree the app never saw (FR-7). The
//! file holds process names, paths, start times and folders, never
//! command-line arguments or environment (NFR-2).

use crate::agents::{Agent, Session};
use crate::processes::ProcInfo;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::launcher::start_key;

/// Ended sessions are forgotten after a day (FR-6).
const KEEP_SECS: f64 = 24. * 3600.;
const MAX_MEMBERS: usize = 256;
const MAX_SESSIONS: usize = 64;
const SHELLS: [&str; 6] = ["zsh", "bash", "sh", "fish", "login", "dash"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub pid: i32,
    pub uid: u32,
    pub name: String,
    pub exe_path: String,
    pub start_unix: f64,
    /// Listening ports, refreshed while it runs; not saved.
    #[serde(skip)]
    pub ports: Vec<u16>,
}

impl Member {
    fn of(p: &ProcInfo) -> Self {
        let name = Path::new(&p.exe_path).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| p.comm.clone());
        Member { pid: p.pid, uid: p.uid, name, exe_path: p.exe_path.clone(), start_unix: p.start_unix, ports: Vec::new() }
    }

    pub fn key(&self) -> (i32, u64) {
        (self.pid, start_key(self.start_unix))
    }

    /// What `process_control::signal` needs to re-verify it.
    pub fn proc_info(&self) -> ProcInfo {
        ProcInfo { pid: self.pid, uid: self.uid, comm: self.name.clone(), exe_path: self.exe_path.clone(), start_unix: self.start_unix, ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tracked {
    pub agent: String,
    pub folder: Option<String>,
    pub root_pid: i32,
    pub root_start: f64,
    /// When the root was seen gone; None while the session runs.
    pub ended_at: Option<f64>,
    pub members: Vec<Member>,
}

impl Tracked {
    pub fn root_key(&self) -> (i32, u64) {
        (self.root_pid, start_key(self.root_start))
    }

    pub fn agent(&self) -> Option<Agent> {
        [Agent::ClaudeCode, Agent::Codex, Agent::Gemini, Agent::Cursor, Agent::Copilot].into_iter().find(|a| a.name() == self.agent)
    }

    fn add(&mut self, p: &ProcInfo) -> bool {
        if self.members.len() >= MAX_MEMBERS || self.members.iter().any(|m| m.key() == (p.pid, start_key(p.start_unix))) {
            return false;
        }
        self.members.push(Member::of(p));
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub sessions: Vec<Tracked>,
}

fn is_shell(p: &ProcInfo) -> bool {
    let name = Path::new(&p.exe_path).file_name().and_then(|n| n.to_str()).unwrap_or(p.comm.as_str());
    SHELLS.contains(&name.trim_start_matches('-'))
}

/// Fold one pass into the file: add live trees, mark ended roots, drop
/// members that exited and sessions with nothing left. True when it changed.
pub fn observe(file: &mut File, sessions: &[Session], procs: &[ProcInfo], now: f64) -> bool {
    let alive: HashSet<(i32, u64)> = procs.iter().map(|p| (p.pid, start_key(p.start_unix))).collect();
    let in_live: HashSet<(i32, u64)> = sessions.iter().flat_map(|s| s.tree.iter().map(|p| (p.pid, start_key(p.start_unix)))).collect();
    let mut changed = false;
    for s in sessions {
        let key = (s.root.pid, start_key(s.root.start_unix));
        let i = match file.sessions.iter().position(|t| t.root_key() == key) {
            Some(i) => i,
            None => {
                file.sessions.push(Tracked {
                    agent: s.agent.name().into(),
                    folder: s.folder.clone(),
                    root_pid: s.root.pid,
                    root_start: s.root.start_unix,
                    ended_at: None,
                    members: Vec::new(),
                });
                changed = true;
                file.sessions.len() - 1
            }
        };
        let t = &mut file.sessions[i];
        if t.folder.is_none() && s.folder.is_some() {
            t.folder = s.folder.clone();
            changed = true;
        }
        for p in s.tree.iter().skip(1).filter(|p| !is_shell(p)) {
            changed |= t.add(p);
        }
    }
    for t in &mut file.sessions {
        if t.ended_at.is_none() && !alive.contains(&t.root_key()) {
            t.ended_at = Some(now);
            changed = true;
        }
        if t.ended_at.is_some() {
            let before = t.members.len();
            // Gone, reused PID, or now part of another live session.
            t.members.retain(|m| alive.contains(&m.key()) && !in_live.contains(&m.key()));
            changed |= t.members.len() != before;
        }
    }
    let before = file.sessions.len();
    file.sessions.retain(|t| match t.ended_at {
        None => true,
        Some(at) => !t.members.is_empty() && now - at < KEEP_SECS,
    });
    // Live sessions first, so a cap never drops one that is running.
    if file.sessions.len() > MAX_SESSIONS {
        file.sessions.sort_by(|a, b| a.ended_at.is_some().cmp(&b.ended_at.is_some()).then(b.ended_at.partial_cmp(&a.ended_at).unwrap_or(std::cmp::Ordering::Equal)));
        file.sessions.truncate(MAX_SESSIONS);
    }
    changed || file.sessions.len() != before
}

/// The sessions that ended and left something running, newest first.
pub fn left_running(file: &File) -> Vec<Tracked> {
    let mut out: Vec<Tracked> = file.sessions.iter().filter(|t| t.ended_at.is_some() && !t.members.is_empty()).cloned().collect();
    out.sort_by(|a, b| b.ended_at.partial_cmp(&a.ended_at).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn dismiss(file: &mut File, root: (i32, u64)) -> bool {
    let before = file.sessions.len();
    file.sessions.retain(|t| t.root_key() != root);
    file.sessions.len() != before
}

/// pid + start -> agent for every leftover, for the Ports page's launcher.
pub fn leftover_agents(file: &File) -> HashMap<(i32, u64), Agent> {
    file.sessions
        .iter()
        .filter(|t| t.ended_at.is_some())
        .filter_map(|t| t.agent().map(|a| (t, a)))
        .flat_map(|(t, a)| t.members.iter().map(move |m| (m.key(), a)))
        .collect()
}

pub fn path() -> PathBuf {
    directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("tidegauge-{}", unsafe { libc::getuid() })))
        .join("ended-sessions.json")
}

/// Read-modify-write `path` under an flock, shared by the app and the
/// SessionEnd hook. The file is written only when `f` returns true.
pub fn with_file<T>(path: &Path, f: impl FnOnce(&mut File) -> (bool, T)) -> Option<T> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let mut handle = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).ok()?;
    if unsafe { libc::flock(handle.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return None;
    }
    let mut text = String::new();
    let _ = handle.read_to_string(&mut text);
    let mut file: File = serde_json::from_str(&text).unwrap_or_default();
    let (write, out) = f(&mut file);
    if write {
        let body = serde_json::to_string(&file).ok()?;
        handle.set_len(0).ok()?;
        handle.rewind().ok()?;
        handle.write_all(body.as_bytes()).ok()?;
    }
    // The lock ends when `handle` closes.
    Some(out)
}

/// The app's copy, kept in memory between passes; the file is re-read only
/// when the hook may have changed it (its mtime moved).
struct State {
    file: File,
    mtime: Option<std::time::SystemTime>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// One engine pass: update memory and the file; the sessions that left
/// something running, with live ports filled in.
pub fn pass(sessions: &[Session], procs: &[ProcInfo], sockets: &crate::projects::Sockets) -> Vec<Tracked> {
    let path = path();
    let now = crate::metrics::now_unix();
    let mut guard = STATE.lock().unwrap();
    let on_disk = mtime(&path);
    let mut file = match guard.as_ref() {
        Some(state) if on_disk.is_some() && state.mtime == on_disk => state.file.clone(),
        _ => with_file(&path, |f| (false, f.clone())).unwrap_or_default(),
    };
    let mut probe = file.clone();
    if observe(&mut probe, sessions, procs, now) {
        // Apply the same pass to the locked file, so a session the hook
        // wrote since the read above is kept.
        file = with_file(&path, |f| {
            observe(f, sessions, procs, now);
            (true, f.clone())
        })
        .unwrap_or(probe);
    }
    *guard = Some(State { file: file.clone(), mtime: mtime(&path) });
    drop(guard);
    let mut out = left_running(&file);
    for t in &mut out {
        for m in &mut t.members {
            let mut ports: Vec<u16> = sockets.get(&m.pid).map(|v| v.iter().map(|(port, _)| *port).collect()).unwrap_or_default();
            ports.sort_unstable();
            ports.dedup();
            m.ports = ports;
        }
    }
    out
}

/// Forget one ended session (the Dismiss button).
pub fn forget(root: (i32, u64)) {
    let path = path();
    with_file(&path, |f| (dismiss(f, root), ()));
    let mut guard = STATE.lock().unwrap();
    if let Some(state) = guard.as_mut() {
        dismiss(&mut state.file, root);
        state.mtime = mtime(&path);
    }
}

/// The app's current leftovers by process, for the launcher walk.
pub fn known_leftovers() -> HashMap<(i32, u64), Agent> {
    STATE.lock().unwrap().as_ref().map(|s| leftover_agents(&s.file)).unwrap_or_default()
}

/// The agent session that `pid` runs inside: the nearest ancestor (or
/// itself) that `classify` names an agent, and that ancestor's whole tree.
/// The nearest, because that is the agent whose SessionEnd hook runs; an
/// agent started inside another agent ends on its own.
pub fn session_of(pid: i32, procs: &[ProcInfo], classify: &dyn Fn(&ProcInfo) -> Option<Agent>) -> Option<(Agent, Vec<ProcInfo>)> {
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut current = pid;
    let mut found = None;
    for _ in 0..64 {
        let Some(p) = by_pid.get(&current) else { break };
        if let Some(agent) = classify(p) {
            found = Some((agent, *p));
            break;
        }
        if p.ppid <= 1 || p.ppid == current {
            break;
        }
        current = p.ppid;
    }
    let (agent, root) = found?;
    let mut children: HashMap<i32, Vec<&ProcInfo>> = HashMap::new();
    for p in procs {
        children.entry(p.ppid).or_default().push(p);
    }
    let mut tree = vec![root.clone()];
    let mut queue = vec![root.pid];
    while let Some(pid) = queue.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if tree.len() >= MAX_MEMBERS {
                break;
            }
            tree.push((*child).clone());
            queue.push(child.pid);
        }
    }
    Some((agent, tree))
}

/// Add a session's tree to `file` as the hook sees it, just before the
/// agent exits (FR-7). The hook's own chain is left out: it exits next.
pub fn record(file: &mut File, agent: Agent, tree: &[ProcInfo], skip: &HashSet<i32>, folder: Option<String>) -> bool {
    let Some(root) = tree.first() else { return false };
    let key = (root.pid, start_key(root.start_unix));
    let i = match file.sessions.iter().position(|t| t.root_key() == key) {
        Some(i) => i,
        None => {
            file.sessions.push(Tracked { agent: agent.name().into(), folder, root_pid: root.pid, root_start: root.start_unix, ended_at: None, members: Vec::new() });
            file.sessions.len() - 1
        }
    };
    let t = &mut file.sessions[i];
    let mut changed = false;
    for p in tree.iter().skip(1).filter(|p| !is_shell(p) && !skip.contains(&p.pid)) {
        changed |= t.add(p);
    }
    changed || t.members.is_empty()
}

/// `tidegauge hook session-end`: record the ending session's tree. Never
/// fails the hook.
pub fn record_from_hook() {
    let procs = super::basic_procs();
    let me = std::process::id() as i32;
    let classify = |p: &ProcInfo| crate::agents::classify(p, &|pid| crate::processes::proc_args(pid));
    let Some((agent, tree)) = session_of(me, &procs, &classify) else { return };
    // This hook and the shells between it and the agent exit right after.
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut skip = HashSet::new();
    let mut current = me;
    while current != tree[0].pid {
        skip.insert(current);
        match by_pid.get(&current) {
            Some(p) if p.ppid > 1 && p.ppid != current => current = p.ppid,
            _ => break,
        }
    }
    let folder = Some(crate::processes::proc_cwd(tree[0].pid)).filter(|c| !c.is_empty());
    with_file(&path(), |f| (record(f, agent, &tree, &skip, folder), ()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Activity;

    fn proc(pid: i32, ppid: i32, path: &str, start: f64) -> ProcInfo {
        ProcInfo { pid, ppid, uid: 501, exe_path: path.into(), comm: Path::new(path).file_name().unwrap().to_string_lossy().into(), start_unix: start, ..Default::default() }
    }

    fn session(tree: Vec<ProcInfo>) -> Session {
        Session {
            agent: Agent::ClaudeCode,
            root: tree[0].clone(),
            title: None,
            folder: Some("/Users/me/Dev/app".into()),
            host: None,
            activity: Activity::Idle { since: None },
            started: Vec::new(),
            mem_bytes: 0,
            cpu_pct: 0.,
            tree,
        }
    }

    #[test]
    fn a_child_that_outlives_its_session_is_left_running() {
        let root = proc(900, 1, "/Users/me/.local/bin/claude", 100.);
        let shell = proc(901, 900, "/bin/zsh", 101.);
        let vite = proc(950, 901, "/opt/homebrew/bin/node", 102.);
        let mut f = File::default();
        assert!(observe(&mut f, &[session(vec![root.clone(), shell.clone(), vite.clone()])], &[root, shell, vite.clone()], 1000.));
        assert!(left_running(&f).is_empty(), "nothing is left while the session runs");
        // The root exits; vite is reparented to launchd and keeps running.
        let orphan = ProcInfo { ppid: 1, ..vite.clone() };
        assert!(observe(&mut f, &[], &[orphan.clone()], 1010.));
        let left = left_running(&f);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].ended_at, Some(1010.));
        assert_eq!(left[0].members.iter().map(|m| m.pid).collect::<Vec<_>>(), vec![950], "shells are not listed");
        assert_eq!(leftover_agents(&f).get(&(950, start_key(102.))), Some(&Agent::ClaudeCode));
        // It exits: the session is forgotten.
        assert!(observe(&mut f, &[], &[], 1020.));
        assert!(f.sessions.is_empty());
    }

    #[test]
    fn a_session_that_leaves_nothing_is_not_shown() {
        let root = proc(900, 1, "/Users/me/.local/bin/claude", 100.);
        let tool = proc(910, 900, "/usr/bin/git", 101.);
        let mut f = File::default();
        observe(&mut f, &[session(vec![root.clone(), tool.clone()])], &[root, tool], 1000.);
        observe(&mut f, &[], &[], 1010.);
        assert!(left_running(&f).is_empty() && f.sessions.is_empty());
    }

    #[test]
    fn a_reused_pid_is_not_a_leftover() {
        let root = proc(900, 1, "/Users/me/.local/bin/claude", 100.);
        let vite = proc(950, 900, "/opt/homebrew/bin/node", 102.);
        let mut f = File::default();
        observe(&mut f, &[session(vec![root.clone(), vite.clone()])], &[root, vite], 1000.);
        let other = proc(950, 1, "/usr/bin/python3", 500.);
        observe(&mut f, &[], &[other], 1010.);
        assert!(left_running(&f).is_empty());
    }

    #[test]
    fn ended_sessions_expire_after_a_day_and_can_be_dismissed() {
        let root = proc(900, 1, "/Users/me/.local/bin/claude", 100.);
        let vite = proc(950, 900, "/opt/homebrew/bin/node", 102.);
        let mut f = File::default();
        observe(&mut f, &[session(vec![root.clone(), vite.clone()])], &[root.clone(), vite.clone()], 1000.);
        observe(&mut f, &[], &[vite.clone()], 1010.);
        assert_eq!(left_running(&f).len(), 1);
        let mut g = f.clone();
        observe(&mut g, &[], &[vite.clone()], 1010. + KEEP_SECS + 1.);
        assert!(g.sessions.is_empty(), "older than a day");
        assert!(dismiss(&mut f, (900, start_key(100.))));
        assert!(left_running(&f).is_empty());
    }

    #[test]
    fn the_hook_finds_its_own_agent_and_skips_its_own_chain() {
        let ghostty = proc(10, 1, "/Applications/Ghostty.app/Contents/MacOS/ghostty", 1.);
        let claude = proc(900, 10, "/Users/me/.local/bin/claude", 100.);
        let server = proc(950, 900, "/opt/homebrew/bin/node", 102.);
        let hook_sh = proc(960, 900, "/bin/sh", 103.);
        let hook = proc(961, 960, "/Applications/Tidegauge.app/Contents/MacOS/tidegauge", 103.5);
        let procs = vec![ghostty, claude, server, hook_sh, hook];
        let classify = |p: &ProcInfo| (p.comm == "claude").then_some(Agent::ClaudeCode);
        let (agent, tree) = session_of(961, &procs, &classify).unwrap();
        assert_eq!((agent, tree[0].pid, tree.len()), (Agent::ClaudeCode, 900, 4));
        let mut f = File::default();
        assert!(record(&mut f, agent, &tree, &HashSet::from([961, 960]), None));
        assert_eq!(f.sessions[0].members.iter().map(|m| m.pid).collect::<Vec<_>>(), vec![950]);
        // The app later sees the root gone: the server is left running.
        observe(&mut f, &[], &[procs[2].clone()], 2000.);
        assert_eq!(left_running(&f)[0].members[0].pid, 950);
        assert!(session_of(10, &procs, &classify).is_none(), "outside any agent");
        // An agent run inside another: its own hook names the inner one.
        let mut nested = procs.clone();
        nested.push(proc(970, 950, "/Users/me/.local/bin/claude", 104.));
        nested.push(proc(971, 970, "/Applications/Tidegauge.app/Contents/MacOS/tidegauge", 105.));
        assert_eq!(session_of(971, &nested, &classify).unwrap().1[0].pid, 970);
    }

    #[test]
    fn the_file_round_trips_under_its_lock_and_keeps_start_times_exact() {
        let dir = crate::cleanup::test_support::temp("ended-file");
        let path = dir.join("ended-sessions.json");
        let start = 1_794_858_093.123_457;
        let root = proc(900, 1, "/Users/me/.local/bin/claude", start);
        let vite = proc(950, 900, "/opt/homebrew/bin/node", start + 1.5);
        with_file(&path, |f| (observe(f, &[session(vec![root.clone(), vite.clone()])], &[root.clone(), vite.clone()], 1.), ()));
        let back = with_file(&path, |f| (false, f.clone())).unwrap();
        assert_eq!(back.sessions[0].members[0].start_unix, start + 1.5);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("args") && !text.contains("env"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
