//! Persisted settings: menu-bar items, theme, thresholds.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct MenuItem {
    pub metric: String, // cpu|memory|disk|network|battery|gpu|power
    pub style: String,  // value|graph|both
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub dark: Option<bool>, // None = follow system
    pub menu_items: Vec<MenuItem>,
    pub alert_cpu_pct: f64,
    pub alert_mem_pct: f64,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: None,
            menu_items: vec![
                MenuItem { metric: "cpu".into(), style: "both".into() },
                MenuItem { metric: "memory".into(), style: "value".into() },
                MenuItem { metric: "network".into(), style: "value".into() },
            ],
            alert_cpu_pct: 90.0,
            alert_mem_pct: 92.0,
            launch_at_login: false,
        }
    }
}

fn path() -> PathBuf {
    let d = directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/tmp/tidegauge"));
    let _ = std::fs::create_dir_all(&d);
    d.join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path(), b);
        }
    }
}

/// Shared mutable settings: menu-bar actions mutate, pump/UI read.
static STORE: std::sync::OnceLock<std::sync::Mutex<Settings>> = std::sync::OnceLock::new();

fn store() -> &'static std::sync::Mutex<Settings> {
    STORE.get_or_init(|| std::sync::Mutex::new(Settings::load()))
}

pub fn get() -> Settings {
    store().lock().unwrap().clone()
}

/// Mutate the store and persist.
pub fn mutate(f: impl FnOnce(&mut Settings)) {
    let mut s = store().lock().unwrap();
    f(&mut s);
    s.save();
}

/// Signature of everything that changes menu structure — used to know when
/// menus must be rebuilt.
pub fn menu_signature(s: &Settings) -> String {
    format!(
        "{:?}|{}",
        s.menu_items
            .iter()
            .map(|m| (m.metric.clone(), m.style.clone()))
            .collect::<Vec<_>>(),
        s.dark.map(|d| if d { "dark" } else { "light" }).unwrap_or("auto"),
    )
}
