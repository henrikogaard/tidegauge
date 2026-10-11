//! Session-local process attribution. A missing sample is not evidence of exit.

use crate::locale::text as l;
use crate::processes::{bundle_root, proc_bsd, proc_path, ProcInfo};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    pub pid: i32,
    pub started: u64,
}

impl Identity {
    pub fn of(p: &ProcInfo) -> Self {
        Self {
            pid: p.pid,
            started: (p.start_unix * 1_000_000.).round() as u64,
        }
    }

    fn of_native(p: &crate::ffi::proc_bsdinfo) -> Self {
        Self {
            pid: p.pbi_pid as i32,
            started: p.pbi_start_tvsec * 1_000_000 + p.pbi_start_tvusec,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub main: Identity,
    pub bundle: String,
    pub observed_ancestor: bool,
}

#[derive(Clone, Debug)]
pub struct ProcessRow {
    pub process: ProcInfo,
    pub origin: Option<Origin>,
    pub remaining: bool,
}

#[derive(Clone, Debug)]
pub struct RegisteredApp {
    pub process: ProcInfo,
    pub bundle: String,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessGroup {
    pub bundle: String,
    pub name: String,
    pub main: Option<ProcInfo>,
    pub rows: Vec<ProcessRow>,
}

impl ProcessGroup {
    pub fn remaining_count(&self) -> usize {
        self.rows.iter().filter(|r| r.remaining).count()
    }
}

#[derive(Default)]
pub struct Tracker {
    origins: HashMap<Identity, Origin>,
}

impl Tracker {
    pub fn sample(&mut self, procs: &[ProcInfo], enumerated: &HashSet<i32>) -> Vec<ProcessGroup> {
        let apps = registered_apps(procs);
        let exited: HashSet<Identity> = self
            .origins
            .values()
            .map(|o| o.main)
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|id| confirmed_exit(*id))
            .collect();
        self.update(procs, enumerated, &apps, &exited)
    }

    fn update(
        &mut self,
        procs: &[ProcInfo],
        enumerated: &HashSet<i32>,
        apps: &[RegisteredApp],
        exited: &HashSet<Identity>,
    ) -> Vec<ProcessGroup> {
        let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
        self.origins.retain(|id, _| {
            enumerated.contains(&id.pid)
                && by_pid.get(&id.pid).is_none_or(|p| Identity::of(p) == *id)
        });
        let main_by_pid: HashMap<i32, &RegisteredApp> =
            apps.iter().map(|a| (a.process.pid, a)).collect();
        let by_bundle: HashMap<&str, &RegisteredApp> =
            apps.iter().map(|a| (a.bundle.as_str(), a)).collect();
        let previous = self.origins.clone();
        let mut groups: BTreeMap<String, ProcessGroup> = BTreeMap::new();
        for p in procs {
            let id = Identity::of(p);
            if id.started == 0 {
                continue;
            }
            let own_bundle = bundle_root(&p.exe_path);
            // Retain the observed launch session, including across a later app relaunch.
            let mut origin = previous.get(&id).cloned();
            if origin.is_none() {
                let mut current = p;
                let mut visited = HashSet::new();
                for _ in 0..64 {
                    if !visited.insert(current.pid) {
                        break;
                    }
                    if let Some(app) = main_by_pid.get(&current.pid) {
                        if Identity::of(&app.process) == Identity::of(current) {
                            origin = Some(Origin {
                                main: Identity::of(current),
                                bundle: app.bundle.clone(),
                                observed_ancestor: true,
                            });
                        }
                        break;
                    }
                    if current.pid != p.pid {
                        if let Some(parent_origin) = previous.get(&Identity::of(current)) {
                            origin = Some(parent_origin.clone());
                            break;
                        }
                    }
                    if current.ppid <= 1 {
                        break;
                    }
                    let Some(parent) = by_pid.get(&current.ppid) else {
                        break;
                    };
                    if parent.uid != p.uid || parent.start_unix > current.start_unix {
                        break;
                    }
                    current = parent;
                }
            }
            // A bundle path establishes association, not who launched the process.
            if let Some(bundle) = own_bundle.as_ref() {
                if origin.as_ref().is_some_and(|o| o.bundle != *bundle) {
                    origin = None;
                }
                if origin.is_none() {
                    if let Some(app) = by_bundle.get(bundle.as_str()) {
                        if app.process.uid == p.uid {
                            origin = Some(Origin {
                                main: Identity::of(&app.process),
                                bundle: bundle.clone(),
                                observed_ancestor: false,
                            });
                        }
                    }
                }
            }
            let Some(bundle) = own_bundle.or_else(|| origin.as_ref().map(|o| o.bundle.clone()))
            else {
                continue;
            };
            if let Some(o) = &origin {
                self.origins.insert(id, o.clone());
            } else {
                self.origins.remove(&id);
            }
            let remaining = origin
                .as_ref()
                .is_some_and(|o| o.main != id && exited.contains(&o.main));
            let group = groups
                .entry(bundle.clone())
                .or_insert_with(|| ProcessGroup {
                    name: std::path::Path::new(&bundle)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    main: by_bundle.get(bundle.as_str()).map(|a| a.process.clone()),
                    bundle,
                    rows: Vec::new(),
                });
            group.rows.push(ProcessRow {
                process: p.clone(),
                origin,
                remaining,
            });
        }
        let mut groups: Vec<_> = groups.into_values().collect();
        for g in &mut groups {
            g.rows.sort_by_key(|r| (!r.remaining, r.process.pid));
        }
        groups.sort_by(|a, b| {
            b.remaining_count()
                .cmp(&a.remaining_count())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.bundle.cmp(&b.bundle))
        });
        groups
    }
}

fn registered_apps(procs: &[ProcInfo]) -> Vec<RegisteredApp> {
    use objc2_app_kit::NSWorkspace;
    let by_pid: HashMap<_, _> = procs.iter().map(|p| (p.pid, p)).collect();
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter_map(|app| {
            if app.isTerminated() {
                return None;
            }
            let p = *by_pid.get(&app.processIdentifier())?;
            let bundle = app.bundleURL()?.path()?.to_string();
            let executable = app.executableURL()?.path()?.to_string();
            if executable != p.exe_path || bundle_root(&executable).as_deref() != Some(&bundle) {
                return None;
            }
            Some(RegisteredApp {
                process: p.clone(),
                bundle,
            })
        })
        .collect()
}

fn confirmed_exit(id: Identity) -> bool {
    if let Some(info) = proc_bsd(id.pid) {
        if info.pbi_pid == id.pid as u32 && info.pbi_start_tvsec > 0 {
            return Identity::of_native(&info) != id || info.pbi_status == 5;
        }
    }
    unsafe {
        libc::kill(id.pid, 0) != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
}

pub fn controllable(p: &ProcInfo, bundle: &str) -> bool {
    let uid = unsafe { libc::getuid() };
    uid != 0
        && p.uid == uid
        && p.pid > 1
        && p.pid != std::process::id() as i32
        && p.start_unix > 0.
        && !p.exe_path.is_empty()
        && ![
            "/System/",
            "/usr/libexec/",
            "/usr/sbin/",
            "/sbin/",
            "/Library/Apple/",
        ]
        .iter()
        .any(|prefix| p.exe_path.starts_with(prefix) || bundle.starts_with(prefix))
}

fn validate(expected: &ProcInfo, bundle: &str) -> Result<(), String> {
    if !controllable(expected, bundle) {
        return Err(l(
            "This process is protected or belongs to another user.",
            "Denne prosessen er beskyttet eller tilhører en annen bruker.",
        )
        .into());
    }
    let Some(info) = proc_bsd(expected.pid) else {
        return Err(l(
            "The process is gone or its identity cannot be verified.",
            "Prosessen er avsluttet, eller identiteten kan ikke bekreftes.",
        )
        .into());
    };
    if Identity::of_native(&info) != Identity::of(expected)
        || info.pbi_uid != expected.uid
        || info.pbi_ruid != expected.uid
        || proc_path(expected.pid) != expected.exe_path
    {
        return Err(l(
            "The process identity changed. Nothing was stopped.",
            "Prosessidentiteten er endret. Ingenting ble stoppet.",
        )
        .into());
    }
    Ok(())
}

pub fn stop_remaining(row: &ProcessRow, bundle: &str) -> Result<String, String> {
    let id = Identity::of(&row.process);
    let current = crate::engine::latest();
    let matches = current.process_groups.iter().any(|g| {
        g.bundle == bundle
            && g.rows
                .iter()
                .any(|r| r.remaining && Identity::of(&r.process) == id && r.origin == row.origin)
    });
    if !row.remaining || !matches || !row.origin.as_ref().is_some_and(|o| confirmed_exit(o.main)) {
        return Err(l("The confirmed helper is no longer eligible. Refresh and review its state.", "Den bekreftede hjelpeprosessen kan ikke stoppes nå. Oppdater og kontroller tilstanden.").into());
    }
    validate(&row.process, bundle)?;
    if unsafe { libc::kill(id.pid, libc::SIGTERM) } != 0 {
        return Err(format!(
            "{} ({})",
            l("The stop request failed", "Stoppforespørselen mislyktes"),
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        ));
    }
    Ok(wait_for_exit(id, false))
}

pub fn quit_app(main: &ProcInfo, bundle: &str) -> Result<String, String> {
    use objc2_app_kit::NSRunningApplication;
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(main.pid).ok_or_else(
        || {
            l(
                "The app is no longer available.",
                "Appen er ikke lenger tilgjengelig.",
            )
            .to_string()
        },
    )?;
    if app
        .bundleURL()
        .and_then(|u| u.path())
        .map(|s| s.to_string())
        .as_deref()
        != Some(bundle)
        || app
            .executableURL()
            .and_then(|u| u.path())
            .map(|s| s.to_string())
            .as_deref()
            != Some(&main.exe_path)
    {
        return Err(l(
            "The app identity changed. Nothing was quit.",
            "Appidentiteten er endret. Ingenting ble avsluttet.",
        )
        .into());
    }
    validate(main, bundle)?;
    if !app.terminate() {
        return Err(l(
            "The app did not accept the quit request.",
            "Appen godtok ikke avslutningsforespørselen.",
        )
        .into());
    }
    Ok(wait_for_exit(Identity::of(main), true))
}

fn wait_for_exit(id: Identity, app: bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if confirmed_exit(id) {
            return l(
                "The selected process exited.",
                "Den valgte prosessen er avsluttet.",
            )
            .into();
        }
        if Instant::now() >= deadline {
            return if app { l("Quit requested; the app is still running. It may need you to save or dismiss a dialog.", "Avslutning forespurt; appen kjører fortsatt. Du må kanskje lagre eller lukke en dialog.") }
                else { l("SIGTERM sent; the process is still running or its exit cannot be verified. A service may restart it.", "SIGTERM sendt; prosessen kjører fortsatt, eller avslutning kan ikke bekreftes. En tjeneste kan starte den på nytt.") }.into();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn proc(pid: i32, ppid: i32, path: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            uid: 501,
            start_unix: pid as f64,
            exe_path: path.into(),
            ..Default::default()
        }
    }
    fn update(
        t: &mut Tracker,
        procs: &[ProcInfo],
        apps: &[RegisteredApp],
        exited: &[Identity],
    ) -> Vec<ProcessGroup> {
        t.update(
            procs,
            &procs.iter().map(|p| p.pid).collect(),
            apps,
            &exited.iter().copied().collect(),
        )
    }
    #[test]
    fn keeps_observed_external_helper_after_parent_exit_but_not_pid_reuse() {
        let main = proc(10, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let helper = proc(11, 10, "/opt/tool");
        let app = RegisteredApp {
            process: main.clone(),
            bundle: "/Apps/Test.app".into(),
        };
        let mut tracker = Tracker::default();
        let groups = update(&mut tracker, &[helper.clone(), main.clone()], &[app], &[]);
        assert_eq!(groups[0].rows.len(), 2);
        assert_eq!(groups[0].remaining_count(), 0);
        let mut detached = helper;
        detached.ppid = 1;
        let groups = update(&mut tracker, &[detached.clone()], &[], &[]);
        assert_eq!(
            groups[0].remaining_count(),
            0,
            "missing main sample is not exit evidence"
        );
        let groups = update(
            &mut tracker,
            &[detached.clone()],
            &[],
            &[Identity::of(&main)],
        );
        assert_eq!(groups[0].remaining_count(), 1);
        assert!(groups[0].rows[0].origin.as_ref().unwrap().observed_ancestor);
        detached.start_unix += 1.;
        assert!(update(&mut tracker, &[detached], &[], &[Identity::of(&main)]).is_empty());
    }
    #[test]
    fn does_not_invent_origin_for_preexisting_daemons_or_bundle_helpers() {
        let mut tracker = Tracker::default();
        let daemon = proc(11, 1, "/opt/tool");
        let helper = proc(12, 1, "/Apps/Test.app/Contents/Helpers/helper");
        let groups = update(&mut tracker, &[daemon, helper], &[], &[]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].remaining_count(), 0);
        assert!(groups[0].rows[0].origin.is_none());
    }
    #[test]
    fn protected_targets_are_not_controllable() {
        let mut p = proc(
            std::process::id() as i32,
            1,
            "/Apps/Test.app/Contents/MacOS/Test",
        );
        p.uid = unsafe { libc::getuid() };
        assert!(!controllable(&p, "/Apps/Test.app"));
        p.pid = 1;
        assert!(!controllable(&p, "/Apps/Test.app"));
        p.pid = 12345;
        p.uid = unsafe { libc::getuid() }.wrapping_add(1);
        assert!(!controllable(&p, "/Apps/Test.app"));
        p.uid = unsafe { libc::getuid() };
        p.exe_path = "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder".into();
        assert!(!controllable(&p, "/System/Library/CoreServices/Finder.app"));
    }
    #[test]
    fn bundle_association_is_not_ancestry_and_relaunch_does_not_rewrite_origin() {
        let main = proc(10, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let helper = proc(11, 1, "/Apps/Test.app/Contents/Helpers/helper");
        let app = RegisteredApp {
            process: main.clone(),
            bundle: "/Apps/Test.app".into(),
        };
        let mut tracker = Tracker::default();
        let groups = update(&mut tracker, &[main.clone(), helper.clone()], &[app], &[]);
        assert!(
            !groups[0]
                .rows
                .iter()
                .find(|r| r.process.pid == 11)
                .unwrap()
                .origin
                .as_ref()
                .unwrap()
                .observed_ancestor
        );
        let new_main = proc(15, 1, &main.exe_path);
        let new_app = RegisteredApp {
            process: new_main.clone(),
            bundle: "/Apps/Test.app".into(),
        };
        let groups = update(
            &mut tracker,
            &[new_main, helper],
            &[new_app],
            &[Identity::of(&main)],
        );
        assert_eq!(groups[0].remaining_count(), 1);
        assert_eq!(
            groups[0].rows[0].origin.as_ref().unwrap().main,
            Identity::of(&main)
        );
    }
}
