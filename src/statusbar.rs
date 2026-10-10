//! Menu-bar presence: one NSStatusItem per enabled metric, drawn as
//! icons, values, and sparklines. A native context menu provides settings and quit.

#![allow(dead_code)]
#![cfg(target_os = "macos")]

use crate::engine::Snapshot;
use crate::settings::{MenuItem, Settings, MENU_METRICS};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, ClassType};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::sync::atomic::{AtomicBool, Ordering};

pub static TOGGLE_PANEL: AtomicBool = AtomicBool::new(false);
pub static SHOW_SETTINGS: AtomicBool = AtomicBool::new(false);
pub static QUIT_APP: AtomicBool = AtomicBool::new(false);
pub static SHOW_CONTEXT_MENU: AtomicBool = AtomicBool::new(false);

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TideGaugeTarget"]
    struct TideGaugeTarget;
    impl TideGaugeTarget {
        #[unsafe(method(activate:))]
        fn _activate(&self, sender: Option<&NSStatusBarButton>) {
            let Some(button) = sender else { return; };
            let event = NSApplication::sharedApplication(mtm()).currentEvent();
            let context_click = event.as_ref().is_some_and(|event| {
                event.r#type() == NSEventType::RightMouseDown
                    || event.modifierFlags().contains(NSEventModifierFlags::Control)
            });
            if button.tag() == 1 || context_click {
                if let Some(event) = event {
                    let menu = context_menu(self);
                    NSMenu::popUpContextMenu_withEvent_forView(&menu, &event, button);
                }
            } else {
                TOGGLE_PANEL.store(true, Ordering::SeqCst);
            }
        }
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

pub fn menu_bar_dark() -> bool {
    unsafe {
        let app = NSApplication::sharedApplication(mtm());
        let app_obj: &NSObject = &app;
        let appearance: Retained<objc2::runtime::AnyObject> =
            msg_send![app_obj, effectiveAppearance];
        let name: Retained<NSString> = msg_send![&*appearance, name];
        name.to_string().to_lowercase().contains("dark")
    }
}

fn context_menu(target: &TideGaugeTarget) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm());
    for (title, action, key) in [
        ("Open Dashboard", sel!(toggle:), ""),
        ("Settings…", sel!(settings:), ","),
        ("Quit Tidegauge", sel!(quit:), "q"),
    ] {
        if key == "q" { menu.addItem(&NSMenuItem::separatorItem(mtm())); }
        let item = NSMenuItem::new(mtm());
        item.setTitle(&NSString::from_str(title));
        item.setKeyEquivalent(&NSString::from_str(key));
        unsafe {
            item.setTarget(Some(target));
            item.setAction(Some(action));
        }
        menu.addItem(&item);
    }
    menu
}

/// The same menu is available from Settings for keyboard and window users.
pub fn show_context_menu() {
    let app = NSApplication::sharedApplication(mtm());
    let Some(window) = app.keyWindow() else { return; };
    let Some(view) = window.contentView() else { return; };
    let Some(event) = app.currentEvent() else { return; };
    let target: Retained<TideGaugeTarget> = unsafe { msg_send![TideGaugeTarget::class(), new] };
    NSMenu::popUpContextMenu_withEvent_forView(&context_menu(&target), &event, &view);
}

fn symbol_image(metric: &str) -> Option<Retained<NSImage>> {
    let symbol = match metric {
        "cpu" => "cpu", "memory" => "memorychip", "gpu" => "rectangle.3.group",
        "disk" => "internaldrive", "network" => "network", "battery" => "battery.100percent",
        "power" => "bolt.fill", _ => "waveform.path",
    };
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol), Some(&NSString::from_str("Tidegauge")),
    )?;
    image.setSize(objc2_foundation::NSSize { width: 16., height: 16. });
    image.setTemplate(true);
    Some(image)
}

pub struct StatusBar {
    items: Vec<(String, Retained<NSStatusItem>)>,
    launcher: Option<Retained<NSStatusItem>>,
    target: Retained<TideGaugeTarget>,
}

impl StatusBar {
    pub fn new() -> Self {
        let target: Retained<TideGaugeTarget> = unsafe { msg_send![TideGaugeTarget::class(), new] };
        StatusBar { items: Vec::new(), launcher: None, target }
    }

    fn make_item(&self, launcher: bool, tooltip: &str) -> Retained<NSStatusItem> {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(button) = item.button(mtm()) {
            unsafe {
                button.setTarget(Some(&self.target));
                button.setAction(Some(sel!(activate:)));
            }
            button.setTag(if launcher { 1 } else { 0 });
            button.setToolTip(Some(&NSString::from_str(tooltip)));
            button.sendActionOn(NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown);
        }
        item
    }

    pub fn sync(&mut self, settings: &Settings, snap: &Snapshot, dark: bool) {
        let bar = NSStatusBar::systemStatusBar();
        let mut seen = std::collections::HashSet::new();
        let wanted: Vec<&MenuItem> = settings.menu_items.iter().filter(|item| {
            MENU_METRICS.iter().any(|(key, _)| *key == item.metric) && seen.insert(item.metric.as_str())
        }).collect();
        let keys: Vec<&str> = wanted.iter().map(|item| item.metric.as_str()).collect();
        if self.items.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>() != keys {
            for (_, item) in self.items.drain(..) { bar.removeStatusItem(&item); }
            for item in &wanted {
                let native = self.make_item(false, &format!("Tidegauge · {}. Right-click for Settings and Quit.", item.metric));
                self.items.push((item.metric.clone(), native));
            }
        }
        if settings.show_app_icon || wanted.is_empty() {
            if self.launcher.is_none() {
                self.launcher = Some(self.make_item(true, "Tidegauge · Open, Settings, and Quit"));
            }
            if let Some(button) = self.launcher.as_ref().and_then(|item| item.button(mtm())) {
                let image = symbol_image("tidegauge");
                button.setTitle(&NSString::from_str(if image.is_some() { "" } else { "Tg" }));
                button.setImage(image.as_deref());
            }
        } else if let Some(item) = self.launcher.take() {
            bar.removeStatusItem(&item);
        }
        for ((_, item), preference) in self.items.iter().zip(wanted) {
            let (title, image) = render_item(preference, snap, dark);
            if let Some(button) = item.button(mtm()) {
                button.setTitle(&NSString::from_str(&title));
                button.setImage(image.as_deref());
                button.setImagePosition(NSCellImagePosition::ImageLeft);
            }
        }
    }
}

impl Drop for StatusBar {
    fn drop(&mut self) {
        let bar = NSStatusBar::systemStatusBar();
        for (_, item) in self.items.drain(..) { bar.removeStatusItem(&item); }
        if let Some(item) = self.launcher.take() { bar.removeStatusItem(&item); }
    }
}

fn render_item(preference: &MenuItem, s: &Snapshot, dark: bool) -> (String, Option<Retained<NSImage>>) {
    let metric = preference.metric.as_str();
    let (title, pts) = match metric {
        "cpu" => (format!("{:.0}%", s.cpu_pct), Some(s.cpu_hist.pts.clone())),
        "memory" => (
            format!("{:.0}%", s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0),
            Some(s.mem_hist.pts.clone()),
        ),
        "network" => (
            crate::engine::fmt_rate(s.net.rate_in + s.net.rate_out),
            Some(s.net_in_hist.pts.iter().zip(&s.net_out_hist.pts).map(|(a, b)| a + b).collect()),
        ),
        "disk" => (
            crate::engine::fmt_rate(s.disk.read_rate + s.disk.write_rate),
            Some(s.disk_r_hist.pts.iter().zip(&s.disk_w_hist.pts).map(|(a, b)| a + b).collect()),
        ),
        "battery" => (
            s.battery.percent.map(|p| format!("{:.0}%", p)).unwrap_or_else(|| "—".into()),
            if s.battery.present { Some(s.batt_hist.pts.clone()) } else { None },
        ),
        "gpu" => (
            s.gpu.utilization.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "—".into()),
            if s.gpu.supported { Some(s.gpu_hist.pts.clone()) } else { None },
        ),
        "power" => (crate::engine::fmt_watts(s.power_w), Some(s.power_hist.pts.clone())),
        _ => (metric.to_string(), None),
    };
    match preference.style.as_str() {
        "icon" => (String::new(), symbol_image(metric)),
        "graph" => (String::new(), pts.and_then(|p| sparkline_image(&p, dark)).or_else(|| symbol_image(metric))),
        "both" => (title, pts.and_then(|p| sparkline_image(&p, dark)).or_else(|| symbol_image(metric))),
        _ => (title, None),
    }
}

fn sparkline_image(pts: &[f64], dark: bool) -> Option<Retained<NSImage>> {
    if pts.len() < 2 {
        return None;
    }
    unsafe {
        let (w, h) = (34.0f64, 16.0f64);
        let img = NSImage::new();
        img.setSize(objc2_foundation::NSSize { width: w, height: h });
        img.lockFocus();
        let color = if dark {
            NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 1.0, 1.0, 0.8)
        } else {
            NSColor::colorWithSRGBRed_green_blue_alpha(0.15, 0.15, 0.2, 0.85)
        };
        color.setStroke();
        let max = pts.iter().cloned().fold(1.0f64, f64::max);
        let n = pts.len().min(60);
        let tail = &pts[pts.len() - n..];
        let path = NSBezierPath::bezierPath();
        path.setLineWidth(1.2);
        for (i, v) in tail.iter().enumerate() {
            let x = i as f64 / (n - 1).max(1) as f64 * (w - 2.0) + 1.0;
            let y = (v / max).clamp(0.0, 1.0) * (h - 3.0) + 1.5;
            let pt = objc2_foundation::NSPoint { x, y };
            if i == 0 {
                path.moveToPoint(pt);
            } else {
                path.lineToPoint(pt);
            }
        }
        path.stroke();
        img.unlockFocus();
        Some(img)
    }
}
