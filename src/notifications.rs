//! Local alerts use the installed app's identity and bundled icon.

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, ClassType};
use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::sync::atomic::{AtomicU64, Ordering};

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
            crate::statusbar::TOGGLE_PANEL.store(true, Ordering::SeqCst);
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

pub fn send(message: &str) {
    if !is_app_bundle() {
        return;
    }
    let message = message.to_owned();
    let completion = RcBlock::new(move |granted: Bool, error: *mut NSError| {
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
    fn unbundled_binary_does_not_request_notifications() {
        assert!(!is_app_bundle());
        initialize();
        send("This test must not prompt or send an alert.");
    }
}
