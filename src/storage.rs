//! Metadata-only, descriptor-relative scan. Never follows links or reads file contents.
use crate::locale::text as l;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Debug, Default)]
pub struct Item {
    pub path: PathBuf,
    pub logical: u64,
    pub allocated: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub root: PathBuf,
    pub files: u64,
    pub logical: u64,
    pub allocated: u64,
    pub skipped: u64,
    pub errors: u64,
    pub limited: bool,
    pub cancelled: bool,
    pub finished: bool,
    pub folders: Vec<Item>,
    pub largest: Vec<Item>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Scan {
    pub progress: Arc<Mutex<Progress>>,
    pub cancel: Arc<AtomicBool>,
}
impl Drop for Scan {
    fn drop(&mut self) {
        if Arc::strong_count(&self.cancel) <= 2 {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
}

struct Directory(*mut libc::DIR);
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

pub fn start(root: PathBuf) -> Scan {
    let progress = Arc::new(Mutex::new(Progress {
        root: root.clone(),
        ..Default::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let result = Scan {
        progress: progress.clone(),
        cancel: cancel.clone(),
    };
    std::thread::spawn(move || {
        let data = walk(&root, &cancel, &progress);
        *progress.lock().unwrap() = data;
    });
    result
}

fn open_dir(parent: i32, name: &CStr) -> Option<Directory> {
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return None;
    }
    let dir = unsafe { libc::fdopendir(fd) };
    if dir.is_null() {
        unsafe {
            libc::close(fd);
        }
        None
    } else {
        Some(Directory(dir))
    }
}

fn walk(root: &Path, cancel: &AtomicBool, progress: &Mutex<Progress>) -> Progress {
    let mut data = Progress {
        root: root.into(),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn setiopolicy_np(kind: i32, scope: i32, policy: i32) -> i32;
        }
        // Refuse hydration on this scanner thread, including directory placeholders.
        if unsafe { setiopolicy_np(3, 1, 1) } != 0 {
            data.error=Some(l("Cloud-download protection could not be enabled. The scan was not started.","Beskyttelse mot skynedlasting kunne ikke aktiveres. Skanningen ble ikke startet.").into());
            data.finished = true;
            return data;
        }
    }
    let Some(name) = CString::new(root.as_os_str().as_bytes()).ok() else {
        data.error = Some(l("The folder path is invalid.", "Mappestien er ugyldig.").into());
        data.finished = true;
        return data;
    };
    let Some(dir) = open_dir(libc::AT_FDCWD, &name) else {
        data.error = Some(
            l(
                "The folder could not be opened. Check access permissions.",
                "Mappen kunne ikke åpnes. Kontroller tilgangstillatelsene.",
            )
            .into(),
        );
        data.finished = true;
        return data;
    };
    let mut root_stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(libc::dirfd(dir.0), &mut root_stat) } != 0 {
        data.errors += 1;
        data.finished = true;
        return data;
    }
    let mut stack = vec![(dir, root.to_path_buf(), None::<PathBuf>, 0usize)];
    let mut seen = HashSet::new();
    let mut folders: HashMap<PathBuf, Item> = HashMap::new();
    let mut visited = 0u64;
    while let Some((dir, path, top, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            data.cancelled = true;
            break;
        }
        let fd = unsafe { libc::dirfd(dir.0) };
        loop {
            if cancel.load(Ordering::Relaxed) {
                data.cancelled = true;
                break;
            }
            if visited >= 100_000 {
                data.limited = true;
                break;
            }
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            let ent = unsafe { libc::readdir(dir.0) };
            if ent.is_null() {
                #[cfg(target_os = "macos")]
                if unsafe { *libc::__error() } != 0 {
                    data.errors += 1;
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            visited += 1;
            let child = path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
            let mut st = unsafe { std::mem::zeroed::<libc::stat>() };
            if unsafe { libc::fstatat(fd, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } != 0
            {
                data.errors += 1;
                continue;
            }
            if st.st_dev != root_stat.st_dev {
                data.skipped += 1;
                continue;
            }
            // SF_DATALESS: do not traverse or hydrate cloud placeholders.
            #[cfg(target_os = "macos")]
            if st.st_flags & 0x40000000 != 0 {
                data.skipped += 1;
                continue;
            }
            match st.st_mode & libc::S_IFMT {
                libc::S_IFDIR => {
                    if depth >= 64 || stack.len() >= 128 || folders.len() >= 4096 {
                        data.limited = true;
                        data.skipped += 1;
                        continue;
                    }
                    if let Some(sub) = open_dir(fd, name) {
                        let mut actual = unsafe { std::mem::zeroed::<libc::stat>() };
                        if unsafe { libc::fstat(libc::dirfd(sub.0), &mut actual) } != 0
                            || actual.st_dev != st.st_dev
                            || actual.st_ino != st.st_ino
                        {
                            data.errors += 1;
                            continue;
                        }
                        let top = top.clone().unwrap_or_else(|| child.clone());
                        folders.entry(top.clone()).or_insert_with(|| Item {
                            path: top.clone(),
                            ..Default::default()
                        });
                        stack.push((sub, child, Some(top), depth + 1));
                    } else {
                        data.errors += 1;
                    }
                }
                libc::S_IFREG => {
                    if !seen.insert((st.st_dev, st.st_ino)) {
                        data.skipped += 1;
                        continue;
                    }
                    let logical = st.st_size.max(0) as u64;
                    let allocated = (st.st_blocks.max(0) as u64).saturating_mul(512);
                    data.files += 1;
                    data.logical = data.logical.saturating_add(logical);
                    data.allocated = data.allocated.saturating_add(allocated);
                    let key = top.clone().unwrap_or_else(|| root.to_path_buf());
                    let folder = folders.entry(key.clone()).or_insert_with(|| Item {
                        path: key,
                        ..Default::default()
                    });
                    folder.logical = folder.logical.saturating_add(logical);
                    folder.allocated = folder.allocated.saturating_add(allocated);
                    data.largest.push(Item {
                        path: child,
                        logical,
                        allocated,
                    });
                    data.largest.sort_by(|a, b| {
                        b.allocated
                            .cmp(&a.allocated)
                            .then_with(|| a.path.cmp(&b.path))
                    });
                    data.largest.truncate(30);
                }
                _ => data.skipped += 1,
            }
            if visited % 250 == 0 {
                *progress.lock().unwrap() = data.clone();
            }
        }
        if data.cancelled || visited >= 100_000 {
            break;
        }
    }
    data.folders = folders.into_values().collect();
    data.folders.sort_by(|a, b| {
        b.allocated
            .cmp(&a.allocated)
            .then_with(|| a.path.cmp(&b.path))
    });
    data.finished = true;
    data
}

pub fn reveal(path: &Path) {
    if path.is_absolute() && path.exists() {
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_sparse_files_and_cancellation_are_honest() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("tidegauge-storage-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Folder.app")).unwrap();
        std::fs::write(root.join("file"), vec![1u8; 4096]).unwrap();
        std::fs::hard_link(root.join("file"), root.join("hard")).unwrap();
        symlink(&root, root.join("cycle")).unwrap();
        let sparse = std::fs::File::create(root.join("Folder.app/sparse")).unwrap();
        sparse.set_len(1_000_000).unwrap();
        let p = Mutex::new(Progress::default());
        let d = walk(&root, &AtomicBool::new(false), &p);
        assert_eq!(d.files, 2);
        assert_eq!(d.logical, 1_004_096);
        assert!(d.allocated < d.logical);
        assert_eq!(d.skipped, 2);
        assert!(d.finished);
        let d = walk(&root, &AtomicBool::new(true), &p);
        assert!(d.cancelled);
        assert_eq!(d.files, 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
