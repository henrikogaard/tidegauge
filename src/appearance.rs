use gpui_kit::Window;
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
