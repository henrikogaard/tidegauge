//! Coding-agent sessions found in the process list, and AI plan usage read
//! from the snapshot file OpenQuota saves on this Mac. Tidegauge never reads
//! credentials or calls a provider: if OpenQuota is not installed, plan
//! usage is simply unavailable.

use crate::engine::ProjectGroup;
use crate::processes::{bundle_root, proc_args, proc_cwd, ProcInfo};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

// ---------- plans (from OpenQuota) ----------

#[derive(Deserialize)]
struct RawWindow {
    id: String,
    label: String,
    kind: Option<String>,
    used: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    #[serde(rename = "resetsAt")]
    resets_at: Option<String>,
}

/// Only the plan name is read; account labels (often an email) are skipped.
#[derive(Deserialize)]
struct RawAccount {
    plan: Option<String>,
}

#[derive(Deserialize)]
struct RawSnapshot {
    #[serde(rename = "providerID")]
    provider_id: String,
    account: Option<RawAccount>,
    #[serde(default)]
    windows: Vec<RawWindow>,
    #[serde(rename = "creditsRemaining")]
    credits_remaining: Option<f64>,
    #[serde(rename = "creditsUnit")]
    credits_unit: Option<String>,
    #[serde(rename = "fetchedAt")]
    fetched_at: Option<String>,
    #[serde(rename = "isStale", default)]
    is_stale: bool,
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub label: String,
    /// 0…1 of the allowance used.
    pub used: f64,
    pub resets_at: Option<f64>,
    /// Window length in seconds when the label names one (5h, week).
    pub length: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub provider: String,
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    pub credits: Option<(f64, String)>,
    pub fetched_at: Option<f64>,
    pub stale: bool,
    pub failed: bool,
}

impl Plan {
    /// The window closest to running out, which decides the headline.
    pub fn tightest(&self) -> Option<&Window> {
        self.windows.iter().max_by(|a, b| a.used.total_cmp(&b.used))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Plans {
    /// OpenQuota has not saved anything on this Mac.
    #[default]
    Missing,
    Unreadable,
    Loaded(Vec<Plan>),
}

/// Same safety valve OpenQuota uses for its own cache.
const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024;

fn snapshot_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home).join("Library/Application Support/openquota/snapshots.json")
    })
}

pub fn parse_plans(text: &str) -> Plans {
    let Ok(raw) = serde_json::from_str::<HashMap<String, RawSnapshot>>(text) else {
        return Plans::Unreadable;
    };
    let mut plans: Vec<Plan> = raw
        .into_values()
        .map(|snap| Plan {
            windows: snap
                .windows
                .into_iter()
                .filter(|w| w.kind.as_deref().is_none_or(|kind| kind == "consumption"))
                .filter_map(|w| {
                    let limit = w.limit.filter(|limit| *limit > 0.)?;
                    let used = match (w.used, w.remaining) {
                        (Some(used), _) => used / limit,
                        (None, Some(remaining)) => 1. - remaining / limit,
                        _ => return None,
                    };
                    used.is_finite().then(|| Window {
                        length: window_length(&w.id, &w.label),
                        label: w.label,
                        used: used.clamp(0., 1.),
                        resets_at: w.resets_at.as_deref().and_then(parse_iso),
                    })
                })
                .collect(),
            plan: snap.account.and_then(|account| account.plan),
            credits: snap.credits_remaining.zip(snap.credits_unit),
            fetched_at: snap.fetched_at.as_deref().and_then(parse_iso),
            stale: snap.is_stale,
            failed: snap.error_message.is_some(),
            provider: snap.provider_id,
        })
        .collect();
    plans.sort_by(|a, b| a.provider.cmp(&b.provider));
    Plans::Loaded(plans)
}

fn read_plans(path: &Path) -> Plans {
    let Ok(meta) = std::fs::metadata(path) else {
        return Plans::Missing;
    };
    if meta.len() > MAX_SNAPSHOT_BYTES {
        return Plans::Unreadable;
    }
    std::fs::read_to_string(path)
        .map(|text| parse_plans(&text))
        .unwrap_or(Plans::Unreadable)
}

static PLAN_CACHE: Mutex<Option<(Option<SystemTime>, Plans)>> = Mutex::new(None);

/// Plan usage, re-read only when OpenQuota rewrites its file.
pub fn plans() -> Plans {
    let Some(path) = snapshot_path() else {
        return Plans::Missing;
    };
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut cache = PLAN_CACHE.lock().unwrap();
    if let Some((seen, plans)) = cache.as_ref() {
        if *seen == modified && modified.is_some() {
            return plans.clone();
        }
    }
    let plans = read_plans(&path);
    *cache = Some((modified, plans.clone()));
    plans
}

fn window_length(id: &str, label: &str) -> Option<f64> {
    let label = label.to_lowercase();
    let id = id.to_lowercase();
    if label == "5h" || id.ends_with(".5h") || id.contains("five_hour") {
        Some(5. * 3600.)
    } else if label == "week" || label == "1w" || label == "7d" || id.contains("week") {
        Some(7. * 86400.)
    } else if label == "24h" || label == "day" || id.contains("daily") {
        Some(86400.)
    } else {
        None
    }
}

/// Seconds since the Unix epoch for an RFC 3339 / ISO 8601 UTC timestamp
/// such as `2026-10-14T20:44:13Z` or `…T20:44:13.512+02:00`.
pub fn parse_iso(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    let mut fraction = 0.;
    if let Some(after) = rest.strip_prefix('.') {
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        fraction = format!("0.{}", &after[..digits]).parse().ok()?;
        rest = &after[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        zone if zone.len() == 6 && (zone.starts_with('+') || zone.starts_with('-')) => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let h: i64 = zone.get(1..3)?.parse().ok()?;
            let m: i64 = zone.get(4..6)?.parse().ok()?;
            sign * (h * 3600 + m * 60)
        }
        _ => return None,
    };
    // Days from civil (Howard Hinnant), proleptic Gregorian.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86400 + hour * 3600 + minute * 60 + second - offset) as f64 + fraction)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pace {
    /// Too early in the window, or its length or reset is unknown.
    Unknown,
    /// At the current rate the window ends with this fraction to spare.
    OnPace { spare: f64 },
    /// At the current rate the allowance runs out this many seconds from now.
    RunsOut { in_secs: f64 },
    UsedUp,
}

pub fn pace(window: &Window, now: f64) -> Pace {
    if window.used >= 1. {
        return Pace::UsedUp;
    }
    let (Some(reset), Some(length)) = (window.resets_at, window.length) else {
        return Pace::Unknown;
    };
    let left = reset - now;
    if left <= 0. || left > length {
        return Pace::Unknown;
    }
    let elapsed = length - left;
    if elapsed < length * 0.05 {
        return Pace::Unknown;
    }
    if window.used <= 0. {
        return Pace::OnPace { spare: 1. };
    }
    let rate = window.used / elapsed;
    let projected = rate * length;
    if projected <= 1. {
        Pace::OnPace { spare: 1. - projected }
    } else {
        Pace::RunsOut { in_secs: (1. - window.used) / rate }
    }
}

// ---------- agent sessions ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    Codex,
    Gemini,
    Cursor,
    Copilot,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::Cursor => "Cursor Agent",
            Self::Copilot => "Copilot CLI",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Activity {
    Working,
    /// Idle, with the time it went idle when the agent reports it.
    Idle { since: Option<f64> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Server,
    Browser,
    ToolServer,
    Command,
}

/// Something a session started, grouped by app or program name.
#[derive(Clone, Debug)]
pub struct Started {
    pub name: String,
    pub role: Role,
    pub port: Option<u16>,
    pub count: usize,
    pub mem_bytes: u64,
    pub cpu_pct: f64,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub agent: Agent,
    pub root: ProcInfo,
    pub title: Option<String>,
    pub folder: Option<String>,
    /// The app the session runs inside (Claude, Terminal, ChatGPT…), if any.
    pub host: Option<String>,
    pub activity: Activity,
    pub started: Vec<Started>,
    pub mem_bytes: u64,
    pub cpu_pct: f64,
    /// The session and everything it started, for "Quit session".
    pub tree: Vec<ProcInfo>,
}

/// Working when the session's tree uses more CPU than this (one core = 100).
const WORKING_CPU: f64 = 15.;
const MAX_TREE: usize = 256;
const SHELLS: [&str; 5] = ["zsh", "bash", "sh", "fish", "login"];

fn file_name(p: &ProcInfo) -> &str {
    Path::new(&p.exe_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(p.comm.as_str())
}

pub fn classify(p: &ProcInfo, args: &dyn Fn(i32) -> Option<Vec<String>>) -> Option<Agent> {
    match file_name(p) {
        // Exact name: the Claude desktop app's own binary is "Claude".
        "claude" => Some(Agent::ClaudeCode),
        "cursor-agent" => Some(Agent::Cursor),
        "copilot" => Some(Agent::Copilot),
        "codex" => {
            // The same binary also runs as a background host for apps
            // (including OpenQuota's own usage reader); those are not sessions.
            let args = args(p.pid)?;
            (!args
                .iter()
                .skip(1)
                .any(|a| matches!(a.as_str(), "app-server" | "mcp-server" | "mcp" | "proto")))
            .then_some(Agent::Codex)
        }
        "node" | "bun" => {
            let args = args(p.pid)?;
            args.iter()
                .skip(1)
                .take(2)
                .any(|a| a.contains("@google/gemini-cli") || a.ends_with("/gemini"))
                .then_some(Agent::Gemini)
        }
        _ => None,
    }
}

#[derive(Deserialize, Clone)]
struct ClaudeSessionFile {
    pid: i32,
    cwd: Option<String>,
    status: Option<String>,
    name: Option<String>,
    #[serde(rename = "statusUpdatedAt")]
    status_updated_at: Option<f64>,
    #[serde(rename = "procStart")]
    proc_start: Option<String>,
}

/// `ctime`-style start time, as Claude Code records `procStart` (in UTC).
fn ctime(start_unix: f64, utc: bool) -> Option<String> {
    let secs = start_unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let converted = if utc {
        unsafe { libc::gmtime_r(&secs, &mut tm) }
    } else {
        unsafe { libc::localtime_r(&secs, &mut tm) }
    };
    if converted.is_null() {
        return None;
    }
    let mut buf = [0u8; 64];
    let format = c"%a %b %e %H:%M:%S %Y";
    let len = unsafe { libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), format.as_ptr(), &tm) };
    (len > 0).then(|| String::from_utf8_lossy(&buf[..len]).into_owned())
}

fn same_words(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// Whether a session file belongs to the process started at `start_unix`.
/// Claude Code writes UTC; local time is accepted too in case that changes.
fn same_start(proc_start: &str, start_unix: f64) -> bool {
    [true, false]
        .into_iter()
        .filter_map(|utc| ctime(start_unix, utc))
        .any(|stamp| same_words(proc_start, &stamp))
}

/// Session files by path, each parsed again only when its modification time
/// changes (`None` for a file that did not parse).
type SessionCache = HashMap<PathBuf, (SystemTime, Option<ClaudeSessionFile>)>;
static SESSION_FILES: Mutex<Option<SessionCache>> = Mutex::new(None);

fn claude_sessions() -> HashMap<i32, ClaudeSessionFile> {
    let Some(dir) = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude/sessions")) else {
        return HashMap::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    let listing: Vec<(PathBuf, SystemTime)> = entries
        .flatten()
        .take(64)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.len() <= 64 * 1024)?;
            Some((e.path(), meta.modified().ok()?))
        })
        .collect();
    let mut cache = SESSION_FILES.lock().unwrap();
    read_sessions(cache.get_or_insert_with(HashMap::new), listing, |path| {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<ClaudeSessionFile>(&text).ok()
    })
}

/// The parsed session files in `listing`, by pid. `load` runs only for a file
/// that is new or whose modification time differs from the cached one; files
/// no longer listed leave the cache.
fn read_sessions(
    cache: &mut SessionCache,
    listing: Vec<(PathBuf, SystemTime)>,
    load: impl Fn(&Path) -> Option<ClaudeSessionFile>,
) -> HashMap<i32, ClaudeSessionFile> {
    let mut next = SessionCache::new();
    for (path, modified) in listing {
        let entry = match cache.remove(&path) {
            Some((seen, parsed)) if seen == modified => (seen, parsed),
            _ => (modified, load(&path)),
        };
        next.insert(path, entry);
    }
    *cache = next;
    cache
        .values()
        .filter_map(|(_, file)| file.clone())
        .map(|file| (file.pid, file))
        .collect()
}

/// Command-line arguments by process, read once per process: a pid with the
/// same start time and program keeps the arguments it was first read with.
#[derive(Default)]
struct ArgsCache(HashMap<i32, (u64, String, Option<Vec<String>>)>);

static ARGS: Mutex<Option<ArgsCache>> = Mutex::new(None);

impl ArgsCache {
    fn get(&mut self, p: &ProcInfo, fetch: impl FnOnce(i32) -> Option<Vec<String>>) -> Option<Vec<String>> {
        let start = p.start_unix.to_bits();
        if let Some((seen, exe, args)) = self.0.get(&p.pid) {
            if *seen == start && *exe == p.exe_path {
                return args.clone();
            }
        }
        let args = fetch(p.pid);
        self.0.insert(p.pid, (start, p.exe_path.clone(), args.clone()));
        args
    }

    fn retain_live(&mut self, procs: &[ProcInfo]) {
        let live: HashSet<i32> = procs.iter().map(|p| p.pid).collect();
        self.0.retain(|pid, _| live.contains(pid));
    }
}

fn role_of(p: &ProcInfo, port: Option<u16>, args: &dyn Fn(i32) -> Option<Vec<String>>) -> Role {
    if port.is_some() {
        return Role::Server;
    }
    let path = p.exe_path.to_lowercase();
    if ["chrome", "chromium", "firefox", "safari", "webkit", "playwright"]
        .iter()
        .any(|name| path.contains(name))
    {
        return Role::Browser;
    }
    let name = file_name(p).to_lowercase();
    if name.contains("mcp")
        || (matches!(name.as_str(), "node" | "python" | "python3" | "uv" | "uvx" | "npx" | "bun" | "deno")
            && args(p.pid).is_some_and(|a| a.iter().skip(1).take(4).any(|arg| arg.to_lowercase().contains("mcp"))))
    {
        return Role::ToolServer;
    }
    Role::Command
}

/// Find agent sessions and what each one started.
pub fn detect(
    procs: &[ProcInfo],
    projects: &[ProjectGroup],
    args: &dyn Fn(i32) -> Option<Vec<String>>,
    claude: &HashMap<i32, ClaudeSessionFileView>,
) -> Vec<Session> {
    let uid = unsafe { libc::getuid() };
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut children: HashMap<i32, Vec<&ProcInfo>> = HashMap::new();
    for p in procs {
        children.entry(p.ppid).or_default().push(p);
    }
    let ports: HashMap<i32, u16> = projects
        .iter()
        .flat_map(|g| g.servers.iter().map(|s| (s.pid, s.port)))
        .collect();
    let candidates: Vec<(&ProcInfo, Agent)> = procs
        .iter()
        .filter(|p| p.uid == uid && p.pid > 1)
        .filter_map(|p| classify(p, args).map(|agent| (p, agent)))
        .collect();
    let roots: HashSet<i32> = candidates.iter().map(|(p, _)| p.pid).collect();
    let nested = |p: &ProcInfo| {
        let mut current = p.ppid;
        for _ in 0..64 {
            if roots.contains(&current) {
                return true;
            }
            match by_pid.get(&current) {
                Some(parent) if parent.ppid > 1 && parent.ppid != current => current = parent.ppid,
                _ => return false,
            }
        }
        false
    };
    let mut sessions: Vec<Session> = candidates
        .into_iter()
        .filter(|(p, _)| !nested(p))
        .map(|(root, agent)| {
            let mut tree = vec![root.clone()];
            let mut queue = vec![root.pid];
            while let Some(pid) = queue.pop() {
                for child in children.get(&pid).into_iter().flatten() {
                    if tree.len() >= MAX_TREE {
                        break;
                    }
                    tree.push((*child).clone());
                    queue.push(child.pid);
                }
            }
            let mut groups: Vec<Started> = Vec::new();
            for p in tree.iter().skip(1).filter(|p| !SHELLS.contains(&file_name(p))) {
                let name = bundle_root(&p.exe_path)
                    .and_then(|bundle| Path::new(&bundle).file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| file_name(p).to_string());
                let port = ports.get(&p.pid).copied();
                let role = role_of(p, port, args);
                match groups.iter_mut().find(|g| g.name == name && g.role == role) {
                    Some(group) => {
                        group.count += 1;
                        group.mem_bytes += p.mem_bytes;
                        group.cpu_pct += p.cpu_pct;
                        group.port = group.port.or(port);
                    }
                    None => groups.push(Started { name, role, port, count: 1, mem_bytes: p.mem_bytes, cpu_pct: p.cpu_pct }),
                }
            }
            groups.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
            let mem_bytes = tree.iter().map(|p| p.mem_bytes).sum();
            let cpu_pct: f64 = tree.iter().map(|p| p.cpu_pct).sum();
            let file = claude
                .get(&root.pid)
                .filter(|_| agent == Agent::ClaudeCode)
                .filter(|file| file.proc_start.as_deref().is_some_and(|start| same_start(start, root.start_unix)));
            let activity = match file.and_then(|f| f.status.as_deref()) {
                Some("busy" | "working" | "running") => Activity::Working,
                Some(_) => Activity::Idle { since: file.and_then(|f| f.status_updated_at).map(|ms| ms / 1000.) },
                None if cpu_pct >= WORKING_CPU => Activity::Working,
                None => Activity::Idle { since: None },
            };
            let folder = file
                .and_then(|f| f.cwd.clone())
                .or_else(|| Some(proc_cwd(root.pid)).filter(|cwd| !cwd.is_empty()));
            // Nearest ancestor inside an app bundle: where the session lives.
            let host = {
                let mut current = root.ppid;
                let mut found = None;
                for _ in 0..32 {
                    let Some(parent) = by_pid.get(&current) else { break };
                    if let Some(bundle) = bundle_root(&parent.exe_path) {
                        found = Path::new(&bundle).file_stem().map(|s| s.to_string_lossy().into_owned());
                        break;
                    }
                    if parent.ppid <= 1 || parent.ppid == current {
                        break;
                    }
                    current = parent.ppid;
                }
                found
            };
            Session {
                agent,
                host,
                title: file.and_then(|f| f.name.clone()).filter(|name| !name.trim().is_empty()),
                folder,
                activity,
                started: groups,
                mem_bytes,
                cpu_pct,
                tree,
                root: root.clone(),
            }
        })
        .collect();
    sessions.sort_by(|a, b| {
        (b.activity == Activity::Working)
            .cmp(&(a.activity == Activity::Working))
            .then(b.mem_bytes.cmp(&a.mem_bytes))
    });
    sessions
}

/// What Tidegauge keeps from a Claude Code session file.
#[derive(Clone, Debug)]
pub struct ClaudeSessionFileView {
    pub cwd: Option<String>,
    pub status: Option<String>,
    pub name: Option<String>,
    pub status_updated_at: Option<f64>,
    pub proc_start: Option<String>,
}

pub fn claude_session_files() -> HashMap<i32, ClaudeSessionFileView> {
    claude_sessions()
        .into_iter()
        .map(|(pid, f)| {
            (
                pid,
                ClaudeSessionFileView {
                    cwd: f.cwd,
                    status: f.status,
                    name: f.name,
                    status_updated_at: f.status_updated_at,
                    proc_start: f.proc_start,
                },
            )
        })
        .collect()
}

/// Sessions on this Mac now, reading arguments only for programs that need it.
pub fn sessions(procs: &[ProcInfo], projects: &[ProjectGroup]) -> Vec<Session> {
    let mut guard = ARGS.lock().unwrap();
    let cache = std::cell::RefCell::new(guard.get_or_insert_with(ArgsCache::default));
    cache.borrow_mut().retain_live(procs);
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let args = |pid: i32| {
        let p = by_pid.get(&pid)?;
        cache.borrow_mut().get(p, proc_args)
    };
    detect(procs, projects, &args, &claude_session_files())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, ppid: i32, path: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            uid: unsafe { libc::getuid() },
            exe_path: path.into(),
            comm: Path::new(path).file_name().unwrap().to_string_lossy().into(),
            start_unix: 1_700_000_000.,
            mem_bytes: 100,
            ..Default::default()
        }
    }

    #[test]
    fn session_files_are_parsed_again_only_when_they_change() {
        use std::cell::Cell;
        use std::time::Duration;
        let file = |pid| ClaudeSessionFile { pid, cwd: None, status: None, name: None, status_updated_at: None, proc_start: None };
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        let (a, b) = (PathBuf::from("a.json"), PathBuf::from("b.json"));
        let loads = Cell::new(0);
        let load = |path: &Path| {
            loads.set(loads.get() + 1);
            Some(file(if path == Path::new("a.json") { 1 } else { 2 }))
        };
        let mut cache = SessionCache::new();
        let out = read_sessions(&mut cache, vec![(a.clone(), at(10)), (b.clone(), at(10))], load);
        assert_eq!((loads.get(), out.len()), (2, 2));
        let out = read_sessions(&mut cache, vec![(a.clone(), at(10)), (b.clone(), at(10))], load);
        assert_eq!((loads.get(), out.len()), (2, 2), "unchanged files are not read");
        let out = read_sessions(&mut cache, vec![(a.clone(), at(11)), (b.clone(), at(10))], load);
        assert_eq!((loads.get(), out.len()), (3, 2), "a newer file is read again");
        let out = read_sessions(&mut cache, vec![(b.clone(), at(10))], load);
        assert_eq!((loads.get(), out.keys().copied().collect::<Vec<_>>()), (3, vec![2]));
        assert_eq!(cache.len(), 1, "a removed file leaves the cache");
    }

    #[test]
    fn arguments_are_read_once_per_process_start() {
        use std::cell::Cell;
        let reads = Cell::new(0);
        let fetch = |_| {
            reads.set(reads.get() + 1);
            Some(vec!["node".to_string(), format!("run-{}", reads.get())])
        };
        let mut cache = ArgsCache::default();
        let p = proc(7, 1, "/usr/local/bin/node");
        assert_eq!(cache.get(&p, fetch).unwrap()[1], "run-1");
        assert_eq!(cache.get(&p, fetch).unwrap()[1], "run-1");
        assert_eq!(reads.get(), 1);
        // The same pid started again (new start time) is read afresh.
        let restarted = ProcInfo { start_unix: p.start_unix + 5., ..p.clone() };
        assert_eq!(cache.get(&restarted, fetch).unwrap()[1], "run-2");
        // So is the same process running another program.
        let exec = ProcInfo { exe_path: "/usr/local/bin/bun".into(), ..restarted.clone() };
        assert_eq!(cache.get(&exec, fetch).unwrap()[1], "run-3");
        // A process that is gone is forgotten.
        cache.retain_live(&[]);
        assert!(cache.0.is_empty());
    }

    #[test]
    fn iso_timestamps_parse_in_utc() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0.));
        assert_eq!(parse_iso("2026-10-14T20:44:13Z"), Some(1_792_010_653.));
        assert_eq!(parse_iso("2026-10-14T22:44:13+02:00"), Some(1_792_010_653.));
        assert_eq!(parse_iso("2026-10-14T20:44:13.5Z"), Some(1_792_010_653.5));
        assert_eq!(parse_iso("2026-13-14T20:44:13Z"), None);
        assert_eq!(parse_iso("yesterday"), None);
    }

    #[test]
    fn openquota_snapshots_keep_usage_but_not_account_labels() {
        let text = r#"{
          "claude@1": {"providerID":"claude","account":{"providerID":"claude","id":"x","label":"me@example.com"},
            "windows":[{"id":"claude.5h","label":"5h","kind":"consumption","used":20,"limit":100,"unit":"%","resetsAt":"2026-10-14T23:00:00Z"},
                       {"id":"claude.week","label":"Week","kind":"consumption","used":65,"limit":100,"resetsAt":"2026-10-18T00:00:00Z"}],
            "fetchedAt":"2026-10-14T20:00:00Z","isStale":true,"futureField":1},
          "codex@2": {"providerID":"codex","account":{"providerID":"codex","id":"y","plan":"pro"},
            "windows":[{"id":"codex.credits","label":"Credits","kind":"credits","used":5,"limit":10}],
            "creditsRemaining":12.5,"creditsUnit":"credits","fetchedAt":"2026-10-14T20:00:00Z","isStale":false}
        }"#;
        let Plans::Loaded(plans) = parse_plans(text) else { panic!("not parsed") };
        assert_eq!(plans.len(), 2);
        let claude = &plans[0];
        assert_eq!(claude.provider, "claude");
        assert!(claude.stale);
        assert_eq!(claude.windows.len(), 2);
        assert_eq!(claude.tightest().unwrap().label, "Week");
        assert_eq!(claude.windows[0].length, Some(5. * 3600.));
        assert!(!format!("{claude:?}").contains("example.com"));
        let codex = &plans[1];
        assert_eq!(codex.plan.as_deref(), Some("pro"));
        assert!(codex.windows.is_empty(), "credit windows are not rings");
        assert_eq!(codex.credits, Some((12.5, "credits".into())));
        assert_eq!(parse_plans("not json"), Plans::Unreadable);
    }

    #[test]
    fn pace_projects_the_current_rate_to_the_reset() {
        let week = 7. * 86400.;
        let reset = 1_000_000.;
        let window = |used: f64| Window { label: "Week".into(), used, resets_at: Some(reset), length: Some(week) };
        // Half the week gone, 40% used: ends with 20% to spare.
        let half = reset - week / 2.;
        match pace(&window(0.4), half) {
            Pace::OnPace { spare } => assert!((spare - 0.2).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
        // Half gone, 80% used: runs out a quarter-week from now.
        match pace(&window(0.8), half) {
            Pace::RunsOut { in_secs } => assert!((in_secs - week / 8.).abs() < 1.),
            other => panic!("{other:?}"),
        }
        assert_eq!(pace(&window(1.0), half), Pace::UsedUp);
        assert_eq!(pace(&window(0.1), reset - week * 0.99), Pace::Unknown, "too early");
        assert_eq!(pace(&Window { length: None, ..window(0.4) }, half), Pace::Unknown);
    }

    #[test]
    fn agents_are_recognised_by_program_not_by_desktop_app() {
        let none = |_: i32| None;
        assert_eq!(classify(&proc(10, 1, "/Users/x/.local/bin/claude"), &none), Some(Agent::ClaudeCode));
        assert_eq!(classify(&proc(11, 1, "/Applications/Claude.app/Contents/MacOS/Claude"), &none), None);
        let host = |_: i32| Some(vec!["codex".to_string(), "app-server".to_string()]);
        assert_eq!(classify(&proc(12, 1, "/opt/homebrew/bin/codex"), &host), None);
        let cli = |_: i32| Some(vec!["codex".to_string(), "--model".to_string()]);
        assert_eq!(classify(&proc(13, 1, "/opt/homebrew/bin/codex"), &cli), Some(Agent::Codex));
        assert_eq!(classify(&proc(14, 1, "/opt/homebrew/bin/codex"), &none), None, "unknown arguments: not claimed");
        let gemini = |_: i32| Some(vec!["node".to_string(), "/opt/lib/node_modules/@google/gemini-cli/dist/index.js".to_string()]);
        assert_eq!(classify(&proc(15, 1, "/opt/homebrew/bin/node"), &gemini), Some(Agent::Gemini));
    }

    #[test]
    fn a_session_knows_the_app_it_runs_in() {
        let procs = vec![
            proc(50, 1, "/Applications/Claude.app/Contents/MacOS/Claude"),
            proc(51, 50, "/Applications/Claude.app/Contents/Helpers/disclaimer"),
            proc(52, 51, "/Users/x/Library/Application Support/Claude/claude-code/2.1/claude.app/Contents/MacOS/claude"),
            proc(60, 1, "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
            proc(61, 60, "/bin/zsh"),
            proc(62, 61, "/Users/x/.local/bin/claude"),
        ];
        let sessions = detect(&procs, &[], &|_| None, &HashMap::new());
        let host = |pid: i32| sessions.iter().find(|s| s.root.pid == pid).and_then(|s| s.host.clone());
        assert_eq!(host(52).as_deref(), Some("Claude"), "nearest ancestor bundle, not the session's own");
        assert_eq!(host(62).as_deref(), Some("Terminal"));
    }

    #[test]
    fn sessions_group_what_they_started() {
        let mut procs = vec![
            proc(100, 1, "/Users/x/.local/bin/claude"),
            proc(101, 100, "/bin/zsh"),
            proc(102, 101, "/opt/homebrew/bin/node"),
            proc(103, 100, "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
            proc(104, 103, "/Applications/Google Chrome for Testing.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"),
            proc(105, 100, "/Users/x/.local/bin/claude"),
            proc(200, 1, "/opt/homebrew/bin/node"),
        ];
        procs[2].cpu_pct = 20.;
        let projects = vec![ProjectGroup {
            servers: vec![crate::engine::DevServer { pid: 102, port: 3001, ..Default::default() }],
            ..Default::default()
        }];
        let args = |_: i32| None;
        let sessions = detect(&procs, &projects, &args, &HashMap::new());
        assert_eq!(sessions.len(), 1, "the nested claude is part of the first session");
        let s = &sessions[0];
        assert_eq!(s.root.pid, 100);
        assert_eq!(s.host, None, "launchd parent: no host app");
        assert_eq!(s.tree.len(), 6);
        assert_eq!(s.activity, Activity::Working, "child CPU counts toward the session");
        let server = s.started.iter().find(|g| g.role == Role::Server).unwrap();
        assert_eq!((server.name.as_str(), server.port), ("node", Some(3001)));
        let browser = s.started.iter().find(|g| g.role == Role::Browser).unwrap();
        assert_eq!(browser.count, 2, "browser helpers group under the browser app");
        assert!(s.started.iter().all(|g| g.name != "zsh"), "shells are hidden");
    }

    #[test]
    fn claude_session_file_matches_utc_start_time() {
        // 2023-11-14 22:13:20 UTC, as Claude Code writes it.
        let procs = vec![proc(100, 1, "/Users/x/.local/bin/claude")];
        let file = |start: &str| ClaudeSessionFileView {
            cwd: None,
            status: Some("busy".into()),
            name: None,
            status_updated_at: None,
            proc_start: Some(start.into()),
        };
        let busy = detect(&procs, &[], &|_| None, &HashMap::from([(100, file("Tue Nov 14 22:13:20 2023"))]));
        assert_eq!(busy[0].activity, Activity::Working);
        let reused = detect(&procs, &[], &|_| None, &HashMap::from([(100, file("Tue Nov 14 22:13:21 2023"))]));
        assert!(matches!(reused[0].activity, Activity::Idle { .. }), "another start time is another process");
    }
}
