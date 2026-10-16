//! Local 30-day metric and app history in SQLite.

#![allow(dead_code)]

use crate::engine::Snapshot;
use rusqlite::Connection;
use std::sync::{Mutex, OnceLock};

static DB: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();

pub fn database_path() -> std::path::PathBuf {
    directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|d| d.data_dir().join("history.db"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/tidegauge/history.db"))
}

fn conn() -> &'static Mutex<Option<Connection>> {
    DB.get_or_init(|| {
        let dir = directories::ProjectDirs::from("no", "ogard", "tidegauge")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/tidegauge"));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("history.db");
        let c = Connection::open(path).ok().map(|c| {
            let _ = c.execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS metric(
                     ts REAL, metric TEXT, value REAL,
                     PRIMARY KEY(ts, metric));
                 CREATE TABLE IF NOT EXISTS app_metric(
                     ts REAL, app TEXT, cpu REAL, mem REAL, disk REAL, net REAL, power REAL,
                     PRIMARY KEY(ts, app));
                 CREATE INDEX IF NOT EXISTS idx_metric_ts ON metric(ts);
                 CREATE INDEX IF NOT EXISTS idx_metric_key_ts ON metric(metric,ts);
                 CREATE INDEX IF NOT EXISTS idx_app_name_ts ON app_metric(app,ts);
                 CREATE INDEX IF NOT EXISTS idx_app_ts ON app_metric(ts);
                 CREATE TABLE IF NOT EXISTS battery_daily(
                     day TEXT PRIMARY KEY, ts REAL, health REAL, cycles INTEGER,
                     full_mah INTEGER, design_mah INTEGER);",
            );
            c
        });
        Mutex::new(c)
    })
}

/// Persist one tick's headline metrics + top-app rows.
pub fn record(s: &Snapshot) {
    let g = conn().lock().unwrap();
    let Some(c) = g.as_ref() else { return };
    record_on(c, s);
}

fn record_on(c: &Connection, s: &Snapshot) {
    if !s.at.is_finite() || s.at <= 0. { return; }
    let Ok(transaction) = c.unchecked_transaction() else { return; };
    let c = &transaction;
    let ts = s.at;
    let rows = [
        ("cpu", s.cpu_pct),
        ("swap_used", s.mem.swap_used as f64),
        ("disk_r", s.disk.read_rate),
        ("disk_w", s.disk.write_rate),
        ("net_in", s.net.rate_in),
        ("net_out", s.net.rate_out),
    ];
    let mut stmt = match c.prepare("INSERT OR REPLACE INTO metric(ts,metric,value) VALUES(?,?,?)") {
        Ok(s) => s,
        Err(_) => return,
    };
    for (m, v) in rows {
        if v.is_finite() && v >= 0.0 {
            let _ = stmt.execute(rusqlite::params![ts, m, v]);
        }
    }
    for (metric, value) in [
        (
            "mem_used",
            (s.mem.total > 0).then(|| s.mem.used as f64 / s.mem.total as f64 * 100.0),
        ),
        ("gpu_measured", s.gpu.utilization),
        ("batt_measured", s.battery.percent),
        ("power_measured", s.power_w),
    ] {
        if let Some(value) = value.filter(|v| v.is_finite() && *v >= 0.) {
            let _ = stmt.execute(rusqlite::params![ts, metric, value]);
        }
    }
    drop(stmt);
    if let Ok(mut st) = c.prepare(
        "INSERT OR REPLACE INTO app_metric(ts,app,cpu,mem,disk,net,power) VALUES(?,?,?,?,?,?,?)",
    ) {
        for a in recorded_apps(&s.apps) {
            let _ = st.execute(rusqlite::params![
                ts,
                a.name,
                a.cpu_pct,
                a.mem_bytes as f64,
                a.disk_read_rate + a.disk_write_rate,
                a.net_in_rate + a.net_out_rate,
                a.power_w
            ]);
        }
    }
    record_battery_day(c, s);
    if should_prune(ts) {
        let cutoff = ts - 30.0 * 86400.0;
        let _ = c.execute("DELETE FROM metric WHERE ts < ?", [cutoff]);
        let _ = c.execute("DELETE FROM app_metric WHERE ts < ?", [cutoff]);
    }
    let _ = transaction.commit();
}

/// The apps one tick records: the 60 using the most memory (the order
/// `apps` arrives in), plus the busiest by CPU, disk and network, so an app
/// that is busy but small is never left out of its own history.
fn recorded_apps(apps: &[crate::processes::AppGroup]) -> Vec<&crate::processes::AppGroup> {
    let mut chosen: Vec<&crate::processes::AppGroup> = apps.iter().take(60).collect();
    let mut add_top = |n: usize, key: fn(&crate::processes::AppGroup) -> f64| {
        let mut ranked: Vec<&crate::processes::AppGroup> = apps.iter().filter(|a| key(a) > 0.).collect();
        ranked.sort_by(|a, b| key(b).total_cmp(&key(a)));
        for app in ranked.into_iter().take(n) {
            if !chosen.iter().any(|c| c.name == app.name) {
                chosen.push(app);
            }
        }
    };
    add_top(20, |a| a.cpu_pct);
    add_top(10, |a| a.disk_read_rate + a.disk_write_rate);
    add_top(10, |a| a.net_in_rate + a.net_out_rate);
    chosen
}

/// One row per local day with the day's latest battery health, kept beyond
/// the 30-day window: a battery wears over months, and a row is tiny.
fn record_battery_day(c: &Connection, s: &Snapshot) {
    let b = &s.battery;
    let Some(health) = b.health.filter(|h| b.present && h.is_finite() && *h > 0.) else { return };
    let _ = c.execute(
        "INSERT OR REPLACE INTO battery_daily(day,ts,health,cycles,full_mah,design_mah) VALUES(?,?,?,?,?,?)",
        rusqlite::params![
            crate::history_browser::local_day(s.at),
            s.at,
            health,
            b.cycles,
            b.full_capacity_mah,
            b.design_capacity_mah
        ],
    );
}

fn should_prune(ts: f64) -> bool {
    static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
    let now = ts as i64;
    let last = LAST.load(std::sync::atomic::Ordering::Relaxed);
    if last == 0 || now < last || now.saturating_sub(last) >= 3600 {
        LAST.store(now, std::sync::atomic::Ordering::Relaxed);
        true
    } else { false }
}

/// Query a metric series for the chart picker (last N seconds).
pub fn series(metric: &str, secs: f64) -> Vec<(f64, f64)> {
    let g = conn().lock().unwrap();
    let Some(c) = g.as_ref() else {
        return Vec::new();
    };
    let cutoff = crate::metrics::now_unix() - secs;
    let mut st = match c.prepare("SELECT ts,value FROM metric WHERE metric=? AND ts>? ORDER BY ts")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    st.query_map(rusqlite::params![metric, cutoff], |r| {
        Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?))
    })
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_health_is_kept_once_a_day_and_only_when_measured() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE metric(ts REAL,metric TEXT,value REAL,PRIMARY KEY(ts,metric)); CREATE TABLE app_metric(ts REAL,app TEXT,cpu REAL,mem REAL,disk REAL,net REAL,power REAL,PRIMARY KEY(ts,app)); CREATE TABLE battery_daily(day TEXT PRIMARY KEY, ts REAL, health REAL, cycles INTEGER, full_mah INTEGER, design_mah INTEGER);").unwrap();
        let mut s = Snapshot { at: 1_800_000_000., ..Snapshot::default() };
        record_on(&c, &s);
        let rows = |c: &Connection| c.query_row("SELECT COUNT(*) FROM battery_daily", [], |r| r.get::<_, u64>(0)).unwrap();
        assert_eq!(rows(&c), 0, "no battery, no row");
        s.battery.present = true;
        s.battery.health = Some(91.5);
        s.battery.cycles = Some(120);
        record_on(&c, &s);
        s.at += 60.;
        s.battery.health = Some(91.4);
        record_on(&c, &s);
        assert_eq!(rows(&c), 1, "one row per day");
        assert_eq!(c.query_row("SELECT health FROM battery_daily", [], |r| r.get::<_, f64>(0)).unwrap(), 91.4, "the day's latest reading");
    }

    #[test]
    fn busy_small_apps_are_recorded_alongside_the_largest() {
        let app = |name: &str, mem: u64, cpu: f64| crate::processes::AppGroup {
            name: name.into(),
            mem_bytes: mem,
            cpu_pct: cpu,
            ..Default::default()
        };
        // Arrives sorted by memory, as the engine delivers it.
        let mut apps: Vec<_> = (0..70).map(|i| app(&format!("big{i}"), 1000 - i, 0.)).collect();
        apps.push(app("busy-small", 1, 90.));
        let names: Vec<&str> = recorded_apps(&apps).iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names.len(), 61);
        assert!(names.contains(&"busy-small"), "a busy app with little memory is recorded");
        assert!(!names.contains(&"big65"), "idle apps past the memory cut stay out");
    }

    #[test]
    fn optional_history_records_measured_zero_not_unavailable_placeholders() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE metric(ts REAL,metric TEXT,value REAL,PRIMARY KEY(ts,metric)); CREATE TABLE app_metric(ts REAL,app TEXT,cpu REAL,mem REAL,disk REAL,net REAL,power REAL,PRIMARY KEY(ts,app));").unwrap();
        let mut snapshot = Snapshot {at:1000., ..Snapshot::default()};
        record_on(&c,&snapshot);
        assert_eq!(c.query_row("SELECT COUNT(*) FROM metric WHERE metric IN ('gpu','gpu_measured','batt_measured','power_measured','mem_used')",[],|r|r.get::<_,u64>(0)).unwrap(),0);
        snapshot.at=1010.;
        snapshot.gpu.utilization=Some(0.);
        snapshot.battery.percent=Some(0.);
        snapshot.mem.total=4096;
        snapshot.mem.used=2048;
        record_on(&c,&snapshot);
        assert_eq!(c.query_row("SELECT value FROM metric WHERE ts=1010 AND metric='gpu_measured'",[],|r|r.get::<_,f64>(0)).unwrap(),0.);
        assert_eq!(c.query_row("SELECT value FROM metric WHERE ts=1010 AND metric='mem_used'",[],|r|r.get::<_,f64>(0)).unwrap(),50.);
        assert_eq!(c.query_row("SELECT COUNT(*) FROM metric WHERE ts=1000",[],|r|r.get::<_,u64>(0)).unwrap(),6);
    }
}
