//! Quit and force quit for apps and processes you own, the way Activity
//! Monitor offers them. Every request re-reads the process first and is
//! refused if its PID, start time, owner or executable changed since the
//! user confirmed it, so a reused PID is never signalled.

use crate::locale::text as l;
use crate::process_activity::{confirmed_exit, Identity};
use crate::processes::{proc_bsd, proc_path, ProcInfo};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Force {
    /// Apps: ask to quit (they may ask to save). Processes: SIGTERM.
    Quit,
    /// Apps: force terminate. Processes: SIGKILL. Unsaved work is lost.
    ForceQuit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    OtherUser,
    Session,
    Tidegauge,
    Unknown,
}

impl Block {
    pub fn reason(self) -> &'static str {
        match self {
            Self::OtherUser => l(
                "Owned by macOS or another user",
                "Eies av macOS eller en annen bruker",
            ),
            Self::Session => l(
                "Needed by your login session",
                "Trengs av påloggingsøkten din",
            ),
            Self::Tidegauge => l("This is Tidegauge", "Dette er Tidegauge"),
            Self::Unknown => l("Identity can't be verified", "Identiteten kan ikke bekreftes"),
        }
    }
}

/// Losing one of these ends or breaks the login session. macOS restarts
/// most other agents by itself, which is why they stay allowed.
const SESSION_CRITICAL: [&str; 4] = ["loginwindow", "launchd", "WindowServer", "kernel_task"];

pub fn blocked(p: &ProcInfo) -> Option<Block> {
    let uid = unsafe { libc::getuid() };
    if p.pid <= 1 || p.start_unix <= 0. {
        return Some(Block::Unknown);
    }
    if p.pid == std::process::id() as i32 {
        return Some(Block::Tidegauge);
    }
    if uid == 0 || p.uid != uid {
        return Some(Block::OtherUser);
    }
    let file = std::path::Path::new(&p.exe_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if SESSION_CRITICAL
        .iter()
        .any(|name| *name == file || *name == p.comm)
    {
        return Some(Block::Session);
    }
    None
}

/// A macOS app or background service (not a command-line tool you ran):
/// allowed, but the confirmation says it may restart or misbehave.
pub fn part_of_macos(p: &ProcInfo) -> bool {
    ["/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/", "/Library/Apple/"]
        .iter()
        .any(|prefix| p.exe_path.starts_with(prefix))
}

fn verify(expected: &ProcInfo) -> Result<(), String> {
    if let Some(block) = blocked(expected) {
        return Err(block.reason().into());
    }
    let Some(info) = proc_bsd(expected.pid) else {
        return Err(l(
            "The process has already exited.",
            "Prosessen er allerede avsluttet.",
        )
        .into());
    };
    if Identity::of_native(&info) != Identity::of(expected)
        || info.pbi_uid != expected.uid
        || proc_path(expected.pid) != expected.exe_path
    {
        return Err(l(
            "The process changed since you chose it. Nothing was quit.",
            "Prosessen er endret siden du valgte den. Ingenting ble avsluttet.",
        )
        .into());
    }
    Ok(())
}

fn wait(ids: &[Identity], force: Force) -> (usize, bool) {
    let deadline = Instant::now() + Duration::from_secs(if force == Force::ForceQuit { 2 } else { 3 });
    loop {
        let exited = ids.iter().filter(|id| confirmed_exit(**id)).count();
        if exited == ids.len() || Instant::now() >= deadline {
            return (exited, exited == ids.len());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn outcome(name: &str, total: usize, exited: usize, force: Force) -> Result<String, String> {
    if exited == total {
        return Ok(format!("{name} {}", l("quit.", "er avsluttet.")));
    }
    if force == Force::Quit {
        return Ok(format!(
            "{name} {}",
            l(
                "was asked to quit and is still running. It may be waiting for you, or ignore the request. Force quit if it doesn't respond.",
                "ble bedt om å avslutte og kjører fortsatt. Den kan vente på deg eller ignorere forespørselen. Tving avslutning hvis den ikke svarer.",
            )
        ));
    }
    Err(format!(
        "{name}: {exited}/{total} {}",
        l(
            "stopped. macOS may have restarted the rest.",
            "stoppet. macOS kan ha startet resten på nytt.",
        )
    ))
}

/// Quit or force quit a running app through AppKit, like the Dock does.
pub fn quit_app(main: &ProcInfo, name: &str, force: Force) -> Result<String, String> {
    use objc2_app_kit::NSRunningApplication;
    verify(main)?;
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(main.pid)
        .ok_or_else(|| l("The app is no longer running.", "Appen kjører ikke lenger.").to_string())?;
    if app
        .executableURL()
        .and_then(|url| url.path())
        .map(|path| path.to_string())
        .as_deref()
        != Some(main.exe_path.as_str())
    {
        return Err(l(
            "The app changed since you chose it. Nothing was quit.",
            "Appen er endret siden du valgte den. Ingenting ble avsluttet.",
        )
        .into());
    }
    let accepted = match force {
        Force::Quit => app.terminate(),
        Force::ForceQuit => app.forceTerminate(),
    };
    if !accepted {
        return Err(format!(
            "{name} {}",
            l("did not accept the request.", "godtok ikke forespørselen.")
        ));
    }
    let (exited, _) = wait(&[Identity::of(main)], force);
    outcome(name, 1, exited, force)
}

/// Send SIGTERM or SIGKILL to each process that still is the one the user
/// confirmed. Processes that changed or exited are skipped, never guessed.
pub fn signal(targets: &[ProcInfo], name: &str, force: Force) -> Result<String, String> {
    let sig = match force {
        Force::Quit => libc::SIGTERM,
        Force::ForceQuit => libc::SIGKILL,
    };
    let mut sent = Vec::new();
    let mut first_error = None;
    for target in targets {
        match verify(target) {
            Ok(()) if unsafe { libc::kill(target.pid, sig) } == 0 => sent.push(Identity::of(target)),
            Ok(()) => {
                first_error.get_or_insert_with(|| {
                    format!(
                        "{} ({} {})",
                        l("macOS refused the request", "macOS avviste forespørselen"),
                        l("error", "feil"),
                        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
                    )
                });
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if sent.is_empty() {
        return Err(first_error.unwrap_or_else(|| {
            l("Nothing could be quit.", "Ingenting kunne avsluttes.").into()
        }));
    }
    let (exited, _) = wait(&sent, force);
    outcome(name, sent.len(), exited, force)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, uid: u32, path: &str, comm: &str) -> ProcInfo {
        ProcInfo {
            pid,
            uid,
            exe_path: path.into(),
            comm: comm.into(),
            start_unix: 1_700_000_000.,
            ..Default::default()
        }
    }

    #[test]
    fn only_your_own_non_critical_processes_are_eligible() {
        let me = unsafe { libc::getuid() };
        assert_eq!(blocked(&proc(4321, me, "/Applications/Safari.app/Contents/MacOS/Safari", "Safari")), None);
        assert_eq!(blocked(&proc(4321, me, "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder", "Finder")), None);
        assert_eq!(blocked(&proc(4321, me + 1, "/usr/sbin/cfprefsd", "cfprefsd")), Some(Block::OtherUser));
        assert_eq!(blocked(&proc(4321, me, "/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow", "loginwindow")), Some(Block::Session));
        assert_eq!(blocked(&proc(1, me, "/sbin/launchd", "launchd")), Some(Block::Unknown));
        assert_eq!(blocked(&proc(std::process::id() as i32, me, "/x", "tidegauge")), Some(Block::Tidegauge));
        let mut unstarted = proc(4321, me, "/usr/local/bin/node", "node");
        unstarted.start_unix = 0.;
        assert_eq!(blocked(&unstarted), Some(Block::Unknown));
    }

    #[test]
    fn macos_parts_are_flagged_for_a_stronger_warning() {
        let me = unsafe { libc::getuid() };
        assert!(part_of_macos(&proc(5, me, "/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock", "Dock")));
        assert!(part_of_macos(&proc(5, me, "/usr/libexec/replayd", "replayd")));
        assert!(!part_of_macos(&proc(5, me, "/Applications/Slack.app/Contents/MacOS/Slack", "Slack")));
        assert!(!part_of_macos(&proc(5, me, "/opt/homebrew/bin/node", "node")));
        assert!(!part_of_macos(&proc(5, me, "/bin/sleep", "sleep")));
        assert!(!part_of_macos(&proc(5, me, "/usr/bin/python3", "python3")));
    }

    #[test]
    fn a_changed_process_is_never_signalled() {
        // Our own parent is alive and owned by us, but a wrong start time
        // must make verification fail before any signal is sent.
        let parent = unsafe { libc::getppid() };
        let mut target = proc(parent, unsafe { libc::getuid() }, "/nonexistent/changed", "changed");
        target.start_unix = 1.;
        let result = signal(&[target], "changed", Force::Quit);
        assert!(result.is_err());
        assert!(unsafe { libc::kill(parent, 0) } == 0, "parent must still be running");
    }

    #[test]
    fn a_disposable_child_can_be_quit_and_force_quit() {
        for force in [Force::Quit, Force::ForceQuit] {
            let mut child = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
            let pid = child.id() as i32;
            let mut info = None;
            for _ in 0..100 {
                if let Some(bsd) = proc_bsd(pid) {
                    if !proc_path(pid).is_empty() {
                        info = Some(bsd);
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let bsd = info.expect("child visible");
            let target = ProcInfo {
                pid,
                uid: bsd.pbi_uid,
                exe_path: proc_path(pid),
                comm: "sleep".into(),
                start_unix: bsd.pbi_start_tvsec as f64 + bsd.pbi_start_tvusec as f64 / 1_000_000.,
                ..Default::default()
            };
            assert_eq!(Identity::of(&target), Identity::of_native(&bsd));
            let reaper = std::thread::spawn(move || child.wait());
            let result = signal(&[target], "sleep", force);
            assert!(result.is_ok(), "{force:?}: {result:?}");
            assert!(reaper.join().unwrap().is_ok());
        }
    }
}
