#![cfg_attr(target_os = "macos", allow(non_snake_case))]

mod agents;
mod agents_panel;
mod alerts;
mod app;
mod app_cpu;
mod app_volume;
mod appearance;
mod audio;
mod battery;
mod baselines;
mod bluetooth;
mod caches;
mod clocks;
mod controls;
mod diagnose;
mod disks;
mod engine;
mod ffi;
mod history;
mod history_browser;
mod gpu_power;
mod icons;
mod instance;
mod leaks;
mod keepawake;
mod locale;
#[cfg(test)]
mod locale_guard;
mod login;
mod menu_panel;
mod menu_style;
mod metrics;
mod move_to_applications;
mod named_sensors;
mod network;
mod notifications;
mod packages;
mod processes;
mod process_activity;
mod process_control;
mod process_panel;
mod process_signing;
mod projects;
mod sensors;
mod resource_activity;
mod startup;
mod storage;
mod tools_panel;
mod settings;
mod settings_page;
mod shortcut;
mod slow_check;
mod statusbar;
mod theme;
mod ui;
pub mod updater;

use gpui_kit::*;
use std::time::Duration;

actions!(tidegauge, [Quit]);

fn main() {
    let _instance = match instance::acquire() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            eprintln!("Tidegauge is already running. Use its menu-bar item to open it.");
            return;
        }
        Err(error) => {
            eprintln!("Cannot start Tidegauge: {error}");
            return;
        }
    };
    env_logger_init();
    sensors::start();
    disks::start();
    app::start_sampler();

    let app = gpui_kit::application().with_assets(icons::TideGaugeAssets::new());

    app.run(|cx| {
        gpui_kit::init(cx);
        cx.on_app_quit(|_| async {
            crate::keepawake::shutdown();
            // Apps play on their own again.
            crate::app_volume::stop_all();
        })
        .detach();
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        #[cfg(target_os = "macos")]
        {
            // Hide Dock icon & menu bar — we're a status item app.
            use objc2::AllocAnyThread;
            use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSImage};
            use objc2_foundation::NSData;
            unsafe {
                let application = NSApplication::sharedApplication(
                    objc2_foundation::MainThreadMarker::new_unchecked(),
                );
                application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                let data = NSData::with_bytes(include_bytes!("../assets/logo.svg"));
                if let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) {
                    application.setApplicationIconImage(Some(&icon));
                }
            }
            notifications::initialize();
            updater::initialize();
            // Before any window: on yes, the copy in Applications takes over.
            move_to_applications::offer();
        }

        let bounds = Bounds::centered(None, size(px(1192.), px(780.)), cx);
        let (window, view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: WindowKind::Normal,
                focus: true,
                show: true,
                is_movable: true,
                titlebar: Some(TitlebarOptions {
                    title: Some("Tidegauge".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(26.))),
                }),
                window_min_size: Some(size(px(1050.), px(650.))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                // Closing the dashboard hides it; the menu-bar item reopens it.
                window.on_window_should_close(cx, |_, cx| {
                    cx.hide();
                    false
                });
                cx.new(|cx| {
                    let mut v = ui::TideGaugeView::new(window, cx);
                    v.snap = engine::latest();
                    v
                })
            },
        )
        .expect("open panel");
        app::set_panel_handle(window);

        #[cfg(target_os = "macos")]
        {
            let mut bar = statusbar::StatusBar::new();
            // A shortcut another app has taken since stays off; Settings
            // says so, since what is in effect is read from shortcut::active.
            let _ = shortcut::apply(settings::Settings::load().panel_shortcut.as_deref());
            // status bar pump: refresh readouts + handle toggles
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    smol::Timer::after(Duration::from_millis(700)).await;
                    let _ = cx.update(|cx: &mut App| app::pump_status_bar(&mut bar, &view, cx));
                    // Native menu tracking runs a nested event loop. Release the
                    // GPUI App borrow before allowing other UI tasks to run.
                    if statusbar::SHOW_CONTEXT_MENU.swap(false, std::sync::atomic::Ordering::SeqCst)
                    {
                        statusbar::show_context_menu();
                    }
                    if updater::CHECK_REQUESTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        let _ = updater::request_check();
                    }
                    if statusbar::SHOW_ITEM_MENU.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        let _ = cx.update(app::close_popover);
                        statusbar::show_item_menu();
                    }
                }
            })
            .detach();
        }
    });
}

fn env_logger_init() {
    if std::env::var("RUST_LOG").is_ok() {
        let _ = env_logger_impl();
    }
}
fn env_logger_impl() -> anyhow::Result<()> {
    Ok(())
}
