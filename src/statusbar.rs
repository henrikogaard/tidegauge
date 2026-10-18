//! One compact native menu-bar item, with optional labeled metric readings.

#![allow(dead_code)]
#![cfg(target_os = "macos")]

use crate::engine::Snapshot;
use crate::locale::text as l;
use crate::menu_style::{self, Tint};
#[cfg(test)]
use crate::settings::MenuItem;
use crate::settings::{localized_metric_label, style_fits, Settings, MENU_METRICS, MENU_STYLES};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, AllocAnyThread, ClassType};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSPoint, NSString};
use std::sync::atomic::{AtomicBool, Ordering};

pub static TOGGLE_PANEL: AtomicBool = AtomicBool::new(false);
pub static SHOW_SETTINGS: AtomicBool = AtomicBool::new(false);
pub static QUIT_APP: AtomicBool = AtomicBool::new(false);
pub static SHOW_CONTEXT_MENU: AtomicBool = AtomicBool::new(false);
/// A left click on the item: open or close the menu-bar panel.
pub static TOGGLE_POPOVER: AtomicBool = AtomicBool::new(false);

/// A right or Control click on the item: show the native menu.
pub static SHOW_ITEM_MENU: AtomicBool = AtomicBool::new(false);
/// A click somewhere else on screen: close the panel.
pub static CLICKED_OUTSIDE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ITEM: std::cell::RefCell<Option<(Retained<NSStatusItem>, Retained<TideGaugeTarget>)>> =
        const { std::cell::RefCell::new(None) };
    static OUTSIDE_MONITOR: std::cell::RefCell<Option<Retained<objc2::runtime::AnyObject>>> =
        const { std::cell::RefCell::new(None) };
}

/// Where the panel goes: the display the item is on, and the item's place
/// on it in points from the display's top-left corner.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub display: u64,
    pub left: f64,
    pub width: f64,
    pub bottom: f64,
    pub display_width: f64,
}

/// Where the last click on the item landed, in Cocoa screen points.
static CLICK_AT: std::sync::Mutex<Option<(f64, f64)>> = std::sync::Mutex::new(None);
/// Where the last click outside the app landed.
static OUTSIDE_AT: std::sync::Mutex<Option<(f64, f64)>> = std::sync::Mutex::new(None);

/// Whether two clicks hit the same place: the item, give or take its width.
fn same_spot(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= 60. && (a.1 - b.1).abs() <= 30.
}

/// Newer macOS hosts status items in another process, so pressing the item
/// also reaches the "click in another app" monitor. Such a press is the
/// item's own click, not a reason to close the panel.
pub fn outside_click_was_on_item() -> bool {
    let outside = *OUTSIDE_AT.lock().unwrap();
    let item = *CLICK_AT.lock().unwrap();
    matches!((outside, item), (Some(o), Some(i)) if same_spot(o, i))
}

/// A display as placement needs it: its number, its frame (x, y, width,
/// height, Cocoa points) and the top of its area below the menu bar.
#[derive(Clone, Copy, Debug)]
struct Display {
    number: u64,
    frame: (f64, f64, f64, f64),
    visible_top: f64,
}

/// Place the panel under a click on the item. The item's own window can't
/// be trusted for this: newer macOS draws status items in another process,
/// so the click point is the one position that is always real.
fn place(click: (f64, f64), displays: &[Display]) -> Option<Anchor> {
    let (x, y) = click;
    let display = displays.iter().find(|d| {
        let (dx, dy, w, h) = d.frame;
        x >= dx && x <= dx + w && y >= dy && y <= dy + h
    })?;
    let (dx, dy, w, h) = display.frame;
    // The menu bar's height: the gap between the display's top and the top
    // of its usable area. A hidden menu bar leaves none; keep a usual gap.
    let menu_bar = (dy + h - display.visible_top).max(24.);
    Some(Anchor {
        display: display.number,
        left: x - dx - 10.,
        width: 20.,
        bottom: menu_bar,
        display_width: w,
    })
}

/// The panel's place under the last click on the item.
pub fn anchor() -> Option<Anchor> {
    let click = (*CLICK_AT.lock().unwrap())?;
    let displays: Vec<Display> = NSScreen::screens(mtm())
        .iter()
        .filter_map(|screen| {
            let number = screen
                .deviceDescription()
                .objectForKey(&NSString::from_str("NSScreenNumber"))?
                .downcast::<objc2_foundation::NSNumber>()
                .ok()?
                .unsignedLongLongValue();
            let frame = screen.frame();
            let visible = screen.visibleFrame();
            Some(Display {
                number,
                frame: (frame.origin.x, frame.origin.y, frame.size.width, frame.size.height),
                visible_top: visible.origin.y + visible.size.height,
            })
        })
        .collect();
    place(click, &displays)
}

/// The shortcut acts like a click on the item. The panel opens where the
/// item was last clicked; before any click, under the item's own window if
/// macOS reports one on a screen, or else at the right of the main menu bar.
pub fn toggle_from_keyboard() {
    let mut click = CLICK_AT.lock().unwrap();
    if click.is_none() {
        let item = ITEM.with(|item| item.borrow().as_ref().map(|(item, _)| item.clone()));
        let frame = item
            .and_then(|item| item.button(mtm()))
            .and_then(|button| button.window())
            .map(|window| window.frame())
            .filter(|f| f.size.width > 0. && NSScreen::screens(mtm()).iter().any(|s| {
                let r = s.frame();
                f.origin.x >= r.origin.x && f.origin.x <= r.origin.x + r.size.width
            }));
        *click = match frame {
            Some(f) => Some((f.origin.x + f.size.width / 2., f.origin.y + f.size.height / 2.)),
            None => NSScreen::mainScreen(mtm()).map(|s| {
                let r = s.frame();
                (r.origin.x + r.size.width - 200., r.origin.y + r.size.height - 10.)
            }),
        };
    }
    TOGGLE_POPOVER.store(true, Ordering::SeqCst);
}

/// Show the native menu from the item, as a right click would.
pub fn show_item_menu() {
    let target = ITEM.with(|item| item.borrow().as_ref().map(|(_, target)| target.clone()));
    let Some(target) = target else {
        return;
    };
    // At the click, in screen points: like the panel, the menu doesn't rely
    // on the item's own window, which newer macOS hosts elsewhere.
    let (x, y) = (*CLICK_AT.lock().unwrap()).unwrap_or_else(|| {
        let at = NSEvent::mouseLocation();
        (at.x, at.y)
    });
    context_menu(&target).popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(x, y - 4.), None);
}

thread_local! {
    static LOGO: Option<Retained<NSImage>> = {
        let data = objc2_foundation::NSData::with_bytes(include_bytes!("../assets/menu-bar-icon.svg"));
        NSImage::initWithData(NSImage::alloc(), &data)
    };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TideGaugeTarget"]
    struct TideGaugeTarget;
    impl TideGaugeTarget {
        #[unsafe(method(settings:))]
        fn _settings(&self, _sender: Option<&NSObject>) {
            SHOW_SETTINGS.store(true, Ordering::SeqCst);
        }
        #[unsafe(method(toggle:))]
        fn _toggle(&self, _sender: Option<&NSObject>) {
            TOGGLE_PANEL.store(true, Ordering::SeqCst);
        }
        #[unsafe(method(quit:))]
        fn _quit(&self, _sender: Option<&NSObject>) {
            QUIT_APP.store(true, Ordering::SeqCst);
        }
        #[unsafe(method(clicked:))]
        fn _clicked(&self, _sender: Option<&NSObject>) {
            let event = NSApplication::sharedApplication(mtm()).currentEvent();
            let secondary = event.as_ref().is_some_and(|event| {
                event.r#type() == NSEventType::RightMouseUp
                    || event.modifierFlags().contains(NSEventModifierFlags::Control)
            });
            let at = NSEvent::mouseLocation();
            *CLICK_AT.lock().unwrap() = Some((at.x, at.y));
            if secondary {
                SHOW_ITEM_MENU.store(true, Ordering::SeqCst);
            } else {
                TOGGLE_POPOVER.store(true, Ordering::SeqCst);
            }
        }
    }
);

fn mtm() -> MainThreadMarker {
    unsafe { MainThreadMarker::new_unchecked() }
}

fn context_menu(target: &TideGaugeTarget) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm());
    for (title, action, key) in [
        (l("Open Dashboard", "Åpne oversikten"), sel!(toggle:), ""),
        (l("Settings…", "Innstillinger…"), sel!(settings:), ","),
        (l("Quit Tidegauge", "Avslutt Tidegauge"), sel!(quit:), "q"),
    ] {
        if key == "q" {
            menu.addItem(&NSMenuItem::separatorItem(mtm()));
        }
        let item = NSMenuItem::new(mtm());
        item.setTitle(&NSString::from_str(title));
        item.setKeyEquivalent(&NSString::from_str(key));
        unsafe {
            item.setTarget(Some(target));
            item.setAction(Some(action));
        }
        menu.addItem(&item);
    }
    let update_item = NSMenuItem::new(mtm());
    update_item.setTitle(&NSString::from_str(crate::updater::check_for_updates_title()));
    update_item.setKeyEquivalent(&NSString::from_str(""));
    let availability = crate::updater::availability();
    if let crate::updater::Availability::Disabled { reason }
    | crate::updater::Availability::Unavailable { reason } = &availability
    {
        let reason = NSString::from_str(reason);
        update_item.setToolTip(Some(&reason));
    }
    if let Some(controller) = crate::updater::menu_target() {
        unsafe {
            update_item.setTarget(Some(&controller));
            update_item.setAction(Some(sel!(checkForUpdates:)));
        }
    } else {
        update_item.setEnabled(false);
    }
    menu.insertItem_atIndex(&update_item, 2);
    menu
}

/// The same menu is available from Settings for keyboard and window users.
pub fn show_context_menu() {
    let app = NSApplication::sharedApplication(mtm());
    let Some(window) = app.keyWindow() else {
        return;
    };
    let Some(view) = window.contentView() else {
        return;
    };
    let Some(event) = app.currentEvent() else {
        return;
    };
    let target: Retained<TideGaugeTarget> = unsafe { msg_send![TideGaugeTarget::class(), new] };
    NSMenu::popUpContextMenu_withEvent_forView(&context_menu(&target), &event, &view);
}

const ITEM_HEIGHT: f64 = 16.;
const ITEM_GAP: f64 = 10.;
const GRAPH_WIDTH: f64 = 24.;
const RING_SIZE: f64 = 14.;
const GAUGE_WIDTH: f64 = 20.;
/// Room for the dash that stands in for bars before any core has reported.
const EMPTY_BARS_WIDTH: f64 = 12.;

/// A single native item keeps readings together and preserves their identity.
pub struct StatusBar {
    item: Retained<NSStatusItem>,
    target: Retained<TideGaugeTarget>,
    /// The language the menu was built in; it is rebuilt when that changes.
    norwegian: bool,
    /// What the image on the item shows now: the image is rebuilt only when
    /// this changes.
    shown: Option<Frame>,
}

/// Everything the image depends on, rounded to what it can show.
#[derive(Clone, PartialEq)]
struct Frame {
    readouts: Vec<Readout>,
    show_icon: bool,
    colored: bool,
    /// The menu bar's appearance, while colours depend on it.
    dark: bool,
    description: String,
}

impl StatusBar {
    pub fn new() -> Self {
        let target: Retained<TideGaugeTarget> = unsafe { msg_send![TideGaugeTarget::class(), new] };
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        // A left click opens the panel; a right or Control click the menu.
        if let Some(button) = item.button(mtm()) {
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(clicked:)));
            }
            button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
        }
        ITEM.with(|slot| *slot.borrow_mut() = Some((item.clone(), target.clone())));
        // Clicks in other apps close the panel, like any menu-bar popover.
        let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
            &block2::RcBlock::new(|_| {
                let at = NSEvent::mouseLocation();
                *OUTSIDE_AT.lock().unwrap() = Some((at.x, at.y));
                CLICKED_OUTSIDE.store(true, Ordering::SeqCst);
            }),
        );
        OUTSIDE_MONITOR.with(|slot| *slot.borrow_mut() = monitor);
        Self {
            item,
            target,
            norwegian: crate::locale::is_norwegian(),
            shown: None,
        }
    }

    /// Whether the menu bar draws light ink on dark. Only coloured readings
    /// care: the monochrome image is a template macOS tints itself.
    fn menu_bar_is_dark(&self) -> bool {
        use objc2_foundation::NSArray;
        let Some(button) = self.item.button(mtm()) else {
            return false;
        };
        let names = unsafe { NSArray::from_slice(&[NSAppearanceNameDarkAqua, NSAppearanceNameAqua]) };
        let appearance = button.effectiveAppearance();
        let best = appearance.bestMatchFromAppearancesWithNames(&names);
        best.is_some_and(|name| unsafe { &*name == NSAppearanceNameDarkAqua })
    }

    pub fn sync(&mut self, settings: &Settings, snap: &Snapshot) {
        // The menu is built when shown, so it always uses the current language.
        self.norwegian = crate::locale::is_norwegian();
        let colored = settings.menu_colored;
        let readouts = menu_readouts(settings, snap, measure_label_width);
        let show_icon = settings.show_app_icon || readouts.is_empty();
        let menu_hint = l(
            "Click for the panel; right-click for the menu.",
            "Klikk for panelet; høyreklikk for menyen.",
        );
        let description = if readouts.is_empty() {
            format!("Tidegauge. {menu_hint}")
        } else {
            format!(
                "Tidegauge. {}. {menu_hint}",
                readouts
                    .iter()
                    .map(|r| {
                        let level = match (r.metric, r.tint) {
                            ("memory", Some(Tint::Caution)) => l(", memory pressure warning", ", minnepress: advarsel"),
                            ("memory", Some(Tint::Critical)) => l(", memory pressure critical", ", minnepress: kritisk"),
                            (_, Some(Tint::Caution)) => l(", warning", ", advarsel"),
                            (_, Some(Tint::Critical)) => l(", critical", ", kritisk"),
                            (_, None) => "",
                        };
                        let charging = if r.charging { l(", charging", ", lader") } else { "" };
                        format!(
                            "{} {}{level}{charging}",
                            localized_metric_label(r.metric).unwrap_or(l("Metric", "Måling")),
                            r.value
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        };
        let dark = colored && self.menu_bar_is_dark();
        let frame = Frame { readouts, show_icon, colored, dark, description };
        // Most samples change nothing the image shows.
        if self.shown.as_ref() == Some(&frame) {
            return;
        }
        let description = frame.description.clone();
        let image = menu_image(&frame.readouts, show_icon, colored, dark);
        self.shown = Some(frame);
        image.setAccessibilityDescription(Some(&NSString::from_str(&description)));
        if let Some(button) = self.item.button(mtm()) {
            button.setTitle(&NSString::from_str(""));
            button.setImage(Some(&image));
            button.setImagePosition(NSCellImagePosition::ImageOnly);
            button.setToolTip(Some(&NSString::from_str(&description)));
        }
    }
}

impl Drop for StatusBar {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

#[derive(Clone, PartialEq)]
struct Readout {
    metric: &'static str,
    label: &'static str,
    label_width: f64,
    style: String,
    value: String,
    /// Only for the graph styles, and finite, so that equal drawings compare equal.
    history: Vec<f64>,
    /// Drawn in colour instead of the menu bar's own ink: memory while macOS
    /// reports memory pressure, and in coloured mode any reading at a
    /// warning level.
    tint: Option<Tint>,
    /// What a ring or gauge fills to, in whole percent.
    fill: Option<u8>,
    charging: bool,
    /// One bar height in pixels per core, and where the bars go.
    cores: Vec<u8>,
    bars: Option<menu_style::BarLayout>,
}

impl Tint {
    fn color(self) -> Retained<NSColor> {
        match self {
            Tint::Caution => NSColor::systemOrangeColor(),
            Tint::Critical => NSColor::systemRedColor(),
        }
    }
}

impl Readout {
    fn value_width(&self) -> f64 {
        match (self.style.as_str(), self.metric) {
            ("bars", _) => self.bars.as_ref().map_or(EMPTY_BARS_WIDTH, |bars| bars.width),
            ("pie", _) => RING_SIZE,
            ("gauge", _) => GAUGE_WIDTH,
            (_, "network" | "disk") => 55.,
            (_, "power") => 42.,
            _ => 31.,
        }
    }
    fn width(&self) -> f64 {
        slot_width(&self.style, self.label_width, self.value_width())
    }
}

fn slot_width(style: &str, label_width: f64, value_width: f64) -> f64 {
    match style {
        "icon" => 14.,
        "graph" => label_width + 4. + GRAPH_WIDTH,
        "both" => label_width + 4. + value_width + 6. + GRAPH_WIDTH,
        _ => label_width + 4. + value_width,
    }
}

thread_local! {
    static LABEL_WIDTHS: std::cell::RefCell<std::collections::HashMap<String, f64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn measure_label_width(text: &str) -> f64 {
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSDictionary;
    if let Some(width) = LABEL_WIDTHS.with(|widths| widths.borrow().get(text).copied()) {
        return width;
    }
    let weight = unsafe { NSFontWeightMedium };
    let font = NSFont::systemFontOfSize_weight(9., weight);
    let font_object: &AnyObject = &font;
    let width = unsafe {
        let attributes = NSDictionary::from_slices(&[NSFontAttributeName], &[font_object]);
        NSString::from_str(text)
            .sizeWithAttributes(Some(&attributes))
            .width
    };
    let width = width.ceil().max(22.);
    LABEL_WIDTHS.with(|widths| widths.borrow_mut().insert(text.to_string(), width));
    width
}

/// The style to draw: one the metric can't show (a hand-edited file, a newer
/// version's style) is drawn as a value.
fn effective_style(metric: &str, style: &str) -> String {
    let known = MENU_STYLES.iter().any(|(key, _)| *key == style);
    if known && style_fits(metric, style) { style.into() } else { "value".into() }
}

fn menu_readouts(settings: &Settings, s: &Snapshot, measure: impl Fn(&str) -> f64) -> Vec<Readout> {
    // Canonical metric order also eliminates duplicate and unsupported entries.
    MENU_METRICS
        .iter()
        .filter_map(|&(metric, _)| {
            let preference = settings
                .menu_items
                .iter()
                .find(|item| item.metric == metric)?;
            let style = effective_style(metric, &preference.style);
            let (label, mut value, history) = match metric {
                "cpu" => ("CPU", crate::engine::fmt_pct(s.cpu_pct), s.cpu_hist.pts.clone()),
                "memory" => (
                    l("MEM", "MINNE"),
                    crate::engine::fmt_pct(s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.),
                    s.mem_hist.pts.clone(),
                ),
                "gpu" => (
                    "GPU",
                    s.gpu
                        .utilization
                        .map(crate::engine::fmt_pct)
                        .unwrap_or_else(|| "—".into()),
                    if s.gpu.utilization.is_some() {
                        s.gpu_hist.pts.clone()
                    } else {
                        Vec::new()
                    },
                ),
                "disk" => (
                    l("DSK", "DISK"),
                    menu_rate(s.disk.read_rate + s.disk.write_rate),
                    s.disk_r_hist
                        .pts
                        .iter()
                        .zip(&s.disk_w_hist.pts)
                        .map(|(a, b)| a + b)
                        .collect(),
                ),
                "network" => (
                    l("NET", "NETT"),
                    menu_rate(s.net.rate_in + s.net.rate_out),
                    s.net_in_hist
                        .pts
                        .iter()
                        .zip(&s.net_out_hist.pts)
                        .map(|(a, b)| a + b)
                        .collect(),
                ),
                "battery" => (
                    l("BAT", "BATT"),
                    s.battery
                        .percent
                        .map(crate::engine::fmt_pct)
                        .unwrap_or_else(|| "—".into()),
                    if s.battery.percent.is_some() {
                        s.batt_hist.pts.clone()
                    } else {
                        Vec::new()
                    },
                ),
                "power" => (
                    l("PWR", "EFF"),
                    s.power_w.map(crate::engine::fmt_watts).unwrap_or_else(|| "—".into()),
                    s.power_hist.pts.clone(),
                ),
                _ => return None,
            };
            // What a ring or gauge fills to: free space for the disk, use for the rest.
            let fill_pct = match metric {
                "cpu" => Some(s.cpu_pct),
                "memory" => Some(s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.),
                "disk" => (s.disk.total > 0).then(|| s.disk.free as f64 / s.disk.total as f64 * 100.),
                "battery" => s.battery.percent,
                _ => None,
            }
            .filter(|_| matches!(style.as_str(), "pie" | "gauge"));
            if metric == "disk" && style == "pie" {
                value = fill_pct
                    .map(|free| format!("{} {}", crate::engine::fmt_pct(free), l("free", "ledig")))
                    .unwrap_or_else(|| "—".into());
            }
            let charging = metric == "battery" && s.battery.is_charging;
            let tint = match metric {
                "memory" => menu_style::memory_tint(s.mem.pressure),
                _ if !settings.menu_colored => None,
                "cpu" => menu_style::cpu_tint(s.cpu_pct),
                "battery" => s.battery.percent.and_then(|p| menu_style::remaining_tint(p, charging)),
                "disk" => fill_pct.and_then(|free| menu_style::remaining_tint(free, false)),
                _ => None,
            };
            let cores: Vec<u8> = if style == "bars" {
                s.core_pct.iter().map(|pct| menu_style::bar_height(*pct)).collect()
            } else {
                Vec::new()
            };
            let bars = (!cores.is_empty()).then(|| {
                let groups: Vec<usize> = s.chip.groups.iter().map(|g| g.count).collect();
                menu_style::bar_layout(&groups, cores.len())
            });
            let history = if matches!(style.as_str(), "graph" | "both") {
                history.into_iter().map(|v| if v.is_finite() { v } else { 0. }).collect()
            } else {
                Vec::new()
            };
            Some(Readout {
                metric,
                label,
                label_width: measure(label),
                style,
                value,
                history,
                tint,
                fill: fill_pct.map(menu_style::whole_pct),
                charging,
                cores,
                bars,
            })
        })
        .collect()
}

fn menu_rate(rate: f64) -> String {
    let rate = if rate.is_finite() { rate.max(0.) } else { 0. };
    let units = ["B/s", "K/s", "M/s", "G/s", "T/s"];
    let mut value = rate;
    let mut unit = 0;
    while value >= 1024. && unit < units.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit > 0 && value < 10. {
        format!("{} {}", crate::locale::number(value, 1), units[unit])
    } else {
        format!("{} {}", crate::locale::number(value, 0), units[unit])
    }
}

fn symbol_image(metric: &str) -> Option<Retained<NSImage>> {
    let symbol = match metric {
        "cpu" => "cpu",
        "memory" => "memorychip",
        "gpu" => "rectangle.3.group",
        "disk" => "internaldrive",
        "network" => "network",
        "battery" => "battery.100percent",
        "power" => "bolt.fill",
        _ => return None,
    };
    NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        Some(&NSString::from_str(metric)),
    )
}

/// The ink for unremarkable text and lines: black in a template image, which
/// macOS tints itself, and the label colour once the image carries colour.
fn ink(template: bool) -> Retained<NSColor> {
    if template { NSColor::blackColor() } else { NSColor::labelColor() }
}

/// Images drawn black (the logo, SF Symbols) take `color` once the menu-bar
/// image is no longer a template that macOS tints for us.
fn recolor(rect: objc2_foundation::NSRect, template: bool, color: &NSColor) {
    if !template {
        color.set();
        NSRectFillUsingOperation(rect, NSCompositingOperation::SourceAtop);
    }
}

fn draw_text(text: &str, x: f64, font_size: f64, value: bool, color: &NSColor) {
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSDictionary;
    let weight = unsafe { NSFontWeightMedium };
    let font = if value {
        NSFont::monospacedDigitSystemFontOfSize_weight(font_size, weight)
    } else {
        NSFont::systemFontOfSize_weight(font_size, weight)
    };
    let font_object: &AnyObject = font.as_ref();
    let color_object: &AnyObject = color.as_ref();
    unsafe {
        let attributes = NSDictionary::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[font_object, color_object],
        );
        let string = NSString::from_str(text);
        let size = string.sizeWithAttributes(Some(&attributes));
        string.drawAtPoint_withAttributes(
            objc2_foundation::NSPoint {
                x,
                y: (ITEM_HEIGHT - size.height) / 2.,
            },
            Some(&attributes),
        );
    }
}

fn draw_graph(history: &[f64], x: f64, metric: &str, color: &NSColor) {
    if history.len() < 2 {
        draw_text("—", x + 6., 10., false, color);
        return;
    }
    let maximum = if matches!(metric, "cpu" | "memory" | "gpu" | "battery") {
        100.
    } else {
        history
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(1., f64::max)
    };
    let count = history.len().min(60);
    let points = &history[history.len() - count..];
    let path = NSBezierPath::bezierPath();
    path.setLineWidth(1.);
    color.setStroke();
    for (i, value) in points.iter().enumerate() {
        let point = objc2_foundation::NSPoint {
            x: x + i as f64 / (count - 1) as f64 * GRAPH_WIDTH,
            y: 3.
                + if value.is_finite() {
                    (value / maximum).clamp(0., 1.) * 10.
                } else {
                    0.
                },
        };
        if i == 0 {
            path.moveToPoint(point);
        } else {
            path.lineToPoint(point);
        }
    }
    path.stroke();
}

/// A metric's accent from the app theme, for coloured readings.
fn accent_color(metric: &str, dark: bool) -> Retained<NSColor> {
    use crate::theme::Metric;
    let metric = match metric {
        "cpu" => Metric::Cpu,
        "memory" => Metric::Memory,
        "gpu" => Metric::Gpu,
        "disk" => Metric::Disk,
        "network" => Metric::Network,
        "battery" => Metric::Battery,
        _ => Metric::Power,
    };
    let color: gpui_kit::Rgba = metric.accent(dark).into();
    NSColor::colorWithSRGBRed_green_blue_alpha(color.r as f64, color.g as f64, color.b as f64, 1.)
}

/// The track behind a bar or ring: the same ink, faint.
fn track(color: &NSColor) -> Retained<NSColor> {
    color.colorWithAlphaComponent(0.25)
}

fn draw_bars(readout: &Readout, x: f64, color: &NSColor) {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(bars) = readout.bars.as_ref() else {
        draw_text("—", x + 2., 10., false, color);
        return;
    };
    let faint = track(color);
    for (bar_x, height) in bars.xs.iter().zip(&readout.cores) {
        let at = |height: f64| NSRect {
            origin: NSPoint { x: x + bar_x, y: 2. },
            size: NSSize { width: bars.bar_width, height },
        };
        faint.set();
        NSRectFillUsingOperation(at(menu_style::BAR_MAX_HEIGHT), NSCompositingOperation::SourceOver);
        color.set();
        NSRectFillUsingOperation(at(f64::from(*height)), NSCompositingOperation::SourceOver);
    }
}

fn draw_ring(fill: Option<u8>, x: f64, color: &NSColor) {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(fill) = fill else {
        draw_text("—", x + 2., 10., false, color);
        return;
    };
    let (center, radius) = (NSPoint { x: x + RING_SIZE / 2., y: ITEM_HEIGHT / 2. }, 5.);
    let oval = NSRect {
        origin: NSPoint { x: center.x - radius, y: center.y - radius },
        size: NSSize { width: radius * 2., height: radius * 2. },
    };
    let ring = NSBezierPath::bezierPath();
    ring.setLineWidth(2.5);
    ring.appendBezierPathWithOvalInRect(oval);
    track(color).setStroke();
    ring.stroke();
    color.setStroke();
    if fill >= 100 {
        ring.stroke();
    } else if fill > 0 {
        // From twelve o'clock, clockwise.
        let arc = NSBezierPath::bezierPath();
        arc.setLineWidth(2.5);
        arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            center,
            radius,
            90.,
            90. - f64::from(fill) * 3.6,
            true,
        );
        arc.stroke();
    }
}

/// A battery: outline and nub in `ink`, filled to the charge in `color`,
/// with a bolt cut into it while charging.
fn draw_gauge(readout: &Readout, x: f64, ink: &NSColor, color: &NSColor) {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(fill) = readout.fill else {
        draw_text("—", x + 2., 10., false, ink);
        return;
    };
    let rect = |x, y, width, height| NSRect { origin: NSPoint { x, y }, size: NSSize { width, height } };
    let body = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect(x + 0.5, 3.5, 17., 9.), 2.5, 2.5);
    body.setLineWidth(1.);
    ink.setStroke();
    body.stroke();
    ink.set();
    NSRectFillUsingOperation(rect(x + 18., 6., 1.5, 4.), NSCompositingOperation::SourceOver);
    if fill > 0 {
        color.set();
        let width = (13. * f64::from(fill) / 100.).max(1.);
        NSRectFillUsingOperation(rect(x + 2.5, 5., width, 6.), NSCompositingOperation::SourceOver);
    }
    if readout.charging {
        let (cx, cy) = (x + 9., ITEM_HEIGHT / 2.);
        let bolt = NSBezierPath::bezierPath();
        bolt.moveToPoint(NSPoint { x: cx + 1., y: cy + 4. });
        for (dx, dy) in [(-2.5, -0.5), (0., -0.5), (-1., -4.), (2.5, 0.5), (0., 0.5)] {
            bolt.lineToPoint(NSPoint { x: cx + dx, y: cy + dy });
        }
        bolt.closePath();
        // A clear halo, then the bolt: it reads on any fill, in a template too.
        let context = NSGraphicsContext::currentContext();
        if let Some(context) = context.as_ref() {
            context.setCompositingOperation(NSCompositingOperation::DestinationOut);
            NSColor::blackColor().set();
            bolt.setLineWidth(2.);
            bolt.stroke();
            bolt.fill();
            context.setCompositingOperation(NSCompositingOperation::SourceOver);
        }
        ink.set();
        bolt.fill();
    }
}

fn menu_image(readouts: &[Readout], show_icon: bool, colored: bool, dark: bool) -> Retained<NSImage> {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let width = readouts.iter().map(Readout::width).sum::<f64>()
        + if show_icon { 16. } else { 0. }
        + (readouts.len() + usize::from(show_icon)).saturating_sub(1) as f64 * ITEM_GAP;
    let readouts = readouts.to_vec();
    // A template image is all one ink, so colour needs a plain image; it
    // stays a template whenever nothing is tinted.
    let template = !colored && readouts.iter().all(|r| r.tint.is_none());
    let logo = LOGO.with(Clone::clone);
    // AppKit invokes the drawing block at the display's backing scale.
    let drawing = block2::RcBlock::new(move |_bounds: NSRect| {
        let mut x = 0.;
        if show_icon {
            if let Some(logo) = logo.as_ref() {
                let rect = NSRect { origin: NSPoint { x, y: 0. }, size: NSSize { width: 16., height: 16. } };
                logo.drawInRect_fromRect_operation_fraction(
                    rect,
                    NSRect { origin: NSPoint { x: 0., y: 0. }, size: NSSize { width: 0., height: 0. } },
                    NSCompositingOperation::SourceOver, 1.,
                );
                recolor(rect, template, &ink(template));
            } else {
                draw_text("T", x + 3., 12., true, &ink(template));
            }
            x += 16. + ITEM_GAP;
        }
        for readout in &readouts {
            let accent = match readout.tint {
                Some(tint) => tint.color(),
                None if colored => accent_color(readout.metric, dark),
                None => ink(template),
            };
            if readout.style == "icon" {
                if let Some(symbol) = symbol_image(readout.metric) {
                    let rect = NSRect {
                        origin: NSPoint { x, y: 1. },
                        size: NSSize {
                            width: 14.,
                            height: 14.,
                        },
                    };
                    symbol.drawInRect_fromRect_operation_fraction(
                        rect,
                        NSRect {
                            origin: NSPoint { x: 0., y: 0. },
                            size: NSSize {
                                width: 0.,
                                height: 0.,
                            },
                        },
                        NSCompositingOperation::SourceOver,
                        1.,
                    );
                    recolor(rect, template, &accent);
                } else {
                    draw_text(readout.label, x, 8., false, &ink(template));
                }
            } else {
                draw_text(readout.label, x, 9., false, &ink(template));
                let value_x = x + readout.label_width + 4.;
                match readout.style.as_str() {
                    "bars" => draw_bars(readout, value_x, &accent),
                    "pie" => draw_ring(readout.fill, value_x, &accent),
                    "gauge" => draw_gauge(readout, value_x, &ink(template), &accent),
                    "graph" => {}
                    _ => draw_text(&readout.value, value_x, 11., true, &accent),
                }
                if matches!(readout.style.as_str(), "graph" | "both") {
                    let graph_x = if readout.style == "both" {
                        value_x + readout.value_width() + 6.
                    } else {
                        value_x
                    };
                    draw_graph(&readout.history, graph_x, readout.metric, &accent);
                }
            }
            x += readout.width() + ITEM_GAP;
        }
        objc2::runtime::Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize {
            width: width.max(16.),
            height: ITEM_HEIGHT,
        },
        false,
        &drawing,
    );
    image.setTemplate(template);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_on_the_item_is_not_an_outside_click() {
        assert!(same_spot((3025., 2160.), (3025.3, 2160.)));
        assert!(same_spot((3000., 2150.), (3040., 2160.)), "anywhere on the item");
        assert!(!same_spot((1200., 900.), (3025., 2160.)), "a click in another app");
        assert!(!same_spot((2800., 2160.), (3025., 2160.)), "another menu-bar item");
    }

    #[test]
    fn the_panel_sits_under_the_click_on_the_display_clicked() {
        let main = Display { number: 1, frame: (0., 0., 3840., 2160.), visible_top: 2126. };
        let left = Display { number: 2, frame: (-1920., 0., 1920., 1080.), visible_top: 1080. };
        let at = place((3000., 2150.), &[main, left]).unwrap();
        assert_eq!((at.display, at.left, at.bottom, at.display_width), (1, 2990., 34., 3840.));
        let at = place((-100., 1075.), &[main, left]).unwrap();
        assert_eq!((at.display, at.left, at.bottom), (2, 1810., 24.), "a hidden menu bar keeps a usual gap");
        assert!(place((9999., 9999.), &[main]).is_none());
    }

    #[test]
    fn menu_readings_are_identified_ordered_and_do_not_resize_for_new_values() {
        let settings = Settings {
            menu_items: vec![
                MenuItem {
                    metric: "network".into(),
                    style: "value".into(),
                },
                MenuItem {
                    metric: "cpu".into(),
                    style: "both".into(),
                },
                MenuItem {
                    metric: "cpu".into(),
                    style: "graph".into(),
                },
                MenuItem {
                    metric: "unsupported".into(),
                    style: "icon".into(),
                },
            ],
            ..Settings::default()
        };
        let mut snapshot = Snapshot {
            cpu_pct: 9.,
            ..Snapshot::default()
        };
        let first = menu_readouts(&settings, &snapshot, |_| 22.);
        assert_eq!(
            first.iter().map(|r| r.label).collect::<Vec<_>>(),
            ["CPU", l("NET", "NETT")]
        );
        assert_eq!(first[0].value, crate::engine::fmt_pct(9.));
        snapshot.cpu_pct = 100.;
        snapshot.net.rate_in = 1024. * 1024. * 999.;
        let next = menu_readouts(&settings, &snapshot, |_| 22.);
        assert_eq!(next[0].value, crate::engine::fmt_pct(100.));
        assert_eq!(next[1].value, "999 M/s");
        assert_eq!(
            first.iter().map(Readout::width).sum::<f64>(),
            next.iter().map(Readout::width).sum::<f64>()
        );
    }

    fn items(items: &[(&str, &str)], colored: bool) -> Settings {
        Settings {
            menu_colored: colored,
            menu_items: items
                .iter()
                .map(|(metric, style)| MenuItem { metric: (*metric).into(), style: (*style).into() })
                .collect(),
            ..Settings::default()
        }
    }

    fn frame(settings: &Settings, snapshot: &Snapshot) -> Frame {
        Frame {
            readouts: menu_readouts(settings, snapshot, |_| 22.),
            show_icon: true,
            colored: settings.menu_colored,
            dark: false,
            description: String::new(),
        }
    }

    #[test]
    fn the_image_is_rebuilt_only_when_something_it_shows_changes() {
        let settings = items(&[("cpu", "both"), ("memory", "pie"), ("network", "value")], false);
        let mut snapshot = Snapshot { cpu_pct: 20.2, ..Snapshot::default() };
        snapshot.net.rate_in = 1000.;
        let before = frame(&settings, &snapshot);
        snapshot.cpu_pct = 20.4; // still "20 %"
        snapshot.mem.total = 1000;
        snapshot.mem.used = 1; // still 0 %
        assert!(before == frame(&settings, &snapshot), "a change below the displayed value");
        snapshot.cpu_pct = 21.;
        assert!(before != frame(&settings, &snapshot), "a new displayed value");
        snapshot.cpu_pct = 20.2;
        assert!(before != frame(&items(&[("cpu", "value")], false), &snapshot), "a new style");
        let mut colored = settings.clone();
        colored.menu_colored = true;
        assert!(before != frame(&colored, &snapshot), "the colour mode");
        snapshot.mem.pressure = Some(crate::metrics::Pressure::Warning);
        assert!(before != frame(&settings, &snapshot), "a tint");
    }

    #[test]
    fn a_graph_changes_the_image_but_only_a_graph_reads_the_history() {
        let mut snapshot = Snapshot::default();
        snapshot.cpu_hist.pts = vec![1., 2., 3.];
        let graph = items(&[("cpu", "graph")], false);
        let before = frame(&graph, &snapshot);
        snapshot.cpu_hist.pts.push(f64::NAN);
        let after = frame(&graph, &snapshot);
        assert!(before != after);
        assert!(after == frame(&graph, &snapshot), "a gap in the history still compares equal");
        let value = items(&[("cpu", "value")], false);
        assert!(frame(&value, &snapshot) == frame(&value, &Snapshot::default()), "history is not drawn");
    }

    #[test]
    fn styles_a_metric_cannot_show_are_drawn_as_values() {
        assert_eq!(effective_style("cpu", "bars"), "bars");
        assert_eq!(effective_style("memory", "bars"), "value");
        assert_eq!(effective_style("network", "pie"), "value");
        assert_eq!(effective_style("cpu", "hologram"), "value");
        assert_eq!(effective_style("battery", "gauge"), "gauge");
    }

    #[test]
    fn colour_follows_the_level_only_when_asked() {
        let mut snapshot = Snapshot { cpu_pct: 97., ..Snapshot::default() };
        snapshot.battery.percent = Some(15.);
        snapshot.disk.total = 100;
        snapshot.disk.free = 5;
        snapshot.mem.pressure = Some(crate::metrics::Pressure::Warning);
        let all = [("cpu", "pie"), ("memory", "value"), ("disk", "pie"), ("battery", "gauge")];
        let tints = |colored| {
            menu_readouts(&items(&all, colored), &snapshot, |_| 22.)
                .iter()
                .map(|r| (r.metric, r.tint))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            tints(false),
            [("cpu", None), ("memory", Some(Tint::Caution)), ("disk", None), ("battery", None)],
            "monochrome keeps today's look: only memory pressure tints"
        );
        assert_eq!(
            tints(true),
            [
                ("cpu", Some(Tint::Critical)),
                ("memory", Some(Tint::Caution)),
                ("disk", Some(Tint::Critical)),
                ("battery", Some(Tint::Caution)),
            ]
        );
        snapshot.battery.is_charging = true;
        let battery = menu_readouts(&items(&[("battery", "gauge")], true), &snapshot, |_| 22.);
        assert_eq!((battery[0].tint, battery[0].charging, battery[0].fill), (None, true, Some(15)));
    }

    #[test]
    fn cpu_bars_follow_the_chip_s_core_groups_and_keep_their_width() {
        let mut snapshot = Snapshot { core_pct: vec![0., 50., 100., 25.], ..Snapshot::default() };
        snapshot.chip.groups = vec![
            crate::metrics::CoreGroup { count: 1, ..Default::default() },
            crate::metrics::CoreGroup { count: 3, ..Default::default() },
        ];
        let settings = items(&[("cpu", "bars")], false);
        let readout = &menu_readouts(&settings, &snapshot, |_| 22.)[0];
        assert_eq!(readout.cores, [1, 6, 12, 3]);
        assert_eq!(readout.value_width(), 4. * 2. + 2. + 3.);
        assert_eq!(readout.width(), 22. + 4. + readout.value_width());
        snapshot.core_pct = vec![90.; 4];
        assert_eq!(menu_readouts(&settings, &snapshot, |_| 22.)[0].width(), readout.width());
        snapshot.core_pct.clear();
        let empty = &menu_readouts(&settings, &snapshot, |_| 22.)[0];
        assert!(empty.bars.is_none());
        assert_eq!(empty.value_width(), EMPTY_BARS_WIDTH);
    }

    #[test]
    fn rings_show_whole_percent_and_the_disk_ring_shows_free_space() {
        let mut snapshot = Snapshot { cpu_pct: 49.6, ..Snapshot::default() };
        snapshot.disk.total = 1000;
        snapshot.disk.free = 250;
        let settings = items(&[("cpu", "pie"), ("disk", "pie")], false);
        let readouts = menu_readouts(&settings, &snapshot, |_| 22.);
        assert_eq!(readouts[0].fill, Some(50));
        assert_eq!(readouts[1].fill, Some(25));
        assert!(readouts[1].value.contains(&crate::engine::fmt_pct(25.)));
        assert_eq!(readouts[1].value_width(), RING_SIZE);
        snapshot.disk.total = 0;
        assert_eq!(menu_readouts(&settings, &snapshot, |_| 22.)[1].fill, None, "no disk reading, no ring");
    }

    #[test]
    fn slot_width_reserves_measured_labels_and_fixed_geometry() {
        assert_eq!(slot_width("icon", 22., 31.), 14.);
        assert_eq!(slot_width("icon", 34., 31.), 14.);
        assert_eq!(slot_width("value", 22., 31.), 22. + 4. + 31.);
        assert_eq!(slot_width("graph", 22., 31.), 22. + 4. + GRAPH_WIDTH);
        assert_eq!(
            slot_width("both", 22., 55.),
            22. + 4. + 55. + 6. + GRAPH_WIDTH
        );
        assert_eq!(slot_width("value", 34., 31.), 34. + 4. + 31.);
        assert_eq!(slot_width("graph", 34., 31.), 34. + 4. + GRAPH_WIDTH);
        assert_eq!(
            slot_width("both", 34., 55.),
            34. + 4. + 55. + 6. + GRAPH_WIDTH
        );
        assert!(slot_width("value", 34., 31.) > slot_width("value", 22., 31.));
    }
}
