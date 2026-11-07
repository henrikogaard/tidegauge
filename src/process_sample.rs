//! Sample Process, the way Activity Monitor offers it: records what a
//! process's threads are doing for a few seconds, which shows where a hung
//! app is stuck. It runs `/usr/bin/sample` for a process you own and saves the
//! report in Downloads. The process is verified again right before it runs.

use crate::locale::text as l;
use crate::processes::ProcInfo;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SAMPLE: &str = "/usr/bin/sample";
/// Seconds `sample` watches the process.
const SECONDS: u32 = 3;
/// Sampling a healthy process ends after `SECONDS`; this only stops a stuck one.
const TIMEOUT: Duration = Duration::from_secs(30);

/// "Safari-1234-sample-2026-11-07-091500.txt"; no slash or colon from an
/// app's name reaches the file system.
pub fn file_name(name: &str, pid: i32, stamp: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') || c.is_control() { '-' } else { c })
        .collect();
    let clean: String = clean.trim().trim_start_matches('.').chars().take(64).collect();
    let clean = if clean.is_empty() { "process" } else { clean.as_str() };
    format!("{clean}-{pid}-sample-{stamp}.txt")
}

/// The arguments for `sample`: the PID, the seconds, and where to write.
pub fn arguments(pid: i32, path: &Path) -> Vec<std::ffi::OsString> {
    vec![pid.to_string().into(), SECONDS.to_string().into(), "-file".into(), path.into()]
}

/// A new, empty file in `dir`, or "… 2.txt", "… 3.txt" when the name is
/// taken, so an earlier sample is never overwritten.
fn create_new(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let stem = name.strip_suffix(".txt").unwrap_or(name);
    for n in 1..100 {
        let path = if n == 1 { dir.join(name) } else { dir.join(format!("{stem} {n}") + ".txt") };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::ErrorKind::AlreadyExists.into())
}

fn downloads() -> Result<PathBuf, String> {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(Path::to_path_buf))
        .ok_or_else(|| l("Downloads could not be found.", "Fant ikke Nedlastinger.").to_string())
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.by_ref().take(64 * 1024).read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Runs `sample` for `pid` into `path`, stopping it after `timeout`.
fn run_sample(program: &str, pid: i32, path: &Path, timeout: Duration) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(arguments(pid, path))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| l("Sample could not be started.", "Stikkprøven kunne ikke startes.").to_string())?;
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(l(
                    "Sampling took too long and was stopped. The app may be too stuck to be sampled.",
                    "Stikkprøven tok for lang tid og ble stoppet. Appen kan sitte for fast til å bli undersøkt.",
                )
                .into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(l("Sampling could not be checked.", "Stikkprøven kunne ikke kontrolleres.").to_string())
            }
        }
    };
    let said = [out, err]
        .into_iter()
        .flatten()
        .filter_map(|reader| reader.join().ok())
        .collect::<Vec<_>>()
        .join("\n");
    if status.success() {
        return Ok(());
    }
    Err(failure(&said))
}

/// What `sample` said, in plain words: its own last line when it gave one.
fn failure(said: &str) -> String {
    let reason = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("Sampling process"))
        .last()
        .unwrap_or("");
    let head = l("Sampling failed", "Stikkprøven mislyktes");
    if reason.is_empty() {
        format!("{head}.")
    } else {
        format!("{head}: {reason}")
    }
}

/// Samples `target` for a few seconds and returns the saved report. Refused
/// unless it is still the same process of yours the user chose.
pub fn sample(target: &ProcInfo, name: &str) -> Result<PathBuf, String> {
    crate::process_control::verify_unchanged(
        target,
        l(
            "The process changed since you chose it. Nothing was sampled.",
            "Prosessen er endret siden du valgte den. Ingen stikkprøve ble tatt.",
        ),
    )?;
    let stamp = crate::history_browser::format_time(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0., |d| d.as_secs_f64()),
        c"%Y-%m-%d-%H%M%S",
    );
    let failed = |_| l("The file could not be saved in Downloads.", "Filen kunne ikke lagres i Nedlastinger.").to_string();
    let path = create_new(&downloads()?, &file_name(name, target.pid, &stamp)).map_err(failed)?;
    match run_sample(SAMPLE, target.pid, &path, TIMEOUT) {
        Ok(()) if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) => Ok(path),
        Ok(()) => {
            let _ = std::fs::remove_file(&path);
            Err(l("Sample finished without a report.", "Stikkprøven ble ferdig uten en rapport.").into())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe_and_carry_pid_and_time() {
        assert_eq!(file_name("Safari", 42, "2026-11-07-091500"), "Safari-42-sample-2026-11-07-091500.txt");
        assert_eq!(file_name("a/b:c\\d", 7, "t"), "a-b-c-d-7-sample-t.txt");
        assert_eq!(file_name("../etc", 7, "t"), "-etc-7-sample-t.txt");
        assert_eq!(file_name("  ", 7, "t"), "process-7-sample-t.txt");
        assert_eq!(file_name("", 7, "t"), "process-7-sample-t.txt");
        assert_eq!(file_name(".hidden", 7, "t"), "hidden-7-sample-t.txt");
        assert!(file_name(&"x".repeat(500), 7, "t").len() < 100);
        assert!(!file_name("tab\tnew\nline", 7, "t").contains(['\t', '\n']));
    }

    #[test]
    fn arguments_name_the_pid_duration_and_file() {
        let args = arguments(123, Path::new("/Users/me/Downloads/x.txt"));
        assert_eq!(args, ["123", "3", "-file", "/Users/me/Downloads/x.txt"]);
    }

    #[test]
    fn an_existing_sample_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("tidegauge-sample-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = create_new(&dir, "A-1-sample-t.txt").unwrap();
        std::fs::write(&first, "kept").unwrap();
        let second = create_new(&dir, "A-1-sample-t.txt").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failures_are_reported_in_plain_words() {
        let said = failure("Sampling process 9 for 3 seconds\nsample cannot examine process 9 because you do not have appropriate privileges\n");
        assert!(said.ends_with(": sample cannot examine process 9 because you do not have appropriate privileges"), "{said}");
        assert!(!said.contains("for 3 seconds"));
        assert!(failure("").ends_with('.'));
    }

    #[test]
    fn a_stuck_sampler_is_stopped_at_the_timeout() {
        let started = Instant::now();
        // `sleep` given sample's arguments rejects them or waits; either way
        // it must come back well before its own 30 seconds.
        let result = run_sample("/bin/sleep", 30, Path::new("/dev/null"), Duration::from_millis(300));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_changed_process_is_not_sampled() {
        use crate::processes::{proc_bsd, proc_path};
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        let result = (|| {
            let mut bsd = None;
            for _ in 0..100 {
                if let Some(info) = proc_bsd(pid) {
                    if !proc_path(pid).is_empty() {
                        bsd = Some(info);
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let bsd = bsd.ok_or("child not visible")?;
            let real = ProcInfo {
                pid,
                uid: bsd.pbi_uid,
                exe_path: proc_path(pid),
                comm: "sleep".into(),
                start_unix: bsd.pbi_start_tvsec as f64 + bsd.pbi_start_tvusec as f64 / 1_000_000.,
                ..Default::default()
            };
            // The unchanged child is eligible and passes the identity check.
            let check = |t: &ProcInfo| crate::process_control::verify_unchanged(t, "changed");
            if let Err(e) = check(&real) {
                return Err(format!("unchanged child was refused: {e}"));
            }
            let mut moved = real.clone();
            moved.start_unix += 100.;
            let mut other_exe = real.clone();
            other_exe.exe_path = "/nonexistent/other".into();
            for (what, target) in [("start time", moved), ("executable", other_exe)] {
                if check(&target) != Err("changed".to_string()) {
                    return Err(format!("a changed {what} was not refused as changed"));
                }
                if sample(&target, "sleep") != Err(l(
                    "The process changed since you chose it. Nothing was sampled.",
                    "Prosessen er endret siden du valgte den. Ingen stikkprøve ble tatt.",
                )
                .to_string())
                {
                    return Err(format!("sample did not refuse a changed {what}"));
                }
            }
            // A different owner is refused before the identity check.
            let mut other_user = real.clone();
            other_user.uid += 1;
            if sample(&other_user, "sleep").is_ok() {
                return Err("another user's process was sampled".into());
            }
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        result.unwrap();
    }

    #[test]
    #[ignore]
    fn native_sample_of_a_disposable_child() {
        use crate::processes::{proc_bsd, proc_path};
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        let result = (|| {
            let mut visible = false;
            for _ in 0..100 {
                if proc_bsd(pid).is_some() && !proc_path(pid).is_empty() {
                    visible = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if !visible {
                return Err("child not visible".to_string());
            }
            let dir = std::env::temp_dir().join(format!("tidegauge-sample-native-{}", std::process::id()));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = create_new(&dir, &file_name("sleep", pid, "native")).map_err(|e| e.to_string())?;
            let ran = run_sample(SAMPLE, pid, &path, TIMEOUT);
            let report = std::fs::read_to_string(&path).unwrap_or_default();
            let _ = std::fs::remove_dir_all(&dir);
            ran?;
            if report.contains("Call graph") {
                Ok(())
            } else {
                Err(format!("no call graph in report: {report:.200}"))
            }
        })();
        let _ = child.kill();
        let _ = child.wait();
        result.unwrap();
    }
}
