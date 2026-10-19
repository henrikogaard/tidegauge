//! Readings for the desktop widgets (`widgets/`).
//!
//! The widget extension is sandboxed and Tidegauge keeps no network access, so
//! the two meet in a file: a small JSON snapshot in the app group container
//! (`~/Library/Group Containers/<GROUP_ID>/`). The extension drops a marker
//! file there whenever macOS asks it for a timeline; the snapshot is written
//! only while that marker is recent, so with no widget on the desktop nothing
//! is built or written.

use crate::engine::{Hist, Snapshot};
use crate::theme::Metric;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The app group both the app and the widget extension declare. Prefixed with
/// the Team ID, which macOS accepts for Developer ID apps without a
/// provisioning profile.
pub const GROUP_ID: &str = "45AD7E7G5G.no.ogard.tidegauge";
pub const SNAPSHOT_FILE: &str = "widget-snapshot.json";
/// Touched by the widget extension each time macOS builds a timeline.
pub const MARKER_FILE: &str = "widget-active";

const SCHEMA: u32 = 1;
/// A snapshot older than this is "not running" to the widget.
pub const STALE_AFTER_SECS: u32 = 120;
/// Points kept per sparkline.
const POINTS: usize = 30;
/// The widget counts as present for this long after its last timeline.
const MARKER_MAX_AGE: Duration = Duration::from_secs(36 * 60 * 60);

#[derive(Serialize, Debug, PartialEq)]
pub struct Export {
    pub schema: u32,
    /// When this file was written, Unix seconds.
    pub written_at: f64,
    /// When the readings were sampled, Unix seconds. Staleness is judged on
    /// this one.
    pub sample_at: f64,
    pub stale_after_secs: u32,
    pub metrics: Vec<Reading>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Reading {
    /// `cpu`, `memory`, `gpu`, `battery`, `network` or `disk`.
    pub id: &'static str,
    pub available: bool,
    /// Percent (0-100) for every metric but `network`, where it is bytes
    /// received per second.
    pub value: f64,
    /// `percent` or `bytes_per_second`.
    pub unit: &'static str,
    /// The ring's fill, 0-1.
    pub fraction: f64,
    /// A second figure: bytes sent per second for `network`, bytes read and
    /// written per second for `disk`.
    pub detail: Option<f64>,
    /// `charging`, `on_battery` or `charged`, for `battery`.
    pub state: Option<&'static str>,
    /// Recent values, oldest first, at most `POINTS`.
    pub series: Vec<f64>,
    /// What the sparkline's top is: 100 for a percentage, the series' own
    /// peak for a rate.
    pub series_max: f64,
    /// 0xRRGGBB colours as `#rrggbb`, for a light and a dark widget.
    pub light: String,
    pub dark: String,
}

/// `hist` squeezed to `POINTS` means of equal runs, oldest first.
fn downsample(points: &[f64]) -> Vec<f64> {
    if points.len() <= POINTS {
        return points.to_vec();
    }
    (0..POINTS)
        .map(|i| {
            let from = i * points.len() / POINTS;
            let to = ((i + 1) * points.len() / POINTS).max(from + 1);
            let run = &points[from..to];
            run.iter().sum::<f64>() / run.len() as f64
        })
        .collect()
}

fn rgb(metric: Metric, dark: bool) -> String {
    format!("#{:06x}", crate::theme::metric_rgb(metric, dark) & 0xff_ffff)
}

fn reading(
    id: &'static str,
    metric: Metric,
    available: bool,
    value: f64,
    fraction: f64,
    hist: &Hist,
) -> Reading {
    let series = downsample(&hist.pts);
    Reading {
        id,
        available,
        value: finite(value),
        unit: "percent",
        fraction: finite(fraction).clamp(0., 1.),
        detail: None,
        state: None,
        series: series.into_iter().map(finite).collect(),
        series_max: 100.,
        light: rgb(metric, false),
        dark: rgb(metric, true),
    }
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.
    }
}

fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.
    } else {
        part as f64 / whole as f64
    }
}

pub fn build(snap: &Snapshot, written_at: f64) -> Export {
    let cpu = reading("cpu", Metric::Cpu, true, snap.cpu_pct, snap.cpu_pct / 100., &snap.cpu_hist);

    let memory_pct = share(snap.mem.used, snap.mem.total) * 100.;
    let memory = reading(
        "memory",
        Metric::Memory,
        snap.mem.total > 0,
        memory_pct,
        memory_pct / 100.,
        &snap.mem_hist,
    );

    let gpu_pct = snap.gpu.utilization.unwrap_or(0.);
    let gpu = reading(
        "gpu",
        Metric::Gpu,
        snap.gpu.supported && snap.gpu.utilization.is_some(),
        gpu_pct,
        gpu_pct / 100.,
        &snap.gpu_hist,
    );

    let battery_pct = snap.battery.percent.unwrap_or(0.);
    let mut battery = reading(
        "battery",
        Metric::Battery,
        snap.battery.present && snap.battery.percent.is_some(),
        battery_pct,
        battery_pct / 100.,
        &snap.batt_hist,
    );
    battery.state = Some(if snap.battery.is_charging {
        "charging"
    } else if snap.battery.on_battery {
        "on_battery"
    } else {
        "charged"
    });

    // A rate has no natural 100 %: the ring fills against the busiest moment
    // in the series, and never against less than 1 MB/s.
    let in_series = downsample(&snap.net_in_hist.pts);
    let out_series = downsample(&snap.net_out_hist.pts);
    let total_series: Vec<f64> = in_series
        .iter()
        .zip(out_series.iter().chain(std::iter::repeat(&0.)))
        .map(|(i, o)| i + o)
        .collect();
    let net_peak = total_series.iter().copied().fold(1_000_000., f64::max);
    let net_total = snap.net.rate_in + snap.net.rate_out;
    let mut network = reading(
        "network",
        Metric::Network,
        true,
        snap.net.rate_in,
        net_total / net_peak,
        &Hist::default(),
    );
    network.unit = "bytes_per_second";
    network.detail = Some(finite(snap.net.rate_out));
    network.series = total_series.into_iter().map(finite).collect();
    network.series_max = net_peak;

    let disk_pct = share(snap.disk.used, snap.disk.total) * 100.;
    let io_series: Vec<f64> = downsample(&snap.disk_r_hist.pts)
        .iter()
        .zip(downsample(&snap.disk_w_hist.pts).iter().chain(std::iter::repeat(&0.)))
        .map(|(r, w)| r + w)
        .collect();
    let io_peak = io_series.iter().copied().fold(1_000_000., f64::max);
    let mut disk = reading(
        "disk",
        Metric::Disk,
        snap.disk.total > 0,
        disk_pct,
        disk_pct / 100.,
        &Hist::default(),
    );
    disk.detail = Some(finite(snap.disk.read_rate + snap.disk.write_rate));
    disk.series = io_series.into_iter().map(finite).collect();
    disk.series_max = io_peak;

    Export {
        schema: SCHEMA,
        written_at,
        sample_at: finite(snap.at),
        stale_after_secs: STALE_AFTER_SECS,
        metrics: vec![cpu, memory, gpu, battery, network, disk],
    }
}

/// Writes `export` to `dir` so a reader sees the old file or the new one,
/// never half of one.
pub fn write_atomic(dir: &Path, export: &Export) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(export).map_err(std::io::Error::other)?;
    let temp = dir.join(format!("{SNAPSHOT_FILE}.tmp"));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, dir.join(SNAPSHOT_FILE))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Whether a widget has asked for a timeline recently.
pub fn widget_present(dir: &Path, now: SystemTime) -> bool {
    std::fs::metadata(dir.join(MARKER_FILE))
        .and_then(|m| m.modified())
        .is_ok_and(|touched| {
            now.duration_since(touched)
                .map_or(true, |age| age <= MARKER_MAX_AGE)
        })
}

/// Whether a snapshot sampled at `sample_at` is too old for a widget to show
/// at `now` (both Unix seconds). The extension applies the same rule.
pub fn is_stale(sample_at: f64, now: f64, stale_after_secs: u32) -> bool {
    now - sample_at > f64::from(stale_after_secs)
}

fn container() -> Option<PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().join("Library/Group Containers").join(GROUP_ID))
    })
    .clone()
}

/// Writes the readings for the widgets, if one is on the desktop. Cheap when
/// none is: one `stat`. Called by the sampler every few passes.
pub fn publish(snap: &Snapshot) {
    let Some(dir) = container() else { return };
    let now = SystemTime::now();
    if !widget_present(&dir, now) {
        return;
    }
    let seconds = now.duration_since(UNIX_EPOCH).map_or(0., |d| d.as_secs_f64());
    // A sample that is already old is not worth presenting as the latest.
    if is_stale(snap.at, seconds, STALE_AFTER_SECS) {
        return;
    }
    let _ = write_atomic(&dir, &build(snap, seconds));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tidegauge-widget-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn snapshot() -> Snapshot {
        let mut snap = Snapshot::default();
        snap.at = 1_000.;
        snap.cpu_pct = 42.5;
        for i in 0..100 {
            snap.cpu_hist.push(i as f64);
        }
        snap.mem.used = 8;
        snap.mem.total = 16;
        snap.net.rate_in = 2_000_000.;
        snap.net.rate_out = 500_000.;
        snap.net_in_hist.push(2_000_000.);
        snap.net_out_hist.push(500_000.);
        snap.disk.used = 300;
        snap.disk.total = 1_000;
        snap
    }

    #[test]
    fn schema_has_every_metric_and_the_fields_the_widget_reads() {
        let json = serde_json::to_value(build(&snapshot(), 1_005.)).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["written_at"], 1_005.);
        assert_eq!(json["sample_at"], 1_000.);
        assert_eq!(json["stale_after_secs"], 120);
        let ids: Vec<&str> = json["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["cpu", "memory", "gpu", "battery", "network", "disk"]);
        for metric in json["metrics"].as_array().unwrap() {
            for key in [
                "available", "value", "unit", "fraction", "detail", "state", "series",
                "series_max", "light", "dark",
            ] {
                assert!(metric.get(key).is_some(), "{} lacks {key}", metric["id"]);
            }
            for key in ["light", "dark"] {
                let colour = metric[key].as_str().unwrap();
                assert_eq!(colour.len(), 7, "{colour}");
                assert!(colour.starts_with('#'));
            }
        }
    }

    #[test]
    fn values_fractions_and_series_follow_the_snapshot() {
        let export = build(&snapshot(), 0.);
        let by = |id: &str| export.metrics.iter().find(|m| m.id == id).unwrap();
        let cpu = by("cpu");
        assert_eq!(cpu.value, 42.5);
        assert_eq!(cpu.fraction, 0.425);
        assert_eq!(cpu.series.len(), 30);
        assert!(cpu.series.windows(2).all(|w| w[0] < w[1]), "oldest first");
        assert_eq!(by("memory").value, 50.);
        assert_eq!(by("disk").fraction, 0.3);
        let network = by("network");
        assert_eq!((network.value, network.detail), (2_000_000., Some(500_000.)));
        assert_eq!(network.unit, "bytes_per_second");
        assert_eq!(network.series, [2_500_000.]);
        assert_eq!(network.fraction, 1.);
        // No battery and no GPU reading in the default snapshot.
        assert!(!by("battery").available);
        assert!(!by("gpu").available);
    }

    #[test]
    fn non_finite_and_out_of_range_numbers_stay_valid_json() {
        let mut snap = snapshot();
        snap.cpu_pct = f64::NAN;
        snap.at = f64::INFINITY;
        snap.gpu.utilization = Some(250.);
        snap.gpu.supported = true;
        let export = build(&snap, 0.);
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["metrics"][0]["value"], 0.);
        assert_eq!(json["sample_at"], 0.);
        let gpu = export.metrics.iter().find(|m| m.id == "gpu").unwrap();
        assert_eq!(gpu.fraction, 1.);
    }

    #[test]
    fn write_is_atomic_and_leaves_no_temporary_file() {
        let dir = temp("write");
        let first = build(&snapshot(), 1.);
        write_atomic(&dir, &first).unwrap();
        let mut snap = snapshot();
        snap.cpu_pct = 7.;
        write_atomic(&dir, &build(&snap, 2.)).unwrap();
        let read: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join(SNAPSHOT_FILE)).unwrap()).unwrap();
        assert_eq!(read["written_at"], 2.);
        assert_eq!(read["metrics"][0]["value"], 7.);
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [SNAPSHOT_FILE]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_write_removes_its_temporary_file() {
        let dir = temp("fail");
        // A directory where the target file should go makes the rename fail.
        std::fs::create_dir(dir.join(SNAPSHOT_FILE)).unwrap();
        assert!(write_atomic(&dir, &build(&snapshot(), 1.)).is_err());
        assert!(!dir.join(format!("{SNAPSHOT_FILE}.tmp")).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn staleness_is_judged_on_the_sample_time() {
        assert!(!is_stale(1_000., 1_120., 120));
        assert!(is_stale(1_000., 1_121., 120));
        assert!(!is_stale(1_000., 900., 120), "a clock set back is not stale");
    }

    #[test]
    fn nothing_is_written_without_a_recent_marker() {
        let dir = temp("marker");
        let now = SystemTime::now();
        assert!(!widget_present(&dir, now));
        std::fs::write(dir.join(MARKER_FILE), b"").unwrap();
        assert!(widget_present(&dir, now));
        assert!(widget_present(&dir, now - Duration::from_secs(60)));
        assert!(!widget_present(&dir, now + MARKER_MAX_AGE + Duration::from_secs(60)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn downsampling_keeps_short_series_and_averages_long_ones() {
        assert_eq!(downsample(&[1., 2., 3.]), [1., 2., 3.]);
        let long: Vec<f64> = (0..360).map(f64::from).collect();
        let short = downsample(&long);
        assert_eq!(short.len(), POINTS);
        assert_eq!(short[0], 5.5);
        assert_eq!(short[POINTS - 1], 353.5);
    }
}
