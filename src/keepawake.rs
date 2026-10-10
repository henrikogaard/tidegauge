//! Process-owned system-sleep prevention with a bounded optional lease.

use crate::ffi::{
    CFRelease, CFStringCreateWithCString, CFTypeRef, IOPMAssertionCreateWithName,
    IOPMAssertionRelease,
};
use std::ffi::CString;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const ASSERTION_LEVEL_ON: u32 = 255;
const UTF8_ENCODING: u32 = 0x0800_0100;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub active: bool,
    pub remaining: Option<Duration>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Lease {
    active: bool,
    deadline: Option<Instant>,
}

impl Lease {
    fn start_at(&mut self, now: Instant, duration: Option<Duration>) {
        self.active = true;
        self.deadline = duration.and_then(|duration| now.checked_add(duration));
    }

    fn cancel(&mut self) {
        self.active = false;
        self.deadline = None;
    }

    fn expire_at(&mut self, now: Instant) -> bool {
        if self.active && self.deadline.is_some_and(|deadline| now >= deadline) {
            self.active = false;
            self.deadline = None;
            true
        } else {
            false
        }
    }

    fn remaining_at(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }
}

struct Assertion {
    id: u32,
    released: bool,
}

impl Assertion {
    fn create() -> Result<Self, String> {
        let assertion_type = CString::new("PreventUserIdleSystemSleep").unwrap();
        let assertion_name = CString::new("Tidegauge").unwrap();
        unsafe {
            let assertion_type =
                CFStringCreateWithCString(std::ptr::null(), assertion_type.as_ptr(), UTF8_ENCODING);
            let assertion_name_ref =
                CFStringCreateWithCString(std::ptr::null(), assertion_name.as_ptr(), UTF8_ENCODING);
            if assertion_type.is_null() || assertion_name_ref.is_null() {
                if !assertion_type.is_null() {
                    CFRelease(assertion_type as CFTypeRef);
                }
                if !assertion_name_ref.is_null() {
                    CFRelease(assertion_name_ref as CFTypeRef);
                }
                return Err(error_message(
                    "Could not create the sleep-prevention assertion name.",
                    -1,
                ));
            }
            let mut id = 0;
            let status = IOPMAssertionCreateWithName(
                assertion_type,
                ASSERTION_LEVEL_ON,
                assertion_name_ref,
                &mut id,
            );
            CFRelease(assertion_type as CFTypeRef);
            CFRelease(assertion_name_ref as CFTypeRef);
            if status == 0 {
                Ok(Self {
                    id,
                    released: false,
                })
            } else {
                Err(error_message("Could not keep the system awake.", status))
            }
        }
    }

    fn release(&mut self) -> Result<(), i32> {
        if self.released {
            return Ok(());
        }
        let status = unsafe { IOPMAssertionRelease(self.id) };
        if status == 0 {
            self.released = true;
            Ok(())
        } else {
            Err(status)
        }
    }
}

impl Drop for Assertion {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

struct State {
    assertion: Option<Assertion>,
    lease: Lease,
    error: Option<String>,
}

struct Manager {
    state: Mutex<State>,
    wake: Condvar,
}

static MANAGER: OnceLock<Arc<Manager>> = OnceLock::new();

fn manager() -> Arc<Manager> {
    MANAGER
        .get_or_init(|| {
            let manager = Arc::new(Manager {
                state: Mutex::new(State {
                    assertion: None,
                    lease: Lease::default(),
                    error: None,
                }),
                wake: Condvar::new(),
            });
            let worker = manager.clone();
            std::thread::Builder::new()
                .name("tidegauge-keepawake".into())
                .spawn(move || expiry_worker(worker))
                .expect("keep-awake expiry worker");
            manager
        })
        .clone()
}

fn expiry_worker(manager: Arc<Manager>) {
    loop {
        let mut state = manager.state.lock().unwrap();
        while state.assertion.is_none() {
            state = manager.wake.wait(state).unwrap();
        }
        let now = Instant::now();
        let Some(deadline) = state.lease.deadline else {
            state = manager.wake.wait(state).unwrap();
            drop(state);
            continue;
        };
        if now < deadline {
            let wait = deadline.saturating_duration_since(now);
            let (next, _) = manager.wake.wait_timeout(state, wait).unwrap();
            drop(next);
            continue;
        }
        state.lease.expire_at(now);
        if let Some(assertion) = state.assertion.as_mut() {
            match assertion.release() {
                Ok(()) => {
                    state.assertion = None;
                    state.error = None;
                    manager.wake.notify_all();
                }
                Err(status) => {
                    state.error = Some(error_message(
                        "Could not release the sleep-prevention assertion.",
                        status,
                    ));
                    state.lease.active = true;
                    state.lease.deadline = Some(Instant::now() + Duration::from_secs(5));
                    let (next, _) = manager
                        .wake
                        .wait_timeout(state, Duration::from_secs(5))
                        .unwrap();
                    drop(next);
                }
            }
        }
    }
}

fn error_message(english: &'static str, status: i32) -> String {
    format!(
        "{} (OSStatus {status})",
        crate::locale::text(
            english,
            match english {
                "Could not create the sleep-prevention assertion name." =>
                    "Kunne ikke opprette navnet på søvnforhindringspåstanden.",
                "Could not keep the system awake." => "Kunne ikke holde systemet våkent.",
                _ => "Kunne ikke frigjøre søvnforhindringspåstanden.",
            }
        )
    )
}

pub fn start(duration: Option<Duration>) -> Result<(), String> {
    if duration.is_some_and(|duration| duration.is_zero()) {
        return Err(crate::locale::text(
            "The keep-awake duration must be positive.",
            "Varigheten for å holde våken må være positiv.",
        )
        .into());
    }
    let manager = manager();
    let mut state = manager.state.lock().unwrap();
    if state.assertion.is_none() {
        state.assertion = Some(Assertion::create()?);
        state.error = None;
    }
    state.lease.start_at(Instant::now(), duration);
    manager.wake.notify_all();
    Ok(())
}

pub fn stop() -> Result<(), String> {
    let manager = manager();
    let mut state = manager.state.lock().unwrap();
    let Some(assertion) = state.assertion.as_mut() else {
        state.lease.cancel();
        state.error = None;
        return Ok(());
    };
    if let Err(status) = assertion.release() {
        let error = error_message("Could not release the sleep-prevention assertion.", status);
        state.error = Some(error.clone());
        return Err(error);
    }
    state.assertion = None;
    state.lease.cancel();
    state.error = None;
    manager.wake.notify_all();
    Ok(())
}

pub fn shutdown() {
    let _ = stop();
}

pub fn status() -> Status {
    let manager = manager();
    let state = manager.state.lock().unwrap();
    Status {
        active: state.assertion.is_some(),
        remaining: state
            .assertion
            .as_ref()
            .and_then(|_| state.lease.remaining_at(Instant::now())),
        error: state.error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_is_controllable_without_sleeping() {
        let now = Instant::now();
        let mut lease = Lease::default();
        lease.start_at(now, Some(Duration::from_secs(30)));
        assert_eq!(
            lease.remaining_at(now + Duration::from_secs(10)),
            Some(Duration::from_secs(20))
        );
        assert!(!lease.expire_at(now + Duration::from_secs(29)));
        assert!(lease.expire_at(now + Duration::from_secs(30)));
        assert!(!lease.active);
    }

    #[test]
    fn cancel_clears_expiry() {
        let now = Instant::now();
        let mut lease = Lease::default();
        lease.start_at(now, Some(Duration::from_secs(30)));
        lease.cancel();
        assert!(!lease.active);
        assert_eq!(lease.remaining_at(now), None);
    }

    #[test]
    fn replacement_updates_deadline_or_indefinite_state() {
        let now = Instant::now();
        let mut lease = Lease::default();
        lease.start_at(now, Some(Duration::from_secs(30)));
        lease.start_at(now + Duration::from_secs(5), Some(Duration::from_secs(60)));
        assert_eq!(
            lease.remaining_at(now + Duration::from_secs(5)),
            Some(Duration::from_secs(60))
        );
        lease.start_at(now + Duration::from_secs(6), None);
        assert_eq!(lease.remaining_at(now + Duration::from_secs(6)), None);
        assert!(lease.active);
    }

    #[test]
    #[ignore = "requires a live IOKit power assertion service"]
    fn native_keepawake_assertion_round_trip() {
        start(Some(Duration::from_secs(1))).unwrap();
        assert!(status().active);
        stop().unwrap();
        assert!(!status().active);
    }
}
