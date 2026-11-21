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

/// Take the window off screen without closing it. Hiding the app alone is
/// not enough: macOS unhides a hidden app, with every window it had, as soon
/// as any of its windows is ordered in, so the menu-bar panel would bring the
/// dashboard back with it.
pub fn order_out(window: &Window) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // The borrowed GPUI window owns this AppKit view for the duration of this call.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    if let Some(native_window) = view.window() {
        native_window.orderOut(None);
    }
}

/// Whether macOS has "Reduce transparency" on; the see-through settings give
/// way to it.
pub fn reduce_transparency() -> bool {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

/// The window background that goes with `see_through`. A see-through window
/// is transparent to GPUI, and macOS's own frosted glass (`set_glass`) sits
/// behind its content; GPUI's built-in blur strips the glass of its tint, so a
/// dark window over a light desktop turned muddy grey.
pub fn background(see_through: bool) -> WindowBackgroundAppearance {
    if see_through {
        WindowBackgroundAppearance::Transparent
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// Let the window's content show through the UI kit's root, or not. The kit
/// paints its theme background (opaque) under every window it opens, so with
/// that in place nothing behind the window could ever show through. Deferred:
/// the root is being drawn when the dashboard notices the setting changed.
pub fn clear_root_background(window: &Window, cx: &mut gpui_kit::App, see_through: bool) {
    window.defer(cx, move |window, cx| {
        if window.root::<gpui_kit::base::Root>().flatten().is_none() {
            return;
        }
        gpui_kit::base::Root::update(window, cx, |root, _, cx| {
            use gpui_kit::Styled as _;
            root.style().background = see_through.then(|| gpui_kit::transparent_black().into());
            cx.notify();
        });
    });
}

/// Which macOS glass sits behind a see-through window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glass {
    /// The window's own ground, as behind a Finder window.
    Window,
    /// The sidebar material, for "Sidebar only".
    Sidebar,
    /// The menu-bar panel, like other menu-bar popovers.
    Popover,
}

/// The identifier of the glass view Tidegauge adds, so it is found again.
const GLASS_ID: &str = "no.ogard.tidegauge.glass";

/// Put macOS's frosted glass behind the window's content, or take it away
/// with `None`. The glass follows the window's appearance, so Light and Dark
/// in Settings tint it too.
pub fn set_glass(window: &Window, glass: Option<Glass>) {
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
        NSUserInterfaceItemIdentification, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use objc2_foundation::NSString;
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // The borrowed GPUI window owns this AppKit view for the duration of this call.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let Some(native_window) = view.window() else { return };
    let Some(content) = native_window.contentView() else { return };
    let id = NSString::from_str(GLASS_ID);
    let existing = content
        .subviews()
        .iter()
        .find(|sub| sub.identifier().is_some_and(|ident| *ident == *id));
    let Some(glass) = glass else {
        if let Some(existing) = existing {
            existing.removeFromSuperview();
        }
        return;
    };
    let material = match glass {
        Glass::Window => NSVisualEffectMaterial::UnderWindowBackground,
        Glass::Sidebar => NSVisualEffectMaterial::Sidebar,
        Glass::Popover => NSVisualEffectMaterial::Popover,
    };
    if let Some(existing) = existing {
        if let Some(effect) = existing.downcast_ref::<NSVisualEffectView>() {
            if effect.material() != material {
                effect.setMaterial(material);
            }
        }
        return;
    }
    let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(content.mtm()), content.bounds());
    effect.setMaterial(material);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    effect.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    effect.setIdentifier(Some(&id));
    content.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, None);
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
