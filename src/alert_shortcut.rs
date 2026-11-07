//! Run one of the user's Shortcuts when an alert is sent.
//!
//! Shortcuts are listed and run with `/usr/bin/shortcuts`, always on a
//! background thread with a deadline, so neither the sampler nor the UI waits
//! on them. The name travels as its own argument, never through a shell.

use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SHORTCUTS: &str = "/usr/bin/shortcuts";
/// Listing the library is quick; a stuck helper is given up on after this.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// A Shortcut may do real work, but not forever.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// At most one run this often, whatever the alerts do.
const MIN_GAP: Duration = Duration::from_secs(30);
/// The menu is read again at most this often while Settings is open.
const LIST_REFRESH: Duration = Duration::from_secs(15);

/// The outcome of the last run.
#[derive(Clone, Debug, PartialEq)]
pub struct LastRun {
    /// Unix time the run ended.
    pub at: f64,
    /// `Err` holds a short reason.
    pub outcome: Result<(), String>,
}

/// The Shortcuts menu as last read.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub names: Vec<String>,
    /// A read finished, with or without Shortcuts to show.
    pub loaded: bool,
    /// The last read failed, so the names may be stale.
    pub failed: bool,
}

#[derive(Default)]
struct ListState {
    library: Library,
    started: Option<Instant>,
    running: bool,
}

static LIST: Mutex<Option<ListState>> = Mutex::new(None);
static LAST: Mutex<Option<LastRun>> = Mutex::new(None);
static FLOOR: Mutex<RateFloor> = Mutex::new(RateFloor { last: None });

/// Allows one run per `MIN_GAP`, by a clock the caller supplies.
struct RateFloor {
    last: Option<Duration>,
}

impl RateFloor {
    /// Whether a run may start at `now`; a run that is let through is counted.
    fn take(&mut self, now: Duration) -> bool {
        if self.last.is_some_and(|last| now < last || now - last < MIN_GAP) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

fn since_start() -> Duration {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

/// The Shortcuts as last read.
pub fn library() -> Library {
    LIST.lock().unwrap().as_ref().map(|state| state.library.clone()).unwrap_or_default()
}

pub fn last_run() -> Option<LastRun> {
    LAST.lock().unwrap().clone()
}

/// The arguments that list the library.
fn list_args() -> Vec<OsString> {
    vec!["list".into()]
}

/// The arguments that run `name` with the alert text read from `input`.
fn run_args(name: &str, input: &std::path::Path) -> Vec<OsString> {
    vec!["run".into(), name.into(), "--input-path".into(), input.into()]
}

/// One name per line, sorted without regard to case, empty lines and
/// repeats dropped.
fn parse_list(output: &str) -> Vec<String> {
    let mut names: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

/// What a Shortcut receives: the alert as shown, then its category.
fn input_text(metric: &str, message: &str) -> String {
    // A fixed key for the Shortcut to parse, not UI text.
    let category = ["category", metric].join(": ");
    format!("{message}\n\n{category}\n")
}

struct Finished {
    success: bool,
    stdout: String,
    stderr: String,
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(1 << 20).read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Run a program to its end or its deadline, whichever comes first. A program
/// still going at the deadline is killed.
fn run_bounded(program: &str, args: &[OsString], timeout: Duration) -> Result<Finished, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(error.to_string()),
        }
    };
    Ok(Finished {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Read the library again on a background thread, unless a read is running
/// or one started a moment ago. Settings calls this as it draws.
pub fn refresh_library() {
    {
        let mut guard = LIST.lock().unwrap();
        let state = guard.get_or_insert_with(ListState::default);
        if state.running || state.started.is_some_and(|at| at.elapsed() < LIST_REFRESH) {
            return;
        }
        state.running = true;
        state.started = Some(Instant::now());
    }
    let _ = std::thread::Builder::new().name("tidegauge-shortcuts-list".into()).spawn(|| {
        let result = run_bounded(SHORTCUTS, &list_args(), LIST_TIMEOUT);
        let mut guard = LIST.lock().unwrap();
        let state = guard.get_or_insert_with(ListState::default);
        state.running = false;
        match result {
            Ok(done) if done.success => {
                state.library = Library { names: parse_list(&done.stdout), loaded: true, failed: false };
            }
            _ => {
                state.library.loaded = true;
                state.library.failed = true;
            }
        }
        drop(guard);
        crate::app::PUMP.notify();
    });
}

/// Run the chosen Shortcut for an alert that was just sent. Does nothing when
/// none is chosen or one ran in the last 30 seconds.
pub fn run_for_alert(shortcut: Option<&str>, metric: &str, message: &str) {
    let Some(name) = shortcut.map(str::trim).filter(|name| !name.is_empty()) else {
        return;
    };
    if !FLOOR.lock().unwrap().take(since_start()) {
        return;
    }
    let (name, text) = (name.to_string(), input_text(metric, message));
    let _ = std::thread::Builder::new().name("tidegauge-shortcut-run".into()).spawn(move || {
        let outcome = run(&name, &text);
        *LAST.lock().unwrap() = Some(LastRun { at: crate::metrics::now_unix(), outcome });
        crate::app::PUMP.notify();
    });
}

fn run(name: &str, text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tidegauge-alert-{}-{}.txt",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|error| error.to_string())?;
    let result = run_bounded(SHORTCUTS, &run_args(name, &path), RUN_TIMEOUT);
    let _ = std::fs::remove_file(&path);
    match result? {
        done if done.success => Ok(()),
        done => Err(done.stderr.lines().next().unwrap_or("failed").trim().chars().take(120).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_output_is_sorted_trimmed_and_deduplicated() {
        assert_eq!(parse_list("Zebra\n  alpha \n\nMorning\nalpha\n"), ["alpha", "Morning", "Zebra"]);
        assert!(parse_list("\n \n").is_empty());
    }

    #[test]
    fn the_name_is_one_argument_whatever_it_holds() {
        let path = std::path::Path::new("/tmp/in.txt");
        let args = run_args("Calm; rm -rf ~ $(x) \"y\"", path);
        assert_eq!(
            args,
            ["run", "Calm; rm -rf ~ $(x) \"y\"", "--input-path", "/tmp/in.txt"].map(OsString::from)
        );
        assert_eq!(list_args(), ["list"].map(OsString::from));
    }

    #[test]
    fn the_shortcut_receives_the_alert_and_its_category() {
        let text = input_text("cpu", "High CPU usage: 95 %");
        assert_eq!(text, "High CPU usage: 95 %\n\ncategory: cpu\n");
    }

    #[test]
    fn at_most_one_run_per_thirty_seconds() {
        let secs = Duration::from_secs;
        let mut floor = RateFloor { last: None };
        assert!(floor.take(secs(5)));
        assert!(!floor.take(secs(5)));
        assert!(!floor.take(secs(34)));
        assert!(floor.take(secs(35)), "thirty seconds after the last run");
        assert!(!floor.take(secs(36)));
        assert!(!floor.take(secs(10)), "a clock that went back does not open the floor");
        assert!(floor.take(secs(70)));
    }

    #[test]
    fn nothing_runs_without_a_chosen_shortcut() {
        run_for_alert(None, "cpu", "x");
        run_for_alert(Some("  "), "cpu", "x");
        assert!(last_run().is_none());
    }

    #[test]
    fn a_bounded_run_reports_success_failure_and_timeout() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        let ok = run_bounded("/bin/echo", &args(&["hi"]), Duration::from_secs(5)).unwrap();
        assert!(ok.success && ok.stdout.trim() == "hi");
        let bad = run_bounded("/bin/sh", &args(&["-c", "echo oops >&2; exit 3"]), Duration::from_secs(5)).unwrap();
        assert!(!bad.success && bad.stderr.trim() == "oops");
        let slow = Instant::now();
        let timed_out = run_bounded("/bin/sleep", &args(&["30"]), Duration::from_millis(200));
        assert_eq!(timed_out.err().as_deref(), Some("timed out"));
        assert!(slow.elapsed() < Duration::from_secs(5));
        assert!(run_bounded("/nonexistent/program", &[], Duration::from_secs(1)).is_err());
    }
}
