//! `tidegauge ports [--json]` (FR-9) and the MCP tools `whats_on_port` and
//! `stop_port` (FR-10, FR-12, D-4). Output never holds command-line
//! arguments or environment (NFR-2).

use super::Listener;
use crate::locale::text as l;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn now_listeners() -> Vec<Listener> {
    let procs = super::basic_procs();
    let projects = crate::projects::detect_projects(&procs, Duration::ZERO);
    let sessions = crate::agents::sessions(&procs, &projects);
    super::listeners(&procs, &sessions, Duration::ZERO)
}

fn home_short(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

pub fn listener_json(x: &Listener) -> Value {
    json!({
        "port": x.port,
        "binds": x.binds.iter().map(|b| b.addr().to_string()).collect::<Vec<_>>(),
        "exposed": x.exposed(),
        "pid": x.process.pid,
        "process": x.name,
        "executable": x.process.exe_path,
        "started_unix": x.process.start_unix.floor() as i64,
        "project": x.project,
        "dev_server": x.dev_server,
        "launcher": {"kind": x.launcher.kind(), "name": x.launcher.label()},
    })
}

/// The table `tidegauge ports` prints.
pub fn table(rows: &[Listener]) -> String {
    let head = [
        l("PORT", "PORT"),
        l("ADDRESS", "ADRESSE"),
        "PID",
        l("PROCESS", "PROSESS"),
        l("STARTED BY", "STARTET AV"),
        l("FOLDER", "MAPPE"),
    ];
    let mut lines: Vec<[String; 6]> = vec![head.map(str::to_string)];
    for x in rows {
        let mark = if x.warn_exposed() { "!" } else { "" };
        lines.push([
            format!("{}{mark}", x.port),
            x.binds_text(),
            x.process.pid.to_string(),
            x.name.clone(),
            x.launcher.label(),
            x.project.as_deref().map(home_short).unwrap_or_else(|| "-".into()),
        ]);
    }
    let widths: Vec<usize> = (0..5).map(|i| lines.iter().map(|r| r[i].chars().count()).max().unwrap_or(0)).collect();
    let mut out = String::new();
    for r in &lines {
        for (i, cell) in r.iter().enumerate() {
            if i < 5 {
                out.push_str(&format!("{cell:<w$}  ", w = widths[i]));
            } else {
                out.push_str(cell);
            }
        }
        out.push('\n');
    }
    if rows.is_empty() {
        out.push_str(l("Nothing is listening.", "Ingenting lytter."));
        out.push('\n');
    }
    if rows.iter().any(Listener::warn_exposed) {
        out.push_str(&format!("\n! {}\n", super::exposed_warning()));
    }
    out
}

/// `tidegauge ports …`; the exit code.
pub fn run(args: &[&str]) -> i32 {
    let rows = now_listeners();
    if args.contains(&"--json") {
        let v = json!({ "ports": rows.iter().map(listener_json).collect::<Vec<_>>() });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    } else {
        print!("{}", table(&rows));
    }
    0
}

pub fn whats_on_port(rows: &[Listener], port: u16) -> Value {
    let found: Vec<Value> = rows.iter().filter(|x| x.port == port).map(listener_json).collect();
    if found.is_empty() {
        json!({"port": port, "listeners": [], "note": "Nothing of yours is listening on this port. Ports owned by macOS or other users are not visible to Tidegauge."})
    } else {
        let exposed = rows.iter().any(|x| x.port == port && x.warn_exposed());
        json!({"port": port, "listeners": found, "warning": exposed.then(super::exposed_warning)})
    }
}

/// The project folder of the calling agent: the MCP server runs in its
/// folder. None outside a project (say, the home folder), so an agent there
/// can stop nothing.
fn caller_root(cwd: &Path) -> Option<PathBuf> {
    let root = crate::projects::project_root(&cwd.to_string_lossy())?;
    std::fs::canonicalize(root).ok()
}

/// Why `stop_port` will not stop this listener for an agent in `caller`, or
/// None when it may.
pub fn stop_refusal(x: &Listener, caller: Option<&Path>) -> Option<String> {
    if let Some(block) = crate::process_control::blocked(&x.process) {
        return Some(block.reason().to_string());
    }
    if !x.dev_server {
        return Some(format!("{} is not a dev server in a project folder ({}).", x.name, x.launcher.label()));
    }
    let project = x.project.as_deref().and_then(|p| std::fs::canonicalize(p).ok());
    match (project, caller) {
        (Some(project), Some(caller)) if project.starts_with(caller) => None,
        _ => Some(format!(
            "{} runs in {}, outside this agent's folder; stop it yourself in Tidegauge.",
            x.name,
            x.project.as_deref().map(home_short).unwrap_or_default()
        )),
    }
}

pub fn stop_target(x: &Listener) -> crate::projects::StopTarget {
    let cwd = crate::processes::proc_cwd(x.process.pid);
    let cwd = std::fs::canonicalize(&cwd).map(|p| p.to_string_lossy().into_owned()).unwrap_or(cwd);
    crate::projects::StopTarget {
        server: crate::engine::DevServer {
            pid: x.process.pid,
            name: x.name.clone(),
            exe_path: x.process.exe_path.clone(),
            cwd,
            port: x.port,
            up_since: x.process.start_unix,
            ..Default::default()
        },
        project_dir: x.project.clone().unwrap_or_default(),
        ports: vec![x.port],
    }
}

/// `stop_port`: refuses unless the setting is on (FR-12), then stops only
/// dev servers inside the caller's folder (D-4), each re-verified (FR-11).
pub fn stop_port(rows: &[Listener], port: u16, allowed: bool, caller_cwd: &Path) -> Value {
    if !allowed {
        return json!({"port": port, "stopped": [], "error": "Stopping is off. Turn on \"Let agents stop dev servers\" in Tidegauge Settings > General > Cleanup."});
    }
    let on_port: Vec<&Listener> = rows.iter().filter(|x| x.port == port).collect();
    if on_port.is_empty() {
        return json!({"port": port, "stopped": [], "error": "Nothing of yours is listening on this port."});
    }
    let caller = caller_root(caller_cwd);
    let mut refused = Vec::new();
    let mut targets = Vec::new();
    for x in on_port {
        match stop_refusal(x, caller.as_deref()) {
            Some(why) => refused.push(json!({"pid": x.process.pid, "process": x.name, "why": why})),
            None => targets.push(stop_target(x)),
        }
    }
    let results = crate::projects::stop_targets(targets);
    let stopped: Vec<Value> = results
        .iter()
        .filter(|r| matches!(r.outcome, crate::projects::StopOutcome::Exited | crate::projects::StopOutcome::AlreadyStopped))
        .map(|r| json!({"pid": r.pid, "process": r.name, "message": r.message()}))
        .collect();
    for r in results.iter().filter(|r| !matches!(r.outcome, crate::projects::StopOutcome::Exited | crate::projects::StopOutcome::AlreadyStopped)) {
        refused.push(json!({"pid": r.pid, "process": r.name, "why": r.message()}));
    }
    json!({"port": port, "stopped": stopped, "refused": refused})
}

pub fn mcp_tools() -> Vec<Value> {
    vec![
        json!({"name": "whats_on_port", "description": "What is listening on a TCP port on this Mac: process, PID, project folder, who started it (agent, Terminal, editor, Docker, launchd) and whether other devices on the network can reach it.", "inputSchema": {"type": "object", "properties": {"port": {"type": "integer", "minimum": 1, "maximum": 65535}}, "required": ["port"]}}),
        json!({"name": "stop_port", "description": "Stop the dev server listening on a port, if it runs in this agent's own project folder. Needs \"Let agents stop dev servers\" on in Tidegauge Settings.", "inputSchema": {"type": "object", "properties": {"port": {"type": "integer", "minimum": 1, "maximum": 65535}}, "required": ["port"]}}),
    ]
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, "whats_on_port" | "stop_port")
}

/// One call of a ports MCP tool (`is_tool`).
pub fn mcp_call(name: &str, args: &Value) -> Result<Value, String> {
    let Some(port) = args["port"].as_u64().filter(|p| (1..=65535).contains(p)).map(|p| p as u16) else {
        return Err("port must be a number from 1 to 65535".into());
    };
    let rows = now_listeners();
    Ok(match name {
        "whats_on_port" => whats_on_port(&rows, port),
        _ => {
            let allowed = crate::settings::Settings::load().agents_stop_servers;
            let cwd = std::env::current_dir().unwrap_or_default();
            stop_port(&rows, port, allowed, &cwd)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{Bind, Launcher};
    use crate::processes::ProcInfo;

    fn listener(port: u16, bind: &str, project: Option<&str>, dev: bool) -> Listener {
        Listener {
            port,
            binds: vec![Bind::of(bind.parse().unwrap())],
            process: ProcInfo { pid: 4242, ppid: 1, uid: unsafe { libc::getuid() }, comm: "node".into(), exe_path: "/opt/homebrew/bin/node".into(), start_unix: 1_700_000_000., ..Default::default() },
            name: "node".into(),
            project: project.map(str::to_string),
            dev_server: dev,
            launcher: Launcher::Terminal("Terminal".into()),
        }
    }

    #[test]
    fn table_lists_rows_and_explains_the_mark() {
        let out = table(&[listener(5173, "0.0.0.0", Some("/tmp/app"), true), listener(8080, "127.0.0.1", None, false)]);
        assert!(out.contains("5173!") && out.contains("*") && out.contains("/tmp/app"));
        assert!(out.lines().any(|line| line.starts_with("8080 ") && line.contains("127.0.0.1")));
        assert!(out.contains("--host 127.0.0.1"), "the warning names the fix");
        assert!(!table(&[listener(8080, "127.0.0.1", None, true)]).contains('!'));
    }

    #[test]
    fn json_has_no_arguments_or_environment() {
        let v = listener_json(&listener(3000, "::", Some("/tmp/app"), true));
        assert_eq!(v["port"], 3000);
        assert_eq!(v["exposed"], true);
        assert_eq!(v["launcher"]["kind"], "terminal");
        let text = v.to_string();
        assert!(!text.contains("args") && !text.contains("env"));
    }

    #[test]
    fn whats_on_port_finds_or_explains() {
        let rows = [listener(3000, "::", Some("/tmp/app"), true)];
        assert_eq!(whats_on_port(&rows, 3000)["listeners"][0]["pid"], 4242);
        assert!(whats_on_port(&rows, 3000)["warning"].is_string());
        assert_eq!(whats_on_port(&rows, 4000)["listeners"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn stop_port_needs_the_setting_and_the_callers_folder() {
        let dir = crate::cleanup::test_support::temp("ports-stop");
        let app = dir.join("app");
        let other = dir.join("other");
        std::fs::create_dir_all(app.join("web")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(app.join("package.json"), "{}").unwrap();
        std::fs::write(app.join("web/package.json"), "{}").unwrap();
        std::fs::write(other.join("package.json"), "{}").unwrap();
        let rows = [listener(3000, "127.0.0.1", Some(&app.join("web").to_string_lossy()), true)];
        let off = stop_port(&rows, 3000, false, &app);
        assert!(off["error"].as_str().unwrap().contains("Let agents stop dev servers"));
        assert!(stop_port(&rows, 3001, true, &app)["error"].is_string());
        // Inside the caller's project (a subfolder): allowed by the rules.
        assert_eq!(stop_refusal(&rows[0], caller_root(&app).as_deref()), None);
        // Another project: refused, and nothing is signalled.
        let r = stop_port(&rows, 3000, true, &other);
        assert_eq!(r["stopped"].as_array().unwrap().len(), 0);
        assert!(r["refused"][0]["why"].as_str().unwrap().contains("outside"));
        // A caller outside any project (no marker) owns nothing.
        assert_eq!(caller_root(&dir), None);
        assert!(stop_refusal(&rows[0], None).is_some());
        // Not a dev server: refused.
        let plain = listener(3000, "127.0.0.1", Some(&app.to_string_lossy()), false);
        assert!(stop_refusal(&plain, caller_root(&app).as_deref()).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bad_port_arguments_are_errors() {
        assert!(mcp_call("whats_on_port", &json!({})).is_err());
        assert!(mcp_call("stop_port", &json!({"port": 70000})).is_err());
        assert!(is_tool("stop_port") && !is_tool("disk_status"));
    }
}
