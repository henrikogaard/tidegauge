//! App orchestration: sampler thread, status bar pump, window lifecycle.

#![allow(dead_code)]

use crate::alerts::AlertEngine;
use crate::engine::{engine, Alert};
use crate::settings::Settings;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

/// Latest un-notified alerts, drained by the UI/notification layer.
static ALERTS: OnceLock<Mutex<Vec<Alert>>> = OnceLock::new();

pub fn drain_alerts(settings: &Settings) -> Vec<Alert> {
    let mut alerts = std::mem::take(
        &mut *ALERTS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap(),
    );
    alerts.retain(|alert| still_enabled(&alert.metric, settings));
    alerts
}

/// Whether the switch behind an alert is still on when it is delivered.
fn still_enabled(metric: &str, settings: &Settings) -> bool {
    match metric {
        "cpu" => settings.alert_cpu_enabled,
        "memory" => settings.alert_mem_enabled,
        "disk" => settings.alert_disk_enabled,
        "network" => settings.alert_network_enabled,
        app if app.starts_with("app:") => settings.alert_app_enabled,
        app if app.starts_with("appmem:") => settings.alert_app_mem_enabled,
        "low_disk" => settings.alert_low_disk_enabled,
        "thermal" => settings.alert_thermal_enabled,
        "pressure" => settings.alert_pressure_enabled,
        "temperature" => settings.alert_temp_enabled,
        "battery_low" => settings.alert_battery_low_enabled,
        "battery_full" => settings.alert_battery_full_enabled,
        disk if disk.starts_with("disk_health:") => settings.alert_disk_health_enabled,
        "offline" | "online" => settings.alert_offline_enabled,
        _ => false,
    }
}

/// Background sampler: process/audio/projects every ~2s; BT every ~20s
/// (system_profiler is expensive); history write every ~10s.
pub fn start_sampler() {
    thread::Builder::new()
        .name("tidegauge-sampler".into())
        .spawn(move || {
            let eng = engine();
            let mut alerts = AlertEngine::new();
            let mut bt = crate::engine::BtInfo::default();
            let mut tick = 0u64;
            loop {
                let settings = Settings::load();
                let procs_audio = crate::audio::audio_processes();
                if tick % 10 == 0 {
                    bt = crate::bluetooth::bt_info();
                }
                {
                    let mut e = eng.lock().unwrap();
                    e.sample(procs_audio, bt.clone());
                    crate::engine::publish(&e.snap);
                    let new_alerts = alerts.check(&e.snap, &settings);
                    if !new_alerts.is_empty() {
                        ALERTS
                            .get_or_init(|| Mutex::new(Vec::new()))
                            .lock()
                            .unwrap()
                            .extend(new_alerts);
                    }
                    if tick % 5 == 0 {
                        crate::history::record(&e.snap);
                    }
                    tick += 1;
                }
                thread::sleep(Duration::from_millis(2000));
            }
        })
        .expect("sampler thread");
}

/// Called once per UI frame-pump (~500ms) from main: applies toggles and
/// pushes fresh numbers into the menu bar.
pub fn pump_status_bar(
    bar: &mut crate::statusbar::StatusBar,
    view: &gpui_kit::Entity<crate::ui::TideGaugeView>,
    cx: &mut gpui_kit::App,
) {
    use crate::statusbar::*;
    use std::sync::atomic::Ordering;
    let toggled = TOGGLE_POPOVER.swap(false, Ordering::SeqCst);
    let outside = CLICKED_OUTSIDE.swap(false, Ordering::SeqCst);
    if toggled {
        toggle_popover(view.clone(), cx);
    }
    // A press on the item itself also counts as outside on newer macOS; the
    // toggle above already handled that click, so only a real outside click
    // closes the panel.
    if outside && !toggled && !outside_click_was_on_item() {
        close_popover(cx);
    }
    if TOGGLE_PANEL.swap(false, Ordering::SeqCst) {
        close_popover(cx);
        view.update(cx, |view, cx| {
            view.show_settings = false;
            cx.notify();
        });
        toggle_panel(cx);
    }
    if SHOW_SETTINGS.swap(false, Ordering::SeqCst) {
        view.update(cx, |view, cx| {
            view.show_settings = true;
            view.scroll = gpui_kit::ScrollHandle::new();
            view.settings_error = None;
            cx.notify();
        });
        toggle_panel(cx);
    }
    if QUIT_APP.swap(false, Ordering::SeqCst) {
        cx.quit();
    }
    let settings = Settings::load();
    let snap = crate::engine::latest();
    bar.sync(&settings, &snap);
    for a in drain_alerts(&settings) {
        crate::notifications::send(&a.message);
    }
}

static PANEL: OnceLock<Mutex<Option<gpui_kit::AnyWindowHandle>>> = OnceLock::new();
static POPOVER: Mutex<Option<gpui_kit::AnyWindowHandle>> = Mutex::new(None);

/// Open the menu-bar panel under the item, or close it if it is open.
pub fn toggle_popover(main: gpui_kit::Entity<crate::ui::TideGaugeView>, cx: &mut gpui_kit::App) {
    use gpui_kit::*;
    if POPOVER.lock().unwrap().is_some() {
        close_popover(cx);
        return;
    }
    let Some(anchor) = crate::statusbar::anchor() else {
        // Without a known position, the menu instead.
        crate::statusbar::SHOW_ITEM_MENU.store(true, std::sync::atomic::Ordering::SeqCst);
        return;
    };
    let (width, height) = (crate::menu_panel::WIDTH as f64, crate::menu_panel::height() as f64);
    let left = (anchor.left + anchor.width / 2. - width / 2.).clamp(8., (anchor.display_width - width - 8.).max(8.));
    let bounds = Bounds::new(
        point(px(left as f32), px((anchor.bottom + 6.) as f32)),
        size(px(width as f32), px(height as f32)),
    );
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            kind: WindowKind::PopUp,
            focus: true,
            show: true,
            is_movable: false,
            titlebar: None,
            display_id: Some(DisplayId::from(anchor.display)),
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| crate::menu_panel::MenuPanel::new(window, cx, main)),
    );
    match opened {
        Ok(handle) => *POPOVER.lock().unwrap() = Some(handle.into()),
        // A click must always do something: without the panel, the menu.
        Err(_) => crate::statusbar::SHOW_ITEM_MENU.store(true, std::sync::atomic::Ordering::SeqCst),
    }
}

pub fn close_popover(cx: &mut gpui_kit::App) {
    if let Some(handle) = POPOVER.lock().unwrap().take() {
        // Deferred: the panel may be the one asking (its own button or
        // Escape), and a window can't be updated while it handles an event.
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
    }
}

pub fn set_panel_handle(h: gpui_kit::AnyWindowHandle) {
    *PANEL.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(h);
}

pub fn toggle_panel(cx: &mut gpui_kit::App) {
    cx.activate(true);
    let g = PANEL.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some(h) = g.as_ref() {
        let h2 = h.clone();
        let _ = h2.update(cx, |_, window, _| {
            window.activate_window();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::still_enabled;
    use crate::settings::Settings;

    #[test]
    fn every_alert_kind_reaches_delivery_while_its_switch_is_on() {
        let on = Settings {
            alert_cpu_enabled: true,
            alert_mem_enabled: true,
            alert_disk_enabled: true,
            alert_network_enabled: true,
            alert_app_enabled: true,
            alert_app_mem_enabled: true,
            alert_low_disk_enabled: true,
            alert_thermal_enabled: true,
            alert_pressure_enabled: true,
            alert_temp_enabled: true,
            alert_battery_low_enabled: true,
            alert_battery_full_enabled: true,
            alert_disk_health_enabled: true,
            alert_offline_enabled: true,
            ..Settings::default()
        };
        for metric in ["cpu", "memory", "disk", "network", "app:com.example.busy", "appmem:com.example.busy", "low_disk", "thermal", "pressure", "temperature", "battery_low", "battery_full", "disk_health:disk0", "offline", "online"] {
            assert!(still_enabled(metric, &on), "{metric} was dropped");
            assert!(!still_enabled(metric, &Settings::default()), "{metric} passed while off");
        }
        assert!(!still_enabled("unknown", &on));
    }
}
