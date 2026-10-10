use gpui_kit::Window;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSView,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Override only the dashboard, leaving the menu bar's native appearance alone.
pub fn apply(window: &Window, dark: Option<bool>) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // The borrowed GPUI window owns this AppKit view for the duration of this call.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    if let Some(native_window) = view.window() {
        let appearance = dark.and_then(|dark| unsafe {
            NSAppearance::appearanceNamed(if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua })
        });
        native_window.setAppearance(appearance.as_deref());
    }
}
