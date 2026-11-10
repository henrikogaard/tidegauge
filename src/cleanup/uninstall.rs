//! Uninstall an app together with what it left in `~/Library` (FR-36).
//! The app and every leftover go to the Trash after you confirm; a running
//! app, Apple's apps and Tidegauge itself are never offered.

use super::{gate, Candidate, Category, Method, Tier};
use crate::locale::text as l;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where leftovers named by a bundle id live, under `~/Library`.
const PLACES: [&str; 7] = ["Application Support", "Caches", "Saved Application State", "HTTPStorages", "WebKit", "Logs", "Preferences"];

fn bundle_id_of(app: &Path) -> Option<String> {
    let mut c = std::process::Command::new("/usr/bin/plutil");
    c.args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let out = crate::packages::run_command(c, Duration::from_secs(5)).ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !id.is_empty()).then_some(id)
}

/// Whether an id may be offered at all.
pub fn offered(id: &str) -> bool {
    // Only a plain reverse-DNS id: it is joined into Library paths, so "."
    // or a "/" must never get through.
    let plain = id.len() >= 3
        && id.split('.').count() >= 2
        && id.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    plain && !id.starts_with("com.apple.") && !id.contains("tidegauge")
}

/// Folders and files in `~/Library` named exactly for `id`.
pub fn leftovers_for(home: &Path, id: &str) -> Vec<PathBuf> {
    let names = [id.to_string(), format!("{id}.plist"), format!("{id}.savedState")];
    let mut out = Vec::new();
    for place in PLACES {
        for n in &names {
            let p = home.join("Library").join(place).join(n);
            if std::fs::symlink_metadata(&p).is_ok() && gate::validate(&p, home).is_ok() {
                out.push(p);
            }
        }
    }
    out
}

/// An app bundle that may go to the Trash: a real `.app` folder (no link)
/// directly in `/Applications` or `~/Applications`, or one folder below,
/// owned by you.
pub fn validate_app(app: &Path, home: &Path) -> Result<(), String> {
    let refuse = |en: &'static str, nb: &'static str| Err(l(en, nb).to_string());
    if app.extension().is_none_or(|x| x != "app") || app.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return refuse("Not an app.", "Ikke en app.");
    }
    let roots = [PathBuf::from("/Applications"), home.join("Applications")];
    let parent = app.parent().unwrap_or(Path::new("/"));
    let placed = roots.iter().any(|r| parent == r || parent.parent() == Some(r.as_path()));
    if !placed {
        return refuse("Only apps in Applications can be uninstalled here.", "Bare apper i Programmer kan avinstalleres her.");
    }
    let Ok(meta) = std::fs::symlink_metadata(app) else {
        return refuse("It no longer exists.", "Den finnes ikke lenger.");
    };
    if !meta.is_dir() || std::fs::canonicalize(app).ok().as_deref() != Some(app) {
        return refuse("It is a link, so it was left alone.", "Det er en lenke, så den ble latt være.");
    }
    if meta.uid() != unsafe { libc::getuid() } {
        return refuse("An administrator installed it; remove it in Finder.", "En administrator installerte den; fjern den i Finder.");
    }
    Ok(())
}

/// Whether any process runs from inside the app.
pub fn running(app: &Path) -> bool {
    let app = app.to_string_lossy().to_string() + "/";
    crate::processes::list_pids().into_iter().any(|pid| crate::processes::proc_path(pid).starts_with(&app))
}

fn apps_in(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in [PathBuf::from("/Applications"), home.join("Applications")] {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "app") {
                out.push(p);
            } else if e.file_type().is_ok_and(|t| t.is_dir()) {
                if let Ok(inner) = std::fs::read_dir(&p) {
                    out.extend(inner.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "app")));
                }
            }
        }
    }
    out
}

/// One app and its leftovers, ready to confirm.
pub fn candidate(app: &Path, id: &str, home: &Path) -> Candidate {
    let name = app.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut c = Candidate::folder(Category::AppLeftovers, app.to_path_buf(), name, Tier::Rebuild);
    c.id = format!("uninstall:{id}");
    let extra = leftovers_for(home, id);
    c.detail = format!("{} · {}", id, crate::locale::count(extra.len() as u64, "leftover place", "leftover places", "rest", "rester"));
    c.paths.extend(extra);
    c.watch = Vec::new();
    c.method = Method::Trash;
    c.facts = vec![("bundle_id", id.to_string())];
    c
}

/// Every app that may be uninstalled, measured, largest first. Slow: run it
/// off the main thread.
pub fn apps(home: &Path) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = apps_in(home)
        .into_iter()
        .filter(|a| validate_app(a, home).is_ok())
        .filter_map(|a| {
            let id = bundle_id_of(&a)?;
            offered(&id).then(|| candidate(&a, &id, home))
        })
        .map(|mut c| {
            c.bytes = Some(c.paths.iter().map(|p| {
                if p.is_dir() { crate::caches::folder_bytes_until(p, None).unwrap_or(0) } else { std::fs::metadata(p).map_or(0, |m| m.len()) }
            }).sum());
            if running(&c.paths[0]) {
                c.lock = Some(l("Running", "Kjører").into());
            }
            c
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.bytes.unwrap_or(0)));
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::temp;
    use super::*;

    #[test]
    fn finds_only_exact_leftovers() {
        let home = std::fs::canonicalize(temp("uninstall")).unwrap();
        let lib = home.join("Library");
        for p in ["Application Support/com.example.app", "Caches/com.example.app", "Caches/com.example.app.helper", "Saved Application State/com.example.app.savedState", "Application Support/com.example.other"] {
            std::fs::create_dir_all(lib.join(p)).unwrap();
        }
        std::fs::create_dir_all(lib.join("Preferences")).unwrap();
        std::fs::write(lib.join("Preferences/com.example.app.plist"), "x").unwrap();
        let mut got: Vec<String> = leftovers_for(&home, "com.example.app").iter().map(|p| p.strip_prefix(&lib).unwrap().display().to_string()).collect();
        got.sort();
        assert_eq!(got, vec!["Application Support/com.example.app", "Caches/com.example.app", "Preferences/com.example.app.plist", "Saved Application State/com.example.app.savedState"]);
    }

    #[test]
    fn only_apps_in_applications_and_never_apple_or_tidegauge() {
        let home = std::fs::canonicalize(temp("uninstall2")).unwrap();
        let ok = home.join("Applications/Foo.app");
        let nested = home.join("Applications/Tools/Bar.app");
        let outside = home.join("Desktop/Baz.app");
        for p in [&ok, &nested, &outside] {
            std::fs::create_dir_all(p).unwrap();
        }
        assert!(validate_app(&ok, &home).is_ok());
        assert!(validate_app(&nested, &home).is_ok());
        assert!(validate_app(&outside, &home).is_err());
        assert!(validate_app(&home.join("Applications/Gone.app"), &home).is_err());
        std::os::unix::fs::symlink(&ok, home.join("Applications/Link.app")).unwrap();
        assert!(validate_app(&home.join("Applications/Link.app"), &home).is_err());
        assert!(!offered("com.apple.Safari"));
        assert!(!offered("no.ogard.tidegauge"));
        assert!(offered("com.example.app"));
        for bad in [".", "..", "a/b.c", "com..x", "Google/Chrome", "x"] {
            assert!(!offered(bad), "{bad}");
        }
        let c = candidate(&ok, "com.example.app", &home);
        assert_eq!(c.id, "uninstall:com.example.app");
        assert_eq!(c.paths[0], ok);
        assert!(matches!(c.method, Method::Trash));
    }
}
