//! A live JSON file of the headline readings, for scripts.
//!
//! Off by default (Settings → General). While on, every sampling pass writes
//! `live.json` next to Tidegauge's other local data. The file is replaced
//! atomically, so a reader sees the old file or the new one, never half of
//! one. Turning the setting off deletes it. This is separate from the widgets'
//! own snapshot (`widget_export`), whose file and schema stay as they are.

use crate::engine::Snapshot;
use crate::metrics::Pressure;
use crate::processes::AppGroup;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const FILE: &str = "live.json";
/// Bump when a field is renamed, removed or changes unit. Adding a field does
/// not change it.
const SCHEMA: u32 = 1;
/// The busiest apps listed.
const TOP_APPS: usize = 10;

#[derive(Serialize, Debug, PartialEq)]
pub struct Live {
    pub schema: u32,
    /// When this file was written, Unix seconds.
    pub written_at: f64,
    /// When the readings were sampled, Unix seconds.
    pub sample_at: f64,
    pub cpu: Cpu,
    pub memory: Memory,
    pub disk: Disk,
    pub network: Network,
    pub gpu: Gpu,
    pub battery: Battery,
    pub temperatures_c: Temperatures,
    /// The whole Mac's power draw in watts, when macOS reports it.
    pub power_watts: Option<f64>,
    /// The busiest apps by CPU, busiest first.
    pub top_apps: Vec<App>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Cpu {
    /// Share of the whole Mac, 0-100.
    pub percent: f64,
    pub cores: usize,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Memory {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub used_percent: f64,
    pub swap_used_bytes: u64,
    /// `normal`, `warning` or `critical`; null when unknown.
    pub pressure: Option<&'static str>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Disk {
    pub read_bytes_per_second: f64,
    pub write_bytes_per_second: f64,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Network {
    pub in_bytes_per_second: f64,
    pub out_bytes_per_second: f64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Gpu {
    /// 0-100; null when the Mac does not report it.
    pub percent: Option<f64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Battery {
    pub present: bool,
    /// 0-100; null without a battery.
    pub percent: Option<f64>,
    /// `charging`, `on_battery` or `charged`; null without a battery.
    pub state: Option<&'static str>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Temperatures {
    pub chip: Option<f64>,
    pub ssd: Option<f64>,
    pub battery: Option<f64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub bundle_id: Option<String>,
    /// 100 is one full core, as Activity Monitor shows it, whatever the
    /// per-core setting says.
    pub cpu_core_percent: f64,
    pub memory_bytes: u64,
    pub pids: Vec<i32>,
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.
    }
}

fn opt(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite())
}

pub fn build(snap: &Snapshot, written_at: f64) -> Live {
    let used_percent = if snap.mem.total == 0 {
        0.
    } else {
        snap.mem.used as f64 / snap.mem.total as f64 * 100.
    };
    let state = (snap.battery.present && snap.battery.percent.is_some()).then(|| {
        if snap.battery.is_charging {
            "charging"
        } else if snap.battery.on_battery {
            "on_battery"
        } else {
            "charged"
        }
    });
    let mut apps: Vec<&AppGroup> = snap.apps.iter().collect();
    apps.sort_by(|a, b| finite(b.cpu_pct).total_cmp(&finite(a.cpu_pct)));
    Live {
        schema: SCHEMA,
        written_at,
        sample_at: finite(snap.at),
        cpu: Cpu {
            percent: finite(snap.cpu_pct),
            cores: snap.core_pct.len(),
        },
        memory: Memory {
            used_bytes: snap.mem.used,
            total_bytes: snap.mem.total,
            used_percent,
            swap_used_bytes: snap.mem.swap_used,
            pressure: snap.mem.pressure.map(|p| match p {
                Pressure::Normal => "normal",
                Pressure::Warning => "warning",
                Pressure::Critical => "critical",
            }),
        },
        disk: Disk {
            read_bytes_per_second: finite(snap.disk.read_rate),
            write_bytes_per_second: finite(snap.disk.write_rate),
            used_bytes: snap.disk.used,
            total_bytes: snap.disk.total,
        },
        network: Network {
            in_bytes_per_second: finite(snap.net.rate_in),
            out_bytes_per_second: finite(snap.net.rate_out),
        },
        gpu: Gpu {
            percent: if snap.gpu.supported {
                opt(snap.gpu.utilization)
            } else {
                None
            },
        },
        battery: Battery {
            present: snap.battery.present,
            percent: opt(snap.battery.percent).filter(|_| snap.battery.present),
            state,
        },
        temperatures_c: Temperatures {
            chip: opt(snap.temps.chip),
            ssd: opt(snap.temps.ssd),
            battery: opt(snap.temps.battery),
        },
        power_watts: opt(snap.power_w),
        top_apps: apps
            .into_iter()
            .take(TOP_APPS)
            .map(|a| App {
                name: a.name.clone(),
                bundle_id: a.bundle_id.clone(),
                cpu_core_percent: finite(a.cpu_pct),
                memory_bytes: a.mem_bytes,
                pids: a.pids.clone(),
            })
            .collect(),
    }
}

/// Tidegauge's Application Support folder, where History also lives.
fn folder() -> PathBuf {
    directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/tmp/tidegauge"))
}

/// Where the file is written.
pub fn path() -> PathBuf {
    folder().join(FILE)
}

/// Writes `live` to `dir` so a reader sees the old file or the new one.
pub fn write_atomic(dir: &Path, live: &Live) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(live).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{FILE}.tmp"));
    let result = (|| {
        // Private: the file names apps and process ids.
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&bytes)?;
        std::fs::rename(&temp, dir.join(FILE))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Deletes the file and any temp file a failed write left; a missing one is
/// fine.
pub fn remove_in(dir: &Path) -> std::io::Result<()> {
    let mut result = Ok(());
    for name in [FILE.to_string(), format!("{FILE}.tmp")] {
        match std::fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => result = Err(e),
            _ => {}
        }
    }
    result
}

/// Deletes the file, for turning the setting off.
pub fn remove() {
    let _ = remove_in(&folder());
}

/// One pass: writes the file when `enabled`, otherwise writes nothing.
fn apply(dir: &Path, enabled: bool, snap: &Snapshot, now: f64) -> std::io::Result<()> {
    if enabled {
        write_atomic(dir, &build(snap, now))
    } else {
        remove_in(dir)
    }
}

/// Called by the sampler after each pass. Off, it does nothing beyond one
/// delete of a file an earlier run (or an earlier setting) may have left.
pub fn publish(snap: &Snapshot, enabled: bool) {
    static MAY_EXIST: AtomicBool = AtomicBool::new(true);
    if enabled {
        MAY_EXIST.store(true, Ordering::Relaxed);
    } else if !MAY_EXIST.load(Ordering::Relaxed) {
        return;
    }
    let done = apply(&folder(), enabled, snap, crate::metrics::now_unix()).is_ok();
    // A failed delete is tried again on the next pass.
    if !enabled && done {
        MAY_EXIST.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tidegauge-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn app(name: &str, cpu: f64, pids: Vec<i32>) -> AppGroup {
        AppGroup {
            key: name.into(),
            name: name.into(),
            bundle_id: Some(format!("com.example.{name}")),
            icon_path: None,
            pids,
            cpu_pct: cpu,
            mem_bytes: 1_000,
            disk_read_rate: 0.,
            disk_write_rate: 0.,
            net_in_rate: 0.,
            net_out_rate: 0.,
            power_w: 0.,
            gpu_pct: 0.,
            is_system: false,
        }
    }

    fn snapshot() -> Snapshot {
        let mut snap = Snapshot::default();
        snap.at = 1_000.;
        snap.cpu_pct = 42.5;
        snap.core_pct = vec![0.; 8];
        snap.mem.used = 8;
        snap.mem.total = 16;
        snap.mem.pressure = Some(Pressure::Warning);
        snap.net.rate_in = 2_000_000.;
        snap.net.rate_out = 500_000.;
        snap.disk.read_rate = 10.;
        snap.disk.write_rate = 20.;
        snap.power_w = Some(12.5);
        snap.temps.chip = Some(55.);
        snap.apps = (0..12)
            .map(|i| app(&format!("app{i}"), i as f64 * 10., vec![i, i + 100]))
            .collect();
        snap
    }

    #[test]
    fn json_has_the_documented_names_and_units() {
        let json = serde_json::to_value(build(&snapshot(), 1_005.)).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["written_at"], 1_005.);
        assert_eq!(json["sample_at"], 1_000.);
        assert_eq!(json["cpu"]["percent"], 42.5);
        assert_eq!(json["cpu"]["cores"], 8);
        assert_eq!(json["memory"]["used_bytes"], 8);
        assert_eq!(json["memory"]["total_bytes"], 16);
        assert_eq!(json["memory"]["used_percent"], 50.);
        assert_eq!(json["memory"]["pressure"], "warning");
        assert_eq!(json["disk"]["read_bytes_per_second"], 10.);
        assert_eq!(json["disk"]["write_bytes_per_second"], 20.);
        assert_eq!(json["network"]["in_bytes_per_second"], 2_000_000.);
        assert_eq!(json["network"]["out_bytes_per_second"], 500_000.);
        assert_eq!(json["temperatures_c"]["chip"], 55.);
        assert!(json["temperatures_c"]["ssd"].is_null());
        assert_eq!(json["power_watts"], 12.5);
        assert!(
            json["gpu"]["percent"].is_null(),
            "no GPU reading is null, not 0"
        );
        assert_eq!(json["battery"]["present"], false);
        assert!(json["battery"]["state"].is_null());
    }

    #[test]
    fn lists_the_ten_busiest_apps_first() {
        let live = build(&snapshot(), 0.);
        assert_eq!(live.top_apps.len(), TOP_APPS);
        assert_eq!(live.top_apps[0].name, "app11");
        assert_eq!(live.top_apps[0].cpu_core_percent, 110.);
        assert_eq!(live.top_apps[0].pids, vec![11, 111]);
        assert_eq!(live.top_apps[9].name, "app2");
        let json = serde_json::to_value(&live).unwrap();
        assert_eq!(json["top_apps"][0]["bundle_id"], "com.example.app11");
        assert_eq!(json["top_apps"][0]["memory_bytes"], 1_000);
    }

    #[test]
    fn non_finite_numbers_become_zero_or_null() {
        let mut snap = snapshot();
        snap.cpu_pct = f64::NAN;
        snap.power_w = Some(f64::INFINITY);
        let json = serde_json::to_string(&build(&snap, 0.)).unwrap();
        assert!(!json.contains("NaN") && !json.contains("inf"));
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
    }

    #[test]
    fn writes_atomically_and_replaces_the_file() {
        let dir = temp("write");
        write_atomic(&dir, &build(&snapshot(), 1.)).unwrap();
        let mut snap = snapshot();
        snap.cpu_pct = 7.;
        write_atomic(&dir, &build(&snap, 2.)).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(json["cpu"]["percent"], 7.);
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            [std::ffi::OsString::from(FILE)],
            "no temp file is left behind"
        );
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "only the owner can read it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temp() {
        let dir = temp("fail");
        write_atomic(&dir, &build(&snapshot(), 1.)).unwrap();
        // A directory where the temp file goes makes the create fail.
        std::fs::create_dir(dir.join(format!("{FILE}.tmp"))).unwrap();
        assert!(write_atomic(&dir, &build(&snapshot(), 2.)).is_err());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(json["written_at"], 1.);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_written_while_off_and_the_file_is_deleted() {
        let dir = temp("off");
        apply(&dir, false, &snapshot(), 1.).unwrap();
        assert!(!dir.join(FILE).exists(), "off writes nothing");
        apply(&dir, true, &snapshot(), 1.).unwrap();
        assert!(dir.join(FILE).exists());
        apply(&dir, false, &snapshot(), 2.).unwrap();
        assert!(!dir.join(FILE).exists(), "turning it off deletes the file");
        std::fs::write(dir.join(format!("{FILE}.tmp")), b"x").unwrap();
        apply(&dir, false, &snapshot(), 3.).unwrap();
        assert!(
            !dir.join(format!("{FILE}.tmp")).exists(),
            "a leftover temp file goes too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
