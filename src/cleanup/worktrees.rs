//! The worktree auditor (FR-2, FR-3, FR-5): finds the git worktrees agents
//! made and gives each one state. Read-only; removal is `remove.rs`.

use super::gate::{self, Procs};
use super::git;
use super::{Candidate, Category, Fingerprint, Method, ScanConfig, Tier, WorktreeState};
use crate::locale::text as l;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Ignored entries that are only build output, caches or editor state;
/// `git worktree remove` deletes them for good, so the list stays narrow.
/// Any other ignored file (`.env`, local databases, dev-server state) makes
/// a worktree Review (Q-8).
pub const DISPOSABLE: [&str; 21] = [
    "node_modules", "target", ".build", "build", "dist", ".next", ".turbo", ".gradle", ".venv",
    "__pycache__", "DerivedData", ".cache", ".DS_Store", ".pytest_cache", ".mypy_cache",
    ".ruff_cache", "xcuserdata", ".eslintcache", ".parcel-cache", ".svelte-kit",
    ".claude/settings.local.json",
];

/// Whether an ignored path from `git status` is disposable: the whole path,
/// its top-level entry, or (for a file) its own name is on the list. A
/// listed name deeper inside some other folder does not count.
fn disposable(ignored: &str) -> bool {
    let path = ignored.trim_end_matches('/');
    let first = path.split('/').next().unwrap_or(path);
    let name = path.rsplit('/').next().unwrap_or(path);
    DISPOSABLE.contains(&path)
        || DISPOSABLE.contains(&first)
        || name == ".DS_Store"
        || name == "__pycache__"
        || name.ends_with(".pyc")
        || name.ends_with(".tsbuildinfo")
}

pub fn status_hash(s: &git::Status) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.changed.hash(&mut h);
    s.ignored.iter().filter(|i| !disposable(i)).collect::<Vec<_>>().hash(&mut h);
    h.finish()
}

const SKIP_DIRS: [&str; 6] = ["node_modules", "target", ".build", "build", "Pods", "DerivedData"];

/// Main repos (folders holding a `.git` folder) up to three levels below
/// each root, plus the main repos of Codex worktrees found anywhere.
pub fn find_repos(roots: &[PathBuf], home: &Path, cancel: &AtomicBool) -> Vec<PathBuf> {
    let mut repos: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || repos.len() > 2000 {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&dir) else { continue };
        if !meta.is_dir() {
            continue;
        }
        if std::fs::symlink_metadata(dir.join(".git")).is_ok_and(|m| m.is_dir()) {
            if !repos.contains(&dir) {
                repos.push(dir);
            }
            continue;
        }
        if depth >= 3 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if (name.starts_with('.') && name != ".worktrees") || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    for wt in codex_worktrees(home) {
        if let Some((main, true)) = git::link_target(&wt) {
            if main.join(".git").is_dir() && !repos.contains(&main) {
                repos.push(main);
            }
        }
    }
    repos.sort();
    repos
}

fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

fn codex_worktrees(home: &Path) -> Vec<PathBuf> {
    children(&home.join(".codex/worktrees")).iter().flat_map(|d| children(d)).collect()
}

/// When the folder was last touched: its newest commit, or the newest change
/// to it or anything at its top level.
pub fn last_used(path: &Path, commit: Option<i64>) -> Option<i64> {
    let mtime = |p: &Path| {
        std::fs::symlink_metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
        })
    };
    let mut newest = commit.into_iter().chain(mtime(path)).max();
    if let Ok(entries) = std::fs::read_dir(path) {
        for e in entries.flatten().take(500) {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if DISPOSABLE.contains(&name.as_ref()) || name == ".git" {
                continue;
            }
            newest = newest.max(mtime(&e.path()));
        }
    }
    newest
}

/// `gh` (when on) confirms a merged pull request for this exact commit.
fn gh_merged(repo: &Path, branch: &str, head: &str) -> Option<bool> {
    let gh = ["/opt/homebrew/bin/gh", "/usr/local/bin/gh"].into_iter().find(|p| Path::new(p).exists())?;
    let mut c = std::process::Command::new(gh);
    c.args(["pr", "list", "--head", branch, "--state", "merged", "--json", "number,headRefOid", "--limit", "5"])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let out = crate::packages::run_command(c, std::time::Duration::from_secs(15)).ok()?;
    if !out.status.success() {
        return None;
    }
    let prs: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
    Some(prs.iter().any(|p| p["headRefOid"].as_str() == Some(head)))
}

/// Facts about one linked worktree, gathered before it is judged.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub locked: bool,
    pub detached: bool,
    pub in_use: Option<String>,
    pub changed: u32,
    pub ignored_kept: Vec<String>,
    pub unpushed: Option<u32>,
    pub merged_local: bool,
    pub merged_gh: bool,
    pub recent: bool,
}

/// The state, its tier, and the reason to show (FR-3). First match wins.
pub fn classify(f: &Facts) -> (WorktreeState, Tier, String) {
    use WorktreeState::*;
    if f.locked {
        return (Review, Tier::Keep, l("Locked by git", "Låst av git").into());
    }
    if let Some(app) = &f.in_use {
        return (InUse, Tier::Keep, format!("{} {app}", l("In use by", "I bruk av")));
    }
    if f.changed > 0 {
        return (Review, Tier::Keep, crate::locale::count(f.changed as u64, "uncommitted change", "uncommitted changes", "endring som ikke er committet", "endringer som ikke er committet"));
    }
    if !f.ignored_kept.is_empty() {
        let names = f.ignored_kept.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        return (Review, Tier::Keep, format!("{} {names}", l("Has local files:", "Har lokale filer:")));
    }
    if let Some(n) = f.unpushed.filter(|n| *n > 0) {
        return (Review, Tier::Keep, crate::locale::count(n as u64, "unpushed commit", "unpushed commits", "commit som ikke er pushet", "commits som ikke er pushet"));
    }
    // A worktree made minutes ago sits on main with no commits yet, which
    // also reads as merged; only gh can tell a fresh merge from that.
    if f.merged_gh || (f.merged_local && !f.recent) {
        return (Removable, Tier::Safe, l("Merged, nothing local", "Flettet, ingenting lokalt").into());
    }
    if f.recent {
        return (Active, Tier::Keep, l("Touched in the last days", "Endret de siste dagene").into());
    }
    if f.detached {
        return (Review, Tier::Keep, l("Detached, not merged", "Frakoblet, ikke flettet").into());
    }
    (Review, Tier::Keep, l("Not merged", "Ikke flettet").into())
}

/// Every linked worktree of one main repo, judged.
pub fn audit_repo(repo: &Path, cfg: &ScanConfig, home: &Path, procs: &Procs) -> Result<Vec<Candidate>, String> {
    let listed = git::run(repo, &["worktree", "list", "--porcelain"])
        .map(|t| git::parse_worktree_list(&t))
        .ok_or_else(|| l("git did not answer in 10 s", "git svarte ikke innen 10 s").to_string())?;
    let repo_name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let base = git::base(repo);
    let now = super::now();
    let mut out = Vec::new();
    let mut prunable = 0;
    for w in listed.iter().skip(1) {
        if w.bare {
            continue;
        }
        if w.prunable {
            // git's own judgement; a worktree on an unmounted disk is left alone.
            if !w.path.starts_with("/Volumes") {
                prunable += 1;
            }
            continue;
        }
        if !w.path.exists() {
            continue;
        }
        let Ok(path) = std::fs::canonicalize(&w.path) else { continue };
        if gate::is_protected(&path, home) || !path.starts_with(home) {
            continue;
        }
        let Some(status) = git::status(&path) else {
            out.push(kept(&path, &repo_name, w, l("git did not answer", "git svarte ikke")));
            continue;
        };
        let head = w.head.clone();
        let commit = git::last_commit(&path);
        let last = last_used(&path, commit);
        let mut facts = Facts {
            locked: w.locked,
            detached: w.detached,
            in_use: if procs.ok { procs.user_of(&path) } else { Some(l("unknown", "ukjent").into()) },
            changed: status.changed.len() as u32,
            ignored_kept: status.ignored.iter().filter(|i| !disposable(i)).cloned().collect(),
            unpushed: git::unpushed(&path),
            recent: last.is_some_and(|t| now - t < cfg.active_days as i64 * 86_400),
            ..Default::default()
        };
        if let (Some(base), Some(h)) = (&base, &head) {
            facts.merged_local = git::is_ancestor(repo, h, base).unwrap_or(false);
        }
        if cfg.use_gh && !facts.merged_local || cfg.use_gh && facts.recent {
            if let (Some(b), Some(h)) = (&w.branch, &head) {
                facts.merged_gh = gh_merged(repo, b, h).unwrap_or(false);
            }
        }
        let (state, tier, why) = classify(&facts);
        let branch = w.branch.clone().unwrap_or_else(|| l("detached", "frakoblet").into());
        let mut c = Candidate::folder(Category::AgentWorktrees, path.clone(), format!("{repo_name} · {branch}"), tier);
        c.detail = why;
        c.state = Some(state);
        c.last_used = last;
        c.method = if tier == Tier::Keep { Method::None } else { Method::GitWorktree { main: repo.to_path_buf() } };
        c.fingerprint = Fingerprint { head: head.clone(), status: status_hash(&status), identity: gate::identity(&path) };
        if let Some(b) = &w.branch {
            c.facts.push(("branch_ref", b.clone()));
        }
        c.facts.extend([
            ("repo", repo_name.clone()),
            ("branch", branch),
            ("state", state.label().to_string()),
            ("base", base.clone().unwrap_or_default()),
            ("main", repo.display().to_string()),
            ("changes", facts.changed.to_string()),
            ("unpushed", facts.unpushed.map_or_else(|| "?".into(), |n| n.to_string())),
        ]);
        if w.locked {
            c.facts.push(("locked", "yes".into()));
        }
        out.push(c);
    }
    if prunable > 0 {
        out.push(Candidate {
            id: format!("prune:{}", repo.display()),
            category: Category::AgentWorktrees,
            title: format!(
                "{repo_name} · {}",
                crate::locale::count(prunable, "stale git record", "stale git records", "utdatert git-oppføring", "utdaterte git-oppføringer")
            ),
            detail: l("The folders are already gone", "Mappene er allerede borte").into(),
            paths: Vec::new(),
            watch: Vec::new(),
            bytes: Some(0),
            last_used: None,
            tier: Tier::Safe,
            method: Method::Prune { main: repo.to_path_buf() },
            lock: None,
            state: None,
            fingerprint: Fingerprint::default(),
            note: None,
            facts: Vec::new(),
        });
    }
    Ok(out)
}

fn kept(path: &Path, repo: &str, w: &git::Listed, why: &str) -> Candidate {
    let mut c = Candidate::folder(
        Category::AgentWorktrees,
        path.to_path_buf(),
        format!("{repo} · {}", w.branch.clone().unwrap_or_default()),
        Tier::Keep,
    );
    c.detail = why.to_string();
    c.state = Some(WorktreeState::Review);
    c.method = Method::None;
    c
}

/// Folders in the usual agent places whose git link is broken (FR-3 Orphan).
pub fn orphans(roots: &[PathBuf], home: &Path) -> Vec<Candidate> {
    let mut places: Vec<PathBuf> = codex_worktrees(home);
    for root in roots {
        for repo in children(root) {
            places.extend(children(&repo.join(".claude/worktrees")));
        }
        for repo in children(&root.join(".worktrees")) {
            places.extend(children(&repo));
        }
    }
    let mut out = Vec::new();
    for p in places {
        let Ok(p) = std::fs::canonicalize(&p) else { continue };
        let Some((_, false)) = git::link_target(&p) else { continue };
        if gate::validate(&p, home).is_err() {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Its repo may only be moved, renamed or on another disk, and its
        // work can't be checked without it: shown, never removed here (D-12).
        let mut c = Candidate::folder(Category::AgentWorktrees, p.clone(), name, Tier::Keep);
        c.detail = l("Its repo is gone or moved; check it in Finder", "Repoet er borte eller flyttet; sjekk den i Finder").into();
        c.state = Some(WorktreeState::Orphan);
        c.method = Method::None;
        c.last_used = last_used(&p, None);
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    fn cfg() -> ScanConfig {
        ScanConfig { roots: Vec::new(), active_days: 2, use_gh: false, include_downloads: false, build_idle_days: 30, only_worktrees: false }
    }

    fn audit(repo: &Path, home: &Path) -> Vec<Candidate> {
        audit_repo(repo, &cfg(), home, &Procs { list: Vec::new(), ok: true }).unwrap()
    }

    fn state_of(cands: &[Candidate], name: &str) -> WorktreeState {
        cands.iter().find(|c| c.title.ends_with(name)).unwrap().state.unwrap()
    }

    /// Make a worktree look untouched for a week.
    fn age(path: &Path) {
        let week = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86_400);
        let set = |p: &Path| {
            let f = std::fs::File::open(p).unwrap();
            f.set_modified(week).unwrap();
        };
        for e in std::fs::read_dir(path).unwrap().flatten() {
            if e.file_name() != ".git" {
                set(&e.path());
            }
        }
        set(path);
    }

    #[test]
    fn classify_rules_in_order() {
        let base = Facts::default();
        assert_eq!(classify(&Facts { locked: true, merged_local: true, ..base.clone() }).0, WorktreeState::Review);
        assert_eq!(classify(&Facts { in_use: Some("zsh".into()), merged_local: true, ..base.clone() }).0, WorktreeState::InUse);
        assert_eq!(classify(&Facts { changed: 1, merged_local: true, ..base.clone() }).0, WorktreeState::Review);
        assert_eq!(classify(&Facts { ignored_kept: vec![".env".into()], merged_local: true, ..base.clone() }).0, WorktreeState::Review);
        assert_eq!(classify(&Facts { unpushed: Some(2), merged_local: true, ..base.clone() }).0, WorktreeState::Review);
        assert_eq!(classify(&Facts { merged_local: true, ..base.clone() }).0, WorktreeState::Removable);
        assert_eq!(classify(&Facts { merged_local: true, recent: true, ..base.clone() }).0, WorktreeState::Active);
        assert_eq!(classify(&Facts { merged_gh: true, recent: true, ..base.clone() }).0, WorktreeState::Removable);
        assert_eq!(classify(&Facts { recent: true, ..base.clone() }).0, WorktreeState::Active);
        assert_eq!(classify(&base).0, WorktreeState::Review);
    }

    #[test]
    fn disposable_names() {
        assert!(disposable("node_modules/"));
        assert!(disposable("target/debug/x"));
        assert!(disposable("src/__pycache__/"));
        assert!(!disposable(".env"));
        assert!(!disposable("data/local.db"));
        assert!(!disposable(".wrangler/state/d1.sqlite"), "dev-server state is local data");
        assert!(!disposable("data/build/x.db"), "a listed name deeper down does not count");
        assert!(!disposable("npm-debug.log"));
    }

    #[test]
    fn finds_claude_codex_and_dot_worktrees() {
        let home = temp("find");
        let dev = home.join("Dev");
        std::fs::create_dir_all(&dev).unwrap();
        let a = repo(&dev, "alpha");
        worktree(&a, "one");
        let b = repo(&dev, "beta");
        let dot = dev.join(".worktrees/beta/two");
        git(&b, &["worktree", "add", "-q", "-b", "two", dot.to_str().unwrap()]);
        // A repo outside the roots, with a worktree under ~/.codex/worktrees.
        let elsewhere = home.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let c = repo(&elsewhere, "gamma");
        let codex = home.join(".codex/worktrees/abcd/gamma");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        git(&c, &["worktree", "add", "-q", "-b", "three", codex.to_str().unwrap()]);

        let repos = find_repos(&[dev.clone()], &home, &AtomicBool::new(false));
        assert!(repos.contains(&a) && repos.contains(&b) && repos.contains(&c), "{repos:?}");
        let mut titles: Vec<String> = repos.iter().flat_map(|r| audit(r, &home)).map(|c| c.title).collect();
        titles.sort();
        assert_eq!(titles, vec!["alpha · one", "beta · two", "gamma · three"]);
    }

    #[test]
    fn classify_against_real_git() {
        let home = temp("classify");
        let r = repo(&home, "r");
        let merged = worktree(&r, "merged");
        merge(&r, "merged");
        age(&merged);
        let dirty = worktree(&r, "dirty");
        merge(&r, "dirty");
        std::fs::write(dirty.join("README.md"), "changed\n").unwrap();
        let env = worktree(&r, "env");
        merge(&r, "env");
        std::fs::write(env.join(".env"), "SECRET=1\n").unwrap();
        age(&env);
        let unpushed = worktree(&r, "unpushed");
        git(&unpushed, &["push", "-q", "-u", "origin", "unpushed"]);
        std::fs::write(unpushed.join("more.txt"), "x\n").unwrap();
        git(&unpushed, &["add", "."]);
        git(&unpushed, &["commit", "-qm", "more"]);
        worktree(&r, "recent");
        let built = worktree(&r, "built");
        merge(&r, "built");
        std::fs::create_dir_all(built.join("node_modules/x")).unwrap();
        age(&built);

        let c = audit(&r, &home);
        assert_eq!(state_of(&c, "merged"), WorktreeState::Removable);
        assert_eq!(state_of(&c, "dirty"), WorktreeState::Review);
        assert_eq!(state_of(&c, "env"), WorktreeState::Review);
        assert_eq!(state_of(&c, "unpushed"), WorktreeState::Review);
        assert_eq!(state_of(&c, "recent"), WorktreeState::Active);
        assert_eq!(state_of(&c, "built"), WorktreeState::Removable, "node_modules is build output");
        let m = c.iter().find(|c| c.title.ends_with("merged")).unwrap();
        assert_eq!(m.tier, Tier::Safe);
        assert!(matches!(m.method, Method::GitWorktree { .. }));
    }

    #[test]
    fn ignored_dev_state_keeps_a_merged_worktree() {
        let home = temp("wrangler");
        let r = repo(&home, "r");
        std::fs::write(r.join(".git/info/exclude"), ".wrangler/\n").unwrap();
        let w = worktree(&r, "d1");
        merge(&r, "d1");
        std::fs::create_dir_all(w.join(".wrangler/state")).unwrap();
        std::fs::write(w.join(".wrangler/state/d1.sqlite"), "data").unwrap();
        age(&w);
        let c = audit(&r, &home);
        assert_eq!(state_of(&c, "d1"), WorktreeState::Review, "git worktree remove would delete it for good");
    }

    #[test]
    fn in_use_when_cwd_inside() {
        let home = temp("inuse");
        let r = repo(&home, "r");
        let w = worktree(&r, "busy");
        merge(&r, "busy");
        age(&w);
        let procs = Procs { list: vec![gate::Proc { pid: 1, ppid: 0, name: "zsh".into(), cwd: w.join("src"), args: vec![] }], ok: true };
        let c = audit_repo(&r, &cfg(), &home, &procs).unwrap();
        assert_eq!(state_of(&c, "busy"), WorktreeState::InUse);
        let blind = audit_repo(&r, &cfg(), &home, &Procs::default()).unwrap();
        assert_eq!(state_of(&blind, "busy"), WorktreeState::InUse, "fails closed");
    }

    #[test]
    fn locked_and_orphan() {
        let home = temp("orphan");
        let dev = home.join("Dev");
        std::fs::create_dir_all(&dev).unwrap();
        let r = repo(&dev, "r");
        let l = worktree(&r, "held");
        git(&r, &["worktree", "lock", l.to_str().unwrap()]);
        let c = audit(&r, &home);
        assert_eq!(state_of(&c, "held"), WorktreeState::Review);
        // Break a worktree's link by deleting its record.
        let gone = worktree(&r, "gone");
        std::fs::remove_dir_all(r.join(".git/worktrees/gone")).unwrap();
        let o = orphans(&[dev], &home);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].paths[0], gone);
        assert_eq!(o[0].state, Some(WorktreeState::Orphan));
        assert_eq!(o[0].tier, Tier::Keep, "an orphan's work can't be checked");
        assert!(matches!(o[0].method, Method::None));
    }

    #[test]
    fn prunable_records_counted() {
        let home = temp("prune");
        let r = repo(&home, "r");
        let w = worktree(&r, "lost");
        std::fs::remove_dir_all(&w).unwrap();
        let c = audit(&r, &home);
        let p = c.iter().find(|c| matches!(c.method, Method::Prune { .. })).unwrap();
        assert!(p.title.contains(&crate::locale::count(1, "stale git record", "stale git records", "utdatert git-oppføring", "utdaterte git-oppføringer")));
        assert_eq!(p.tier, Tier::Safe);
    }
}
