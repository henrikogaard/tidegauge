//! Immutable system summaries; no writes to the source History database.
use crate::locale::text as l;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};

pub const WINDOWS: [u32; 3] = [5, 15, 30];
pub const KEYS: [&str; 7] = [
    "cpu",
    "mem_used",
    "swap_used",
    "disk_r",
    "disk_w",
    "net_in",
    "net_out",
];
const MAX_GAP: f64 = 30.;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub start: f64,
    pub end: f64,
    pub minutes: u32,
    pub metrics: Vec<Measure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Measure {
    pub key: String,
    pub mean: Option<f64>,
    pub covered: f64,
}

pub fn label(key: &str) -> &'static str {
    match key {
        "cpu" => "CPU",
        "mem_used" => l("Used memory", "Brukt minne"),
        "swap_used" => "Swap",
        "disk_r" => l("Disk read", "Disklesing"),
        "disk_w" => l("Disk write", "Diskskriving"),
        "net_in" => l("Download", "Nedlasting"),
        "net_out" => l("Upload", "Opplasting"),
        _ => "—",
    }
}

pub fn format(key: &str, value: f64) -> String {
    match key {
        "cpu" | "mem_used" => format!("{value:.1}%"),
        "swap_used" => crate::engine::fmt_bytes(value),
        _ => crate::engine::fmt_rate(value),
    }
}

pub fn summarize(points: &[(f64, f64)], start: f64, end: f64) -> (Option<f64>, f64) {
    let mut integral = 0.;
    let mut covered = 0.;
    for pair in points.windows(2) {
        let (t, value) = pair[0];
        let (next, next_value) = pair[1];
        if !t.is_finite()
            || !next.is_finite()
            || !value.is_finite()
            || value < 0.
            || !next_value.is_finite()
            || next_value < 0.
            || next <= t
            || next - t > MAX_GAP
        {
            continue;
        }
        let span = (next.min(end) - t.max(start)).max(0.);
        integral += value * span;
        covered += span;
    }
    ((covered > 0.).then(|| integral / covered), covered)
}

pub fn query(c: &Connection, minutes: u32, end: f64) -> rusqlite::Result<Summary> {
    if !WINDOWS.contains(&minutes) || !end.is_finite() || end <= 0. {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let start = end - minutes as f64 * 60.;
    let mut metrics = Vec::new();
    let mut statement = c.prepare(
        "SELECT ts,value FROM metric WHERE metric=?1 AND ts>=?2 AND ts<=?3 ORDER BY ts LIMIT 10001",
    )?;
    for key in KEYS {
        let points = statement
            .query_map(params![key, start - MAX_GAP, end], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<(f64, f64)>>>()?;
        if points.len() > 10000 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (mean, covered) = summarize(&points, start, end);
        metrics.push(Measure {
            key: key.into(),
            mean,
            covered,
        });
    }
    Ok(Summary {
        start,
        end,
        minutes,
        metrics,
    })
}

pub fn capture(minutes: u32) -> Result<Summary, String> {
    let query_result = || -> rusqlite::Result<Summary> {
        let mut c = Connection::open_with_flags(
            crate::history::database_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        c.busy_timeout(std::time::Duration::from_millis(500))?;
        let tx = c.transaction()?;
        query(&tx, minutes, crate::metrics::now_unix())
    };
    query_result().map_err(|_| {
        l(
            "History could not be read. Preserve the database before attempting recovery.",
            "Historikken kunne ikke leses. Bevar databasen før du forsøker gjenoppretting.",
        )
        .into()
    })
}

fn path() -> std::path::PathBuf {
    crate::history::database_path().with_file_name("system-baseline-v1.json")
}

pub fn save(summary: &Summary) -> Result<(), String> {
    let result = || -> anyhow::Result<()> {
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(summary)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    };
    result().map_err(|_| {
        l(
            "The baseline could not be saved.",
            "Utgangspunktet kunne ikke lagres.",
        )
        .into()
    })
}

pub fn load() -> Result<Option<Summary>, String> {
    let path = path();
    if !path.exists() {
        return Ok(None);
    }
    let result = || -> anyhow::Result<Summary> {
        if std::fs::metadata(&path)?.len() > 32768 {
            anyhow::bail!("size");
        }
        let s: Summary = serde_json::from_slice(&std::fs::read(path)?)?;
        if !WINDOWS.contains(&s.minutes)
            || !s.start.is_finite()
            || !s.end.is_finite()
            || (s.end - s.start - s.minutes as f64 * 60.).abs() > 0.01
            || s.metrics.len() != KEYS.len()
        {
            anyhow::bail!("shape");
        }
        for (m, key) in s.metrics.iter().zip(KEYS) {
            if m.key != key
                || !m.covered.is_finite()
                || m.covered < 0.
                || m.covered > s.minutes as f64 * 60.
                || m.mean.is_some_and(|v| !v.is_finite() || v < 0.)
            {
                anyhow::bail!("values");
            }
        }
        Ok(s)
    };
    result().map(Some).map_err(|_| {
        l(
            "The saved baseline could not be read. It has not been replaced.",
            "Det lagrede utgangspunktet kunne ikke leses. Det er ikke erstattet.",
        )
        .into()
    })
}

pub fn difference(before: &Measure, after: &Measure, seconds: f64) -> Option<f64> {
    if before.key != after.key || before.covered < seconds * 0.8 || after.covered < seconds * 0.8 {
        return None;
    }
    let a = before.mean?;
    let b = after.mean?;
    (a > 0.).then(|| (b - a) / a * 100.)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn time_weighted_gaps_and_boundaries() {
        assert_eq!(
            summarize(&[(0., 10.), (10., 30.), (30., 50.)], 0., 30.),
            (Some(70. / 3.), 30.)
        );
        assert_eq!(
            summarize(&[(0., 10.), (10., 30.), (100., 50.), (110., 70.)], 0., 120.),
            (Some(30.), 20.)
        );
        assert_eq!(summarize(&[(0., 10.), (10., 30.)], 5., 8.), (Some(10.), 3.));
        assert_eq!(summarize(&[(0., 10.)], 0., 10.), (None, 0.));
    }
    #[test]
    fn zero_and_low_coverage_never_claim_change() {
        let a = Measure {
            key: "cpu".into(),
            mean: Some(0.),
            covered: 300.,
        };
        let mut b = a.clone();
        b.mean = Some(1.);
        assert_eq!(difference(&a, &b, 300.), None);
        b.covered = 20.;
        assert_eq!(difference(&b, &b, 300.), None);
    }
    #[test]
    fn query_preserves_rows_and_frozen_summary() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE metric(ts REAL,metric TEXT,value REAL); INSERT INTO metric VALUES (700,'cpu',10),(710,'cpu',30),(720,'cpu',20);").unwrap();
        let s = query(&c, 5, 1000.).unwrap();
        assert_eq!(s.metrics[0].mean, Some(20.));
        assert_eq!(s.metrics[0].covered, 20.);
        assert_eq!(s.metrics[1].mean, None);
        let frozen = serde_json::to_vec(&s).unwrap();
        c.execute("DELETE FROM metric", []).unwrap();
        let restored: Summary = serde_json::from_slice(&frozen).unwrap();
        assert_eq!(restored.metrics[0].mean, Some(20.));
    }
}
