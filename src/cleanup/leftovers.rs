//! Leftovers beside worktrees: Xcode build data whose project is gone
//! (FR-6), build output in idle projects (FR-7), and a fixed catalog of
//! known places (FR-8). The catalog's path list follows disk-jockey
//! (github.com/bmcreations/disk-jockey, MIT).

use super::gate;
use super::git;
use super::{Candidate, Category, Method, Tier, ToolAction};
use crate::locale::text as l;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The caches Tidegauge already knew (`caches.rs`), as candidates.
pub fn known_caches(cancel: &AtomicBool) -> (Vec<Candidate>, Vec<(String, String)>) {
    use crate::caches::Kind;
    let inv = crate::caches::scan_until(Some(cancel));
    let skipped = inv
        .unmeasured
        .iter()
        .map(|k| (format!("{k:?}"), l("could not be measured", "kunne ikke måles").to_string()))
        .collect();
    let found = crate::caches::listed(&inv.caches)
        .into_iter()
        .filter(|c| c.kind != Kind::Trash)
        .cloned()
        .map(|cache| {
            let (category, tier, title) = match cache.kind {
                Kind::DerivedData => (Category::XcodeBuild, Tier::Rebuild, l("All Xcode build data", "All Xcode-byggdata")),
                Kind::DeviceSupport => (Category::XcodeBuild, Tier::Rebuild, l("Xcode device support", "Xcode-enhetsstøtte")),
                Kind::Simulators => (Category::Simulators, Tier::Safe, l("Unavailable simulators", "Utilgjengelige simulatorer")),
                Kind::Homebrew => (Category::PackageCaches, Tier::Safe, l("Homebrew downloads", "Homebrew-nedlastinger")),
                Kind::Npm => (Category::PackageCaches, Tier::Safe, l("npm cache", "npm-buffer")),
                Kind::Pnpm => (Category::PackageCaches, Tier::Safe, l("pnpm store", "pnpm-lager")),
                Kind::Yarn => (Category::PackageCaches, Tier::Safe, l("Yarn cache", "Yarn-buffer")),
                Kind::Bun => (Category::PackageCaches, Tier::Safe, l("Bun cache", "Bun-buffer")),
                Kind::Cargo => (Category::PackageCaches, Tier::Rebuild, l("Cargo downloads", "Cargo-nedlastinger")),
                Kind::Trash => unreachable!(),
            };
            let id = match cache.paths.first() {
                Some(p) => p.display().to_string(),
                None => format!("tool:{:?}", cache.kind),
            };
            // When it was last used, so Balanced leaves a cache in use alone.
            let last_used = cache.paths.iter().filter_map(|p| super::worktrees::last_used(p, None)).max();
            Candidate {
                id,
                category,
                title: title.into(),
                detail: match (cache.kind.method(), cache.items.is_empty()) {
                    (crate::caches::Method::Tool, false) => cache.items.join(", "),
                    (crate::caches::Method::Tool, true) => l("Cleared by its own tool; can't be undone", "Ryddes av sitt eget verktøy; kan ikke angres").into(),
                    _ => l("Rebuilt when needed", "Bygges opp igjen ved behov").into(),
                },
                watch: Vec::new(),
                paths: cache.paths.clone(),
                bytes: Some(cache.bytes),
                last_used,
                tier,
                method: Method::KnownCache(cache),
                lock: None,
                state: None,
                fingerprint: Default::default(),
                note: None,
                facts: Vec::new(),
            }
        })
        .collect();
    (found, skipped)
}

/// `WorkspacePath` from a DerivedData folder's `info.plist`.
fn workspace_path(folder: &Path) -> Option<PathBuf> {
    let plist = folder.join("info.plist");
    if !plist.is_file() {
        return None;
    }
    let mut c = std::process::Command::new("/usr/bin/plutil");
    c.args(["-extract", "WorkspacePath", "raw", "-o", "-"])
        .arg(&plist)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let out = crate::packages::run_command(c, Duration::from_secs(2)).ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// Whether a recorded project is provably gone: missing, and not on a
/// volume that is just unmounted.
pub fn project_gone(path: &Path) -> bool {
    if path.exists() {
        return false;
    }
    let mut parts = path.components();
    if let (Some(_), Some(v), Some(name)) = (parts.next(), parts.next(), parts.next()) {
        if v.as_os_str() == "Volumes" && !Path::new("/Volumes").join(name.as_os_str()).exists() {
            return false;
        }
    }
    true
}

/// DerivedData folders whose project no longer exists (FR-6).
pub fn derived_orphans(home: &Path) -> Vec<Candidate> {
    derived_orphans_in(&home.join("Library/Developer/Xcode/DerivedData"), home, workspace_path)
}

fn derived_orphans_in(dir: &Path, home: &Path, read: impl Fn(&Path) -> Option<PathBuf>) -> Vec<Candidate> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let folder = e.path();
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some(project) = read(&folder) else { continue };
        if !project_gone(&project) || gate::validate(&folder, home).is_err() {
            continue;
        }
        let name = project.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut c = Candidate::folder(Category::XcodeBuild, folder.clone(), format!("{name} · {}", l("project gone", "prosjektet er borte")), Tier::Safe);
        c.detail = crate::processes::path_summary(&project.display().to_string());
        c.last_used = super::worktrees::last_used(&folder, None);
        out.push(c);
    }
    out
}

/// Build files and the output folders that sit next to them.
const BUILD: [(&str, &[&str]); 9] = [
    ("package.json", &["node_modules", ".next", ".turbo", "dist"]),
    ("Cargo.toml", &["target"]),
    ("Package.swift", &[".build"]),
    ("build.gradle", &["build", ".gradle"]),
    ("build.gradle.kts", &["build", ".gradle"]),
    ("settings.gradle", &["build", ".gradle"]),
    ("settings.gradle.kts", &["build", ".gradle"]),
    ("pyproject.toml", &[".venv", "__pycache__"]),
    ("requirements.txt", &[".venv", "__pycache__"]),
];

const WALK_SKIP: [&str; 8] = ["node_modules", "target", ".build", "build", "dist", "Pods", "DerivedData", "vendor"];

/// The nearest folder at or above `dir` that is a git work tree.
fn repo_top(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

/// Build output in projects up to four levels below each root (FR-7).
/// Projects inside a worktree are left to the worktree.
pub fn build_output(roots: &[PathBuf], worktrees: &[PathBuf], home: &Path, cancel: &AtomicBool) -> (Vec<Candidate>, Vec<(String, String)>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut commits: std::collections::HashMap<PathBuf, Option<i64>> = Default::default();
    let mut stack: Vec<(PathBuf, u32)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if worktrees.iter().any(|w| dir.starts_with(w)) || !seen.insert(dir.clone()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let names: Vec<(String, bool)> = entries
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.file_type().is_ok_and(|t| t.is_dir())))
            .collect();
        let outputs: Vec<&str> = BUILD
            .iter()
            .filter(|(file, _)| names.iter().any(|(n, d)| !d && n == file))
            .flat_map(|(_, outs)| outs.iter().copied())
            .collect();
        for o in outputs {
            let folder = dir.join(o);
            if !names.iter().any(|(n, d)| *d && n == o) || out.iter().any(|c: &Candidate| c.paths[0] == folder) {
                continue;
            }
            match judge_output(&dir, &folder, home, &mut commits) {
                Ok(Some(c)) => out.push(c),
                Ok(None) => {}
                Err(why) => skipped.push((folder.display().to_string(), why)),
            }
        }
        if depth >= 4 {
            continue;
        }
        for (name, is_dir) in names {
            if is_dir && !name.starts_with('.') && !WALK_SKIP.contains(&name.as_str()) {
                stack.push((dir.join(name), depth + 1));
            }
        }
    }
    (out, skipped)
}

fn judge_output(
    project: &Path,
    folder: &Path,
    home: &Path,
    commits: &mut std::collections::HashMap<PathBuf, Option<i64>>,
) -> Result<Option<Candidate>, String> {
    if gate::validate(folder, home).is_err() {
        return Ok(None);
    }
    let last = match repo_top(project) {
        Some(top) => {
            let rel = folder.strip_prefix(&top).map_err(|_| String::new())?.to_string_lossy().into_owned();
            match git::is_ignored(&top, &rel) {
                Some(true) => {}
                Some(false) => return Ok(None),
                None => return Err(l("git did not answer", "git svarte ikke").into()),
            }
            if git::tracks_any(&top, &rel) != Some(false) {
                return Ok(None);
            }
            let commit = *commits.entry(top.clone()).or_insert_with(|| git::last_commit(&top));
            idle_since(project, folder, commit)
        }
        None => {
            // Without git, nothing says `dist` or `build` is not source.
            let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "dist" | "build") {
                return Ok(None);
            }
            idle_since(project, folder, None)
        }
    };
    let project_name = project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let output_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut c = Candidate::folder(Category::BuildOutput, folder.to_path_buf(), format!("{project_name} · {output_name}"), Tier::Rebuild);
    c.watch = vec![project.to_path_buf()];
    c.last_used = last;
    c.detail = crate::processes::path_summary(&project.display().to_string());
    Ok(Some(c))
}

/// Whether `folder` next to `project` is still build output: ignored and
/// untracked when inside a repo; for a folder outside any repo, not `dist`
/// or `build`. Checked again right before removal (NFR-2).
pub fn still_output(project: &Path, folder: &Path) -> bool {
    match repo_top(project) {
        Some(top) => {
            let Ok(rel) = folder.strip_prefix(&top) else { return false };
            let rel = rel.to_string_lossy();
            git::is_ignored(&top, &rel) == Some(true) && git::tracks_any(&top, &rel) == Some(false)
        }
        None => !matches!(folder.file_name().and_then(|n| n.to_str()), Some("dist" | "build")),
    }
}

/// Last activity in a project: its last commit, or the newest top-level
/// change that is not build output.
fn idle_since(project: &Path, folder: &Path, commit: Option<i64>) -> Option<i64> {
    let mtime = |p: &Path| {
        std::fs::symlink_metadata(p).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
    };
    let mut newest = commit;
    if let Ok(entries) = std::fs::read_dir(project) {
        for e in entries.flatten().take(500) {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p == folder || super::worktrees::DISPOSABLE.contains(&name.as_ref()) || name == ".git" {
                continue;
            }
            newest = newest.max(mtime(&p));
        }
    }
    newest
}

fn days_old(p: &Path, days: i64) -> bool {
    std::fs::symlink_metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|e| e.as_secs() as i64 >= days * 86_400)
}

/// One folder from the catalog, if it exists and passes the gate.
fn place(home: &Path, rel: &str, category: Category, tier: Tier, title: &str) -> Option<Candidate> {
    let path = home.join(rel);
    let path = std::fs::canonicalize(&path).ok().filter(|p| *p == path)?;
    if !path.is_dir() || gate::validate(&path, home).is_err() {
        return None;
    }
    let mut c = Candidate::folder(category, path, title.to_string(), tier);
    if tier == Tier::Keep {
        c.method = Method::None;
    }
    Some(c)
}

/// Files directly in `dir` older than `days` whose name passes `keep`.
fn old_files(dir: &Path, days: i64, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter(|e| keep(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .filter(|p| days_old(p, days))
                .collect()
        })
        .unwrap_or_default()
}

fn files_candidate(category: Category, id: String, title: &str, paths: Vec<PathBuf>, tier: Tier, home: &Path) -> Option<Candidate> {
    let paths: Vec<PathBuf> = paths.into_iter().filter(|p| gate::validate(p, home).is_ok()).collect();
    if paths.is_empty() {
        return None;
    }
    let n = paths.len() as u64;
    Some(Candidate {
        id,
        category,
        title: title.to_string(),
        detail: crate::locale::count(n, "file older than 30 days", "files older than 30 days", "fil eldre enn 30 dager", "filer eldre enn 30 dager"),
        watch: Vec::new(),
        paths,
        bytes: None,
        last_used: None,
        tier,
        method: Method::Trash,
        lock: None,
        state: None,
        fingerprint: Default::default(),
        note: None,
        facts: Vec::new(),
    })
}

/// The fixed catalog (FR-8). Downloads only when switched on, which avoids
/// macOS's privacy prompt for that folder.
pub fn catalog(home: &Path, include_downloads: bool) -> Vec<Candidate> {
    use Category as C;
    let mut out: Vec<Candidate> = [
        (".gradle/caches", C::PackageCaches, Tier::Rebuild, l("Gradle caches", "Gradle-bufre")),
        (".gradle/wrapper/dists", C::PackageCaches, Tier::Rebuild, l("Gradle distributions", "Gradle-distribusjoner")),
        ("Library/Caches/CocoaPods", C::PackageCaches, Tier::Safe, l("CocoaPods cache", "CocoaPods-buffer")),
        ("Library/Caches/pip", C::PackageCaches, Tier::Safe, l("pip cache", "pip-buffer")),
        ("Library/Caches/pypoetry", C::PackageCaches, Tier::Safe, l("Poetry cache", "Poetry-buffer")),
        (".cache/uv", C::PackageCaches, Tier::Safe, l("uv cache", "uv-buffer")),
        ("Library/Caches/org.swift.swiftpm", C::PackageCaches, Tier::Safe, l("SwiftPM cache", "SwiftPM-buffer")),
        ("Library/Caches/ms-playwright", C::PackageCaches, Tier::Rebuild, l("Playwright browsers", "Playwright-nettlesere")),
        ("Library/Logs/CoreSimulator", C::Logs, Tier::Safe, l("Simulator logs", "Simulatorlogger")),
        ("Library/Developer/Xcode/Archives", C::Archives, Tier::Keep, l("Xcode archives", "Xcode-arkiver")),
        ("Library/Application Support/MobileSync/Backup", C::Archives, Tier::Keep, l("iPhone and iPad backups", "Sikkerhetskopier av iPhone og iPad")),
        (".claude/projects", C::AgentHistory, Tier::Keep, l("Claude Code transcripts", "Claude Code-samtaler")),
        (".codex/sessions", C::AgentHistory, Tier::Keep, l("Codex sessions", "Codex-økter")),
        // FR-27, D-17: download caches are Safe, package stores Rebuild,
        // anything installed or made by you Keep.
        (".android/cache", C::PackageCaches, Tier::Safe, l("Android SDK downloads", "Android SDK-nedlastinger")),
        (".pub-cache", C::PackageCaches, Tier::Rebuild, l("Dart and Flutter packages", "Dart- og Flutter-pakker")),
        (".m2/repository", C::PackageCaches, Tier::Rebuild, l("Maven packages", "Maven-pakker")),
        (".nvm/.cache", C::PackageCaches, Tier::Safe, l("nvm downloads", "nvm-nedlastinger")),
        (".pyenv/cache", C::PackageCaches, Tier::Safe, l("pyenv downloads", "pyenv-nedlastinger")),
        (".asdf/downloads", C::PackageCaches, Tier::Safe, l("asdf downloads", "asdf-nedlastinger")),
        ("Library/Caches/mise", C::PackageCaches, Tier::Safe, l("mise cache", "mise-buffer")),
        (".rustup/downloads", C::PackageCaches, Tier::Safe, l("rustup downloads", "rustup-nedlastinger")),
        ("Library/Application Support/Code/CachedData", C::AppCaches, Tier::Safe, l("VS Code cache", "VS Code-buffer")),
        ("Library/Application Support/Code/Cache", C::AppCaches, Tier::Safe, l("VS Code web cache", "VS Code-nettbuffer")),
        ("Library/Application Support/Code/CachedExtensionVSIXs", C::AppCaches, Tier::Safe, l("VS Code extension downloads", "VS Code-utvidelsesnedlastinger")),
        ("Library/Application Support/Cursor/CachedData", C::AppCaches, Tier::Safe, l("Cursor cache", "Cursor-buffer")),
        ("Library/Application Support/Cursor/Cache", C::AppCaches, Tier::Safe, l("Cursor web cache", "Cursor-nettbuffer")),
        ("Library/Application Support/Slack/Cache", C::AppCaches, Tier::Safe, l("Slack cache", "Slack-buffer")),
        ("Library/Application Support/Slack/Service Worker/CacheStorage", C::AppCaches, Tier::Safe, l("Slack offline cache", "Slack-frakoblet buffer")),
        ("Library/Application Support/discord/Cache", C::AppCaches, Tier::Safe, l("Discord cache", "Discord-buffer")),
        ("Library/Caches/JetBrains", C::AppCaches, Tier::Rebuild, l("JetBrains caches and indexes", "JetBrains-bufre og indekser")),
        ("Library/Unity/cache", C::AppCaches, Tier::Rebuild, l("Unity package cache", "Unity-pakkebuffer")),
        (".android/avd", C::DevTools, Tier::Keep, l("Android emulators", "Android-emulatorer")),
        ("Library/Android/sdk/system-images", C::DevTools, Tier::Keep, l("Android system images", "Android-systembilder")),
        (".nvm/versions", C::DevTools, Tier::Keep, l("Node versions (nvm)", "Node-versjoner (nvm)")),
        (".pyenv/versions", C::DevTools, Tier::Keep, l("Python versions (pyenv)", "Python-versjoner (pyenv)")),
        (".asdf/installs", C::DevTools, Tier::Keep, l("asdf tools", "asdf-verktøy")),
        (".local/share/mise/installs", C::DevTools, Tier::Keep, l("mise tools", "mise-verktøy")),
        (".rustup/toolchains", C::DevTools, Tier::Keep, l("Rust toolchains", "Rust-verktøykjeder")),
    ]
    .into_iter()
    .filter_map(|(rel, cat, tier, title)| place(home, rel, cat, tier, title))
    .collect();
    // Go's module cache is read-only on disk, so Go clears it itself.
    if let Some(mut go) = place(home, "go/pkg/mod", C::PackageCaches, Tier::Rebuild, l("Go modules", "Go-moduler")) {
        go.method = Method::Tool(ToolAction::GoModCache);
        out.push(go);
    }
    if let Some(c) = files_candidate(
        C::Logs,
        "logs:DiagnosticReports".into(),
        l("Crash reports", "Krasjrapporter"),
        old_files(&home.join("Library/Logs/DiagnosticReports"), 30, |_| true),
        Tier::Safe,
        home,
    ) {
        out.push(c);
    }
    out.extend(scratch(home));
    out.extend(xcodebuildmcp(home, 14));
    if include_downloads {
        if let Some(c) = files_candidate(
            C::Downloads,
            "downloads:installers".into(),
            l("Old installers in Downloads", "Gamle installasjonsfiler i Nedlastinger"),
            old_files(&home.join("Downloads"), 30, |n| {
                let n = n.to_lowercase();
                n.ends_with(".dmg") || n.ends_with(".pkg")
            }),
            Tier::Rebuild,
            home,
        ) {
            out.push(c);
        }
    }
    out
}

/// Whether a folder or file name is a bundle id: three or more dot-separated
/// parts of letters, digits, `-` and `_`.
pub fn bundle_id(name: &str) -> Option<&str> {
    let id = name.strip_suffix(".savedState").or_else(|| name.strip_suffix(".plist")).unwrap_or(name);
    let parts: Vec<&str> = id.split('.').collect();
    let ok = parts.len() >= 3
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        && parts[0].chars().all(|c| c.is_ascii_lowercase());
    ok.then_some(id)
}

/// The vendor part of a bundle id: its first two parts, lowercased.
pub fn vendor(id: &str) -> String {
    id.split('.').take(2).collect::<Vec<_>>().join(".").to_lowercase()
}

/// Bundle ids of the apps installed in the usual folders, up to two folder
/// levels deep, or none when the list can't be trusted: /Applications
/// unreadable, or any app whose id could not be read. Then nothing is
/// called a leftover (fail closed).
fn installed_ids(home: &Path) -> Option<Vec<String>> {
    let mut apps = Vec::new();
    let roots = [
        PathBuf::from("/Applications"),
        home.join("Applications"),
        PathBuf::from("/System/Applications"),
        home.join("Library/Application Support/JetBrains/Toolbox/apps"),
        home.join("Library/Application Support/Setapp/Applications"),
    ];
    std::fs::read_dir("/Applications").ok()?;
    let mut stack: Vec<(PathBuf, u32)> = roots.into_iter().map(|r| (r, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "app") {
                apps.push(p);
            } else if depth < 3 && e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((p, depth + 1));
            }
        }
    }
    let mut ids = Vec::new();
    for app in &apps {
        let mut c = std::process::Command::new("/usr/bin/plutil");
        c.args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let out = crate::packages::run_command(c, Duration::from_secs(5)).ok()?;
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // An app without an id is unusual but harmless; a failed read is not.
        if out.status.success() && !id.is_empty() {
            ids.push(id);
        }
    }
    (!ids.is_empty()).then_some(ids)
}

/// `~/Library` folders and preference files left by apps that are gone
/// (FR-21). A vendor with any installed app keeps all its folders, and
/// Apple's never show. Sandboxed containers are left to macOS.
pub fn app_leftovers(home: &Path) -> Vec<Candidate> {
    match installed_ids(home) {
        Some(ids) => app_leftovers_in(home, &ids),
        None => Vec::new(),
    }
}

fn app_leftovers_in(home: &Path, installed: &[String]) -> Vec<Candidate> {
    let vendors: std::collections::HashSet<String> = installed.iter().map(|i| vendor(i)).collect();
    // One row per maker: tests can leave thousands of ids from one vendor
    // (`app.example.tests.<uuid>.plist`), which must not become thousands
    // of rows.
    let mut by_vendor: std::collections::BTreeMap<String, (std::collections::BTreeSet<String>, Vec<PathBuf>)> = Default::default();
    for place in ["Application Support", "Caches", "Saved Application State", "HTTPStorages", "WebKit", "Logs", "Preferences"] {
        let Ok(entries) = std::fs::read_dir(home.join("Library").join(place)) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(id) = bundle_id(&name) else { continue };
            if (place == "Preferences") != name.ends_with(".plist") {
                continue;
            }
            let v = vendor(id);
            if v.starts_with("com.apple") || vendors.contains(&v) || id.contains("tidegauge") {
                continue;
            }
            let entry = by_vendor.entry(v).or_default();
            entry.0.insert(id.to_string());
            entry.1.push(e.path());
        }
    }
    by_vendor
        .into_iter()
        .filter_map(|(vendor, (ids, paths))| {
            let paths: Vec<PathBuf> = paths.into_iter().filter(|p| gate::validate(p, home).is_ok()).collect();
            if paths.is_empty() {
                return None;
            }
            let last = paths.iter().filter_map(|p| super::worktrees::last_used(p, None)).max();
            let title = match ids.len() {
                1 => ids.iter().next().cloned().unwrap_or_default(),
                n => format!("{vendor} · {}", crate::locale::count(n as u64, "app id", "app ids", "app-id", "app-id-er")),
            };
            let id = ids.iter().next().cloned().unwrap_or_default();
            Some(Candidate {
                id: format!("app:{vendor}"),
                category: Category::AppLeftovers,
                title,
                detail: format!(
                    "{} · {}",
                    crate::locale::count(paths.len() as u64, "place", "places", "sted", "steder"),
                    l("no installed app from this maker", "ingen installert app fra denne utvikleren")
                ),
                watch: Vec::new(),
                paths,
                bytes: None,
                last_used: last,
                tier: Tier::Rebuild,
                method: Method::Trash,
                lock: None,
                state: None,
                fingerprint: Default::default(),
                note: None,
                facts: vec![("bundle_id", id)],
            })
        })
        .collect()
}

/// Agent scratch folders in `/private/tmp/claude-<uid>` untouched for three days.
fn scratch(home: &Path) -> Vec<Candidate> {
    let root = gate::scratch_root();
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| days_old(p, 3) && super::worktrees::last_used(p, None).is_none_or(|t| super::now() - t >= 3 * 86_400))
        .filter(|p| gate::validate(p, home).is_ok())
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // Rebuild: Balanced takes it only after 30 idle days, since a
            // live session may work deeper inside than the top level shows.
            let mut c = Candidate::folder(Category::AgentScratch, p.clone(), name, Tier::Rebuild);
            c.last_used = super::worktrees::last_used(&p, None);
            c
        })
        .collect()
}

/// The newest modification time under `dir`, walking at most `cap` entries.
/// None when it could not be read or the walk hit the cap, so a folder too
/// big to judge is never called idle.
fn newest_mtime(dir: &Path, cap: usize) -> Option<std::time::SystemTime> {
    let mut newest = std::fs::symlink_metadata(dir).ok()?.modified().ok()?;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            seen += 1;
            if seen > cap {
                return None;
            }
            let Ok(m) = e.metadata() else { continue };
            if let Ok(t) = m.modified() {
                newest = newest.max(t);
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    Some(newest)
}

/// XcodeBuildMCP keeps a workspace folder per project (logs, result bundles,
/// test products) and never removes it. With no project record to check, a
/// folder counts as leftover after `days` with no newer file (FR-28, D-20).
pub fn xcodebuildmcp(home: &Path, days: u64) -> Vec<Candidate> {
    xcodebuildmcp_in(&home.join("Library/Developer/XcodeBuildMCP/workspaces"), home, days)
}

fn xcodebuildmcp_in(dir: &Path, home: &Path, days: u64) -> Vec<Candidate> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let limit = std::time::Duration::from_secs(days * 86_400);
    let mut out = Vec::new();
    for e in entries.flatten() {
        let folder = e.path();
        if !e.file_type().is_ok_and(|t| t.is_dir()) || gate::validate(&folder, home).is_err() {
            continue;
        }
        let Some(newest) = newest_mtime(&folder, 20_000) else { continue };
        if newest.elapsed().map_or(true, |age| age < limit) {
            continue;
        }
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // "brev-fc1df51c6e4f" → "brev"
        let project = match name.rsplit_once('-') {
            Some((p, h)) if h.len() == 12 && h.chars().all(|c| c.is_ascii_hexdigit()) => p.to_string(),
            _ => name.clone(),
        };
        let mut c = Candidate::folder(Category::XcodeBuild, folder, format!("XcodeBuildMCP · {project}"), Tier::Safe);
        c.last_used = newest.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64);
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn xcodebuildmcp_workspaces_only_when_idle() {
        let home = temp("xbmcp");
        let ws = home.join("ws");
        let old = ws.join("brev-fc1df51c6e4f");
        let new = ws.join("kan-187836c9ad13");
        for d in [&old, &new] {
            std::fs::create_dir_all(d.join("logs")).unwrap();
            std::fs::write(d.join("logs/a.log"), "x").unwrap();
        }
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(20 * 86_400);
        for p in [old.join("logs/a.log"), old.join("logs"), old.clone()] {
            std::fs::File::open(&p).unwrap().set_modified(past).unwrap();
        }
        let found = xcodebuildmcp_in(&ws, &home, 14);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].paths[0].ends_with("brev-fc1df51c6e4f"));
        assert_eq!(found[0].title, "XcodeBuildMCP · brev");
        assert_eq!(found[0].tier, Tier::Safe);
        // A fresh file deep inside makes it recent again.
        std::fs::write(old.join("logs/b.log"), "x").unwrap();
        assert!(xcodebuildmcp_in(&ws, &home, 14).is_empty());
    }

    #[test]
    fn new_catalog_entries_follow_their_tiers() {
        let home = temp("catalog2");
        for rel in [".android/cache", ".android/avd", ".rustup/toolchains", "Library/Application Support/Code/CachedData", "Library/Caches/JetBrains"] {
            std::fs::create_dir_all(home.join(rel)).unwrap();
        }
        let home = std::fs::canonicalize(&home).unwrap();
        let found = catalog(&home, false);
        let tier = |end: &str| found.iter().find(|c| c.paths[0].ends_with(end)).map(|c| (c.category, c.tier));
        assert_eq!(tier(".android/cache"), Some((Category::PackageCaches, Tier::Safe)));
        assert_eq!(tier(".android/avd"), Some((Category::DevTools, Tier::Keep)));
        assert_eq!(tier(".rustup/toolchains"), Some((Category::DevTools, Tier::Keep)));
        assert_eq!(tier("Code/CachedData"), Some((Category::AppCaches, Tier::Safe)));
        assert_eq!(tier("Caches/JetBrains"), Some((Category::AppCaches, Tier::Rebuild)));
        assert!(found.iter().filter(|c| c.tier == Tier::Keep).all(|c| matches!(c.method, Method::None)));
    }

    #[test]
    fn derived_orphan_when_workspace_missing() {
        let home = temp("derived");
        let dd = home.join("DD");
        for n in ["Gone-abc", "Here-def", "NoPlist-ghi", "Vol-jkl"] {
            std::fs::create_dir_all(dd.join(n)).unwrap();
        }
        let here = home.join("Here.xcodeproj");
        std::fs::create_dir_all(&here).unwrap();
        let read = |f: &Path| match f.file_name()?.to_str()? {
            "Gone-abc" => Some(home.join("Gone.xcodeproj")),
            "Here-def" => Some(here.clone()),
            "Vol-jkl" => Some(PathBuf::from("/Volumes/NotMountedTidegaugeTest/App.xcodeproj")),
            _ => None,
        };
        let found = derived_orphans_in(&dd, &home, read);
        assert_eq!(found.len(), 1);
        assert!(found[0].paths[0].ends_with("Gone-abc"));
        assert_eq!(found[0].tier, Tier::Safe);
    }

    #[test]
    fn build_output_rules() {
        let home = temp("build");
        // A repo whose node_modules is ignored and one whose dist is tracked.
        let r = repo(&home, "web");
        std::fs::write(r.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(r.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(r.join("dist")).unwrap();
        std::fs::write(r.join("dist/app.js"), "x").unwrap();
        git(&r, &["add", "package.json", "dist"]);
        git(&r, &["commit", "-qm", "dist is tracked"]);
        // A target folder with no Cargo.toml next to it is not output.
        std::fs::create_dir_all(r.join("lib/target")).unwrap();
        let (found, _) = build_output(&[home.clone()], &[], &home, &AtomicBool::new(false));
        let names: Vec<String> = found.iter().map(|c| c.title.clone()).collect();
        assert_eq!(names, vec!["web · node_modules"], "{names:?}");
        assert!(still_output(&r, &r.join("node_modules")));
        // Tracked after the scan: no longer output, so the recheck refuses.
        std::fs::write(r.join("node_modules/keep.js"), "x").unwrap();
        git(&r, &["add", "-f", "node_modules/keep.js"]);
        assert!(!still_output(&r, &r.join("node_modules")));
        assert_eq!(found[0].tier, Tier::Rebuild);
        assert_eq!(found[0].watch, vec![r.clone()]);
        // Idle comes from the last commit (ten days ago in the fixture),
        // not from node_modules.
        let idle = super::super::now() - found[0].last_used.unwrap();
        assert!(idle < 86_400 || idle >= 9 * 86_400, "idle {idle}");
    }

    #[test]
    fn build_output_skips_worktrees() {
        let home = temp("buildwt");
        let r = repo(&home, "app");
        let w = worktree(&r, "feat");
        std::fs::write(w.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(w.join("node_modules")).unwrap();
        let (found, _) = build_output(&[home.clone()], &[w], &home, &AtomicBool::new(false));
        assert!(found.is_empty());
    }

    #[test]
    fn catalog_places_and_tiers() {
        let home = temp("catalog");
        for rel in ["Library/Caches/pip", ".gradle/caches", "Library/Developer/Xcode/Archives", ".claude/projects", "go/pkg/mod", "Downloads"] {
            std::fs::create_dir_all(home.join(rel)).unwrap();
        }
        let c = catalog(&home, false);
        let find = |en: &'static str, nb: &'static str| {
            let t = l(en, nb);
            c.iter().find(|c| c.title == t).unwrap_or_else(|| panic!("{t}"))
        };
        assert_eq!(find("pip cache", "pip-buffer").tier, Tier::Safe);
        assert_eq!(find("Gradle caches", "Gradle-bufre").tier, Tier::Rebuild);
        assert_eq!(find("Xcode archives", "Xcode-arkiver").tier, Tier::Keep);
        assert!(matches!(find("Xcode archives", "Xcode-arkiver").method, Method::None));
        assert_eq!(find("Claude Code transcripts", "Claude Code-samtaler").tier, Tier::Keep);
        assert!(matches!(find("Go modules", "Go-moduler").method, Method::Tool(ToolAction::GoModCache)));
        assert!(!c.iter().any(|c| c.category == Category::Downloads), "downloads off by default");
    }

    #[test]
    fn app_leftovers_only_for_gone_vendors() {
        let home = temp("apps");
        let lib = home.join("Library");
        for rel in [
            "Application Support/com.gone.Editor",
            "Caches/com.gone.Editor",
            "Caches/com.google.Chrome",
            "Caches/com.apple.Safari",
            "Application Support/Notes Folder",
            "Containers/com.gone.Sandboxed",
        ] {
            std::fs::create_dir_all(lib.join(rel)).unwrap();
        }
        std::fs::create_dir_all(lib.join("Preferences")).unwrap();
        std::fs::write(lib.join("Preferences/com.gone.Editor.plist"), "x").unwrap();
        let found = app_leftovers_in(&home, &["com.google.drivefs".to_string()]);
        assert_eq!(found.len(), 1, "{:?}", found.iter().map(|c| &c.title).collect::<Vec<_>>());
        assert_eq!(found[0].title, "com.gone.Editor");
        assert_eq!(found[0].paths.len(), 3, "support, cache and preferences");
        assert_eq!(found[0].tier, Tier::Rebuild);
        // Thousands of test ids from one maker are one row.
        for i in 0..50 {
            std::fs::write(lib.join(format!("Preferences/app.tests.run-{i}.plist")), "x").unwrap();
        }
        let found = app_leftovers_in(&home, &["com.google.drivefs".to_string()]);
        assert_eq!(found.len(), 2);
        let tests = found.iter().find(|c| c.id == "app:app.tests").unwrap();
        assert_eq!(tests.paths.len(), 50);
        assert!(tests.title.starts_with("app.tests · 50"), "{}", tests.title);
        assert_eq!(bundle_id("com.gone.Editor.savedState"), Some("com.gone.Editor"));
        assert_eq!(bundle_id("Notes Folder"), None);
        assert_eq!(bundle_id("UBF8T346G9.com.microsoft"), None);
    }

    #[test]
    fn project_gone_respects_unmounted_volumes() {
        assert!(project_gone(Path::new("/Users/nobody-tidegauge/Gone.xcodeproj")));
        assert!(!project_gone(Path::new("/Volumes/NotMountedTidegaugeTest/A.xcodeproj")));
        assert!(!project_gone(Path::new("/")));
    }
}
