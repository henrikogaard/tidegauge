//! Local alerts use the installed app's identity and bundled icon.

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, ClassType};
use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TidegaugeNotificationDelegate"]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }

        // Clicking an alert brings Tidegauge's window forward; the UI pump
        // picks the flag up on the main thread.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            _response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            crate::statusbar::raise(&crate::statusbar::TOGGLE_PANEL);
            completion.call(());
        }
    }
);

thread_local! {
    // The notification center holds only a weak reference to its delegate.
    static DELEGATE: Retained<NotificationDelegate> = unsafe {
        msg_send![NotificationDelegate::class(), new]
    };
}

fn is_app_bundle() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle
        .bundleIdentifier()
        .is_some_and(|id| id.to_string() == "no.ogard.tidegauge")
        && bundle.bundlePath().to_string().ends_with(".app")
}

pub fn initialize() {
    // UserNotifications throws an exception for an unbundled command-line binary.
    if !is_app_bundle() {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    DELEGATE.with(|delegate| {
        center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
    });

    #[cfg(debug_assertions)]
    if std::env::var("TIDEGAUGE_NOTIFICATION_PREVIEW").as_deref() == Ok("1") {
        send(crate::locale::text(
            "This is a test alert from Tidegauge.",
            "Dette er et testvarsel fra Tidegauge.",
        ));
    }
}

fn request(message: &str) -> Retained<UNNotificationRequest> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str("Tidegauge"));
    content.setBody(&NSString::from_str(message));
    let identifier = NSString::from_str(&format!(
        "tidegauge-{}-{}-{}",
        std::process::id(),
        crate::metrics::now_unix(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None)
}

/// Where macOS notification permission stands for Tidegauge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// Not read yet.
    Checking,
    /// Not running as the installed app, which cannot send notifications.
    Unavailable,
    NotAsked,
    Allowed,
    /// Allowed, but the alert style is None in System Settings, so nothing
    /// shows on screen.
    NoBanners,
    /// Turned off in System Settings, or declined at the prompt.
    Blocked,
}

impl Permission {
    /// From `UNAuthorizationStatus`: 0 not determined, 1 denied, 2 authorized,
    /// 3 provisional, 4 ephemeral. Anything newer is read as not asked.
    pub fn from_status(status: isize) -> Permission {
        match status {
            1 => Permission::Blocked,
            2..=4 => Permission::Allowed,
            _ => Permission::NotAsked,
        }
    }

    /// From the authorization status and `alertSetting` (0 not supported,
    /// 1 disabled, 2 enabled): permission without alerts shows nothing.
    pub fn from_settings(status: isize, alert_setting: isize) -> Permission {
        match Permission::from_status(status) {
            Permission::Allowed if alert_setting == 1 => Permission::NoBanners,
            other => other,
        }
    }

    fn code(self) -> u8 {
        self as u8
    }

    fn from_code(code: u8) -> Permission {
        [
            Permission::Checking,
            Permission::Unavailable,
            Permission::NotAsked,
            Permission::Allowed,
            Permission::NoBanners,
            Permission::Blocked,
        ]
        .get(code as usize)
        .copied()
        .unwrap_or(Permission::Checking)
    }
}

static PERMISSION: AtomicU8 = AtomicU8::new(0);
static LAST_CHECK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// The permission as last read; `refresh_permission` keeps it current.
pub fn permission() -> Permission {
    Permission::from_code(PERMISSION.load(Ordering::Relaxed))
}

/// Read the permission again, at most once a second unless `force`d. The
/// answer arrives on another thread and shows on the next draw.
pub fn refresh_permission(force: bool) {
    if !is_app_bundle() {
        PERMISSION.store(Permission::Unavailable.code(), Ordering::Relaxed);
        return;
    }
    {
        let mut last = LAST_CHECK.lock().unwrap();
        if !force && last.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    let completion = RcBlock::new(|settings: std::ptr::NonNull<UNNotificationSettings>| {
        let settings = unsafe { settings.as_ref() };
        let permission =
            Permission::from_settings(settings.authorizationStatus().0, settings.alertSetting().0);
        PERMISSION.store(permission.code(), Ordering::Relaxed);
        crate::app::PUMP.notify();
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .getNotificationSettingsWithCompletionHandler(&completion);
}

/// Show macOS's permission prompt, if it has not been answered.
pub fn request_permission() {
    if !is_app_bundle() {
        return;
    }
    let completion = RcBlock::new(|_granted: Bool, _error: *mut NSError| refresh_permission(true));
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &completion,
        );
}

/// Where macOS keeps notification permissions.
const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

pub fn open_system_settings() {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSURL;
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(SETTINGS_URL)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// A real alert through the normal path, asking for permission if needed.
pub fn send_test() {
    send(crate::locale::text(
        "This is a test alert from Tidegauge.",
        "Dette er et testvarsel fra Tidegauge.",
    ));
}

pub fn send(message: &str) {
    if !is_app_bundle() {
        return;
    }
    let message = message.to_owned();
    let completion = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        refresh_permission(true);
        if !granted.as_bool() || !error.is_null() {
            return;
        }
        let request = request(&message);
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &completion,
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_alert_preserves_body_and_app_title() {
        let body = "Høy CPU-bruk: 99% — \"test\"\nNy linje";
        let first = request(body);
        let second = request(body);
        assert_eq!(first.content().title().to_string(), "Tidegauge");
        assert_eq!(first.content().body().to_string(), body);
        assert!(first.trigger().is_none());
        assert_ne!(first.identifier(), second.identifier());
    }

    #[test]
    fn authorization_status_maps_to_a_permission() {
        assert_eq!(Permission::from_status(0), Permission::NotAsked);
        assert_eq!(Permission::from_status(1), Permission::Blocked);
        for allowed in 2..=4 {
            assert_eq!(Permission::from_status(allowed), Permission::Allowed);
        }
        assert_eq!(Permission::from_status(99), Permission::NotAsked);
        assert_eq!(Permission::from_settings(2, 2), Permission::Allowed);
        assert_eq!(
            Permission::from_settings(2, 1),
            Permission::NoBanners,
            "authorized with alerts off"
        );
        assert_eq!(Permission::from_settings(3, 1), Permission::NoBanners);
        assert_eq!(
            Permission::from_settings(2, 0),
            Permission::Allowed,
            "unsupported is not off"
        );
        assert_eq!(Permission::from_settings(1, 1), Permission::Blocked);
        assert_eq!(Permission::from_settings(0, 1), Permission::NotAsked);
        for p in [
            Permission::Checking,
            Permission::Unavailable,
            Permission::NotAsked,
            Permission::Allowed,
            Permission::Blocked,
            Permission::NoBanners,
        ] {
            assert_eq!(Permission::from_code(p.code()), p);
        }
    }

    #[test]
    fn unbundled_binary_does_not_request_notifications() {
        assert!(!is_app_bundle());
        initialize();
        send("This test must not prompt or send an alert.");
    }
}
