//! App orchestration: sampler thread, status bar pump, window lifecycle.

#![allow(dead_code)]

use crate::engine::{engine, Alert};
use crate::history::AlertEngine;
use crate::settings::Settings;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

/// Latest un-notified alerts, drained by the UI/notification layer.
static ALERTS: OnceLock<Mutex<Vec<Alert>>> = OnceLock::new();

pub fn drain_alerts() -> Vec<Alert> {
    std::mem::take(&mut *ALERTS.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap())
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
                let procs_audio = crate::audio::audio_processes();
                if tick % 10 == 0 {
                    bt = crate::bluetooth::bt_info();
                }
                let projects = {
                    // rebuild proc list for project detection (cheap reuse)
                    let pids = crate::processes::list_pids();
                    let mut procs = Vec::new();
                    for pid in &pids {
                        if let Some(b) = crate::processes::proc_bsd(*pid) {
                            if let Some(r) = crate::processes::proc_rusage(*pid) {
                                procs.push(crate::processes::ProcInfo {
                                    pid: *pid,
                                    ppid: b.pbi_ppid as i32,
                                    uid: b.pbi_uid,
                                    comm: unsafe {
                                        std::ffi::CStr::from_ptr(b.pbi_comm.as_ptr())
                                            .to_string_lossy()
                                            .to_string()
                                    },
                                    exe_path: crate::processes::proc_path(*pid),
                                    start_unix: b.pbi_start_tvsec as f64,
                                    ..Default::default()
                                });
                                let _ = r;
                            }
                        }
                    }
                    crate::projects::detect_projects(&procs)
                };
                {
                    let mut e = eng.lock().unwrap();
                    e.sample(procs_audio, bt.clone(), projects);
                    let new_alerts = alerts.check(&e.snap);
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
pub fn pump_status_bar(bar: &mut crate::statusbar::StatusBar, cx: &mut gpui_kit::App) {
    use crate::statusbar::*;
    use std::sync::atomic::Ordering;
    if TOGGLE_PANEL.swap(false, Ordering::SeqCst) {
        toggle_panel(cx);
    }
    if QUIT_APP.swap(false, Ordering::SeqCst) {
        cx.quit();
    }
    let settings = Settings::load();
    let snap = crate::engine::latest();
    let dark = crate::statusbar::menu_bar_dark();
    bar.sync(&settings, &snap, dark);
    for a in drain_alerts() {
        notify(&a.message);
    }
}

static PANEL: OnceLock<Mutex<Option<gpui_kit::AnyWindowHandle>>> = OnceLock::new();

pub fn set_panel_handle(h: gpui_kit::AnyWindowHandle) {
    *PANEL.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(h);
}

pub fn toggle_panel(cx: &mut gpui_kit::App) {
    let g = PANEL.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some(h) = g.as_ref() {
        let h2 = h.clone();
        let _ = h2.update(cx, |_, window, _| {
            window.activate_window();
        });
    }
}

/// macOS notification for alerts (best-effort via osascript to keep it
/// dependency-free and unsigned-binary safe).
fn notify(msg: &str) {
    let _ = std::process::Command::new("osascript")
        .args([
            "-e",
            &format!(
                "display notification {} with title \"tidegauge\"",
                serde_json::to_string(msg).unwrap_or_else(|_| "\"\"".into())
            ),
        ])
        .spawn();
}
