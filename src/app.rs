//! App orchestration: sampler thread, status bar pump, window lifecycle.

#![allow(dead_code)]

use crate::alerts::AlertEngine;
use crate::engine::{engine, Alert};
use crate::settings::Settings;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

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
        app if app.starts_with("leak:") => settings.alert_leak_enabled,
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

/// A one-slot signal for a loop that would otherwise poll: [`Wake::notify`]
/// from anywhere, and [`Wake::wait`] returns at once, or when the timeout
/// passes. Notifications that arrive together count as one.
pub struct Wake {
    tx: smol::channel::Sender<()>,
    rx: smol::channel::Receiver<()>,
}

impl Wake {
    fn new() -> Self {
        let (tx, rx) = smol::channel::bounded(1);
        Self { tx, rx }
    }

    pub fn notify(&self) {
        let _ = self.tx.try_send(());
    }

    pub async fn wait(&self, timeout: Duration) {
        smol::future::or(
            async {
                let _ = self.rx.recv().await;
            },
            async {
                smol::Timer::after(timeout).await;
            },
        )
        .await;
    }
}

/// Wakes the menu-bar loop: a click on the item, a new sample, new settings.
pub static PUMP: std::sync::LazyLock<Wake> = std::sync::LazyLock::new(Wake::new);
/// Wakes the dashboard's refresh loop when the window is brought forward.
pub static REFRESH: std::sync::LazyLock<Wake> = std::sync::LazyLock::new(Wake::new);

/// Background sampler: light metrics every ~2s; the process list, projects
/// and agents every 2s while a window or the panel is open and every ~8s
/// otherwise; Bluetooth, `nettop`, `lsof` and `diskutil` at the pace
/// `crate::demand` asks for; history write every ~10s.
pub fn start_sampler() {
    thread::Builder::new()
        .name("tidegauge-sampler".into())
        .spawn(move || {
            crate::demand::utility_thread();
            let eng = engine();
            let mut alerts = AlertEngine::new();
            let mut tick = 0u64;
            let mut last_audio: Option<Instant> = None;
            loop {
                let started = Instant::now();
                let settings = Settings::load();
                crate::demand::set_alerts(settings.alert_network_enabled, settings.alert_disk_health_enabled);
                // Audio apps are read for a window or the panel, for an app
                // held at a volume, and now and then otherwise.
                let audio_due = last_audio.is_none_or(|at| {
                    crate::demand::current().audio_due(at.elapsed(), crate::app_volume::active())
                });
                let procs_audio = if audio_due {
                    last_audio = Some(Instant::now());
                    Some(crate::audio::audio_processes())
                } else {
                    None
                };
                let bt = crate::bluetooth::bt_info(crate::demand::current().bluetooth_period());
                {
                    let mut e = eng.lock().unwrap();
                    e.sample(procs_audio, bt);
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
                // The menu-bar item and any alerts follow the new sample.
                PUMP.notify();
                // A steady 2 s from the start of one pass to the next.
                thread::sleep(TICK.saturating_sub(started.elapsed()));
            }
        })
        .expect("sampler thread");
}

const TICK: Duration = Duration::from_millis(2000);

/// Called by the menu-bar loop in main on each wake (see [`PUMP`]): applies
/// toggles and pushes fresh numbers into the menu bar.
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
            view.reset_settings_scroll();
            view.scroll = gpui_kit::ScrollHandle::new();
            view.settings_error = None;
            cx.notify();
        });
        toggle_panel(cx);
    }
    if SHOW_ABOUT.swap(false, Ordering::SeqCst) {
        view.update(cx, |view, cx| {
            view.show_settings = true;
            view.settings_error = None;
            view.scroll_settings_to_about();
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
        crate::statusbar::raise(&crate::statusbar::SHOW_ITEM_MENU);
        return;
    };
    let see_through = crate::menu_panel::see_through(&Settings::load());
    // Decided now and handed to the panel, so the window's size and what it
    // draws always agree.
    let layout = crate::menu_panel::Layout::now();
    let (width, height) = (crate::menu_panel::WIDTH as f64, layout.height() as f64);
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
            // Frosted when see-through; the panel has no frame of its own, so
            // its corners are clipped to match its rounded body.
            window_background: if see_through {
                WindowBackgroundAppearance::Blurred
            } else {
                WindowBackgroundAppearance::Transparent
            },
            ..Default::default()
        },
        |window, cx| {
            if see_through {
                crate::appearance::round_corners(window, f32::from(crate::theme::RADIUS_CARD) as f64);
            }
            cx.new(|cx| crate::menu_panel::MenuPanel::new(window, cx, main, layout))
        },
    );
    match opened {
        Ok(handle) => *POPOVER.lock().unwrap() = Some(handle.into()),
        // A click must always do something: without the panel, the menu.
        Err(_) => crate::statusbar::raise(&crate::statusbar::SHOW_ITEM_MENU),
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
    REFRESH.notify();
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
    use super::{still_enabled, Wake};
    use std::time::{Duration, Instant};

    #[test]
    fn a_notified_wake_returns_at_once_and_counts_once() {
        let wake = Wake::new();
        wake.notify();
        wake.notify();
        let start = Instant::now();
        smol::block_on(wake.wait(Duration::from_secs(30)));
        assert!(start.elapsed() < Duration::from_secs(5));
        // The second notify was folded into the first: this one times out.
        let start = Instant::now();
        smol::block_on(wake.wait(Duration::from_millis(80)));
        assert!(start.elapsed() >= Duration::from_millis(70));
    }

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
            alert_leak_enabled: true,
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
        for metric in ["cpu", "memory", "disk", "network", "app:com.example.busy", "appmem:com.example.busy", "leak:com.example.busy", "low_disk", "thermal", "pressure", "temperature", "battery_low", "battery_full", "disk_health:disk0", "offline", "online"] {
            assert!(still_enabled(metric, &on), "{metric} was dropped");
            assert!(!still_enabled(metric, &Settings::default()), "{metric} passed while off");
        }
        assert!(!still_enabled("unknown", &on));
    }
}
