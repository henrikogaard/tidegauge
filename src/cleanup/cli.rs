//! Cleanup for agents and scripts (FR-25, D-15), without the window:
//!
//! - `tidegauge cleanup status`: free space and what a Balanced cleanup frees.
//! - `tidegauge cleanup worktrees [--merged] [--apply]`: list worktrees and
//!   their state; `--apply` removes the Removable ones (after the recheck).
//! - `tidegauge hook session-start`: one line for an agent's SessionStart
//!   hook when free space is under the limit; silent otherwise.
//! - `tidegauge hook pre-tool` and `hook session-end`: cap booted simulators
//!   and emulators, and shut down the simulators a session left behind
//!   (see `hooks`).
//! - `tidegauge mcp`: an MCP server on stdio with `disk_status`,
//!   `list_worktrees` and `remove_merged_worktrees` (needs `apply: true`).
//!
//! Tidegauge never edits an agent's config; Settings shows the snippets.

use super::{auto, remove, scan_blocking, Category, Candidate, ScanConfig, WorktreeState};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize};

/// Handle a command-line mode; None when `args` is not one, so the app starts.
pub fn run(args: &[String]) -> Option<i32> {
    let words: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match words.as_slice() {
        ["cleanup", "status", ..] => Some(print(&status())),
        ["cleanup", "worktrees", rest @ ..] => {
            let merged = rest.contains(&"--merged");
            let apply = rest.contains(&"--apply");
            Some(print(&worktrees(merged, apply)))
        }
        ["hook", "session-start", rest @ ..] => {
            if let Some(line) = session_warning() {
                println!("{line}");
            }
            if let Some(line) = super::hooks::memory_line() {
                println!("{line}");
            }
            super::hooks::session_start(rest);
            Some(0)
        }
        ["hook", "session-end", rest @ ..] => {
            super::hooks::session_end(rest);
            Some(0)
        }
        ["hook", "pre-tool", rest @ ..] => Some(super::hooks::run_pre_tool(rest)),
        ["mcp", ..] => {
            serve(std::io::stdin().lock(), std::io::stdout().lock());
            Some(0)
        }
        ["cleanup", ..] | ["hook", ..] => {
            eprintln!("usage: tidegauge cleanup status | cleanup worktrees [--merged] [--apply] | hook session-start | hook pre-tool [--simulators N] [--emulators N] | hook session-end | mcp");
            Some(2)
        }
        _ => None,
    }
}

fn print(v: &Value) -> i32 {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    0
}

fn gb(b: u64) -> f64 {
    (b as f64 / 1e8).round() / 10.
}

pub fn status() -> Value {
    let disk = crate::metrics::disk_stats();
    let cfg = ScanConfig::current();
    let idle = cfg.build_idle_days;
    let scan = scan_blocking(cfg);
    let mut by: Vec<(Category, u64)> = Vec::new();
    let now = super::now();
    let ready = super::without_nested(scan.candidates.iter().filter(|c| c.selectable() && super::Level::Balanced.ticks(c, idle, now)).collect());
    for c in &ready {
        match by.iter_mut().find(|(k, _)| *k == c.category) {
            Some((_, b)) => *b += c.bytes.unwrap_or(0),
            None => by.push((c.category, c.bytes.unwrap_or(0))),
        }
    }
    json!({
        "free_gb": gb(disk.free),
        "total_gb": gb(disk.total),
        "ready_gb": gb(ready.iter().filter_map(|c| c.bytes).sum()),
        "ready_by_category": by.iter().map(|(k, b)| json!({"category": format!("{k:?}"), "gb": gb(*b)})).collect::<Vec<_>>(),
    })
}

fn worktree_json(c: &Candidate) -> Value {
    json!({
        "path": c.paths.first().map(|p| p.display().to_string()),
        "title": c.title,
        "state": c.state.map(|s| format!("{s:?}")),
        "why": c.detail,
        "gb": c.bytes.map(gb),
        "lock": c.lock,
    })
}

pub fn worktrees(merged_only: bool, apply: bool) -> Value {
    let scan = scan_blocking(ScanConfig { only_worktrees: true, ..ScanConfig::current() });
    let all: Vec<&Candidate> = scan.candidates.iter().filter(|c| c.category == Category::AgentWorktrees && c.state.is_some()).collect();
    let listed: Vec<Value> = all
        .iter()
        .filter(|c| !merged_only || c.state == Some(WorktreeState::Removable))
        .map(|c| worktree_json(c))
        .collect();
    if !apply {
        return json!({ "worktrees": listed, "applied": false });
    }
    if !crate::settings::Settings::load().cleanup_agent_remove {
        return json!({
            "worktrees": listed,
            "applied": false,
            "error": "Removing is off. Turn on \"Let agents remove merged worktrees\" in Tidegauge Settings > General > Cleanup.",
        });
    }
    let plan = auto::merged_plan(&scan.candidates);
    let r = remove::run(plan, &AtomicBool::new(false), &AtomicUsize::new(0));
    json!({
        "worktrees": listed,
        "applied": true,
        "removed": r.removed,
        "skipped": r.skipped.iter().map(|(w, why)| json!({"title": w, "why": why})).collect::<Vec<_>>(),
        "freed_gb": gb(r.freed_bytes),
    })
}

/// The SessionStart line, or none when there is room.
pub fn session_warning() -> Option<String> {
    let limit = crate::settings::Settings::load().cleanup_auto_limit_gb.max(1) as u64 * 1_000_000_000;
    let free = crate::metrics::disk_stats().free;
    (free < limit).then(|| {
        format!(
            "Disk space is low: {} GB free. Reuse booted simulators, avoid parallel builds, and run `tidegauge cleanup worktrees --merged --apply` from a main checkout to remove merged worktrees.",
            gb(free)
        )
    })
}

fn tools() -> Value {
    json!([
        {"name": "disk_status", "description": "Free disk space and what a balanced cleanup would free, by category.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "list_worktrees", "description": "Agent worktrees with their state (Removable, Review, Active, InUse, Orphan) and size.", "inputSchema": {"type": "object", "properties": {"merged_only": {"type": "boolean"}}}},
        {"name": "remove_merged_worktrees", "description": "Remove worktrees that are merged, clean, pushed and unused, with git; branches stay. Without apply=true it only lists them.", "inputSchema": {"type": "object", "properties": {"apply": {"type": "boolean"}}}}
    ])
}

/// One JSON-RPC message in, at most one out.
pub fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "tidegauge", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let args = &msg["params"]["arguments"];
            let out = match msg["params"]["name"].as_str() {
                Some("disk_status") => status(),
                Some("list_worktrees") => worktrees(args["merged_only"].as_bool().unwrap_or(false), false),
                Some("remove_merged_worktrees") => worktrees(true, args["apply"].as_bool() == Some(true)),
                _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "unknown tool"}})),
            };
            json!({"content": [{"type": "text", "text": out.to_string()}]})
        }
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn serve(mut input: impl BufRead, mut output: impl Write) {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        // A line that is not UTF-8 or not JSON is skipped, not fatal.
        let line = String::from_utf8_lossy(&buf);
        let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else { continue };
        if let Some(reply) = handle(&msg) {
            let _ = writeln!(output, "{reply}");
            let _ = output.flush();
        }
    }
}

/// The snippets Settings shows for an agent's config.
pub fn snippets() -> (String, String) {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "/Applications/Tidegauge.app/Contents/MacOS/tidegauge".into());
    let hook = |sub: &str, timeout: u32| json!({"type": "command", "command": format!("\"{exe}\" hook {sub}"), "timeout": timeout});
    let hooks = json!({"hooks": {
        "SessionStart": [{"hooks": [hook("session-start", 10)]}],
        "PreToolUse": [{"matcher": "Bash|mcp__.*", "hooks": [hook("pre-tool", 5)]}],
        "SessionEnd": [{"hooks": [hook("session-end", 15)]}],
    }});
    let mcp = json!({"mcpServers": {"tidegauge": {"command": exe, "args": ["mcp"]}}});
    (
        serde_json::to_string_pretty(&hooks).unwrap_or_default(),
        serde_json::to_string_pretty(&mcp).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_lists_tools_and_rejects_unknown() {
        let init = handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let list = handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["disk_status", "list_worktrees", "remove_merged_worktrees"]);
        let bad = handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "rm_rf"}})).unwrap();
        assert!(bad["error"].is_object());
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none(), "no reply to a notification");
    }

    #[test]
    fn mcp_survives_bad_lines() {
        let input: &[u8] = b"\xff\xfe not json\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n";
        let mut out = Vec::new();
        serve(input, &mut out);
        assert!(String::from_utf8(out).unwrap().contains("\"id\":7"));
    }

    #[test]
    fn cli_ignores_app_arguments() {
        assert_eq!(run(&["tidegauge".into()]), None);
        assert_eq!(run(&["tidegauge".into(), "-psn_0_1".into()]), None);
        assert_eq!(run(&["tidegauge".into(), "cleanup".into(), "nope".into()]), Some(2));
    }
}
