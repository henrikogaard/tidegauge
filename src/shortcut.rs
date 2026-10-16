//! A keyboard shortcut that opens the menu-bar panel from any app. Carbon's
//! hot keys need no Accessibility permission and see only this one key
//! combination, never other typing.

use std::ffi::c_void;

/// The shortcuts on offer: the setting's key, how macOS writes it, and the
/// Carbon key code and modifiers. None of them is a macOS default.
pub const CHOICES: [(&str, &str, u32, u32); 2] = [
    ("ctrl-opt-cmd-t", "⌃⌥⌘T", KEY_T, CONTROL | OPTION | COMMAND),
    ("ctrl-opt-cmd-m", "⌃⌥⌘M", KEY_M, CONTROL | OPTION | COMMAND),
];

const KEY_T: u32 = 0x11;
const KEY_M: u32 = 0x2e;
const COMMAND: u32 = 0x0100;
const OPTION: u32 = 0x0800;
const CONTROL: u32 = 0x1000;

const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const EVENT_HOT_KEY_PRESSED: u32 = 5;
const SIGNATURE: u32 = u32::from_be_bytes(*b"TDGG");
/// Carbon's "that hot key is already registered" status.
const HOT_KEY_EXISTS: i32 = -9878;

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

type Handler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: u32,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hot_key: *mut c_void) -> i32;
}

extern "C" fn pressed(_call: *mut c_void, _event: *mut c_void, _data: *mut c_void) -> i32 {
    crate::statusbar::toggle_from_keyboard();
    0
}

thread_local! {
    static HANDLER_INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static REGISTERED: std::cell::Cell<*mut c_void> = const { std::cell::Cell::new(std::ptr::null_mut()) };
    /// The choice that is actually registered, which may differ from the
    /// saved one when another app took it while Tidegauge was not running.
    static ACTIVE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// The shortcut in effect now, as a `CHOICES` key. Main thread only.
pub fn active() -> Option<&'static str> {
    ACTIVE.with(|a| a.get())
}

/// How a saved choice is shown: "⌃⌥⌘T", or `None` for off or unknown.
pub fn label(key: Option<&str>) -> Option<&'static str> {
    CHOICES.iter().find(|c| Some(c.0) == key).map(|c| c.1)
}

/// Register `key` (one of `CHOICES`) in place of any earlier shortcut, or
/// none for `None`. Main thread only. An error says why it can't be used.
pub fn apply(key: Option<&str>) -> Result<(), String> {
    use crate::locale::text as l;
    REGISTERED.with(|registered| {
        let old = registered.replace(std::ptr::null_mut());
        if !old.is_null() {
            unsafe { UnregisterEventHotKey(old) };
        }
    });
    ACTIVE.with(|a| a.set(None));
    let Some(&(choice, _, code, modifiers)) = CHOICES.iter().find(|c| Some(c.0) == key) else {
        return Ok(());
    };
    unsafe {
        let target = GetApplicationEventTarget();
        if !HANDLER_INSTALLED.with(|h| h.get()) {
            let spec = EventTypeSpec { event_class: EVENT_CLASS_KEYBOARD, event_kind: EVENT_HOT_KEY_PRESSED };
            if InstallEventHandler(target, pressed, 1, &spec, std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
                return Err(l("The shortcut could not be set up.", "Snarveien kunne ikke settes opp.").into());
            }
            HANDLER_INSTALLED.with(|h| h.set(true));
        }
        let mut hot_key = std::ptr::null_mut();
        match RegisterEventHotKey(code, modifiers, EventHotKeyId { signature: SIGNATURE, id: 1 }, target, 0, &mut hot_key) {
            0 => {
                REGISTERED.with(|r| r.set(hot_key));
                ACTIVE.with(|a| a.set(Some(choice)));
                Ok(())
            }
            HOT_KEY_EXISTS => Err(l("Another app already uses that shortcut.", "En annen app bruker allerede den snarveien.").into()),
            _ => Err(l("The shortcut could not be set up.", "Snarveien kunne ikke settes opp.").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{active, apply, label, CHOICES};

    #[test]
    fn off_and_unknown_choices_leave_no_shortcut_in_effect() {
        assert_eq!(apply(None), Ok(()));
        assert_eq!(active(), None);
        assert_eq!(apply(Some("cmd-q")), Ok(()), "an unknown saved value is ignored, not registered");
        assert_eq!(active(), None);
    }

    #[test]
    fn choices_have_labels_and_unknown_keys_are_off() {
        assert_eq!(label(Some("ctrl-opt-cmd-t")), Some("⌃⌥⌘T"));
        assert_eq!(label(Some("ctrl-opt-cmd-m")), Some("⌃⌥⌘M"));
        assert_eq!(label(None), None);
        assert_eq!(label(Some("cmd-q")), None, "an unknown saved value is no shortcut");
        assert!(CHOICES.iter().all(|c| c.3 & 0x0100 != 0), "every choice needs ⌘, so plain typing is never caught");
    }
}
