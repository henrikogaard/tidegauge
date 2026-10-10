//! Persisted settings: menu-bar items, theme, thresholds.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct MenuItem {
    pub metric: String, // cpu|memory|disk|network|battery|gpu|power
    pub style: String,  // icon|value|graph|both
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub dark: Option<bool>, // None = follow system
    pub show_app_icon: bool,
    pub menu_items: Vec<MenuItem>,
    pub alert_cpu_pct: f64,
    pub alert_mem_pct: f64,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: None,
            show_app_icon: true,
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
    pub fn save(&self) -> std::io::Result<()> {
        self.write_to(&path())
    }

    fn write_to(&self, destination: &std::path::Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self)?;
        let temporary = destination.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, destination)
    }

    pub fn set_menu_style(&mut self, metric: &str, style: Option<&str>) {
        if !MENU_METRICS.iter().any(|(key, _)| *key == metric) { return; }
        if style.is_some_and(|style| !MENU_STYLES.iter().any(|(key, _)| *key == style)) { return; }
        self.menu_items.retain(|item| item.metric != metric);
        if let Some(style) = style {
            self.menu_items.push(MenuItem { metric: metric.into(), style: style.into() });
        }
        self.menu_items.sort_by_key(|item| MENU_METRICS.iter().position(|(key, _)| *key == item.metric));
    }
}

pub const MENU_METRICS: [(&str, &str); 7] = [
    ("cpu", "CPU"), ("memory", "Memory"), ("gpu", "GPU"), ("disk", "Disk"),
    ("network", "Network"), ("battery", "Battery"), ("power", "Power"),
];
pub const MENU_STYLES: [(&str, &str); 4] = [
    ("icon", "Icon"), ("value", "Value"), ("graph", "Graph"), ("both", "Value + graph"),
];

/// Read the latest file before changing one setting, preserving other preferences.
pub fn mutate(f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
    let mut settings = Settings::load();
    f(&mut settings);
    settings.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_keep_defaults_and_metric_changes_do_not_duplicate_items() {
        let mut settings: Settings = serde_json::from_str(r#"{"menu_items":[{"metric":"cpu","style":"value"}]}"#).unwrap();
        assert!(settings.show_app_icon);
        settings.set_menu_style("cpu", Some("icon"));
        settings.set_menu_style("cpu", Some("both"));
        assert_eq!(settings.menu_items.len(), 1);
        assert_eq!(settings.menu_items[0].style, "both");
        settings.set_menu_style("cpu", Some("invalid"));
        assert_eq!(settings.menu_items[0].style, "both");
        settings.set_menu_style("cpu", None);
        assert!(settings.menu_items.is_empty());
    }

    #[test]
    fn settings_round_trip_preserves_other_preferences() {
        let destination = std::env::temp_dir().join(format!("tidegauge-settings-test-{}.json", std::process::id()));
        let mut settings = Settings { alert_cpu_pct: 73., ..Settings::default() };
        settings.set_menu_style("gpu", Some("graph"));
        settings.write_to(&destination).unwrap();
        let loaded: Settings = serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
        assert_eq!(loaded.alert_cpu_pct, 73.);
        assert!(loaded.menu_items.iter().any(|item| item.metric == "gpu" && item.style == "graph"));
        assert!(!destination.with_extension("json.tmp").exists());
        std::fs::remove_file(destination).unwrap();
    }
}
