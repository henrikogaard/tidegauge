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
                 CREATE INDEX IF NOT EXISTS idx_app_ts ON app_metric(ts);",
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
        for a in s.apps.iter().take(60) {
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
    if should_prune(ts) {
        let cutoff = ts - 30.0 * 86400.0;
        let _ = c.execute("DELETE FROM metric WHERE ts < ?", [cutoff]);
        let _ = c.execute("DELETE FROM app_metric WHERE ts < ?", [cutoff]);
    }
    let _ = transaction.commit();
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
