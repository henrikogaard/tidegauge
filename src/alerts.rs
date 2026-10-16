//! Threshold alerts with independent sustained episodes and cooldowns.

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
    low_disk: Episode,
    thermal: Episode,
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
            low_disk: Episode::default(),
            thermal: Episode::default(),
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

        if let Some(alert) = self.cpu.check(
            at,
            settings.alert_cpu_enabled,
            cpu_threshold,
            Some(snapshot.cpu_pct),
            CPU_MEMORY_DURATION,
            |value| Alert {
                metric: "cpu".into(),
                message: format!(
                    "{}: {} {}",
                    crate::locale::text("High CPU usage", "Høy CPU-bruk"),
                    crate::locale::percent(value, 0),
                    crate::locale::text("for over 20 seconds", "i over 20 sekunder")
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
                message: format!(
                    "{}: {} {}",
                    crate::locale::text("Heavy disk activity", "Stor diskaktivitet"),
                    crate::engine::fmt_rate(value),
                    crate::locale::text("read+write", "lesing+skriving")
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
                message: format!(
                    "{}: {}",
                    crate::locale::text("Heavy network activity", "Stor nettverksaktivitet"),
                    crate::engine::fmt_rate(value)
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
                    crate::engine::fmt_bytes(free),
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
