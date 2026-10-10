#![cfg_attr(target_os = "macos", allow(non_snake_case))]

mod app;
mod battery;
mod audio;
mod bluetooth;
mod engine;
mod ffi;
mod history;
mod icons;
mod instance;
mod locale;
mod metrics;
mod processes;
mod projects;
mod settings;
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
        Ok(None) => { eprintln!("Tidegauge is already running. Use its menu-bar item to open it."); return; }
        Err(error) => { eprintln!("Cannot start Tidegauge: {error}"); return; }
    };
    env_logger_init();
    app::start_sampler();

    let app = gpui_kit::application().with_assets(icons::TideGaugeAssets::new());

    app.run(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        #[cfg(target_os = "macos")]
        {
            // Hide Dock icon & menu bar — we're a status item app.
            use objc2::AllocAnyThread;
            use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSImage};
            use objc2_foundation::NSData;
            unsafe {
                let application = NSApplication::sharedApplication(objc2_foundation::MainThreadMarker::new_unchecked());
                application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                let data = NSData::with_bytes(include_bytes!("../assets/logo.png"));
                if let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) {
                    application.setApplicationIconImage(Some(&icon));
                }
            }
            updater::initialize();
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
            // status bar pump: refresh readouts + handle toggles
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    smol::Timer::after(Duration::from_millis(700)).await;
                    let _ = cx.update(|cx: &mut App| app::pump_status_bar(&mut bar, &view, cx));
                    // Native menu tracking runs a nested event loop. Release the
                    // GPUI App borrow before allowing other UI tasks to run.
                    if statusbar::SHOW_CONTEXT_MENU.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        statusbar::show_context_menu();
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
