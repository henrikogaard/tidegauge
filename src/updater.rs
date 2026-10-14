//! Optional native update checks for packaged application bundles.

#![cfg(target_os = "macos")]

use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::msg_send;
use objc2_foundation::{NSBundle, NSObject, NSString};
use std::cell::RefCell;

const CHECK_FOR_UPDATES: &str = "Check for Updates…";
const CHECK_FOR_UPDATES_NO: &str = "Se etter oppdateringer…";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BundleMode {
    Preview,
    Production,
    Qa,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Availability {
    Available,
    Disabled { reason: String },
    Unavailable { reason: String },
}

struct UpdaterState {
    availability: Availability,
    controller: Option<Retained<NSObject>>,
    _framework: Option<Retained<NSBundle>>,
}

thread_local! {
    static STATE: RefCell<Option<UpdaterState>> = const { RefCell::new(None) };
}

/// Initialise Sparkle after AppKit has been created, retaining it for the
/// application lifetime on the main thread.
pub fn initialize() {
    STATE.with(|state| {
        state.replace(Some(UpdaterState::initialize()));
    });
}

/// Return the current updater availability for UI status and diagnostics.
pub fn availability() -> Availability {
    STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map(|updater| updater.availability.clone())
            .unwrap_or_else(not_initialized)
    })
}

/// Request a user-initiated update check through Sparkle.
pub fn request_check() -> Result<(), Availability> {
    STATE.with(|state| {
        let borrowed = state.borrow();
        let Some(updater) = borrowed.as_ref() else {
            return Err(not_initialized());
        };
        let Some(controller) = updater.controller.as_ref() else {
            return Err(updater.availability.clone());
        };
        unsafe {
            let _: () = msg_send![controller, checkForUpdates: Option::<&NSObject>::None];
        }
        Ok(())
    })
}

/// Return the localized title used by the native menu item.
pub fn check_for_updates_title() -> &'static str {
    crate::locale::text(CHECK_FOR_UPDATES, CHECK_FOR_UPDATES_NO)
}

/// Return the retained Sparkle controller for native target/action wiring.
pub fn menu_target() -> Option<Retained<NSObject>> {
    STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .and_then(|updater| updater.controller.clone())
    })
}

impl UpdaterState {
    fn initialize() -> Self {
        let bundle = NSBundle::mainBundle();
        match mode(&bundle) {
            Some(BundleMode::Preview) => Self {
                availability: Availability::Disabled {
                    reason: crate::locale::text(
                        "Updater is disabled in preview bundles.",
                        "Oppdatering er deaktivert i forhåndsvisninger.",
                    ).to_string(),
                },
                controller: None,
                _framework: None,
            },
            Some(BundleMode::Production) | Some(BundleMode::Qa) => match load(&bundle) {
                Ok((framework, controller)) => Self {
                    availability: Availability::Available,
                    controller: Some(controller),
                    _framework: Some(framework),
                },
                Err(reason) => Self {
                    availability: Availability::Unavailable { reason },
                    controller: None,
                    _framework: None,
                },
            },
            _ => Self {
                availability: Availability::Unavailable {
                    reason: crate::locale::text(
                        "Sparkle is available only in a packaged app bundle.",
                        "Sparkle er bare tilgjengelig i en pakket app.",
                    ).to_string(),
                },
                controller: None,
                _framework: None,
            },
        }
    }
}

fn mode(bundle: &NSBundle) -> Option<BundleMode> {
    let key = NSString::from_str("TidegaugeUpdaterMode");
    bundle
        .objectForInfoDictionaryKey(&key)
        .and_then(|value| value.downcast::<NSString>().ok())
        .and_then(|value| mode_name(&value.to_string()))
}

fn mode_name(value: &str) -> Option<BundleMode> {
    match value {
        "preview" => Some(BundleMode::Preview),
        "production" => Some(BundleMode::Production),
        "qa" => Some(BundleMode::Qa),
        _ => None,
    }
}

fn load(bundle: &NSBundle) -> Result<(Retained<NSBundle>, Retained<NSObject>), String> {
    let framework_path = bundle
        .bundlePath()
        .stringByAppendingString(&NSString::from_str(
            "/Contents/Frameworks/Sparkle.framework",
        ));
    let framework = NSBundle::bundleWithPath(&framework_path)
        .ok_or_else(|| crate::locale::text("Sparkle.framework is missing from the app bundle.", "Sparkle.framework mangler i apppakken.").to_string())?;
    unsafe {
        framework
            .loadAndReturnError()
            .map_err(|_| crate::locale::text("Sparkle.framework could not be loaded.", "Sparkle.framework kunne ikke lastes.").to_string())?;
    }
    let class = AnyClass::get(c"SPUStandardUpdaterController").ok_or_else(|| {
        crate::locale::text(
            "Sparkle updater controller is unavailable.",
            "Sparkle-oppdateringskontrolleren er ikke tilgjengelig.",
        ).to_string()
    })?;
    let allocated: *mut NSObject = unsafe { msg_send![class, alloc] };
    if allocated.is_null() {
        return Err(crate::locale::text(
            "Sparkle updater could not be allocated.",
            "Sparkle-oppdateringen kunne ikke allokeres.",
        ).to_string());
    }
    let controller: *mut NSObject = unsafe {
        msg_send![
            allocated,
            initWithStartingUpdater: true,
            updaterDelegate: Option::<&NSObject>::None,
            userDriverDelegate: Option::<&NSObject>::None
        ]
    };
    let controller = unsafe { Retained::from_raw(controller) }.ok_or_else(|| {
        crate::locale::text(
            "Sparkle updater could not be initialised.",
            "Sparkle-oppdateringen kunne ikke startes.",
        ).to_string()
    })?;
    Ok((framework, controller))
}

fn not_initialized() -> Availability {
    Availability::Unavailable {
        reason: crate::locale::text(
            "Updater has not been initialised.",
            "Oppdateringen er ikke initialisert.",
        ).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_modes_are_not_available() {
        assert_eq!(mode_name("development"), None);
        assert_eq!(mode_name("preview"), Some(BundleMode::Preview));
    }
}
