#![cfg_attr(target_os = "macos", allow(non_snake_case))]

mod app;
mod audio;
mod bluetooth;
mod engine;
mod ffi;
mod history;
mod icons;
mod metrics;
mod processes;
mod projects;
mod settings;
mod statusbar;
mod theme;
mod ui;

use gpui_kit::*;
use std::time::Duration;

fn main() {
    env_logger_init();
    app::start_sampler();

    let app = gpui_kit::application().with_assets(icons::TideGaugeAssets::new());

    app.run(|cx| {
        gpui_kit::init(cx);
        #[cfg(target_os = "macos")]
        {
            // Hide Dock icon & menu bar — we're a status item app.
            use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
            unsafe {
                NSApplication::sharedApplication(objc2_foundation::MainThreadMarker::new_unchecked())
                    .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            }
        }

        let bounds = Bounds::centered(None, size(px(860.), px(640.)), cx);
        let (window, view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: WindowKind::PopUp,
                focus: true,
                show: true,
                is_movable: true,
                titlebar: None,
                window_min_size: Some(size(px(700.), px(500.))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut v = ui::TideGaugeView::new(window, cx);
                    v.snap = engine::latest();
                    v
                })
            },
        )
        .expect("open panel");
        app::set_panel_handle(window);
        let _ = view;

        #[cfg(target_os = "macos")]
        {
            let mut bar = statusbar::StatusBar::new();
            // status bar pump: refresh readouts + handle toggles
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    smol::Timer::after(Duration::from_millis(700)).await;
                    let _ = cx.update(|cx: &mut App| app::pump_status_bar(&mut bar, cx));
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
