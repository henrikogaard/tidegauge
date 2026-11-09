//! "Is it really files?" (FR-15): space and memory that no folder shows —
//! swap, local APFS snapshots (a staged macOS update among them), and
//! booted simulators. Read-only; the only action is shutting a simulator
//! down, which the page asks about first.

use super::{Candidate, Category, Method, Tier, ToolAction};
use crate::locale::text as l;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Pressure {
    pub swap_used: Option<u64>,
    pub snapshots: Option<u32>,
    pub update_snapshots: u32,
    pub simulators: Vec<Simulator>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simulator {
    pub udid: String,
    pub name: String,
    pub runtime: String,
}

impl Pressure {
    pub fn is_empty(&self) -> bool {
        self.swap_used.unwrap_or(0) < 512 << 20 && self.snapshots.unwrap_or(0) == 0 && self.simulators.is_empty()
    }
}

fn output(program: &str, args: &[&str], secs: u64) -> Option<String> {
    let mut c = std::process::Command::new(program);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .current_dir("/");
    let out = crate::packages::run_command(c, Duration::from_secs(secs)).ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn read() -> Pressure {
    let (all, update) = output("/usr/bin/tmutil", &["listlocalsnapshots", "/"], 10)
        .map(|t| parse_snapshots(&t))
        .map_or((None, 0), |(a, u)| (Some(a), u));
    let simulators = if std::path::Path::new("/Library/Developer/CommandLineTools").exists()
        || std::path::Path::new("/Applications/Xcode.app").exists()
    {
        output("/usr/bin/xcrun", &["simctl", "list", "devices", "booted", "-j"], 30)
            .map(|t| parse_simctl_booted(&t))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Pressure {
        swap_used: crate::metrics::swap_used(),
        snapshots: all,
        update_snapshots: update,
        simulators,
    }
}

/// All local snapshots, and those a staged macOS update made.
pub fn parse_snapshots(text: &str) -> (u32, u32) {
    let names: Vec<&str> = text.lines().map(str::trim).filter(|l| l.starts_with("com.apple.")).collect();
    let update = names.iter().filter(|n| n.starts_with("com.apple.os.update-")).count();
    (names.len() as u32, update as u32)
}

pub fn parse_simctl_booted(json: &str) -> Vec<Simulator> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(runtimes) = v["devices"].as_object() {
        for (runtime, devices) in runtimes {
            for d in devices.as_array().into_iter().flatten() {
                if d["state"].as_str() != Some("Booted") {
                    continue;
                }
                let (Some(udid), Some(name)) = (d["udid"].as_str(), d["name"].as_str()) else { continue };
                let runtime = runtime.rsplit('.').next().unwrap_or(runtime).replace('-', " ");
                out.push(Simulator { udid: udid.into(), name: name.into(), runtime });
            }
        }
    }
    out
}

/// A booted simulator as something to shut down. It frees memory, not disk,
/// so it counts no bytes and is never ticked by a level.
pub fn simulator_candidates(p: &Pressure) -> Vec<Candidate> {
    p.simulators
        .iter()
        .map(|s| Candidate {
            id: format!("sim:{}", s.udid),
            category: Category::Simulators,
            title: format!("{} · {}", s.name, l("booted", "startet")),
            detail: format!("{} · {}", s.runtime, l("shutting down frees memory, keeps its data", "å slå av frigjør minne og beholder dataene")),
            paths: Vec::new(),
            watch: Vec::new(),
            bytes: Some(0),
            last_used: None,
            tier: Tier::Keep,
            method: Method::Tool(ToolAction::ShutdownSimulator(s.udid.clone())),
            lock: None,
            state: None,
            fingerprint: Default::default(),
            note: None,
            facts: Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tmutil_update_snapshots() {
        let t = "Snapshots for disk /:\ncom.apple.TimeMachine.2026-11-09-101010.local\ncom.apple.os.update-ABCDEF\ncom.apple.os.update-123456\n";
        assert_eq!(parse_snapshots(t), (3, 2));
        assert_eq!(parse_snapshots("Snapshots for disk /:\n"), (0, 0));
    }

    #[test]
    fn parses_simctl_booted() {
        let j = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-18-2":[{"udid":"AAA","name":"iPhone 16","state":"Booted"},{"udid":"BBB","name":"iPad","state":"Shutdown"}]}}"#;
        let s = parse_simctl_booted(j);
        assert_eq!(s, vec![Simulator { udid: "AAA".into(), name: "iPhone 16".into(), runtime: "iOS 18 2".into() }]);
        assert!(parse_simctl_booted("not json").is_empty());
    }
}
