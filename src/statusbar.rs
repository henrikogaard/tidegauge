//! One compact native menu-bar item, with optional labeled metric readings.

#![allow(dead_code)]
#![cfg(target_os = "macos")]

use crate::engine::Snapshot;
use crate::locale::text as l;
#[cfg(test)]
use crate::settings::MenuItem;
use crate::settings::{localized_metric_label, Settings, MENU_METRICS};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, AllocAnyThread, ClassType};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::sync::atomic::{AtomicBool, Ordering};

pub static TOGGLE_PANEL: AtomicBool = AtomicBool::new(false);
pub static SHOW_SETTINGS: AtomicBool = AtomicBool::new(false);
pub static QUIT_APP: AtomicBool = AtomicBool::new(false);
pub static SHOW_CONTEXT_MENU: AtomicBool = AtomicBool::new(false);

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

/// A single native item keeps readings together and preserves their identity.
pub struct StatusBar {
    item: Retained<NSStatusItem>,
    _target: Retained<TideGaugeTarget>,
}

impl StatusBar {
    pub fn new() -> Self {
        let target: Retained<TideGaugeTarget> = unsafe { msg_send![TideGaugeTarget::class(), new] };
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        item.setMenu(Some(&context_menu(&target)));
        Self {
            item,
            _target: target,
        }
    }

    pub fn sync(&mut self, settings: &Settings, snap: &Snapshot) {
        let readouts = menu_readouts(settings, snap, measure_label_width);
        let show_icon = settings.show_app_icon || readouts.is_empty();
        let menu_hint = l(
            "Click for Open Dashboard, Settings, and Quit.",
            "Klikk for å åpne oversikten, innstillinger eller avslutte.",
        );
        let description = if readouts.is_empty() {
            format!("Tidegauge. {menu_hint}")
        } else {
            format!(
                "Tidegauge. {}. {menu_hint}",
                readouts
                    .iter()
                    .map(|r| {
                        format!(
                            "{} {}",
                            localized_metric_label(r.metric).unwrap_or(l("Metric", "Måling")),
                            r.value
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        };
        let image = menu_image(&readouts, show_icon);
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

#[derive(Clone)]
struct Readout {
    metric: &'static str,
    label: &'static str,
    label_width: f64,
    style: String,
    value: String,
    history: Vec<f64>,
}

impl Readout {
    fn value_width(&self) -> f64 {
        match self.metric {
            "network" | "disk" => 55.,
            "power" => 42.,
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

fn menu_readouts(settings: &Settings, s: &Snapshot, measure: impl Fn(&str) -> f64) -> Vec<Readout> {
    // Canonical metric order also eliminates duplicate and unsupported entries.
    MENU_METRICS
        .iter()
        .filter_map(|&(metric, _)| {
            let preference = settings
                .menu_items
                .iter()
                .find(|item| item.metric == metric)?;
            let (label, value, history) = match metric {
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
            Some(Readout {
                metric,
                label,
                label_width: measure(label),
                style: preference.style.clone(),
                value,
                history,
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

fn draw_text(text: &str, x: f64, font_size: f64, value: bool) {
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSDictionary;
    let weight = unsafe { NSFontWeightMedium };
    let font = if value {
        NSFont::monospacedDigitSystemFontOfSize_weight(font_size, weight)
    } else {
        NSFont::systemFontOfSize_weight(font_size, weight)
    };
    let color = NSColor::blackColor();
    let font_object: &AnyObject = &font;
    let color_object: &AnyObject = &color;
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

fn draw_graph(history: &[f64], x: f64, metric: &str) {
    if history.len() < 2 {
        draw_text("—", x + 6., 10., false);
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
    NSColor::blackColor().setStroke();
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

fn menu_image(readouts: &[Readout], show_icon: bool) -> Retained<NSImage> {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let width = readouts.iter().map(Readout::width).sum::<f64>()
        + if show_icon { 16. } else { 0. }
        + (readouts.len() + usize::from(show_icon)).saturating_sub(1) as f64 * ITEM_GAP;
    let readouts = readouts.to_vec();
    let logo = LOGO.with(Clone::clone);
    // AppKit invokes the drawing block at the display's backing scale.
    let drawing = block2::RcBlock::new(move |_bounds: NSRect| {
        let mut x = 0.;
        if show_icon {
            if let Some(logo) = logo.as_ref() {
                logo.drawInRect_fromRect_operation_fraction(
                    NSRect { origin: NSPoint { x, y: 0. }, size: NSSize { width: 16., height: 16. } },
                    NSRect { origin: NSPoint { x: 0., y: 0. }, size: NSSize { width: 0., height: 0. } },
                    NSCompositingOperation::SourceOver, 1.,
                );
            } else {
                draw_text("T", x + 3., 12., true);
            }
            x += 16. + ITEM_GAP;
        }
        for readout in &readouts {
            if readout.style == "icon" {
                if let Some(symbol) = symbol_image(readout.metric) {
                    symbol.drawInRect_fromRect_operation_fraction(
                        NSRect {
                            origin: NSPoint { x, y: 1. },
                            size: NSSize {
                                width: 14.,
                                height: 14.,
                            },
                        },
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
                } else {
                    draw_text(readout.label, x, 8., false);
                }
            } else {
                draw_text(readout.label, x, 9., false);
                let value_x = x + readout.label_width + 4.;
                if readout.style != "graph" {
                    draw_text(&readout.value, value_x, 11., true);
                }
                if matches!(readout.style.as_str(), "graph" | "both") {
                    let graph_x = if readout.style == "both" {
                        value_x + readout.value_width() + 6.
                    } else {
                        value_x
                    };
                    draw_graph(&readout.history, graph_x, readout.metric);
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
    image.setTemplate(true);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

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
