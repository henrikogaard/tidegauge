//! Bounded inventory for legacy macOS startup registrations.
//!
//! A plist on disk is registration evidence only. It is not evidence that
//! launchd accepted, authorized, or started the service.

use crate::locale::text as localized;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_PLIST_BYTES: u64 = 1024 * 1024;
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FILES_PER_DIRECTORY: usize = 256;
const COMMAND_TIMEOUT: Duration = Duration::from_millis(750);
const SCAN_TIMEOUT: Duration = Duration::from_secs(10);
const DIRECT_CONTROLS_SUPPORTED: bool = false;

#[derive(Clone, Debug)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub path: PathBuf,
    pub executable: Option<String>,
    pub source: String,
    pub disabled: Option<bool>,
    pub manageable: bool,
    pub reason: String,
    identity: IdentityMetadata,
    source_kind: SourceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    UserLaunchAgent,
    SharedLaunchAgent,
    SharedLaunchDaemon,
    SystemLaunchAgent,
    SystemLaunchDaemon,
}

impl SourceKind {
    fn is_user(self) -> bool {
        matches!(self, Self::UserLaunchAgent)
    }

    fn expected_owner(self, uid: u32) -> u32 {
        if self.is_user() {
            uid
        } else {
            0
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IdentityMetadata {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    mode: u32,
    owner: u32,
    content_sha256: [u8; 32],
}

struct RootSpec {
    path: PathBuf,
    source: String,
    kind: SourceKind,
}

#[derive(Debug)]
struct Snapshot {
    identity: IdentityMetadata,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct ParsedRegistration {
    label: String,
    executable: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rejection {
    Symlink,
    NonRegular,
    Ownership,
    UnsafeAncestor,
    Changed,
    TooLarge,
    Read,
    InvalidPlist,
    InvalidLabel,
}

/// Return the bounded startup inventory. This function performs no mutations.
pub fn scan() -> Inventory {
    let scan_started = Instant::now();
    let Some(home) = actual_home() else {
        return Inventory {
            entries: Vec::new(),
            warnings: vec![localized(
                "Could not determine the current user's home directory.",
                "Kunne ikke finne hjemmekatalogen til gjeldende bruker.",
            )
            .to_string()],
        };
    };
    let uid = unsafe { libc::getuid() };
    let roots = root_specs(&home);
    let disabled = read_disabled_services(uid).ok();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    for root in roots {
        if scan_started.elapsed() >= SCAN_TIMEOUT {
            warnings.push(
                localized(
                    "Startup scanning reached its time limit.",
                    "Oppstartsskanningen nådde tidsgrensen.",
                )
                .to_string(),
            );
            break;
        }
        let directory = match fs::read_dir(&root.path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                warnings.push(
                    localized(
                        "A startup directory could not be read.",
                        "En oppstartskatalog kunne ikke leses.",
                    )
                    .to_string(),
                );
                continue;
            }
        };
        for (index, item) in directory.enumerate() {
            if scan_started.elapsed() >= SCAN_TIMEOUT {
                warnings.push(
                    localized(
                        "Startup scanning reached its time limit.",
                        "Oppstartsskanningen nådde tidsgrensen.",
                    )
                    .to_string(),
                );
                break;
            }
            if index >= MAX_FILES_PER_DIRECTORY {
                warnings.push(localized(
                    "A startup directory contains more entries than the bounded scan allows.",
                    "En oppstartskatalog inneholder flere oppføringer enn den begrensede skanningen tillater.",
                ).to_string());
                break;
            }
            let Ok(item) = item else {
                warnings.push(
                    localized(
                        "A startup registration could not be inspected.",
                        "En oppstartsregistrering kunne ikke undersøkes.",
                    )
                    .to_string(),
                );
                continue;
            };
            let path = item.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("plist") {
                continue;
            }
            let snapshot =
                match read_bounded_snapshot(&path, root.kind.expected_owner(uid), &root.path) {
                    Ok(snapshot) => snapshot,
                    Err(rejection) => {
                        warnings.push(rejection_message(rejection).to_string());
                        continue;
                    }
                };
            let registration = match parse_plist_bytes(&snapshot.bytes) {
                Ok(registration) => registration,
                Err(rejection) => {
                    warnings.push(rejection_message(rejection).to_string());
                    continue;
                }
            };
            let safe_ancestors = ancestors_are_trusted(&path, root.kind.expected_owner(uid));
            let disabled_state = root
                .kind
                .is_user()
                .then(|| {
                    disabled
                        .as_ref()
                        .and_then(|states| states.get(&registration.label).copied())
                })
                .flatten();
            entries.push(Entry {
                label: registration.label,
                path,
                executable: registration.executable,
                source: root.source.clone(),
                disabled: disabled_state,
                manageable: false,
                reason: if root.kind.is_user() && safe_ancestors {
                    direct_control_limitation()
                } else if !safe_ancestors {
                    localized(
                        "This registration has an untrusted or writable ancestor and cannot be changed.",
                        "Denne registreringen har en upålitelig eller skrivbar overordnet katalog og kan ikke endres.",
                    ).to_string()
                } else {
                    localized(
                        "Only verified user LaunchAgents are eligible for direct controls.",
                        "Bare verifiserte LaunchAgents for brukeren kan ha direkte kontroller.",
                    ).to_string()
                },
                identity: snapshot.identity,
                source_kind: root.kind,
            });
        }
    }
    mark_duplicate_labels(&mut entries, &mut warnings);
    if disabled.is_none() && entries.iter().any(|entry| entry.source_kind.is_user()) {
        warnings.push(localized(
            "The disabled state could not be read safely; direct controls remain unavailable.",
            "Status for deaktivering kunne ikke leses trygt; direkte kontroller er fortsatt utilgjengelige.",
        ).to_string());
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    warnings.sort();
    warnings.dedup();
    Inventory { entries, warnings }
}

/// Change a verified legacy user LaunchAgent.
///
/// Current macOS documented interfaces do not expose whether a registration
/// is managed or protected. The backend therefore fails closed until that
/// eligibility can be established without guessing.
pub fn set_disabled(entry: &Entry, _disabled: bool) -> Result<(), String> {
    let Some(home) = actual_home() else {
        return Err(localized(
            "Startup controls are unavailable because the current user's home directory could not be determined. Open System Settings > General > Login Items.",
            "Oppstartskontroller er utilgjengelige fordi hjemmekatalogen til gjeldende bruker ikke kunne finnes. Åpne Systeminnstillinger > Generelt > Påloggingsobjekter.",
        ).to_string());
    };
    let uid = unsafe { libc::getuid() };
    let expected_root = home.join("Library").join("LaunchAgents");
    let snapshot = read_bounded_snapshot(&entry.path, uid, &expected_root)
        .map_err(|rejection| rejection_message(rejection).to_string())?;
    let registration = parse_plist_bytes(&snapshot.bytes)
        .map_err(|rejection| rejection_message(rejection).to_string())?;
    if entry.source_kind != SourceKind::UserLaunchAgent
        || entry.path.parent() != Some(expected_root.as_path())
        || registration.label != entry.label
        || snapshot.identity != entry.identity
    {
        return Err(localized(
            "This startup registration changed and cannot be safely revalidated. Open System Settings > General > Login Items.",
            "Denne oppstartsregistreringen er endret og kan ikke valideres trygt på nytt. Åpne Systeminnstillinger > Generelt > Påloggingsobjekter.",
        ).to_string());
    }
    if !entry.manageable || !DIRECT_CONTROLS_SUPPORTED {
        return Err(direct_control_limitation());
    }
    Err(direct_control_limitation())
}

/// Open the macOS Login Items settings handoff.
pub fn open_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyClass;
        let Some(class) = AnyClass::get(c"SMAppService") else {
            return Err(localized(
                "System Settings could not be opened.",
                "Systeminnstillinger kunne ikke åpnes.",
            )
            .to_string());
        };
        let selector = objc2::sel!(openSystemSettingsLoginItems);
        if !class.responds_to(selector) {
            return Err(localized(
                "This macOS version does not provide the Login Items settings handoff.",
                "Denne macOS-versjonen tilbyr ikke overføringen til Påloggingsobjekter.",
            )
            .to_string());
        }
        unsafe {
            let _: () = objc2::msg_send![class, openSystemSettingsLoginItems];
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(localized(
            "System Settings are unavailable on this platform.",
            "Systeminnstillinger er utilgjengelige på denne plattformen.",
        )
        .to_string())
    }
}

fn root_specs(home: &Path) -> Vec<RootSpec> {
    vec![
        RootSpec {
            path: home.join("Library/LaunchAgents"),
            source: localized("User LaunchAgent", "LaunchAgent for bruker").to_string(),
            kind: SourceKind::UserLaunchAgent,
        },
        RootSpec {
            path: PathBuf::from("/Library/LaunchAgents"),
            source: localized("Shared LaunchAgent", "Delt LaunchAgent").to_string(),
            kind: SourceKind::SharedLaunchAgent,
        },
        RootSpec {
            path: PathBuf::from("/Library/LaunchDaemons"),
            source: localized("Shared LaunchDaemon", "Delt LaunchDaemon").to_string(),
            kind: SourceKind::SharedLaunchDaemon,
        },
        RootSpec {
            path: PathBuf::from("/System/Library/LaunchAgents"),
            source: localized("System LaunchAgent", "System-LaunchAgent").to_string(),
            kind: SourceKind::SystemLaunchAgent,
        },
        RootSpec {
            path: PathBuf::from("/System/Library/LaunchDaemons"),
            source: localized("System LaunchDaemon", "System-LaunchDaemon").to_string(),
            kind: SourceKind::SystemLaunchDaemon,
        },
    ]
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
    if home.is_empty() || !home.starts_with('/') {
        None
    } else {
        Some(PathBuf::from(home))
    }
}

fn read_bounded_snapshot(
    path: &Path,
    expected_owner: u32,
    expected_root: &Path,
) -> Result<Snapshot, Rejection> {
    if path.parent() != Some(expected_root) || !path.starts_with(expected_root) {
        return Err(Rejection::UnsafeAncestor);
    }
    let before = fs::symlink_metadata(path).map_err(|_| Rejection::Read)?;
    validate_file_metadata(&before, expected_owner)?;
    if !ancestors_are_trusted(path, expected_owner) {
        return Err(Rejection::UnsafeAncestor);
    }
    let mut file = OpenOptions::new();
    file.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = file.open(path).map_err(|_| Rejection::Read)?;
    let opened = file.metadata().map_err(|_| Rejection::Read)?;
    if !same_file_identity(&before, &opened) {
        return Err(Rejection::Changed);
    }
    if opened.len() > MAX_PLIST_BYTES {
        return Err(Rejection::TooLarge);
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    (&mut file)
        .take(MAX_PLIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Rejection::Read)?;
    if bytes.len() as u64 > MAX_PLIST_BYTES {
        return Err(Rejection::TooLarge);
    }
    let after = file.metadata().map_err(|_| Rejection::Read)?;
    if !same_file_identity(&opened, &after) || after.len() != bytes.len() as u64 {
        return Err(Rejection::Changed);
    }
    Ok(Snapshot {
        identity: identity_metadata(&after, &bytes),
        bytes,
    })
}

fn validate_file_metadata(
    metadata: &std::fs::Metadata,
    expected_owner: u32,
) -> Result<(), Rejection> {
    if metadata.file_type().is_symlink() {
        return Err(Rejection::Symlink);
    }
    if !metadata.file_type().is_file() {
        return Err(Rejection::NonRegular);
    }
    if metadata.uid() != expected_owner {
        return Err(Rejection::Ownership);
    }
    Ok(())
}

fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.uid() == right.uid()
        && left.mode() == right.mode()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
}

fn identity_metadata(metadata: &std::fs::Metadata, bytes: &[u8]) -> IdentityMetadata {
    IdentityMetadata {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        mode: metadata.mode(),
        owner: metadata.uid(),
        content_sha256: content_sha256(bytes),
    }
}

fn ancestors_are_trusted(path: &Path, expected_owner: u32) -> bool {
    let mut current = path.parent();
    while let Some(directory) = current {
        let Ok(metadata) = fs::symlink_metadata(directory) else {
            return false;
        };
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_dir()
            || metadata.mode() & 0o022 != 0
            || (metadata.uid() != 0 && metadata.uid() != expected_owner)
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

fn parse_plist_bytes(bytes: &[u8]) -> Result<ParsedRegistration, Rejection> {
    let output = run_plutil(bytes).map_err(|_| Rejection::InvalidPlist)?;
    let value: Value = serde_json::from_slice(&output).map_err(|_| Rejection::InvalidPlist)?;
    parse_registration_value(&value)
}

fn parse_registration_value(value: &Value) -> Result<ParsedRegistration, Rejection> {
    let dictionary = value.as_object().ok_or(Rejection::InvalidPlist)?;
    let label = dictionary
        .get("Label")
        .and_then(Value::as_str)
        .ok_or(Rejection::InvalidLabel)?;
    if !valid_label(label) {
        return Err(Rejection::InvalidLabel);
    }
    let executable = dictionary
        .get("Program")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            dictionary
                .get("ProgramArguments")
                .and_then(Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        });
    Ok(ParsedRegistration {
        label: label.to_owned(),
        executable,
    })
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 255
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn mark_duplicate_labels(entries: &mut [Entry], warnings: &mut Vec<String>) {
    let mut counts = HashMap::<String, usize>::new();
    for entry in entries.iter() {
        *counts.entry(entry.label.clone()).or_default() += 1;
    }
    let mut warned = false;
    for entry in entries {
        if counts.get(&entry.label).copied().unwrap_or_default() > 1 {
            entry.manageable = false;
            entry.reason = localized(
                "This label is shared by multiple registration paths and cannot be safely controlled. Open System Settings > General > Login Items.",
                "Denne etiketten deles av flere registreringsstier og kan ikke kontrolleres trygt. Åpne Systeminnstillinger > Generelt > Påloggingsobjekter.",
            )
            .to_string();
            warned = true;
        }
    }
    if warned {
        warnings.push(
            localized(
                "Some startup labels are shared by multiple registration paths.",
                "Noen oppstartsetiketter deles av flere registreringsstier.",
            )
            .to_string(),
        );
    }
}

fn read_disabled_services(uid: u32) -> Result<BTreeMap<String, bool>, ()> {
    #[cfg(target_os = "macos")]
    {
        let domain = format!("gui/{uid}");
        let mut command = Command::new("/bin/launchctl");
        command.args(["print-disabled", &domain]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let output = run_command_bounded(command).map_err(|_| ())?;
        if !output.status.success() {
            return Err(());
        }
        parse_disabled_output(&output.stdout).map_err(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = uid;
        Err(())
    }
}

fn parse_disabled_output(bytes: &[u8]) -> Result<BTreeMap<String, bool>, &'static str> {
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err("output too large");
    }
    let output = std::str::from_utf8(bytes).map_err(|_| "invalid output")?;
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.first() != Some(&"disabled services = {") {
        return Err("missing header");
    }
    if lines.last() != Some(&"}") {
        return Err("missing closure");
    }
    let mut states = BTreeMap::new();
    for line in &lines[1..lines.len() - 1] {
        if !line.starts_with('"') {
            return Err("unexpected line");
        }
        let Some((label, state)) = line[1..].split_once("\" => ") else {
            return Err("invalid entry");
        };
        let state = state.trim();
        let disabled = match state {
            "disabled" | "true" => true,
            "enabled" | "false" => false,
            _ => return Err("invalid state"),
        };
        if !valid_label(label) {
            return Err("invalid label");
        }
        if states.insert(label.to_owned(), disabled).is_some() {
            return Err("duplicate label");
        }
    }
    Ok(states)
}

fn run_plutil(bytes: &[u8]) -> Result<Vec<u8>, ()> {
    let mut command = Command::new("/usr/bin/plutil");
    command.args(["-convert", "json", "-o", "-", "--", "-"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| ())?;
    let mut stdin = child.stdin.take();
    let input = bytes.to_vec();
    let writer = thread::spawn(move || {
        if let Some(mut stdin) = stdin.take() {
            stdin.write_all(&input).is_ok()
        } else {
            false
        }
    });
    let output = wait_bounded(&mut child).map_err(|_| ())?;
    let _ = writer.join();
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(())
    }
}

fn run_command_bounded(mut command: Command) -> Result<Output, ()> {
    let mut child = command.spawn().map_err(|_| ())?;
    wait_bounded(&mut child)
}

fn wait_bounded(child: &mut Child) -> Result<Output, ()> {
    let stdout_reader = child.stdout.take().map(|mut stdout| {
        thread::spawn(move || {
            let mut output = Vec::new();
            let _ = (&mut stdout)
                .take(MAX_JSON_BYTES + 1)
                .read_to_end(&mut output);
            output
        })
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| ())? {
            break status;
        }
        if started.elapsed() >= COMMAND_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = if let Some(reader) = stdout_reader {
        while !reader.is_finished() {
            if started.elapsed() >= COMMAND_TIMEOUT {
                return Err(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        reader.join().map_err(|_| ())?
    } else {
        Vec::new()
    };
    if stdout.len() as u64 > MAX_JSON_BYTES {
        return Err(());
    }
    Ok(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

fn rejection_message(rejection: Rejection) -> &'static str {
    match rejection {
        Rejection::Symlink => localized(
            "A startup registration is a symlink and was ignored.",
            "En oppstartsregistrering er en symbolsk lenke og ble ignorert.",
        ),
        Rejection::NonRegular => localized(
            "A startup registration is not a regular file and was ignored.",
            "En oppstartsregistrering er ikke en vanlig fil og ble ignorert.",
        ),
        Rejection::Ownership => localized(
            "A startup registration has unexpected ownership and was ignored.",
            "En oppstartsregistrering har uventet eierskap og ble ignorert.",
        ),
        Rejection::UnsafeAncestor => localized(
            "A startup registration has an untrusted or writable ancestor and was ignored.",
            "En oppstartsregistrering har en upålitelig eller skrivbar overordnet katalog og ble ignorert.",
        ),
        Rejection::Changed => localized(
            "A startup registration changed while it was being inspected.",
            "En oppstartsregistrering ble endret mens den ble undersøkt.",
        ),
        Rejection::TooLarge => localized(
            "A startup registration exceeds the bounded size limit and was ignored.",
            "En oppstartsregistrering overskrider den begrensede størrelsesgrensen og ble ignorert.",
        ),
        Rejection::Read => localized(
            "A startup registration could not be read safely.",
            "En oppstartsregistrering kunne ikke leses trygt.",
        ),
        Rejection::InvalidPlist => localized(
            "A startup registration is not a valid property list and was ignored.",
            "En oppstartsregistrering er ikke en gyldig egenskapsliste og ble ignorert.",
        ),
        Rejection::InvalidLabel => localized(
            "A startup registration has an invalid or ambiguous label and was ignored.",
            "En oppstartsregistrering har en ugyldig eller tvetydig etikett og ble ignorert.",
        ),
    }
}

fn direct_control_limitation() -> String {
    localized(
        "Direct startup changes are unavailable: documented launchctl interfaces do not expose whether a registration is managed or protected. Archive, remove, and restore are also unavailable because a safe prior-override record cannot be proven. Open System Settings > General > Login Items.",
        "Direkte oppstartsendringer er utilgjengelige: dokumenterte launchctl-grensesnitt viser ikke om en registrering er administrert eller beskyttet. Arkivering, fjerning og gjenoppretting er også utilgjengelig fordi en trygg oversikt over tidligere overstyring ikke kan bevises. Åpne Systeminnstillinger > Generelt > Påloggingsobjekter.",
    )
    .to_string()
}

#[cfg(target_os = "macos")]
#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
fn content_sha256(bytes: &[u8]) -> [u8; 32] {
    unsafe extern "C" {
        fn CC_SHA256(data: *const std::ffi::c_void, len: u32, digest: *mut u8) -> *mut u8;
    }
    let mut digest = [0u8; 32];
    unsafe {
        CC_SHA256(
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            digest.as_mut_ptr(),
        );
    }
    digest
}

#[cfg(not(target_os = "macos"))]
fn content_sha256(bytes: &[u8]) -> [u8; 32] {
    use std::hash::{Hash, Hasher};
    let mut digest = [0u8; 32];
    for (index, chunk) in digest.chunks_exact_mut(8).enumerate() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        index.hash(&mut hasher);
        bytes.hash(&mut hasher);
        chunk.copy_from_slice(&hasher.finish().to_ne_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tidegauge-startup-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn parser_prefers_program_and_never_exposes_arguments() {
        let value = serde_json::json!({
            "Label": "com.example.worker",
            "Program": "/usr/libexec/example",
            "ProgramArguments": ["/usr/bin/example", "--secret", "value"],
            "EnvironmentVariables": {"SECRET": "hidden"}
        });
        let registration = parse_registration_value(&value).unwrap();
        assert_eq!(registration.label, "com.example.worker");
        assert_eq!(
            registration.executable.as_deref(),
            Some("/usr/libexec/example")
        );
    }

    #[test]
    fn parser_rejects_invalid_and_ambiguous_labels() {
        for label in ["", "com/example", "contains space", "ümlaut"] {
            let value = serde_json::json!({"Label": label});
            assert_eq!(
                parse_registration_value(&value).unwrap_err(),
                Rejection::InvalidLabel
            );
        }
    }

    #[test]
    fn duplicate_labels_are_rejected_for_every_path() {
        let identity = IdentityMetadata {
            device: 1,
            inode: 1,
            size: 1,
            modified_seconds: 1,
            modified_nanoseconds: 1,
            mode: 0o100600,
            owner: 1,
            content_sha256: [0; 32],
        };
        let first = Entry {
            label: "com.example.worker".into(),
            path: PathBuf::from("/one.plist"),
            executable: None,
            source: "User".into(),
            disabled: Some(false),
            manageable: true,
            reason: "temporary".into(),
            identity: identity.clone(),
            source_kind: SourceKind::UserLaunchAgent,
        };
        let second = Entry {
            path: PathBuf::from("/two.plist"),
            ..first.clone()
        };
        let mut entries = vec![first, second];
        let mut warnings = Vec::new();
        mark_duplicate_labels(&mut entries, &mut warnings);
        assert!(entries.iter().all(|entry| !entry.manageable));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn identity_changes_when_file_content_changes() {
        let path = temp_path("identity");
        fs::write(&path, b"first").unwrap();
        let first_bytes = fs::read(&path).unwrap();
        let first_metadata = fs::metadata(&path).unwrap();
        let first = identity_metadata(&first_metadata, &first_bytes);
        fs::write(&path, b"second").unwrap();
        let second_bytes = fs::read(&path).unwrap();
        let second_metadata = fs::metadata(&path).unwrap();
        let second = identity_metadata(&second_metadata, &second_bytes);
        assert_ne!(first.content_sha256, second.content_sha256);
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_rejected_before_reading() {
        use std::os::unix::fs::symlink;
        let target = temp_path("target");
        let link = temp_path("link");
        fs::write(&target, b"plist").unwrap();
        symlink(&target, &link).unwrap();
        let metadata = fs::symlink_metadata(&link).unwrap();
        assert_eq!(
            validate_file_metadata(&metadata, unsafe { libc::getuid() }).unwrap_err(),
            Rejection::Symlink
        );
        let _ = fs::remove_file(link);
        let _ = fs::remove_file(target);
    }

    #[test]
    fn ownership_mismatch_is_rejected() {
        let path = temp_path("ownership");
        fs::write(&path, b"plist").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(
            validate_file_metadata(&metadata, unsafe { libc::getuid() + 1 }).unwrap_err(),
            Rejection::Ownership
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn launchctl_disabled_output_parser_is_strict_and_bounded() {
        let output = br#"disabled services = {
    "com.example.off" => disabled
    "com.example.on" => false
    "com.example.bool" => true
    "com.example.text" => enabled
}
"#;
        let states = parse_disabled_output(output).unwrap();
        assert_eq!(states.get("com.example.off"), Some(&true));
        assert_eq!(states.get("com.example.on"), Some(&false));
        assert_eq!(states.get("com.example.bool"), Some(&true));
        assert_eq!(states.get("com.example.text"), Some(&false));
        assert!(parse_disabled_output(
            br#"disabled services = {
"bad label" => disabled
}"#
        )
        .is_err());
        assert!(parse_disabled_output(
            br#"disabled services = {
"com.example.off" => maybe
}"#
        )
        .is_err());
        assert!(parse_disabled_output(
            br#"disabled services = {
"com.example.off" => 1
}"#
        )
        .is_err());
        assert!(parse_disabled_output(
            br#"disabled services = {
"com.example.off" => TRUE
}"#
        )
        .is_err());
    }

    #[test]
    fn launchctl_disabled_output_requires_complete_braced_map() {
        assert!(parse_disabled_output(b"").is_err());
        assert!(parse_disabled_output(b"permission denied").is_err());
        assert!(parse_disabled_output(b"disabled services = {\n").is_err());
        assert!(parse_disabled_output(
            b"disabled services = {\n\t\"com.example.off\" => disabled\n"
        )
        .is_err());
        assert!(parse_disabled_output(
            b"disabled services = {\n\tgarbage\n\t\"com.example.off\" => disabled\n}\n"
        )
        .is_err());
        assert!(parse_disabled_output(b"disabled services = {\n}\ntrailing\n").is_err());
        assert!(parse_disabled_output(
            b"disabled services = {\n\t\"com.example.off\" => disabled\n\t\"com.example.off\" => disabled\n}\n"
        )
        .is_err());
        let too_large = vec![b' '; (MAX_JSON_BYTES + 1) as usize];
        assert!(parse_disabled_output(&too_large).is_err());
        let empty = parse_disabled_output(b"disabled services = {\n}\n").unwrap();
        assert!(empty.is_empty());
        let observed = br#"disabled services = {
	"com.apple.ManagedClientAgent.enrollagent" => disabled
	"com.apple.Siri.agent" => disabled
	"com.apple.FolderActionsDispatcher" => disabled
	"com.apple.appleseed.seedusaged.postinstall" => disabled
	"com.apple.ScriptMenuApp" => disabled
}
"#;
        let states = parse_disabled_output(observed).unwrap();
        assert_eq!(states.len(), 5);
        assert!(states.values().all(|disabled| *disabled));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "read-only native startup inventory probe"]
    fn native_read_only_scan_and_override_probe() {
        let inventory = scan();
        let override_count = read_disabled_services(unsafe { libc::getuid() })
            .map(|states| states.len())
            .unwrap_or(0);
        println!(
            "startup probe: entries={}, warnings={}, explicit_overrides={}",
            inventory.entries.len(),
            inventory.warnings.len(),
            override_count
        );
    }
}
