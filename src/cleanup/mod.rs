//! Cleanup: find what agent work and developer tools leave behind, and
//! remove what is provably safe after you confirm. See
//! `docs/design/cleanup.md` for the requirements (FR-n) and decisions (D-n).
//!
//! Nothing here removes anything during a scan. Removal happens only in
//! `remove::run`, for a plan the user confirmed, and each item is checked
//! again right before it goes (`gate::recheck`).

pub mod agent;
pub mod auto;
pub mod cli;
pub mod gate;
pub mod git;
pub mod leftovers;
pub mod pressure;
pub mod remove;
pub mod worktrees;

use crate::locale::text as l;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    AgentWorktrees,
    XcodeBuild,
    Simulators,
    BuildOutput,
    PackageCaches,
    Logs,
    AgentScratch,
    Downloads,
    Archives,
    AgentHistory,
    AppLeftovers,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Self::AgentWorktrees,
        Self::XcodeBuild,
        Self::Simulators,
        Self::BuildOutput,
        Self::PackageCaches,
        Self::Logs,
        Self::AgentScratch,
        Self::Downloads,
        Self::Archives,
        Self::AgentHistory,
        Self::AppLeftovers,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::AgentWorktrees => l("Agent worktrees", "Agent-arbeidstrær"),
            Self::XcodeBuild => l("Xcode build data", "Xcode-byggdata"),
            Self::Simulators => l("Simulators", "Simulatorer"),
            Self::BuildOutput => l("Project build output", "Byggresultater i prosjekter"),
            Self::PackageCaches => l("Package caches", "Pakkebufre"),
            Self::Logs => l("Logs and crash reports", "Logger og krasjrapporter"),
            Self::AgentScratch => l("Agent scratch folders", "Agentenes kladdemapper"),
            Self::Downloads => l("Downloads and installers", "Nedlastinger og installasjonsfiler"),
            Self::Archives => l("Backups and archives", "Sikkerhetskopier og arkiver"),
            Self::AgentHistory => l("Agent history", "Agenthistorikk"),
            Self::AppLeftovers => l("Leftovers of removed apps", "Rester etter fjernede apper"),
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::AgentWorktrees => "icons/git-compare-arrows.svg",
            Self::XcodeBuild => "icons/hammer.svg",
            Self::Simulators => "icons/smartphone.svg",
            Self::BuildOutput => "icons/folder-code.svg",
            Self::PackageCaches => "icons/package.svg",
            Self::Logs => "icons/file-text.svg",
            Self::AgentScratch => "icons/bot.svg",
            Self::Downloads => "icons/download.svg",
            Self::Archives => "icons/layers.svg",
            Self::AgentHistory => "icons/clock.svg",
            Self::AppLeftovers => "icons/app-window.svg",
        }
    }
}

/// Safe: a pure cache or provably finished work. Rebuild: costs a download
/// or a build. Keep: holds real data; shown, never removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Safe,
    Rebuild,
    Keep,
}

/// How far a cleanup goes (FR-10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Level {
    Safe,
    #[default]
    Balanced,
    Deep,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => l("Safe", "Trygg"),
            Self::Balanced => l("Balanced", "Balansert"),
            Self::Deep => l("Deep", "Grundig"),
        }
    }
    /// Whether this level ticks `c`. `idle_days` is the Balanced threshold.
    pub fn ticks(self, c: &Candidate, idle_days: u32, now: i64) -> bool {
        let idle = |days: u32| c.last_used.is_none_or(|t| now - t >= days as i64 * 86_400);
        match (c.tier, self) {
            (Tier::Keep, _) => false,
            (Tier::Safe, _) => true,
            (Tier::Rebuild, Level::Safe) => false,
            (Tier::Rebuild, Level::Balanced) => idle(idle_days),
            (Tier::Rebuild, Level::Deep) => idle(7),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolAction {
    GoModCache,
    ShutdownSimulator(String),
}

#[derive(Clone, Debug)]
pub enum Method {
    /// `git worktree remove` from the main checkout; the branch stays.
    GitWorktree { main: PathBuf },
    /// `git worktree prune` for records whose folder is already gone.
    Prune { main: PathBuf },
    /// Every path moves to the Trash.
    Trash,
    /// A known cache, cleared by `caches::clear` (which validates again).
    KnownCache(crate::caches::Cache),
    Tool(ToolAction),
    /// Shown only.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeState {
    Removable,
    Review,
    Active,
    InUse,
    Orphan,
}

impl WorktreeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Removable => l("Removable", "Kan fjernes"),
            Self::Review => l("Review", "Se over"),
            Self::Active => l("Active", "Aktiv"),
            Self::InUse => l("In use", "I bruk"),
            Self::Orphan => l("Orphan", "Foreldreløs"),
        }
    }
}

/// What made the candidate safe at scan time, compared again before removal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fingerprint {
    pub head: Option<String>,
    pub status: u64,
    pub identity: Option<(u64, u64)>,
}

/// The agent's advice on one candidate (FR-18). Advice only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentNote {
    pub keep: bool,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    /// Stable across rescans: the canonical path, or a tool tag.
    pub id: String,
    pub category: Category,
    pub title: String,
    /// One more line: the reason for its state, its branch, its idle time.
    pub detail: String,
    pub paths: Vec<PathBuf>,
    /// Folders where a running process makes it wait.
    pub watch: Vec<PathBuf>,
    pub bytes: Option<u64>,
    pub last_used: Option<i64>,
    pub tier: Tier,
    pub method: Method,
    pub lock: Option<String>,
    pub state: Option<WorktreeState>,
    pub fingerprint: Fingerprint,
    pub note: Option<AgentNote>,
    /// For the agent: repo, branch and so on.
    pub facts: Vec<(&'static str, String)>,
}

impl Candidate {
    pub fn folder(category: Category, path: PathBuf, title: String, tier: Tier) -> Self {
        Self {
            id: path.display().to_string(),
            category,
            title,
            detail: String::new(),
            watch: vec![path.clone()],
            fingerprint: Fingerprint { identity: gate::identity(&path), ..Default::default() },
            paths: vec![path],
            bytes: None,
            last_used: None,
            tier,
            method: Method::Trash,
            lock: None,
            state: None,
            note: None,
            facts: Vec::new(),
        }
    }
    /// Whether it can be ticked now.
    pub fn selectable(&self) -> bool {
        self.tier != Tier::Keep && self.lock.is_none() && self.bytes.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Finding,
    Measuring,
    Done,
}

#[derive(Clone, Debug)]
pub struct ScanState {
    pub candidates: Vec<Candidate>,
    /// Places that could not be checked, with why.
    pub skipped: Vec<(String, String)>,
    pub phase: Phase,
    pub finished: Option<Instant>,
    pub pressure: Option<pressure::Pressure>,
    /// Whether git could run at all.
    pub git: bool,
}

impl ScanState {
    pub(crate) fn new() -> Self {
        Self {
            candidates: Vec::new(),
            skipped: Vec::new(),
            phase: Phase::Finding,
            finished: None,
            pressure: None,
            git: git::executable().is_some(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScanConfig {
    pub roots: Vec<PathBuf>,
    pub active_days: u32,
    pub use_gh: bool,
    pub include_downloads: bool,
    /// Rebuild items idle this long are part of a Balanced cleanup.
    pub build_idle_days: u32,
    /// Look for worktrees only (clean after merge, `tidegauge cleanup worktrees`).
    pub only_worktrees: bool,
}

impl ScanConfig {
    pub fn from_settings(s: &crate::settings::Settings, agent_folders: Vec<PathBuf>) -> Self {
        let mut roots = default_roots();
        roots.extend(s.cleanup_roots.iter().cloned());
        roots.extend(agent_folders);
        Self {
            roots,
            active_days: s.cleanup_active_days.max(1),
            use_gh: s.cleanup_use_gh,
            include_downloads: s.cleanup_include_downloads,
            build_idle_days: s.cleanup_build_idle_days.max(1),
            only_worktrees: false,
        }
    }

    /// From the saved settings, with the folders running agents work in.
    pub fn current() -> Self {
        let agents = crate::engine::latest().agents.iter().filter_map(|s| s.folder.as_ref()).map(PathBuf::from).collect();
        Self::from_settings(&crate::settings::Settings::load(), agents)
    }
}

/// `~/Dev`, `~/Developer`, `~/Projects` and `~/code` when they exist (Q-1).
pub fn default_roots() -> Vec<PathBuf> {
    let Some(home) = gate::home() else { return Vec::new() };
    ["Dev", "Developer", "Projects", "code"]
        .iter()
        .map(|d| home.join(d))
        .filter(|p| p.is_dir())
        .collect()
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Sizes of unchanged folders, kept for the app's life so a rescan does not
/// walk them again: (dev, ino) → (mtime, head, bytes).
type SizeCache = Mutex<HashMap<(u64, u64), (i64, Option<String>, u64)>>;

fn size_cache() -> &'static SizeCache {
    static CACHE: OnceLock<SizeCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn measure(c: &Candidate, cancel: &AtomicBool) -> Option<u64> {
    let mut total = 0u64;
    for path in &c.paths {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.is_file() {
            use std::os::unix::fs::MetadataExt;
            total += meta.blocks() * 512;
            continue;
        }
        use std::os::unix::fs::MetadataExt;
        let key = (meta.dev(), meta.ino());
        let mtime = meta.mtime();
        // A worktree's root mtime does not change when files deep inside do,
        // so its HEAD is part of the key; other folders are tool-managed.
        if let Some((m, h, b)) = size_cache().lock().unwrap().get(&key) {
            if *m == mtime && *h == c.fingerprint.head && c.category != Category::AgentScratch {
                total += b;
                continue;
            }
        }
        let bytes = crate::caches::folder_bytes_until(path, Some(cancel))?;
        size_cache().lock().unwrap().insert(key, (mtime, c.fingerprint.head.clone(), bytes));
        total += bytes;
    }
    Some(total)
}

/// What the last full scan found ready at the Balanced level, and when:
/// the menu-bar panel's "ready to free" line (FR-17).
pub static READY: Mutex<Option<(u64, i64)>> = Mutex::new(None);

/// Ticked items minus any inside another ticked item, so nothing counts twice.
pub fn without_nested(on: Vec<&Candidate>) -> Vec<&Candidate> {
    on.iter()
        .copied()
        .enumerate()
        .filter(|(i, c)| {
            !on.iter().enumerate().any(|(j, o)| {
                o.id != c.id
                    && c.paths.iter().any(|p| o.paths.iter().any(|q| (p != q && p.starts_with(q)) || (p == q && j < *i)))
            })
        })
        .map(|(_, c)| c)
        .collect()
}

/// The size of a Balanced cleanup with no manual ticks.
pub fn balanced_bytes(cands: &[Candidate], idle_days: u32) -> u64 {
    let now = now();
    without_nested(cands.iter().filter(|c| c.selectable() && Level::Balanced.ticks(c, idle_days, now)).collect())
        .iter()
        .filter_map(|c| c.bytes)
        .sum()
}

/// Run a whole scan on this thread and return the result.
pub fn scan_blocking(cfg: ScanConfig) -> ScanState {
    let state = Arc::new(Mutex::new(ScanState::new()));
    run_scan(cfg, Arc::new(AtomicBool::new(false)), state.clone());
    let s = state.lock().unwrap().clone();
    s
}

/// Start a scan on worker threads (D-9). The view reads the returned state
/// while it draws; `cancel` stops it within about a second.
pub fn scan(cfg: ScanConfig, cancel: Arc<AtomicBool>) -> Arc<Mutex<ScanState>> {
    let state = Arc::new(Mutex::new(ScanState::new()));
    let shared = state.clone();
    std::thread::Builder::new()
        .name("cleanup-scan".into())
        .spawn(move || run_scan(cfg, cancel, shared))
        .ok();
    state
}

fn push(state: &Mutex<ScanState>, mut found: Vec<Candidate>) {
    let mut s = state.lock().unwrap();
    for c in found.drain(..) {
        // A path already listed under another name (the SwiftPM cache is
        // also a bundle-id folder) is listed once, by whoever came first.
        let claimed = !c.paths.is_empty() && c.paths.iter().all(|p| s.candidates.iter().any(|x| x.paths.contains(p)));
        if !claimed && !s.candidates.iter().any(|x| x.id == c.id) {
            s.candidates.push(c);
        }
    }
}

fn skip(state: &Mutex<ScanState>, place: &Path, why: &str) {
    state.lock().unwrap().skipped.push((place.display().to_string(), why.to_string()));
}

fn run_scan(cfg: ScanConfig, cancel: Arc<AtomicBool>, state: Arc<Mutex<ScanState>>) {
    let Some(home) = gate::home() else {
        state.lock().unwrap().phase = Phase::Done;
        return;
    };
    let procs = gate::Procs::read();
    let stop = || cancel.load(Ordering::Relaxed);

    // Worktrees first: the biggest win and the riskiest contract.
    if git::executable().is_some() {
        let repos = worktrees::find_repos(&cfg.roots, &home, &cancel);
        let queue = Mutex::new(repos);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| loop {
                    if stop() {
                        return;
                    }
                    let Some(repo) = queue.lock().unwrap().pop() else { return };
                    match worktrees::audit_repo(&repo, &cfg, &home, &procs) {
                        Ok(found) => push(&state, found),
                        Err(why) => skip(&state, &repo, &why),
                    }
                });
            }
        });
        if !stop() {
            push(&state, worktrees::orphans(&cfg.roots, &home));
        }
    }

    // Leftovers and known places.
    if !stop() && !cfg.only_worktrees {
        let worktree_paths: Vec<PathBuf> = state
            .lock()
            .unwrap()
            .candidates
            .iter()
            .filter(|c| c.category == Category::AgentWorktrees)
            .flat_map(|c| c.paths.clone())
            .collect();
        let (caches, unmeasured) = leftovers::known_caches(&cancel);
        push(&state, caches);
        for (place, why) in unmeasured {
            skip(&state, Path::new(&place), &why);
        }
        push(&state, leftovers::derived_orphans(&home));
        if !stop() && git::executable().is_some() {
            let (found, skipped) = leftovers::build_output(&cfg.roots, &worktree_paths, &home, &cancel);
            push(&state, found);
            for (place, why) in skipped {
                skip(&state, Path::new(&place), &why);
            }
        }
        push(&state, leftovers::catalog(&home, cfg.include_downloads));
        push(&state, leftovers::app_leftovers(&home));
    }
    if !stop() && !cfg.only_worktrees {
        let p = pressure::read();
        let sims = pressure::simulator_candidates(&p);
        let mut s = state.lock().unwrap();
        s.pressure = Some(p);
        drop(s);
        push(&state, sims);
    }

    // Locks, then sizes.
    {
        let mut s = state.lock().unwrap();
        for c in s.candidates.iter_mut() {
            if c.lock.is_none() && !matches!(c.method, Method::Tool(ToolAction::ShutdownSimulator(_))) {
                c.lock = gate::lock(c, &procs);
            }
        }
        s.phase = Phase::Measuring;
    }
    let todo: Vec<(String, Candidate)> = state
        .lock()
        .unwrap()
        .candidates
        .iter()
        .filter(|c| c.bytes.is_none())
        .map(|c| (c.id.clone(), c.clone()))
        .collect();
    let queue = Mutex::new(todo);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| loop {
                if stop() {
                    return;
                }
                let Some((id, c)) = queue.lock().unwrap().pop() else { return };
                let bytes = measure(&c, &cancel);
                let mut s = state.lock().unwrap();
                if let Some(x) = s.candidates.iter_mut().find(|x| x.id == id) {
                    x.bytes = bytes.or(Some(0));
                }
            });
        }
    });
    let mut s = state.lock().unwrap();
    s.phase = Phase::Done;
    s.finished = Some(Instant::now());
    if !cfg.only_worktrees && !stop() {
        let ready = balanced_bytes(&s.candidates, cfg.build_idle_days);
        READY.lock().unwrap().replace((ready, now()));
    }
}

/// Sizes in a category, for the row and the summary.
pub fn category_bytes<'a>(cands: impl Iterator<Item = &'a Candidate>) -> u64 {
    cands.filter_map(|c| c.bytes).sum()
}

#[cfg(test)]
pub mod test_support {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A fresh, canonical temp folder for one test.
    pub fn temp(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tidegauge-cleanup-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(dir).unwrap()
    }

    pub fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new(super::git::executable().expect("git"))
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false"])
            .args(args)
            // Commits are ten days old, so only file times make a worktree recent.
            .env("GIT_AUTHOR_DATE", format!("@{} +0000", super::now() - 10 * 86_400))
            .env("GIT_COMMITTER_DATE", format!("@{} +0000", super::now() - 10 * 86_400))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A repo with one commit on main, a bare origin it is pushed to, and
    /// origin/HEAD set.
    pub fn repo(root: &Path, name: &str) -> PathBuf {
        let origin = root.join(format!("{name}-origin.git"));
        git(root, &["init", "--bare", "-q", origin.to_str().unwrap()]);
        let repo = root.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        std::fs::write(repo.join("README.md"), "hi\n").unwrap();
        std::fs::write(repo.join(".gitignore"), "node_modules/\n.env\ntarget/\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "init"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        git(&repo, &["remote", "set-head", "origin", "main"]);
        repo
    }

    /// A linked worktree at `<repo>/.claude/worktrees/<name>` on branch `name`
    /// with one commit of its own.
    pub fn worktree(repo: &Path, name: &str) -> PathBuf {
        let path = repo.join(".claude/worktrees").join(name);
        git(repo, &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()]);
        std::fs::write(path.join(format!("{name}.txt")), "work\n").unwrap();
        git(&path, &["add", "."]);
        git(&path, &["commit", "-qm", name]);
        path
    }

    /// Merge `branch` into main and push main.
    pub fn merge(repo: &Path, branch: &str) {
        git(repo, &["merge", "-q", "--no-ff", "-m", "merge", branch]);
        git(repo, &["push", "-q", "origin", "main"]);
    }
}
