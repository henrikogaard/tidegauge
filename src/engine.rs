//! The sampling engine: runs on a background thread every ~2s, diffs counters,
//! produces a SystemSnapshot consumed by the UI, status bar, history & alerts.

#![allow(dead_code)]

use crate::metrics::*;
use crate::processes::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Hist {
    pub pts: Vec<f64>, // oldest -> newest, max ~360 per metric
    pub max_len: usize,
}
impl Hist {
    pub fn push(&mut self, v: f64) {
        self.pts.push(v);
        if self.pts.len() > self.max_len.max(1) {
            let drop = self.pts.len() - self.max_len.max(1);
            self.pts.drain(0..drop);
        }
    }
    pub fn latest(&self) -> f64 {
        self.pts.last().copied().unwrap_or(0.0)
    }
}

#[derive(Clone, Default)]
pub struct NetSample {
    pub rate_in: f64,
    pub rate_out: f64,
    pub active: bool,
    pub total_in: u64,
    pub total_out: u64,
    pub primary: String,
    pub kind: String,
    pub peak_in: f64,
    pub peak_out: f64,
}

#[derive(Clone, Default)]
pub struct DiskSample {
    pub read_rate: f64,
    pub write_rate: f64,
    pub used: u64,
    pub total: u64,
    pub free: u64,
    pub volume_count: usize,
    pub peak_read: f64,
    pub peak_write: f64,
}

#[derive(Clone, Default)]
pub struct AudioApp {
    pub pid: i32,
    pub name: String,
    pub icon_path: Option<String>,
    pub running_output: bool,
    pub running_input: bool,
    pub volume: Option<f32>,
    pub muted: bool,
    pub level: f32,
}

#[derive(Clone, Default)]
pub struct AudioInfo {
    pub device_name: String,
    pub device_volume: f32,
    pub device_muted: bool,
    pub devices: Vec<String>,
    pub apps: Vec<AudioApp>,
}

#[derive(Clone, Default)]
pub struct BtDevice {
    pub name: String,
    pub kind: String,
    pub battery: Option<f64>,
    pub second_battery: Option<f64>,
    pub case_battery: Option<f64>,
    pub connected: bool,
    pub charging: bool,
}

#[derive(Clone, Default)]
pub struct BtInfo {
    pub enabled: bool,
    pub devices: Vec<BtDevice>,
}

#[derive(Clone, Default)]
pub struct ProjectGroup {
    pub name: String,
    pub dir: String,
    pub servers: Vec<DevServer>,
    pub idle: bool,
}
#[derive(Clone, Default)]
pub struct DevServer {
    pub pid: i32,
    pub name: String,
    pub runtime: String,
    pub port: u16,
    pub status: String,
    pub mem_bytes: u64,
    pub cpu_pct: f64,
    pub up_since: f64,
}

#[derive(Clone, Default)]
pub struct Alert {
    pub metric: String,
    pub message: String,
    pub at: f64,
}

#[derive(Clone)]
pub struct Snapshot {
    pub at: f64,
    pub cpu_pct: f64,
    pub cpu_hist: Hist,
    pub load_avg: [f64; 3],
    pub mem: MemStats,
    pub mem_hist: Hist,
    pub swap_hist: Hist,
    pub disk: DiskSample,
    pub disk_r_hist: Hist,
    pub disk_w_hist: Hist,
    pub net: NetSample,
    pub net_in_hist: Hist,
    pub net_out_hist: Hist,
    pub gpu: GpuStats,
    pub gpu_hist: Hist,
    pub battery: BatteryStats,
    pub batt_hist: Hist,
    pub audio: AudioInfo,
    pub bt: BtInfo,
    pub projects: Vec<ProjectGroup>,
    pub apps: Vec<AppGroup>,
    pub proc_count: usize,
    pub app_count: usize,
    pub chip: ChipInfo,
    pub power_w: f64,
    pub power_hist: Hist,
    pub thermal: String,
    pub neural: Option<f64>,
    pub neural_hist: Hist,
    pub uptime: Duration,
    pub alerts: Vec<Alert>,
}

impl Default for Snapshot {
    fn default() -> Self {
        let h = || Hist { pts: Vec::new(), max_len: 360 };
        Snapshot {
            at: now_unix(),
            cpu_pct: 0.0,
            cpu_hist: h(),
            load_avg: [0.0; 3],
            mem: MemStats::default(),
            mem_hist: h(),
            swap_hist: h(),
            disk: DiskSample::default(),
            disk_r_hist: h(),
            disk_w_hist: h(),
            net: NetSample::default(),
            net_in_hist: h(),
            net_out_hist: h(),
            gpu: GpuStats::default(),
            gpu_hist: h(),
            battery: BatteryStats::default(),
            batt_hist: h(),
            audio: AudioInfo::default(),
            bt: BtInfo::default(),
            projects: Vec::new(),
            apps: Vec::new(),
            proc_count: 0,
            app_count: 0,
            chip: ChipInfo::default(),
            power_w: 0.0,
            power_hist: h(),
            thermal: "Nominal".into(),
            neural: None,
            neural_hist: h(),
            uptime: Duration::ZERO,
            alerts: Vec::new(),
        }
    }
}

// ---------- engine internals ----------

struct PrevProc {
    cpu_ns: u64,
    disk_read: u64,
    disk_write: u64,
    energy_nj: u64,
    start_unix: f64,
}

pub struct Engine {
    pub snap: Snapshot,
    pub started: Instant,
    last_at: Instant,
    last_ticks: Option<CpuTicks>,
    last_net: (u64, u64),
    last_disk: (u64, u64),
    prev: HashMap<i32, PrevProc>,
    tick: u64,
}

pub static ENGINE: OnceLock<Arc<Mutex<Engine>>> = OnceLock::new();
pub fn engine() -> Arc<Mutex<Engine>> {
    ENGINE
        .get_or_init(|| Arc::new(Mutex::new(Engine::new())))
        .clone()
}

impl Engine {
    pub fn new() -> Self {
        let mut e = Engine {
            snap: Snapshot::default(),
            started: Instant::now(),
            last_at: Instant::now(),
            last_ticks: cpu_ticks(),
            last_net: net_counters(),
            last_disk: disk_io_totals(),
            prev: HashMap::new(),
            tick: 0,
        };
        e.snap.chip = chip_info();
        e.snap.disk = DiskSample {
            used: disk_stats().used,
            total: disk_stats().total,
            free: disk_stats().free,
            volume_count: disk_stats().volume_count,
            ..Default::default()
        };
        e
    }

    /// One sampling pass. dt is measured so rates are exact.
    pub fn sample(&mut self, audio_apps: Vec<AudioApp>, bt: BtInfo, projects: Vec<ProjectGroup>) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_at).as_secs_f64().max(0.1);
        self.last_at = now;
        self.tick += 1;
        let s = &mut self.snap;
        s.at = now_unix();

        // CPU %
        let ticks = cpu_ticks();
        if let (Some(a), Some(b)) = (self.last_ticks, ticks) {
            let du = (b.user - a.user) as f64 + (b.nice - a.nice) as f64;
            let ds = (b.system - a.system) as f64;
            let di = (b.idle - a.idle) as f64;
            let tot = (du + ds + di).max(1.0);
            s.cpu_pct = ((du + ds) / tot * 100.0).clamp(0.0, 100.0);
        }
        self.last_ticks = ticks;
        s.cpu_hist.push(s.cpu_pct);
        s.load_avg = load_avg();

        // Memory
        s.mem = mem_stats();
        let mem_used_pct = s.mem.used as f64 / s.mem.total as f64 * 100.0;
        s.mem_hist.push(mem_used_pct);
        let swp = if s.mem.swap_total > 0 {
            s.mem.swap_used as f64 / s.mem.swap_total as f64 * 100.0
        } else { 0.0 };
        s.swap_hist.push(swp);

        // Disk
        let (r, w) = disk_io_totals();
        s.disk.read_rate = (r.saturating_sub(self.last_disk.0)) as f64 / dt;
        s.disk.write_rate = (w.saturating_sub(self.last_disk.1)) as f64 / dt;
        self.last_disk = (r, w);
        let ds = disk_stats();
        s.disk.used = ds.used;
        s.disk.total = ds.total;
        s.disk.free = ds.free;
        s.disk.volume_count = ds.volume_count;
        s.disk_r_hist.push(s.disk.read_rate);
        s.disk_w_hist.push(s.disk.write_rate);
        s.disk.peak_read = s.disk.peak_read.max(s.disk.read_rate);
        s.disk.peak_write = s.disk.peak_write.max(s.disk.write_rate);

        // Network
        let (rx, tx) = net_counters();
        s.net.rate_in = (rx.saturating_sub(self.last_net.0)) as f64 / dt;
        s.net.rate_out = (tx.saturating_sub(self.last_net.1)) as f64 / dt;
        s.net.active = s.net.rate_in > 4096.0 || s.net.rate_out > 4096.0;
        s.net.total_in = rx;
        s.net.total_out = tx;
        self.last_net = (rx, tx);
        s.net.primary = default_interface();
        s.net.kind = "Wi-Fi".into();
        s.net_in_hist.push(s.net.rate_in);
        s.net_out_hist.push(s.net.rate_out);
        s.net.peak_in = s.net.peak_in.max(s.net.rate_in);
        s.net.peak_out = s.net.peak_out.max(s.net.rate_out);

        // GPU
        s.gpu = gpu_stats();
        s.gpu_hist.push(s.gpu.utilization.unwrap_or(0.0));

        // Battery + system power
        s.battery = battery_stats();
        if let Some(p) = s.battery.percent {
            s.batt_hist.push(p);
        }

        // Processes
        let pids = list_pids();
        let mut procs = Vec::with_capacity(pids.len());
        for pid in pids {
            let Some(b) = proc_bsd(pid) else { continue };
            let Some(r) = proc_rusage(pid) else { continue };
            let start = b.pbi_start_tvsec as f64 + b.pbi_start_tvusec as f64 / 1e6;
            let cpu_ns = r.ri_user_time + r.ri_system_time;
            let prev = self.prev.get(&pid);
            let (cpu_pct, rr, wr, pw) = if let Some(pv) = prev {
                let alive = if pv.start_unix == start { dt } else { 0.0 };
                if alive > 0.0 {
                    (
                        (cpu_ns.saturating_sub(pv.cpu_ns)) as f64 / dt / 1e9 * 100.0,
                        (r.ri_diskio_bytesread.saturating_sub(pv.disk_read)) as f64 / dt,
                        (r.ri_diskio_byteswritten.saturating_sub(pv.disk_write)) as f64 / dt,
                        (r.ri_billed_energy.saturating_sub(pv.energy_nj)) as f64 / dt / 1e9,
                    )
                } else {
                    (0.0, 0.0, 0.0, 0.0)
                }
            } else {
                // first sight: estimate from lifetime
                let life = (now_unix() - start).max(1.0);
                (cpu_ns as f64 / life / 1e9 * 100.0, r.ri_diskio_bytesread as f64 / life, r.ri_diskio_byteswritten as f64 / life, r.ri_billed_energy as f64 / life / 1e9)
            };
            let exe = proc_path(pid);
            procs.push(ProcInfo {
                pid,
                ppid: b.pbi_ppid as i32,
                uid: b.pbi_uid,
                comm: unsafe {
                    std::ffi::CStr::from_ptr(b.pbi_comm.as_ptr())
                        .to_string_lossy()
                        .to_string()
                },
                exe_path: exe,
                start_unix: start,
                cpu_ns,
                mem_bytes: r.ri_phys_footprint,
                disk_read: r.ri_diskio_bytesread,
                disk_write: r.ri_diskio_byteswritten,
                energy_nj: r.ri_billed_energy,
                cpu_pct: cpu_pct.clamp(0.0, 3200.0),
                read_rate: rr.max(0.0),
                write_rate: wr.max(0.0),
                power_w: pw.max(0.0),
            });
            self.prev.insert(
                pid,
                PrevProc {
                    cpu_ns,
                    disk_read: r.ri_diskio_bytesread,
                    disk_write: r.ri_diskio_byteswritten,
                    energy_nj: r.ri_billed_energy,
                    start_unix: start,
                },
            );
        }
        // prune dead pids
        let live: std::collections::HashSet<i32> = procs.iter().map(|p| p.pid).collect();
        self.prev.retain(|p, _| live.contains(p));

        s.proc_count = procs.len();
        s.apps = group_procs(&procs);
        s.app_count = s.apps.len();

        // attach per-app network via nettop (every other tick to save CPU)
        if self.tick % 2 == 0 {
            crate::projects::apply_nettop(&mut s.apps);
        }

        // system power: sum of billed app power + baseline, or battery draw
        let app_power: f64 = s.apps.iter().map(|a| a.power_w).sum();
        s.power_w = if let Some(w) = s.battery.power_w {
            w.max(app_power)
        } else {
            app_power.max(s.cpu_pct / 100.0 * 8.0) // VM estimate floor
        };
        s.power_hist.push(s.power_w);

        s.audio = AudioInfo { apps: audio_apps, ..s.audio.clone() };
        s.audio.device_name = crate::audio::default_output_name();
        s.audio.device_volume = crate::audio::default_output_volume().unwrap_or(0.0);
        s.audio.device_muted = crate::audio::default_output_muted().unwrap_or(false);
        if self.tick % 10 == 0 {
            s.audio.devices = crate::audio::output_device_names();
        }
        s.bt = bt;
        s.projects = projects;
        s.uptime = self.started.elapsed();

        s.neural = None; // no public ANE counter; keep chart at 0 when unsupported
        s.neural_hist.push(s.neural.unwrap_or(0.0));
    }
}

pub fn latest() -> Snapshot {
    engine().lock().unwrap().snap.clone()
}

pub fn fmt_bytes(n: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1.0 { return "0 B".into(); }
    let mut v = n;
    let mut i = 0;
    while v >= 1024.0 && i < 4 { v /= 1024.0; i += 1; }
    if i == 0 { format!("{} B", v as u64) }
    else if v >= 100.0 { format!("{:.0} {}", v, U[i]) }
    else if v >= 10.0 { format!("{:.1} {}", v, U[i]) }
    else { format!("{:.2} {}", v, U[i]) }
}
pub fn fmt_rate(n: f64) -> String {
    format!("{}/s", fmt_bytes(n))
}
pub fn fmt_watts(w: f64) -> String {
    if w < 0.05 { "0.00 W".into() } else if w < 10.0 { format!("{:.2} W", w) } else { format!("{:.1} W", w) }
}
pub fn fmt_pct(v: f64) -> String {
    format!("{:.0}%", v)
}
pub fn fmt_up(secs: f64) -> String {
    let s = secs as u64;
    if s < 90 { format!("{} seconds", s.max(1)) }
    else if s < 5400 { format!("{} minutes", s / 60) }
    else if s < 129600 { format!("{} hours", s / 3600) }
    else { format!("{} days", s / 86400) }
}
