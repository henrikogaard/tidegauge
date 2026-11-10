//! Agent hooks (ideas from disk-jockey, MIT): keep agents from piling up
//! booted devices, and shut down the simulators a session left behind.
//!
//! - `tidegauge hook pre-tool [--simulators N] [--emulators N]`: a Claude
//!   Code PreToolUse hook. It denies a call that would boot one more device
//!   than the cap, and prints nothing otherwise. It never prints "allow",
//!   because that would skip the user's own permission prompts, and it
//!   never blocks on its own failure.
//! - `tidegauge hook session-start` registers the session;
//!   `tidegauge hook session-end` removes it, and when no session is left
//!   shuts down the simulators that were not booted before the first one.
//!   Emulators are not shut down: killing a qemu process from here could
//!   lose an unsaved AVD state, so they are only counted.

use super::pressure::{parse_simctl_booted, Simulator};
use crate::locale::text as l;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Read};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SWAP_LIMIT: u64 = 2_000_000_000;
const STALE_SECS: u64 = 24 * 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Simulator,
    Emulator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    pub simulators: usize,
    pub emulators: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Caps { simulators: 2, emulators: 1 }
    }
}

fn flag(args: &[&str], name: &str) -> Option<usize> {
    let i = args.iter().position(|a| *a == name)?;
    args.get(i + 1)?.parse().ok()
}

pub fn caps_from(args: &[&str]) -> Caps {
    let d = Caps::default();
    Caps { simulators: flag(args, "--simulators").unwrap_or(d.simulators), emulators: flag(args, "--emulators").unwrap_or(d.emulators) }
}

pub fn session_arg(args: &[&str]) -> Option<String> {
    let i = args.iter().position(|a| *a == "--session")?;
    args.get(i + 1).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

/// Stdin as text, only when it is a pipe or file, and bounded in time.
fn read_stdin() -> String {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return String::new();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::stdin().lock().take(1 << 20).read_to_string(&mut s);
        let _ = tx.send(s);
    });
    rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
}

fn session_from_json(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v["session_id"].as_str().map(str::to_string).filter(|s| !s.is_empty())
}

// ---------- pre-tool ----------

/// Does this tool call boot a device? Decided from the text alone.
pub fn boot_kind(tool_name: &str, tool_input: &Value) -> Option<Device> {
    if tool_name == "Bash" {
        let cmd = tool_input["command"].as_str()?;
        return bash_boot(cmd);
    }
    if tool_name.contains("boot_sim") || tool_name.contains("build_run_sim") {
        return Some(Device::Simulator);
    }
    None
}

fn bash_boot(cmd: &str) -> Option<Device> {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let base = w.rsplit('/').next().unwrap_or(w);
        if base == "simctl" {
            // `simctl --set x boot ABC`: skip flags (and their value) up to the subcommand.
            let mut j = i + 1;
            while j < words.len() && words[j].starts_with('-') {
                j += 1;
            }
            // A flag's value may sit between the flag and the subcommand.
            for w2 in words.iter().skip(i + 1).take(4) {
                if *w2 == "boot" {
                    return Some(Device::Simulator);
                }
            }
            if words.get(j) == Some(&"boot") {
                return Some(Device::Simulator);
            }
        }
        if base == "emulator" && words.iter().skip(i + 1).any(|a| *a == "-avd" || a.starts_with('@')) {
            return Some(Device::Emulator);
        }
    }
    None
}

fn names_device(text: &str, booted: &[Simulator]) -> bool {
    booted.iter().any(|s| {
        text.contains(&s.udid) || {
            let mut from = 0;
            let mut found = false;
            while let Some(p) = text[from..].find(&s.name) {
                let start = from + p;
                let end = start + s.name.len();
                let before = text[..start].chars().next_back();
                let after = text[end..].chars().next();
                if !before.is_some_and(|c| c.is_alphanumeric()) && !after.is_some_and(|c| c.is_alphanumeric() || c == ' ') {
                    found = true;
                    break;
                }
                from = end;
            }
            found
        }
    })
}

/// The deny reason, or None when the call may go on (to the user's prompts).
pub fn decide(kind: Device, booted: &[Simulator], emulators: usize, caps: Caps, input_text: &str, pressure: bool) -> Option<String> {
    let mut reason = match kind {
        Device::Simulator => {
            if booted.len() < caps.simulators || names_device(input_text, booted) {
                return None;
            }
            let list = booted.iter().map(|s| format!("{} ({})", s.name, s.udid)).collect::<Vec<_>>().join(", ");
            format!(
                "{} ({} {} {}): {}. {}",
                l("Simulator cap reached", "Grensen for simulatorer er nådd"),
                booted.len(),
                l("of", "av"),
                caps.simulators,
                if list.is_empty() { "-".to_string() } else { list },
                l("Reuse one of these instead of booting another.", "Bruk en av disse i stedet for å starte en til."),
            )
        }
        Device::Emulator => {
            if emulators < caps.emulators {
                return None;
            }
            format!(
                "{} ({} {} {}). {}",
                l("Emulator cap reached", "Grensen for emulatorer er nådd"),
                emulators,
                l("of", "av"),
                caps.emulators,
                l("Reuse the running emulator instead of starting another.", "Bruk emulatoren som kjører i stedet for å starte en til."),
            )
        }
    };
    if pressure {
        reason.push(' ');
        reason.push_str(l(
            "Memory is under pressure: reuse booted devices and do not start parallel builds.",
            "Minnet er under press: bruk enhetene som allerede kjører, og ikke start parallelle bygg.",
        ));
    }
    Some(reason)
}

pub fn deny_json(reason: &str) -> String {
    json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}).to_string()
}

fn run_text(program: &str, args: &[&str], secs: u64) -> Option<String> {
    let mut c = std::process::Command::new(program);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).current_dir("/");
    let out = crate::packages::run_command(c, Duration::from_secs(secs)).ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn booted_simulators() -> Option<Vec<Simulator>> {
    let t = run_text("/usr/bin/xcrun", &["simctl", "list", "devices", "booted", "-j"], 5)?;
    serde_json::from_str::<Value>(&t).ok()?;
    Some(parse_simctl_booted(&t))
}

fn emulator_count() -> usize {
    let p = super::gate::Procs::read();
    if !p.ok {
        return 0;
    }
    p.list.iter().filter(|p| p.name.starts_with("qemu-system")).count()
}

fn high_pressure() -> bool {
    crate::metrics::swap_used().unwrap_or(0) > SWAP_LIMIT
}

/// `hook pre-tool`: the line to print, if any.
pub fn pre_tool(args: &[&str], stdin: &str) -> Option<String> {
    let v: Value = serde_json::from_str(stdin).ok()?;
    let tool = v["tool_name"].as_str()?;
    let input = &v["tool_input"];
    let kind = boot_kind(tool, input)?;
    let caps = caps_from(args);
    let text = input.to_string();
    let (booted, emulators) = match kind {
        Device::Simulator => (booted_simulators()?, 0),
        Device::Emulator => (Vec::new(), emulator_count()),
    };
    decide(kind, &booted, emulators, caps, &text, high_pressure()).map(|r| deny_json(&r))
}

pub fn run_pre_tool(args: &[&str]) -> i32 {
    if let Some(line) = pre_tool(args, &read_stdin()) {
        println!("{line}");
    }
    0
}

// ---------- sessions ----------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionFile {
    pub sessions: BTreeMap<String, u64>,
    pub baseline: Vec<String>,
}

impl SessionFile {
    fn parse(text: &str) -> Self {
        let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        SessionFile {
            sessions: v["sessions"].as_object().map(|o| o.iter().filter_map(|(k, t)| Some((k.clone(), t.as_u64()?))).collect()).unwrap_or_default(),
            baseline: v["baseline"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        }
    }

    fn to_json(&self) -> String {
        json!({"sessions": self.sessions, "baseline": self.baseline}).to_string()
    }

    fn drop_stale(&mut self, now: u64) {
        self.sessions.retain(|_, t| now.saturating_sub(*t) < STALE_SECS);
    }
}

/// Register a session. `baseline_now` is read only when no session is live.
pub fn add_session(file: &mut SessionFile, id: &str, now: u64, baseline_now: impl FnOnce() -> Vec<String>) {
    file.drop_stale(now);
    if file.sessions.is_empty() {
        file.baseline = baseline_now();
    }
    file.sessions.insert(id.to_string(), now);
}

/// Remove a session; the udids to shut down when it was the last one.
pub fn end_session(file: &mut SessionFile, id: &str, now: u64, booted_now: &[String]) -> Vec<String> {
    file.drop_stale(now);
    file.sessions.remove(id);
    if !file.sessions.is_empty() {
        return Vec::new();
    }
    let out = to_shut_down(&file.baseline, booted_now);
    file.baseline.clear();
    out
}

pub fn to_shut_down(baseline: &[String], booted_now: &[String]) -> Vec<String> {
    booted_now.iter().filter(|u| !baseline.contains(u)).cloned().collect()
}

fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("tidegauge-{}", unsafe { libc::getuid() })))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Read-modify-write the session file under an flock on `<path>.lock`.
pub fn with_file<T>(path: &Path, f: impl FnOnce(&mut SessionFile) -> T) -> Option<T> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(PathBuf::from(lock_name)).ok()?;
    // Bounded wait: poll a non-blocking lock for up to 3 s.
    let mut got = false;
    for _ in 0..30 {
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            got = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !got {
        return None;
    }
    let mut file = SessionFile::parse(&std::fs::read_to_string(path).unwrap_or_default());
    let out = f(&mut file);
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, file.to_json()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
    unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) };
    Some(out)
}

fn session_path() -> PathBuf {
    config_dir().join("hook-sessions.json")
}

fn booted_udids() -> Vec<String> {
    booted_simulators().unwrap_or_default().into_iter().map(|s| s.udid).collect()
}

/// `hook session-start`: register the session. Never fails the hook.
pub fn session_start(args: &[&str]) {
    let id = session_arg(args).or_else(|| session_from_json(&read_stdin()));
    let Some(id) = id else { return };
    with_file(&session_path(), |f| add_session(f, &id, unix_now(), booted_udids));
}

/// The extra SessionStart line when swap is high.
pub fn memory_line() -> Option<String> {
    high_pressure().then(|| {
        l(
            "Memory is under pressure (swap over 2 GB). Reuse booted devices and avoid parallel builds.",
            "Minnet er under press (over 2 GB swap). Bruk enhetene som allerede kjører, og unngå parallelle bygg.",
        )
        .to_string()
    })
}

/// `hook session-end`: remove the session and, for the last one, shut down
/// the simulators it left booted.
pub fn session_end(args: &[&str]) {
    let id = session_arg(args).or_else(|| session_from_json(&read_stdin()));
    let Some(id) = id else { return };
    let now = unix_now();
    let down = with_file(&session_path(), |f| {
        let was_empty_after = {
            let mut probe = f.clone();
            probe.drop_stale(now);
            probe.sessions.remove(&id);
            probe.sessions.is_empty()
        };
        let booted = if was_empty_after { booted_udids() } else { Vec::new() };
        end_session(f, &id, now, &booted)
    })
    .unwrap_or_default();
    for udid in down {
        let _ = run_text("/usr/bin/xcrun", &["simctl", "shutdown", &udid], 10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(u: &str, n: &str) -> Simulator {
        Simulator { udid: u.into(), name: n.into(), runtime: "iOS 18".into() }
    }
    fn two() -> Vec<Simulator> {
        vec![sim("AAA-1", "iPhone 16"), sim("BBB-2", "iPad Pro")]
    }

    #[test]
    fn unrelated_bash_is_no_boot() {
        assert_eq!(boot_kind("Bash", &json!({"command": "cargo test && xcrun simctl list devices"})), None);
        assert_eq!(boot_kind("Bash", &json!({"command": "xcrun simctl shutdown ABC"})), None);
        assert_eq!(boot_kind("Read", &json!({"file_path": "/x"})), None);
    }

    #[test]
    fn detects_simulator_emulator_and_mcp() {
        assert_eq!(boot_kind("Bash", &json!({"command": "xcrun simctl boot ABC"})), Some(Device::Simulator));
        assert_eq!(boot_kind("Bash", &json!({"command": "xcrun   simctl  --set /tmp/x boot ABC"})), Some(Device::Simulator));
        assert_eq!(boot_kind("Bash", &json!({"command": "emulator -avd Pixel_8"})), Some(Device::Emulator));
        assert_eq!(boot_kind("Bash", &json!({"command": "~/Library/Android/sdk/emulator/emulator @Pixel_8 -no-window"})), Some(Device::Emulator));
        assert_eq!(boot_kind("Bash", &json!({"command": "emulator -list-avds"})), None);
        assert_eq!(boot_kind("mcp__xcode__boot_sim", &json!({})), Some(Device::Simulator));
        assert_eq!(boot_kind("mcp__x__build_run_sim", &json!({})), Some(Device::Simulator));
    }

    #[test]
    fn malformed_input_prints_nothing() {
        let a: [&str; 0] = [];
        assert_eq!(pre_tool(&a, "not json"), None);
        assert_eq!(pre_tool(&a, "{}"), None);
        assert_eq!(pre_tool(&a, r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#), None);
    }

    #[test]
    fn denies_at_cap_and_lists_devices() {
        let r = decide(Device::Simulator, &two(), 0, Caps::default(), r#"{"command":"xcrun simctl boot CCC"}"#, false).unwrap();
        assert!(r.contains("iPhone 16 (AAA-1)") && r.contains("iPad Pro (BBB-2)"));
        let j: Value = serde_json::from_str(&deny_json(&r)).unwrap();
        assert_eq!(j["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(j["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    }

    #[test]
    fn allows_already_booted_and_under_cap() {
        assert_eq!(decide(Device::Simulator, &two(), 0, Caps::default(), "xcrun simctl boot AAA-1", false), None);
        assert_eq!(decide(Device::Simulator, &two(), 0, Caps::default(), r#"{"device":"iPad Pro"}"#, false), None);
        assert!(decide(Device::Simulator, &two(), 0, Caps::default(), r#"{"device":"iPad Pro Max"}"#, false).is_some());
        assert_eq!(decide(Device::Simulator, &two()[..1], 0, Caps::default(), "simctl boot CCC", false), None);
        assert_eq!(decide(Device::Simulator, &two(), 0, Caps { simulators: 3, emulators: 1 }, "simctl boot CCC", false), None);
    }

    #[test]
    fn emulator_cap_and_pressure() {
        assert!(decide(Device::Emulator, &[], 1, Caps::default(), "emulator -avd X", false).is_some());
        assert_eq!(decide(Device::Emulator, &[], 0, Caps::default(), "emulator -avd X", false), None);
        let plain = decide(Device::Simulator, &two(), 0, Caps::default(), "x", false).unwrap();
        let hot = decide(Device::Simulator, &two(), 0, Caps::default(), "x", true).unwrap();
        assert!(hot.len() > plain.len() && hot.starts_with(&plain));
    }

    #[test]
    fn caps_parse() {
        assert_eq!(caps_from(&["--simulators", "4", "--emulators", "0"]), Caps { simulators: 4, emulators: 0 });
        assert_eq!(caps_from(&["--simulators", "x"]), Caps::default());
    }

    #[test]
    fn session_add_remove_and_stale() {
        let mut f = SessionFile::default();
        add_session(&mut f, "a", 1000, || vec!["U1".into()]);
        add_session(&mut f, "b", 1010, || panic!("baseline only for the first session"));
        assert_eq!(f.baseline, vec!["U1".to_string()]);
        assert_eq!(f.sessions.len(), 2);
        // A day later both are stale: a new session starts over with a new baseline.
        add_session(&mut f, "c", 1010 + STALE_SECS + 5, || vec!["U9".into()]);
        assert_eq!(f.sessions.keys().collect::<Vec<_>>(), vec!["c"]);
        assert_eq!(f.baseline, vec!["U9".to_string()]);
    }

    #[test]
    fn last_session_shuts_down_only_new_simulators() {
        let mut f = SessionFile::default();
        add_session(&mut f, "a", 1000, || vec!["U1".into()]);
        add_session(&mut f, "b", 1001, Vec::new);
        let now = vec!["U1".to_string(), "U2".to_string()];
        assert!(end_session(&mut f, "a", 1002, &now).is_empty(), "another session remains");
        assert_eq!(end_session(&mut f, "b", 1003, &now), vec!["U2".to_string()]);
        assert!(f.sessions.is_empty() && f.baseline.is_empty());
        assert_eq!(to_shut_down(&["U1".into()], &["U1".into()]), Vec::<String>::new());
    }

    #[test]
    fn session_file_roundtrip_under_lock() {
        let dir = std::env::temp_dir().join(format!("tidegauge-hooks-test-{}-{}", std::process::id(), unix_now()));
        let path = dir.join("hook-sessions.json");
        with_file(&path, |f| add_session(f, "s1", 5000, || vec!["U1".into()])).unwrap();
        let got = with_file(&path, |f| f.clone()).unwrap();
        assert_eq!(got.sessions.get("s1"), Some(&5000));
        assert_eq!(got.baseline, vec!["U1".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
