//! Automatic cleanup (FR-23, FR-24, D-14): off by default. When switched
//! on, a background thread removes only Safe items whose method can be
//! undone or is a merged, clean worktree, after the same recheck as a
//! manual cleanup, at most once every 30 minutes, and says what went.

use super::{remove, scan_blocking, Candidate, Category, Method, ScanConfig, Tier, WorktreeState};
use crate::locale::text as l;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Mutex;

const EVERY: i64 = 30 * 60;

static LAST_AUTO: Mutex<i64> = Mutex::new(0);
static LAST_MERGE: Mutex<i64> = Mutex::new(0);
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Messages for the notification center, taken by the app's main loop.
pub fn drain_notes() -> Vec<String> {
    std::mem::take(&mut *NOTES.lock().unwrap())
}

/// What an automatic cleanup may take: Safe tier, nothing waiting, and
/// only the Trash or a merged, clean, pushed, unused worktree. Never a tool
/// command, never Rebuild or Keep.
pub fn auto_plan(cands: &[Candidate]) -> Vec<Candidate> {
    let ok: Vec<&Candidate> = cands
        .iter()
        .filter(|c| c.tier == Tier::Safe && c.selectable())
        .filter(|c| match &c.method {
            Method::Trash => true,
            Method::GitWorktree { .. } => c.state == Some(WorktreeState::Removable),
            _ => false,
        })
        .collect();
    super::without_nested(ok).into_iter().cloned().collect()
}

/// The second step (FR-35, D-19): build output that goes to the Trash, from
/// projects idle `idle_days` or more, oldest first, until `need` bytes are
/// covered. Nothing waiting, nothing unmeasured.
pub fn ladder_plan(cands: &[Candidate], need: u64, now: i64, idle_days: i64) -> Vec<Candidate> {
    let mut ok: Vec<&Candidate> = cands
        .iter()
        .filter(|c| c.category == Category::BuildOutput && matches!(c.method, Method::Trash) && c.tier != Tier::Keep)
        .filter(|c| c.lock.is_none() && c.bytes.is_some_and(|b| b > 0))
        .filter(|c| c.last_used.is_some_and(|t| now - t >= idle_days * 86_400))
        .collect();
    ok.sort_by_key(|c| c.last_used);
    let mut out = Vec::new();
    let mut got = 0u64;
    for c in super::without_nested(ok) {
        if got >= need {
            break;
        }
        got += c.bytes.unwrap_or(0);
        out.push(c.clone());
    }
    out
}

/// Removable worktrees only, for clean-after-merge.
pub fn merged_plan(cands: &[Candidate]) -> Vec<Candidate> {
    auto_plan(cands).into_iter().filter(|c| matches!(c.method, Method::GitWorktree { .. })).collect()
}

fn due(last: &Mutex<i64>, now: i64) -> bool {
    let mut l = last.lock().unwrap();
    if now - *l < EVERY {
        return false;
    }
    *l = now;
    true
}

fn note(what: &str, r: &remove::Report) {
    if r.removed.is_empty() {
        return;
    }
    let mut m = format!(
        "{what}: {} · {} {}",
        crate::locale::count(r.removed.len() as u64, "item", "items", "element", "elementer"),
        l("freed", "frigjort"),
        crate::engine::fmt_disk_bytes(r.freed_bytes as f64)
    );
    if r.trashed_bytes > 0 {
        m.push_str(&format!(" · {} {}", crate::engine::fmt_disk_bytes(r.trashed_bytes as f64), l("in the Trash", "i papirkurven")));
    }
    NOTES.lock().unwrap().push(m);
}

fn tick() {
    let s = crate::settings::Settings::load();
    let now = super::now();
    if s.cleanup_after_merge && due(&LAST_MERGE, now) {
        let cfg = ScanConfig { only_worktrees: true, ..ScanConfig::current() };
        let plan = merged_plan(&scan_blocking(cfg).candidates);
        if !plan.is_empty() {
            let r = remove::run(plan, &AtomicBool::new(false), &AtomicUsize::new(0));
            note(l("Removed merged worktrees", "Fjernet flettede arbeidstrær"), &r);
        }
    }
    let low = crate::metrics::disk_stats().free < s.cleanup_auto_limit_gb.max(1) as u64 * 1_000_000_000;
    let limit = s.cleanup_auto_limit_gb.max(1) as u64 * 1_000_000_000;
    if s.cleanup_auto && low && due(&LAST_AUTO, now) {
        let cands = scan_blocking(ScanConfig::current()).candidates;
        let plan = auto_plan(&cands);
        let mut got = 0;
        if !plan.is_empty() {
            let r = remove::run(plan, &AtomicBool::new(false), &AtomicUsize::new(0));
            got = r.freed_bytes + r.trashed_bytes;
            note(l("Cleaned up by itself", "Ryddet av seg selv"), &r);
        }
        // Trashed bytes count toward the limit: they are gone once the
        // Trash is emptied, which the notification says.
        let free = crate::metrics::disk_stats().free;
        let need = limit.saturating_sub(free).saturating_sub(got);
        if s.cleanup_auto_build && need > 0 {
            let plan = ladder_plan(&cands, need, now, 14);
            if !plan.is_empty() {
                let r = remove::run(plan, &AtomicBool::new(false), &AtomicUsize::new(0));
                note(l("Cleared old build output", "Fjernet gamle byggresultater"), &r);
            }
        }
    }
}

/// Start the background check; it wakes every five minutes and does
/// nothing unless a switch is on.
pub fn start() {
    let _ = std::thread::Builder::new().name("cleanup-auto".into()).spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(120));
        loop {
            tick();
            std::thread::sleep(std::time::Duration::from_secs(300));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::{Category, Candidate, ToolAction};
    use super::*;
    use std::path::PathBuf;

    fn c(id: &str, tier: Tier, method: Method) -> Candidate {
        let mut c = Candidate::folder(Category::PackageCaches, PathBuf::from(id), id.into(), tier);
        c.method = method;
        c.bytes = Some(1);
        c
    }

    #[test]
    fn ladder_takes_old_build_output_oldest_first_until_enough() {
        let now = 100 * 86_400;
        let b = |id: &str, days: i64, bytes: u64| {
            let mut c = Candidate::folder(Category::BuildOutput, PathBuf::from(id), id.into(), Tier::Rebuild);
            c.last_used = Some(now - days * 86_400);
            c.bytes = Some(bytes);
            c
        };
        let mut locked = b("/locked", 90, 9);
        locked.lock = Some("x".into());
        let mut keep = b("/keep", 90, 9);
        keep.tier = Tier::Keep;
        let cands = vec![b("/young", 3, 50), b("/old", 60, 5), b("/older", 80, 5), b("/mid", 20, 5), locked, keep, c("/cache", Tier::Safe, Method::Trash)];
        let ids = |need| ladder_plan(&cands, need, now, 14).into_iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(8), vec!["/older", "/old"], "oldest first, stops once enough");
        assert_eq!(ids(100), vec!["/older", "/old", "/mid"], "never under 14 days idle");
        assert!(ids(0).is_empty());
    }

    #[test]
    fn auto_takes_only_safe_undoable_items() {
        let mut review = c("/w2", Tier::Safe, Method::GitWorktree { main: PathBuf::from("/r") });
        review.state = Some(WorktreeState::Review);
        let mut merged = c("/w1", Tier::Safe, Method::GitWorktree { main: PathBuf::from("/r") });
        merged.state = Some(WorktreeState::Removable);
        let mut locked = c("/l", Tier::Safe, Method::Trash);
        locked.lock = Some("x".into());
        let cands = vec![
            c("/t", Tier::Safe, Method::Trash),
            c("/r", Tier::Rebuild, Method::Trash),
            c("/k", Tier::Keep, Method::None),
            c("/g", Tier::Safe, Method::Tool(ToolAction::GoModCache)),
            merged,
            review,
            locked,
        ];
        let ids: Vec<String> = auto_plan(&cands).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["/t", "/w1"]);
        let ids: Vec<String> = merged_plan(&cands).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["/w1"]);
    }
}
