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
    pub alert_cpu_enabled: bool,
    pub alert_mem_enabled: bool,
    pub alert_disk_enabled: bool,
    pub alert_network_enabled: bool,
    /// Alert when one app stays above `alert_app_cpu_pct` (of one core) for
    /// five minutes.
    pub alert_app_enabled: bool,
    pub alert_app_cpu_pct: f64,
    /// Alert when one app holds more than `alert_app_mem_gb` for five minutes.
    pub alert_app_mem_enabled: bool,
    pub alert_app_mem_gb: f64,
    /// Alert when the startup disk has less than 10 GB free.
    pub alert_low_disk_enabled: bool,
    /// Alert when macOS reports a serious or critical thermal state.
    pub alert_thermal_enabled: bool,
    /// Alert when macOS reports warning or critical memory pressure.
    pub alert_pressure_enabled: bool,
    pub launch_at_login: bool,
    pub dashboard_tabs: Vec<String>,
    pub favorite_metrics: Vec<String>,
    pub compact_dashboard: bool,
    /// The app's language: "en", "nb", or `None` to follow macOS.
    pub language: Option<String>,
    /// The keyboard shortcut that opens the menu-bar panel, one of
    /// `shortcut::CHOICES`, or `None` for none.
    pub panel_shortcut: Option<String>,
    /// "Don't ask again" on the Move to Applications prompt.
    pub move_prompt_declined: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: None,
            show_app_icon: true,
            menu_items: Vec::new(),
            alert_cpu_pct: 90.0,
            alert_mem_pct: 92.0,
            alert_cpu_enabled: false,
            alert_mem_enabled: false,
            alert_disk_enabled: false,
            alert_network_enabled: false,
            alert_app_enabled: false,
            alert_app_cpu_pct: APP_CPU_DEFAULT,
            alert_app_mem_enabled: false,
            alert_app_mem_gb: APP_MEMORY_DEFAULT,
            alert_low_disk_enabled: false,
            alert_thermal_enabled: false,
            alert_pressure_enabled: false,
            launch_at_login: false,
            dashboard_tabs: DASHBOARD_TABS.iter().map(|s| (*s).into()).collect(),
            favorite_metrics: FAVORITE_METRICS.iter().map(|s| (*s).into()).collect(),
            compact_dashboard: false,
            language: None,
            panel_shortcut: None,
            move_prompt_declined: false,
        }
    }
}

fn path() -> PathBuf {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let d = directories::ProjectDirs::from("no", "ogard", "tidegauge")
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/tmp/tidegauge"));
        let _ = std::fs::create_dir_all(&d);
        d.join("settings.json")
    })
    .clone()
}

/// The file's identity when it was last read: unchanged identity means the
/// parsed settings can be reused. Drawing the window asks for settings many
/// times a frame, so this saves a read and a parse each time.
type Stamp = (Option<std::time::SystemTime>, u64);
static CACHE: std::sync::Mutex<Option<(Stamp, Settings)>> = std::sync::Mutex::new(None);

fn stamp(path: &std::path::Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok(), metadata.len()))
}

impl Settings {
    pub fn load() -> Self {
        let path = path();
        let current = stamp(&path);
        let mut cache = CACHE.lock().unwrap();
        if let (Some(current), Some((cached, settings))) = (current, cache.as_ref()) {
            if *cached == current {
                return settings.clone();
            }
        }
        let mut settings: Self = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        settings.normalize();
        *cache = current.map(|current| (current, settings.clone()));
        settings
    }
    pub fn save(&self) -> std::io::Result<()> {
        let result = self.write_to(&path());
        // Read the saved file afresh next time, whatever its timestamp says.
        *CACHE.lock().unwrap() = None;
        result
    }

    pub fn normalize(&mut self) {
        self.alert_cpu_pct = valid_threshold(self.alert_cpu_pct, 90.);
        self.alert_mem_pct = valid_threshold(self.alert_mem_pct, 92.);
        self.alert_app_cpu_pct = valid_app_threshold(self.alert_app_cpu_pct);
        self.alert_app_mem_gb = valid_app_memory_gb(self.alert_app_mem_gb);
        normalize_choices(&mut self.dashboard_tabs, &DASHBOARD_TABS);
        normalize_choices(&mut self.favorite_metrics, &FAVORITE_METRICS);
    }

    pub fn reset_dashboard(&mut self) {
        self.dashboard_tabs = DASHBOARD_TABS.iter().map(|s| (*s).into()).collect();
        self.favorite_metrics = FAVORITE_METRICS.iter().map(|s| (*s).into()).collect();
        self.compact_dashboard = false;
    }

    fn write_to(&self, destination: &std::path::Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self)?;
        let temporary = destination.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, destination)
    }

    pub fn set_menu_style(&mut self, metric: &str, style: Option<&str>) {
        if !MENU_METRICS.iter().any(|(key, _)| *key == metric) {
            return;
        }
        if style.is_some_and(|style| !MENU_STYLES.iter().any(|(key, _)| *key == style)) {
            return;
        }
        self.menu_items.retain(|item| item.metric != metric);
        if let Some(style) = style {
            self.menu_items.push(MenuItem {
                metric: metric.into(),
                style: style.into(),
            });
        }
        self.menu_items
            .sort_by_key(|item| MENU_METRICS.iter().position(|(key, _)| *key == item.metric));
    }
}

pub const DASHBOARD_TABS: [&str; 10] = [
    "cpu",
    "memory",
    "disk",
    "network",
    "gpu",
    "battery",
    "sound",
    "bluetooth",
    "projects",
    "history",
];
pub const FAVORITE_METRICS: [&str; 6] = ["cpu", "memory", "gpu", "disk", "network", "battery"];

pub fn valid_threshold(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(1., 100.)
    } else {
        fallback
    }
}

pub const APP_CPU_DEFAULT: f64 = 80.;
pub const APP_CPU_RANGE: (f64, f64) = (20., 400.);

/// An app's CPU can pass 100 % across cores, so its threshold runs to 400 %.
pub fn valid_app_threshold(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(APP_CPU_RANGE.0, APP_CPU_RANGE.1)
    } else {
        APP_CPU_DEFAULT
    }
}

pub const APP_MEMORY_DEFAULT: f64 = 4.;
pub const APP_MEMORY_RANGE: (f64, f64) = (1., 64.);

pub fn valid_app_memory_gb(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(APP_MEMORY_RANGE.0, APP_MEMORY_RANGE.1)
    } else {
        APP_MEMORY_DEFAULT
    }
}

fn normalize_choices(choices: &mut Vec<String>, supported: &[&str]) {
    let mut seen = std::collections::HashSet::new();
    choices.retain(|s| supported.contains(&s.as_str()) && seen.insert(s.clone()));
}

pub const MENU_METRICS: [(&str, &str); 7] = [
    ("cpu", "CPU"),
    ("memory", "Memory"),
    ("gpu", "GPU"),
    ("disk", "Disk"),
    ("network", "Network"),
    ("battery", "Battery"),
    ("power", "Power"),
];
pub const MENU_STYLES: [(&str, &str); 4] = [
    ("icon", "Icon"),
    ("value", "Value"),
    ("graph", "Graph"),
    ("both", "Value + graph"),
];

pub fn localized_metric_label(metric: &str) -> Option<&'static str> {
    use crate::locale::text as l;
    Some(match metric {
        "cpu" => l("CPU", "CPU"),
        "memory" => l("Memory", "Minne"),
        "gpu" => l("GPU", "GPU"),
        "disk" => l("Disk", "Disk"),
        "network" => l("Network", "Nettverk"),
        "battery" => l("Battery", "Batteri"),
        "power" => l("Power", "Effekt"),
        _ => return None,
    })
}

pub fn localized_style_label(style: &str) -> Option<&'static str> {
    use crate::locale::text as l;
    Some(match style {
        "icon" => l("Icon", "Ikon"),
        "value" => l("Value", "Verdi"),
        "graph" => l("Graph", "Graf"),
        "both" => l("Value + graph", "Verdi + graf"),
        _ => return None,
    })
}

/// Read the latest file before changing one setting, preserving other preferences.
pub fn mutate(f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
    let mut settings = Settings::load();
    f(&mut settings);
    settings.normalize();
    settings.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roadmap_settings_migrate_normalize_and_reset_independently() {
        let mut settings: Settings = serde_json::from_str(
            r#"{"dark":true,"alert_cpu_pct":73,"menu_items":[{"metric":"cpu","style":"value"}]}"#,
        )
        .unwrap();
        assert!(!settings.alert_cpu_enabled);
        assert_eq!(settings.dashboard_tabs.len(), 10);
        settings.dashboard_tabs = vec![
            "history".into(),
            "cpu".into(),
            "history".into(),
            "invalid".into(),
        ];
        settings.favorite_metrics.clear();
        settings.alert_mem_pct = f64::NAN;
        settings.normalize();
        assert_eq!(settings.dashboard_tabs, ["history", "cpu"]);
        assert!(settings.favorite_metrics.is_empty());
        assert_eq!(settings.alert_mem_pct, 92.);
        let loaded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded.dashboard_tabs, settings.dashboard_tabs);
        settings.reset_dashboard();
        assert_eq!(settings.favorite_metrics.len(), 6);
        assert_eq!(settings.dark, Some(true));
        assert_eq!(settings.alert_cpu_pct, 73.);
        assert_eq!(settings.menu_items[0].style, "value");
    }

    #[test]
    fn appearance_round_trip_preserves_menu_items_and_login_preference() {
        for dark in [None, Some(false), Some(true)] {
            let mut settings = Settings {
                dark,
                launch_at_login: true,
                ..Settings::default()
            };
            settings.set_menu_style("cpu", Some("both"));
            let loaded: Settings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert_eq!(loaded.dark, dark);
            assert!(loaded.launch_at_login);
            assert_eq!(loaded.menu_items.len(), 1);
            assert_eq!(loaded.menu_items[0].metric, "cpu");
            assert_eq!(loaded.menu_items[0].style, "both");
        }
    }

    #[test]
    fn default_menu_bar_is_icon_only() {
        let settings = Settings::default();
        assert!(settings.show_app_icon);
        assert!(settings.menu_items.is_empty());
    }

    #[test]
    fn old_settings_keep_defaults_and_metric_changes_do_not_duplicate_items() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"menu_items":[{"metric":"cpu","style":"value"}]}"#).unwrap();
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
        let destination = std::env::temp_dir().join(format!(
            "tidegauge-settings-test-{}.json",
            std::process::id()
        ));
        let mut settings = Settings {
            alert_cpu_pct: 73.,
            ..Settings::default()
        };
        settings.set_menu_style("gpu", Some("graph"));
        settings.write_to(&destination).unwrap();
        let loaded: Settings =
            serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
        assert_eq!(loaded.alert_cpu_pct, 73.);
        assert!(loaded
            .menu_items
            .iter()
            .any(|item| item.metric == "gpu" && item.style == "graph"));
        assert!(!destination.with_extension("json.tmp").exists());
        std::fs::remove_file(destination).unwrap();
    }
}
