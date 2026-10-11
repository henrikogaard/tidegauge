//! Bounded inventory and narrowly scoped actions for installed packages.
//!
//! Inventory is intentionally partial. It covers installed applications,
//! Homebrew's installed formulae/casks, and global npm packages without
//! executing shell profiles or project configuration.

use crate::locale::text as localized;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 512;
const MAX_TOTAL_DIRECTORY_ENTRIES: usize = 2048;
const MAX_PACKAGES_PER_MANAGER: usize = 512;
const MAX_NPM_CANDIDATES: usize = 16;
const MAX_APP_DEPTH: usize = 2;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const REGISTRY_TIMEOUT: Duration = Duration::from_secs(10);
const ACTION_TIMEOUT: Duration = Duration::from_secs(600);
const SCAN_TIMEOUT: Duration = Duration::from_secs(40);
const TERMINATION_GRACE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    App,
    BrewFormula,
    BrewCask,
    Npm,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::App => localized("App", "Program"),
            Self::BrewFormula => localized("Homebrew formula", "Homebrew-formel"),
            Self::BrewCask => localized("Homebrew cask", "Homebrew-cask"),
            Self::Npm => localized("Global npm package", "Global npm-pakke"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source: Source,
    pub path: PathBuf,
    pub latest: Option<String>,
    pub can_update: bool,
    pub can_remove: bool,
    pub reason: String,
    identity: Identity,
}

#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub items: Vec<Package>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Update,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    name: String,
    version: String,
    source: Source,
    path: PathBuf,
    manager: Option<PathBuf>,
    prefix: Option<PathBuf>,
    runtime: Option<PathBuf>,
    manager_fingerprint: Option<FsFingerprint>,
    runtime_fingerprint: Option<FsFingerprint>,
    prefix_fingerprint: Option<FsFingerprint>,
    path_fingerprint: Option<FsFingerprint>,
    related_fingerprints: Vec<FsFingerprint>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FsFingerprint {
    canonical_path: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    owner: u32,
    group: u32,
    mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SourceKey {
    source: Source,
    manager: FsFingerprint,
    prefix: FsFingerprint,
    runtime: Option<FsFingerprint>,
}

impl Identity {
    fn source_key(&self) -> Option<SourceKey> {
        if self.source == Source::Npm && self.runtime_fingerprint.is_none() {
            return None;
        }
        Some(SourceKey {
            source: self.source,
            manager: self.manager_fingerprint.clone()?,
            prefix: self.prefix_fingerprint.clone()?,
            runtime: self.runtime_fingerprint.clone(),
        })
    }
}

fn source_key_for(
    source: Source,
    manager: &Path,
    prefix: &Path,
    runtime: Option<&Path>,
) -> Option<SourceKey> {
    let runtime = match runtime {
        Some(runtime) => Some(FsFingerprint::capture(runtime)?),
        None => None,
    };
    Some(SourceKey {
        source,
        manager: FsFingerprint::capture(manager)?,
        prefix: FsFingerprint::capture(prefix)?,
        runtime,
    })
}

fn capture_identity(
    name: &str,
    version: &str,
    source: Source,
    path: &Path,
    manager: Option<PathBuf>,
    prefix: Option<PathBuf>,
    runtime: Option<PathBuf>,
    related_paths: &[PathBuf],
) -> Identity {
    capture_identity_with_related_fingerprints(
        name,
        version,
        source,
        path,
        manager,
        prefix,
        runtime,
        related_paths,
        None,
    )
}

fn capture_identity_with_related_fingerprints(
    name: &str,
    version: &str,
    source: Source,
    path: &Path,
    manager: Option<PathBuf>,
    prefix: Option<PathBuf>,
    runtime: Option<PathBuf>,
    related_paths: &[PathBuf],
    expected_related: Option<&[FsFingerprint]>,
) -> Identity {
    Identity {
        name: name.to_owned(),
        version: version.to_owned(),
        source,
        path: path.to_path_buf(),
        manager_fingerprint: manager.as_deref().and_then(FsFingerprint::capture),
        runtime_fingerprint: runtime.as_deref().and_then(FsFingerprint::capture),
        prefix_fingerprint: prefix.as_deref().and_then(FsFingerprint::capture),
        path_fingerprint: FsFingerprint::capture(path),
        related_fingerprints: expected_related.map(ToOwned::to_owned).unwrap_or_else(|| {
            related_paths
                .iter()
                .filter_map(|path| FsFingerprint::capture(path))
                .collect()
        }),
        manager,
        prefix,
        runtime,
    }
}

impl FsFingerprint {
    fn capture(path: &Path) -> Option<Self> {
        let canonical_path = fs::canonicalize(path).ok()?;
        if path_has_symlink_component(&canonical_path) {
            return None;
        }
        let metadata = fs::symlink_metadata(&canonical_path).ok()?;
        if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
            return None;
        }
        Some(Self {
            canonical_path,
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            owner: metadata.uid(),
            group: metadata.gid(),
            mode: metadata.mode(),
        })
    }
}

fn fingerprints_are_current(identity: &Identity) -> bool {
    let current = |expected: &Option<FsFingerprint>| {
        expected.as_ref().is_some_and(|expected| {
            FsFingerprint::capture(&expected.canonical_path).as_ref() == Some(expected)
        })
    };
    if !current(&identity.path_fingerprint) {
        return false;
    }
    if identity.manager.is_some() && !current(&identity.manager_fingerprint) {
        return false;
    }
    if identity.prefix.is_some() && !current(&identity.prefix_fingerprint) {
        return false;
    }
    if identity.runtime.is_some() && !current(&identity.runtime_fingerprint) {
        return false;
    }
    identity
        .related_fingerprints
        .iter()
        .all(|expected| FsFingerprint::capture(&expected.canonical_path).as_ref() == Some(expected))
}

struct ScanSnapshot {
    inventory: Inventory,
    complete_sources: HashSet<SourceKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagerKind {
    BrewFormula,
    BrewCask,
    Npm,
}

struct ManagerRecord {
    package: Package,
    overlap_paths: Vec<PathBuf>,
}

struct CaskMetadata {
    targets: Vec<PathBuf>,
    related_paths: Vec<PathBuf>,
    related_fingerprints: Vec<FsFingerprint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActionPlan {
    program: PathBuf,
    args: Vec<String>,
    manager: ManagerKind,
}

static ACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct ScanBudget {
    started: Instant,
    remaining_entries: usize,
    exceeded: bool,
}

impl ScanBudget {
    fn consume_entry(&mut self) -> bool {
        if self.started.elapsed() >= SCAN_TIMEOUT || self.remaining_entries == 0 {
            self.exceeded = true;
            return false;
        }
        self.remaining_entries -= 1;
        true
    }

    fn remaining_time(&self) -> Duration {
        SCAN_TIMEOUT.saturating_sub(self.started.elapsed())
    }
}

/// Scan the bounded set of supported package sources.
pub fn scan(check_updates: bool) -> Inventory {
    scan_detailed(check_updates).inventory
}

fn scan_detailed(check_updates: bool) -> ScanSnapshot {
    let mut budget = ScanBudget {
        started: Instant::now(),
        remaining_entries: MAX_TOTAL_DIRECTORY_ENTRIES,
        exceeded: false,
    };
    let home = actual_home();
    let mut inventory = Inventory::default();
    let mut complete_sources = HashSet::new();
    let mut app_paths = HashSet::new();
    if let Some(home) = home.as_ref() {
        discover_apps(
            &home.join("Applications"),
            0,
            &mut app_paths,
            &mut inventory,
            &mut budget,
        );
    }
    discover_apps(
        Path::new("/Applications"),
        0,
        &mut app_paths,
        &mut inventory,
        &mut budget,
    );
    discover_apps(
        Path::new("/System/Applications"),
        0,
        &mut app_paths,
        &mut inventory,
        &mut budget,
    );

    if budget.exceeded || budget.started.elapsed() >= SCAN_TIMEOUT {
        inventory.warnings.push(
            localized(
                "Package scanning reached its time or item limit.",
                "Pakkeskanningen nådde tids- eller elementgrensen.",
            )
            .to_string(),
        );
    }
    let brew_records = if !budget.exceeded && !budget.remaining_time().is_zero() {
        discover_brew(
            check_updates,
            &mut inventory,
            &budget,
            &mut complete_sources,
        )
    } else {
        Vec::new()
    };
    let npm_records = if !budget.exceeded && !budget.remaining_time().is_zero() {
        discover_npm(
            check_updates,
            &mut inventory,
            &budget,
            &mut complete_sources,
        )
    } else {
        Vec::new()
    };
    if budget.remaining_time().is_zero() {
        inventory.warnings.push(
            localized(
                "Package scanning reached its time limit.",
                "Pakkeskanningen nådde tidsgrensen.",
            )
            .to_string(),
        );
    }
    let mut records = brew_records
        .into_iter()
        .chain(npm_records)
        .collect::<Vec<_>>();
    mark_overlapping_paths(&mut inventory.items, &mut records);
    inventory
        .items
        .extend(records.into_iter().map(|record| record.package));
    inventory.items.sort_by(|left, right| {
        (left.source as u8, &left.name, &left.path).cmp(&(
            right.source as u8,
            &right.name,
            &right.path,
        ))
    });
    inventory.warnings.sort();
    inventory.warnings.dedup();
    ScanSnapshot {
        inventory,
        complete_sources,
    }
}

/// Update or remove exactly one package after fresh identity validation.
pub fn act(package: &Package, action: Action) -> Result<String, String> {
    let lock = ACTION_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().map_err(|_| {
        localized(
            "Package actions are temporarily unavailable.",
            "Pakkehandlinger er midlertidig utilgjengelige.",
        )
        .to_string()
    })?;
    if package.source == Source::App {
        return Err(localized(
            "Applications can only be revealed or managed by their own updater.",
            "Programmer kan bare vises eller håndteres av sin egen oppdaterer.",
        )
        .to_string());
    }
    let before = scan_detailed(false);
    let fresh = preflight_action(package, &before, action)?;
    let plan = action_plan(&fresh, action)?;
    if !fingerprints_are_current(&fresh.identity) {
        return Err(identity_changed_message());
    }
    let action_result = run_action(&plan);
    let after = scan_detailed(false);
    let key = fresh
        .identity
        .source_key()
        .ok_or_else(identity_changed_message)?;
    let unique_after = unique_source_package(&after.inventory, &key, &fresh.name)?;
    verify_action_outcome(
        action,
        &fresh.version,
        action_result.is_ok(),
        after.complete_sources.contains(&key),
        unique_after,
    )
}

fn unique_source_package<'a>(
    inventory: &'a Inventory,
    key: &SourceKey,
    name: &str,
) -> Result<Option<&'a Package>, String> {
    let mut matching = inventory
        .items
        .iter()
        .filter(|item| item.identity.source_key().as_ref() == Some(key) && item.name == name);
    let first = matching.next();
    if matching.next().is_some() {
        return Err(outcome_unverified());
    }
    Ok(first)
}

fn preflight_action(
    package: &Package,
    snapshot: &ScanSnapshot,
    action: Action,
) -> Result<Package, String> {
    let Some(key) = package.identity.source_key() else {
        return Err(identity_changed_message());
    };
    if !snapshot.complete_sources.contains(&key) {
        return Err(localized(
            "The package source could not be read completely; no action was taken.",
            "Pakkekilden kunne ikke leses fullstendig; ingen handling ble utført.",
        )
        .to_string());
    }
    let mut matching = snapshot.inventory.items.iter().filter(|item| {
        item.source == package.source
            && item.name == package.name
            && item.identity.source_key().as_ref() == Some(&key)
    });
    let Some(fresh) = matching.next() else {
        return Err(localized(
            "The package is no longer present in a complete inventory.",
            "Pakken finnes ikke lenger i en fullstendig oversikt.",
        )
        .to_string());
    };
    if matching.next().is_some()
        || fresh.identity != package.identity
        || fresh.path != package.path
        || fresh.version != package.version
        || fresh.name != package.name
        || fresh.source != package.source
    {
        return Err(identity_changed_message());
    }
    if (action == Action::Update && !fresh.can_update)
        || (action == Action::Remove && !fresh.can_remove)
    {
        return Err(fresh.reason.clone());
    }
    if !fingerprints_are_current(&fresh.identity) {
        return Err(identity_changed_message());
    }
    Ok(fresh.clone())
}

fn verify_action_outcome(
    action: Action,
    before_version: &str,
    action_succeeded: bool,
    source_complete: bool,
    after: Option<&Package>,
) -> Result<String, String> {
    if !action_succeeded {
        return Err(localized(
            "The action may have partly completed; verify package state and repair it if needed.",
            "Handlingen kan være delvis fullført; kontroller pakkestatus og reparer ved behov.",
        )
        .to_string());
    }
    if !source_complete {
        return Err(outcome_unverified());
    }
    match (action, after) {
        (Action::Remove, None) => {
            Ok(localized("The package was removed.", "Pakken ble fjernet.").to_string())
        }
        (Action::Update, Some(package)) if package.version != before_version => {
            Ok(localized("The package was updated.", "Pakken ble oppdatert.").to_string())
        }
        _ => Err(outcome_unverified()),
    }
}

fn package_action_eligibility(
    official: bool,
    is_manager: bool,
    path_verified: bool,
    simple_cask: bool,
    pinned: Option<bool>,
) -> (bool, bool) {
    let eligible = official && !is_manager && path_verified && simple_cask;
    (
        eligible && pinned == Some(false),
        eligible && pinned.is_some(),
    )
}

fn identity_changed_message() -> String {
    localized(
        "The package or manager identity changed; no action was taken.",
        "Pakken eller håndtereridentiteten ble endret; ingen handling ble utført.",
    )
    .to_string()
}

fn discover_apps(
    root: &Path,
    depth: usize,
    app_paths: &mut HashSet<PathBuf>,
    inventory: &mut Inventory,
    budget: &mut ScanBudget,
) {
    if depth > MAX_APP_DEPTH || budget.exceeded {
        return;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            inventory.warnings.push(
                localized(
                    "An application directory could not be read.",
                    "En programkatalog kunne ikke leses.",
                )
                .to_string(),
            );
            return;
        }
    };
    for (index, entry) in entries.enumerate() {
        if !budget.consume_entry() {
            break;
        }
        if index >= MAX_DIRECTORY_ENTRIES {
            inventory.warnings.push(
                localized(
                    "An application directory exceeded the bounded scan.",
                    "En programkatalog overskred den begrensede skanningen.",
                )
                .to_string(),
            );
            break;
        }
        let Ok(entry) = entry else {
            inventory.warnings.push(
                localized(
                    "An application entry could not be inspected.",
                    "En programoppføring kunne ikke undersøkes.",
                )
                .to_string(),
            );
            continue;
        };
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("app")
            && metadata.is_dir()
        {
            let Ok(canonical) = fs::canonicalize(&path) else {
                continue;
            };
            if !app_paths.insert(canonical.clone()) {
                continue;
            }
            match parse_app(&canonical, budget.remaining_time()) {
                Ok(package) => inventory.items.push(package),
                Err(message) => inventory.warnings.push(message.to_string()),
            }
        } else if metadata.is_dir() && depth < MAX_APP_DEPTH {
            discover_apps(&path, depth + 1, app_paths, inventory, budget);
        }
    }
}

fn parse_app(path: &Path, timeout: Duration) -> Result<Package, &'static str> {
    let info = path.join("Contents/Info.plist");
    let bytes = read_bounded_file(&info).map_err(|_| {
        localized(
            "An application Info.plist could not be read safely.",
            "Info.plist for et program kunne ikke leses trygt.",
        )
    })?;
    let json = plutil_json(&bytes, timeout).map_err(|_| {
        localized(
            "An application Info.plist could not be parsed.",
            "Info.plist for et program kunne ikke tolkes.",
        )
    })?;
    let dictionary = json.as_object().ok_or_else(|| {
        localized(
            "An application has invalid metadata.",
            "Et program har ugyldige metadata.",
        )
    })?;
    let name = dictionary
        .get("CFBundleDisplayName")
        .or_else(|| dictionary.get("CFBundleName"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            localized(
                "An application has no usable name.",
                "Et program har ikke et brukbart navn.",
            )
        })?;
    let version = dictionary
        .get("CFBundleShortVersionString")
        .or_else(|| dictionary.get("CFBundleVersion"))
        .and_then(Value::as_str)
        .unwrap_or(localized("Unknown", "Ukjent"));
    let related_paths = [info];
    Ok(Package {
        name: name.to_owned(),
        version: version.to_owned(),
        source: Source::App,
        path: path.to_path_buf(),
        latest: None,
        can_update: false,
        can_remove: false,
        reason: localized(
            "Reveal the application or use its own updater.",
            "Vis programmet eller bruk sin egen oppdaterer.",
        )
        .to_string(),
        identity: capture_identity(
            name,
            version,
            Source::App,
            path,
            None,
            None,
            None,
            &related_paths,
        ),
    })
}

fn discover_brew(
    check_updates: bool,
    inventory: &mut Inventory,
    budget: &ScanBudget,
    complete_sources: &mut HashSet<SourceKey>,
) -> Vec<ManagerRecord> {
    let mut records = Vec::new();
    for brew in manager_executables(&["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]) {
        if budget.remaining_time().is_zero() {
            break;
        }
        records.extend(discover_brew_at(
            &brew,
            check_updates,
            inventory,
            budget,
            complete_sources,
        ));
    }
    mark_duplicate_manager_packages(&mut records);
    records
}

fn discover_brew_at(
    brew: &Path,
    check_updates: bool,
    inventory: &mut Inventory,
    budget: &ScanBudget,
    complete_sources: &mut HashSet<SourceKey>,
) -> Vec<ManagerRecord> {
    let Some(prefix_output) = run_manager(
        brew,
        &["--prefix"],
        None,
        true,
        COMMAND_TIMEOUT.min(budget.remaining_time()),
    ) else {
        inventory.warnings.push(
            localized(
                "Homebrew installation prefix could not be verified.",
                "Installasjonsprefikset for Homebrew kunne ikke bekreftes.",
            )
            .to_string(),
        );
        return Vec::new();
    };
    if !prefix_output.status.success() {
        inventory.warnings.push(
            localized(
                "Homebrew could not report its installation prefix.",
                "Homebrew kunne ikke rapportere installasjonsprefikset sitt.",
            )
            .to_string(),
        );
        return Vec::new();
    }
    let Some(prefix_text) = bounded_utf8(&prefix_output.stdout) else {
        inventory.warnings.push(
            localized(
                "Homebrew installation prefix was not valid bounded text.",
                "Homebrew-installasjonsprefikset var ikke gyldig avgrenset tekst.",
            )
            .to_string(),
        );
        return Vec::new();
    };
    let prefix_reported = PathBuf::from(prefix_text.trim());
    let Some(prefix) = fs::canonicalize(&prefix_reported)
        .ok()
        .filter(|prefix| trusted_brew_prefix(prefix, brew))
    else {
        inventory.warnings.push(
            localized(
                "Homebrew reported an untrusted installation prefix.",
                "Homebrew rapporterte et utrygt installasjonsprefiks.",
            )
            .to_string(),
        );
        return Vec::new();
    };
    let Some(output) = run_manager(
        brew,
        &["info", "--json=v2", "--installed"],
        None,
        true,
        COMMAND_TIMEOUT.min(budget.remaining_time()),
    ) else {
        inventory.warnings.push(
            localized(
                "Homebrew inventory could not be read.",
                "Homebrew-inventaret kunne ikke leses.",
            )
            .to_string(),
        );
        return Vec::new();
    };
    if !output.status.success() {
        inventory.warnings.push(
            localized(
                "Homebrew inventory returned an error.",
                "Homebrew-inventaret returnerte en feil.",
            )
            .to_string(),
        );
        return Vec::new();
    }
    let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) else {
        inventory.warnings.push(
            localized(
                "Homebrew inventory was not valid JSON.",
                "Homebrew-inventaret var ikke gyldig JSON.",
            )
            .to_string(),
        );
        return Vec::new();
    };
    if !json.is_object() {
        inventory.warnings.push(
            localized(
                "Homebrew inventory had an invalid top-level structure.",
                "Homebrew-inventaret hadde en ugyldig toppnivåstruktur.",
            )
            .to_string(),
        );
        return Vec::new();
    }
    let updates = if check_updates {
        brew_outdated(&brew, inventory, budget.remaining_time())
    } else {
        HashMap::new()
    };
    let mut records = Vec::new();
    for (key, source) in [
        ("formulae", Source::BrewFormula),
        ("casks", Source::BrewCask),
    ] {
        let Some(items) = json.get(key).and_then(Value::as_array) else {
            inventory.warnings.push(
                localized(
                    "Homebrew inventory omitted a package category.",
                    "Homebrew-inventaret utelot en pakkekategori.",
                )
                .to_string(),
            );
            continue;
        };
        let mut category_complete = items.len() <= MAX_PACKAGES_PER_MANAGER;
        for item in items.iter().take(MAX_PACKAGES_PER_MANAGER) {
            let source_count = records
                .iter()
                .filter(|record: &&ManagerRecord| record.package.source == source)
                .count();
            if source_count >= MAX_PACKAGES_PER_MANAGER {
                inventory.warnings.push(
                    localized(
                        "Homebrew inventory reached its package limit.",
                        "Homebrew-inventaret nådde pakkebegrensningen.",
                    )
                    .to_string(),
                );
                category_complete = false;
                break;
            }
            let Some(name) = item
                .get("full_name")
                .or_else(|| item.get("full_token"))
                .or_else(|| item.get("name"))
                .or_else(|| item.get("token"))
                .and_then(Value::as_str)
                .filter(|name| valid_manager_name(name))
            else {
                inventory.warnings.push(
                    localized(
                        "A Homebrew package had missing or unsafe identity data.",
                        "En Homebrew-pakke manglet eller hadde utrygge identitetsdata.",
                    )
                    .to_string(),
                );
                category_complete = false;
                continue;
            };
            let multiple_versions = item
                .get("installed")
                .and_then(Value::as_array)
                .is_some_and(|installed| installed.len() > 1);
            let version = brew_version(item).or_else(|| {
                multiple_versions
                    .then(|| localized("Multiple versions", "Flere versjoner").to_string())
            });
            let Some(version) = version else {
                inventory.warnings.push(
                    localized(
                        "A Homebrew package had no installed version.",
                        "En Homebrew-pakke hadde ingen installert versjon.",
                    )
                    .to_string(),
                );
                category_complete = false;
                continue;
            };
            let version_unambiguous = brew_version(item).is_some();
            let (path, path_verified, _overlap_paths) =
                brew_package_path(item, source, name, &version, &prefix);
            let path_verified = path_verified && version_unambiguous;
            let tap = item.get("tap").and_then(Value::as_str).unwrap_or_default();
            let official = official_tap(tap, source);
            let pinned = item.get("pinned").and_then(Value::as_bool);
            let latest = updates.get(name).cloned();
            let package_token = name.rsplit('/').next().unwrap_or(name);
            let is_manager = matches!(
                package_token.split('@').next().unwrap_or(package_token),
                "npm" | "node" | "corepack" | "brew" | "homebrew"
            );
            let cask_metadata = if source == Source::BrewCask {
                cask_metadata(
                    item,
                    &prefix,
                    name.rsplit('/').next().unwrap_or(name),
                    &version,
                )
            } else {
                None
            };
            let overlap_paths = cask_metadata
                .as_ref()
                .map(|metadata| metadata.targets.clone())
                .unwrap_or_default();
            let related_paths = cask_metadata
                .as_ref()
                .map(|metadata| metadata.related_paths.clone())
                .unwrap_or_else(|| cask_target_snapshot_paths(&overlap_paths));
            let identity = capture_identity_with_related_fingerprints(
                name,
                &version,
                source,
                &path,
                Some(brew.to_path_buf()),
                Some(prefix.clone()),
                None,
                &related_paths,
                cask_metadata
                    .as_ref()
                    .map(|metadata| metadata.related_fingerprints.as_slice()),
            );
            let target_snapshots_complete =
                identity.related_fingerprints.len() == related_paths.len();
            let simple_cask = source != Source::BrewCask
                || (cask_metadata.is_some()
                    && !overlap_paths.is_empty()
                    && target_snapshots_complete);
            let (can_update, can_remove) = package_action_eligibility(
                official,
                is_manager,
                path_verified,
                simple_cask,
                pinned,
            );
            let reason = if !official {
                localized(
                    "This Homebrew tap is not an official core or cask source; actions are disabled.",
                    "Denne Homebrew-tappen er ikke en offisiell core- eller cask-kilde; handlinger er deaktivert.",
                ).to_string()
            } else if multiple_versions {
                localized(
                    "Multiple installed versions make this package read-only.",
                    "Flere installerte versjoner gjør denne pakken skrivebeskyttet.",
                )
                .to_string()
            } else if is_manager {
                localized(
                    "Package managers and runtimes cannot be removed here.",
                    "Pakkehåndterere og kjøremiljøer kan ikke fjernes her.",
                )
                .to_string()
            } else if !path_verified {
                localized(
                    "The installed package path could not be verified; actions are disabled.",
                    "Den installerte pakkestien kunne ikke bekreftes; handlinger er deaktivert.",
                )
                .to_string()
            } else if pinned.is_none() {
                localized(
                    "Homebrew package state is incomplete; actions are disabled.",
                    "Homebrew-pakketstatus er ufullstendig; handlinger er deaktivert.",
                )
                .to_string()
            } else if pinned == Some(true) {
                localized(
                    "This Homebrew package is pinned; unpin it before updating.",
                    "Denne Homebrew-pakken er låst; lås den opp før oppdatering.",
                )
                .to_string()
            } else if source == Source::BrewCask && cask_metadata.is_none() {
                localized(
                    "Installed cask metadata could not be verified; actions are disabled.",
                    "Installert cask-metadata kunne ikke bekreftes; handlinger er deaktivert.",
                )
                .to_string()
            } else if !simple_cask {
                localized(
                    "This cask has installer or non-app artifacts; actions are disabled.",
                    "Denne casken har installasjons- eller andre artefakter enn programmer; handlinger er deaktivert.",
                )
                .to_string()
            } else {
                localized(
                    "Homebrew may change dependencies; package actions are not atomic.",
                    "Homebrew kan endre avhengigheter; pakkehandlinger er ikke atomiske.",
                )
                .to_string()
            };
            records.push(ManagerRecord {
                package: Package {
                    name: name.to_owned(),
                    version: version.clone(),
                    source,
                    path: path.clone(),
                    latest,
                    can_update,
                    can_remove,
                    reason,
                    identity,
                },
                overlap_paths,
            });
        }
        if category_complete {
            if let Some(key) = source_key_for(source, brew, &prefix, None) {
                complete_sources.insert(key);
            } else {
                inventory.warnings.push(
                    localized(
                        "A Homebrew source identity could not be captured.",
                        "En Homebrew-kildeidentitet kunne ikke registreres.",
                    )
                    .to_string(),
                );
            }
        }
    }
    records
}

fn discover_npm(
    check_updates: bool,
    inventory: &mut Inventory,
    budget: &ScanBudget,
    complete_sources: &mut HashSet<SourceKey>,
) -> Vec<ManagerRecord> {
    let candidates = npm_candidates();
    let mut records = Vec::new();
    let mut seen_contexts = HashSet::new();
    for npm in candidates {
        if budget.remaining_time().is_zero() {
            break;
        }
        let Some(runtime) = resolve_node_runtime(&npm) else {
            inventory.warnings.push(
                localized(
                    "A global npm executable had no matching trusted Node runtime.",
                    "Et globalt npm-program manglet et tilhørende betrodd Node-kjøremiljø.",
                )
                .to_string(),
            );
            continue;
        };
        let Some(prefix_output) = run_npm(
            &npm,
            &runtime,
            &["prefix".to_owned(), "--global".to_owned()],
            COMMAND_TIMEOUT.min(budget.remaining_time()),
        ) else {
            inventory.warnings.push(
                localized(
                    "Global npm prefix could not be read.",
                    "Global npm-prefiks kunne ikke leses.",
                )
                .to_string(),
            );
            continue;
        };
        if !prefix_output.status.success() {
            inventory.warnings.push(
                localized(
                    "Global npm prefix returned an error.",
                    "Global npm-prefiks returnerte en feil.",
                )
                .to_string(),
            );
            continue;
        }
        let Some(prefix_text) = bounded_utf8(&prefix_output.stdout) else {
            inventory.warnings.push(
                localized(
                    "Global npm prefix was not valid bounded text.",
                    "Globalt npm-prefiks var ikke gyldig avgrenset tekst.",
                )
                .to_string(),
            );
            continue;
        };
        let prefix_reported = PathBuf::from(prefix_text.trim());
        let Some(prefix) = fs::canonicalize(&prefix_reported).ok() else {
            inventory.warnings.push(
                localized(
                    "Global npm reported an unavailable prefix.",
                    "Global npm rapporterte et utilgjengelig prefiks.",
                )
                .to_string(),
            );
            continue;
        };
        if !prefix.is_absolute() || path_has_symlink_component(&prefix) {
            inventory.warnings.push(
                localized(
                    "Global npm reported an unsafe prefix.",
                    "Global npm rapporterte et utrygt prefiks.",
                )
                .to_string(),
            );
            continue;
        }
        if !seen_contexts.insert((npm.clone(), prefix.clone())) {
            continue;
        }
        let prefix_verified = trusted_prefix(&prefix, &npm);
        let list_args = [
            "ls".to_owned(),
            "--global".to_owned(),
            "--prefix".to_owned(),
            prefix.to_string_lossy().into_owned(),
            "--depth=0".to_owned(),
            "--json".to_owned(),
            "--long".to_owned(),
        ];
        let Some(list_output) = run_npm(
            &npm,
            &runtime,
            &list_args,
            COMMAND_TIMEOUT.min(budget.remaining_time()),
        ) else {
            inventory.warnings.push(
                localized(
                    "Global npm inventory could not be read.",
                    "Global npm-inventaret kunne ikke leses.",
                )
                .to_string(),
            );
            continue;
        };
        let inventory_healthy = list_output.status.success();
        if !inventory_healthy {
            inventory.warnings.push(
                localized(
                    "Global npm inventory reported a partial or invalid dependency tree; actions are disabled.",
                    "Global npm-inventaret rapporterte et delvis eller ugyldig avhengighetstre; handlinger er deaktivert.",
                )
                .to_string(),
            );
        }
        let Ok(json) = serde_json::from_slice::<Value>(&list_output.stdout) else {
            inventory.warnings.push(
                localized(
                    "Global npm inventory was not valid JSON.",
                    "Global npm-inventaret var ikke gyldig JSON.",
                )
                .to_string(),
            );
            continue;
        };
        let Some(dependencies) = json.get("dependencies").and_then(Value::as_object) else {
            inventory.warnings.push(
                localized(
                    "Global npm inventory omitted its dependency list.",
                    "Global npm-inventaret utelot avhengighetslisten.",
                )
                .to_string(),
            );
            continue;
        };
        let updates = if check_updates {
            npm_outdated(
                &npm,
                &runtime,
                &prefix,
                inventory,
                REGISTRY_TIMEOUT.min(budget.remaining_time()),
            )
        } else {
            HashMap::new()
        };
        let mut source_complete = inventory_healthy
            && dependencies.len() <= MAX_PACKAGES_PER_MANAGER
            && FsFingerprint::capture(&prefix).is_some();
        for (index, (name, item)) in dependencies.iter().enumerate() {
            if index >= MAX_PACKAGES_PER_MANAGER {
                inventory.warnings.push(
                    localized(
                        "Global npm inventory reached its package limit.",
                        "Global npm-inventaret nådde pakkebegrensningen.",
                    )
                    .to_string(),
                );
                source_complete = false;
                break;
            }
            let valid_name = valid_npm_name(name);
            let version = item
                .get("version")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or(localized("Unknown", "Ukjent"));
            if item.get("version").and_then(Value::as_str).is_none() {
                inventory.warnings.push(
                    localized(
                        "A global npm package had no installed version.",
                        "En global npm-pakke hadde ingen installert versjon.",
                    )
                    .to_string(),
                );
                source_complete = false;
            };
            let expected = valid_name.then(|| prefix.join("lib/node_modules").join(name));
            let reported = item.get("path").and_then(Value::as_str).map(PathBuf::from);
            if reported.is_none() {
                inventory.warnings.push(
                    localized(
                        "A global npm package had no installed path.",
                        "En global npm-pakke hadde ingen installert sti.",
                    )
                    .to_string(),
                );
                source_complete = false;
            }
            let raw_path = reported
                .clone()
                .or_else(|| expected.clone())
                .unwrap_or_else(|| prefix.join("lib/node_modules"));
            let canonical_path = fs::canonicalize(&raw_path).ok();
            let linked = item.get("link").and_then(Value::as_bool) == Some(true);
            let resolved_official = npm_resolved_is_official(item, name);
            let path_verified = npm_package_path_verified(
                prefix_verified,
                inventory_healthy,
                valid_name,
                linked,
                resolved_official,
                reported.as_deref(),
                expected.as_deref(),
            );
            let manager_package = matches!(
                name.as_str(),
                "npm" | "node" | "corepack" | "brew" | "homebrew"
            );
            let package_path = canonical_path.clone().unwrap_or(raw_path);
            let package_json = package_path.join("package.json");
            let related_paths = [package_json];
            let identity = capture_identity(
                name,
                version,
                Source::Npm,
                &package_path,
                Some(npm.clone()),
                Some(prefix.clone()),
                Some(runtime.clone()),
                &related_paths,
            );
            let target_fingerprints_complete = identity.path_fingerprint.is_some()
                && identity.related_fingerprints.len() == related_paths.len();
            let writable = path_verified && !manager_package && target_fingerprints_complete;
            let latest = updates.get(name).cloned();
            let reason = if manager_package {
                localized(
                    "This package manages the Node/npm installation; use its owning manager.",
                    "Denne pakken håndterer Node/npm-installasjonen; bruk eierens pakkehåndterer.",
                )
                .to_string()
            } else if writable {
                localized(
                    "npm may run package lifecycle scripts and change dependencies; review before continuing.",
                    "npm kan kjøre pakkelivssyklusskript og endre avhengigheter; vurder dette før du fortsetter.",
                )
                .to_string()
            } else {
                localized(
                    "This npm package is linked, custom-resolved, invalid, or outside the trusted global location; actions are disabled.",
                    "Denne npm-pakken er lenket, hentet fra en egendefinert kilde, ugyldig eller utenfor den betrodde globale plasseringen; handlinger er deaktivert.",
                ).to_string()
            };
            records.push(ManagerRecord {
                package: Package {
                    name: name.clone(),
                    version: version.to_owned(),
                    source: Source::Npm,
                    path: package_path,
                    latest,
                    can_update: writable,
                    can_remove: writable,
                    reason,
                    identity,
                },
                overlap_paths: Vec::new(),
            });
        }
        if source_complete {
            if let Some(key) = source_key_for(Source::Npm, &npm, &prefix, Some(&runtime)) {
                complete_sources.insert(key);
            } else {
                inventory.warnings.push(
                    localized(
                        "A global npm source identity could not be captured.",
                        "En global npm-kildeidentitet kunne ikke registreres.",
                    )
                    .to_string(),
                );
            }
        }
    }
    mark_duplicate_manager_packages(&mut records);
    records
}

fn mark_overlapping_paths(apps: &mut Vec<Package>, managers: &mut [ManagerRecord]) {
    let app_paths: HashSet<PathBuf> = apps.iter().map(|app| app.path.clone()).collect();
    let mut owners = HashMap::<PathBuf, Vec<usize>>::new();
    for (index, record) in managers.iter().enumerate() {
        if record.package.source == Source::BrewCask {
            for path in &record.overlap_paths {
                if app_paths.contains(path) {
                    owners.entry(path.clone()).or_default().push(index);
                }
            }
        }
    }
    let mut owned_app_paths = HashSet::new();
    let mut ambiguous_app_paths = HashSet::new();
    for (path, casks) in &owners {
        if casks.len() == 1 {
            owned_app_paths.insert(path.clone());
        } else {
            ambiguous_app_paths.insert(path.clone());
        }
    }
    for app in apps.iter_mut() {
        if ambiguous_app_paths.contains(&app.path) {
            app.can_update = false;
            app.can_remove = false;
            app.reason = ambiguous_ownership_reason();
        }
    }
    for record in managers {
        if record.package.source == Source::BrewCask
            && record
                .overlap_paths
                .iter()
                .any(|path| ambiguous_app_paths.contains(path))
        {
            record.package.can_update = false;
            record.package.can_remove = false;
            record.package.reason = ambiguous_ownership_reason();
        }
    }
    apps.retain(|app| !owned_app_paths.contains(&app.path));
}

fn ambiguous_ownership_reason() -> String {
    localized(
        "This application and cask target have ambiguous ownership; package actions are disabled.",
        "Eierskapet til dette programmet og cask-målet er tvetydig; pakkehandlinger er deaktivert.",
    )
    .to_string()
}

fn action_plan(package: &Package, action: Action) -> Result<ActionPlan, String> {
    let manager = match package.source {
        Source::BrewFormula => ManagerKind::BrewFormula,
        Source::BrewCask => ManagerKind::BrewCask,
        Source::Npm => ManagerKind::Npm,
        Source::App => {
            return Err(localized(
                "Applications do not support package-manager actions.",
                "Programmer støtter ikke handlinger via pakkehåndterer.",
            )
            .to_string())
        }
    };
    if !valid_manager_name(&package.name) && manager != ManagerKind::Npm
        || !valid_npm_name(&package.name) && manager == ManagerKind::Npm
    {
        return Err(localized(
            "The package name is not safe for a manager action.",
            "Pakkenavnet er ikke trygt for en pakkehåndtererhandling.",
        )
        .to_string());
    }
    let args = match manager {
        ManagerKind::BrewFormula | ManagerKind::BrewCask => {
            let kind = if manager == ManagerKind::BrewFormula {
                "formula"
            } else {
                "cask"
            };
            let verb = if action == Action::Update {
                "upgrade"
            } else {
                "uninstall"
            };
            vec![verb.to_owned(), format!("--{kind}"), package.name.clone()]
        }
        ManagerKind::Npm => {
            let verb = if action == Action::Update {
                "update"
            } else {
                "uninstall"
            };
            let npm = package
                .identity
                .manager
                .as_ref()
                .ok_or_else(identity_changed_message)?;
            vec![
                npm.to_string_lossy().into_owned(),
                verb.to_owned(),
                "--global".to_owned(),
                "--prefix".to_owned(),
                package
                    .identity
                    .prefix
                    .as_ref()
                    .ok_or_else(identity_changed_message)?
                    .to_string_lossy()
                    .into_owned(),
                "--".to_owned(),
                package.name.clone(),
            ]
        }
    };
    let program = if manager == ManagerKind::Npm {
        package.identity.runtime.clone()
    } else {
        package.identity.manager.clone()
    }
    .ok_or_else(identity_changed_message)?;
    Ok(ActionPlan {
        program,
        args,
        manager,
    })
}

fn run_action(plan: &ActionPlan) -> Result<String, String> {
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    command.stdin(Stdio::null());
    command.current_dir("/");
    if matches!(
        plan.manager,
        ManagerKind::BrewFormula | ManagerKind::BrewCask
    ) {
        command.envs([
            ("HOMEBREW_NO_AUTO_UPDATE", "1"),
            ("HOMEBREW_NO_ANALYTICS", "1"),
            ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
            ("HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK", "1"),
            ("HOMEBREW_NO_AUTOREMOVE", "1"),
            ("NONINTERACTIVE", "1"),
        ]);
    }
    if plan.manager == ManagerKind::Npm {
        command.env("PATH", deterministic_node_path(&plan.program));
    }
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let output = run_command(command, ACTION_TIMEOUT).map_err(|_| {
        localized(
            "The package manager action could not be started or timed out.",
            "Pakkehåndtererhandlingen kunne ikke startes eller fikk tidsavbrudd.",
        )
        .to_string()
    })?;
    if output.status.success() {
        Ok(localized(
            "The manager completed the action.",
            "Håndtereren fullførte handlingen.",
        )
        .to_string())
    } else {
        Err(localized(
            "The package manager rejected the action.",
            "Pakkehåndtereren avviste handlingen.",
        )
        .to_string())
    }
}

fn outcome_unverified() -> String {
    localized(
        "The package manager returned, but the requested result could not be verified.",
        "Pakkehåndtereren returnerte, men det forespurte resultatet kunne ikke bekreftes.",
    )
    .to_string()
}

fn brew_outdated(
    brew: &Path,
    inventory: &mut Inventory,
    timeout: Duration,
) -> HashMap<String, String> {
    let Some(output) = run_manager(brew, &["outdated", "--json=v2"], None, true, timeout) else {
        inventory.warnings.push(
            localized(
                "Homebrew update information could not be read.",
                "Homebrew-oppdateringsinformasjon kunne ikke leses.",
            )
            .to_string(),
        );
        return HashMap::new();
    };
    if !output.status.success() && output.status.code() != Some(1) {
        inventory.warnings.push(
            localized(
                "Homebrew update information could not be read.",
                "Homebrew-oppdateringsinformasjon kunne ikke leses.",
            )
            .to_string(),
        );
        return HashMap::new();
    }
    parse_brew_outdated(&output.stdout).unwrap_or_else(|| {
        inventory.warnings.push(
            localized(
                "Homebrew update information was not valid JSON.",
                "Homebrew-oppdateringsinformasjonen var ikke gyldig JSON.",
            )
            .to_string(),
        );
        HashMap::new()
    })
}

fn npm_outdated(
    npm: &Path,
    runtime: &Path,
    prefix: &Path,
    inventory: &mut Inventory,
    timeout: Duration,
) -> HashMap<String, String> {
    let args = [
        "outdated".to_owned(),
        "--global".to_owned(),
        "--prefix".to_owned(),
        prefix.to_string_lossy().into_owned(),
        "--depth=0".to_owned(),
        "--json".to_owned(),
    ];
    let Some(output) = run_npm(npm, runtime, &args, timeout) else {
        inventory.warnings.push(
            localized(
                "Global npm update information could not be read.",
                "Global npm-oppdateringsinformasjon kunne ikke leses.",
            )
            .to_string(),
        );
        return HashMap::new();
    };
    if !output.status.success() && output.status.code() != Some(1) {
        inventory.warnings.push(
            localized(
                "Global npm update information could not be read.",
                "Global npm-oppdateringsinformasjon kunne ikke leses.",
            )
            .to_string(),
        );
        return HashMap::new();
    }
    parse_npm_outdated(&output.stdout).unwrap_or_else(|| {
        inventory.warnings.push(
            localized(
                "Global npm update information was not valid JSON.",
                "Global npm-oppdateringsinformasjonen var ikke gyldig JSON.",
            )
            .to_string(),
        );
        HashMap::new()
    })
}

fn parse_brew_outdated(bytes: &[u8]) -> Option<HashMap<String, String>> {
    let json: Value = serde_json::from_slice(bytes).ok()?;
    let mut result = HashMap::new();
    let mut found_category = false;
    for key in ["formulae", "casks"] {
        let Some(items) = json.get(key).and_then(Value::as_array) else {
            continue;
        };
        found_category = true;
        for item in items {
            let name = item
                .get("full_name")
                .or_else(|| item.get("full_token"))
                .or_else(|| item.get("name"))
                .or_else(|| item.get("token"))?
                .as_str()?;
            let latest = item
                .get("latest_version")
                .or_else(|| item.get("latest"))
                .or_else(|| item.get("wanted"))?
                .as_str()?;
            result.insert(name.to_owned(), latest.to_owned());
        }
    }
    found_category.then_some(result)
}

fn parse_npm_outdated(bytes: &[u8]) -> Option<HashMap<String, String>> {
    let json: Value = serde_json::from_slice(bytes).ok()?;
    let object = json.as_object()?;
    let mut result = HashMap::new();
    for (name, item) in object {
        if let Some(version) = item.get("latest").and_then(Value::as_str) {
            result.insert(name.clone(), version.to_owned());
        }
    }
    Some(result)
}

fn npm_resolved_is_official(item: &Value, name: &str) -> bool {
    let Some(resolved) = item.get("resolved").and_then(Value::as_str) else {
        return false;
    };
    if resolved.contains('?') || resolved.contains('#') {
        return false;
    }
    let Some(path) = resolved.strip_prefix("https://registry.npmjs.org/") else {
        return false;
    };
    let encoded_name = name.replace('/', "%2f");
    let upper_encoded_name = name.replace('/', "%2F");
    path.starts_with(&format!("{name}/-/"))
        || path.starts_with(&format!("{encoded_name}/-/"))
        || path.starts_with(&format!("{upper_encoded_name}/-/"))
}

fn npm_package_path_verified(
    prefix_trusted: bool,
    inventory_healthy: bool,
    valid_name: bool,
    linked: bool,
    resolved_official: bool,
    reported: Option<&Path>,
    expected: Option<&Path>,
) -> bool {
    let (Some(reported), Some(expected)) = (reported, expected) else {
        return false;
    };
    let reported_canonical = fs::canonicalize(reported).ok();
    let expected_canonical = fs::canonicalize(expected).ok();
    prefix_trusted
        && inventory_healthy
        && valid_name
        && !linked
        && resolved_official
        && reported_canonical.is_some()
        && reported_canonical == expected_canonical
        && !path_has_symlink_component(reported)
        && !path_has_symlink_component(expected)
}

fn mark_duplicate_manager_packages(records: &mut [ManagerRecord]) {
    let mut counts = HashMap::<(Source, String), usize>::new();
    for record in records.iter() {
        *counts
            .entry((record.package.source, record.package.name.clone()))
            .or_default() += 1;
    }
    for record in records {
        if counts
            .get(&(record.package.source, record.package.name.clone()))
            .copied()
            .unwrap_or_default()
            > 1
        {
            record.package.can_update = false;
            record.package.can_remove = false;
            record.package.reason = localized(
                "This package identity is duplicated in manager data; actions are disabled.",
                "Denne pakkeidentiteten finnes flere ganger i håndtererdata; handlinger er deaktivert.",
            )
            .to_string();
        }
    }
}

fn brew_version(item: &Value) -> Option<String> {
    if let Some(version) = item.get("installed").and_then(Value::as_str) {
        return (!version.is_empty()).then(|| version.to_owned());
    }
    let installed = item.get("installed").and_then(Value::as_array)?;
    if installed.len() != 1 {
        return None;
    }
    installed
        .first()
        .and_then(|installed| installed.get("version"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn brew_package_path(
    item: &Value,
    source: Source,
    name: &str,
    version: &str,
    prefix: &Path,
) -> (PathBuf, bool, Vec<PathBuf>) {
    let token = name.rsplit('/').next().unwrap_or(name);
    let root_name = if source == Source::BrewCask {
        "Caskroom"
    } else {
        "Cellar"
    };
    let managed_root = prefix.join(root_name);
    let reported = if source == Source::BrewFormula {
        item.get("installed")
            .and_then(Value::as_array)
            .and_then(|installed| installed.first())
            .and_then(|installed| installed.get("prefix"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
    } else {
        None
    };
    let raw_path = reported.unwrap_or_else(|| managed_root.join(token).join(version));
    let canonical_root = fs::canonicalize(&managed_root).ok();
    let canonical_path = fs::canonicalize(&raw_path).ok();
    let path_verified = canonical_root.as_ref().is_some_and(|root| {
        canonical_path
            .as_ref()
            .is_some_and(|path| path.starts_with(root) && path.is_dir())
    }) && !path_has_symlink_component(&raw_path);
    let path = canonical_path.unwrap_or(raw_path);
    let overlap_paths = Vec::new();
    (path, path_verified, overlap_paths)
}

fn cask_metadata(item: &Value, prefix: &Path, token: &str, version: &str) -> Option<CaskMetadata> {
    let config_path = prefix
        .join("Caskroom")
        .join(token)
        .join(".metadata/config.json");
    let receipt_path = prefix
        .join("Caskroom")
        .join(token)
        .join(".metadata/INSTALL_RECEIPT.json");
    let (config, config_fingerprint) = read_trusted_json(&config_path)?;
    let (receipt, receipt_fingerprint) = read_trusted_json(&receipt_path)?;
    let appdir = cask_appdir(&config)?;
    let current = resolve_cask_targets(item.get("artifacts")?, &appdir)?;
    let receipt_object = receipt.as_object()?;
    if receipt_object
        .get("uninstall_flight_blocks")
        .and_then(Value::as_bool)
        != Some(false)
        || receipt_object
            .get("source")
            .and_then(Value::as_object)
            .and_then(|source| source.get("tap"))
            .and_then(Value::as_str)
            != Some("homebrew/cask")
        || receipt_object
            .get("source")
            .and_then(Value::as_object)
            .and_then(|source| source.get("version"))
            .and_then(Value::as_str)
            != Some(version)
    {
        return None;
    }
    let receipt_targets =
        resolve_cask_targets(receipt_object.get("uninstall_artifacts")?, &appdir)?;
    if current != receipt_targets {
        return None;
    }
    let mut related_paths = vec![config_path.clone(), receipt_path.clone()];
    related_paths.extend(cask_target_snapshot_paths(&current));
    let mut related_fingerprints = vec![config_fingerprint, receipt_fingerprint];
    related_fingerprints.extend(
        related_paths
            .iter()
            .skip(2)
            .map(|path| FsFingerprint::capture(path))
            .collect::<Option<Vec<_>>>()?,
    );
    if FsFingerprint::capture(&config_path).as_ref() != Some(&related_fingerprints[0])
        || FsFingerprint::capture(&receipt_path).as_ref() != Some(&related_fingerprints[1])
    {
        return None;
    }
    Some(CaskMetadata {
        targets: current,
        related_paths,
        related_fingerprints,
    })
}

fn cask_appdir(config: &Value) -> Option<PathBuf> {
    let mut appdir = None;
    for key in ["explicit", "env", "default"] {
        if let Some(section) = config.get(key) {
            let object = section.as_object()?;
            if let Some(value) = object.get("appdir") {
                appdir = Some(value.as_str()?);
                break;
            }
        }
    }
    let appdir = appdir?;
    let path = PathBuf::from(appdir);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| part == std::path::Component::ParentDir)
        || path_has_symlink_component(&path)
    {
        return None;
    }
    let canonical = fs::canonicalize(&path).ok()?;
    if !trusted_directory_path(&canonical) {
        return None;
    }
    Some(canonical)
}

fn resolve_cask_targets(artifacts: &Value, appdir: &Path) -> Option<Vec<PathBuf>> {
    let artifacts = artifacts.as_array()?;
    let mut targets = HashSet::new();
    for artifact in artifacts {
        let object = artifact.as_object()?;
        if object.len() == 1 && object.contains_key("zap") {
            continue;
        }
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "app" | "target"))
        {
            return None;
        }
        let app = object.get("app")?.as_array()?;
        if app.is_empty() || app.len() > 2 {
            return None;
        }
        let source = app[0].as_str()?;
        let source_path = Path::new(source);
        if source_path.is_absolute()
            || source_path.components().count() != 1
            || source_path.file_name() != Some(source_path.as_os_str())
            || source_path.extension().and_then(|ext| ext.to_str()) != Some("app")
        {
            return None;
        }
        let target = if app.len() == 1 {
            appdir.join(source_path)
        } else {
            let target = app[1].as_object()?;
            if target.len() != 1 {
                return None;
            }
            let target = target.get("target")?.as_str()?;
            let target_path = Path::new(target);
            if target_path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
            {
                return None;
            }
            if target_path.is_absolute() {
                target_path.to_path_buf()
            } else {
                appdir.join(target_path)
            }
        };
        let canonical = canonical_app_target(&target)?;
        if !canonical.starts_with(appdir) || !trusted_directory_path(&canonical) {
            return None;
        }
        if let Some(reported_target) = object.get("target") {
            let reported_target = Path::new(reported_target.as_str()?);
            if !reported_target.is_absolute()
                || reported_target
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
                || canonical_app_target(reported_target)? != canonical
            {
                return None;
            }
        }
        if !targets.insert(canonical) {
            return None;
        }
    }
    let mut targets: Vec<_> = targets.into_iter().collect();
    targets.sort();
    (!targets.is_empty()).then_some(targets)
}

fn trusted_metadata_path(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    let uid = unsafe { libc::getuid() };
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && metadata.mode() & 0o022 == 0
        && (metadata.uid() == 0 || metadata.uid() == uid)
        && !path_has_symlink_component(path)
        && trusted_metadata_ancestors(path)
}

fn trusted_directory_path(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    let uid = unsafe { libc::getuid() };
    metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && directory_mode_is_trusted(metadata.mode(), metadata.gid(), trusted_admin_gid())
        && (metadata.uid() == 0 || metadata.uid() == uid)
        && !path_has_symlink_component(path)
        && trusted_metadata_ancestors(path)
}

fn trusted_metadata_ancestors(path: &Path) -> bool {
    let uid = unsafe { libc::getuid() };
    let mut current = path.parent();
    while let Some(directory) = current {
        let Ok(metadata) = fs::symlink_metadata(directory) else {
            return false;
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !directory_mode_is_trusted(metadata.mode(), metadata.gid(), trusted_admin_gid())
            || (metadata.uid() != 0 && metadata.uid() != uid)
        {
            return false;
        }
        if directory == Path::new("/") {
            break;
        }
        current = directory.parent();
    }
    true
}

fn read_trusted_json(path: &Path) -> Option<(Value, FsFingerprint)> {
    let before = FsFingerprint::capture(path)?;
    if !trusted_metadata_path(path) {
        return None;
    }
    let bytes = read_bounded_file(path).ok()?;
    let after = FsFingerprint::capture(path)?;
    if before != after {
        return None;
    }
    Some((serde_json::from_slice(&bytes).ok()?, before))
}

fn cask_target_snapshot_paths(targets: &[PathBuf]) -> Vec<PathBuf> {
    targets
        .iter()
        .flat_map(|target| [target.clone(), target.join("Contents/Info.plist")])
        .collect()
}

fn canonical_app_target(path: &Path) -> Option<PathBuf> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("app")
        || path_has_symlink_component(path)
    {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    canonical.is_dir().then_some(canonical)
}

fn official_tap(tap: &str, source: Source) -> bool {
    match source {
        Source::BrewFormula => tap == "homebrew/core",
        Source::BrewCask => tap == "homebrew/cask",
        _ => false,
    }
}

fn manager_executables(candidates: &[&str]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    candidates
        .iter()
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .filter(|canonical| {
            trusted_executable(canonical)
                && (canonical.starts_with("/opt/homebrew") || canonical.starts_with("/usr/local"))
                && seen.insert(canonical.clone())
        })
        .collect()
}

fn npm_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/npm"),
        PathBuf::from("/usr/local/bin/npm"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .filter(|directory| directory.is_absolute())
                .take(32)
                .map(|directory| directory.join("npm")),
        );
    }
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|path| {
            let canonical = fs::canonicalize(path).ok()?;
            if trusted_executable(&canonical) && seen.insert(canonical.clone()) {
                Some(canonical)
            } else {
                None
            }
        })
        .take(MAX_NPM_CANDIDATES)
        .collect()
}

fn resolve_node_runtime(npm: &Path) -> Option<PathBuf> {
    let allowed_root = if npm.starts_with("/opt/homebrew") {
        Path::new("/opt/homebrew")
    } else if npm.starts_with("/usr/local") {
        Path::new("/usr/local")
    } else {
        return None;
    };
    let mut candidates = Vec::new();
    let mut ancestor = npm.parent();
    while let Some(path) = ancestor {
        candidates.push(path.join("node"));
        candidates.push(path.join("bin/node"));
        ancestor = path.parent();
    }
    candidates.push(allowed_root.join("bin/node"));
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .filter(|directory| directory.is_absolute())
                .take(32)
                .map(|directory| directory.join("node")),
        );
    }
    let mut seen = HashSet::new();
    candidates.into_iter().find_map(|candidate| {
        let canonical = fs::canonicalize(candidate).ok()?;
        (canonical.starts_with(allowed_root)
            && trusted_executable(&canonical)
            && seen.insert(canonical.clone()))
        .then_some(canonical)
    })
}

fn deterministic_node_path(runtime: &Path) -> std::ffi::OsString {
    let mut paths = Vec::new();
    if let Some(parent) = runtime.parent() {
        paths.push(parent.to_path_buf());
    }
    paths.extend([
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);
    std::env::join_paths(paths).unwrap_or_default()
}

fn run_npm(npm: &Path, runtime: &Path, args: &[String], timeout: Duration) -> Option<Output> {
    if timeout.is_zero() {
        return None;
    }
    let mut command = Command::new(runtime);
    command
        .arg(npm)
        .args(args)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("PATH", deterministic_node_path(runtime));
    run_command(command, timeout.min(REGISTRY_TIMEOUT)).ok()
}

fn trusted_brew_prefix(prefix: &Path, brew: &Path) -> bool {
    let roots = [Path::new("/opt/homebrew"), Path::new("/usr/local")]
        .into_iter()
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .collect::<Vec<_>>();
    brew_prefix_matches_roots(prefix, brew, &roots)
        && FsFingerprint::capture(prefix).is_some()
        && !path_has_symlink_component(prefix)
}

fn brew_prefix_matches_roots(prefix: &Path, brew: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| prefix == root && brew.starts_with(root))
}

fn trusted_prefix(prefix: &Path, npm: &Path) -> bool {
    let Ok(prefix) = fs::canonicalize(prefix) else {
        return false;
    };
    [Path::new("/opt/homebrew"), Path::new("/usr/local")]
        .into_iter()
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .any(|candidate| candidate == prefix && npm.starts_with(&candidate))
}

fn trusted_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let uid = unsafe { libc::getuid() };
    if !metadata.is_file()
        || !executable_mode_is_trusted(metadata.mode())
        || (metadata.uid() != 0 && metadata.uid() != uid)
    {
        return false;
    }
    let mut current = path.parent();
    while let Some(directory) = current {
        let Ok(metadata) = fs::symlink_metadata(directory) else {
            return false;
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !directory_mode_is_trusted(metadata.mode(), metadata.gid(), trusted_admin_gid())
            || (metadata.uid() != 0 && metadata.uid() != uid)
        {
            return false;
        }
        if directory == Path::new("/") {
            break;
        }
        current = directory.parent();
    }
    true
}

fn executable_mode_is_trusted(mode: u32) -> bool {
    mode & 0o111 != 0 && mode & 0o022 == 0
}

fn directory_mode_is_trusted(mode: u32, gid: u32, admin_gid: Option<u32>) -> bool {
    mode & 0o002 == 0 && (mode & 0o020 == 0 || Some(gid) == admin_gid)
}

fn trusted_admin_gid() -> Option<u32> {
    static ADMIN: OnceLock<Option<u32>> = OnceLock::new();
    *ADMIN.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            let mut group = unsafe { std::mem::zeroed::<libc::group>() };
            let mut result = std::ptr::null_mut();
            let mut buffer = vec![0u8; 16 * 1024];
            let status = unsafe {
                libc::getgrnam_r(
                    c"admin".as_ptr(),
                    &mut group,
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut result,
                )
            };
            (status == 0 && !result.is_null()).then_some(group.gr_gid)
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    })
}
fn path_has_symlink_component(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            return true;
        };
        if metadata.file_type().is_symlink() {
            return true;
        }
    }
    false
}

fn valid_manager_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+' | b'@')
                })
        })
}

fn valid_npm_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 214 {
        return false;
    }
    let valid_component = |component: &str| {
        let mut bytes = component.bytes();
        matches!(bytes.next(), Some(byte) if byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && bytes.all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'~' | b'-')
            })
            && component != "."
            && component != ".."
    };
    if let Some(scoped) = name.strip_prefix('@') {
        let Some((scope, package)) = scoped.split_once('/') else {
            return false;
        };
        !package.contains('/') && valid_component(scope) && valid_component(package)
    } else {
        !name.contains('/') && valid_component(name)
    }
}

fn actual_home() -> Option<PathBuf> {
    let uid = unsafe { libc::getuid() };
    let mut passwd = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16 * 1024];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || passwd.pw_dir.is_null() {
        return None;
    }
    let home = unsafe { std::ffi::CStr::from_ptr(passwd.pw_dir) }
        .to_str()
        .ok()?;
    Some(PathBuf::from(home))
}

fn read_bounded_file(path: &Path) -> Result<Vec<u8>, ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_JSON_BYTES {
        return Err(());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|_| ())?;
    let opened = file.metadata().map_err(|_| ())?;
    if opened.ino() != metadata.ino() || opened.dev() != metadata.dev() {
        return Err(());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(());
    }
    Ok(bytes)
}

fn plutil_json(bytes: &[u8], timeout: Duration) -> Result<Value, ()> {
    if timeout.is_zero() {
        return Err(());
    }
    let mut command = Command::new("/usr/bin/plutil");
    command.args(["-convert", "json", "-o", "-", "--", "-"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.process_group(0);
    let mut child = command.spawn().map_err(|_| ())?;
    let mut child_stdin = child.stdin.take();
    let input = bytes.to_vec();
    let writer = thread::spawn(move || {
        let Some(mut stdin) = child_stdin.take() else {
            return false;
        };
        stdin.write_all(&input).is_ok()
    });
    let output_result = wait_bounded(&mut child, timeout.min(COMMAND_TIMEOUT));
    let writer_result = writer.join().map_err(|_| ())?;
    if !writer_result {
        return Err(());
    }
    let output = output_result?;
    if !output.status.success() {
        return Err(());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| ())
}

fn run_manager(
    program: &Path,
    args: &[&str],
    cwd: Option<&Path>,
    bounded_output: bool,
    timeout: Duration,
) -> Option<Output> {
    if timeout.is_zero() {
        return None;
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.current_dir(cwd.unwrap_or_else(|| Path::new("/")));
    if program.file_name().and_then(|name| name.to_str()) == Some("brew") {
        command.envs([
            ("HOMEBREW_NO_AUTO_UPDATE", "1"),
            ("HOMEBREW_NO_ANALYTICS", "1"),
            ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
            ("HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK", "1"),
            ("HOMEBREW_NO_AUTOREMOVE", "1"),
            ("NONINTERACTIVE", "1"),
        ]);
    }
    let output = run_command(command, REGISTRY_TIMEOUT.min(timeout)).ok()?;
    if bounded_output
        && (output.stdout.len() as u64 > MAX_JSON_BYTES
            || output.stderr.len() as u64 > MAX_JSON_BYTES)
    {
        None
    } else {
        Some(output)
    }
}

fn run_command(mut command: Command, timeout: Duration) -> Result<Output, ()> {
    command.process_group(0);
    let mut child = command.spawn().map_err(|_| ())?;
    wait_bounded(&mut child, timeout)
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> Result<Output, ()> {
    let process_group = child.id() as i32;
    let mut stdout = child.stdout.take();
    if let Some(pipe) = stdout.as_ref() {
        let descriptor = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            terminate_owned_group(process_group, child);
            return Err(());
        }
    }
    let started = Instant::now();
    let mut status = None;
    let mut output = Vec::new();
    let mut stdout_eof;
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exited)) => status = Some(exited),
                Ok(None) => {}
                Err(_) => {
                    terminate_owned_group(process_group, child);
                    return Err(());
                }
            }
        }
        match drain_stdout(&mut stdout, &mut output) {
            Ok(eof) => stdout_eof = eof,
            Err(()) => {
                terminate_owned_group(process_group, child);
                return Err(());
            }
        }
        if output.len() as u64 > MAX_JSON_BYTES {
            terminate_owned_group(process_group, child);
            return Err(());
        }
        if status.is_some() && stdout_eof {
            break;
        }
        if started.elapsed() >= timeout {
            terminate_owned_group(process_group, child);
            return Err(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    if process_group_exists(process_group) {
        terminate_owned_group(process_group, child);
        return Err(());
    }
    let status = status.ok_or(())?;
    Ok(Output {
        status,
        stdout: output,
        stderr: Vec::new(),
    })
}

fn drain_stdout(
    stdout: &mut Option<std::process::ChildStdout>,
    output: &mut Vec<u8>,
) -> Result<bool, ()> {
    let Some(pipe) = stdout.as_ref() else {
        return Ok(true);
    };
    let mut buffer = [0u8; 8192];
    loop {
        let count =
            unsafe { libc::read(pipe.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if count > 0 {
            output.extend_from_slice(&buffer[..count as usize]);
            if output.len() as u64 > MAX_JSON_BYTES {
                return Ok(false);
            }
            continue;
        }
        if count == 0 {
            *stdout = None;
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error
            .raw_os_error()
            .is_some_and(|code| code == libc::EAGAIN || code == libc::EWOULDBLOCK)
        {
            return Ok(false);
        }
        return Err(());
    }
}

fn process_group_exists(process_group: i32) -> bool {
    if unsafe { libc::kill(-process_group, 0) } == 0 {
        true
    } else {
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

fn terminate_owned_group(process_group: i32, child: &mut Child) {
    if process_group_exists(process_group) {
        unsafe {
            libc::kill(-process_group, libc::SIGTERM);
        }
        let grace_deadline = Instant::now() + TERMINATION_GRACE;
        while process_group_exists(process_group) && Instant::now() < grace_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if process_group_exists(process_group) {
            unsafe {
                libc::kill(-process_group, libc::SIGKILL);
            }
            let cleanup_deadline = Instant::now() + TERMINATION_GRACE;
            while process_group_exists(process_group) && Instant::now() < cleanup_deadline {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
    let _ = child.wait();
}

fn bounded_utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes)
        .ok()
        .filter(|text| text.len() <= 4096)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::SystemTime;

    fn temporary_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tidegauge-packages-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn fixture_formula(root: &Path) -> Package {
        let manager = root.join("bin/brew");
        let prefix = root.join("prefix");
        let target = prefix.join("Cellar/tool/1");
        fs::create_dir_all(manager.parent().unwrap()).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&prefix).unwrap();
        fs::write(&manager, b"manager").unwrap();
        fs::set_permissions(&manager, fs::Permissions::from_mode(0o755)).unwrap();
        let path = fs::canonicalize(target).unwrap();
        let manager = fs::canonicalize(manager).unwrap();
        let prefix = fs::canonicalize(prefix).unwrap();
        Package {
            name: "tool".to_owned(),
            version: "1".to_owned(),
            source: Source::BrewFormula,
            path: path.clone(),
            latest: Some("2".to_owned()),
            can_update: true,
            can_remove: true,
            reason: String::new(),
            identity: capture_identity(
                "tool",
                "1",
                Source::BrewFormula,
                &path,
                Some(manager),
                Some(prefix),
                None,
                &[],
            ),
        }
    }

    fn complete_snapshot(package: Package) -> ScanSnapshot {
        let key = package.identity.source_key().unwrap();
        ScanSnapshot {
            inventory: Inventory {
                items: vec![package],
                warnings: Vec::new(),
            },
            complete_sources: HashSet::from([key]),
        }
    }

    fn fixture_package(source: Source, name: &str, path: &Path) -> Package {
        let version = "1".to_owned();
        let manager = match source {
            Source::BrewFormula | Source::BrewCask => Some(PathBuf::from("/opt/homebrew/bin/brew")),
            Source::Npm => Some(PathBuf::from("/opt/homebrew/bin/npm")),
            Source::App => None,
        };
        let prefix = (source != Source::App).then(|| PathBuf::from("/opt/homebrew"));
        let runtime = (source == Source::Npm).then(|| PathBuf::from("/opt/homebrew/bin/node"));
        Package {
            name: name.to_owned(),
            version: version.clone(),
            source,
            path: path.to_path_buf(),
            latest: None,
            can_update: true,
            can_remove: true,
            reason: String::new(),
            identity: capture_identity(name, &version, source, path, manager, prefix, runtime, &[]),
        }
    }

    #[test]
    fn source_labels_are_localized() {
        assert!(!Source::App.label().is_empty());
        assert!(!Source::BrewFormula.label().is_empty());
    }

    #[test]
    fn manager_names_reject_options_paths_and_injection() {
        let too_long = "a".repeat(215);
        for name in ["", "--all", "../package", "name with space", "name;rm"] {
            assert!(!valid_manager_name(name));
        }
        assert!(valid_npm_name("@scope/package"));
        assert!(!valid_npm_name("/absolute"));
        for name in [
            "",
            ".",
            "..",
            "--all",
            "@Scope/package",
            "@scope/",
            "@scope/package/extra",
            "name with space",
            "https://example.invalid/name",
            "file:../name",
            "git+https://example.invalid/name",
            too_long.as_str(),
        ] {
            assert!(!valid_npm_name(name));
        }
        for name in ["a", "1package", "a.b", "a_b", "a~b", "@scope/package"] {
            assert!(valid_npm_name(name));
        }
    }

    #[test]
    fn homebrew_and_npm_real_schema_fixtures_use_installed_identity() {
        let formula: Value = serde_json::from_str(
            r#"{"name":"sample","tap":"homebrew/core","installed":[{"version":"1.2.3"}]}"#,
        )
        .unwrap();
        assert_eq!(brew_version(&formula).as_deref(), Some("1.2.3"));
        let cask = serde_json::json!({"token":"sample", "installed":"1.2.3", "version":"2.0"});
        assert_eq!(brew_version(&cask).as_deref(), Some("1.2.3"));
        assert!(brew_version(&serde_json::json!({"version":"2.0"})).is_none());

        let multiple: Value = serde_json::from_str(
            r#"{"name":"sample","tap":"homebrew/core","installed":[{"version":"1.2.3"},{"version":"1.1.0"}]}"#,
        )
        .unwrap();
        assert!(brew_version(&multiple).is_none());
        assert_eq!(multiple["installed"].as_array().unwrap().len(), 2);

        let npm: Value = serde_json::from_str(
            r#"{"dependencies":{"sample":{"version":"1.2.3","resolved":"https://registry.npmjs.org/sample/-/sample-1.2.3.tgz","path":"/opt/homebrew/lib/node_modules/sample","link":false}}}"#,
        )
        .unwrap();
        let item = &npm["dependencies"]["sample"];
        assert_eq!(item["version"].as_str(), Some("1.2.3"));
        assert_eq!(
            item["path"].as_str(),
            Some("/opt/homebrew/lib/node_modules/sample")
        );
        assert!(npm_resolved_is_official(item, "sample"));
        assert!(!npm_resolved_is_official(
            &serde_json::json!({"resolved":"https://registry.example.invalid/sample/-/sample.tgz"}),
            "sample"
        ));
    }

    #[test]
    fn npm_linked_custom_invalid_or_missing_paths_are_read_only() {
        let root = temporary_root("npm-path");
        let prefix = root.join("prefix");
        let package = prefix.join("lib/node_modules/sample");
        fs::create_dir_all(&package).unwrap();
        let canonical_package = fs::canonicalize(&package).unwrap();

        assert!(npm_package_path_verified(
            true,
            true,
            true,
            false,
            true,
            Some(&canonical_package),
            Some(&canonical_package),
        ));
        assert!(!npm_package_path_verified(
            true,
            true,
            true,
            true,
            true,
            Some(&canonical_package),
            Some(&canonical_package),
        ));
        assert!(!npm_package_path_verified(
            true,
            true,
            true,
            false,
            false,
            Some(&canonical_package),
            Some(&canonical_package),
        ));
        assert!(!npm_package_path_verified(
            true,
            true,
            false,
            false,
            true,
            Some(&canonical_package),
            Some(&canonical_package),
        ));
        assert!(!npm_package_path_verified(
            true,
            true,
            true,
            false,
            true,
            None,
            Some(&canonical_package),
        ));
        assert!(!npm_package_path_verified(
            false,
            true,
            true,
            false,
            true,
            Some(&canonical_package),
            Some(&canonical_package),
        ));

        let alias = root.join("linked-package");
        std::os::unix::fs::symlink(&canonical_package, &alias).unwrap();
        assert!(!npm_package_path_verified(
            true,
            true,
            true,
            false,
            true,
            Some(&alias),
            Some(&canonical_package),
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_cask_metadata_resolves_default_and_renamed_targets() {
        let root = std::env::current_dir()
            .unwrap()
            .join(format!("target/tidegauge-cask-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let appdir = root.join("Applications");
        let target = appdir.join("Sample.app");
        fs::create_dir_all(target.join("Contents")).unwrap();
        fs::write(target.join("Contents/Info.plist"), b"metadata").unwrap();
        let cask = root.join("Caskroom/sample");
        fs::create_dir_all(cask.join(".metadata")).unwrap();
        fs::write(
            cask.join(".metadata/config.json"),
            serde_json::to_vec(&serde_json::json!({
                "explicit": {},
                "env": {},
                "default": {"appdir": appdir}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            cask.join(".metadata/INSTALL_RECEIPT.json"),
            serde_json::to_vec(&serde_json::json!({
                "source": {"tap": "homebrew/cask", "version": "1"},
                "uninstall_flight_blocks": false,
                "uninstall_artifacts": [{"app": ["Sample.app"]}, {"zap": []}]
            }))
            .unwrap(),
        )
        .unwrap();
        let item = serde_json::json!({
            "artifacts": [{
                "app": ["Sample.app"]
            }, {"zap": [{"trash": ["~/Library/Caches/Sample"]}]}]
        });
        fs::write(
            cask.join(".metadata/config.json"),
            br#"{"explicit":{"appdir":7},"env":{"appdir":"/Applications"},"default":{"appdir":"/Applications"}}"#,
        )
        .unwrap();
        assert!(cask_metadata(&item, &root, "sample", "1").is_none());
        fs::write(
            cask.join(".metadata/config.json"),
            serde_json::to_vec(&serde_json::json!({
                "explicit": {},
                "env": {},
                "default": {"appdir": appdir}
            }))
            .unwrap(),
        )
        .unwrap();
        let metadata = cask_metadata(&item, &root, "sample", "1").unwrap();
        let reported_item = serde_json::json!({
            "artifacts": [{"app": ["Sample.app"], "target": target}]
        });
        assert_eq!(
            cask_metadata(&reported_item, &root, "sample", "1")
                .unwrap()
                .targets,
            metadata.targets
        );
        assert_eq!(
            package_action_eligibility(true, false, true, true, Some(false)),
            (true, true)
        );
        assert_eq!(
            package_action_eligibility(true, false, true, true, None),
            (false, false)
        );
        assert_eq!(metadata.targets, vec![fs::canonicalize(&target).unwrap()]);
        let identity = capture_identity_with_related_fingerprints(
            "sample",
            "1",
            Source::BrewCask,
            &target,
            None,
            None,
            None,
            &metadata.related_paths,
            Some(&metadata.related_fingerprints),
        );
        fs::write(
            cask.join(".metadata/INSTALL_RECEIPT.json"),
            br#"{"source":{"tap":"homebrew/cask","version":"1"},"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["Sample.app"]}]}"#,
        )
        .unwrap();
        assert!(!fingerprints_are_current(&identity));
        fs::write(
            cask.join(".metadata/INSTALL_RECEIPT.json"),
            serde_json::to_vec(&serde_json::json!({
                "source": {"tap": "homebrew/cask", "version": "1"},
                "uninstall_flight_blocks": false,
                "uninstall_artifacts": [{"app": ["Sample.app"]}, {"zap": []}]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(cask_metadata(
            &serde_json::json!({"artifacts": [{"pkg": "Sample.pkg"}]}),
            &root,
            "sample",
            "1"
        )
        .is_none());
        assert!(cask_metadata(
            &serde_json::json!({"artifacts": [{"app": ["../Sample.app"]}]}),
            &root,
            "sample",
            "1"
        )
        .is_none());
        assert!(cask_metadata(
            &serde_json::json!({"artifacts": [
                {"app": ["Sample.app"]},
                {"app": ["Sample.app"]}
            ]}),
            &root,
            "sample",
            "1"
        )
        .is_none());
        assert!(cask_metadata(
            &serde_json::json!({"artifacts": [{
                "app": ["Sample.app", {"target": "/tmp/../Applications/Sample.app"}]
            }]}),
            &root,
            "sample",
            "1"
        )
        .is_none());

        let renamed = appdir.join("Nested/Renamed.app");
        fs::create_dir_all(renamed.join("Contents")).unwrap();
        fs::write(renamed.join("Contents/Info.plist"), b"metadata").unwrap();
        assert!(resolve_cask_targets(
            &serde_json::json!([{"app": ["Sample.app"], "target": renamed}]),
            &appdir
        )
        .is_none());
        let renamed_item = serde_json::json!({
            "artifacts": [{"app": ["Sample.app", {"target": "Nested/Renamed.app"}]}]
        });
        let receipt = serde_json::json!({
            "source": {"tap": "homebrew/cask", "version": "1"},
            "uninstall_flight_blocks": false,
            "uninstall_artifacts": [{"app": ["Sample.app", {"target": "Nested/Renamed.app"}]}]
        });
        let mismatched_receipt = serde_json::json!({
            "source": {"tap": "homebrew/cask", "version": "2"},
            "uninstall_flight_blocks": false,
            "uninstall_artifacts": [{"app": ["Sample.app"]}]
        });
        fs::write(
            cask.join(".metadata/INSTALL_RECEIPT.json"),
            serde_json::to_vec(&mismatched_receipt).unwrap(),
        )
        .unwrap();
        assert!(cask_metadata(&renamed_item, &root, "sample", "1").is_none());
        fs::write(
            cask.join(".metadata/INSTALL_RECEIPT.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        let renamed_metadata = cask_metadata(&renamed_item, &root, "sample", "1").unwrap();
        assert_eq!(
            renamed_metadata.targets,
            vec![fs::canonicalize(renamed).unwrap()]
        );
        assert!(cask_metadata(
            &serde_json::json!({"artifacts": [{"app": ["Sample.app"], "target": "Sample.app"}]}),
            &root,
            "sample",
            "1"
        )
        .is_none());
        fs::remove_file(cask.join(".metadata/INSTALL_RECEIPT.json")).unwrap();
        assert!(cask_metadata(&item, &root, "sample", "1").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_writable_manager_executable_modes_are_rejected() {
        assert!(executable_mode_is_trusted(0o755));
        assert!(!executable_mode_is_trusted(0o775));
        assert!(!executable_mode_is_trusted(0o757));
        assert!(!executable_mode_is_trusted(0o644));
        assert!(directory_mode_is_trusted(0o775, 80, Some(80)));
        assert!(!directory_mode_is_trusted(0o775, 20, Some(80)));
        assert!(!directory_mode_is_trusted(0o777, 80, Some(80)));
        assert!(!directory_mode_is_trusted(0o775, 80, None));
    }

    #[test]
    fn brew_prefix_accepts_usr_local_homebrew_layout() {
        let roots = [PathBuf::from("/opt/homebrew"), PathBuf::from("/usr/local")];
        assert!(brew_prefix_matches_roots(
            Path::new("/usr/local"),
            Path::new("/usr/local/Homebrew/bin/brew"),
            &roots
        ));
        assert!(!brew_prefix_matches_roots(
            Path::new("/usr/local/Homebrew"),
            Path::new("/usr/local/Homebrew/bin/brew"),
            &roots
        ));
    }

    #[test]
    fn official_taps_are_the_only_writable_brew_sources() {
        assert!(official_tap("homebrew/core", Source::BrewFormula));
        assert!(official_tap("homebrew/cask", Source::BrewCask));
        assert!(!official_tap("user/tap", Source::BrewFormula));
        assert!(!official_tap("homebrew/core", Source::BrewCask));
        assert!(!official_tap("core", Source::BrewFormula));
    }

    #[test]
    fn outdated_parsers_accept_partial_json() {
        let brew = br#"{"formulae":[{"name":"tool","current_version":"1","latest_version":"2"}],"casks":[{"token":"app","current_version":"1","latest_version":"3"}]}"#;
        let outdated = parse_brew_outdated(brew).unwrap();
        assert_eq!(outdated.get("tool"), Some(&"2".to_owned()));
        assert_eq!(outdated.get("app"), Some(&"3".to_owned()));
        let npm = br#"{"tool":{"current":"1","wanted":"1","latest":"2"}}"#;
        assert_eq!(
            parse_npm_outdated(npm).unwrap().get("tool"),
            Some(&"2".to_owned())
        );
    }

    #[test]
    fn exact_single_cask_ownership_deduplicates_app_inventory() {
        let path = PathBuf::from("/Applications/Example.app");
        let mut apps = vec![fixture_package(Source::App, "Example", &path)];
        let mut records = vec![ManagerRecord {
            package: fixture_package(
                Source::BrewCask,
                "example",
                &PathBuf::from("/opt/homebrew/Caskroom/example/1"),
            ),
            overlap_paths: vec![path],
        }];

        mark_overlapping_paths(&mut apps, &mut records);

        assert!(apps.is_empty());
        assert!(records[0].package.can_remove);
    }

    #[test]
    fn multiple_cask_owners_keep_ambiguous_app_read_only() {
        let path = PathBuf::from("/Applications/Example.app");
        let mut apps = vec![fixture_package(Source::App, "Example", &path)];
        let mut records = ["one", "two"]
            .into_iter()
            .map(|name| ManagerRecord {
                package: fixture_package(
                    Source::BrewCask,
                    name,
                    &PathBuf::from(format!("/opt/homebrew/Caskroom/{name}/1")),
                ),
                overlap_paths: vec![path.clone()],
            })
            .collect::<Vec<_>>();

        mark_overlapping_paths(&mut apps, &mut records);

        assert_eq!(apps.len(), 1);
        assert!(!apps[0].can_update);
        assert!(records
            .iter()
            .all(|record| !record.package.can_update && !record.package.can_remove));
    }

    #[test]
    fn action_plans_keep_package_name_as_one_argument() {
        let package = Package {
            name: "tool".into(),
            version: "1".into(),
            source: Source::Npm,
            path: PathBuf::from("/opt/homebrew/lib/node_modules/tool"),
            latest: Some("2".into()),
            can_update: true,
            can_remove: true,
            reason: "test".into(),
            identity: capture_identity(
                "tool",
                "1",
                Source::Npm,
                Path::new("/opt/homebrew/lib/node_modules/tool"),
                Some(PathBuf::from("/opt/homebrew/bin/npm")),
                Some(PathBuf::from("/opt/homebrew")),
                Some(PathBuf::from("/opt/homebrew/bin/node")),
                &[],
            ),
        };
        let plan = action_plan(&package, Action::Update).unwrap();
        assert_eq!(plan.program, Path::new("/opt/homebrew/bin/node"));
        assert_eq!(
            plan.args,
            [
                "/opt/homebrew/bin/npm",
                "update",
                "--global",
                "--prefix",
                "/opt/homebrew",
                "--",
                "tool",
            ]
            .map(str::to_owned)
        );
    }

    #[test]
    fn brew_action_plans_select_one_source_and_package() {
        let package = fixture_package(
            Source::BrewFormula,
            "tool",
            Path::new("/opt/homebrew/Cellar/tool/1"),
        );
        let plan = action_plan(&package, Action::Remove).unwrap();

        assert_eq!(
            plan.args,
            ["uninstall", "--formula", "tool"].map(str::to_owned)
        );
    }

    #[test]
    fn stale_identity_is_not_equal_after_version_drift() {
        let first = capture_identity(
            "tool",
            "1",
            Source::BrewFormula,
            Path::new("/opt/homebrew/Cellar/tool"),
            Some(PathBuf::from("/opt/homebrew/bin/brew")),
            Some(PathBuf::from("/opt/homebrew")),
            None,
            &[],
        );
        let mut second = first.clone();
        second.version = "2".into();
        assert_ne!(first, second);

        second = first.clone();
        second.manager = Some(PathBuf::from("/usr/local/bin/brew"));
        assert_ne!(first, second);
    }

    #[test]
    fn preflight_requires_complete_source_and_current_manager_and_target_fingerprints() {
        let root = temporary_root("preflight");
        let package = fixture_formula(&root);
        let complete = complete_snapshot(package.clone());
        assert!(preflight_action(&package, &complete, Action::Remove).is_ok());

        let incomplete = ScanSnapshot {
            inventory: complete.inventory.clone(),
            complete_sources: HashSet::new(),
        };
        assert!(preflight_action(&package, &incomplete, Action::Remove).is_err());

        let manager = package.identity.manager.as_ref().unwrap();
        let replacement = manager.with_extension("old");
        fs::rename(manager, &replacement).unwrap();
        fs::write(manager, b"replaced").unwrap();
        fs::set_permissions(manager, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(preflight_action(&package, &complete, Action::Remove).is_err());
        fs::remove_file(manager).unwrap();
        fs::rename(replacement, manager).unwrap();

        let target = package.identity.path.clone();
        let old_target = target.with_extension("old");
        fs::rename(&target, &old_target).unwrap();
        fs::create_dir_all(&target).unwrap();
        assert!(preflight_action(&package, &complete, Action::Remove).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn post_action_lookup_matches_exact_manager_prefix_and_unique_name() {
        let first_root = temporary_root("lookup-one");
        let second_root = temporary_root("lookup-two");
        let first = fixture_formula(&first_root);
        let second = fixture_formula(&second_root);
        let first_key = first.identity.source_key().unwrap();
        let second_key = second.identity.source_key().unwrap();
        let inventory = Inventory {
            items: vec![first.clone(), second],
            warnings: Vec::new(),
        };
        assert_eq!(
            unique_source_package(&inventory, &first_key, "tool")
                .unwrap()
                .unwrap()
                .path,
            first.path
        );
        assert_eq!(
            unique_source_package(&inventory, &second_key, "tool")
                .unwrap()
                .unwrap()
                .path,
            inventory.items[1].path
        );

        let duplicate_inventory = Inventory {
            items: vec![first.clone(), first.clone()],
            warnings: Vec::new(),
        };
        assert!(unique_source_package(&duplicate_inventory, &first_key, "tool").is_err());
        assert!(
            unique_source_package(&Inventory::default(), &first_key, "tool")
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(first_root).unwrap();
        fs::remove_dir_all(second_root).unwrap();
    }

    #[test]
    fn action_outcomes_require_successful_action_and_complete_source() {
        let mut package = fixture_package(
            Source::BrewFormula,
            "tool",
            Path::new("/opt/homebrew/Cellar/tool/2"),
        );
        package.version = "2".to_owned();
        assert!(verify_action_outcome(Action::Remove, "1", true, false, None).is_err());
        assert!(verify_action_outcome(Action::Update, "1", false, true, Some(&package)).is_err());
        assert!(verify_action_outcome(Action::Remove, "1", true, true, Some(&package)).is_err());
        assert_eq!(
            verify_action_outcome(Action::Remove, "1", true, true, None).unwrap(),
            localized("The package was removed.", "Pakken ble fjernet.")
        );
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_command_terminates_its_descendants_and_returns_no_success() {
        let root = temporary_root("timeout");
        let marker = root.join("descendant-finished");
        let script = format!("sleep 1; /usr/bin/touch '{}'", marker.display());
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        assert!(run_command(command, Duration::from_millis(100)).is_err());
        assert!(verify_action_outcome(Action::Update, "1", false, true, None).is_err());
        thread::sleep(Duration::from_millis(1_100));
        assert!(!marker.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "read-only native package and manager inventory probe"]
    fn native_read_only_package_inventory_probe() {
        let snapshot = scan_detailed(false);
        let counts = snapshot
            .inventory
            .items
            .iter()
            .fold([0usize; 4], |mut counts, package| {
                counts[package.source as usize] += 1;
                counts
            });
        println!(
            "apps={} formulae={} casks={} npm={} warnings={} complete_sources={}",
            counts[Source::App as usize],
            counts[Source::BrewFormula as usize],
            counts[Source::BrewCask as usize],
            counts[Source::Npm as usize],
            snapshot.inventory.warnings.len(),
            snapshot.complete_sources.len()
        );
        for package in snapshot
            .inventory
            .items
            .iter()
            .filter(|package| package.source == Source::BrewCask)
        {
            println!(
                "cask={} can_update={} can_remove={} reason={}",
                package.name, package.can_update, package.can_remove, package.reason
            );
        }
    }
}
