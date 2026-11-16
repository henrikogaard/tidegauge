//! Who started a listener (FR-2, FR-13): walk up the parent chain and name
//! the first thing a person would recognise.

use crate::agents::{Agent, Session};
use crate::locale::text as l;
use crate::processes::{bundle_root, ProcInfo};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launcher {
    Agent(Agent),
    /// An agent session that has ended (slice 4).
    EndedSession(Agent),
    Terminal(String),
    Editor(String),
    Docker,
    /// A launchd job, with its label.
    Launchd(String),
    /// The listener is an app, or runs inside one.
    App(String),
    Unknown,
}

impl Launcher {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Agent(_) => "agent",
            Self::EndedSession(_) => "ended_session",
            Self::Terminal(_) => "terminal",
            Self::Editor(_) => "editor",
            Self::Docker => "docker",
            Self::Launchd(_) => "launchd",
            Self::App(_) => "app",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Agent(agent) => agent.name().to_string(),
            Self::EndedSession(agent) => format!("{} ({})", agent.name(), l("ended", "avsluttet")),
            Self::Terminal(name) | Self::Editor(name) | Self::App(name) => name.clone(),
            Self::Docker => "Docker".into(),
            Self::Launchd(label) => format!("launchd · {label}"),
            Self::Unknown => l("Unknown", "Ukjent").into(),
        }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, Self::Agent(_) | Self::EndedSession(_))
    }
}

const TERMINALS: &[&str] = &["Terminal", "iTerm", "iTerm2", "Ghostty", "Warp", "WezTerm", "Alacritty", "kitty", "Hyper", "Tabby", "Rio"];
const EDITORS: &[&str] = &[
    "Code", "Visual Studio Code", "Visual Studio Code - Insiders", "Cursor", "Windsurf", "Zed", "Zed Preview", "Xcode", "Nova",
    "Sublime Text", "IntelliJ IDEA", "IntelliJ IDEA CE", "WebStorm", "PyCharm", "PyCharm CE", "RustRover", "GoLand",
    "PhpStorm", "RubyMine", "CLion", "Android Studio", "Fleet",
];

/// The app an executable belongs to. An interpreter's helper bundle inside
/// a framework (Python.framework/…/Python.app) is not an app.
fn bundle_name(exe: &str) -> Option<String> {
    let bundle = bundle_root(exe)?;
    if bundle.contains(".framework/") {
        return None;
    }
    Path::new(&bundle).file_stem().map(|s| s.to_string_lossy().into_owned())
}

fn file_name(p: &ProcInfo) -> &str {
    Path::new(&p.exe_path).file_name().and_then(|n| n.to_str()).unwrap_or(p.comm.as_str())
}

/// Docker Desktop forwards container ports from these processes.
pub fn is_docker(p: &ProcInfo) -> bool {
    let name = file_name(p);
    name.starts_with("com.docker") || name.starts_with("vpnkit") || name == "docker-proxy"
        || bundle_name(&p.exe_path).is_some_and(|b| b == "Docker" || b == "Docker Desktop")
}

fn from_bundle(name: String) -> Launcher {
    if TERMINALS.contains(&name.as_str()) {
        Launcher::Terminal(name)
    } else if EDITORS.contains(&name.as_str()) || name.starts_with("IntelliJ") {
        Launcher::Editor(name)
    } else if name == "Docker" || name == "Docker Desktop" {
        Launcher::Docker
    } else {
        Launcher::App(name)
    }
}

/// What the launcher walk needs besides the process list.
#[derive(Default)]
pub struct Context {
    /// pid -> agent, for every process inside a live session's tree.
    pub in_session: HashMap<i32, Agent>,
    /// pid + start time -> agent, for leftovers of ended sessions.
    pub ended: HashMap<(i32, u64), Agent>,
    /// pid -> launchd job label (apps left out).
    pub launchd: HashMap<i32, String>,
}

impl Context {
    pub fn from_sessions(sessions: &[Session]) -> Self {
        let mut in_session = HashMap::new();
        for s in sessions {
            for p in &s.tree {
                in_session.insert(p.pid, s.agent);
            }
        }
        Context { in_session, ..Default::default() }
    }
}

/// Start time rounded to milliseconds, as a map key.
pub fn start_key(start_unix: f64) -> u64 {
    (start_unix * 1000.).round().max(0.) as u64
}

pub fn launcher(p: &ProcInfo, by_pid: &HashMap<i32, &ProcInfo>, cx: &Context) -> Launcher {
    if is_docker(p) {
        return Launcher::Docker;
    }
    if let Some(agent) = cx.in_session.get(&p.pid) {
        return Launcher::Agent(*agent);
    }
    if let Some(agent) = cx.ended.get(&(p.pid, start_key(p.start_unix))) {
        return Launcher::EndedSession(*agent);
    }
    if let Some(label) = cx.launchd.get(&p.pid) {
        return Launcher::Launchd(label.clone());
    }
    // Nearest ancestor inside an app bundle, or a launchd job on the way up.
    let mut current = p.ppid;
    for _ in 0..32 {
        if current <= 1 {
            break;
        }
        let Some(parent) = by_pid.get(&current) else { break };
        if let Some(name) = bundle_name(&parent.exe_path) {
            return from_bundle(name);
        }
        if let Some(label) = cx.launchd.get(&parent.pid) {
            return Launcher::Launchd(label.clone());
        }
        if parent.ppid == current {
            break;
        }
        current = parent.ppid;
    }
    match bundle_name(&p.exe_path) {
        Some(name) => from_bundle(name),
        None => Launcher::Unknown,
    }
}

/// `launchctl list`: "PID\tStatus\tLabel" rows. Apps are launchd jobs too
/// ("application.…"); they are named by their bundle instead.
pub fn parse_launchctl(text: &str) -> HashMap<i32, String> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let pid: i32 = parts.next()?.trim().parse().ok()?;
            let label = parts.nth(1)?.trim();
            (pid > 1 && !label.is_empty() && !label.starts_with("application.")).then(|| (pid, label.to_string()))
        })
        .collect()
}

static LAUNCHD: Mutex<Option<(Instant, HashMap<i32, String>)>> = Mutex::new(None);

/// The user's launchd jobs, read at most once a minute.
pub fn launchd_jobs() -> HashMap<i32, String> {
    let mut cache = LAUNCHD.lock().unwrap();
    if let Some((at, jobs)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return jobs.clone();
        }
    }
    let jobs = std::process::Command::new("/bin/launchctl")
        .arg("list")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_launchctl(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    *cache = Some((Instant::now(), jobs.clone()));
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, ppid: i32, path: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            exe_path: path.into(),
            comm: Path::new(path).file_name().unwrap().to_string_lossy().into(),
            start_unix: 1_700_000_000.,
            ..Default::default()
        }
    }

    fn walk(procs: &[ProcInfo], pid: i32, cx: &Context) -> Launcher {
        let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
        launcher(by_pid[&pid], &by_pid, cx)
    }

    #[test]
    fn terminal_editor_and_app_ancestors() {
        let procs = [
            proc(10, 1, "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
            proc(11, 10, "/bin/zsh"),
            proc(12, 11, "/opt/homebrew/bin/node"),
            proc(20, 1, "/Applications/Visual Studio Code.app/Contents/MacOS/Electron"),
            proc(21, 20, "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper"),
            proc(22, 21, "/usr/bin/python3"),
            proc(30, 1, "/Applications/Spotify.app/Contents/MacOS/Spotify"),
        ];
        let cx = Context::default();
        assert_eq!(walk(&procs, 12, &cx), Launcher::Terminal("Terminal".into()));
        assert_eq!(walk(&procs, 22, &cx), Launcher::Editor("Visual Studio Code".into()));
        assert_eq!(walk(&procs, 30, &cx), Launcher::App("Spotify".into()));
    }

    #[test]
    fn agent_session_wins_over_the_terminal_it_runs_in() {
        let procs = [
            proc(10, 1, "/Applications/Ghostty.app/Contents/MacOS/ghostty"),
            proc(11, 10, "/Users/me/.local/bin/claude"),
            proc(12, 11, "/opt/homebrew/bin/node"),
        ];
        let mut cx = Context::default();
        cx.in_session.insert(11, Agent::ClaudeCode);
        cx.in_session.insert(12, Agent::ClaudeCode);
        assert_eq!(walk(&procs, 12, &cx), Launcher::Agent(Agent::ClaudeCode));
        let mut cx = Context::default();
        cx.ended.insert((12, start_key(1_700_000_000.)), Agent::Codex);
        assert_eq!(walk(&procs, 12, &cx), Launcher::EndedSession(Agent::Codex));
    }

    #[test]
    fn docker_launchd_and_orphans() {
        let procs = [
            proc(40, 1, "/Applications/Docker.app/Contents/MacOS/com.docker.backend"),
            proc(50, 1, "/opt/homebrew/opt/postgresql@16/bin/postgres"),
            proc(60, 1, "/opt/homebrew/bin/node"),
            proc(70, 1, "/opt/homebrew/Frameworks/Python.framework/Versions/3.13/Resources/Python.app/Contents/MacOS/Python"),
        ];
        let mut cx = Context::default();
        cx.launchd.insert(50, "homebrew.mxcl.postgresql@16".into());
        assert_eq!(walk(&procs, 40, &cx), Launcher::Docker);
        assert_eq!(walk(&procs, 50, &cx), Launcher::Launchd("homebrew.mxcl.postgresql@16".into()));
        assert_eq!(walk(&procs, 60, &cx), Launcher::Unknown);
        assert_eq!(walk(&procs, 70, &cx), Launcher::Unknown, "framework Python is not an app");
    }

    #[test]
    fn launchctl_rows_skip_apps_and_jobs_without_a_pid() {
        let text = "PID\tStatus\tLabel\n812\t0\thomebrew.mxcl.redis\n-\t0\tcom.apple.idle\n901\t0\tapplication.com.spotify.client.123\n";
        let jobs = parse_launchctl(text);
        assert_eq!(jobs.get(&812).map(String::as_str), Some("homebrew.mxcl.redis"));
        assert_eq!(jobs.len(), 1);
    }
}
