//! Known caches that are safe to clear, from a fixed list.
//!
//! Nothing here deletes a file itself. A cache folder goes to the Trash, so it
//! can be put back; tool-owned caches (Homebrew, Xcode's simulators) are
//! cleared by the tool's own command. Only the paths in `FIXED`, a pnpm store
//! and Homebrew's reported cache are ever considered, and a folder is only
//! moved after `validate` has confirmed it is a real folder inside the user's
//! home, not a link and not reached through one.
use crate::locale::text as l;
use std::collections::HashSet;
use std::os::macos::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Smallest cache worth listing, in the 1024-based units the app shows.
pub const MIN_BYTES: u64 = 100 * 1024 * 1024;

/// How long a scan stays current when the page is opened again.
const FRESH: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    DerivedData,
    DeviceSupport,
    Simulators,
    Homebrew,
    Npm,
    Pnpm,
    Yarn,
    Bun,
    Cargo,
    Trash,
}

/// What a button does to a cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// The folder goes to the Trash.
    Trash,
    /// The tool that owns the cache clears it.
    Tool,
    /// Information only; the button opens the Trash in Finder.
    Open,
}

impl Kind {
    pub fn method(self) -> Method {
        match self {
            Self::Homebrew | Self::Simulators => Method::Tool,
            Self::Trash => Method::Open,
            _ => Method::Trash,
        }
    }
}

/// Folders inside the home folder that can be moved to the Trash whole.
const FIXED: &[(Kind, &str)] = &[
    (Kind::DerivedData, "Library/Developer/Xcode/DerivedData"),
    (
        Kind::DeviceSupport,
        "Library/Developer/Xcode/iOS DeviceSupport",
    ),
    (Kind::Npm, ".npm/_cacache"),
    (Kind::Yarn, "Library/Caches/Yarn"),
    (Kind::Bun, ".bun/install/cache"),
    (Kind::Cargo, ".cargo/registry/cache"),
];

/// Where pnpm keeps its store when no other place is configured.
const PNPM_DEFAULT_PARENTS: &[&str] = &[
    "Library/pnpm/store",
    ".local/share/pnpm/store",
    ".pnpm-store",
];

#[derive(Clone, Debug)]
pub struct Cache {
    pub kind: Kind,
    /// The folders measured; for a Trash move, the folders that move.
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
    /// One line per item the action covers, when it covers named items
    /// (the unavailable simulators).
    pub items: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Inventory {
    pub caches: Vec<Cache>,
    /// Kinds whose size could not be read this time (a command failed or a
    /// folder could not be measured). A kind that is neither here nor in
    /// `caches` was confirmed gone.
    pub unmeasured: Vec<Kind>,
    pub scanned: Instant,
}

impl Inventory {
    pub fn is_fresh(&self) -> bool {
        self.scanned.elapsed() < FRESH
    }
    pub fn bytes_of(&self, kind: Kind) -> u64 {
        self.caches
            .iter()
            .find(|c| c.kind == kind)
            .map_or(0, |c| c.bytes)
    }
    /// What is left of a cache: its size, 0 when it is confirmed gone, and
    /// none when this scan could not measure it.
    pub fn remaining(&self, kind: Kind) -> Option<u64> {
        (!self.unmeasured.contains(&kind)).then(|| self.bytes_of(kind))
    }
}

/// What an action freed, or none when what is left could not be measured, so
/// that a failed rescan is never reported as everything freed.
pub fn freed_result(before: u64, after: Option<u64>) -> Option<u64> {
    after.map(|after| freed(before, after))
}

/// The folders that could be measured, with their total. A folder that could
/// not be measured is left out, so the size shown is the size that moves.
fn measured(paths: Vec<PathBuf>, size: impl Fn(&Path) -> Option<u64>) -> (Vec<PathBuf>, u64, bool) {
    let total = paths.len();
    let sized: Vec<(PathBuf, u64)> = paths
        .into_iter()
        .filter_map(|path| size(&path).map(|bytes| (path, bytes)))
        .collect();
    let complete = sized.len() == total;
    let bytes = sized.iter().map(|(_, bytes)| bytes).sum();
    (
        sized.into_iter().map(|(path, _)| path).collect(),
        bytes,
        complete,
    )
}

/// The caches worth showing: over the threshold, biggest first, with the
/// Trash (information only) last.
pub fn listed(caches: &[Cache]) -> Vec<&Cache> {
    let mut shown: Vec<&Cache> = caches.iter().filter(|c| c.bytes >= MIN_BYTES).collect();
    shown.sort_by(|a, b| {
        (a.kind == Kind::Trash)
            .cmp(&(b.kind == Kind::Trash))
            .then_with(|| b.bytes.cmp(&a.bytes))
            .then_with(|| a.paths.cmp(&b.paths))
    });
    shown
}

/// What an action freed: what the cache measured before, less what is left.
pub fn freed(before: u64, after: u64) -> u64 {
    before.saturating_sub(after)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAllowlisted,
    OutsideHome,
    Missing,
    Link,
    NotFolder,
}

impl Refusal {
    fn message(&self) -> String {
        match self {
            Self::NotAllowlisted => l(
                "That folder is not on the list of known caches.",
                "Mappen står ikke på listen over kjente bufre.",
            ),
            Self::OutsideHome => l(
                "That folder is outside your home folder.",
                "Mappen ligger utenfor hjemmemappen din.",
            ),
            Self::Missing => l(
                "That folder no longer exists.",
                "Mappen finnes ikke lenger.",
            ),
            Self::Link => l(
                "That folder is, or sits behind, a link, so it was left alone.",
                "Mappen er, eller ligger bak, en lenke, så den ble latt være.",
            ),
            Self::NotFolder => l("That path is not a folder.", "Stien er ikke en mappe."),
        }
        .to_string()
    }
}

/// A pnpm store is a `v<number>` folder inside a path that names pnpm.
fn is_pnpm_store(home: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(home) else {
        return false;
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let versioned = name
        .strip_prefix('v')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    versioned
        && rel.components().count() >= 2
        && rel.components().any(|c| {
            c.as_os_str()
                .to_string_lossy()
                .to_lowercase()
                .contains("pnpm")
        })
}

fn allowlisted(kind: Kind, home: &Path, path: &Path) -> bool {
    match kind {
        Kind::Pnpm => is_pnpm_store(home, path),
        _ => FIXED
            .iter()
            .any(|(k, rel)| *k == kind && path == home.join(rel)),
    }
}

/// Whether `path` may be moved to the Trash as the cache `kind`: an absolute
/// path that is on the allowlist, inside `home` (which must be canonical),
/// a real folder, and not reached through a link. Returns the path to use.
pub fn validate(kind: Kind, home: &Path, path: &Path) -> Result<PathBuf, Refusal> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(Refusal::NotAllowlisted);
    }
    if !path.starts_with(home) {
        return Err(Refusal::OutsideHome);
    }
    if !allowlisted(kind, home, path) {
        return Err(Refusal::NotAllowlisted);
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| Refusal::Missing)?;
    if meta.file_type().is_symlink() {
        return Err(Refusal::Link);
    }
    if !meta.is_dir() {
        return Err(Refusal::NotFolder);
    }
    // A link further up would let the move reach somewhere else.
    if std::fs::canonicalize(path).ok().as_deref() != Some(path) {
        return Err(Refusal::Link);
    }
    Ok(path.to_path_buf())
}

/// The user's home folder, resolved, or none when it cannot be trusted.
fn home() -> Option<PathBuf> {
    let home = std::fs::canonicalize(std::env::var_os("HOME").map(PathBuf::from)?).ok()?;
    (home.is_absolute() && home.components().count() > 1).then_some(home)
}

/// Allocated bytes under a folder, without following links, leaving the
/// volume, or entering cloud placeholders; a hard link counts once. None
/// when the folder cannot be read.
pub fn folder_bytes(root: &Path) -> Option<u64> {
    let top = std::fs::symlink_metadata(root).ok()?;
    if !top.is_dir() {
        return None;
    }
    std::fs::read_dir(root).ok()?;
    let device = top.st_dev();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // DirEntry::metadata does not follow links.
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.st_dev() != device || meta.st_flags() & 0x4000_0000 != 0 {
                continue;
            }
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file()
                && (meta.st_nlink() < 2 || seen.insert((meta.st_dev(), meta.st_ino())))
            {
                total = total.saturating_add(meta.st_blocks().saturating_mul(512));
            }
        }
    }
    Some(total)
}

fn brew() -> Option<PathBuf> {
    crate::packages::manager_executables(&["/opt/homebrew/bin/brew", "/usr/local/bin/brew"])
        .into_iter()
        .next()
}

/// Run a read-only query and return its first output line.
fn query(program: &Path, args: &[&str]) -> Option<String> {
    let output = crate::packages::run_manager(program, args, None, true, Duration::from_secs(5))?;
    output.status.success().then_some(())?;
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Homebrew's download cache, as Homebrew reports it. It may sit outside
/// the home folder, and is only measured: Homebrew clears it itself. Ok(None)
/// when there is no Homebrew or no cache folder; Err when Homebrew would not say.
fn brew_cache() -> Result<Option<PathBuf>, ()> {
    let Some(brew) = brew() else {
        return Ok(None);
    };
    let path = PathBuf::from(query(&brew, &["--cache"]).ok_or(())?);
    Ok(match std::fs::symlink_metadata(&path) {
        Ok(meta) if path.is_absolute() && meta.is_dir() && !meta.file_type().is_symlink() => {
            Some(path)
        }
        _ => None,
    })
}

/// Every pnpm store folder found: the one pnpm reports, else the usual places.
fn pnpm_stores(home: &Path) -> Vec<PathBuf> {
    let reported =
        crate::packages::manager_executables(&["/opt/homebrew/bin/pnpm", "/usr/local/bin/pnpm"])
            .first()
            .and_then(|pnpm| query(pnpm, &["store", "path"]))
            .map(PathBuf::from);
    let mut found: Vec<PathBuf> = match reported {
        Some(path) => vec![path],
        None => PNPM_DEFAULT_PARENTS
            .iter()
            .filter_map(|parent| std::fs::read_dir(home.join(parent)).ok())
            .flat_map(|entries| entries.flatten().map(|e| e.path()))
            .collect(),
    };
    found.sort();
    found.dedup();
    found
        .into_iter()
        .filter(|p| validate(Kind::Pnpm, home, p).is_ok() && p.join("files").is_dir())
        .collect()
}

/// Simulators whose runtime is gone: their names with data sizes, and the
/// total. Asked only when Xcode is selected and has simulators, so macOS
/// never opens its "install developer tools" prompt. Ok(None) when there is
/// nothing to ask or none are unavailable; Err when simctl would not answer.
fn unavailable_simulators(home: &Path) -> Result<Option<(Vec<String>, u64)>, ()> {
    if !home
        .join("Library/Developer/CoreSimulator/Devices")
        .is_dir()
    {
        return Ok(None);
    }
    let selected = Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !selected {
        return Ok(None);
    }
    let output = crate::packages::run_manager(
        Path::new("/usr/bin/xcrun"),
        &["simctl", "list", "devices", "unavailable", "-j"],
        None,
        true,
        Duration::from_secs(10),
    )
    .ok_or(())?;
    if !output.status.success() {
        return Err(());
    }
    let (items, total) = parse_simulators(&output.stdout).ok_or(())?;
    Ok((!items.is_empty()).then_some((items, total)))
}

/// The unavailable simulators in simctl's JSON; none when it is not simctl's JSON.
fn parse_simulators(json: &[u8]) -> Option<(Vec<String>, u64)> {
    let value: serde_json::Value = serde_json::from_slice(json).ok()?;
    let mut items = Vec::new();
    let mut total = 0u64;
    for devices in value.get("devices")?.as_object()?.values() {
        for device in devices.as_array()? {
            if device.get("isAvailable").and_then(|v| v.as_bool()) != Some(false) {
                continue;
            }
            let size = device
                .get("dataPathSize")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let name = device.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            total = total.saturating_add(size);
            items.push(format!(
                "{name} · {}",
                crate::engine::fmt_bytes(size as f64)
            ));
        }
    }
    Some((items, total))
}

/// Measure the folders of one cache and list the ones that could be read.
fn add_folders(
    caches: &mut Vec<Cache>,
    unmeasured: &mut Vec<Kind>,
    kind: Kind,
    paths: Vec<PathBuf>,
) {
    let (paths, bytes, complete) = measured(paths, folder_bytes);
    if !complete {
        unmeasured.push(kind);
    }
    if !paths.is_empty() {
        caches.push(Cache {
            kind,
            paths,
            bytes,
            items: Vec::new(),
        });
    }
}

/// Measure every known cache that exists. Blocking: run it off the main thread.
pub fn scan() -> Inventory {
    let mut caches = Vec::new();
    let mut unmeasured = Vec::new();
    let Some(home) = home() else {
        return Inventory {
            caches,
            unmeasured: vec![Kind::Homebrew, Kind::Simulators],
            scanned: Instant::now(),
        };
    };
    for (kind, rel) in FIXED {
        match validate(*kind, &home, &home.join(rel)) {
            Ok(path) => add_folders(&mut caches, &mut unmeasured, *kind, vec![path]),
            Err(Refusal::Missing) => {}
            Err(_) => unmeasured.push(*kind),
        }
    }
    let stores = pnpm_stores(&home);
    if !stores.is_empty() {
        add_folders(&mut caches, &mut unmeasured, Kind::Pnpm, stores);
    }
    match brew_cache() {
        Ok(Some(path)) => add_folders(&mut caches, &mut unmeasured, Kind::Homebrew, vec![path]),
        Ok(None) => {}
        Err(()) => unmeasured.push(Kind::Homebrew),
    }
    // The Trash is only information, and macOS may refuse to list it.
    add_folders(
        &mut caches,
        &mut unmeasured,
        Kind::Trash,
        vec![home.join(".Trash")],
    );
    match unavailable_simulators(&home) {
        Ok(Some((items, bytes))) => caches.push(Cache {
            kind: Kind::Simulators,
            paths: Vec::new(),
            bytes,
            items,
        }),
        Ok(None) => {}
        Err(()) => unmeasured.push(Kind::Simulators),
    }
    Inventory {
        caches,
        unmeasured,
        scanned: Instant::now(),
    }
}

fn tool_environment(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir("/")
        .envs([
            ("HOMEBREW_NO_AUTO_UPDATE", "1"),
            ("HOMEBREW_NO_ANALYTICS", "1"),
            ("NONINTERACTIVE", "1"),
        ]);
}

fn run_tool(mut command: Command) -> Result<(), String> {
    tool_environment(&mut command);
    let ok = crate::packages::run_command(command, crate::packages::ACTION_TIMEOUT)
        .is_ok_and(|o| o.status.success());
    if ok {
        Ok(())
    } else {
        Err(l(
            "The cleanup command failed or timed out.",
            "Opprydningskommandoen mislyktes eller fikk tidsavbrudd.",
        )
        .to_string())
    }
}

/// Clear one cache. Folders move to the Trash after being validated again;
/// Homebrew and Xcode's simulators are cleared by their own commands.
pub fn clear(cache: &Cache) -> Result<(), String> {
    match cache.kind.method() {
        Method::Trash => {
            let home = home().ok_or_else(|| Refusal::OutsideHome.message())?;
            for path in &cache.paths {
                let path = validate(cache.kind, &home, path).map_err(|r| r.message())?;
                crate::move_to_applications::trash(&path)?;
            }
            Ok(())
        }
        Method::Tool => {
            let mut command;
            if cache.kind == Kind::Homebrew {
                let brew = brew().ok_or_else(|| {
                    l("Homebrew was not found.", "Fant ikke Homebrew.").to_string()
                })?;
                command = Command::new(brew);
                command.args(["cleanup", "--prune=all"]);
            } else {
                command = Command::new("/usr/bin/xcrun");
                command.args(["simctl", "delete", "unavailable"]);
            }
            run_tool(command)
        }
        Method::Open => Err(Refusal::NotAllowlisted.message()),
    }
}

/// Show the Trash in Finder. It is never emptied from here.
pub fn open_trash() {
    if let Some(home) = home() {
        let _ = Command::new("/usr/bin/open")
            .arg(home.join(".Trash"))
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn sandbox(name: &str) -> PathBuf {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "tidegauge-caches-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn allowlisted_folders_inside_home_are_accepted() {
        let home = sandbox("ok");
        let npm = home.join(".npm/_cacache");
        std::fs::create_dir_all(&npm).unwrap();
        assert_eq!(validate(Kind::Npm, &home, &npm), Ok(npm.clone()));
        // The same folder under another kind's name is not that kind's cache.
        assert_eq!(
            validate(Kind::Bun, &home, &npm),
            Err(Refusal::NotAllowlisted)
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn paths_outside_home_or_off_the_list_are_refused() {
        let home = sandbox("refuse");
        let outside = sandbox("elsewhere");
        std::fs::create_dir_all(outside.join(".npm/_cacache")).unwrap();
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        assert_eq!(
            validate(Kind::Npm, &home, &outside.join(".npm/_cacache")),
            Err(Refusal::OutsideHome)
        );
        assert_eq!(
            validate(Kind::Npm, &home, &home),
            Err(Refusal::NotAllowlisted)
        );
        assert_eq!(
            validate(Kind::Npm, &home, &home.join("Documents")),
            Err(Refusal::NotAllowlisted)
        );
        assert_eq!(
            validate(Kind::Npm, &home, Path::new(".npm/_cacache")),
            Err(Refusal::NotAllowlisted)
        );
        assert_eq!(
            validate(Kind::Npm, &home, &home.join(".npm/../Documents")),
            Err(Refusal::NotAllowlisted)
        );
        assert_eq!(
            validate(Kind::Npm, &home, &home.join(".npm/_cacache")),
            Err(Refusal::Missing)
        );
        assert_eq!(
            validate(Kind::Trash, &home, &home.join(".Trash")),
            Err(Refusal::NotAllowlisted)
        );
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn links_never_carry_a_move_out_of_the_target() {
        let home = sandbox("links");
        let outside = sandbox("target");
        std::fs::create_dir_all(home.join(".npm")).unwrap();
        // The cache itself is a link to a folder elsewhere.
        symlink(&outside, home.join(".npm/_cacache")).unwrap();
        assert_eq!(
            validate(Kind::Npm, &home, &home.join(".npm/_cacache")),
            Err(Refusal::Link)
        );
        // A link above the cache would redirect the move just as well.
        std::fs::create_dir_all(outside.join("_cacache")).unwrap();
        std::fs::remove_file(home.join(".npm/_cacache")).unwrap();
        std::fs::remove_dir(home.join(".npm")).unwrap();
        symlink(&outside, home.join(".npm")).unwrap();
        assert_eq!(
            validate(Kind::Npm, &home, &home.join(".npm/_cacache")),
            Err(Refusal::Link)
        );
        // A file where the folder should be is not a folder.
        std::fs::remove_file(home.join(".npm")).unwrap();
        std::fs::create_dir_all(home.join(".npm")).unwrap();
        std::fs::write(home.join(".npm/_cacache"), b"x").unwrap();
        assert_eq!(
            validate(Kind::Npm, &home, &home.join(".npm/_cacache")),
            Err(Refusal::NotFolder)
        );
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn only_versioned_pnpm_stores_are_accepted() {
        let home = sandbox("pnpm");
        let store = home.join("Library/pnpm/store/v10");
        std::fs::create_dir_all(&store).unwrap();
        assert_eq!(validate(Kind::Pnpm, &home, &store), Ok(store.clone()));
        for odd in [
            "Library/pnpm/store",
            "Library/pnpm",
            "v10",
            "Documents/v10",
            "Library/pnpm/store/vX",
            "Library/pnpm/store/v",
        ] {
            let path = home.join(odd);
            std::fs::create_dir_all(&path).unwrap();
            assert_eq!(
                validate(Kind::Pnpm, &home, &path),
                Err(Refusal::NotAllowlisted),
                "{odd}"
            );
        }
        assert_eq!(
            validate(Kind::Pnpm, &home, &home),
            Err(Refusal::NotAllowlisted)
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    fn cache(kind: Kind, bytes: u64) -> Cache {
        Cache {
            kind,
            paths: vec![PathBuf::from(format!("/{bytes}"))],
            bytes,
            items: Vec::new(),
        }
    }

    #[test]
    fn small_caches_are_hidden_and_the_rest_sorted() {
        let found = [
            cache(Kind::Npm, MIN_BYTES - 1),
            cache(Kind::Bun, MIN_BYTES),
            cache(Kind::Cargo, MIN_BYTES * 5),
            cache(Kind::Trash, MIN_BYTES * 9),
            cache(Kind::Yarn, 0),
        ];
        let kinds: Vec<Kind> = listed(&found).iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [Kind::Cargo, Kind::Bun, Kind::Trash]);
    }

    #[test]
    fn freed_is_what_is_gone_and_never_negative() {
        assert_eq!(freed(3_000, 1_000), 2_000);
        assert_eq!(freed(3_000, 0), 3_000);
        assert_eq!(freed(1_000, 1_000), 0);
        assert_eq!(freed(1_000, 5_000), 0, "a cache that grew freed nothing");
        let inventory = Inventory {
            caches: vec![cache(Kind::Bun, 7)],
            unmeasured: vec![Kind::Homebrew],
            scanned: Instant::now(),
        };
        assert_eq!(inventory.bytes_of(Kind::Bun), 7);
        assert_eq!(inventory.bytes_of(Kind::Npm), 0);
    }

    #[test]
    fn a_cache_that_could_not_be_measured_is_not_reported_as_freed() {
        let inventory = Inventory {
            caches: vec![cache(Kind::Bun, 7)],
            unmeasured: vec![Kind::Homebrew, Kind::Bun],
            scanned: Instant::now(),
        };
        // Confirmed gone: measured as 0, so everything was freed.
        assert_eq!(inventory.remaining(Kind::Npm), Some(0));
        assert_eq!(
            freed_result(3_000, inventory.remaining(Kind::Npm)),
            Some(3_000)
        );
        // Still there and measured: only the difference was freed.
        let measured = Inventory {
            unmeasured: Vec::new(),
            ..inventory.clone()
        };
        assert_eq!(measured.remaining(Kind::Bun), Some(7));
        assert_eq!(
            freed_result(3_000, measured.remaining(Kind::Bun)),
            Some(2_993)
        );
        // The rescan could not say: no number at all, not "everything".
        assert_eq!(inventory.remaining(Kind::Homebrew), None);
        assert_eq!(
            freed_result(3_000, inventory.remaining(Kind::Homebrew)),
            None
        );
        assert_eq!(
            inventory.remaining(Kind::Bun),
            None,
            "partly measured is unknown"
        );
    }

    #[test]
    fn only_measured_folders_are_kept_so_the_size_shown_is_the_size_moved() {
        let sizes = |path: &Path| match path.to_str() {
            Some("/a") => Some(10),
            Some("/c") => Some(5),
            _ => None,
        };
        let paths = vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ];
        let (kept, bytes, complete) = measured(paths, sizes);
        assert_eq!(kept, [PathBuf::from("/a"), PathBuf::from("/c")]);
        assert_eq!(bytes, 15);
        assert!(!complete, "the unmeasured folder is reported");
        let (kept, bytes, complete) = measured(vec![PathBuf::from("/b")], sizes);
        assert!(kept.is_empty() && bytes == 0 && !complete);
        let (kept, bytes, complete) = measured(vec![PathBuf::from("/a")], sizes);
        assert_eq!((kept.len(), bytes, complete), (1, 10, true));
    }

    #[test]
    fn trash_and_tools_are_told_apart() {
        assert_eq!(Kind::DerivedData.method(), Method::Trash);
        assert_eq!(Kind::Pnpm.method(), Method::Trash);
        assert_eq!(Kind::Homebrew.method(), Method::Tool);
        assert_eq!(Kind::Simulators.method(), Method::Tool);
        assert_eq!(Kind::Trash.method(), Method::Open);
        assert!(
            clear(&cache(Kind::Trash, MIN_BYTES)).is_err(),
            "the Trash is never emptied"
        );
    }

    #[test]
    fn folder_bytes_counts_hard_links_once_and_ignores_links() {
        let root = sandbox("size");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/file"), vec![1u8; 8192]).unwrap();
        std::fs::hard_link(root.join("a/file"), root.join("a/b/again")).unwrap();
        let outside = sandbox("big");
        std::fs::write(outside.join("big"), vec![1u8; 1 << 20]).unwrap();
        symlink(&outside, root.join("a/out")).unwrap();
        symlink(outside.join("big"), root.join("a/outfile")).unwrap();
        let size = folder_bytes(&root).unwrap();
        assert!((8192..8192 + 65536).contains(&size), "{size}");
        assert_eq!(folder_bytes(&root.join("missing")), None);
        assert_eq!(
            folder_bytes(&root.join("a/file")),
            None,
            "a file is not a folder"
        );
        assert_eq!(
            folder_bytes(&root.join("a/out")),
            None,
            "a link is not followed"
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn unavailable_simulators_are_summed_from_simctl() {
        let json = br#"{"devices":{"rt":[
            {"name":"iPhone A","isAvailable":false,"dataPathSize":3000},
            {"name":"iPhone B","isAvailable":true,"dataPathSize":9000},
            {"name":"iPhone C","isAvailable":false,"dataPathSize":500}]}}"#;
        let (items, total) = parse_simulators(json).unwrap();
        assert_eq!(total, 3500);
        assert_eq!(items.len(), 2);
        assert!(items[0].starts_with("iPhone A"));
        let (none, zero) = parse_simulators(br#"{"devices":{}}"#).unwrap();
        assert!(
            none.is_empty() && zero == 0,
            "none unavailable is a measurement"
        );
        assert!(parse_simulators(b"not json").is_none());
    }

    #[test]
    #[ignore = "Measures this Mac's real caches and asks Homebrew, pnpm and simctl for paths; read-only"]
    fn native_scan_measures_without_changing_anything() {
        let inventory = scan();
        for cache in &inventory.caches {
            println!(
                "{:?} {} bytes {:?} {:?}",
                cache.kind, cache.bytes, cache.paths, cache.items
            );
        }
        println!(
            "listed: {:?}",
            listed(&inventory.caches)
                .iter()
                .map(|c| c.kind)
                .collect::<Vec<_>>()
        );
        assert!(inventory.is_fresh());
    }

    #[test]
    #[ignore = "Moves a temporary folder it creates into the real Trash; run once by hand"]
    fn native_trash_moves_the_folder_and_leaves_its_contents() {
        let home = sandbox("trash");
        let target = home.join(".npm/_cacache");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("entry"), b"cache").unwrap();
        let outside = sandbox("kept");
        std::fs::write(outside.join("kept"), b"keep").unwrap();
        symlink(&outside, target.join("out")).unwrap();
        assert_eq!(validate(Kind::Npm, &home, &target), Ok(target.clone()));
        crate::move_to_applications::trash(&target).unwrap();
        assert!(!target.exists(), "the folder left its place");
        assert!(
            outside.join("kept").exists(),
            "a link inside never carried the move out"
        );
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}
