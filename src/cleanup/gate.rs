//! The safety gate: what may be listed, what waits, and the last check
//! before anything is removed (FR-9, FR-13, FR-14, NFR-2).

use super::{Candidate, Category, Method};
use crate::locale::text as l;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

/// The processes of this user, with what they are and where they run.
#[derive(Clone, Debug, Default)]
pub struct Procs {
    pub list: Vec<Proc>,
    /// False when the process list could not be read; then everything is
    /// treated as in use (fail closed).
    pub ok: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Proc {
    #[allow(dead_code)]
    pub pid: i32,
    pub name: String,
    pub cwd: PathBuf,
    pub args: Vec<String>,
}

impl Procs {
    pub fn read() -> Self {
        let uid = unsafe { libc::getuid() };
        let pids = crate::processes::list_pids();
        let mut list = Vec::new();
        for pid in &pids {
            let Some(bsd) = crate::processes::proc_bsd(*pid) else { continue };
            if bsd.pbi_uid != uid {
                continue;
            }
            let exe = crate::processes::proc_path(*pid);
            let name = Path::new(&exe)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let cwd = crate::processes::proc_cwd(*pid);
            let args = if matches!(name.as_str(), "java" | "node") {
                crate::processes::proc_args(*pid).unwrap_or_default()
            } else {
                Vec::new()
            };
            list.push(Proc { pid: *pid, name, cwd: PathBuf::from(cwd), args });
        }
        Self { ok: !pids.is_empty() && !list.is_empty(), list }
    }

    /// The first process whose working folder is inside `path`.
    pub fn inside(&self, path: &Path) -> Option<&Proc> {
        self.list
            .iter()
            .find(|p| !p.cwd.as_os_str().is_empty() && p.cwd.starts_with(path))
    }

    fn running(&self, test: impl Fn(&Proc) -> bool) -> Option<&Proc> {
        self.list.iter().find(|p| test(p))
    }
}

/// Why something must wait right now, or none.
pub fn lock(candidate: &Candidate, procs: &Procs) -> Option<String> {
    if !procs.ok {
        return Some(l("Can't check running apps", "Kan ikke sjekke kjørende apper").into());
    }
    let waits = |app: &str| format!("{} {app}", l("Waits for", "Venter på"));
    let named = |names: &[&str]| procs.running(|p| names.contains(&p.name.as_str()));
    match candidate.category {
        Category::XcodeBuild => named(&["Xcode", "xcodebuild", "XCBBuildService"]).map(|_| waits("Xcode")),
        Category::Simulators => named(&["Simulator", "launchd_sim"]).map(|_| waits("Simulator")),
        Category::PackageCaches => {
            if candidate.id.contains(".gradle") {
                return procs
                    .running(|p| p.name == "java" && p.args.iter().any(|a| a.contains("GradleDaemon")))
                    .map(|_| waits("Gradle"));
            }
            let tools = cache_tools(&candidate.id);
            procs
                .running(|p| {
                    tools.contains(&p.name.as_str())
                        || (p.name == "node"
                            && p.args.iter().take(2).any(|a| tools.iter().any(|t| a.contains(&format!("/{t}")))))
                })
                .map(|p| waits(&p.name))
        }
        _ => candidate
            .watch
            .iter()
            .find_map(|path| procs.inside(path))
            .map(|p| format!("{} {}", l("In use by", "I bruk av"), p.name)),
    }
}

/// The programs that use one package cache, by its id (path or tool tag).
/// Only these make it wait: a running cargo says nothing about pip's cache.
pub fn cache_tools(id: &str) -> &'static [&'static str] {
    const MAP: [(&str, &[&str]); 12] = [
        ("/.npm", &["npm"]),
        ("pnpm", &["pnpm"]),
        ("Yarn", &["yarn"]),
        ("/.bun", &["bun"]),
        ("/.cargo", &["cargo"]),
        ("Homebrew", &["brew"]),
        ("CocoaPods", &["pod"]),
        ("/pip", &["pip", "pip3"]),
        ("pypoetry", &["poetry"]),
        ("/uv", &["uv"]),
        ("swiftpm", &["swift-build", "swift-package", "xcodebuild"]),
        ("/go/pkg/mod", &["go"]),
    ];
    MAP.iter().find(|(k, _)| id.contains(k)).map_or(&[], |(_, t)| t)
}

/// The user's home folder, resolved.
pub fn home() -> Option<PathBuf> {
    let home = std::fs::canonicalize(std::env::var_os("HOME").map(PathBuf::from)?).ok()?;
    (home.is_absolute() && home.components().count() > 2).then_some(home)
}

/// Agent scratch is the one place outside home: `/private/tmp/claude-<uid>`.
pub fn scratch_root() -> PathBuf {
    PathBuf::from(format!("/private/tmp/claude-{}", unsafe { libc::getuid() }))
}

/// Paths that are never offered, nor any folder holding one.
fn protected(home: &Path) -> Vec<PathBuf> {
    [
        ".ssh",
        "Library/Keychains",
        "Library/MobileDevice/Provisioning Profiles",
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".claude/CLAUDE.md",
        ".codex/config.toml",
        ".config/gh",
        ".gnupg",
        ".aws",
        ".kube",
        ".docker/config.json",
    ]
    .iter()
    .map(|r| home.join(r))
    .collect()
}

const PROTECTED_NAMES: [&str; 3] = ["gradle.properties", "debug.keystore", "release.keystore"];

/// Whether `path` is protected, holds a protected path, or is home itself.
pub fn is_protected(path: &Path, home: &Path) -> bool {
    if path == home || home.starts_with(path) {
        return true;
    }
    if protected(home).iter().any(|p| p.starts_with(path) || path.starts_with(p)) {
        return true;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if PROTECTED_NAMES.contains(&name) || name.ends_with(".keystore") || name.ends_with(".jks") {
        return true;
    }
    false
}

/// A path that may be moved to the Trash: absolute, inside home (or the
/// user's agent scratch), not a link nor behind one, owned by the user,
/// and not protected. Returns the path to use.
pub fn validate(path: &Path, home: &Path) -> Result<PathBuf, String> {
    let refuse = |en: &'static str, nb: &'static str| Err(l(en, nb).to_string());
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return refuse("Not a plain path.", "Ikke en vanlig sti.");
    }
    let scratch = scratch_root();
    let in_scratch = path.starts_with(&scratch) && path != scratch;
    if !in_scratch && (!path.starts_with(home) || path == home) {
        return refuse("Outside your home folder.", "Utenfor hjemmemappen din.");
    }
    if is_protected(path, home) {
        return refuse("A protected file or folder.", "En beskyttet fil eller mappe.");
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return refuse("It no longer exists.", "Den finnes ikke lenger.");
    };
    if meta.file_type().is_symlink() {
        return refuse("It is a link, so it was left alone.", "Det er en lenke, så den ble latt være.");
    }
    if meta.uid() != unsafe { libc::getuid() } {
        return refuse("It belongs to another user.", "Den tilhører en annen bruker.");
    }
    if std::fs::canonicalize(path).ok().as_deref() != Some(path) {
        return refuse("It sits behind a link, so it was left alone.", "Den ligger bak en lenke, så den ble latt være.");
    }
    Ok(path.to_path_buf())
}

/// What a folder looked like when it was scanned.
pub fn identity(path: &Path) -> Option<(u64, u64)> {
    std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// The last check, right before removal: refuses when anything that made the
/// candidate safe has changed since the scan.
pub fn recheck(candidate: &Candidate, home: &Path, procs: &Procs) -> Result<(), String> {
    if !procs.ok {
        return Err(l("Running apps could not be checked.", "Kjørende apper kunne ikke sjekkes.").into());
    }
    let shutdown = matches!(candidate.method, Method::Tool(super::ToolAction::ShutdownSimulator(_)));
    if let Some(reason) = lock(candidate, procs).filter(|_| !shutdown) {
        return Err(reason);
    }
    match &candidate.method {
        Method::GitWorktree { .. } => {
            let path = &candidate.paths[0];
            validate(path, home)?;
            let changed = |en: &'static str, nb: &'static str| {
                Err(format!("{} {}", l("Changed since the scan:", "Endret siden skanningen:"), l(en, nb)))
            };
            if identity(path) != candidate.fingerprint.identity {
                return changed("the folder was replaced", "mappen ble byttet ut");
            }
            if super::git::head(path) != candidate.fingerprint.head {
                return changed("new commit", "ny commit");
            }
            match super::git::status(path) {
                Some(s) if s.changed.is_empty() && super::worktrees::status_hash(&s) == candidate.fingerprint.status => Ok(()),
                Some(_) => changed("files changed", "filer endret"),
                None => Err(l("git did not answer.", "git svarte ikke.").into()),
            }
        }
        Method::Prune { .. } | Method::KnownCache(_) | Method::Tool(_) => Ok(()),
        Method::Trash => {
            for path in &candidate.paths {
                validate(path, home)?;
                if let Some(id) = candidate.fingerprint.identity {
                    if candidate.paths.len() == 1 && identity(path) != Some(id) {
                        return Err(l("Changed since the scan: the folder was replaced.", "Endret siden skanningen: mappen ble byttet ut.").into());
                    }
                }
            }
            Ok(())
        }
        Method::None => Err(l("Kept items are never removed.", "Elementer som beholdes fjernes aldri.").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_cache_waits_only_for_its_own_tool() {
        use super::super::{Candidate, Category, Tier};
        let procs = Procs {
            list: vec![Proc { pid: 1, name: "cargo".into(), cwd: PathBuf::from("/"), args: vec![] }],
            ok: true,
        };
        let pip = Candidate::folder(Category::PackageCaches, PathBuf::from("/Users/u/Library/Caches/pip"), "pip".into(), Tier::Safe);
        let cargo = Candidate::folder(Category::PackageCaches, PathBuf::from("/Users/u/.cargo/registry/cache"), "c".into(), Tier::Rebuild);
        assert_eq!(lock(&pip, &procs), None);
        assert!(lock(&cargo, &procs).is_some());
        let xcode = Procs { list: vec![Proc { pid: 2, name: "Xcode".into(), ..Default::default() }], ok: true };
        let dd = Candidate::folder(Category::XcodeBuild, PathBuf::from("/Users/u/Library/Developer/Xcode/DerivedData/A"), "a".into(), Tier::Safe);
        assert!(lock(&dd, &xcode).is_some());
    }

    #[test]
    fn protected_paths_never_listed() {
        let home = Path::new("/Users/u");
        for p in [
            "/Users/u/.ssh",
            "/Users/u/.ssh/id_ed25519",
            "/Users/u/Library/Keychains",
            "/Users/u/.claude/settings.json",
            "/Users/u/app/android/gradle.properties",
            "/Users/u/app/debug.keystore",
            "/Users/u/app/sign.jks",
            "/Users/u",
        ] {
            assert!(is_protected(Path::new(p), home), "{p}");
        }
        assert!(!is_protected(Path::new("/Users/u/Dev/r/target"), home));
    }

    #[test]
    fn parent_of_protected_refused() {
        let home = Path::new("/Users/u");
        assert!(is_protected(Path::new("/Users/u/Library"), home));
        assert!(is_protected(Path::new("/Users/u/.claude"), home));
        assert!(is_protected(Path::new("/Users"), home));
    }

    #[test]
    fn validate_refuses_links_and_outside_home() {
        let dir = super::super::test_support::temp("gate");
        let home = std::fs::canonicalize(&dir).unwrap();
        let real = home.join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, home.join("link")).unwrap();
        assert!(validate(&real, &home).is_ok());
        assert!(validate(&home.join("link"), &home).is_err());
        assert!(validate(Path::new("/etc"), &home).is_err());
        assert!(validate(&home, &home).is_err());
        assert!(validate(&home.join("real/../real"), &home).is_err());
    }
}
