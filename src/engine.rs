//! The sampling engine: runs on a background thread every ~2s, diffs counters,
//! produces a SystemSnapshot consumed by the UI, status bar, history & alerts.

#![allow(dead_code)]

use crate::locale::text as l;
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
    pub kind: crate::network::LinkKind,
    /// Whether macOS reports a usable network path; `None` before the first
    /// sample.
    pub online: Option<bool>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub wifi: Option<crate::network::Wifi>,
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
    /// The Core Audio process object, which per-app volume taps.
    pub object: u32,
    pub name: String,
    pub icon_path: Option<String>,
    pub running_output: bool,
    pub running_input: bool,
}

#[derive(Clone, Default)]
pub struct AudioInfo {
    pub output_id: u32,
    pub device_name: String,
    pub device_volume: Option<f32>,
    pub device_muted: Option<bool>,
    pub can_volume: bool,
    pub can_mute: bool,
    /// Every output device, by id and name, for choosing the default.
    pub outputs: Vec<(u32, String)>,
    /// The default input device (microphone) and every input device.
    pub input: crate::audio::OutputState,
    pub inputs: Vec<(u32, String)>,
    pub apps: Vec<AudioApp>,
    /// Volumes Tidegauge holds per audio process object.
    pub levels: std::collections::HashMap<u32, crate::app_volume::Level>,
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
#[derive(Clone, Debug, PartialEq)]
pub enum ServerState {
    Working,
    Idle { seconds: u64 },
    BarelyUsed { uptime_seconds: u64 },
}

impl Default for ServerState {
    fn default() -> Self {
        Self::Idle { seconds: 0 }
    }
}

impl ServerState {
    pub fn is_idle(&self) -> bool {
        !matches!(self, Self::Working)
    }
    pub fn label(&self) -> String {
        self.label_with(crate::locale::text, fmt_up)
    }
    pub fn label_with(
        &self,
        l: impl Fn(&'static str, &'static str) -> &'static str,
        up: impl Fn(f64) -> String,
    ) -> String {
        match self {
            Self::Working => l("working", "arbeider").into(),
            Self::Idle { seconds } => {
                format!("{} {}", l("idle", "inaktiv i"), up(*seconds as f64))
            }
            Self::BarelyUsed { uptime_seconds } => format!(
                "{} {}, {}",
                l("up", "oppe i"),
                up(*uptime_seconds as f64),
                l("barely used", "nesten ikke brukt")
            ),
        }
    }
}

#[derive(Clone, Default)]
pub struct DevServer {
    pub pid: i32,
    pub name: String,
    pub exe_path: String,
    pub cwd: String,
    pub runtime: String,
    pub port: u16,
    pub state: ServerState,
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
    /// Every sampled process, for per-process rows and quit actions.
    pub procs: Vec<crate::processes::ProcInfo>,
    pub agents: Vec<crate::agents::Session>,
    pub plans: crate::agents::Plans,
    pub process_groups: Vec<crate::process_activity::ProcessGroup>,
    pub resource_activity: Vec<crate::resource_activity::Activity>,
    pub proc_count: usize,
    pub app_count: usize,
    pub chip: ChipInfo,
    /// Each logical CPU's busy share since the last sample, 0–100 %.
    pub core_pct: Vec<f64>,
    /// CPU (per cluster type) and GPU clock speeds, in MHz.
    pub clocks: crate::clocks::Clocks,
    pub power_w: Option<f64>,
    pub power_hist: Hist,
    /// The chip's hottest named sensor over time, °C.
    pub temp_hist: Hist,
    /// Each fan's speed (RPM) and share of its top speed (%), and the
    /// fastest share over time.
    pub fans: Vec<(f64, Option<f64>)>,
    pub fan_hist: Hist,
    pub thermal: String,
    /// The same state as a level, 0 nominal to 3 critical, for alerts.
    pub thermal_level: Option<u8>,
    /// Chip, SSD and battery temperatures from the sensors macOS names;
    /// empty while the sensor reading is stale.
    pub temps: crate::named_sensors::Temps,
    pub uptime: Duration,
    /// When the Mac last started, in Unix seconds, where macOS reports it.
    pub boot_unix: Option<f64>,
    /// Low Power Mode is on.
    pub low_power: bool,
    pub alerts: Vec<Alert>,
    /// Apps whose memory has climbed steadily for hours.
    pub leaks: Vec<crate::leaks::Leak>,
}

impl Default for Snapshot {
    fn default() -> Self {
        let h = || Hist {
            pts: Vec::new(),
            max_len: 360,
        };
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
            procs: Vec::new(),
            agents: Vec::new(),
            plans: crate::agents::Plans::Missing,
            process_groups: Vec::new(),
            resource_activity: Vec::new(),
            proc_count: 0,
            app_count: 0,
            chip: ChipInfo::default(),
            core_pct: Vec::new(),
            clocks: Default::default(),
            power_w: None,
            power_hist: h(),
            temp_hist: h(),
            fans: Vec::new(),
            fan_hist: h(),
            thermal: "—".into(),
            thermal_level: None,
            temps: Default::default(),
            uptime: Duration::ZERO,
            boot_unix: None,
            low_power: false,
            alerts: Vec::new(),
            leaks: Vec::new(),
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
    /// Name and path read when this process was first seen; they are read
    /// again only if the process starts again or changes its name (exec).
    comm: String,
    exe_path: String,
}

/// The name and path already known for a process that is still the same one:
/// same start time and same name.
fn known_identity<'a>(prev: Option<&'a PrevProc>, start: f64, comm: &str) -> Option<&'a str> {
    prev.filter(|p| p.start_unix == start && p.comm == comm)
        .map(|p| p.exe_path.as_str())
}

pub struct Engine {
    pub snap: Snapshot,
    pub started: Instant,
    last_at: Instant,
    last_ticks: Option<CpuTicks>,
    last_core_ticks: Option<Vec<CpuTicks>>,
    last_net: (u64, u64),
    last_disk: (u64, u64),
    prev: HashMap<i32, PrevProc>,
    process_tracker: crate::process_activity::Tracker,
    resource_tracker: crate::resource_activity::Tracker,
    leaks: crate::leaks::Tracker,
    tick: u64,
    /// How many audio passes have run; the device lists are read every tenth.
    audio_passes: u64,
    /// When the IOReport readings were last taken.
    last_rates: Option<Instant>,
    /// When the process pass last ran; its rates are per this interval.
    last_heavy: Instant,
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
            last_core_ticks: crate::metrics::core_ticks(),
            last_net: net_counters(),
            last_disk: disk_io_totals(),
            prev: HashMap::new(),
            process_tracker: crate::process_activity::Tracker::default(),
            resource_tracker: crate::resource_activity::Tracker::default(),
            leaks: crate::leaks::Tracker::default(),
            tick: 0,
            audio_passes: 0,
            last_rates: None,
            last_heavy: Instant::now(),
        };
        e.snap.chip = chip_info();
        e.snap.boot_unix = boot_unix();
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
    pub fn sample(&mut self, audio_apps: Option<Vec<AudioApp>>, bt: BtInfo) {
        let now = Instant::now();
        let demand = crate::demand::current();
        // Clock speeds and GPU power come from IOReport, which is costly; with
        // nothing on screen they are read every few seconds.
        let rates_gap = self.last_rates.map(|at| now.duration_since(at));
        let read_rates = rates_gap.is_none_or(|gap| demand.rates_due(gap));
        let rates_current = rates_gap.is_none_or(crate::demand::Demand::rates_current);
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
        let cores = crate::metrics::core_ticks();
        s.core_pct = match (&self.last_core_ticks, &cores) {
            (Some(a), Some(b)) if a.len() == b.len() => a.iter().zip(b).map(|(a, b)| crate::metrics::busy_pct(*a, *b)).collect(),
            _ => Vec::new(),
        };
        self.last_core_ticks = cores;
        if read_rates {
            let clocks = crate::clocks::sample();
            if rates_current {
                s.clocks = clocks;
            }
        }
        s.cpu_hist.push(s.cpu_pct);
        s.load_avg = load_avg();
        s.thermal = thermal_state().unwrap_or("—").into();
        s.thermal_level = crate::metrics::thermal_level();
        s.low_power = crate::metrics::low_power_mode();
        let sensors = crate::sensors::latest();
        s.temps = if sensors.is_fresh() { crate::named_sensors::temps(&sensors.named) } else { Default::default() };
        match s.temps.chip {
            Some(c) => s.temp_hist.push(c),
            None => s.temp_hist.pts.clear(),
        }
        s.fans = if sensors.is_fresh() {
            sensors
                .fans
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    let rpm = r.value?;
                    let max = sensors.fan_limits.get(i).and_then(|l| l.1);
                    Some((rpm, crate::sensors::fan_share(rpm, max)))
                })
                .collect()
        } else {
            Vec::new()
        };
        match s.fans.iter().filter_map(|f| f.1).reduce(f64::max) {
            Some(share) => s.fan_hist.push(share),
            None => s.fan_hist.pts.clear(),
        }

        // Memory
        s.mem = mem_stats();
        let mem_used_pct = s.mem.used as f64 / s.mem.total as f64 * 100.0;
        s.mem_hist.push(mem_used_pct);
        let swp = if s.mem.swap_total > 0 {
            s.mem.swap_used as f64 / s.mem.swap_total as f64 * 100.0
        } else {
            0.0
        };
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
        let link = crate::network::details();
        s.net.primary = link.interface;
        s.net.kind = link.kind;
        s.net.online = Some(link.online);
        s.net.ipv4 = link.ipv4;
        s.net.ipv6 = link.ipv6;
        s.net.wifi = link.wifi;
        s.net_in_hist.push(s.net.rate_in);
        s.net_out_hist.push(s.net.rate_out);
        s.net.peak_in = s.net.peak_in.max(s.net.rate_in);
        s.net.peak_out = s.net.peak_out.max(s.net.rate_out);

        // GPU
        let last_gpu_power = s.gpu.power_w;
        s.gpu = gpu_stats();
        s.gpu.power_w = if read_rates {
            let power = crate::gpu_power::sample();
            if rates_current {
                power
            } else {
                last_gpu_power
            }
        } else {
            last_gpu_power
        };
        if read_rates {
            self.last_rates = Some(now);
        }
        if let Some(utilization) = s.gpu.utilization {
            s.gpu_hist.push(utilization);
        }

        // Battery + system power
        s.battery = battery_stats();
        if let Some(p) = s.battery.percent {
            s.batt_hist.push(p);
        } else {
            s.batt_hist.pts.clear();
        }

        // Processes. Hidden, this pass runs every few seconds; the rest of
        // the sample is light and runs every tick.
        let since_heavy = now.duration_since(self.last_heavy);
        let heavy = self.tick == 1 || demand.heavy_due(since_heavy);
        if heavy {
            let pdt = since_heavy.as_secs_f64().max(0.1);
            self.last_heavy = now;
            let pids = list_pids();
            let enumerated = pids.iter().copied().collect();
            let mut procs = Vec::with_capacity(pids.len());
            for pid in pids {
                let Some(b) = proc_bsd(pid) else { continue };
                let Some(r) = proc_rusage(pid) else { continue };
                let start = b.pbi_start_tvsec as f64 + b.pbi_start_tvusec as f64 / 1e6;
                let cpu_ns = mach_ticks_to_ns(r.ri_user_time.saturating_add(r.ri_system_time));
                let comm = unsafe {
                    std::ffi::CStr::from_ptr(b.pbi_comm.as_ptr())
                        .to_string_lossy()
                        .to_string()
                };
                let prev = self.prev.get(&pid);
                let (cpu_pct, rr, wr, pw) = if let Some(pv) = prev {
                    let alive = if pv.start_unix == start { pdt } else { 0.0 };
                    if alive > 0.0 {
                        (
                            (cpu_ns.saturating_sub(pv.cpu_ns)) as f64 / pdt / 1e9 * 100.0,
                            (r.ri_diskio_bytesread.saturating_sub(pv.disk_read)) as f64 / pdt,
                            (r.ri_diskio_byteswritten.saturating_sub(pv.disk_write)) as f64 / pdt,
                            (r.ri_energy_nj.saturating_sub(pv.energy_nj)) as f64 / pdt / 1e9,
                        )
                    } else {
                        (0.0, 0.0, 0.0, 0.0)
                    }
                } else {
                    // first sight: estimate from lifetime
                    let life = (now_unix() - start).max(1.0);
                    (
                        cpu_ns as f64 / life / 1e9 * 100.0,
                        r.ri_diskio_bytesread as f64 / life,
                        r.ri_diskio_byteswritten as f64 / life,
                        r.ri_energy_nj as f64 / life / 1e9,
                    )
                };
                let exe = known_identity(prev, start, &comm)
                    .map(str::to_string)
                    .unwrap_or_else(|| proc_path(pid));
                self.prev.insert(
                    pid,
                    PrevProc {
                        cpu_ns,
                        disk_read: r.ri_diskio_bytesread,
                        disk_write: r.ri_diskio_byteswritten,
                        energy_nj: r.ri_energy_nj,
                        start_unix: start,
                        comm: comm.clone(),
                        exe_path: exe.clone(),
                    },
                );
                procs.push(ProcInfo {
                    pid,
                    ppid: b.pbi_ppid as i32,
                    uid: b.pbi_uid,
                    comm,
                    exe_path: exe,
                    start_unix: start,
                    cpu_ns,
                    mem_bytes: r.ri_phys_footprint,
                    disk_read: r.ri_diskio_bytesread,
                    disk_write: r.ri_diskio_byteswritten,
                    energy_nj: r.ri_energy_nj,
                    interrupt_wakeups: Some(r.ri_interrupt_wkups),
                    cpu_pct: cpu_pct.clamp(0.0, 3200.0),
                    read_rate: rr.max(0.0),
                    write_rate: wr.max(0.0),
                    power_w: pw.max(0.0),
                });
            }
            // prune dead pids
            let live: std::collections::HashSet<i32> = procs.iter().map(|p| p.pid).collect();
            self.prev.retain(|p, _| live.contains(p));

            s.proc_count = procs.len();
            s.process_groups = self.process_tracker.sample(&procs, &enumerated);
            s.resource_activity = self.resource_tracker.sample(&procs, self.started.elapsed().as_secs_f64(), now_unix());
            s.apps = regroup_apps(&procs, &s.apps);
            s.app_count = s.apps.len();
            self.leaks.update(s.at, &s.apps);
            s.leaks = self.leaks.flagged().to_vec();
            // Per-app network comes from the nettop worker's latest reading.
            crate::projects::apply_net_rates(&mut s.apps);
            s.procs = procs;
        }

        // System power is the power controller's own measurement; battery
        // terminal power and per-app energy are not the whole Mac.
        s.power_w = s.battery.system_power_w;
        match s.power_w {
            Some(w) => s.power_hist.push(w),
            None => s.power_hist.pts.clear(),
        }

        // Taps follow their apps and the default output. Hidden, the audio
        // pass is skipped most ticks and the last answer stands.
        if let Some(audio_apps) = audio_apps {
            self.audio_passes += 1;
            crate::app_volume::follow(&audio_apps.iter().map(|a| a.object).collect::<Vec<_>>());
            s.audio = AudioInfo {
                apps: audio_apps,
                levels: crate::app_volume::levels(),
                ..s.audio.clone()
            };
            let output = crate::audio::output_state();
            s.audio.output_id = output.id;
            s.audio.device_name = output.name;
            s.audio.device_volume = output.volume;
            s.audio.device_muted = output.muted;
            s.audio.can_volume = output.can_volume;
            s.audio.can_mute = output.can_mute;
            s.audio.input = crate::audio::device_state(crate::audio::Direction::Input);
            if self.audio_passes.is_multiple_of(10) {
                s.audio.outputs = crate::audio::devices(crate::audio::Direction::Output);
                s.audio.inputs = crate::audio::devices(crate::audio::Direction::Input);
            }
        }
        s.bt = bt;
        if heavy {
            s.projects = crate::projects::detect_projects(&s.procs, demand.listeners_period());
            s.agents = crate::agents::sessions(&s.procs, &s.projects);
            s.plans = crate::agents::plans();
        }
        s.uptime = self.started.elapsed();
    }
}

// UI readers share only the last completed sample, never the collection lock.
// They clone the pointer, not the sample.
static PUBLISHED: OnceLock<Mutex<Arc<Snapshot>>> = OnceLock::new();

fn published() -> &'static Mutex<Arc<Snapshot>> {
    PUBLISHED.get_or_init(|| Mutex::new(Arc::new(Snapshot::default())))
}

pub fn publish(snap: &Snapshot) {
    let snapshot = Arc::new(snap.clone());
    *published().lock().unwrap() = snapshot;
}

pub fn latest() -> Arc<Snapshot> {
    published().lock().unwrap().clone()
}

pub fn fmt_bytes(n: f64) -> String {
    fmt_scaled(n, 1024.0)
}
/// Disk capacity and free space, in the decimal units Finder uses
/// (1 GB = 1000³ bytes). Memory stays binary through `fmt_bytes`.
pub fn fmt_disk_bytes(n: f64) -> String {
    fmt_scaled(n, 1000.0)
}
fn fmt_scaled(n: f64, base: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1.0 {
        return "0 B".into();
    }
    let mut v = n;
    let mut i = 0;
    // Divide by the base but move up a unit once the shown value would reach
    // four digits ("1007 B", "1010 KB" become "0.98 KB", "0.99 MB").
    while v >= if i == 0 { 1000.0 } else { 999.5 } && i < 4 {
        v /= base;
        i += 1;
    }
    if i == 0 {
        format!("{} B", v as u64)
    } else if v >= 100.0 {
        format!("{} {}", crate::locale::number(v, 0), U[i])
    } else if v >= 10.0 {
        format!("{} {}", crate::locale::number(v, 1), U[i])
    } else {
        format!("{} {}", crate::locale::number(v, 2), U[i])
    }
}
pub fn fmt_rate(n: f64) -> String {
    format!("{}/s", fmt_bytes(n))
}
pub fn fmt_watts(w: f64) -> String {
    if w < 0.05 {
        format!("{} W", crate::locale::number(0.0, 2))
    } else if w < 10.0 {
        format!("{} W", crate::locale::number(w, 2))
    } else {
        format!("{} W", crate::locale::number(w, 1))
    }
}
/// Signed power with one decimal, for flows that can reverse ("+1.5 W").
pub fn fmt_signed_watts(w: f64) -> String {
    crate::locale::decimal(format!("{w:+.1} W"))
}
pub fn fmt_volts(v: f64) -> String {
    format!("{} V", crate::locale::number(v, 2))
}
/// "2.53 GHz" from 1 GHz up, "338 MHz" below.
pub fn fmt_clock(mhz: f64) -> String {
    if mhz >= 1000. {
        format!("{} GHz", crate::locale::number(mhz / 1000., 2))
    } else {
        format!("{} MHz", crate::locale::number(mhz, 0))
    }
}

pub fn fmt_celsius(c: f64) -> String {
    format!("{} °C", crate::locale::number(c, 1))
}
/// "2 h 5 min" / "2 t 5 min" (Norwegian hours are "timer", abbreviated "t").
pub fn fmt_hours_minutes(minutes: i64) -> String {
    format!("{} {} {} min", minutes / 60, l("h", "t"), minutes % 60)
}
pub fn fmt_pct(v: f64) -> String {
    crate::locale::percent(v, 0)
}
pub fn fmt_up(secs: f64) -> String {
    let s = secs as u64;
    if s < 90 {
        crate::locale::count(s.max(1), "second", "seconds", "sekund", "sekunder")
    } else if s < 5400 {
        crate::locale::count(s / 60, "minute", "minutes", "minutt", "minutter")
    } else if s < 129600 {
        crate::locale::count(s / 3600, "hour", "hours", "time", "timer")
    } else {
        crate::locale::count(s / 86400, "day", "days", "døgn", "dager")
    }
}

fn regroup_apps(procs: &[ProcInfo], previous: &[AppGroup]) -> Vec<AppGroup> {
    let previous: HashMap<&str, &AppGroup> =
        previous.iter().map(|app| (app.key.as_str(), app)).collect();
    let mut apps = group_procs(procs);
    for app in &mut apps {
        if let Some(old) = previous.get(app.key.as_str()) {
            // Do not transfer rates to a different process group with the same name.
            if app.pids.len() == old.pids.len() && app.pids.iter().all(|pid| old.pids.contains(pid))
            {
                app.net_in_rate = old.net_in_rate;
                app.net_out_rate = old.net_out_rate;
            }
        }
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_up_uses_singular_and_plural_localized_units() {
        for (secs, en, no) in [
            (0., "second", "sekund"),
            (1., "second", "sekund"),
            (89., "seconds", "sekunder"),
            (90., "minute", "minutt"),
            (119., "minute", "minutt"),
            (120., "minutes", "minutter"),
            (5399., "minutes", "minutter"),
            (5400., "hour", "time"),
            (7199., "hour", "time"),
            (7200., "hours", "timer"),
            (129599., "hours", "timer"),
            (129600., "day", "døgn"),
            (172799., "day", "døgn"),
            (172800., "days", "dager"),
        ] {
            let n = secs as u64;
            let value = if n < 90 {
                n.max(1)
            } else if n < 5400 {
                n / 60
            } else if n < 129600 {
                n / 3600
            } else {
                n / 86400
            };
            assert_eq!(fmt_up(secs), format!("{} {}", value, l(en, no)), "secs={secs}");
        }
    }

    #[test]
    fn byte_values_never_show_four_digits() {
        let point = if crate::locale::is_norwegian() { "," } else { "." };
        assert_eq!(fmt_bytes(999.0), "999 B");
        assert_eq!(fmt_bytes(1007.0), format!("0{point}98 KB"));
        assert_eq!(fmt_rate(1007.0), format!("0{point}98 KB/s"));
        assert_eq!(fmt_bytes(1010.0 * 1024.), format!("0{point}99 MB"));
        assert_eq!(fmt_bytes(999.6 * 1024.), format!("0{point}98 MB"));
        assert_eq!(fmt_bytes(1024.0), format!("1{point}00 KB"));
    }

    #[test]
    fn displayed_numbers_follow_the_language() {
        let nb = crate::locale::is_norwegian();
        let (point, pct, hours) = if nb { (',', " %", "t") } else { ('.', "%", "h") };
        assert_eq!(fmt_bytes(6.74 * 1024. * 1024. * 1024.), format!("6{point}74 GB"));
        assert_eq!(fmt_bytes(512.0 * 1024.), "512 KB");
        // Disks use decimal units like Finder; memory stays binary.
        assert_eq!(fmt_disk_bytes(500e9), "500 GB");
        assert_eq!(fmt_disk_bytes(994.66e9), "995 GB");
        assert_eq!(fmt_disk_bytes(1e12), format!("1{point}00 TB"));
        assert_eq!(fmt_bytes(500e9), format!("466 GB"));
        assert_eq!(fmt_rate(1.5 * 1024. * 1024.), format!("1{point}50 MB/s"));
        assert_eq!(fmt_watts(12.34), format!("12{point}3 W"));
        assert_eq!(fmt_signed_watts(-1.25), format!("-1{point}2 W"));
        assert_eq!(fmt_celsius(32.5), format!("32{point}5 °C"));
        assert_eq!(fmt_volts(12.6), format!("12{point}60 V"));
        assert_eq!(fmt_pct(50.0), format!("50{pct}"));
        assert_eq!(fmt_hours_minutes(125), format!("2 {hours} 5 min"));
    }

    #[test]
    fn regrouping_does_not_transfer_rates_to_replacement_processes() {
        let mut proc = ProcInfo {
            pid: 42,
            comm: "server".into(),
            exe_path: "/usr/bin/server".into(),
            ..Default::default()
        };
        let mut previous = group_procs(&[proc.clone()]);
        previous[0].net_in_rate = 4096.0;
        proc.pid = 43;
        assert_eq!(regroup_apps(&[proc], &previous)[0].net_in_rate, 0.0);
    }

    #[test]
    #[ignore = "starts a disposable Python listener and samples native process counters"]
    fn native_projects_use_measured_cpu_memory_and_full_directory() {
        use std::io::{BufRead, BufReader};
        use std::process::{Child, Command, Stdio};
        struct TestServer(Child);
        impl Drop for TestServer {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut server = TestServer(Command::new("python3")
            .args(["-u", "-c", "import socket\ns=socket.socket()\ns.bind(('127.0.0.1',0))\ns.listen()\nmemory=bytearray(8*1024*1024)\nprint(s.getsockname()[1],flush=True)\nwhile True: sum(range(50000))"])
            .stdout(Stdio::piped())
            .spawn().unwrap());
        let mut port = String::new();
        BufReader::new(server.0.stdout.take().unwrap())
            .read_line(&mut port)
            .unwrap();
        let port: u16 = port.trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(700));
        let mut sampler = Engine::new();
        sampler.sample(Some(Vec::new()), BtInfo::default());
        let (project, process) = sampler
            .snap
            .projects
            .iter()
            .flat_map(|project| project.servers.iter().map(move |server| (project, server)))
            .find(|(_, process)| process.pid == server.0.id() as i32 && process.port == port)
            .expect("disposable listener missing from project detection");
        assert!(
            process.cpu_pct > 15.0,
            "busy server CPU was not measured: {}%",
            process.cpu_pct
        );
        assert!(process.mem_bytes > 0, "server memory was not measured");
        assert_eq!(process.state, ServerState::Working);
        assert!(!project.idle);
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(std::path::Path::new(&project.dir), cwd.as_path());
        let app = sampler
            .snap
            .apps
            .iter()
            .find(|app| app.pids.contains(&process.pid))
            .unwrap();
        assert!(app.cpu_pct >= process.cpu_pct);
        assert!(app.mem_bytes >= process.mem_bytes);
    }

    #[test]
    fn regrouping_preserves_network_rates_on_skipped_collection_passes() {
        let procs = vec![ProcInfo {
            pid: 42,
            comm: "server".into(),
            exe_path: "/usr/bin/server".into(),
            ..Default::default()
        }];
        let mut previous = group_procs(&procs);
        previous[0].net_in_rate = 4096.0;
        previous[0].net_out_rate = 1024.0;
        let fresh = regroup_apps(&procs, &previous);
        assert_eq!(fresh[0].net_in_rate, 4096.0);
        assert_eq!(fresh[0].net_out_rate, 1024.0);
    }

    fn prev_proc(start: f64, comm: &str, exe: &str) -> PrevProc {
        PrevProc {
            cpu_ns: 0,
            disk_read: 0,
            disk_write: 0,
            energy_nj: 0,
            start_unix: start,
            comm: comm.into(),
            exe_path: exe.into(),
        }
    }

    #[test]
    fn a_known_process_keeps_its_path_until_it_starts_again_or_changes_name() {
        let prev = prev_proc(100.5, "node", "/usr/bin/node");
        assert_eq!(known_identity(Some(&prev), 100.5, "node"), Some("/usr/bin/node"));
        // The pid was reused: a new start time.
        assert_eq!(known_identity(Some(&prev), 200.5, "node"), None);
        // The same process ran another program (exec).
        assert_eq!(known_identity(Some(&prev), 100.5, "bun"), None);
        assert_eq!(known_identity(None, 100.5, "node"), None);
    }

    /// Prints what the sampler costs with nothing on screen and with the
    /// Network page open, to compare before and after a change. Counts
    /// `lsof`, `nettop` and `system_profiler` launches through shims on PATH.
    #[test]
    #[ignore = "samples for about a minute per mode; cargo test --release native_sampling_cost -- --ignored --nocapture"]
    fn native_sampling_cost_hidden_and_visible() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tidegauge-shims-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("spawns.log");
        std::fs::write(&log, "").unwrap();
        for (name, real) in [
            ("lsof", "/usr/sbin/lsof"),
            ("nettop", "/usr/bin/nettop"),
            ("system_profiler", "/usr/sbin/system_profiler"),
        ] {
            let script = dir.join(name);
            std::fs::write(&script, format!("#!/bin/sh\necho {name} >> '{}'\nexec {real} \"$@\"\n", log.display())).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::env::set_var("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()));
        let spawns = |tool: &str| std::fs::read_to_string(&log).unwrap().lines().filter(|l| *l == tool).count();
        let cpu = || unsafe {
            let mut total = 0.0;
            for who in [libc::RUSAGE_SELF, libc::RUSAGE_CHILDREN] {
                let mut r: libc::rusage = std::mem::zeroed();
                libc::getrusage(who, &mut r);
                total += r.ru_utime.tv_sec as f64 + r.ru_utime.tv_usec as f64 / 1e6;
                total += r.ru_stime.tv_sec as f64 + r.ru_stime.tv_usec as f64 / 1e6;
            }
            total
        };
        crate::sensors::start();
        crate::disks::start();
        crate::projects::start_nettop();
        let mut engine = Engine::new();
        // Start-up work (first inventory, sensor discovery, first readings)
        // is not what idle costs.
        for _ in 0..10 {
            let bt = crate::bluetooth::bt_info(crate::demand::current().bluetooth_period());
            engine.sample(Some(Vec::new()), bt);
            std::thread::sleep(Duration::from_secs(2));
        }
        for (label, page) in [
            ("hidden", None),
            ("Network page open", Some(crate::demand::Page::Network)),
        ] {
            crate::demand::set_window(page);
            let collections = || crate::disks::inventory().collected_at;
            let (cpu0, wall0, collected0) = (cpu(), Instant::now(), collections());
            let before = ["lsof", "nettop", "system_profiler"].map(spawns);
            let mut busy = Duration::ZERO;
            let mut inventories = 0;
            let ticks = 30;
            for _ in 0..ticks {
                let started = Instant::now();
                let bt = crate::bluetooth::bt_info(crate::demand::current().bluetooth_period());
                engine.sample(Some(Vec::new()), bt);
                publish(&engine.snap);
                busy += started.elapsed();
                std::thread::sleep(Duration::from_secs(2).saturating_sub(started.elapsed()));
            }
            if collections() != collected0 {
                inventories += 1;
            }
            let after = ["lsof", "nettop", "system_profiler"].map(spawns);
            println!(
                "{label}: {:.1} ms per sample, cpu {:.2}% of one core (children included), lsof {} nettop {} system_profiler {}, diskutil inventory refreshed: {}",
                busy.as_secs_f64() * 1000.0 / ticks as f64,
                (cpu() - cpu0) / wall0.elapsed().as_secs_f64() * 100.0,
                after[0] - before[0],
                after[1] - before[1],
                after[2] - before[2],
                inventories > 0,
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_reads_do_not_wait_for_collection() {
        let shared_engine = engine();
        let guard = shared_engine.lock().unwrap();
        let (send, recv) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || send.send(latest().at).unwrap());
        let result = recv.recv_timeout(Duration::from_secs(1));
        drop(guard);
        reader.join().unwrap();
        assert!(result.is_ok(), "snapshot read blocked behind collection");
    }
}
