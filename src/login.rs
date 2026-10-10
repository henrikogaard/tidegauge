//! Native launch-at-login state and user-initiated mutations.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Unsupported,
    Unavailable,
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoginStatus {
    state: State,
}

impl LoginStatus {
    pub fn is_registered(self) -> bool {
        matches!(self.state, State::Enabled | State::RequiresApproval)
    }

    pub fn can_change(self) -> bool {
        matches!(
            self.state,
            State::NotRegistered | State::NotFound | State::Enabled | State::RequiresApproval
        )
    }

    pub fn control_label(self) -> &'static str {
        match self.state {
            State::Enabled => crate::locale::text("On", "På"),
            State::RequiresApproval => crate::locale::text("Pending", "Venter"),
            State::NotRegistered | State::NotFound => crate::locale::text("Off", "Av"),
            _ => crate::locale::text("Unavailable", "Utilgjengelig"),
        }
    }

    pub fn message(self) -> &'static str {
        match self.state {
            State::Enabled => crate::locale::text(
                "Launch at login is enabled.",
                "Start ved pålogging er aktivert.",
            ),
            State::RequiresApproval => crate::locale::text(
                "Approve Tidegauge in System Settings > General > Login Items before it can start.",
                "Godkjenn Tidegauge i Systeminnstillinger > Generelt > Påloggingsobjekter før appen kan starte.",
            ),
            State::NotRegistered => crate::locale::text(
                "Launch at login is off.",
                "Start ved pålogging er av.",
            ),
            State::Unavailable => crate::locale::text(
                "Install Tidegauge in /Applications or ~/Applications to manage launch at login.",
                "Installer Tidegauge i /Applications eller ~/Applications for å styre start ved pålogging.",
            ),
            State::Unsupported => crate::locale::text(
                "Launch at login requires macOS 13 or later.",
                "Start ved pålogging krever macOS 13 eller nyere.",
            ),
            State::NotFound => crate::locale::text(
                "Not registered with macOS. Enable to add Tidegauge as a login item.",
                "Ikke registrert hos macOS. Aktiver for å legge til Tidegauge som påloggingsobjekt.",
            ),
        }
    }
}

pub fn status() -> LoginStatus {
    LoginStatus {
        state: native_state(),
    }
}

pub fn set_enabled(enabled: bool) -> Result<LoginStatus, &'static str> {
    set_enabled_using(enabled, status, native_set_enabled)
}

fn set_enabled_using(
    enabled: bool,
    mut read_status: impl FnMut() -> LoginStatus,
    mut apply: impl FnMut(bool) -> Result<(), ()>,
) -> Result<LoginStatus, &'static str> {
    let current = read_status();
    if !current.can_change() {
        return Err(current.message());
    }
    if current.is_registered() == enabled {
        return Ok(current);
    }

    if apply(enabled).is_err() {
        return Err(crate::locale::text(
            "Tidegauge could not update launch at login. Check Login Items in System Settings.",
            "Tidegauge kunne ikke endre start ved pålogging. Sjekk Påloggingsobjekter i Systeminnstillinger.",
        ));
    }

    let updated = read_status();
    if !updated.can_change() || updated.is_registered() != enabled {
        return Err(crate::locale::text(
            "The launch-at-login setting did not reach the requested state. Check Login Items in System Settings.",
            "Start ved pålogging fikk ikke ønsket tilstand. Sjekk Påloggingsobjekter i Systeminnstillinger.",
        ));
    }
    Ok(updated)
}

#[cfg(target_os = "macos")]
fn native_state() -> State {
    let Some(bundle_path) = main_bundle_path() else {
        return State::Unavailable;
    };
    if !installed_bundle_path(&bundle_path) {
        return State::Unavailable;
    }

    let Some(service) = service() else {
        return State::Unsupported;
    };
    let status: isize = unsafe { objc2::msg_send![&*service, status] };
    match status {
        0 => State::NotRegistered,
        1 => State::Enabled,
        2 => State::RequiresApproval,
        3 => State::NotFound,
        _ => State::Unavailable,
    }
}

#[cfg(not(target_os = "macos"))]
fn native_state() -> State {
    State::Unsupported
}

#[cfg(target_os = "macos")]
fn native_set_enabled(enabled: bool) -> Result<(), ()> {
    let Some(service) = service() else {
        return Err(());
    };
    let mut error: *mut objc2::runtime::AnyObject = std::ptr::null_mut();
    let success: bool = unsafe {
        if enabled {
            objc2::msg_send![&*service, registerAndReturnError: &mut error]
        } else {
            objc2::msg_send![&*service, unregisterAndReturnError: &mut error]
        }
    };
    if success {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(not(target_os = "macos"))]
fn native_set_enabled(_: bool) -> Result<(), ()> {
    Err(())
}

#[cfg(target_os = "macos")]
fn service() -> Option<objc2::rc::Retained<objc2_foundation::NSObject>> {
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSObject;

    let class = AnyClass::get(c"SMAppService")?;
    let service: Option<objc2::rc::Retained<NSObject>> =
        unsafe { objc2::msg_send![class, mainAppService] };
    service
}

#[cfg(target_os = "macos")]
fn main_bundle_path() -> Option<PathBuf> {
    Some(
        objc2_foundation::NSBundle::mainBundle()
            .bundlePath()
            .to_string()
            .into(),
    )
}

#[cfg(target_os = "macos")]
#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

fn installed_bundle_path(bundle: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let user_applications = Path::new(&home).join("Applications");
    canonical_bundle_path_is_eligible(
        bundle,
        &[Path::new("/Applications"), user_applications.as_path()],
    )
}

fn canonical_bundle_path_is_eligible(bundle: &Path, roots: &[&Path]) -> bool {
    let Ok(bundle) = std::fs::canonicalize(bundle) else {
        return false;
    };
    let roots: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect();
    bundle_path_is_eligible(&bundle, &roots)
}

fn bundle_path_is_eligible(bundle: &Path, roots: &[PathBuf]) -> bool {
    bundle.extension().is_some_and(|extension| extension == "app")
        && roots.iter().any(|root| bundle.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn registered_states_are_truthful_and_changeable() {
        for state in [State::Enabled, State::RequiresApproval] {
            let status = LoginStatus { state };
            assert!(status.is_registered());
            assert!(status.can_change());
        }
    }

    #[test]
    fn unavailable_states_cannot_change() {
        for state in [State::Unsupported, State::Unavailable] {
            let status = LoginStatus { state };
            assert!(!status.is_registered());
            assert!(!status.can_change());
        }
    }

    #[test]
    fn installed_path_requires_component_boundary_and_app_extension() {
        let applications = Path::new("/Applications");
        let user_applications = Path::new("/Users/test/Applications");
        assert!(bundle_path_is_eligible(
            Path::new("/Applications/Tidegauge.app"),
            &[applications.to_path_buf(), user_applications.to_path_buf()]
        ));
        assert!(bundle_path_is_eligible(
            Path::new("/Users/test/Applications/Tidegauge.app"),
            &[applications.to_path_buf(), user_applications.to_path_buf()]
        ));
        assert!(!bundle_path_is_eligible(
            Path::new("/Applications-old/Tidegauge.app"),
            &[applications.to_path_buf(), user_applications.to_path_buf()]
        ));
        assert!(!bundle_path_is_eligible(
            Path::new("/Volumes/Tidegauge/Tidegauge.app"),
            &[applications.to_path_buf(), user_applications.to_path_buf()]
        ));
        assert!(!bundle_path_is_eligible(
            Path::new("/Applications/Tidegauge"),
            &[applications.to_path_buf(), user_applications.to_path_buf()]
        ));
    }

    #[test]
    fn canonical_path_rejects_a_symlink_outside_allowed_roots() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tidegauge-login-path-{}-{nonce}",
            std::process::id()
        ));
        let applications = root.join("Applications");
        let outside = root.join("outside");
        let installed = applications.join("Tidegauge.app");
        let external = outside.join("Tidegauge.app");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        let link = applications.join("Linked.app");
        symlink(&external, &link).unwrap();

        assert!(canonical_bundle_path_is_eligible(
            &installed,
            &[applications.as_path()]
        ));
        assert!(!canonical_bundle_path_is_eligible(
            &link,
            &[applications.as_path()]
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_requests_are_idempotent_and_native_failures_are_localized() {
        let mut state = State::Enabled;
        let mut mutations = 0;
        let unchanged = set_enabled_using(
            true,
            || LoginStatus { state },
            |_| {
                mutations += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(unchanged.state, State::Enabled);
        assert_eq!(mutations, 0);

        state = State::NotRegistered;
        let unchanged = set_enabled_using(
            false,
            || LoginStatus { state },
            |_| {
                mutations += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(unchanged.state, State::NotRegistered);
        assert_eq!(mutations, 0);

        let failed = set_enabled_using(
            true,
            || LoginStatus {
                state: State::NotRegistered,
            },
            |_| Err(()),
        );
        assert!(failed.is_err());
        let error = failed.unwrap_err();
        assert!(
            error.contains("System Settings") || error.contains("Systeminnstillinger")
        );
    }

    #[test]
    fn post_mutation_state_must_match_requested_registration() {
        for (enabled, before, after) in [
            (true, State::NotRegistered, State::NotRegistered),
            (false, State::Enabled, State::Unavailable),
        ] {
            let mut states = [before, after].into_iter();
            let result = set_enabled_using(
                enabled,
                || LoginStatus { state: states.next().unwrap() },
                |_| Ok(()),
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn missing_main_app_record_can_be_registered_explicitly() {
        let initial = LoginStatus { state: State::NotFound };
        assert!(initial.can_change());
        assert!(!initial.is_registered());
        assert_eq!(initial.control_label(), crate::locale::text("Off", "Av"));
        let mut states = [State::NotFound, State::Enabled].into_iter();
        let mut mutations = 0;
        let updated = set_enabled_using(
            true,
            || LoginStatus { state: states.next().unwrap() },
            |enabled| {
                assert!(enabled);
                mutations += 1;
                Ok(())
            },
        ).unwrap();
        assert!(updated.is_registered());
        assert_eq!(mutations, 1);
    }

    #[test]
    fn approval_pending_is_a_registered_post_mutation_state() {
        let mut reads = 0;
        let result = set_enabled_using(
            true,
            || {
                reads += 1;
                LoginStatus {
                    state: if reads == 1 {
                        State::NotRegistered
                    } else {
                        State::RequiresApproval
                    },
                }
            },
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(result.state, State::RequiresApproval);
    }

    #[test]
    fn pending_registration_has_distinct_control_label() {
        assert_eq!(
            LoginStatus {
                state: State::RequiresApproval
            }
            .control_label(),
            crate::locale::text("Pending", "Venter")
        );
    }
}
