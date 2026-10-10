//! A process-held file lock prevents duplicate monitors for the current user.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

pub struct InstanceGuard {
    _file: File,
}

pub fn acquire() -> io::Result<Option<InstanceGuard>> {
    let directory = directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("tidegauge-{}", unsafe { libc::getuid() })));
    std::fs::create_dir_all(&directory)?;
    lock(&directory.join("instance.lock"))
}

fn lock(path: &Path) -> io::Result<Option<InstanceGuard>> {
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(InstanceGuard { _file: file }));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) { Ok(None) } else { Err(error) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_lock_fails_and_exit_releases_it() {
        let path = std::env::temp_dir().join(format!("tidegauge-lock-test-{}", std::process::id()));
        let first = lock(&path).unwrap().unwrap();
        assert!(lock(&path).unwrap().is_none());
        drop(first);
        assert!(lock(&path).unwrap().is_some());
        std::fs::remove_file(path).unwrap();
    }
}
