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
    alerts.retain(|alert| match alert.metric.as_str() {
        "cpu" => settings.alert_cpu_enabled,
        "memory" => settings.alert_mem_enabled,
        "disk" => settings.alert_disk_enabled,
        "network" => settings.alert_network_enabled,
        _ => false,
    });
    alerts
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
    if TOGGLE_PANEL.swap(false, Ordering::SeqCst) {
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
