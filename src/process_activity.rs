//! Session-local process attribution. A missing sample is not evidence of exit.

use crate::processes::{bundle_root, proc_bsd, ProcInfo};
use std::collections::{BTreeMap, HashMap, HashSet};

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

    pub(crate) fn of_native(p: &crate::ffi::proc_bsdinfo) -> Self {
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
    pub mains: Vec<ProcInfo>,
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
    /// The bundle macOS registers for each running app. Fixed for the life of
    /// a process, so each is asked for once: every lookup is a round trip to
    /// LaunchServices.
    bundles: BundleCache,
}

/// What was learned about each process, keyed by pid and start time.
#[derive(Default)]
struct BundleCache(HashMap<Identity, String>);

impl BundleCache {
    /// The cached bundle for `id`, else `lookup()`, which is then kept. No
    /// answer is not kept: an app may not be registered yet.
    fn resolve(&mut self, id: Identity, lookup: impl FnOnce() -> Option<String>) -> Option<String> {
        if let Some(bundle) = self.0.get(&id) {
            return Some(bundle.clone());
        }
        let bundle = lookup()?;
        self.0.insert(id, bundle.clone());
        Some(bundle)
    }

    /// Forget processes that are no longer running.
    fn retain(&mut self, live: &HashSet<Identity>) {
        self.0.retain(|id, _| live.contains(id));
    }
}

impl Tracker {
    pub fn sample(&mut self, procs: &[ProcInfo], enumerated: &HashSet<i32>) -> Vec<ProcessGroup> {
        let apps = registered_apps(procs, &mut self.bundles);
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
        self.origins.retain(|id, origin| {
            origin.observed_ancestor
                && enumerated.contains(&id.pid)
                && by_pid.get(&id.pid).is_none_or(|p| Identity::of(p) == *id)
        });
        let main_by_pid: HashMap<i32, &RegisteredApp> =
            apps.iter().map(|a| (a.process.pid, a)).collect();
        let mut by_bundle: HashMap<&str, Vec<&RegisteredApp>> = HashMap::new();
        for app in apps {
            by_bundle.entry(app.bundle.as_str()).or_default().push(app);
        }
        let previous = self.origins.clone();
        let mut groups: BTreeMap<String, ProcessGroup> = BTreeMap::new();
        for p in procs {
            let id = Identity::of(p);
            if id.started == 0 {
                continue;
            }
            let own_bundle = bundle_root(&p.exe_path);
            // Retain the observed launch session, including across a later app relaunch.
            let mut origin = if main_by_pid.contains_key(&p.pid) {
                None
            } else {
                previous
                    .get(&id)
                    .filter(|origin| origin.observed_ancestor)
                    .cloned()
            };
            if origin.is_none() {
                let mut current = p;
                let mut visited = HashSet::new();
                for _ in 0..64 {
                    if !visited.insert(current.pid) {
                        break;
                    }
                    if let Some(app) = main_by_pid.get(&current.pid) {
                        if Identity::of(&app.process) == Identity::of(current)
                            && Identity::of(current) != id
                        {
                            origin = Some(Origin {
                                main: Identity::of(current),
                                bundle: app.bundle.clone(),
                                observed_ancestor: true,
                            });
                        }
                        break;
                    }
                    if current.pid != p.pid {
                        if let Some(parent_origin) = previous
                            .get(&Identity::of(current))
                            .filter(|origin| origin.observed_ancestor)
                        {
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
            }
            if origin
                .as_ref()
                .is_some_and(|origin| !origin.observed_ancestor)
            {
                origin = None;
            }
            let Some(bundle) = own_bundle.or_else(|| origin.as_ref().map(|o| o.bundle.clone()))
            else {
                continue;
            };
            if let Some(o) = origin.as_ref().filter(|o| o.observed_ancestor) {
                self.origins.insert(id, o.clone());
            } else {
                self.origins.remove(&id);
            }
            let remaining = origin
                .as_ref()
                .is_some_and(|o| o.observed_ancestor && o.main != id && exited.contains(&o.main));
            let group = groups
                .entry(bundle.clone())
                .or_insert_with(|| ProcessGroup {
                    name: std::path::Path::new(&bundle)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    mains: by_bundle
                        .get(bundle.as_str())
                        .into_iter()
                        .flatten()
                        .map(|app| app.process.clone())
                        .collect(),
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

fn registered_apps(procs: &[ProcInfo], cache: &mut BundleCache) -> Vec<RegisteredApp> {
    use objc2_app_kit::NSWorkspace;
    let by_pid: HashMap<_, _> = procs.iter().map(|p| (p.pid, p)).collect();
    cache.retain(&procs.iter().map(Identity::of).collect());
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter_map(|app| {
            let p = *by_pid.get(&app.processIdentifier())?;
            let bundle = cache.resolve(Identity::of(p), || {
                if app.isTerminated() {
                    return None;
                }
                let bundle = app.bundleURL()?.path()?.to_string();
                let executable = app.executableURL()?.path()?.to_string();
                (executable == p.exe_path && bundle_root(&executable).as_deref() == Some(&bundle))
                    .then_some(bundle)
            })?;
            Some(RegisteredApp {
                process: p.clone(),
                bundle,
            })
        })
        .collect()
}

pub(crate) fn confirmed_exit(id: Identity) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_is_looked_up_once_for_as_long_as_it_runs() {
        let mut cache = BundleCache::default();
        let (a, b) = (
            Identity {
                pid: 1,
                started: 10,
            },
            Identity {
                pid: 2,
                started: 20,
            },
        );
        let mut lookups = 0;
        for _ in 0..3 {
            assert_eq!(
                cache.resolve(a, || {
                    lookups += 1;
                    Some("/A.app".into())
                }),
                Some("/A.app".into())
            );
            assert_eq!(
                cache.resolve(b, || {
                    lookups += 1;
                    None
                }),
                None
            );
        }
        // A: once. B has no bundle, so it is asked every time.
        assert_eq!(lookups, 4);
        // The pid comes back for another process: it is asked again.
        cache.retain(&HashSet::from([b]));
        let reused = Identity {
            pid: 1,
            started: 99,
        };
        assert_eq!(
            cache.resolve(reused, || {
                lookups += 1;
                Some("/B.app".into())
            }),
            Some("/B.app".into())
        );
        assert_eq!(
            cache.resolve(a, || {
                lookups += 1;
                None
            }),
            None
        );
        assert_eq!(lookups, 6);
    }
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
    fn bundle_only_processes_have_unknown_lifecycle() {
        let mut tracker = Tracker::default();
        let daemon = proc(11, 1, "/opt/tool");
        let helper = proc(12, 1, "/Apps/Test.app/Contents/Helpers/helper");
        let groups = update(&mut tracker, &[daemon, helper], &[], &[]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].remaining_count(), 0);
        assert!(groups[0].rows.iter().all(|row| row.origin.is_none()));
    }
    #[test]
    fn bundle_only_helper_with_two_mains_never_becomes_remaining() {
        let first = proc(10, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let second = proc(20, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let helper = proc(30, 1, "/Apps/Test.app/Contents/Helpers/helper");
        let apps = [
            RegisteredApp {
                process: first.clone(),
                bundle: "/Apps/Test.app".into(),
            },
            RegisteredApp {
                process: second.clone(),
                bundle: "/Apps/Test.app".into(),
            },
        ];
        let mut tracker = Tracker::default();
        let groups = update(
            &mut tracker,
            &[first.clone(), second.clone(), helper.clone()],
            &apps,
            &[],
        );
        assert_eq!(groups[0].mains.len(), 2);
        assert!(groups[0]
            .rows
            .iter()
            .filter(|row| row.process.pid == first.pid || row.process.pid == second.pid)
            .all(|row| row.origin.is_none()));
        let helper_row = groups[0]
            .rows
            .iter()
            .find(|row| row.process.pid == helper.pid)
            .unwrap();
        assert!(helper_row.origin.is_none());
        assert!(!helper_row.remaining);

        tracker.origins.insert(
            Identity::of(&helper),
            Origin {
                main: Identity::of(&first),
                bundle: "/Apps/Test.app".into(),
                observed_ancestor: false,
            },
        );
        let groups = update(
            &mut tracker,
            &[second.clone(), helper.clone()],
            &[apps[1].clone()],
            &[Identity::of(&first)],
        );
        let helper_row = groups[0]
            .rows
            .iter()
            .find(|row| row.process.pid == helper.pid)
            .unwrap();
        assert!(helper_row.origin.is_none());
        assert!(!helper_row.remaining);

        let groups = update(
            &mut tracker,
            std::slice::from_ref(&helper),
            &[],
            &[Identity::of(&first), Identity::of(&second)],
        );
        let helper_row = &groups[0].rows[0];
        assert!(helper_row.origin.is_none());
        assert!(!helper_row.remaining);
    }
    #[test]
    fn observed_helper_is_remaining_after_its_main_exits_while_another_runs() {
        let first = proc(10, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let second = proc(20, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let helper = proc(30, first.pid, "/opt/tool");
        let apps = [
            RegisteredApp {
                process: first.clone(),
                bundle: "/Apps/Test.app".into(),
            },
            RegisteredApp {
                process: second.clone(),
                bundle: "/Apps/Test.app".into(),
            },
        ];
        let mut tracker = Tracker::default();
        let groups = update(
            &mut tracker,
            &[first.clone(), second.clone(), helper.clone()],
            &apps,
            &[],
        );
        let origin = groups[0]
            .rows
            .iter()
            .find(|row| row.process.pid == helper.pid)
            .unwrap()
            .origin
            .as_ref()
            .unwrap();
        assert!(origin.observed_ancestor);
        assert_eq!(origin.main, Identity::of(&first));

        let mut detached = helper;
        detached.ppid = 1;
        let groups = update(
            &mut tracker,
            &[second.clone(), detached.clone()],
            &[apps[1].clone()],
            &[Identity::of(&first)],
        );
        assert_eq!(groups[0].mains.len(), 1);
        let helper_row = groups[0]
            .rows
            .iter()
            .find(|row| row.process.pid == detached.pid)
            .unwrap();
        assert!(helper_row.remaining);
        assert_eq!(
            helper_row.origin.as_ref().unwrap().main,
            Identity::of(&first)
        );
    }
    #[test]
    fn observed_ancestry_survives_app_relaunch_without_reassignment() {
        let main = proc(10, 1, "/Apps/Test.app/Contents/MacOS/Test");
        let helper = proc(11, main.pid, "/opt/tool");
        let app = RegisteredApp {
            process: main.clone(),
            bundle: "/Apps/Test.app".into(),
        };
        let mut tracker = Tracker::default();
        let groups = update(&mut tracker, &[main.clone(), helper.clone()], &[app], &[]);
        let origin = groups[0]
            .rows
            .iter()
            .find(|row| row.process.pid == helper.pid)
            .unwrap()
            .origin
            .as_ref()
            .unwrap();
        assert!(origin.observed_ancestor);
        assert_eq!(origin.main, Identity::of(&main));

        let mut detached = helper;
        detached.ppid = 1;
        let new_main = proc(15, 1, &main.exe_path);
        let new_app = RegisteredApp {
            process: new_main.clone(),
            bundle: "/Apps/Test.app".into(),
        };
        let groups = update(
            &mut tracker,
            &[new_main, detached],
            &[new_app],
            &[Identity::of(&main)],
        );
        assert_eq!(groups[0].remaining_count(), 1);
        assert_eq!(
            groups[0]
                .rows
                .iter()
                .find(|row| row.process.pid == 11)
                .unwrap()
                .origin
                .as_ref()
                .unwrap()
                .main,
            Identity::of(&main)
        );
    }
}
