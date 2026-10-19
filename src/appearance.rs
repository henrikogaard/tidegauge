use gpui_kit::{Window, WindowBackgroundAppearance};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor,
    NSColorSpace, NSView,
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

/// Whether the window can be watched: shown, on the active Space, and not
/// fully covered (macOS's occlusion state also drops minimized windows and a
/// hidden app).
pub fn is_visible(window: &Window) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return true };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return true };
    // The borrowed GPUI window owns this AppKit view for the duration of this call.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    view.window().is_none_or(|window| {
        window.isVisible()
            && window.isOnActiveSpace()
            && window.occlusionState().contains(objc2_app_kit::NSWindowOcclusionState::Visible)
    })
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

/// The accent colour chosen in System Settings, as 0xRRGGBB in sRGB.
pub fn system_accent() -> Option<u32> {
    let color =
        NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let (mut r, mut g, mut b, mut a) = (0., 0., 0., 0.);
    // The four pointers are locals that live for the call.
    unsafe { color.getRed_green_blue_alpha(&mut r, &mut g, &mut b, &mut a) };
    let byte = |v: f64| (v.clamp(0., 1.) * 255.).round() as u32;
    Some(byte(r) << 16 | byte(g) << 8 | byte(b))
}
