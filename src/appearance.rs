use gpui_kit::{Window, WindowBackgroundAppearance};
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

/// Whether macOS has "Reduce transparency" on; the see-through settings give
/// way to it.
pub fn reduce_transparency() -> bool {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

/// The window background that goes with `see_through`: frosted glass, or plain.
pub fn background(see_through: bool) -> WindowBackgroundAppearance {
    if see_through {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// Clip the window's content, blur included, to rounded corners. The menu-bar
/// panel has no frame of its own, so its blur would show square corners.
pub fn round_corners(window: &Window, radius: f64) {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // The borrowed GPUI window owns this AppKit view for the duration of this call.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let Some(native_window) = view.window() else { return };
    let Some(content) = native_window.contentView() else { return };
    content.setWantsLayer(true);
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![&*content, layer];
        if let Some(layer) = layer {
            let _: () = msg_send![&*layer, setCornerRadius: radius];
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
}
