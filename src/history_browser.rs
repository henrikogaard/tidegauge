//! Bounded read-only history queries. Empty buckets remain missing, never zero-filled.

use crate::locale::text as l;
use rusqlite::{params, Connection, OpenFlags};

pub const BUCKETS: usize = 180;
pub const RANGES: [(u32, (&str, &str)); 5] = [
    (1, ("1 h", "1 t")),
    (12, ("12 h", "12 t")),
    (24, ("24 h", "24 t")),
    (168, ("7 d", "7 d")),
    (720, ("30 d", "30 d")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Cpu,
    Memory,
    DiskRead,
    DiskWrite,
    Download,
    Upload,
    Gpu,
    Battery,
    Power,
}

impl Metric {
    pub const ALL: [Self; 9] = [
        Self::Cpu,
        Self::Memory,
        Self::DiskRead,
        Self::DiskWrite,
        Self::Download,
        Self::Upload,
        Self::Gpu,
        Self::Battery,
        Self::Power,
    ];
    pub const APP: [Self; 4] = [Self::Cpu, Self::Memory, Self::DiskRead, Self::Download];

    pub fn label(self, app: bool) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => l("Memory", "Minne"),
            Self::DiskRead if app => l("Disk I/O", "Diskaktivitet"),
            Self::DiskRead => l("Disk read", "Disklesing"),
            Self::DiskWrite => l("Disk write", "Diskskriving"),
            Self::Download if app => l("Network I/O", "Nettverksaktivitet"),
            Self::Download => l("Download", "Nedlasting"),
            Self::Upload => l("Upload", "Opplasting"),
            Self::Gpu => "GPU",
            Self::Battery => l("Battery", "Batteri"),
            Self::Power => l("Power", "Effekt"),
        }
    }

    pub fn unit(self, app: bool) -> &'static str {
        match self {
            Self::Memory if app => "B",
            Self::Cpu | Self::Memory | Self::Gpu | Self::Battery => "%",
            Self::Power => "W",
            _ => "B/s",
        }
    }

    /// A stable name for exported files, the same in every language.
    pub fn id(self, app: bool) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::DiskRead if app => "disk_io",
            Self::DiskRead => "disk_read",
            Self::DiskWrite => "disk_write",
            Self::Download if app => "network_io",
            Self::Download => "download",
            Self::Upload => "upload",
            Self::Gpu => "gpu",
            Self::Battery => "battery",
            Self::Power => "power",
        }
    }

    fn keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Cpu => ("cpu", ""),
            Self::Memory => ("mem_used", ""),
            Self::DiskRead => ("disk_r", ""),
            Self::DiskWrite => ("disk_w", ""),
            Self::Download => ("net_in", ""),
            Self::Upload => ("net_out", ""),
            Self::Gpu => ("gpu_measured", "gpu"),
            Self::Battery => ("batt_measured", "batt"),
            Self::Power => ("power_measured", ""),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub metric: Metric,
    pub hours: u32,
    pub app: Option<String>,
    pub end: f64,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            metric: Metric::Cpu,
            hours: 12,
            app: None,
            end: crate::metrics::now_unix(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Data {
    pub values: Vec<Option<f64>>,
    pub counts: Vec<u64>,
    pub apps: Vec<String>,
    pub start: f64,
    pub end: f64,
    /// The first sample this source ever recorded, when that falls inside
    /// the range: recording began then, so the plot is empty before it.
    pub recording_began: Option<f64>,
}

pub fn load(request: &Request) -> Result<Data, String> {
    let path = crate::history::database_path();
    if !path.exists() {
        return Ok(empty(request));
    }
    let query = || -> rusqlite::Result<Data> {
        let mut c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        c.busy_timeout(std::time::Duration::from_millis(500))?;
        let transaction = c.transaction()?;
        query_connection(&transaction, request)
    };
    query().map_err(read_error)
}

/// One app's CPU and memory over the last hour, for its row in Apps &
/// processes. Same rules as History: gaps stay gaps.
pub fn app_hour(app: &str) -> Result<(Data, Data), String> {
    let request = |metric| Request { metric, hours: 1, app: Some(app.to_owned()), end: crate::metrics::now_unix() };
    Ok((load(&request(Metric::Cpu))?, load(&request(Metric::Memory))?))
}

fn read_error(error: rusqlite::Error) -> String {
    if matches!(error, rusqlite::Error::SqliteFailure(code, _) if matches!(code.code, rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase))
    {
        l("The history database is damaged. Back it up before recovery; refreshing will not repair it.", "Historikkdatabasen er skadet. Ta en sikkerhetskopi før gjenoppretting; oppdatering reparerer ikke databasen.").into()
    } else {
        l(
            "History could not be read. Tidegauge tries again in a minute.",
            "Historikken kunne ikke leses. Tidegauge prøver igjen om et minutt.",
        )
        .into()
    }
}

fn empty(request: &Request) -> Data {
    Data {
        values: vec![None; BUCKETS],
        counts: vec![0; BUCKETS],
        start: request.end - f64::from(request.hours) * 3600.,
        end: request.end,
        ..Data::default()
    }
}

pub fn query_connection(c: &Connection, request: &Request) -> rusqlite::Result<Data> {
    if !RANGES.iter().any(|(h, _)| *h == request.hours) || !request.end.is_finite() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut data = empty(request);
    let width = (data.end - data.start) / BUCKETS as f64;
    let mut apps = c.prepare("SELECT DISTINCT app FROM app_metric WHERE ts >= ?1 AND ts < ?2 ORDER BY app COLLATE NOCASE, app")?;
    data.apps = apps
        .query_map(params![data.start, data.end], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let rows: Vec<(usize, f64, u64)> = if let Some(app) = &request.app {
        // Per-app energy counters have not been validated as measured watts.
        let column = match request.metric {
            Metric::Cpu => "cpu",
            Metric::Memory => "mem",
            Metric::DiskRead => "disk",
            Metric::Download => "net",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        let sql = format!("SELECT CAST((ts - ?1) / ?3 AS INTEGER), AVG({column}), COUNT({column}) FROM app_metric WHERE ts >= ?1 AND ts < ?2 AND app = ?4 AND {column} >= 0 AND {column} < 1e300 GROUP BY 1 ORDER BY 1");
        let mut statement = c.prepare(&sql)?;
        let result = statement
            .query_map(params![data.start, data.end, width, app], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        result
    } else {
        let (key, legacy) = request.metric.keys();
        // Positive legacy GPU/battery samples are usable; zero could mean unavailable.
        // Legacy system/per-app power was estimated, so it is never mixed with measurements.
        let mut statement = c.prepare(
            "SELECT CAST((ts - ?1) / ?3 AS INTEGER), AVG(value), COUNT(value)
             FROM metric m WHERE ts >= ?1 AND ts < ?2 AND value >= 0 AND value < 1e300
             AND (metric = ?4 OR (metric = ?5 AND ?5 != '' AND value > 0
                 AND NOT EXISTS (SELECT 1 FROM metric newer WHERE newer.ts = m.ts AND newer.metric = ?4)))
             GROUP BY 1 ORDER BY 1"
        )?;
        let result = statement
            .query_map(params![data.start, data.end, width, key, legacy], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        result
    };
    for (index, value, count) in rows {
        if index < BUCKETS && value.is_finite() {
            data.values[index] = Some(value);
            data.counts[index] = count;
        }
    }
    // The real first sample of this source, not a bucket boundary: any
    // system metric for System, the app's own rows for an app.
    let first: Option<f64> = match &request.app {
        Some(app) => c.query_row("SELECT MIN(ts) FROM app_metric WHERE app = ?1", params![app], |r| r.get(0))?,
        None => c.query_row("SELECT MIN(ts) FROM metric", [], |r| r.get(0))?,
    };
    data.recording_began = first.filter(|ts| *ts > data.start && *ts < data.end);
    Ok(data)
}

/// Every recorded sample in the request's range, oldest first, under the
/// same rules as the chart: no legacy placeholders, no unvalidated power.
pub fn samples_connection(c: &Connection, request: &Request) -> rusqlite::Result<Vec<(f64, f64)>> {
    if !RANGES.iter().any(|(h, _)| *h == request.hours) || !request.end.is_finite() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let start = request.end - f64::from(request.hours) * 3600.;
    let map = |r: &rusqlite::Row| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?));
    if let Some(app) = &request.app {
        let column = match request.metric {
            Metric::Cpu => "cpu",
            Metric::Memory => "mem",
            Metric::DiskRead => "disk",
            Metric::Download => "net",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        let sql = format!("SELECT ts, {column} FROM app_metric WHERE ts >= ?1 AND ts < ?2 AND app = ?3 AND {column} >= 0 AND {column} < 1e300 ORDER BY ts");
        let mut statement = c.prepare(&sql)?;
        let rows = statement.query_map(params![start, request.end, app], map)?.collect();
        rows
    } else {
        let (key, legacy) = request.metric.keys();
        let mut statement = c.prepare(
            "SELECT ts, value FROM metric m WHERE ts >= ?1 AND ts < ?2 AND value >= 0 AND value < 1e300
             AND (metric = ?3 OR (metric = ?4 AND ?4 != '' AND value > 0
                 AND NOT EXISTS (SELECT 1 FROM metric newer WHERE newer.ts = m.ts AND newer.metric = ?3)))
             ORDER BY ts",
        )?;
        let rows = statement.query_map(params![start, request.end, key, legacy], map)?.collect();
        rows
    }
}

/// The samples as CSV: one row each, local time first, with the source,
/// metric and unit spelled out so the file stands on its own.
pub fn csv(request: &Request, samples: &[(f64, f64)]) -> String {
    let app = request.app.is_some();
    let source = cell(request.app.as_deref().unwrap_or("System"));
    let (metric, unit) = (request.metric.id(app), request.metric.unit(app));
    let mut out = String::from("local_time,unix_time,source,metric,unit,value\n");
    for (ts, value) in samples {
        out.push_str(&format!("{},{ts},{source},{metric},{unit},{value}\n", format_time(*ts, c"%Y-%m-%d %H:%M:%S")));
    }
    out
}

/// A quoted text cell. A name starting like a formula gets a leading
/// apostrophe, so a spreadsheet shows it as text instead of running it.
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text.to_string() };
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// A new file in `dir` named `name`, or "name 2.csv", "name 3.csv"… when
/// that is taken, so an earlier export is never overwritten.
fn create_new(dir: &std::path::Path, name: &str) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    let stem = name.strip_suffix(".csv").unwrap_or(name);
    for n in 1..100 {
        let path = if n == 1 { dir.join(name) } else { dir.join(format!("{stem} {n}") + ".csv") };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::ErrorKind::AlreadyExists.into())
}

/// Writes the request's samples to a CSV file in Downloads and returns it.
pub fn export(request: &Request) -> Result<std::path::PathBuf, String> {
    let path = crate::history::database_path();
    let samples = if path.exists() {
        let read = || -> rusqlite::Result<Vec<(f64, f64)>> {
            let mut c = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            c.busy_timeout(std::time::Duration::from_millis(500))?;
            let transaction = c.transaction()?;
            samples_connection(&transaction, request)
        };
        read().map_err(read_error)?
    } else {
        Vec::new()
    };
    let downloads = directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(std::path::Path::to_path_buf))
        .ok_or_else(|| l("Downloads could not be found.", "Fant ikke Nedlastinger.").to_string())?;
    let failed = |_| l("The file could not be saved in Downloads.", "Filen kunne ikke lagres i Nedlastinger.").to_string();
    let (path, mut file) = create_new(&downloads, &file_name(request)).map_err(failed)?;
    std::io::Write::write_all(&mut file, csv(request, &samples).as_bytes()).map_err(failed)?;
    Ok(path)
}

/// "Tidegauge Safari cpu 12h 2026-10-16 0815.csv"; no slashes or colons from
/// an app's name reach the file system.
fn file_name(request: &Request) -> String {
    let app = request.app.is_some();
    let source: String = request
        .app
        .as_deref()
        .unwrap_or("System")
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') || c.is_control() { '-' } else { c })
        .collect();
    let parts = [
        "Tidegauge".to_string(),
        source.trim().to_string(),
        request.metric.id(app).to_string(),
        request.hours.to_string() + "h",
        format_time(request.end, c"%Y-%m-%d %H%M"),
    ];
    parts.join(" ") + ".csv"
}

pub fn local_time(ts: f64) -> String {
    format_time(ts, c"%Y-%m-%d %H:%M")
}

fn format_time(ts: f64, format: &std::ffi::CStr) -> String {
    let time = ts as libc::time_t;
    let mut date = unsafe { std::mem::zeroed::<libc::tm>() };
    let mut output = [0u8; 48];
    unsafe {
        if libc::localtime_r(&time, &mut date).is_null() {
            return "—".into();
        }
        let size = libc::strftime(
            output.as_mut_ptr().cast(),
            output.len(),
            format.as_ptr(),
            &date,
        );
        String::from_utf8_lossy(&output[..size]).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corruption_is_not_reported_as_a_refreshable_error() {
        for code in [rusqlite::ffi::SQLITE_CORRUPT, rusqlite::ffi::SQLITE_NOTADB] {
            let error = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None);
            assert_eq!(read_error(error), l("The history database is damaged. Back it up before recovery; refreshing will not repair it.", "Historikkdatabasen er skadet. Ta en sikkerhetskopi før gjenoppretting; oppdatering reparerer ikke databasen."));
        }
        assert_eq!(
            read_error(rusqlite::Error::InvalidQuery),
            l(
                "History could not be read. Tidegauge tries again in a minute.",
                "Historikken kunne ikke leses. Tidegauge prøver igjen om et minutt."
            )
        );
    }

    fn database() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE metric(ts REAL,metric TEXT,value REAL,PRIMARY KEY(ts,metric)); CREATE TABLE app_metric(ts REAL,app TEXT,cpu REAL,mem REAL,disk REAL,net REAL,power REAL,PRIMARY KEY(ts,app));").unwrap();
        c
    }

    #[test]
    fn ranges_preserve_boundaries_gaps_means_and_genuine_zero() {
        for (hours, _) in RANGES {
            let c = database();
            let r = Request {
                metric: Metric::Cpu,
                hours,
                app: None,
                end: f64::from(hours) * 3600.,
            };
            let width = r.end / BUCKETS as f64;
            for (ts, value) in [
                (-1., 99.),
                (0., 10.),
                (1., 30.),
                (width * 2., 0.),
                (r.end, 99.),
            ] {
                c.execute("INSERT INTO metric VALUES(?1,'cpu',?2)", params![ts, value])
                    .unwrap();
            }
            let d = query_connection(&c, &r).unwrap();
            assert_eq!(d.values.len(), BUCKETS);
            assert_eq!(d.values[0], Some(20.));
            assert_eq!(d.counts[0], 2);
            assert_eq!(d.values[1], None);
            assert_eq!(d.values[2], Some(0.));
            assert_eq!(d.counts.iter().sum::<u64>(), 3);
        }
    }

    #[test]
    fn recording_start_is_the_first_real_sample() {
        let c = database();
        let r = Request { metric: Metric::Cpu, hours: 12, app: None, end: 43200. };
        c.execute_batch("INSERT INTO metric VALUES(5000.5,'memory',1),(9000,'cpu',2);").unwrap();
        assert_eq!(query_connection(&c, &r).unwrap().recording_began, Some(5000.5), "any metric marks the start, not the bucket");
        c.execute("INSERT INTO metric VALUES(-10,'memory',1)", []).unwrap();
        assert_eq!(query_connection(&c, &r).unwrap().recording_began, None, "recording began before the range");
    }

    #[test]
    fn legacy_optional_zeros_and_synthetic_power_are_not_measurements() {
        let c = database();
        c.execute_batch("INSERT INTO metric VALUES(0,'gpu',0),(240,'gpu',20),(480,'gpu_measured',0),(480,'gpu',99),(0,'power',5);").unwrap();
        let mut r = Request {
            metric: Metric::Gpu,
            hours: 12,
            app: None,
            end: 43200.,
        };
        let d = query_connection(&c, &r).unwrap();
        assert_eq!(&d.values[..3], &[None, Some(20.), Some(0.)]);
        r.metric = Metric::Power;
        assert!(query_connection(&c, &r)
            .unwrap()
            .values
            .iter()
            .all(Option::is_none));
    }

    #[test]
    fn the_hour_range_uses_twenty_second_intervals() {
        let c = database();
        c.execute_batch("INSERT INTO app_metric VALUES(10,'Editor',5,1,0,0,0),(15,'Editor',15,1,0,0,0),(25,'Editor',30,1,0,0,0);").unwrap();
        let r = Request { metric: Metric::Cpu, hours: 1, app: Some("Editor".into()), end: 3600. };
        let d = query_connection(&c, &r).unwrap();
        assert_eq!(d.values.len(), BUCKETS);
        assert_eq!(&d.values[..3], &[Some(10.), Some(30.), None], "0–20 s averages two samples, 20–40 s holds one");
    }

    #[test]
    fn exports_hold_every_sample_in_range_and_no_placeholders() {
        let c = database();
        c.execute_batch(
            "INSERT INTO metric VALUES(-1,'cpu',9),(10,'cpu',5),(20,'cpu',0),(30,'gpu',0),(40,'gpu',20),(3600,'cpu',7);
             INSERT INTO app_metric VALUES(15,'Editor',12.5,2048,0,0,0),(25,'Other',1,1,0,0,0);",
        )
        .unwrap();
        let mut r = Request { metric: Metric::Cpu, hours: 1, app: None, end: 3600. };
        assert_eq!(samples_connection(&c, &r).unwrap(), [(10., 5.), (20., 0.)], "start in, end out, zero kept");
        r.metric = Metric::Gpu;
        assert_eq!(samples_connection(&c, &r).unwrap(), [(40., 20.)], "a legacy zero is not a sample");
        r.metric = Metric::Memory;
        r.app = Some("Editor".into());
        let samples = samples_connection(&c, &r).unwrap();
        assert_eq!(samples, [(15., 2048.)]);
        let text = csv(&r, &samples);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("local_time,unix_time,source,metric,unit,value"));
        assert!(lines.next().unwrap().ends_with(",15,\"Editor\",memory,B,2048"));
        r.metric = Metric::Power;
        assert!(samples_connection(&c, &r).is_err(), "per-app power is not exported");
    }

    #[test]
    fn formula_like_names_are_kept_as_text() {
        for name in ["=HYPERLINK(\"x\")", "+1", "-2", "@SUM(A1)"] {
            assert!(cell(name).starts_with("\"'"), "{name} → {}", cell(name));
        }
        assert_eq!(cell("Safari"), "\"Safari\"");
    }

    #[test]
    fn a_second_export_gets_its_own_file() {
        let dir = std::env::temp_dir().join(format!("tidegauge-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (first, _) = create_new(&dir, "Tidegauge x.csv").unwrap();
        std::fs::write(&first, "kept").unwrap();
        let (second, _) = create_new(&dir, "Tidegauge x.csv").unwrap();
        assert_eq!(second.file_name().unwrap(), "Tidegauge x 2.csv");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "kept", "the first export is untouched");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn export_names_are_safe_and_quotes_are_escaped() {
        let r = Request { metric: Metric::Cpu, hours: 12, app: Some("a/b:\"c\"".into()), end: 0. };
        let name = file_name(&r);
        assert!(name.starts_with("Tidegauge a-b-\"c\" cpu 12h ") && name.ends_with(".csv"), "{name}");
        assert!(csv(&r, &[(1., 2.)]).contains(",\"a/b:\"\"c\"\"\","));
    }

    #[test]
    fn app_filters_are_bound_and_absence_is_not_zero() {
        let c = database();
        c.execute_batch("INSERT INTO app_metric VALUES(0,'Editor',10,2048,3,4,5),(240,'Browser',80,4096,8,9,10);").unwrap();
        let mut r = Request {
            metric: Metric::Memory,
            hours: 12,
            app: Some("Editor".into()),
            end: 43200.,
        };
        let d = query_connection(&c, &r).unwrap();
        assert_eq!(d.apps, ["Browser", "Editor"]);
        assert_eq!(&d.values[..2], &[Some(2048.), None]);
        r.app = Some("' OR 1=1 --".into());
        assert!(query_connection(&c, &r)
            .unwrap()
            .values
            .iter()
            .all(Option::is_none));
        r.metric = Metric::Power;
        assert!(query_connection(&c, &r).is_err());
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM app_metric", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn dense_history_is_bounded_and_survives_reopening() {
        let path = std::env::temp_dir().join(format!(
            "tidegauge-history-reopen-{}.db",
            std::process::id()
        ));
        let mut c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE metric(ts REAL,metric TEXT,value REAL,PRIMARY KEY(ts,metric)); CREATE TABLE app_metric(ts REAL,app TEXT,cpu REAL,mem REAL,disk REAL,net REAL,power REAL,PRIMARY KEY(ts,app));").unwrap();
        let tx = c.transaction().unwrap();
        for i in 0..4320 {
            tx.execute(
                "INSERT INTO metric VALUES(?1,'cpu',?2)",
                params![i * 10, i % 24],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        drop(c);
        let c = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let request = Request {
            metric: Metric::Cpu,
            hours: 12,
            app: None,
            end: 43200.,
        };
        let data = query_connection(&c, &request).unwrap();
        assert_eq!(data.values, vec![Some(11.5); BUCKETS]);
        assert_eq!(data.counts, vec![24; BUCKETS]);
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM metric", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            4320
        );
        drop(c);
        std::fs::remove_file(path).unwrap();
    }
}
