//! Persisted settings: menu-bar items, theme, thresholds.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct MenuItem {
    pub metric: String, // cpu|memory|disk|network|battery|gpu|power
    pub style: String,  // icon|value|graph|both|bars|pie|gauge, see `style_fits`
}

/// How much of the dashboard window lets the desktop show through.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Transparency {
    #[default]
    Off,
    /// Only the sidebar.
    Sidebar,
    /// The window's ground and sidebar; cards stay mostly opaque.
    Window,
}

/// Where metric colours come from (Settings → Colors). The transform lives in
/// `theme::Look`; warning and error colours ignore it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// One colour per metric.
    #[default]
    Tidegauge,
    /// Every metric in the macOS accent colour.
    Macos,
    /// Every metric in one muted blue-gray.
    Calm,
}

/// How sparklines and area charts draw a series (Settings → Charts).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChartStyle {
    Line,
    #[default]
    Filled,
    Bars,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub dark: Option<bool>, // None = follow system
    pub colors: ColorMode,
    pub charts: ChartStyle,
    pub show_app_icon: bool,
    /// Menu-bar readings in each metric's accent colours, turning orange and
    /// red at warning levels. Off: the menu bar's own ink.
    pub menu_colored: bool,
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
    /// Alert when one app's memory has climbed steadily for three hours.
    pub alert_leak_enabled: bool,
    /// Alert when the startup disk has less than 10 GB free.
    pub alert_low_disk_enabled: bool,
    /// Alert when macOS reports a serious or critical thermal state.
    pub alert_thermal_enabled: bool,
    /// Alert when macOS reports warning or critical memory pressure.
    pub alert_pressure_enabled: bool,
    /// Alert when the chip's hottest sensor stays above `alert_temp_c`.
    pub alert_temp_enabled: bool,
    pub alert_temp_c: f64,
    /// Alert when the charge falls to `alert_battery_low_pct` on battery.
    pub alert_battery_low_enabled: bool,
    pub alert_battery_low_pct: f64,
    /// Alert once when the battery reaches full on the adapter.
    pub alert_battery_full_enabled: bool,
    /// Alert when an internal disk reports failing health.
    pub alert_disk_health_enabled: bool,
    /// Alert when the Mac has had no usable network path for ten seconds, and
    /// again when it is back.
    pub alert_offline_enabled: bool,
    pub launch_at_login: bool,
    pub dashboard_tabs: Vec<String>,
    pub favorite_metrics: Vec<String>,
    /// The tiles in the menu-bar panel, in order; any of `PANEL_TILES`.
    pub panel_tiles: Vec<String>,
    /// Show the AI plans section in the menu-bar panel. Plans that are not
    /// updating are left out whatever this says.
    pub panel_show_plans: bool,
    /// Show the Busiest apps section in the menu-bar panel.
    pub panel_show_apps: bool,
    /// App CPU as a share of one core (100 % is one full core, like Activity
    /// Monitor) instead of a share of the whole Mac.
    pub app_cpu_per_core: bool,
    pub compact_dashboard: bool,
    /// The app's language: "en", "nb", or `None` to follow macOS.
    pub language: Option<String>,
    /// The keyboard shortcut that opens the menu-bar panel, one of
    /// `shortcut::CHOICES`, or `None` for none.
    pub panel_shortcut: Option<String>,
    /// "Don't ask again" on the Move to Applications prompt.
    pub move_prompt_declined: bool,
    /// The first-run welcome on Overview has been dismissed. A settings file
    /// from before the welcome existed lacks this key and counts as
    /// dismissed: only a first launch, with no file, shows it.
    #[serde(default = "already_welcomed")]
    pub welcomed: bool,
    /// Where the dashboard window is see-through.
    pub transparency: Transparency,
    /// Whether the menu-bar panel is see-through.
    pub panel_transparency: bool,
    /// How see-through, in percent; shared by the window and the panel.
    pub transparency_amount: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: None,
            colors: ColorMode::default(),
            charts: ChartStyle::default(),
            show_app_icon: true,
            menu_colored: false,
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
            alert_leak_enabled: false,
            alert_low_disk_enabled: false,
            alert_thermal_enabled: false,
            alert_pressure_enabled: false,
            alert_temp_enabled: false,
            alert_temp_c: CHIP_TEMP_DEFAULT,
            alert_battery_low_enabled: false,
            alert_battery_low_pct: BATTERY_LOW_DEFAULT,
            alert_battery_full_enabled: false,
            alert_disk_health_enabled: false,
            alert_offline_enabled: false,
            launch_at_login: false,
            dashboard_tabs: DASHBOARD_TABS.iter().map(|s| (*s).into()).collect(),
            favorite_metrics: DEFAULT_FAVORITES.iter().map(|s| (*s).into()).collect(),
            panel_tiles: DEFAULT_PANEL_TILES.iter().map(|s| (*s).into()).collect(),
            panel_show_plans: true,
            panel_show_apps: true,
            app_cpu_per_core: false,
            compact_dashboard: false,
            language: None,
            panel_shortcut: None,
            move_prompt_declined: false,
            welcomed: false,
            transparency: Transparency::Off,
            panel_transparency: false,
            transparency_amount: TRANSPARENCY_DEFAULT,
        }
    }
}

/// What a settings file without the `welcomed` key means: an existing user.
fn already_welcomed() -> bool {
    true
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

/// How long a read is trusted without even looking at the file again. A frame
/// asks many times within a millisecond; a change made by another process is
/// picked up within this long.
const TRUST: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Default)]
struct Cache {
    entry: Option<(Stamp, std::time::Instant, Settings)>,
}

static CACHE: std::sync::Mutex<Cache> = std::sync::Mutex::new(Cache { entry: None });

fn stamp(path: &std::path::Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok(), metadata.len()))
}

/// The settings in `path`. Only a missing file gives the first-run defaults;
/// a file that exists but cannot be read or parsed belongs to an existing
/// user, so it falls back to defaults that do not show the welcome.
fn read_file(path: &std::path::Path) -> Settings {
    let fallback = || Settings { welcomed: true, ..Settings::default() };
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| fallback()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(_) => fallback(),
    }
}

impl Cache {
    /// The settings in `path`. With `trust_recent` a read from the last
    /// moments is returned without looking at the file; otherwise the file's
    /// identity is checked and the file is parsed again only if it changed.
    fn load(&mut self, path: &std::path::Path, trust_recent: bool) -> Settings {
        if let Some((_, at, settings)) = &self.entry {
            if trust_recent && at.elapsed() < TRUST {
                return settings.clone();
            }
        }
        let current = stamp(path);
        if let (Some(current), Some((cached, at, settings))) = (current, self.entry.as_mut()) {
            if *cached == current {
                *at = std::time::Instant::now();
                return settings.clone();
            }
        }
        let mut settings = read_file(path);
        settings.normalize();
        self.entry = current.map(|current| (current, std::time::Instant::now(), settings.clone()));
        settings
    }
}

impl Settings {
    /// The saved settings, reused for a quarter of a second at a time.
    pub fn load() -> Self {
        CACHE.lock().unwrap().load(&path(), true)
    }
    /// The saved settings as the file holds them now, for a read that is
    /// followed by a write.
    fn load_fresh() -> Self {
        CACHE.lock().unwrap().load(&path(), false)
    }
    pub fn save(&self) -> std::io::Result<()> {
        let result = self.write_to(&path());
        // Read the saved file afresh next time, whatever its timestamp says.
        CACHE.lock().unwrap().entry = None;
        // The menu-bar item may show something else now.
        crate::app::PUMP.notify();
        result
    }

    pub fn normalize(&mut self) {
        self.alert_cpu_pct = valid_threshold(self.alert_cpu_pct, 90.);
        self.alert_mem_pct = valid_threshold(self.alert_mem_pct, 92.);
        self.alert_app_cpu_pct = valid_app_threshold(self.alert_app_cpu_pct);
        self.alert_app_mem_gb = valid_app_memory_gb(self.alert_app_mem_gb);
        self.alert_temp_c = valid_chip_temp(self.alert_temp_c);
        self.alert_battery_low_pct = valid_battery_low(self.alert_battery_low_pct);
        self.transparency_amount = valid_transparency_amount(self.transparency_amount);
        normalize_choices(&mut self.dashboard_tabs, &DASHBOARD_TABS);
        normalize_choices(&mut self.favorite_metrics, &FAVORITE_METRICS);
        normalize_choices(&mut self.panel_tiles, &PANEL_TILES);
    }

    /// Where the dashboard is see-through, given whether macOS has "Reduce
    /// transparency" on, which always wins.
    pub fn window_transparency(&self, reduce_transparency: bool) -> Transparency {
        if reduce_transparency {
            Transparency::Off
        } else {
            self.transparency
        }
    }

    /// Whether the menu-bar panel is see-through, under the same rule.
    pub fn panel_is_transparent(&self, reduce_transparency: bool) -> bool {
        self.panel_transparency && !reduce_transparency
    }

    /// Whether Overview shows the first-run welcome.
    pub fn show_welcome(&self) -> bool {
        !self.welcomed
    }

    pub fn reset_dashboard(&mut self) {
        self.dashboard_tabs = DASHBOARD_TABS.iter().map(|s| (*s).into()).collect();
        self.favorite_metrics = DEFAULT_FAVORITES.iter().map(|s| (*s).into()).collect();
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
        if style.is_some_and(|style| !style_fits(metric, style)) {
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
/// Every card Overview can show, in its order.
pub const FAVORITE_METRICS: [&str; 9] = ["cpu", "memory", "gpu", "disk", "network", "battery", "temperature", "fans", "power"];
/// The cards a new or reset Overview shows.
pub const DEFAULT_FAVORITES: [&str; 6] = ["cpu", "memory", "gpu", "disk", "network", "battery"];
/// Every tile the menu-bar panel can show, in its order.
pub const PANEL_TILES: [&str; 9] = ["cpu", "memory", "network", "disk", "gpu", "battery", "temperature", "fans", "power"];
pub const DEFAULT_PANEL_TILES: [&str; 4] = ["cpu", "memory", "network", "disk"];

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

pub const CHIP_TEMP_DEFAULT: f64 = 95.;
pub const CHIP_TEMP_RANGE: (f64, f64) = (70., 105.);

pub fn valid_chip_temp(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(CHIP_TEMP_RANGE.0, CHIP_TEMP_RANGE.1)
    } else {
        CHIP_TEMP_DEFAULT
    }
}

pub const BATTERY_LOW_DEFAULT: f64 = 20.;
pub const BATTERY_LOW_RANGE: (f64, f64) = (5., 50.);

pub fn valid_battery_low(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(BATTERY_LOW_RANGE.0, BATTERY_LOW_RANGE.1)
    } else {
        BATTERY_LOW_DEFAULT
    }
}

pub const TRANSPARENCY_DEFAULT: f64 = 40.;
pub const TRANSPARENCY_RANGE: (f64, f64) = (0., 100.);

pub fn valid_transparency_amount(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(TRANSPARENCY_RANGE.0, TRANSPARENCY_RANGE.1)
    } else {
        TRANSPARENCY_DEFAULT
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

/// The favorite or panel-tile choices to offer: a Mac macOS says has no
/// battery is not offered a battery card.
pub fn offered_choices(all: &'static [&'static str], battery_absent: bool) -> Vec<&'static &'static str> {
    all.iter().filter(|key| !(battery_absent && **key == "battery")).collect()
}

/// The panel tiles to draw: on a Mac with no battery the battery tile gives
/// way to Power, so a saved choice never leaves the panel empty.
pub fn panel_tiles_shown(saved: &[String], battery_absent: bool) -> Vec<String> {
    let mut shown: Vec<String> = Vec::new();
    for tile in saved {
        let tile = if battery_absent && tile == "battery" { "power" } else { tile.as_str() };
        if !shown.iter().any(|t| t == tile) {
            shown.push(tile.to_string());
        }
    }
    shown
}

/// How many of the saved tiles are on offer, which is what "keep at least
/// one" counts.
pub fn offered_tile_count(saved: &[String], battery_absent: bool) -> usize {
    saved.iter().filter(|t| !(battery_absent && *t == "battery")).count()
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
pub const MENU_STYLES: [(&str, &str); 7] = [
    ("icon", "Icon"),
    ("value", "Value"),
    ("graph", "Graph"),
    ("both", "Value + graph"),
    ("bars", "Bars"),
    ("pie", "Ring"),
    ("gauge", "Gauge"),
];

/// Whether a style suits a metric: bars show each CPU core, a ring shows one
/// percentage (CPU, memory, free disk space, battery) and the gauge is a
/// battery.
pub fn style_fits(metric: &str, style: &str) -> bool {
    match style {
        "icon" | "value" | "graph" | "both" => true,
        "bars" => metric == "cpu",
        "pie" => matches!(metric, "cpu" | "memory" | "disk" | "battery"),
        "gauge" => metric == "battery",
        _ => false,
    }
}

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
        "bars" => l("Bars", "Søyler"),
        "pie" => l("Ring", "Ring"),
        "gauge" => l("Gauge", "Måler"),
        _ => return None,
    })
}

/// Read the latest file before changing one setting, preserving other preferences.
pub fn mutate(f: impl FnOnce(&mut Settings)) -> std::io::Result<()> {
    let mut settings = Settings::load_fresh();
    f(&mut settings);
    settings.normalize();
    settings.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_battery_tile_never_empties_the_panel_without_a_battery() {
        let tiles = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(panel_tiles_shown(&tiles(&["battery"]), true), tiles(&["power"]));
        assert_eq!(panel_tiles_shown(&tiles(&["battery", "cpu"]), true), tiles(&["power", "cpu"]));
        assert_eq!(panel_tiles_shown(&tiles(&["battery", "power"]), true), tiles(&["power"]));
        assert_eq!(panel_tiles_shown(&tiles(&["battery"]), false), tiles(&["battery"]));
    }

    #[test]
    fn the_last_tile_check_counts_only_offered_tiles() {
        let tiles = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(offered_tile_count(&tiles(&["battery", "cpu"]), true), 1);
        assert_eq!(offered_tile_count(&tiles(&["battery", "cpu"]), false), 2);
    }

    #[test]
    fn no_battery_choice_is_offered_without_a_battery() {
        assert!(offered_choices(&FAVORITE_METRICS, false).iter().any(|k| **k == "battery"));
        let without = offered_choices(&PANEL_TILES, true);
        assert!(!without.iter().any(|k| **k == "battery") && without.iter().any(|k| **k == "power"));
        assert_eq!(without.len(), PANEL_TILES.len() - 1);
    }

    #[test]
    fn only_a_missing_file_is_a_first_run() {
        let dir = std::env::temp_dir().join(format!("tidegauge-welcome-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        let _ = std::fs::remove_file(&file);
        assert!(read_file(&file).show_welcome(), "no file is a first run");
        std::fs::write(&file, b"{ not json").unwrap();
        assert!(!read_file(&file).show_welcome(), "a malformed file is an existing user");
        std::fs::remove_file(&file).unwrap();
        // Unreadable: a directory where the file should be.
        std::fs::create_dir(&file).unwrap();
        assert!(!read_file(&file).show_welcome(), "an unreadable file is an existing user");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_first_launch_without_a_settings_file_is_welcomed() {
        // No file: the defaults, which have not welcomed anyone.
        assert!(Settings::default().show_welcome());
        // A file from before the welcome existed: an existing user.
        let existing: Settings = serde_json::from_str(r#"{"dark":true}"#).unwrap();
        assert!(!existing.show_welcome());
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert!(!empty.show_welcome());
        // A first run that saved something before dismissing still sees it,
        // and dismissing is remembered.
        let mut saved: Settings = serde_json::from_str(&serde_json::to_string(&Settings::default()).unwrap()).unwrap();
        assert!(saved.show_welcome());
        saved.welcomed = true;
        let again: Settings = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(!again.show_welcome());
    }

    #[test]
    fn roadmap_settings_migrate_normalize_and_reset_independently() {
        let mut settings: Settings = serde_json::from_str(
            r#"{"dark":true,"alert_cpu_pct":73,"menu_items":[{"metric":"cpu","style":"value"}]}"#,
        )
        .unwrap();
        assert!(!settings.alert_cpu_enabled);
        assert!(!settings.alert_battery_low_enabled, "older files get the new alerts off");
        assert!(!settings.alert_battery_full_enabled);
        assert!(!settings.alert_disk_health_enabled);
        assert_eq!(settings.alert_battery_low_pct, 20.);
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
        settings.favorite_metrics = vec!["fans".into(), "nonsense".into(), "temperature".into()];
        settings.panel_tiles = vec!["power".into(), "power".into(), "gpu".into()];
        settings.normalize();
        assert_eq!(settings.favorite_metrics, ["fans", "temperature"], "new cards are kept, in the order chosen");
        assert_eq!(settings.panel_tiles, ["power", "gpu"], "one of each, in the order chosen");
        settings.reset_dashboard();
        assert_eq!(settings.favorite_metrics.len(), 6);
        assert_eq!(settings.dark, Some(true));
        assert_eq!(settings.alert_cpu_pct, 73.);
        assert_eq!(settings.menu_items[0].style, "value");
    }

    #[test]
    fn old_settings_files_get_the_panel_section_and_app_cpu_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"panel_tiles":["cpu"],"dark":false}"#).unwrap();
        assert!(settings.panel_show_plans);
        assert!(settings.panel_show_apps);
        assert!(!settings.app_cpu_per_core, "a share of the whole Mac unless chosen otherwise");
        assert_eq!(settings.panel_tiles, ["cpu"]);
        let off: Settings = serde_json::from_str(r#"{"panel_show_plans":false,"app_cpu_per_core":true}"#).unwrap();
        assert!(!off.panel_show_plans && off.panel_show_apps && off.app_cpu_per_core);
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
    fn transparency_defaults_off_for_old_files_and_round_trips() {
        let old: Settings = serde_json::from_str(r#"{"dark":true}"#).unwrap();
        assert_eq!(old.transparency, Transparency::Off);
        assert!(!old.panel_transparency);
        assert_eq!(old.transparency_amount, TRANSPARENCY_DEFAULT);
        let saved: Settings = serde_json::from_str(
            r#"{"transparency":"window","panel_transparency":true,"transparency_amount":70}"#,
        )
        .unwrap();
        assert_eq!(saved.transparency, Transparency::Window);
        assert!(saved.panel_transparency);
        assert_eq!(saved.transparency_amount, 70.);
        let sidebar: Settings = serde_json::from_str(r#"{"transparency":"sidebar"}"#).unwrap();
        assert_eq!(sidebar.transparency, Transparency::Sidebar);
        let again: Settings = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(again.transparency, Transparency::Window);
    }

    #[test]
    fn transparency_amount_is_clamped_and_reduce_transparency_turns_it_off() {
        let mut settings = Settings {
            transparency_amount: 250.,
            ..Settings::default()
        };
        settings.normalize();
        assert_eq!(settings.transparency_amount, 100.);
        settings.transparency_amount = f64::NAN;
        settings.normalize();
        assert_eq!(settings.transparency_amount, TRANSPARENCY_DEFAULT);
        let on = Settings {
            transparency: Transparency::Window,
            panel_transparency: true,
            ..Settings::default()
        };
        assert_eq!(on.window_transparency(false), Transparency::Window);
        assert!(on.panel_is_transparent(false));
        assert_eq!(on.window_transparency(true), Transparency::Off);
        assert!(!on.panel_is_transparent(true));
    }

    #[test]
    fn old_settings_files_keep_todays_colors_and_charts() {
        let settings: Settings =
            serde_json::from_str(r#"{"dark":false,"alert_cpu_pct":73}"#).unwrap();
        assert_eq!(settings.colors, ColorMode::Tidegauge);
        assert_eq!(settings.charts, ChartStyle::Filled);
        let defaults = Settings::default();
        assert_eq!(
            (defaults.colors, defaults.charts),
            (ColorMode::Tidegauge, ChartStyle::Filled)
        );
    }

    #[test]
    fn colors_and_charts_round_trip_by_name() {
        let settings: Settings =
            serde_json::from_str(r#"{"colors":"macos","charts":"bars"}"#).unwrap();
        assert_eq!(
            (settings.colors, settings.charts),
            (ColorMode::Macos, ChartStyle::Bars)
        );
        let json = serde_json::to_string(&Settings {
            colors: ColorMode::Calm,
            charts: ChartStyle::Line,
            ..Settings::default()
        })
        .unwrap();
        assert!(json.contains(r#""colors":"calm""#) && json.contains(r#""charts":"line""#));
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            (loaded.colors, loaded.charts),
            (ColorMode::Calm, ChartStyle::Line)
        );
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
    fn new_styles_fit_only_the_metrics_they_suit() {
        for (metric, _) in MENU_METRICS {
            for style in ["icon", "value", "graph", "both"] {
                assert!(style_fits(metric, style));
            }
        }
        assert!(style_fits("cpu", "bars") && !style_fits("memory", "bars"));
        for metric in ["cpu", "memory", "disk", "battery"] {
            assert!(style_fits(metric, "pie"));
        }
        assert!(!style_fits("network", "pie") && !style_fits("gpu", "pie") && !style_fits("power", "pie"));
        assert!(style_fits("battery", "gauge") && !style_fits("cpu", "gauge"));
        assert!(!style_fits("cpu", "sparkles"));
        let mut settings = Settings::default();
        settings.set_menu_style("memory", Some("bars"));
        assert!(settings.menu_items.is_empty(), "a style that doesn't fit is refused");
        settings.set_menu_style("cpu", Some("bars"));
        settings.set_menu_style("battery", Some("gauge"));
        assert_eq!(settings.menu_items.len(), 2);
        for (style, _) in MENU_STYLES {
            assert!(localized_style_label(style).is_some(), "{style}");
        }
    }

    #[test]
    fn old_settings_files_load_uncolored_and_new_ones_round_trip() {
        let old: Settings =
            serde_json::from_str(r#"{"menu_items":[{"metric":"cpu","style":"graph"}]}"#).unwrap();
        assert!(!old.menu_colored, "monochrome stays the default");
        assert_eq!(old.menu_items[0].style, "graph");
        let new = Settings { menu_colored: true, ..Settings::default() };
        let loaded: Settings = serde_json::from_slice(&serde_json::to_vec(&new).unwrap()).unwrap();
        assert!(loaded.menu_colored);
        // A style a later version wrote still loads; the menu bar draws it as a value.
        let future: Settings =
            serde_json::from_str(r#"{"menu_items":[{"metric":"cpu","style":"hologram"}]}"#).unwrap();
        assert_eq!(future.menu_items[0].style, "hologram");
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

    #[test]
    fn cached_settings_follow_saves_and_changes_by_other_processes() {
        let path = std::env::temp_dir().join(format!("tidegauge-settings-cache-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut cache = Cache::default();
        assert_eq!(cache.load(&path, true).alert_cpu_pct, 90., "no file: defaults");
        Settings { alert_cpu_pct: 70., ..Settings::default() }.write_to(&path).unwrap();
        // A missing file was not cached, so the new one is read at once.
        assert_eq!(cache.load(&path, true).alert_cpu_pct, 70.);
        // Another process rewrites it: the recent read is trusted for a moment...
        std::fs::write(&path, br#"{"alert_cpu_pct": 60.5, "padding": "different length"}"#).unwrap();
        assert_eq!(cache.load(&path, true).alert_cpu_pct, 70., "trusted without touching the file");
        // ...but a read that precedes a write, or a read after the trust runs out, sees it.
        assert_eq!(cache.load(&path, false).alert_cpu_pct, 60.5);
        std::fs::write(&path, br#"{"alert_cpu_pct": 55}"#).unwrap();
        std::thread::sleep(TRUST + std::time::Duration::from_millis(50));
        assert_eq!(cache.load(&path, true).alert_cpu_pct, 55.);
        // Saving drops the entry whatever the clock says.
        cache.entry = None;
        assert_eq!(cache.load(&path, true).alert_cpu_pct, 55.);
        std::fs::remove_file(&path).unwrap();
    }
}
