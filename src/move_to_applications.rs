//! Offer, once, to move Tidegauge into Applications when it runs from
//! anywhere else (Downloads, a disk image, a folder). Launch at login and
//! updates expect it there, and a copy left in Downloads tends to be lost.
//!
//! The move copies the bundle with `ditto` (keeping its signature), puts any
//! older copy in Applications and the original in the Trash rather than
//! deleting them, and relaunches the new copy once this one has quit.

use crate::locale::text as l;
use std::path::{Path, PathBuf};

/// Where the bundle should go: /Applications when it can be written to,
/// otherwise the user's own Applications folder.
pub fn destination_folder(system_writable: bool, home: &Path) -> PathBuf {
    if system_writable {
        PathBuf::from("/Applications")
    } else {
        home.join("Applications")
    }
}

/// Whether the prompt applies: a real Tidegauge bundle, outside both
/// Applications folders, not declined before, and not a development
/// preview (unless `TIDEGAUGE_MOVE_PROMPT=1` asks for it in a check).
pub fn should_offer(is_bundle: bool, installed: bool, declined: bool, preview: bool, forced: bool) -> bool {
    is_bundle && !installed && !declined && (!preview || forced)
}

/// A disk image or other read-only volume: the original can't be trashed,
/// so it is left where it is.
fn on_read_only_volume(path: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return true;
    };
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    unsafe { libc::statfs(c.as_ptr(), &mut fs) == 0 && fs.f_flags & libc::MNT_RDONLY as u32 != 0 }
}

/// Gatekeeper may run a downloaded app from a randomised read-only copy
/// ("App Translocation"); the original is what should move.
fn original_path(running: &Path) -> PathBuf {
    type Translocated = unsafe extern "C" fn(*const std::ffi::c_void, *mut bool, *mut *const std::ffi::c_void) -> bool;
    type Original = unsafe extern "C" fn(*const std::ffi::c_void, *mut *const std::ffi::c_void) -> *const std::ffi::c_void;
    unsafe {
        let handle = libc::dlopen(c"/System/Library/Frameworks/Security.framework/Security".as_ptr(), libc::RTLD_LAZY);
        if handle.is_null() {
            return running.to_path_buf();
        }
        let is = libc::dlsym(handle, c"SecTranslocateIsTranslocatedURL".as_ptr());
        let original = libc::dlsym(handle, c"SecTranslocateCreateOriginalPathForURL".as_ptr());
        if is.is_null() || original.is_null() {
            return running.to_path_buf();
        }
        let url = objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&running.to_string_lossy()));
        let url_ref = objc2::rc::Retained::as_ptr(&url) as *const std::ffi::c_void;
        let mut translocated = false;
        let is: Translocated = std::mem::transmute(is);
        if !is(url_ref, &mut translocated, std::ptr::null_mut()) || !translocated {
            return running.to_path_buf();
        }
        let original: Original = std::mem::transmute(original);
        let found = original(url_ref, std::ptr::null_mut());
        if found.is_null() {
            return running.to_path_buf();
        }
        let found: &objc2_foundation::NSURL = &*(found as *const objc2_foundation::NSURL);
        let path = found.path().map(|p| PathBuf::from(p.to_string())).unwrap_or_else(|| running.to_path_buf());
        crate::ffi::CFRelease(found as *const _ as *const std::ffi::c_void);
        path
    }
}

pub(crate) fn trash(path: &Path) -> Result<(), String> {
    let url = objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&path.to_string_lossy()));
    objc2_foundation::NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| e.localizedDescription().to_string())
}

/// Whether dotted version `a` is newer than `b` ("0.3.10" > "0.3.9").
/// Anything unreadable counts as not newer.
pub fn is_newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|p| p.trim().parse::<u64>().ok()).collect::<Option<Vec<_>>>();
    match (parse(a), parse(b)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

fn bundle_version(bundle: &Path) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(bundle.join("Contents/Info"))
        .arg("CFBundleShortVersionString")
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Copy the bundle in beside the old one, and only once the copy is whole
/// retire the old copies to the Trash, so a failed copy never leaves
/// Applications without a working Tidegauge.
fn move_bundle(source: &Path, folder: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let target = folder.join(source.file_name().unwrap_or_else(|| "Tidegauge.app".as_ref()));
    let staging = folder.join(String::from(".Tidegauge-moving-") + &std::process::id().to_string() + ".app");
    let _ = std::fs::remove_dir_all(&staging);
    let failed = || l("Copying into Applications failed.", "Kopieringen til Programmer mislyktes.").to_string();
    let copied = std::process::Command::new("/usr/bin/ditto")
        .arg(source)
        .arg(&staging)
        .status()
        .is_ok_and(|s| s.success());
    if !copied {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(failed());
    }
    if target.exists() {
        if let Err(error) = trash(&target) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
    }
    std::fs::rename(&staging, &target).map_err(|_| failed())?;
    if !on_read_only_volume(source) {
        // The copy is in place; an original that won't go to the Trash is
        // left alone rather than deleted.
        let _ = trash(source);
    }
    Ok(target)
}

fn relaunch(target: &Path) {
    // The new copy waits for this one to quit, so the single-instance lock
    // is free when it starts.
    const SCRIPT: &str = "while kill -0 \"$2\" 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"$1\"";
    let _ = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(SCRIPT)
        .arg("sh")
        .arg(target)
        .arg(std::process::id().to_string())
        .spawn();
}

fn alert(title: &str, text: &str, buttons: &[&str], suppression: bool) -> (isize, bool) {
    use objc2_app_kit::{NSAlert, NSApplication};
    use objc2_foundation::{MainThreadMarker, NSString};
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    alert.setShowsSuppressionButton(suppression);
    if let Some(button) = alert.suppressionButton() {
        button.setTitle(&NSString::from_str(l("Don't ask again", "Ikke spør igjen")));
    }
    let answer = alert.runModal();
    let suppressed = alert.suppressionButton().is_some_and(|b| b.state() != 0);
    (answer, suppressed)
}

/// Ask and, on yes, move and relaunch. Main thread, at startup.
pub fn offer() {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    let running = PathBuf::from(bundle.bundlePath().to_string());
    let is_bundle = running.extension().is_some_and(|e| e == "app")
        && bundle.bundleIdentifier().is_some_and(|id| id.to_string() == "no.ogard.tidegauge");
    let settings = crate::settings::Settings::load();
    let forced = std::env::var("TIDEGAUGE_MOVE_PROMPT").as_deref() == Ok("1");
    if !should_offer(is_bundle, crate::login::installed_here(&running), settings.move_prompt_declined, crate::updater::is_preview(), forced) {
        return;
    }
    const FIRST: isize = 1000;
    let (answer, dont_ask) = alert(
        l("Move Tidegauge to Applications?", "Flytte Tidegauge til Programmer?"),
        l(
            "Tidegauge works from here, but launch at login and automatic updates need it in Applications. The copy here goes to the Trash.",
            "Tidegauge virker herfra, men start ved pålogging og automatiske oppdateringer krever at den ligger i Programmer. Kopien her legges i papirkurven.",
        ),
        &[l("Move to Applications", "Flytt til Programmer"), l("Not Now", "Ikke nå")],
        true,
    );
    if answer != FIRST {
        if dont_ask {
            let _ = crate::settings::mutate(|s| s.move_prompt_declined = true);
        }
        return;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let writable = unsafe { libc::access(c"/Applications".as_ptr(), libc::W_OK) == 0 };
    let source = original_path(&running);
    let folder = destination_folder(writable, &home);
    let installed = folder.join(source.file_name().unwrap_or_else(|| "Tidegauge.app".as_ref()));
    // Never replace a newer installed copy with this one; open that instead.
    if let (Some(there), Some(here)) = (bundle_version(&installed), bundle_version(&source)) {
        if is_newer(&there, &here) {
            let (answer, _) = alert(
                l("A newer Tidegauge is already in Applications", "En nyere Tidegauge ligger allerede i Programmer"),
                &format!("{there} > {here}. {}", l("Open that one instead? This copy is left as it is.", "Åpne den i stedet? Denne kopien blir liggende som den er.")),
                &[l("Open It", "Åpne den"), l("Not Now", "Ikke nå")],
                false,
            );
            if answer == FIRST {
                relaunch(&installed);
                std::process::exit(0);
            }
            return;
        }
    }
    match move_bundle(&source, &folder) {
        Ok(target) => {
            relaunch(&target);
            std::process::exit(0);
        }
        Err(error) => {
            alert(
                l("Tidegauge could not be moved", "Tidegauge kunne ikke flyttes"),
                &format!("{error}\n\n{}", l("Drag it to Applications in Finder instead.", "Dra den til Programmer i Finder i stedet.")),
                &["OK"],
                false,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{destination_folder, is_newer, move_bundle, should_offer};

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(is_newer("0.4", "0.3.9"));
        assert!(!is_newer("0.3.5", "0.3.5"));
        assert!(!is_newer("0.3.5", "0.3.5.0"));
        assert!(!is_newer("0.3.4", "0.3.5"));
        assert!(!is_newer("beta", "0.3.5"), "unreadable is not newer");
    }

    #[test]
    fn a_failed_copy_leaves_the_installed_copy_in_place() {
        let root = std::env::temp_dir().join(String::from("tidegauge-move-") + &std::process::id().to_string());
        let folder = root.join("Applications");
        let installed = folder.join("Tidegauge.app");
        std::fs::create_dir_all(installed.join("Contents")).unwrap();
        std::fs::write(installed.join("Contents/marker"), "installed").unwrap();
        let missing = root.join("Downloads/Tidegauge.app");
        assert!(move_bundle(&missing, &folder).is_err());
        assert_eq!(std::fs::read_to_string(installed.join("Contents/marker")).unwrap(), "installed");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "no staging copy is left behind");
        std::fs::remove_dir_all(&root).unwrap();
    }
    use std::path::Path;

    #[test]
    fn applications_is_preferred_and_the_home_folder_is_the_fallback() {
        assert_eq!(destination_folder(true, Path::new("/Users/a")), Path::new("/Applications"));
        assert_eq!(destination_folder(false, Path::new("/Users/a")), Path::new("/Users/a/Applications"));
    }

    #[test]
    fn the_prompt_is_offered_once_and_only_where_it_helps() {
        assert!(should_offer(true, false, false, false, false), "a release bundle outside Applications");
        assert!(!should_offer(true, true, false, false, false), "already installed");
        assert!(!should_offer(true, false, true, false, false), "declined before");
        assert!(!should_offer(false, false, false, false, false), "a bare binary from cargo run");
        assert!(!should_offer(true, false, false, true, false), "a development preview");
        assert!(should_offer(true, false, false, true, true), "a preview asked to show it");
    }
}
