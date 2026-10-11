//! On-demand signing metadata, not signature validation or a reputation verdict.
use crate::processes::{proc_bsd, proc_path, ProcInfo};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pid: i32,
    started: u64,
    uid: u32,
    path: String,
}

impl Key {
    pub fn of(process: &ProcInfo) -> Self {
        Self {
            pid: process.pid,
            started: crate::process_activity::Identity::of(process).started,
            uid: process.uid,
            path: process.exe_path.clone(),
        }
    }

    fn is_current(&self) -> bool {
        if self.pid <= 0 || self.started == 0 || !self.path.starts_with('/') {
            return false;
        }
        proc_bsd(self.pid).is_some_and(|p| {
            p.pbi_pid == self.pid as u32
                && p.pbi_start_tvsec * 1_000_000 + p.pbi_start_tvusec == self.started
                && p.pbi_uid == self.uid
                && proc_path(self.pid) == self.path
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub identifier: Option<String>,
    pub team: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Changed,
    Unavailable,
}

#[derive(Default)]
pub struct State {
    pub pending: Option<Key>,
    result: Option<(Key, Result<Metadata, Failure>, Instant)>,
}

impl State {
    pub fn finish(&mut self, key: Key, result: Result<Metadata, Failure>) {
        self.pending = None;
        self.result = Some((key, result, Instant::now()));
    }

    pub fn result(&self, key: &Key) -> Option<&Result<Metadata, Failure>> {
        self.result.as_ref().and_then(|(saved, result, at)| {
            (saved == key && at.elapsed() < Duration::from_secs(60)).then_some(result)
        })
    }
}

pub fn inspect(key: &Key) -> Result<Metadata, Failure> {
    inspect_with(|| key.is_current(), || native_metadata(key.pid))
}

fn inspect_with(
    current: impl Fn() -> bool,
    lookup: impl FnOnce() -> Result<Metadata, Failure>,
) -> Result<Metadata, Failure> {
    if !current() {
        return Err(Failure::Changed);
    }
    let result = lookup();
    if !current() {
        return Err(Failure::Changed);
    }
    result
}

#[cfg(target_os = "macos")]
fn native_metadata(pid: i32) -> Result<Metadata, Failure> {
    use crate::ffi::*;
    use objc2_core_foundation::{CFDictionary, CFNumber, CFString};
    use std::ffi::{c_void, CStr};
    use std::ptr;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecGuestAttributePid: CFStringRef;
        static kSecCodeInfoIdentifier: CFStringRef;
        static kSecCodeInfoTeamIdentifier: CFStringRef;
        fn SecCodeCopyGuestWithAttributes(
            host: CFTypeRef,
            attributes: CFDictionaryRef,
            flags: u32,
            guest: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecCodeCopySigningInformation(
            code: CFTypeRef,
            flags: u32,
            information: *mut CFDictionaryRef,
        ) -> OSStatus;
    }

    struct Owned(CFTypeRef);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    unsafe fn string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
        let value = CFDictionaryGetValue(dict, key);
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return None;
        }
        let mut buffer = [0i8; 512];
        if CFStringGetCString(value, buffer.as_mut_ptr(), 512, K_CF_STRING_ENCODING_UTF8) == 0 {
            return None;
        }
        let text = CStr::from_ptr(buffer.as_ptr()).to_str().ok()?;
        (!text.is_empty() && !text.chars().any(char::is_control)).then(|| text.to_owned())
    }

    unsafe {
        let pid_key = (kSecGuestAttributePid as *const CFString)
            .as_ref()
            .ok_or(Failure::Unavailable)?;
        let pid_value = CFNumber::new_i32(pid);
        let attributes = CFDictionary::from_slices(&[pid_key], &[&*pid_value]);
        let mut code = Owned(ptr::null());
        if SecCodeCopyGuestWithAttributes(
            ptr::null(),
            (&*attributes as *const CFDictionary<CFString, CFNumber>).cast::<c_void>(),
            0,
            &mut code.0,
        ) != 0
            || code.0.is_null()
        {
            return Err(Failure::Unavailable);
        }
        let mut info = Owned(ptr::null());
        // kSecCSSigningInformation; no validity, trust, or online reputation check.
        if SecCodeCopySigningInformation(code.0, 1 << 1, &mut info.0) != 0
            || info.0.is_null()
            || CFGetTypeID(info.0) != CFDictionaryGetTypeID()
        {
            return Err(Failure::Unavailable);
        }
        Ok(Metadata {
            identifier: string(info.0, kSecCodeInfoIdentifier),
            team: string(info.0, kSecCodeInfoTeamIdentifier),
        })
    }
}

#[cfg(not(target_os = "macos"))]
fn native_metadata(_: i32) -> Result<Metadata, Failure> {
    Err(Failure::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_changes_discard_metadata_and_skip_stale_requests() {
        assert_eq!(
            inspect_with(|| false, || panic!("stale lookup must not run")),
            Err(Failure::Changed)
        );
        let reads = std::cell::Cell::new(0);
        assert_eq!(
            inspect_with(
                || {
                    reads.set(reads.get() + 1);
                    reads.get() == 1
                },
                || Ok(Metadata {
                    identifier: Some("example".into()),
                    team: None
                })
            ),
            Err(Failure::Changed)
        );
        assert_eq!(
            inspect_with(|| true, || Err(Failure::Unavailable)),
            Err(Failure::Unavailable)
        );
    }

    #[test]
    fn cache_is_bounded_and_does_not_follow_reused_or_changed_processes() {
        let process = ProcInfo {
            pid: 100,
            uid: 501,
            start_unix: 10.,
            exe_path: "/example".into(),
            ..Default::default()
        };
        let key = Key::of(&process);
        let mut state = State::default();
        let unsigned = Ok(Metadata {
            identifier: None,
            team: None,
        });
        state.finish(key.clone(), unsigned.clone());
        assert_eq!(state.result(&key), Some(&unsigned));
        for other in [
            ProcInfo {
                start_unix: 11.,
                ..process.clone()
            },
            ProcInfo {
                uid: 502,
                ..process.clone()
            },
            ProcInfo {
                exe_path: "/changed".into(),
                ..process.clone()
            },
        ] {
            assert!(state.result(&Key::of(&other)).is_none());
        }
        state.result.as_mut().unwrap().2 = Instant::now() - Duration::from_secs(61);
        assert!(state.result(&key).is_none());
    }

    #[test]
    #[ignore = "read-only native signing metadata probe"]
    fn native_signing_probe() {
        let pid = std::process::id() as i32;
        let native = proc_bsd(pid).unwrap();
        let process = ProcInfo {
            pid,
            uid: native.pbi_uid,
            start_unix: native.pbi_start_tvsec as f64 + native.pbi_start_tvusec as f64 / 1_000_000.,
            exe_path: proc_path(pid),
            ..Default::default()
        };
        let metadata = inspect(&Key::of(&process)).unwrap();
        assert!(metadata.identifier.is_some());
    }
}
