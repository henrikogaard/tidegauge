//! The remover (FR-4, FR-12): runs a confirmed plan one item at a time,
//! checking each again first. A failure leaves the rest untouched and is
//! never retried on its own.

use super::gate::{self, Procs};
use super::{Candidate, Method, ToolAction};
use crate::locale::text as l;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Measured gain in free space.
    pub freed_bytes: u64,
    /// Sizes of what went to the Trash (freed once the Trash is emptied).
    pub trashed_bytes: u64,
    pub removed: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub errors: Vec<(String, String)>,
}

fn free_now() -> u64 {
    crate::metrics::disk_stats().free
}

/// Remove every candidate in `plan`. `done` counts finished items for the
/// progress line.
pub fn run(plan: Vec<Candidate>, cancel: &AtomicBool, done: &AtomicUsize) -> Report {
    let mut report = Report::default();
    let Some(home) = gate::home() else {
        report.errors.push((String::new(), l("Your home folder could not be resolved.", "Hjemmemappen kunne ikke finnes.").into()));
        return report;
    };
    let before = free_now();
    let delete_branches = crate::settings::Settings::load().cleanup_delete_branches;
    for c in plan {
        if cancel.load(Ordering::Relaxed) {
            report.skipped.push((c.title.clone(), l("Stopped", "Stoppet").into()));
            continue;
        }
        let procs = Procs::read();
        match gate::recheck(&c, &home, &procs).and_then(|()| apply(&c)) {
            Ok(()) => {
                if matches!(c.method, Method::Trash | Method::TrashWorktree { .. }) {
                    report.trashed_bytes += c.bytes.unwrap_or(0);
                }
                let mut title = c.title.clone();
                if delete_branches && delete_branch(&c) {
                    title = format!("{title} ({})", l("branch deleted", "grenen slettet"));
                }
                report.removed.push(title);
            }
            Err(why) => report.skipped.push((c.title.clone(), why)),
        }
        done.fetch_add(1, Ordering::Relaxed);
    }
    report.freed_bytes = free_now().saturating_sub(before);
    report
}

/// After a merged worktree is gone, delete its local branch with
/// `git branch -d`, which refuses a branch git does not see as merged.
/// The remote branch is never touched (FR-22).
pub fn delete_branch(c: &Candidate) -> bool {
    let Method::GitWorktree { main } = &c.method else { return false };
    let Some((_, branch)) = c.facts.iter().find(|(k, _)| *k == "branch_ref") else { return false };
    if branch.starts_with('-') || branch.is_empty() {
        return false;
    }
    super::git::write(main, &["branch", "-d", "--", branch])
}

fn apply(c: &Candidate) -> Result<(), String> {
    match &c.method {
        Method::GitWorktree { main } => {
            // Never --force: git refuses a tree with changes or new files.
            let ok = super::git::write(main, &["worktree", "remove", c.paths[0].to_str().unwrap_or("")]);
            if ok && !c.paths[0].exists() {
                Ok(())
            } else {
                Err(l("git refused to remove it; it may have changed.", "git nektet å fjerne den; den kan ha endret seg.").into())
            }
        }
        Method::TrashWorktree { main } => {
            crate::move_to_applications::trash(&c.paths[0])?;
            // The record now points at nothing; prune drops it. The branch
            // and its commits stay.
            super::git::write(main, &["worktree", "prune"]);
            Ok(())
        }
        Method::Prune { main } => {
            if super::git::write(main, &["worktree", "prune"]) {
                Ok(())
            } else {
                Err(l("git worktree prune failed.", "git worktree prune mislyktes.").into())
            }
        }
        Method::Trash => {
            for p in &c.paths {
                crate::move_to_applications::trash(p)?;
            }
            Ok(())
        }
        Method::KnownCache(cache) => crate::caches::clear(cache),
        Method::Tool(action) => tool(action),
        Method::None => Err(l("Kept items are never removed.", "Elementer som beholdes fjernes aldri.").into()),
    }
}

fn tool(action: &ToolAction) -> Result<(), String> {
    let mut c = match action {
        ToolAction::GoModCache => {
            let go = ["/opt/homebrew/bin/go", "/usr/local/go/bin/go", "/usr/local/bin/go"]
                .into_iter()
                .find(|p| std::path::Path::new(p).exists())
                .ok_or_else(|| l("Go was not found.", "Fant ikke Go.").to_string())?;
            // Pin the folder the row showed; go would otherwise clean
            // whatever GOMODCACHE points to.
            let home = gate::home().ok_or_else(|| l("Your home folder could not be resolved.", "Hjemmemappen kunne ikke finnes.").to_string())?;
            let mut c = std::process::Command::new(go);
            c.args(["clean", "-modcache"]).env("GOMODCACHE", home.join("go/pkg/mod")).env("GOFLAGS", "");
            c
        }
        ToolAction::ShutdownSimulator(udid) => {
            let mut c = std::process::Command::new("/usr/bin/xcrun");
            c.args(["simctl", "shutdown", udid]);
            c
        }
    };
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .current_dir("/");
    match crate::packages::run_command(c, Duration::from_secs(120)) {
        Ok(o) if o.status.success() => Ok(()),
        _ => Err(l("The command failed or timed out.", "Kommandoen mislyktes eller fikk tidsavbrudd.").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::worktrees::audit_repo;
    use super::super::{ScanConfig, WorktreeState};
    use super::*;

    fn removable(r: &std::path::Path, home: &std::path::Path, name: &str) -> Candidate {
        let cfg = ScanConfig { roots: vec![], active_days: 2, use_gh: false, include_downloads: false, build_idle_days: 30, only_worktrees: false };
        // Recent mtimes would make it Active; age the folder.
        let c = audit_repo(r, &ScanConfig { active_days: 0, ..cfg }, home, &Procs { list: vec![], ok: true }).unwrap();
        let c = c.into_iter().find(|c| c.title.ends_with(name)).unwrap();
        assert_eq!(c.state, Some(WorktreeState::Removable));
        c
    }

    #[test]
    fn remove_keeps_branch_and_drops_listing() {
        let home = temp("remove");
        let r = repo(&home, "r");
        let w = worktree(&r, "done");
        merge(&r, "done");
        let c = removable(&r, &home, "done");
        let p = Procs { list: vec![], ok: true };
        gate::recheck(&c, &home, &p).unwrap();
        apply(&c).unwrap();
        assert!(!w.exists());
        let list = git(&r, &["worktree", "list"]);
        assert!(!list.contains("done"), "{list}");
        assert!(git(&r, &["branch", "--list", "done"]).contains("done"), "branch stays");
    }

    #[test]
    fn branch_delete_only_when_merged() {
        let home = temp("branch");
        let r = repo(&home, "r");
        worktree(&r, "merged");
        merge(&r, "merged");
        let c = removable(&r, &home, "merged");
        apply(&c).unwrap();
        assert!(delete_branch(&c));
        assert!(!git(&r, &["branch", "--list", "merged"]).contains("merged"));
        // An unmerged branch is refused by git -d.
        let w = worktree(&r, "open");
        git(&r, &["worktree", "remove", w.to_str().unwrap()]);
        let mut fake = c.clone();
        fake.facts = vec![("branch_ref", "open".into())];
        assert!(!delete_branch(&fake));
        assert!(git(&r, &["branch", "--list", "open"]).contains("open"));
    }

    #[test]
    fn recheck_refuses_after_new_commit_or_file() {
        let home = temp("recheck");
        let r = repo(&home, "r");
        let w = worktree(&r, "a");
        merge(&r, "a");
        let c = removable(&r, &home, "a");
        let p = Procs { list: vec![], ok: true };
        std::fs::write(w.join("new.txt"), "x").unwrap();
        assert!(gate::recheck(&c, &home, &p).is_err(), "new file");
        git(&w, &["add", "."]);
        git(&w, &["commit", "-qm", "later"]);
        assert!(gate::recheck(&c, &home, &p).is_err(), "new commit");
    }

    #[test]
    fn recheck_refuses_after_folder_swap_and_new_process() {
        let home = temp("swap");
        let r = repo(&home, "r");
        let w = worktree(&r, "b");
        merge(&r, "b");
        let c = removable(&r, &home, "b");
        let busy = Procs { list: vec![gate::Proc { pid: 9, name: "node".into(), cwd: w.clone(), args: vec![] }], ok: true };
        assert!(gate::recheck(&c, &home, &busy).is_err(), "process inside");
        assert!(gate::recheck(&c, &home, &Procs::default()).is_err(), "blind fails closed");
        let moved = home.join("moved");
        std::fs::rename(&w, &moved).unwrap();
        std::fs::create_dir(&w).unwrap();
        assert!(gate::recheck(&c, &home, &Procs { list: vec![], ok: true }).is_err(), "swapped folder");
    }

    #[test]
    fn dirty_worktree_can_be_trashed_by_hand_only_when_safe() {
        let home = temp("manual");
        let r = repo(&home, "r");
        let w = worktree(&r, "wip");
        std::fs::write(w.join("draft.txt"), "x").unwrap();
        let cfg = ScanConfig { roots: vec![], active_days: 0, use_gh: false, include_downloads: false, build_idle_days: 30, only_worktrees: false };
        let none = Procs { list: vec![], ok: true };
        let c = audit_repo(&r, &cfg, &home, &none).unwrap().into_iter().find(|c| c.title.ends_with("wip")).unwrap();
        assert_eq!(c.state, Some(WorktreeState::Review));
        assert!(!c.selectable(), "never in a normal cleanup");
        let t = crate::cleanup_panel::manual_target(&c).expect("a Review worktree can be trashed by hand");
        assert!(matches!(t.method, Method::TrashWorktree { .. }));
        assert_eq!(crate::cleanup_panel::confirm_name(&t), "wip");
        gate::recheck(&t, &home, &none).unwrap();
        let busy = Procs { list: vec![gate::Proc { pid: 9, name: "zsh".into(), cwd: w.clone(), args: vec![] }], ok: true };
        assert!(gate::recheck(&t, &home, &busy).is_err(), "in use");
        assert!(gate::recheck(&t, &home, &Procs::default()).is_err(), "blind fails closed");
        // In use at scan time: not offered at all.
        let used = audit_repo(&r, &cfg, &home, &busy).unwrap().into_iter().find(|c| c.title.ends_with("wip")).unwrap();
        assert!(crate::cleanup_panel::manual_target(&used).is_none());
        // Locked by git: not offered.
        git(&r, &["worktree", "lock", w.to_str().unwrap()]);
        let locked = audit_repo(&r, &cfg, &home, &none).unwrap().into_iter().find(|c| c.title.ends_with("wip")).unwrap();
        assert!(crate::cleanup_panel::manual_target(&locked).is_none());
        git(&r, &["worktree", "unlock", w.to_str().unwrap()]);
        // Swapped folder: refused.
        std::fs::rename(&w, home.join("moved")).unwrap();
        std::fs::create_dir(&w).unwrap();
        assert!(gate::recheck(&t, &home, &none).is_err(), "swapped folder");
    }

    #[test]
    fn git_refuses_dirty_even_if_recheck_passed() {
        let home = temp("dirty");
        let r = repo(&home, "r");
        let w = worktree(&r, "c");
        merge(&r, "c");
        let c = removable(&r, &home, "c");
        std::fs::write(w.join("late.txt"), "x").unwrap();
        assert!(apply(&c).is_err());
        assert!(w.join("late.txt").exists());
    }
}
