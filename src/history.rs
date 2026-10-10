//! 30-day metrics history in SQLite + alert generation + notifications.

#![allow(dead_code)]

use crate::engine::{Alert, Snapshot};
use rusqlite::Connection;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static DB: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();

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
    let ts = s.at;
    let rows = [
        ("cpu", s.cpu_pct),
        ("mem_used", s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0),
        ("swap_used", s.mem.swap_used as f64),
        ("disk_r", s.disk.read_rate),
        ("disk_w", s.disk.write_rate),
        ("net_in", s.net.rate_in),
        ("net_out", s.net.rate_out),
        ("gpu", s.gpu.utilization.unwrap_or(0.0)),
    ];
    let mut stmt = match c.prepare("INSERT OR REPLACE INTO metric(ts,metric,value) VALUES(?,?,?)") {
        Ok(s) => s,
        Err(_) => return,
    };
    for (m, v) in rows {
        let _ = stmt.execute(rusqlite::params![ts, m, v]);
    }
    for (metric, value) in [("batt", s.battery.percent), ("power", s.power_w)] {
        if let Some(value) = value {
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
    // prune >30 days, every ~200 ticks piggyback on ts modulo
    if (ts as i64) % (30 * 60 * 24 * 7) < 300 {
        let cutoff = ts - 30.0 * 86400.0;
        let _ = c.execute("DELETE FROM metric WHERE ts < ?", [cutoff]);
        let _ = c.execute("DELETE FROM app_metric WHERE ts < ?", [cutoff]);
    }
}

/// Query a metric series for the chart picker (last N seconds).
pub fn series(metric: &str, secs: f64) -> Vec<(f64, f64)> {
    let g = conn().lock().unwrap();
    let Some(c) = g.as_ref() else { return Vec::new() };
    let cutoff = crate::metrics::now_unix() - secs;
    let mut st = match c.prepare(
        "SELECT ts,value FROM metric WHERE metric=? AND ts>? ORDER BY ts",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    st.query_map(rusqlite::params![metric, cutoff], |r| {
        Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?))
    })
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

// ---- alerts ----

pub struct AlertEngine {
    cpu_since: f64,
    mem_since: f64,
    disk_since: f64,
    net_since: f64,
    last_sent: f64,
}

impl AlertEngine {
    pub fn new() -> Self {
        AlertEngine { cpu_since: 0.0, mem_since: 0.0, disk_since: 0.0, net_since: 0.0, last_sent: 0.0 }
    }

    /// Check thresholds; sustained >20s breaches generate at most one alert
    /// per 10 min.
    pub fn check(&mut self, s: &Snapshot) -> Vec<Alert> {
        let t = s.at;
        let mut out = Vec::new();
        if s.cpu_pct > 90.0 {
            if self.cpu_since == 0.0 { self.cpu_since = t; }
            if t - self.cpu_since > 20.0 && t - self.last_sent > 600.0 {
                out.push(Alert {
                    metric: "cpu".into(),
                    message: format!("High CPU usage: {:.0}% for over 20 seconds", s.cpu_pct),
                    at: t,
                });
                self.last_sent = t;
            }
        } else { self.cpu_since = 0.0; }

        let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
        if mem_pct > 92.0 {
            if self.mem_since == 0.0 { self.mem_since = t; }
            if t - self.mem_since > 20.0 && t - self.last_sent > 600.0 {
                out.push(Alert {
                    metric: "memory".into(),
                    message: format!("Memory pressure high: {:.0}% used", mem_pct),
                    at: t,
                });
                self.last_sent = t;
            }
        } else { self.mem_since = 0.0; }

        if s.disk.read_rate + s.disk.write_rate > 400e6 {
            if self.disk_since == 0.0 { self.disk_since = t; }
            if t - self.disk_since > 30.0 && t - self.last_sent > 600.0 {
                out.push(Alert {
                    metric: "disk".into(),
                    message: format!(
                        "Heavy disk activity: {} read+write",
                        crate::engine::fmt_rate(s.disk.read_rate + s.disk.write_rate)
                    ),
                    at: t,
                });
                self.last_sent = t;
            }
        } else { self.disk_since = 0.0; }

        if s.net.rate_in + s.net.rate_out > 80e6 {
            if self.net_since == 0.0 { self.net_since = t; }
            if t - self.net_since > 30.0 && t - self.last_sent > 600.0 {
                out.push(Alert {
                    metric: "network".into(),
                    message: format!(
                        "Heavy network activity: {}",
                        crate::engine::fmt_rate(s.net.rate_in + s.net.rate_out)
                    ),
                    at: t,
                });
                self.last_sent = t;
            }
        } else { self.net_since = 0.0; }
        out
    }
}
