//! Menu-bar presence: one NSStatusItem per enabled metric, drawn as
//! value text + sparkline image. Click toggles the panel.

#![allow(dead_code)]
#![cfg(target_os = "macos")]

use crate::engine::Snapshot;
use crate::settings::Settings;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, ClassType};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::sync::atomic::{AtomicBool, Ordering};

pub static TOGGLE_PANEL: AtomicBool = AtomicBool::new(false);
pub static QUIT_APP: AtomicBool = AtomicBool::new(false);

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TideGaugeTarget"]
    struct TideGaugeTarget;
    impl TideGaugeTarget {
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

pub struct StatusBar {
    items: Vec<(String, Retained<NSStatusItem>)>,
    target: Retained<TideGaugeTarget>,
}

impl StatusBar {
    pub fn new() -> Self {
        let target: Retained<TideGaugeTarget> = unsafe { msg_send![TideGaugeTarget::class(), new] };
        StatusBar { items: Vec::new(), target }
    }

    pub fn sync(&mut self, settings: &Settings, snap: &Snapshot, dark: bool) {
        let want: Vec<String> = settings.menu_items.iter().map(|m| m.metric.clone()).collect();
        self.items.retain(|(m, _)| want.contains(m));
        for mi in &settings.menu_items {
            if !self.items.iter().any(|(m, _)| m == &mi.metric) {
                unsafe {
                    let bar = NSStatusBar::systemStatusBar();
                    let item = bar.statusItemWithLength(-1.0);
                    if let Some(btn) = item.button(mtm()) {
                        btn.setTarget(Some(&self.target));
                        btn.setAction(Some(sel!(toggle:)));
                        btn.sendActionOn(NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown);
                    }
                    self.items.push((mi.metric.clone(), item));
                }
            }
        }
        for (m, item) in &self.items {
            let (title, img) = render_item(m, snap, dark);
            unsafe {
                if let Some(btn) = item.button(mtm()) {
                    btn.setTitle(&NSString::from_str(&title));
                    if let Some(img) = img {
                        btn.setImage(Some(&img));
                        btn.setImagePosition(NSCellImagePosition::ImageLeft);
                    } else {
                        btn.setImage(None);
                    }
                }
            }
        }
    }
}

fn render_item(metric: &str, s: &Snapshot, dark: bool) -> (String, Option<Retained<NSImage>>) {
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
    let img = pts.and_then(|p| sparkline_image(&p, dark));
    (title, img)
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
