//! Child processes that never block the caller for long. A tool can hang in
//! uninterruptible kernel sleep, where even `kill -9` does not remove it, so
//! waiting on it forever, or starting it again on top of it, is not an option.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// One external command that is run now and then. At most one instance lives
/// at a time: while a run that timed out has not died, the next run is
/// refused instead of piling another child on top of it.
pub struct Probe {
    stuck: Mutex<Option<Child>>,
}

impl Probe {
    pub const fn new() -> Self {
        Self {
            stuck: Mutex::new(None),
        }
    }

    /// The standard output of `command` if it exits successfully within
    /// `timeout`. None means unavailable: it failed, it timed out (and was
    /// killed), a previous run is still alive, or another run is in progress.
    pub fn run(&self, command: Command, timeout: Duration) -> Option<Vec<u8>> {
        let (code, bytes) = self.run_with(command, None, timeout)?;
        (code == 0).then_some(bytes)
    }

    /// Like [`Probe::run`], but feeds `input` to the command and returns the
    /// exit code with the output whatever the code is. At most 2 MiB of
    /// output is accepted.
    pub fn run_with(
        &self,
        mut command: Command,
        input: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Option<(i32, Vec<u8>)> {
        let mut stuck = self.stuck.try_lock().ok()?;
        if let Some(old) = stuck.as_mut() {
            match old.try_wait() {
                Ok(None) => {
                    let _ = old.kill();
                    return None;
                }
                _ => *stuck = None,
            }
        }
        let mut child = command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let Some(pipe) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("tidegauge-probe-read".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let read = pipe
                    .take(MAX_OUTPUT as u64 + 1)
                    .read_to_end(&mut bytes)
                    .ok();
                let _ = tx.send(read.filter(|_| bytes.len() <= MAX_OUTPUT).map(|_| bytes));
            });
        if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
            let _ = std::thread::Builder::new()
                .name("tidegauge-probe-write".into())
                .spawn(move || {
                    let _ = stdin.write_all(&bytes);
                });
        }
        let deadline = Instant::now() + timeout;
        let status = if spawned.is_ok() {
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    _ => break None,
                }
            }
        } else {
            None
        };
        let Some(status) = status else {
            // Stop waiting. A child that survives the kill is kept so the
            // next run can see it and stay away.
            let _ = child.kill();
            let reaped = Instant::now() + Duration::from_millis(500);
            while !matches!(child.try_wait(), Ok(Some(_))) {
                if Instant::now() >= reaped {
                    *stuck = Some(child);
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            return None;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let bytes = rx
            .recv_timeout(remaining.max(Duration::from_secs(1)))
            .ok()??;
        Some((status.code()?, bytes))
    }
}

const MAX_OUTPUT: usize = 2 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_returned() {
        let probe = Probe::new();
        let mut command = Command::new("/bin/echo");
        command.arg("hi");
        assert_eq!(
            probe.run(command, Duration::from_secs(5)),
            Some(b"hi\n".to_vec())
        );
    }

    #[test]
    fn failure_is_unavailable() {
        let probe = Probe::new();
        let command = Command::new("/usr/bin/false");
        assert_eq!(probe.run(command, Duration::from_secs(5)), None);
    }

    #[test]
    fn a_hanging_child_is_given_up_on_promptly() {
        let probe = Probe::new();
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let started = Instant::now();
        assert_eq!(probe.run(command, Duration::from_secs(1)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
        // A killable child is gone, so the probe is usable again.
        let mut command = Command::new("/bin/echo");
        command.arg("again");
        assert_eq!(
            probe.run(command, Duration::from_secs(5)),
            Some(b"again\n".to_vec())
        );
    }

    #[test]
    fn input_and_nonzero_exit_codes_are_returned() {
        let probe = Probe::new();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "cat; exit 3"]);
        assert_eq!(
            probe.run_with(command, Some(b"data".to_vec()), Duration::from_secs(5)),
            Some((3, b"data".to_vec()))
        );
    }

    #[test]
    fn no_second_instance_while_the_first_is_alive() {
        let probe = Probe::new();
        // Stand in for a child that survived the kill.
        let alive = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = alive.id() as i32;
        *probe.stuck.lock().unwrap() = Some(alive);
        let marker = std::env::temp_dir().join(format!("tidegauge-probe-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let mut command = Command::new("/usr/bin/touch");
        command.arg(&marker);
        // The old child is killed on this attempt, but the run is refused.
        assert_eq!(probe.run(command, Duration::from_secs(5)), None);
        assert!(!marker.exists(), "a second instance was started");
        // Once it is dead the probe runs again.
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        std::thread::sleep(Duration::from_millis(200));
        let mut command = Command::new("/usr/bin/touch");
        command.arg(&marker);
        assert_eq!(probe.run(command, Duration::from_secs(5)), Some(Vec::new()));
        assert!(marker.exists());
        let _ = std::fs::remove_file(&marker);
    }
}
