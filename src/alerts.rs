//! Threshold alerts with independent sustained episodes and cooldowns.

use crate::battery::BatteryStats;
use crate::disks::{Device, Health};
use crate::engine::{Alert, Snapshot};
use crate::settings::{valid_threshold, Settings};

const CPU_DEFAULT_THRESHOLD: f64 = 90.0;
const MEMORY_DEFAULT_THRESHOLD: f64 = 92.0;
const CPU_MEMORY_DURATION: f64 = 20.0;
const IO_DURATION: f64 = 30.0;
const COOLDOWN: f64 = 600.0;
/// One app has to stay busy this long: a build or an export that ends in a
/// minute is not worth a notification.
const APP_DURATION: f64 = 300.0;
/// Free space on the startup disk below which it is worth a word.
pub const LOW_DISK_BYTES: f64 = 10e9;
/// A minute, so a big download that is cleaned up at once does not alert.
const LOW_DISK_DURATION: f64 = 60.0;
/// Serious (2) or critical (3): macOS is slowing the Mac down to cool it.
const THERMAL_LEVEL: f64 = 1.5;
const THERMAL_DURATION: f64 = 10.0;
/// Warning (2) or critical (4) memory pressure, as macOS numbers them.
const PRESSURE_LEVEL: f64 = 1.5;
const PRESSURE_DURATION: f64 = 20.0;
/// Long enough that a burst while compiling does not count.
const TEMPERATURE_DURATION: f64 = 30.0;
/// Long enough that a Wi-Fi roam or a wake from sleep does not count.
const OFFLINE_DURATION: f64 = 10.0;

#[derive(Default)]
struct Episode {
    since: Option<f64>,
    sent_for_episode: bool,
    last_sent: Option<f64>,
    last_at: Option<f64>,
    last_enabled: Option<bool>,
    last_threshold: Option<f64>,
}

impl Episode {
    fn reset_episode(&mut self) {
        self.since = None;
        self.sent_for_episode = false;
    }

    fn check<F>(
        &mut self,
        at: f64,
        enabled: bool,
        threshold: f64,
        value: Option<f64>,
        duration: f64,
        make_alert: F,
    ) -> Option<Alert>
    where
        F: FnOnce(f64) -> Alert,
    {
        let config_changed = self.last_enabled != Some(enabled)
            || self
                .last_threshold
                .is_some_and(|previous| previous != threshold);
        if config_changed {
            self.reset_episode();
        }
        self.last_enabled = Some(enabled);
        self.last_threshold = Some(threshold);

        if !at.is_finite() {
            self.reset_episode();
            self.last_at = None;
            return None;
        }
        if self.last_at.is_some_and(|previous| at < previous) {
            self.reset_episode();
        }
        self.last_at = Some(at);

        let Some(value) = value.filter(|value| value.is_finite()) else {
            self.reset_episode();
            return None;
        };
        if !enabled || value.partial_cmp(&threshold) != Some(std::cmp::Ordering::Greater) {
            self.reset_episode();
            return None;
        }

        let since = *self.since.get_or_insert(at);
        if self.sent_for_episode || at - since < duration {
            return None;
        }
        if self
            .last_sent
            .is_some_and(|previous| at - previous < COOLDOWN)
        {
            return None;
        }
        self.sent_for_episode = true;
        self.last_sent = Some(at);
        Some(make_alert(value))
    }
}

pub struct AlertEngine {
    cpu: Episode,
    memory: Episode,
    disk: Episode,
    network: Episode,
    /// One episode per app, keyed by its group key: CPU, then memory.
    apps: std::collections::HashMap<String, Episode>,
    app_memory: std::collections::HashMap<String, Episode>,
    app_leaks: std::collections::HashMap<String, Episode>,
    low_disk: Episode,
    thermal: Episode,
    pressure: Episode,
    temperature: Episode,
    low_battery: LowBattery,
    charged_battery: ChargedBattery,
    disk_health: DiskHealth,
    offline: Episode,
    /// "Network lost" went out, so "Network back" is owed.
    offline_alerted: bool,
}

impl AlertEngine {
    pub fn new() -> Self {
        Self {
            cpu: Episode::default(),
            memory: Episode::default(),
            disk: Episode::default(),
            network: Episode::default(),
            apps: std::collections::HashMap::new(),
            app_memory: std::collections::HashMap::new(),
            app_leaks: std::collections::HashMap::new(),
            low_disk: Episode::default(),
            thermal: Episode::default(),
            pressure: Episode::default(),
            temperature: Episode::default(),
            low_battery: LowBattery::default(),
            charged_battery: ChargedBattery::default(),
            disk_health: DiskHealth::default(),
            offline: Episode::default(),
            offline_alerted: false,
        }
    }

    pub fn check(&mut self, snapshot: &Snapshot, settings: &Settings) -> Vec<Alert> {
        let at = snapshot.at;
        let cpu_threshold = valid_threshold(settings.alert_cpu_pct, CPU_DEFAULT_THRESHOLD);
        let memory_threshold = valid_threshold(settings.alert_mem_pct, MEMORY_DEFAULT_THRESHOLD);
        let memory_pct = (snapshot.mem.total > 0)
            .then(|| snapshot.mem.used as f64 / snapshot.mem.total as f64 * 100.0);
        let disk_rate = snapshot.disk.read_rate + snapshot.disk.write_rate;
        let network_rate = snapshot.net.rate_in + snapshot.net.rate_out;
        let mut alerts = Vec::new();
        let top_cpu = top_app(snapshot, |app| app.cpu_pct);
        let top_disk = top_app(snapshot, |app| app.disk_read_rate + app.disk_write_rate);
        let top_network = top_app(snapshot, |app| app.net_in_rate + app.net_out_rate);

        if let Some(alert) = self.cpu.check(
            at,
            settings.alert_cpu_enabled,
            cpu_threshold,
            Some(snapshot.cpu_pct),
            CPU_MEMORY_DURATION,
            |value| Alert {
                metric: "cpu".into(),
                message: with_top_app(
                    format!(
                        "{}: {} {}",
                        crate::locale::text("High CPU usage", "Høy CPU-bruk"),
                        crate::locale::percent(value, 0),
                        crate::locale::text("for over 20 seconds", "i over 20 sekunder")
                    ),
                    top_cpu,
                ),
                at,
            },
        ) {
            alerts.push(alert);
        }

        if let Some(alert) = self.memory.check(
            at,
            settings.alert_mem_enabled,
            memory_threshold,
            memory_pct,
            CPU_MEMORY_DURATION,
            |value| Alert {
                metric: "memory".into(),
                message: format!(
                    "{}: {} {}",
                    crate::locale::text("High memory usage", "Høy minnebruk"),
                    crate::locale::percent(value, 0),
                    crate::locale::text("used for over 20 seconds", "brukt i over 20 sekunder")
                ),
                at,
            },
        ) {
            alerts.push(alert);
        }

        let disk_value = (snapshot.disk.read_rate.is_finite()
            && snapshot.disk.write_rate.is_finite()
            && disk_rate.is_finite())
        .then_some(disk_rate);
        if let Some(alert) = self.disk.check(
            at,
            settings.alert_disk_enabled,
            400e6,
            disk_value,
            IO_DURATION,
            |value| Alert {
                metric: "disk".into(),
                message: with_top_app(
                    format!(
                        "{}: {} {}",
                        crate::locale::text("Heavy disk activity", "Stor diskaktivitet"),
                        crate::engine::fmt_rate(value),
                        crate::locale::text("read+write", "lesing+skriving")
                    ),
                    top_disk,
                ),
                at,
            },
        ) {
            alerts.push(alert);
        }

        let network_value = (snapshot.net.rate_in.is_finite()
            && snapshot.net.rate_out.is_finite()
            && network_rate.is_finite())
        .then_some(network_rate);
        if let Some(alert) = self.network.check(
            at,
            settings.alert_network_enabled,
            80e6,
            network_value,
            IO_DURATION,
            |value| Alert {
                metric: "network".into(),
                message: with_top_app(
                    format!(
                        "{}: {}",
                        crate::locale::text("Heavy network activity", "Stor nettverksaktivitet"),
                        crate::engine::fmt_rate(value)
                    ),
                    top_network,
                ),
                at,
            },
        ) {
            alerts.push(alert);
        }

        // Episodes alert above a threshold, so low free space goes in as the
        // amount it falls short of 10 GB. A disk that reports no size is no
        // reading at all.
        let free = snapshot.disk.free as f64;
        let shortfall = (snapshot.disk.total > 0).then(|| LOW_DISK_BYTES - free);
        if let Some(alert) = self.low_disk.check(
            at,
            settings.alert_low_disk_enabled,
            0.,
            shortfall,
            LOW_DISK_DURATION,
            |_| Alert {
                metric: "low_disk".into(),
                message: format!(
                    "{}: {} {}",
                    crate::locale::text("Startup disk almost full", "Oppstartsdisken er nesten full"),
                    crate::engine::fmt_disk_bytes(free),
                    crate::locale::text("free", "ledig")
                ),
                at,
            },
        ) {
            alerts.push(alert);
        }

        if let Some(alert) = self.thermal.check(
            at,
            settings.alert_thermal_enabled,
            THERMAL_LEVEL,
            snapshot.thermal_level.map(f64::from),
            THERMAL_DURATION,
            |level| Alert {
                metric: "thermal".into(),
                message: if level >= 3. {
                    crate::locale::text(
                        "macOS reports a critical thermal state and is slowing the Mac down hard to cool it",
                        "macOS melder kritisk termisk tilstand og senker farten på Macen kraftig for å kjøle den",
                    )
                } else {
                    crate::locale::text(
                        "macOS reports a serious thermal state and is slowing the Mac down to cool it",
                        "macOS melder alvorlig termisk tilstand og senker farten på Macen for å kjøle den",
                    )
                }
                .into(),
                at,
            },
        ) {
            alerts.push(alert);
        }

        let app_threshold = crate::settings::valid_app_threshold(settings.alert_app_cpu_pct);
        alerts.extend(check_apps(
            &mut self.apps,
            snapshot,
            settings.alert_app_enabled,
            app_threshold,
            |app| app.cpu_pct,
            "app",
            |name, value| {
                format!(
                    "{name} {} {} {}",
                    crate::locale::text("has used", "har brukt"),
                    crate::locale::percent(value, 0),
                    crate::locale::text("CPU for over 5 minutes", "CPU i over 5 minutter")
                )
            },
        ));
        let memory_threshold = crate::settings::valid_app_memory_gb(settings.alert_app_mem_gb) * GIB;
        alerts.extend(check_apps(
            &mut self.app_memory,
            snapshot,
            settings.alert_app_mem_enabled,
            memory_threshold,
            |app| app.mem_bytes as f64,
            "appmem",
            |name, value| {
                format!(
                    "{name} {} {} {}",
                    crate::locale::text("has used", "har brukt"),
                    crate::engine::fmt_bytes(value),
                    crate::locale::text("of memory for over 5 minutes", "minne i over 5 minutter")
                )
            },
        ));

        // The tracker has already watched the climb for hours; an app that
        // is flagged alerts once, with the bytes it gained as the value.
        alerts.extend(check_apps(
            &mut self.app_leaks,
            snapshot,
            settings.alert_leak_enabled,
            0.,
            |app| snapshot.leaks.iter().find(|leak| leak.key == app.key).map_or(0., |leak| leak.growth),
            "leak",
            |name, growth| {
                format!(
                    "{name}: {} +{} {}. {}",
                    crate::locale::text("memory keeps growing,", "minnet vokser stadig,"),
                    crate::engine::fmt_bytes(growth),
                    crate::locale::text("in 3 hours", "på 3 timer"),
                    crate::locale::text("Restarting it frees the memory.", "Å starte den på nytt frigjør minnet.")
                )
            },
        ));

        let pressure = snapshot.mem.pressure.map(|p| match p {
            crate::metrics::Pressure::Normal => 1.,
            crate::metrics::Pressure::Warning => 2.,
            crate::metrics::Pressure::Critical => 4.,
        });
        if let Some(alert) = self.pressure.check(
            at,
            settings.alert_pressure_enabled,
            PRESSURE_LEVEL,
            pressure,
            PRESSURE_DURATION,
            |level| Alert {
                metric: "pressure".into(),
                message: if level >= 4. {
                    crate::locale::text(
                        "Memory pressure is critical: macOS is out of memory to hand out. Quit something you don't need.",
                        "Minnepresset er kritisk: macOS har ikke mer minne å gi ut. Avslutt noe du ikke trenger.",
                    )
                } else {
                    crate::locale::text(
                        "Memory pressure is high: macOS is compressing and swapping memory to keep up.",
                        "Minnepresset er høyt: macOS komprimerer og veksler minne for å henge med.",
                    )
                }
                .into(),
                at,
            },
        ) {
            alerts.push(alert);
        }

        if let Some(alert) = self.temperature.check(
            at,
            settings.alert_temp_enabled,
            crate::settings::valid_chip_temp(settings.alert_temp_c),
            snapshot.temps.chip,
            TEMPERATURE_DURATION,
            |value| Alert {
                metric: "temperature".into(),
                message: format!(
                    "{}: {} {}",
                    crate::locale::text("Chip running hot", "Brikken er varm"),
                    crate::engine::fmt_celsius(value),
                    crate::locale::text("for over 30 seconds", "i over 30 sekunder")
                ),
                at,
            },
        ) {
            alerts.push(alert);
        }

        // Offline is an episode like the others: 1 for no network path, 0 for
        // one, nothing before the first reading. "Back" follows only a "lost"
        // that was sent, and is not held back by the cooldown.
        let offline = snapshot.net.online.map(|online| if online { 0. } else { 1. });
        if let Some(alert) = self.offline.check(
            at,
            settings.alert_offline_enabled,
            0.5,
            offline,
            OFFLINE_DURATION,
            |_| Alert {
                metric: "offline".into(),
                message: crate::locale::text(
                    "Network lost: no connection for over 10 seconds",
                    "Nettverk borte: ingen tilkobling i over 10 sekunder",
                )
                .into(),
                at,
            },
        ) {
            self.offline_alerted = true;
            alerts.push(alert);
        } else if self.offline_alerted && !settings.alert_offline_enabled {
            self.offline_alerted = false;
        } else if self.offline_alerted && snapshot.net.online == Some(true) {
            self.offline_alerted = false;
            alerts.push(Alert {
                metric: "online".into(),
                message: crate::locale::text("Network back", "Nettverket er tilbake").into(),
                at,
            });
        }

        alerts.extend(self.check_power_and_disks(snapshot, settings));
        alerts
    }

    /// The battery and disk-health alerts, which watch states rather than
    /// sustained thresholds.
    fn check_power_and_disks(&mut self, snapshot: &Snapshot, settings: &Settings) -> Vec<Alert> {
        let at = snapshot.at;
        let mut alerts = Vec::new();
        let threshold = crate::settings::valid_battery_low(settings.alert_battery_low_pct);
        if let Some(percent) = self.low_battery.check(
            at,
            settings.alert_battery_low_enabled,
            threshold,
            &snapshot.battery,
        ) {
            alerts.push(Alert {
                metric: "battery_low".into(),
                message: format!(
                    "{}: {} {}",
                    crate::locale::text("Low battery", "Lavt batteri"),
                    crate::locale::percent(percent, 0),
                    crate::locale::text("left", "igjen")
                ),
                at,
            });
        }
        if let Some(percent) =
            self.charged_battery
                .check(at, settings.alert_battery_full_enabled, &snapshot.battery)
        {
            alerts.push(Alert {
                metric: "battery_full".into(),
                message: format!(
                    "{}: {}",
                    crate::locale::text("Battery charged", "Batteriet er ladet"),
                    crate::locale::percent(percent, 0)
                ),
                at,
            });
        }
        // Only read the disk inventory when the alert is on.
        let inventory = settings
            .alert_disk_health_enabled
            .then(crate::disks::inventory);
        let devices = inventory
            .as_ref()
            .filter(|inventory| inventory.available)
            .map_or(&[][..], |inventory| &inventory.devices[..]);
        alerts.extend(
            self.disk_health
                .check(at, settings.alert_disk_health_enabled, devices),
        );
        alerts
    }
}

/// The app using the most of a reading, if any is using any.
fn top_app(
    snapshot: &Snapshot,
    value: impl Fn(&crate::processes::AppGroup) -> f64,
) -> Option<&str> {
    snapshot
        .apps
        .iter()
        .filter(|app| !app.name.is_empty())
        .map(|app| (app.name.as_str(), value(app)))
        .filter(|(_, value)| value.is_finite() && *value > 0.)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(name, _)| name)
}

/// The alert's text with the busiest app named after it.
fn with_top_app(message: String, app: Option<&str>) -> String {
    match app {
        Some(name) => format!(
            "{message} · {}: {name}",
            crate::locale::text("Top app", "Mest aktive app")
        ),
        None => message,
    }
}

/// Whether a send at `last_sent` still counts as too recent at `at`.
fn cooling_down(last_sent: Option<f64>, at: f64) -> bool {
    last_sent.is_some_and(|previous| (0.0..COOLDOWN).contains(&(at - previous)))
}

/// Re-arm this far above the low-battery level, so a charge that hovers at
/// the level does not alert again and again.
const BATTERY_REARM_MARGIN: f64 = 5.;
/// A full battery on the adapter that sags below this starts a new charge.
const BATTERY_NEW_CHARGE_BELOW: f64 = 95.;

/// Low battery: one alert per crossing of the level while on battery.
#[derive(Default)]
struct LowBattery {
    /// Alerted for this crossing; the charge has to climb above the level
    /// plus 5 points to arm it again.
    alerted: bool,
    last_sent: Option<f64>,
    last_threshold: Option<f64>,
}

impl LowBattery {
    /// The charge to report when the battery has just crossed the level.
    fn check(
        &mut self,
        at: f64,
        enabled: bool,
        threshold: f64,
        battery: &BatteryStats,
    ) -> Option<f64> {
        if self
            .last_threshold
            .is_some_and(|previous| previous != threshold)
        {
            self.alerted = false;
        }
        self.last_threshold = Some(threshold);
        if !enabled {
            self.alerted = false;
            return None;
        }
        let percent = battery
            .percent
            .filter(|percent| percent.is_finite() && battery.present && at.is_finite())?;
        if percent > threshold + BATTERY_REARM_MARGIN {
            self.alerted = false;
            return None;
        }
        if self.alerted
            || !battery.on_battery
            || percent > threshold
            || cooling_down(self.last_sent, at)
        {
            return None;
        }
        self.alerted = true;
        self.last_sent = Some(at);
        Some(percent)
    }
}

/// Battery charged: one alert per charge session, when the Mac on the adapter
/// is full.
#[derive(Default)]
struct ChargedBattery {
    sent: bool,
    last_sent: Option<f64>,
    /// A battery has been read once; a battery already full then is no
    /// news, so a restart at full charge stays quiet.
    seen: bool,
}

impl ChargedBattery {
    /// The charge to report when a charge session has just finished.
    fn check(&mut self, at: f64, enabled: bool, battery: &BatteryStats) -> Option<f64> {
        let percent = battery
            .percent
            .filter(|percent| percent.is_finite() && battery.present && at.is_finite())?;
        // macOS marks the battery charged at its own limit too, so a Mac
        // held at 80 % counts as full.
        let full =
            battery.ac_power && !battery.on_battery && (battery.is_charged || percent >= 100.);
        if !self.seen {
            self.seen = true;
            self.sent = full;
        }
        if !enabled {
            // Switching it on while already full is not a new charge.
            self.sent = full;
            return None;
        }
        if !full {
            if battery.on_battery || percent < BATTERY_NEW_CHARGE_BELOW {
                self.sent = false;
            }
            return None;
        }
        if self.sent || cooling_down(self.last_sent, at) {
            return None;
        }
        self.sent = true;
        self.last_sent = Some(at);
        Some(percent)
    }
}

/// Disk health: one alert each time an internal disk turns to failing.
#[derive(Default)]
struct DiskHealth {
    /// The last verdict seen for each disk, passed or failed. A disk whose
    /// health goes unreadable keeps its last verdict.
    seen: std::collections::HashMap<String, Health>,
    last_sent: std::collections::HashMap<String, f64>,
}

impl DiskHealth {
    fn check(&mut self, at: f64, enabled: bool, devices: &[Device]) -> Vec<Alert> {
        if !enabled || !at.is_finite() {
            // Switched on later, a disk that already fails is news.
            self.seen.clear();
            return Vec::new();
        }
        let mut alerts = Vec::new();
        for device in devices
            .iter()
            .filter(|device| device.internal == Some(true))
        {
            match device.smart.health {
                Health::Passed => {
                    self.seen.insert(device.id.clone(), Health::Passed);
                }
                Health::Failed => {
                    if self.seen.get(&device.id) == Some(&Health::Failed)
                        || cooling_down(self.last_sent.get(&device.id).copied(), at)
                    {
                        continue;
                    }
                    self.seen.insert(device.id.clone(), Health::Failed);
                    self.last_sent.insert(device.id.clone(), at);
                    let name = if device.name.is_empty() {
                        crate::locale::text("An internal disk", "En intern disk").to_string()
                    } else {
                        device.name.clone()
                    };
                    alerts.push(Alert {
                        metric: format!("disk_health:{}", device.id),
                        message: format!(
                            "{}: {name} {}",
                            crate::locale::text("Disk health warning", "Diskhelse-advarsel"),
                            crate::locale::text("reports failing health", "melder dårlig helse")
                        ),
                        at,
                    });
                }
                Health::Unsupported | Health::Unavailable => {}
            }
        }
        alerts
    }
}

/// The memory threshold is set in GB as Tidegauge shows them: 1024³ bytes.
const GIB: f64 = 1024. * 1024. * 1024.;

/// One episode per app for a per-app reading: an app alerts once it stays
/// above `threshold` for five minutes, as `"{prefix}:{key}"`.
fn check_apps(
    episodes: &mut std::collections::HashMap<String, Episode>,
    snapshot: &Snapshot,
    enabled: bool,
    threshold: f64,
    value: impl Fn(&crate::processes::AppGroup) -> f64,
    prefix: &str,
    message: impl Fn(&str, f64) -> String,
) -> Vec<Alert> {
    let at = snapshot.at;
    let mut alerts = Vec::new();
    for app in &snapshot.apps {
        let episode = episodes.entry(app.key.clone()).or_default();
        if let Some(alert) = episode.check(at, enabled, threshold, Some(value(app)), APP_DURATION, |v| Alert {
            metric: format!("{prefix}:{}", app.key),
            message: message(&app.name, v),
            at,
        }) {
            alerts.push(alert);
        }
    }
    // An app that has gone ends its episode; its cooldown is kept, so a
    // quick relaunch does not alert again at once.
    episodes.retain(|key, episode| {
        snapshot.apps.iter().any(|a| &a.key == key)
            || episode.last_sent.is_some_and(|sent| at - sent < COOLDOWN)
    });
    for (key, episode) in episodes.iter_mut() {
        if !snapshot.apps.iter().any(|a| &a.key == key) {
            episode.reset_episode();
        }
    }
    alerts
}

impl Default for AlertEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings {
            alert_cpu_enabled: true,
            alert_mem_enabled: true,
            alert_disk_enabled: true,
            alert_network_enabled: true,
            ..Settings::default()
        }
    }

    fn snapshot(at: f64) -> Snapshot {
        Snapshot {
            at,
            ..Snapshot::default()
        }
    }

    fn busy_app(at: f64, cpu: f64) -> Snapshot {
        let mut s = snapshot(at);
        s.apps = vec![crate::processes::AppGroup {
            key: "com.example.busy".into(),
            name: "Busy".into(),
            cpu_pct: cpu,
            ..Default::default()
        }];
        s
    }

    fn app_settings() -> Settings {
        Settings { alert_app_enabled: true, ..Settings::default() }
    }

    #[test]
    fn one_app_alerts_after_five_busy_minutes() {
        let settings = app_settings();
        let mut engine = AlertEngine::new();
        assert!(engine.check(&busy_app(0., 120.), &settings).is_empty());
        assert!(engine.check(&busy_app(299., 120.), &settings).is_empty());
        let alerts = engine.check(&busy_app(300., 120.), &settings);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].metric, "app:com.example.busy");
        assert!(alerts[0].message.starts_with("Busy "), "{}", alerts[0].message);
        assert!(engine.check(&busy_app(400., 120.), &settings).is_empty(), "one alert per episode");
    }

    #[test]
    fn app_alerts_respect_threshold_and_switch() {
        let mut engine = AlertEngine::new();
        let mut settings = app_settings();
        settings.alert_app_cpu_pct = 150.;
        for at in [0., 300., 600.] {
            assert!(engine.check(&busy_app(at, 120.), &settings).is_empty(), "below the threshold");
        }
        let off = Settings::default();
        let mut engine = AlertEngine::new();
        for at in [0., 300., 600.] {
            assert!(engine.check(&busy_app(at, 300.), &off).is_empty(), "off by default");
        }
    }

    #[test]
    fn a_quiet_moment_or_a_quit_restarts_the_five_minutes() {
        let settings = app_settings();
        let mut engine = AlertEngine::new();
        engine.check(&busy_app(0., 120.), &settings);
        engine.check(&busy_app(200., 10.), &settings);
        assert!(engine.check(&busy_app(400., 120.), &settings).is_empty());
        engine.check(&snapshot(450.), &settings);
        assert!(engine.check(&busy_app(500., 120.), &settings).is_empty());
        assert!(engine.check(&busy_app(799., 120.), &settings).is_empty());
        assert_eq!(engine.check(&busy_app(800., 120.), &settings).len(), 1);
    }

    #[test]
    fn one_app_alerts_after_five_minutes_above_its_memory_limit() {
        let settings = Settings { alert_app_mem_enabled: true, alert_app_mem_gb: 2., ..Settings::default() };
        let mut engine = AlertEngine::new();
        let big = |at: f64, gb: f64| {
            let mut s = busy_app(at, 0.);
            s.apps[0].mem_bytes = (gb * GIB) as u64;
            s
        };
        assert!(engine.check(&big(0., 1.5), &settings).is_empty());
        assert!(engine.check(&big(10., 3.), &settings).is_empty());
        assert!(engine.check(&big(309., 3.), &settings).is_empty());
        let alerts = engine.check(&big(310., 3.), &settings);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].metric, "appmem:com.example.busy");
        assert!(engine.check(&big(400., 3.), &settings).is_empty(), "one alert per episode");
        assert!(engine.check(&big(1000., 3.), &Settings::default()).is_empty(), "off by default");
    }

    #[test]
    fn a_growing_app_alerts_once_per_episode_and_only_when_switched_on() {
        let settings = Settings { alert_leak_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let growing = |at: f64, flagged: bool| {
            let mut s = busy_app(at, 0.);
            if flagged {
                s.leaks = vec![crate::leaks::Leak { key: "com.example.busy".into(), name: "Busy".into(), growth: 2.3 * GIB }];
            }
            s
        };
        assert!(engine.check(&growing(0., false), &settings).is_empty(), "not flagged");
        assert!(engine.check(&growing(10., true), &settings).is_empty());
        assert!(engine.check(&growing(309., true), &settings).is_empty());
        let alerts = engine.check(&growing(310., true), &settings);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].metric, "leak:com.example.busy");
        assert!(alerts[0].message.starts_with("Busy: "), "{}", alerts[0].message);
        assert!(alerts[0].message.contains("2.30") || alerts[0].message.contains("2,30"), "{}", alerts[0].message);
        assert!(engine.check(&growing(400., true), &settings).is_empty(), "one alert per episode");
        // The climb ends, then the same app is flagged again inside the cooldown.
        assert!(engine.check(&growing(410., false), &settings).is_empty());
        assert!(engine.check(&growing(420., true), &settings).is_empty());
        assert!(engine.check(&growing(800., true), &settings).is_empty(), "the cooldown holds a second alert back");
        assert_eq!(engine.check(&growing(1000., true), &settings).len(), 1, "a new episode alerts after the cooldown");
        let mut engine = AlertEngine::new();
        for at in [0., 310., 700., 1500.] {
            assert!(engine.check(&growing(at, true), &Settings::default()).is_empty(), "off by default");
        }
    }

    #[test]
    fn app_thresholds_are_clamped() {
        assert_eq!(crate::settings::valid_app_memory_gb(0.2), 1.);
        assert_eq!(crate::settings::valid_app_memory_gb(500.), 64.);
        assert_eq!(crate::settings::valid_app_memory_gb(f64::NAN), 4.);
        assert_eq!(crate::settings::valid_app_threshold(5.), 20.);
        assert_eq!(crate::settings::valid_app_threshold(900.), 400.);
        assert_eq!(crate::settings::valid_app_threshold(f64::NAN), 80.);
    }

    #[test]
    fn low_disk_alerts_after_a_minute_under_ten_gigabytes() {
        let settings = Settings { alert_low_disk_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.);
        s.disk.total = 500e9 as u64;
        s.disk.free = 12e9 as u64;
        assert!(engine.check(&s, &settings).is_empty(), "12 GB free is fine");
        s.disk.free = 8e9 as u64;
        s.at = 10.;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 70.;
        let alerts = engine.check(&s, &settings);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].metric, "low_disk");
        s.disk.total = 0;
        s.at = 2000.;
        assert!(engine.check(&s, &settings).is_empty(), "no disk reading is not low space");
    }

    #[test]
    fn thermal_alerts_on_serious_or_critical_only() {
        let settings = Settings { alert_thermal_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.);
        s.thermal_level = Some(1);
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 60.;
        assert!(engine.check(&s, &settings).is_empty(), "fair is not worth an alert");
        s.thermal_level = Some(2);
        s.at = 61.;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 71.;
        assert_eq!(engine.check(&s, &settings)[0].metric, "thermal");
        let mut engine = AlertEngine::new();
        s.thermal_level = None;
        for at in [100., 200.] {
            s.at = at;
            assert!(engine.check(&s, &settings).is_empty(), "an unknown state never alerts");
        }
    }

    #[test]
    fn pressure_alerts_on_warning_or_critical_for_twenty_seconds() {
        use crate::metrics::Pressure;
        let settings = Settings { alert_pressure_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let mut s = snapshot(100.);
        s.mem.pressure = Some(Pressure::Normal);
        assert!(engine.check(&s, &settings).is_empty());
        s.mem.pressure = Some(Pressure::Warning);
        s.at = 101.;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 121.;
        let alerts = engine.check(&s, &settings);
        assert_eq!(alerts[0].metric, "pressure");
        assert!(!alerts[0].message.contains("critical") && !alerts[0].message.contains("kritisk"), "warning is not worded as critical: {}", alerts[0].message);
        let mut engine = AlertEngine::new();
        s.mem.pressure = None;
        for at in [200., 300.] {
            s.at = at;
            assert!(engine.check(&s, &settings).is_empty(), "an unknown level never alerts");
        }
    }

    #[test]
    fn chip_temperature_alerts_after_thirty_seconds_above_the_level() {
        let settings = Settings { alert_temp_enabled: true, alert_temp_c: 90., ..Settings::default() };
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.);
        s.temps.chip = Some(92.);
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 29.;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 30.;
        assert_eq!(engine.check(&s, &settings)[0].metric, "temperature");
        let mut engine = AlertEngine::new();
        s.temps.chip = None;
        for at in [100., 200.] {
            s.at = at;
            assert!(engine.check(&s, &settings).is_empty(), "no reading is not a hot chip");
        }
    }

    fn battery(percent: f64, on_battery: bool) -> BatteryStats {
        BatteryStats {
            available: true,
            present: true,
            on_battery,
            ac_power: !on_battery,
            percent: Some(percent),
            ..BatteryStats::default()
        }
    }

    fn with_battery(at: f64, battery: BatteryStats) -> Snapshot {
        Snapshot {
            battery,
            ..snapshot(at)
        }
    }

    #[test]
    fn low_battery_alerts_once_per_crossing_and_rearms_above_the_margin() {
        let settings = Settings {
            alert_battery_low_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(engine
            .check(&with_battery(0., battery(30., true)), &settings)
            .is_empty());
        assert!(
            engine
                .check(&with_battery(10., battery(21., true)), &settings)
                .is_empty(),
            "above 20 %"
        );
        let alerts = engine.check(&with_battery(20., battery(20., true)), &settings);
        assert_eq!(alerts.len(), 1, "falling to the level alerts");
        assert_eq!(alerts[0].metric, "battery_low");
        assert!(alerts[0].message.contains("20"), "{}", alerts[0].message);
        assert!(
            engine
                .check(&with_battery(30., battery(19., true)), &settings)
                .is_empty(),
            "once per crossing"
        );
        // Charging a little, but not past 25 %, keeps it quiet after the cooldown.
        assert!(engine
            .check(&with_battery(40., battery(25., false)), &settings)
            .is_empty());
        assert!(
            engine
                .check(&with_battery(1000., battery(20., true)), &settings)
                .is_empty(),
            "25 % is not above 25 %"
        );
        // Above level + 5 re-arms; the next fall alerts again.
        assert!(engine
            .check(&with_battery(1010., battery(26., false)), &settings)
            .is_empty());
        assert!(
            engine
                .check(&with_battery(1020., battery(18., true)), &settings)
                .len()
                == 1
        );
    }

    #[test]
    fn low_battery_needs_battery_power_a_reading_and_the_switch() {
        let settings = Settings {
            alert_battery_low_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(
            engine
                .check(&with_battery(0., battery(10., false)), &settings)
                .is_empty(),
            "on the adapter"
        );
        let mut no_reading = battery(10., true);
        no_reading.percent = None;
        assert!(engine
            .check(&with_battery(1., no_reading), &settings)
            .is_empty());
        let mut absent = battery(10., true);
        absent.present = false;
        assert!(
            engine
                .check(&with_battery(2., absent), &settings)
                .is_empty(),
            "no battery in this Mac"
        );
        assert!(engine
            .check(&with_battery(3., BatteryStats::default()), &settings)
            .is_empty());
        assert!(
            engine
                .check(&with_battery(4., battery(10., true)), &Settings::default())
                .is_empty(),
            "off by default"
        );
        // Unplugged while already low: that is the crossing.
        assert_eq!(
            engine
                .check(&with_battery(5., battery(10., true)), &settings)
                .len(),
            1
        );
    }

    #[test]
    fn low_battery_follows_its_adjustable_level() {
        let mut settings = Settings {
            alert_battery_low_enabled: true,
            alert_battery_low_pct: 10.,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(engine
            .check(&with_battery(0., battery(15., true)), &settings)
            .is_empty());
        assert_eq!(
            engine
                .check(&with_battery(1., battery(10., true)), &settings)
                .len(),
            1
        );
        // Out-of-range values are clamped.
        settings.alert_battery_low_pct = 500.;
        assert_eq!(
            crate::settings::valid_battery_low(settings.alert_battery_low_pct),
            50.
        );
        assert_eq!(crate::settings::valid_battery_low(1.), 5.);
        assert_eq!(crate::settings::valid_battery_low(f64::NAN), 20.);
    }

    #[test]
    fn low_battery_cooldown_holds_back_a_quick_second_crossing() {
        let settings = Settings {
            alert_battery_low_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert_eq!(
            engine
                .check(&with_battery(0., battery(19., true)), &settings)
                .len(),
            1
        );
        engine.check(&with_battery(10., battery(30., false)), &settings);
        assert!(
            engine
                .check(&with_battery(20., battery(19., true)), &settings)
                .is_empty(),
            "within 10 minutes"
        );
        assert_eq!(
            engine
                .check(&with_battery(601., battery(19., true)), &settings)
                .len(),
            1,
            "after the cooldown"
        );
    }

    fn charged(percent: f64, is_charged: bool) -> BatteryStats {
        BatteryStats {
            is_charged,
            ..battery(percent, false)
        }
    }

    #[test]
    fn battery_charged_alerts_once_per_charge_session() {
        let settings = Settings {
            alert_battery_full_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(engine
            .check(&with_battery(0., charged(60., false)), &settings)
            .is_empty());
        assert!(engine
            .check(&with_battery(10., charged(99., false)), &settings)
            .is_empty());
        let alerts = engine.check(&with_battery(20., charged(100., true)), &settings);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].metric, "battery_full");
        assert!(
            engine
                .check(&with_battery(30., charged(100., true)), &settings)
                .is_empty(),
            "once per session"
        );
        // A dip to 97 % on the adapter is the same session.
        assert!(engine
            .check(&with_battery(40., charged(97., false)), &settings)
            .is_empty());
        assert!(engine
            .check(&with_battery(1000., charged(100., true)), &settings)
            .is_empty());
        // Unplugging ends the session; the next full charge alerts again.
        assert!(engine
            .check(&with_battery(1010., battery(90., true)), &settings)
            .is_empty());
        assert!(
            engine
                .check(&with_battery(1020., charged(100., true)), &settings)
                .len()
                == 1
        );
    }

    #[test]
    fn battery_charged_stays_quiet_when_it_starts_full() {
        let settings = Settings {
            alert_battery_full_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(
            engine
                .check(&with_battery(0., charged(100., true)), &settings)
                .is_empty(),
            "already full at launch"
        );
        assert!(engine
            .check(&with_battery(10., charged(100., true)), &settings)
            .is_empty());
        // Unplugged and charged again: that is a real charge.
        assert!(engine
            .check(&with_battery(20., battery(90., true)), &settings)
            .is_empty());
        assert_eq!(
            engine
                .check(&with_battery(30., charged(100., true)), &settings)
                .len(),
            1
        );
    }

    #[test]
    fn battery_charged_accepts_the_limit_macos_reports() {
        let settings = Settings {
            alert_battery_full_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(engine
            .check(&with_battery(0., charged(78., false)), &settings)
            .is_empty());
        let alerts = engine.check(&with_battery(10., charged(80., true)), &settings);
        assert_eq!(alerts.len(), 1, "held at an 80 % limit counts as charged");
        assert!(
            engine
                .check(&with_battery(20., charged(80., true)), &settings)
                .is_empty(),
            "the limit does not repeat"
        );
    }

    #[test]
    fn battery_charged_ignores_battery_power_switch_off_and_no_battery() {
        let settings = Settings {
            alert_battery_full_enabled: true,
            ..Settings::default()
        };
        let mut engine = AlertEngine::new();
        assert!(
            engine
                .check(&with_battery(0., battery(100., true)), &settings)
                .is_empty(),
            "on battery"
        );
        assert!(
            engine
                .check(&with_battery(1., BatteryStats::default()), &settings)
                .is_empty(),
            "no battery"
        );
        assert!(
            engine
                .check(&with_battery(2., charged(100., true)), &Settings::default())
                .is_empty(),
            "off by default"
        );
        // Switching it on while already full is not a new charge.
        assert!(engine
            .check(&with_battery(3., charged(100., true)), &settings)
            .is_empty());
        engine.check(&with_battery(4., battery(50., true)), &settings);
        assert_eq!(
            engine
                .check(&with_battery(5., charged(100., true)), &settings)
                .len(),
            1
        );
    }

    fn internal_disk(id: &str, health: Health) -> Device {
        Device {
            id: id.into(),
            name: format!("Disk {id}"),
            internal: Some(true),
            smart: crate::disks::Smart {
                health,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn disk_health_alerts_once_when_an_internal_disk_turns_failing() {
        let mut watch = DiskHealth::default();
        assert!(watch
            .check(0., true, &[internal_disk("disk0", Health::Passed)])
            .is_empty());
        let alerts = watch.check(30., true, &[internal_disk("disk0", Health::Failed)]);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].metric, "disk_health:disk0");
        assert!(
            alerts[0].message.contains("Disk disk0"),
            "{}",
            alerts[0].message
        );
        for at in [60., 90., 5000.] {
            assert!(
                watch
                    .check(at, true, &[internal_disk("disk0", Health::Failed)])
                    .is_empty(),
                "no repeat at {at}"
            );
        }
        // An unreadable status keeps the last verdict instead of repeating it.
        assert!(watch
            .check(6000., true, &[internal_disk("disk0", Health::Unavailable)])
            .is_empty());
        assert!(watch
            .check(6030., true, &[internal_disk("disk0", Health::Failed)])
            .is_empty());
        // Recovering and failing again is a new state change.
        assert!(watch
            .check(7000., true, &[internal_disk("disk0", Health::Passed)])
            .is_empty());
        assert_eq!(
            watch
                .check(7030., true, &[internal_disk("disk0", Health::Failed)])
                .len(),
            1
        );
    }

    #[test]
    fn disk_health_skips_healthy_unknown_external_and_switched_off() {
        let mut watch = DiskHealth::default();
        let mut external = internal_disk("disk4", Health::Failed);
        external.internal = Some(false);
        let mut unknown = internal_disk("disk5", Health::Failed);
        unknown.internal = None;
        let devices = [
            internal_disk("disk0", Health::Passed),
            internal_disk("disk1", Health::Unsupported),
            internal_disk("disk2", Health::Unavailable),
            external,
            unknown,
        ];
        assert!(watch.check(0., true, &devices).is_empty());
        let failing = [internal_disk("disk0", Health::Failed)];
        assert!(
            watch.check(1., false, &failing).is_empty(),
            "off by default"
        );
        // Switched on with a disk already failing: worth one alert.
        assert_eq!(watch.check(2., true, &failing).len(), 1);
        let two = [
            internal_disk("disk0", Health::Failed),
            internal_disk("disk1", Health::Failed),
        ];
        let alerts = watch.check(3., true, &two);
        assert_eq!(alerts.len(), 1, "only the new failure");
        assert_eq!(alerts[0].metric, "disk_health:disk1");
    }

    #[test]
    fn disk_health_is_off_by_default_in_the_engine() {
        let mut engine = AlertEngine::new();
        assert!(engine.check(&snapshot(0.), &Settings::default()).is_empty());
        let settings = Settings {
            alert_disk_health_enabled: true,
            ..Settings::default()
        };
        assert!(
            engine.check(&snapshot(1.), &settings).is_empty(),
            "no inventory, no alert"
        );
    }

    #[test]
    fn cpu_disk_and_network_alerts_name_the_top_app() {
        let settings = settings();
        let top_label = crate::locale::text("Top app", "Mest aktive app");
        let top = format!("{top_label}: Xcode");
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.);
        s.cpu_pct = 95.;
        s.disk.read_rate = 450e6;
        s.net.rate_in = 90e6;
        s.apps = vec![
            crate::processes::AppGroup {
                key: "a".into(),
                name: "Quiet".into(),
                cpu_pct: 5.,
                disk_read_rate: 1e6,
                net_in_rate: 1e6,
                ..Default::default()
            },
            crate::processes::AppGroup {
                key: "b".into(),
                name: "Xcode".into(),
                cpu_pct: 93.,
                disk_write_rate: 400e6,
                net_out_rate: 80e6,
                ..Default::default()
            },
        ];
        engine.check(&s, &settings);
        s.at = 20.;
        let cpu = engine.check(&s, &settings);
        assert!(cpu[0].message.ends_with(&top), "{}", cpu[0].message);
        s.at = 30.;
        let io = engine.check(&s, &settings);
        assert_eq!(io.len(), 2);
        assert!(
            io.iter().all(|alert| alert.message.ends_with(&top)),
            "{:?}",
            io.iter().map(|a| &a.message).collect::<Vec<_>>()
        );
        // Without any app using it, the text is as before.
        let mut s = snapshot(1000.);
        s.cpu_pct = 95.;
        let mut engine = AlertEngine::new();
        engine.check(&s, &settings);
        s.at = 1020.;
        assert!(!engine.check(&s, &settings)[0].message.contains(top_label));
    }

    #[test]
    fn network_loss_alerts_after_ten_seconds_and_again_when_back() {
        let settings = Settings { alert_offline_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let at = |t: f64, online: Option<bool>| {
            let mut s = snapshot(t);
            s.net.online = online;
            s
        };
        assert!(engine.check(&at(0., Some(true)), &settings).is_empty());
        assert!(engine.check(&at(2., Some(false)), &settings).is_empty());
        assert!(engine.check(&at(11., Some(false)), &settings).is_empty(), "nine seconds is a blip");
        let lost = engine.check(&at(12., Some(false)), &settings);
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].metric, "offline");
        assert!(engine.check(&at(60., Some(false)), &settings).is_empty(), "one alert per episode");
        let back = engine.check(&at(70., Some(true)), &settings);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].metric, "online");
        assert!(engine.check(&at(72., Some(true)), &settings).is_empty(), "back is said once");
    }

    #[test]
    fn a_short_outage_says_nothing_and_neither_does_an_unknown_state() {
        let settings = Settings { alert_offline_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let at = |t: f64, online: Option<bool>| {
            let mut s = snapshot(t);
            s.net.online = online;
            s
        };
        assert!(engine.check(&at(0., Some(false)), &settings).is_empty());
        assert!(engine.check(&at(8., Some(false)), &settings).is_empty());
        assert!(engine.check(&at(9., Some(true)), &settings).is_empty(), "no lost alert, so no back alert");
        for t in [100., 200.] {
            assert!(engine.check(&at(t, None), &settings).is_empty(), "an unknown state never alerts");
        }
        assert!(engine.check(&at(300., Some(false)), &Settings::default()).is_empty());
        assert!(engine.check(&at(400., Some(false)), &Settings::default()).is_empty(), "off by default");
    }

    #[test]
    fn network_loss_respects_the_cooldown_and_the_switch() {
        let settings = Settings { alert_offline_enabled: true, ..Settings::default() };
        let mut engine = AlertEngine::new();
        let at = |t: f64, online: bool| {
            let mut s = snapshot(t);
            s.net.online = Some(online);
            s
        };
        engine.check(&at(0., false), &settings);
        assert_eq!(engine.check(&at(10., false), &settings).len(), 1);
        assert_eq!(engine.check(&at(20., true), &settings)[0].metric, "online");
        engine.check(&at(30., false), &settings);
        assert!(engine.check(&at(45., false), &settings).is_empty(), "inside the ten-minute cooldown");
        assert!(engine.check(&at(50., true), &settings).is_empty(), "nothing was said, so no back alert");
        // Switching it off between "lost" and "back" owes nothing.
        let mut engine = AlertEngine::new();
        engine.check(&at(0., false), &settings);
        assert_eq!(engine.check(&at(10., false), &settings).len(), 1);
        assert!(engine.check(&at(20., false), &Settings::default()).is_empty());
        assert!(engine.check(&at(30., true), &settings).is_empty());
    }

    #[test]
    fn custom_threshold_is_used() {
        let mut settings = settings();
        settings.alert_mem_enabled = false;
        settings.alert_cpu_pct = 50.0;
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.0);
        s.cpu_pct = 51.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 20.0;
        assert_eq!(engine.check(&s, &settings)[0].metric, "cpu");
    }

    #[test]
    fn sustained_duration_and_one_episode() {
        let mut settings = settings();
        settings.alert_mem_enabled = false;
        settings.alert_disk_enabled = false;
        settings.alert_network_enabled = false;
        let mut engine = AlertEngine::new();
        let mut s = snapshot(100.0);
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 119.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 120.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);
        s.at = 180.0;
        assert!(engine.check(&s, &settings).is_empty());
    }

    #[test]
    fn recovery_rearms_after_cooldown() {
        let mut settings = settings();
        settings.alert_mem_enabled = false;
        settings.alert_disk_enabled = false;
        settings.alert_network_enabled = false;
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.0);
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 20.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);
        s.at = 21.0;
        s.cpu_pct = 10.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 22.0;
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 41.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 42.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 619.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 620.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);
    }

    #[test]
    fn independent_metrics_do_not_share_cooldown() {
        let mut settings = settings();
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.0);
        s.cpu_pct = 95.0;
        s.mem.total = 100;
        s.mem.used = 95;
        s.disk.read_rate = 450e6;
        s.net.rate_in = 90e6;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 20.0;
        let alerts = engine.check(&s, &settings);
        assert_eq!(alerts.len(), 2);
        assert!(alerts.iter().any(|alert| alert.metric == "cpu"));
        assert!(alerts.iter().any(|alert| alert.metric == "memory"));
        s.at = 30.0;
        let alerts = engine.check(&s, &settings);
        assert_eq!(alerts.len(), 2);
        assert!(alerts.iter().any(|alert| alert.metric == "disk"));
        assert!(alerts.iter().any(|alert| alert.metric == "network"));
        settings.alert_cpu_enabled = false;
        assert!(engine.check(&s, &settings).is_empty());
    }

    #[test]
    fn disabled_metrics_rearm_without_losing_cooldown() {
        let mut settings = settings();
        settings.alert_mem_enabled = false;
        settings.alert_disk_enabled = false;
        settings.alert_network_enabled = false;
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.0);
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 20.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);
        settings.alert_cpu_enabled = false;
        s.at = 21.0;
        assert!(engine.check(&s, &settings).is_empty());
        settings.alert_cpu_enabled = true;
        s.at = 40.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 60.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 619.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 620.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);
    }

    #[test]
    fn invalid_memory_and_backwards_clock_reset_episode() {
        let mut settings = settings();
        settings.alert_cpu_enabled = false;
        settings.alert_disk_enabled = false;
        settings.alert_network_enabled = false;
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.0);
        s.mem.total = 100;
        s.mem.used = 99;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 10.0;
        s.mem.total = 0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 11.0;
        s.mem.total = 100;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 30.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 31.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);

        settings.alert_mem_enabled = false;
        settings.alert_cpu_enabled = true;
        let mut engine = AlertEngine::new();
        let mut s = snapshot(0.0);
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 10.0;
        s.cpu_pct = f64::NAN;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 11.0;
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 30.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 31.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);

        let mut engine = AlertEngine::new();
        let mut s = snapshot(20.0);
        s.cpu_pct = 95.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 10.0;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 29.9;
        assert!(engine.check(&s, &settings).is_empty());
        s.at = 30.0;
        assert_eq!(engine.check(&s, &settings).len(), 1);
    }
}
