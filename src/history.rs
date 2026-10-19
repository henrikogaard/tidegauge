//! Local 30-day metric and app history in SQLite.
//!
//! Layout (schema version 2):
//! - `metric(ts, metric, value)`: headline series, raw for 24 h, then one row
//!   per five minutes.
//! - `apps(id, name, last_seen)` and `app_sample(ts, app_id, …)`: per-app
//!   samples with each name stored once; same raw/five-minute retention.
//! - `app_metric`: a view over the two, shaped like the version 1 table, so
//!   `history_browser` reads old and new databases with the same queries.
//!
//! One background thread migrates the schema and then, hourly, prunes past 30
//! days, rolls rows older than 24 h up to five-minute averages, and returns freed
//! pages to the filesystem.

#![allow(dead_code)]

use crate::engine::Snapshot;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

/// Samples are kept this long.
const RETENTION_SECS: f64 = 30.0 * 86400.0;
/// Samples older than this are averaged into one row per five minutes.
const RAW_SECS: f64 = 86400.0;
const ROLLUP_SECS: f64 = 300.0;
/// Rows rolled up per transaction, so the writer is never kept waiting long.
const ROLLUP_CHUNK_SECS: f64 = 3600.0;
/// A full VACUUM runs at most this often.
const VACUUM_EVERY_SECS: f64 = 86400.0;
/// Free space the hourly incremental vacuum bothers to return.
const RECLAIM_BYTES: i64 = 2 * 1024 * 1024;
/// Apps recorded per tick: the biggest by memory, then the busiest by CPU,
/// disk and network, then any app a user alert is currently watching.
const TOP_MEMORY: usize = 16;
const TOP_CPU: usize = 12;
const TOP_DISK: usize = 6;
const TOP_NET: usize = 6;
/// An app's `last_seen` is rewritten at most this often.
const SEEN_EVERY_SECS: f64 = 600.0;

/// Writer state: the connection once the schema is ready, and the id each
/// app name has in `apps` with when its `last_seen` was last written.
struct Writer {
    conn: Option<Connection>,
    ids: HashMap<String, (i64, f64)>,
}

static DB: LazyLock<Mutex<Writer>> = LazyLock::new(|| {
    Mutex::new(Writer {
        conn: None,
        ids: HashMap::new(),
    })
});
static STARTED: OnceLock<()> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);

pub fn database_path() -> std::path::PathBuf {
    directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|d| d.data_dir().join("history.db"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/tidegauge/history.db"))
}

/// Open the database with the per-connection settings: WAL with relaxed
/// syncing (a power cut can lose the last seconds, never corrupt the file),
/// a bounded WAL file, and a wait for the other connection instead of an
/// error.
fn open(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    // Takes effect only on a database with no tables yet; an existing one is
    // converted by the one full VACUUM in `shrink`.
    let _ = c.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;");
    let _ = c.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()));
    c.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA journal_size_limit=8388608;")?;
    Ok(c)
}

/// Create the current schema, or bring an older database up to it. The data
/// move runs in one transaction, so a reader sees the old or the new shape
/// and an interrupted run leaves the old one. Safe to call repeatedly.
pub fn init_schema(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS metric(
             ts REAL, metric TEXT, value REAL,
             PRIMARY KEY(ts, metric));
         CREATE INDEX IF NOT EXISTS idx_metric_key_ts ON metric(metric,ts);
         DROP INDEX IF EXISTS idx_metric_ts;
         CREATE TABLE IF NOT EXISTS battery_daily(
             day TEXT PRIMARY KEY, ts REAL, health REAL, cycles INTEGER,
             full_mah INTEGER, design_mah INTEGER);
         CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value REAL);
         CREATE TABLE IF NOT EXISTS apps(
             id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, last_seen REAL NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS app_sample(
             ts REAL NOT NULL, app_id INTEGER NOT NULL,
             cpu REAL, mem REAL, disk REAL, net REAL, power REAL,
             PRIMARY KEY(ts, app_id)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS idx_app_sample_app_ts ON app_sample(app_id, ts);",
    )?;
    let legacy: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_metric')",
        [],
        |r| r.get(0),
    )?;
    if legacy {
        let tx = c.unchecked_transaction()?;
        tx.execute_batch(
            "INSERT OR IGNORE INTO apps(name, last_seen)
                 SELECT app, MAX(ts) FROM app_metric
                 WHERE app IS NOT NULL AND ts IS NOT NULL GROUP BY app;
             INSERT OR REPLACE INTO app_sample(ts, app_id, cpu, mem, disk, net, power)
                 SELECT m.ts, a.id, m.cpu, m.mem, m.disk, m.net, m.power
                 FROM app_metric m JOIN apps a ON a.name = m.app WHERE m.ts IS NOT NULL;
             DROP TABLE app_metric;",
        )?;
        tx.commit()?;
    }
    c.execute_batch(
        "CREATE VIEW IF NOT EXISTS app_metric(ts, app, cpu, mem, disk, net, power) AS
             SELECT s.ts, a.name, s.cpu, s.mem, s.disk, s.net, s.power
             FROM app_sample s JOIN apps a ON a.id = s.app_id;
         PRAGMA user_version=2;",
    )?;
    Ok(())
}

/// Start the history thread on first use. It opens the database, migrates it
/// if needed, then maintains it hourly. Nothing is recorded until the schema
/// is ready, which on an old database means a few seconds once.
fn start() {
    STARTED.get_or_init(|| {
        let _ = std::thread::Builder::new()
            .name("tidegauge-history".into())
            .spawn(|| {
                let path = database_path();
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let Ok(maintenance) = open(&path) else { return };
                if init_schema(&maintenance).is_err() {
                    return;
                }
                if let Ok(writer) = open(&path) {
                    DB.lock().unwrap().conn = Some(writer);
                    READY.store(true, Ordering::Release);
                }
                loop {
                    let _ = maintain(&maintenance, crate::metrics::now_unix());
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            });
    });
}

/// Persist one tick's headline metrics and the recorded apps.
pub fn record(s: &Snapshot) {
    start();
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let pins = Pins::from_settings(&crate::settings::Settings::load());
    let mut g = DB.lock().unwrap();
    let Writer { conn, ids } = &mut *g;
    let Some(c) = conn.as_ref() else { return };
    record_on(c, ids, s, pins);
}

/// Apps kept beyond the top lists: those a user alert watches and that are
/// currently past its threshold.
#[derive(Clone, Copy, Default)]
struct Pins {
    cpu_pct: Option<f64>,
    mem_bytes: Option<f64>,
}

impl Pins {
    fn from_settings(s: &crate::settings::Settings) -> Self {
        const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
        Self {
            cpu_pct: s
                .alert_app_enabled
                .then(|| crate::settings::valid_app_threshold(s.alert_app_cpu_pct)),
            mem_bytes: s
                .alert_app_mem_enabled
                .then(|| crate::settings::valid_app_memory_gb(s.alert_app_mem_gb) * GIB),
        }
    }

    fn holds(&self, a: &crate::processes::AppGroup) -> bool {
        self.cpu_pct.is_some_and(|t| a.cpu_pct >= t)
            || self.mem_bytes.is_some_and(|t| a.mem_bytes as f64 >= t)
    }
}

fn app_id(
    c: &Connection,
    ids: &mut HashMap<String, (i64, f64)>,
    name: &str,
    ts: f64,
) -> Option<i64> {
    if let Some((id, seen)) = ids.get_mut(name) {
        if ts - *seen < SEEN_EVERY_SECS && ts >= *seen {
            return Some(*id);
        }
        // The row may have been pruned (or its id given to another app) since
        // the id was cached: a refresh that matches no row means look it up again.
        let touched = c
            .execute(
                "UPDATE apps SET last_seen=? WHERE id=? AND name=?",
                rusqlite::params![ts, *id, name],
            )
            .unwrap_or(0);
        if touched > 0 {
            *seen = ts;
            return Some(*id);
        }
        ids.remove(name);
    }
    c.execute(
        "INSERT OR IGNORE INTO apps(name, last_seen) VALUES(?, ?)",
        rusqlite::params![name, ts],
    )
    .ok()?;
    let id: i64 = c
        .query_row("SELECT id FROM apps WHERE name=?", [name], |r| r.get(0))
        .ok()?;
    let _ = c.execute(
        "UPDATE apps SET last_seen=max(last_seen, ?) WHERE id=?",
        rusqlite::params![ts, id],
    );
    ids.insert(name.to_owned(), (id, ts));
    Some(id)
}

fn record_on(c: &Connection, ids: &mut HashMap<String, (i64, f64)>, s: &Snapshot, pins: Pins) {
    if !s.at.is_finite() || s.at <= 0. {
        return;
    }
    let Ok(transaction) = c.unchecked_transaction() else {
        return;
    };
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
        ("temp_chip", s.temps.chip),
        ("temp_ssd", s.temps.ssd),
        ("temp_battery", s.temps.battery),
    ] {
        if let Some(value) = value.filter(|v| v.is_finite() && *v >= 0.) {
            let _ = stmt.execute(rusqlite::params![ts, metric, value]);
        }
    }
    drop(stmt);
    if let Ok(mut st) = c.prepare(
        "INSERT OR REPLACE INTO app_sample(ts,app_id,cpu,mem,disk,net,power) VALUES(?,?,?,?,?,?,?)",
    ) {
        for a in recorded_apps(&s.apps, pins) {
            let Some(id) = app_id(c, ids, &a.name, ts) else {
                continue;
            };
            let _ = st.execute(rusqlite::params![
                ts,
                id,
                a.cpu_pct,
                a.mem_bytes as f64,
                a.disk_read_rate + a.disk_write_rate,
                a.net_in_rate + a.net_out_rate,
                a.power_w
            ]);
        }
    }
    record_battery_day(c, s);
    let _ = transaction.commit();
}

/// The apps one tick records, about 40: the biggest by memory (the order
/// `apps` arrives in) and the busiest by CPU, disk and network, so an app
/// that is busy but small is never left out of its own history, plus every
/// app a user alert is watching and has passed the threshold of.
fn recorded_apps(
    apps: &[crate::processes::AppGroup],
    pins: Pins,
) -> Vec<&crate::processes::AppGroup> {
    type App = crate::processes::AppGroup;
    fn add<'a>(chosen: &mut Vec<&'a App>, ranked: Vec<&'a App>, n: usize) {
        for app in ranked.into_iter().take(n) {
            if !chosen.iter().any(|c| c.name == app.name) {
                chosen.push(app);
            }
        }
    }
    fn top(apps: &[App], key: fn(&App) -> f64) -> Vec<&App> {
        let mut ranked: Vec<&App> = apps.iter().filter(|a| key(a) > 0.).collect();
        ranked.sort_by(|a, b| key(b).total_cmp(&key(a)));
        ranked
    }
    let mut chosen: Vec<&App> = apps.iter().take(TOP_MEMORY).collect();
    add(&mut chosen, top(apps, |a| a.cpu_pct), TOP_CPU);
    add(
        &mut chosen,
        top(apps, |a| a.disk_read_rate + a.disk_write_rate),
        TOP_DISK,
    );
    add(
        &mut chosen,
        top(apps, |a| a.net_in_rate + a.net_out_rate),
        TOP_NET,
    );
    add(
        &mut chosen,
        apps.iter().filter(|a| pins.holds(a)).collect(),
        usize::MAX,
    );
    chosen
}

/// One row per local day with the day's latest battery health, kept beyond
/// the 30-day window: a battery wears over months, and a row is tiny.
fn record_battery_day(c: &Connection, s: &Snapshot) {
    let b = &s.battery;
    let Some(health) = b.health.filter(|h| b.present && h.is_finite() && *h > 0.) else {
        return;
    };
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

fn meta(c: &Connection, key: &str) -> Option<f64> {
    c.query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))
        .ok()
}

fn set_meta(c: &Connection, key: &str, value: f64) -> rusqlite::Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES(?, ?)",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Hourly upkeep, in the order that leaves the least to copy: drop what is
/// past retention, average what is past 24 h, then give the space back.
pub fn maintain(c: &Connection, now: f64) -> rusqlite::Result<()> {
    prune(c, now)?;
    rollup(c, now)?;
    let _ = c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    shrink(c, now)
}

fn prune(c: &Connection, now: f64) -> rusqlite::Result<()> {
    let cutoff = now - RETENTION_SECS;
    c.execute("DELETE FROM metric WHERE ts < ?", [cutoff])?;
    c.execute("DELETE FROM app_sample WHERE ts < ?", [cutoff])?;
    c.execute(
        "DELETE FROM apps WHERE last_seen < ?1
         AND NOT EXISTS (SELECT 1 FROM app_sample s WHERE s.app_id = apps.id)",
        [cutoff],
    )?;
    Ok(())
}

/// Average rows older than 24 h into one row per five minutes, centred in
/// the window (7 d and 30 d charts draw buckets of 56 min and 4 h), an hour of data per transaction. Each series keeps a watermark in
/// `meta`, so a row is rolled once; running again over rolled rows changes
/// nothing. Values the charts would reject (negative or absurd) stay out of
/// the average, and a legacy zero GPU or battery reading, which meant
/// "unavailable", does too. Only averages are kept: History plots means.
fn rollup(c: &Connection, now: f64) -> rusqlite::Result<()> {
    let cutoff = ((now - RAW_SECS) / ROLLUP_SECS).floor() * ROLLUP_SECS;
    c.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS rollup_app(ts REAL, app_id INTEGER, cpu REAL, mem REAL, disk REAL, net REAL, power REAL);
         CREATE TEMP TABLE IF NOT EXISTS rollup_metric(ts REAL, metric TEXT, value REAL);",
    )?;
    for (key, table) in [("rollup_app", "app_sample"), ("rollup_metric", "metric")] {
        let first: Option<f64> =
            c.query_row(&format!("SELECT MIN(ts) FROM {table}"), [], |r| r.get(0))?;
        let Some(first) = first else { continue };
        let mut from = meta(c, key).unwrap_or_else(|| (first / ROLLUP_SECS).floor() * ROLLUP_SECS);
        while from < cutoff {
            let to = (from + ROLLUP_CHUNK_SECS).min(cutoff);
            let tx = c.unchecked_transaction()?;
            if table == "metric" {
                rollup_metric(&tx, from, to)?;
            } else {
                rollup_app(&tx, from, to)?;
            }
            set_meta(&tx, key, to)?;
            tx.commit()?;
            from = to;
        }
    }
    Ok(())
}

fn rollup_app(c: &Connection, from: f64, to: f64) -> rusqlite::Result<()> {
    let mean = |column: &str| {
        [
            "AVG(CASE WHEN ",
            column,
            " >= 0 AND ",
            column,
            " < 1e300 THEN ",
            column,
            " END)",
        ]
        .concat()
    };
    c.execute("DELETE FROM rollup_app", [])?;
    c.execute(
        &format!(
            "INSERT INTO rollup_app
             SELECT CAST(ts / ?3 AS INTEGER) * ?3 + ?3 / 2, app_id, {}, {}, {}, {}, {}
             FROM app_sample WHERE ts >= ?1 AND ts < ?2 GROUP BY 1, 2",
            mean("cpu"),
            mean("mem"),
            mean("disk"),
            mean("net"),
            mean("power")
        ),
        [from, to, ROLLUP_SECS],
    )?;
    c.execute(
        "DELETE FROM app_sample WHERE ts >= ?1 AND ts < ?2",
        [from, to],
    )?;
    c.execute(
        "INSERT OR REPLACE INTO app_sample SELECT * FROM rollup_app",
        [],
    )?;
    Ok(())
}

fn rollup_metric(c: &Connection, from: f64, to: f64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM rollup_metric", [])?;
    c.execute(
        "INSERT INTO rollup_metric
         SELECT ts, metric, value FROM (
             SELECT CAST(ts / ?3 AS INTEGER) * ?3 + ?3 / 2 AS ts, metric,
                    AVG(CASE WHEN value >= 0 AND value < 1e300
                              AND (value > 0 OR metric NOT IN ('gpu', 'batt'))
                         THEN value END) AS value
             FROM metric WHERE ts >= ?1 AND ts < ?2 GROUP BY 1, 2)
         WHERE value IS NOT NULL",
        [from, to, ROLLUP_SECS],
    )?;
    c.execute("DELETE FROM metric WHERE ts >= ?1 AND ts < ?2", [from, to])?;
    c.execute(
        "INSERT OR REPLACE INTO metric SELECT * FROM rollup_metric",
        [],
    )?;
    Ok(())
}

/// Return freed pages to the filesystem. The first run is one full VACUUM
/// that also switches the file to incremental auto-vacuum; after that only
/// the cheap incremental step runs, and a full VACUUM happens at most daily.
fn shrink(c: &Connection, now: f64) -> rusqlite::Result<()> {
    let free: i64 = c.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let page: i64 = c.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let converted = meta(c, "auto_vacuum_converted").is_some();
    let due = now - meta(c, "last_vacuum").unwrap_or(0.) >= VACUUM_EVERY_SECS;
    if !converted {
        if due {
            c.execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM;")?;
            set_meta(c, "auto_vacuum_converted", 1.)?;
            set_meta(c, "last_vacuum", now)?;
        }
    } else if free * page >= RECLAIM_BYTES {
        c.query_row("PRAGMA incremental_vacuum(4096)", [], |_| Ok(()))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(()),
                e => Err(e),
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_SCHEMA: &str = "CREATE TABLE metric(ts REAL, metric TEXT, value REAL, PRIMARY KEY(ts, metric));
        CREATE TABLE app_metric(ts REAL, app TEXT, cpu REAL, mem REAL, disk REAL, net REAL, power REAL, PRIMARY KEY(ts, app));
        CREATE INDEX idx_metric_ts ON metric(ts);
        CREATE INDEX idx_metric_key_ts ON metric(metric,ts);
        CREATE INDEX idx_app_name_ts ON app_metric(app,ts);
        CREATE INDEX idx_app_ts ON app_metric(ts);
        CREATE TABLE battery_daily(day TEXT PRIMARY KEY, ts REAL, health REAL, cycles INTEGER, full_mah INTEGER, design_mah INTEGER);";

    fn fresh() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init_schema(&c).unwrap();
        c
    }

    fn count(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn app(name: &str, mem: u64, cpu: f64) -> crate::processes::AppGroup {
        crate::processes::AppGroup {
            name: name.into(),
            mem_bytes: mem,
            cpu_pct: cpu,
            ..Default::default()
        }
    }

    #[test]
    fn battery_health_is_kept_once_a_day_and_only_when_measured() {
        let c = fresh();
        let mut ids = HashMap::new();
        let mut s = Snapshot {
            at: 1_800_000_000.,
            ..Snapshot::default()
        };
        record_on(&c, &mut ids, &s, Pins::default());
        let rows = |c: &Connection| count(c, "SELECT COUNT(*) FROM battery_daily");
        assert_eq!(rows(&c), 0, "no battery, no row");
        s.battery.present = true;
        s.battery.health = Some(91.5);
        s.battery.cycles = Some(120);
        record_on(&c, &mut ids, &s, Pins::default());
        s.at += 60.;
        s.battery.health = Some(91.4);
        record_on(&c, &mut ids, &s, Pins::default());
        assert_eq!(rows(&c), 1, "one row per day");
        assert_eq!(
            c.query_row("SELECT health FROM battery_daily", [], |r| r
                .get::<_, f64>(0))
                .unwrap(),
            91.4,
            "the day's latest reading"
        );
    }

    #[test]
    fn recorded_apps_are_capped_and_keep_the_busy_and_the_watched() {
        // Arrives sorted by memory, as the engine delivers it.
        let mut apps: Vec<_> = (0..70)
            .map(|i| app(&format!("big{i}"), 1000 - i, 0.))
            .collect();
        apps.push(app("busy-small", 1, 90.));
        apps.push(app("watched", 2, 10.));
        for i in 0..30 {
            apps.push(app(&format!("busy{i}"), 1, 20. + f64::from(i)));
        }
        let names = |pins| {
            recorded_apps(&apps, pins)
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        };
        let plain = names(Pins::default());
        assert!(plain.len() <= 40, "{} apps", plain.len());
        assert!(plain.contains(&"big0".to_string()) && plain.contains(&"busy-small".to_string()));
        assert!(
            !plain.contains(&"big60".to_string()),
            "idle apps past the memory cut stay out"
        );
        assert!(!plain.contains(&"watched".to_string()));
        let mut unique = plain.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), plain.len(), "no app twice");
        let pinned = names(Pins {
            cpu_pct: Some(10.),
            mem_bytes: None,
        });
        assert!(
            pinned.contains(&"watched".to_string()),
            "an app past an alert threshold is kept"
        );
        let watched = names(Pins {
            cpu_pct: None,
            mem_bytes: Some(2.),
        });
        assert!(watched.contains(&"watched".to_string()));
    }

    #[test]
    fn an_app_pruned_and_seen_again_keeps_its_samples_visible() {
        let c = fresh();
        let mut ids = HashMap::new();
        let start = 1_800_000_000.0_f64;
        let mut s = Snapshot {
            at: start,
            ..Snapshot::default()
        };
        s.apps = vec![app("Other", 5, 1.), app("Editor", 5, 1.)];
        record_on(&c, &mut ids, &s, Pins::default());
        // Thirty-one days later only Other is seen and upkeep prunes Editor.
        let later = start + 31. * 86400.;
        s.at = later;
        s.apps = vec![app("Other", 5, 1.)];
        record_on(&c, &mut ids, &s, Pins::default());
        maintain(&c, later).unwrap();
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM apps WHERE name='Editor'"),
            0
        );
        // A new app takes over Editor's freed id, then Editor returns while the
        // writer still caches the old id.
        s.at = later + 10.;
        s.apps = vec![app("Third", 3, 1.)];
        record_on(&c, &mut ids, &s, Pins::default());
        s.at = later + 20.;
        s.apps = vec![
            app("Editor", 7, 2.),
            app("Third", 3, 1.),
            app("Other", 5, 1.),
        ];
        record_on(&c, &mut ids, &s, Pins::default());
        let mem = |name: &str| -> Option<f64> {
            c.query_row(
                "SELECT mem FROM app_metric WHERE app=? AND ts=?",
                rusqlite::params![name, later + 20.],
                |r| r.get(0),
            )
            .ok()
        };
        assert_eq!(mem("Editor"), Some(7.));
        assert_eq!(mem("Third"), Some(3.));
        assert_eq!(mem("Other"), Some(5.));
        assert_eq!(count(&c, "SELECT COUNT(*) FROM apps"), 3);
    }

    #[test]
    fn app_names_are_stored_once_and_seen_times_are_throttled() {
        let c = fresh();
        let mut ids = HashMap::new();
        let mut s = Snapshot {
            at: 1_800_000_000.,
            ..Snapshot::default()
        };
        s.apps = vec![app("Editor", 5, 1.), app("Browser", 9, 2.)];
        for i in 0..5 {
            s.at = 1_800_000_000. + f64::from(i) * 10.;
            record_on(&c, &mut ids, &s, Pins::default());
        }
        assert_eq!(count(&c, "SELECT COUNT(*) FROM apps"), 2);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM app_sample"), 10);
        assert_eq!(
            count(&c, "SELECT COUNT(*) FROM app_metric WHERE app='Editor'"),
            5,
            "the view shows names"
        );
        let seen: f64 = c
            .query_row("SELECT last_seen FROM apps WHERE name='Editor'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            seen, 1_800_000_000.,
            "last_seen is not rewritten every tick"
        );
        s.at = 1_800_000_700.;
        record_on(&c, &mut ids, &s, Pins::default());
        let seen: f64 = c
            .query_row("SELECT last_seen FROM apps WHERE name='Editor'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(seen, 1_800_000_700.);
    }

    #[test]
    fn optional_history_records_measured_zero_not_unavailable_placeholders() {
        let c = fresh();
        let mut ids = HashMap::new();
        let mut snapshot = Snapshot {
            at: 1000.,
            ..Snapshot::default()
        };
        record_on(&c, &mut ids, &snapshot, Pins::default());
        assert_eq!(count(&c, "SELECT COUNT(*) FROM metric WHERE metric IN ('gpu','gpu_measured','batt_measured','power_measured','mem_used')"),0);
        snapshot.at = 1010.;
        snapshot.gpu.utilization = Some(0.);
        snapshot.battery.percent = Some(0.);
        snapshot.mem.total = 4096;
        snapshot.mem.used = 2048;
        record_on(&c, &mut ids, &snapshot, Pins::default());
        assert_eq!(
            c.query_row(
                "SELECT value FROM metric WHERE ts=1010 AND metric='gpu_measured'",
                [],
                |r| r.get::<_, f64>(0)
            )
            .unwrap(),
            0.
        );
        assert_eq!(
            c.query_row(
                "SELECT value FROM metric WHERE ts=1010 AND metric='mem_used'",
                [],
                |r| r.get::<_, f64>(0)
            )
            .unwrap(),
            50.
        );
        assert_eq!(count(&c, "SELECT COUNT(*) FROM metric WHERE ts=1000"), 6);
    }

    #[test]
    fn a_version_1_database_migrates_without_losing_rows_and_still_reads() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(OLD_SCHEMA).unwrap();
        c.execute_batch(
            "INSERT INTO metric VALUES(1000,'cpu',10),(1010,'cpu',20);
             INSERT INTO app_metric VALUES(1000,'Editor',5,100,1,2,0),(1010,'Editor',15,200,3,4,0),(1010,'Browser',50,900,0,0,0);
             INSERT INTO battery_daily VALUES('2026-10-01',1,90,100,4000,4500);",
        )
        .unwrap();
        init_schema(&c).unwrap();
        init_schema(&c).unwrap(); // idempotent
        assert_eq!(count(&c, "SELECT COUNT(*) FROM metric"), 2);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM battery_daily"), 1);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM apps"), 2);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM app_sample"), 3);
        assert_eq!(
            count(
                &c,
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='app_metric'"
            ),
            0,
            "the old table is gone"
        );
        assert_eq!(count(&c, "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('idx_metric_ts','idx_app_ts','idx_app_name_ts')"), 0, "redundant indexes are dropped");
        let editor: (f64, f64, f64, f64, f64) = c
            .query_row(
                "SELECT cpu, mem, disk, net, power FROM app_metric WHERE app='Editor' AND ts=1010",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(editor, (15., 200., 3., 4., 0.));
        assert_eq!(count(&c, "SELECT user_version FROM pragma_user_version"), 2);
        // The History reader answers from the migrated database.
        let request = crate::history_browser::Request {
            metric: crate::history_browser::Metric::Memory,
            hours: 1,
            app: Some("Editor".into()),
            end: 3600.,
        };
        let data = crate::history_browser::query_connection(&c, &request).unwrap();
        assert_eq!(data.apps, ["Browser", "Editor"]);
        assert_eq!(data.values.iter().flatten().count(), 1);
        assert_eq!(
            data.values.iter().flatten().next(),
            Some(&150.),
            "1000 s and 1010 s share a 20 s bucket"
        );
        assert_eq!(data.recording_began, Some(1000.));
    }

    #[test]
    fn rows_past_24_hours_become_five_minute_averages_and_ranges_still_answer() {
        let c = fresh();
        let now = 1_800_000_000.0_f64;
        let old = ((now - 3.0 * 86400.) / 300.).floor() * 300.; // window-aligned, 3 days back
        c.execute(
            "INSERT INTO apps(id,name,last_seen) VALUES(1,'Editor',?)",
            [now],
        )
        .unwrap();
        for (i, (cpu, mem)) in [(10., 100.), (30., 300.), (50., -1.)]
            .into_iter()
            .enumerate()
        {
            c.execute(
                "INSERT INTO app_sample VALUES(?,1,?,?,?,?,0)",
                rusqlite::params![old + 10. * i as f64, cpu, mem, 4.0, 8.0],
            )
            .unwrap();
        }
        // The next window holds one sample, and a recent sample stays raw.
        c.execute(
            "INSERT INTO app_sample VALUES(?,1,70,1,0,0,0)",
            [old + 305.],
        )
        .unwrap();
        c.execute(
            "INSERT INTO app_sample VALUES(?,1,90,1,0,0,0)",
            [now - 100.],
        )
        .unwrap();
        for (i, v) in [2., 4.].into_iter().enumerate() {
            c.execute(
                "INSERT INTO metric VALUES(?,'cpu',?)",
                rusqlite::params![old + 20. * i as f64, v],
            )
            .unwrap();
        }
        c.execute_batch(&format!(
            "INSERT INTO metric VALUES({t},'gpu',0),({t2},'gpu',40),({n},'cpu',77);",
            t = old,
            t2 = old + 5.,
            n = now - 50.
        ))
        .unwrap();
        maintain(&c, now).unwrap();
        let rows: Vec<(f64, f64, f64, f64)> = c
            .prepare("SELECT ts, cpu, mem, disk FROM app_sample ORDER BY ts")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!(
            rows[0],
            (old + 150., 30., 200., 4.),
            "mean of valid values, the negative memory left out"
        );
        assert_eq!(rows[1].0, old + 450.);
        assert_eq!(rows[1].1, 70.);
        assert_eq!(rows[2], (now - 100., 90., 1., 0.), "recent rows stay raw");
        let metric = |m: &str| -> Vec<(f64, f64)> {
            c.prepare("SELECT ts, value FROM metric WHERE metric=? ORDER BY ts")
                .unwrap()
                .query_map([m], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .flatten()
                .collect()
        };
        assert_eq!(metric("cpu"), [(old + 150., 3.), (now - 50., 77.)]);
        assert_eq!(
            metric("gpu"),
            [(old + 150., 40.)],
            "a legacy zero is not averaged in"
        );
        // Rolling again changes nothing.
        let before = count(&c, "SELECT COUNT(*) FROM app_sample");
        maintain(&c, now + 7200.).unwrap();
        assert_eq!(count(&c, "SELECT COUNT(*) FROM app_sample"), before);
        // The 7 d and 30 d ranges answer from the rolled rows.
        for hours in [168, 720] {
            let request = crate::history_browser::Request {
                metric: crate::history_browser::Metric::Cpu,
                hours,
                app: Some("Editor".into()),
                end: now,
            };
            let data = crate::history_browser::query_connection(&c, &request).unwrap();
            let values: Vec<f64> = data.values.iter().flatten().copied().collect();
            assert!(
                !values.is_empty() && values.iter().all(|v| (30. ..=90.).contains(v)),
                "{hours} h: {values:?}"
            );
            assert_eq!(data.apps, ["Editor"]);
        }
    }

    #[test]
    fn retention_removes_old_rows_and_apps_nothing_else_uses() {
        let c = fresh();
        let now = 1_800_000_000.0_f64;
        c.execute_batch(&format!(
            "INSERT INTO apps VALUES(1,'Gone',{old}),(2,'Here',{now});
             INSERT INTO app_sample VALUES({old},1,1,1,1,1,1),({n},2,1,1,1,1,1);
             INSERT INTO metric VALUES({old},'cpu',1),({n},'cpu',2);",
            old = now - 31. * 86400.,
            n = now - 10.
        ))
        .unwrap();
        maintain(&c, now).unwrap();
        assert_eq!(count(&c, "SELECT COUNT(*) FROM app_sample"), 1);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM metric"), 1);
        assert_eq!(
            c.query_row("SELECT name FROM apps", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "Here"
        );
    }

    #[test]
    fn the_file_shrinks_after_a_rollup_and_vacuums_at_most_daily() {
        let path = std::env::temp_dir().join(format!(
            "tidegauge-history-shrink-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let c = open(&path).unwrap();
        c.execute_batch(OLD_SCHEMA).unwrap();
        let now = 1_800_000_000.0_f64;
        let tx = c.unchecked_transaction().unwrap();
        for tick in 0..2000 {
            let ts = now - 5. * 86400. + f64::from(tick) * 10.;
            for app in 0..30 {
                tx.execute(
                    "INSERT INTO app_metric VALUES(?,?,1,2,3,4,5)",
                    rusqlite::params![ts, format!("App number {app}")],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
        init_schema(&c).unwrap();
        let before = std::fs::metadata(&path).unwrap().len();
        maintain(&c, now).unwrap();
        let _ = c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        let after = std::fs::metadata(&path).unwrap().len();
        assert!(after < before / 4, "{before} -> {after}");
        // 2000 ticks 10 s apart span 334 minutes, 67 or 68 windows; each app keeps one row per window.
        let rows = count(&c, "SELECT COUNT(*) FROM app_sample");
        assert!(rows == 30 * 67 || rows == 30 * 68, "{rows}");
        assert_eq!(count(&c, "SELECT user_version FROM pragma_user_version"), 2);
        assert!(meta(&c, "last_vacuum").is_some());
        // Inside a day, a second full vacuum does not run.
        let stamp = meta(&c, "last_vacuum").unwrap();
        maintain(&c, now + 3600.).unwrap();
        assert_eq!(meta(&c, "last_vacuum"), Some(stamp));
        drop(c);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
