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
}

#[derive(Clone, Default)]
pub struct AudioInfo {
    pub output_id: u32,
    pub device_name: String,
    pub device_volume: Option<f32>,
    pub device_muted: Option<bool>,
    pub can_volume: bool,
    pub can_mute: bool,
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
    pub process_groups: Vec<crate::process_activity::ProcessGroup>,
    pub resource_activity: Vec<crate::resource_activity::Activity>,
    pub proc_count: usize,
    pub app_count: usize,
    pub chip: ChipInfo,
    pub power_w: Option<f64>,
    pub power_hist: Hist,
    pub thermal: String,
    pub neural: Option<f64>,
    pub neural_hist: Hist,
    pub uptime: Duration,
    pub alerts: Vec<Alert>,
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
            process_groups: Vec::new(),
            resource_activity: Vec::new(),
            proc_count: 0,
            app_count: 0,
            chip: ChipInfo::default(),
            power_w: None,
            power_hist: h(),
            thermal: "—".into(),
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
    process_tracker: crate::process_activity::Tracker,
    resource_tracker: crate::resource_activity::Tracker,
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
            process_tracker: crate::process_activity::Tracker::default(),
            resource_tracker: crate::resource_activity::Tracker::default(),
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
    pub fn sample(&mut self, audio_apps: Vec<AudioApp>, bt: BtInfo) {
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
        s.thermal = thermal_state().unwrap_or("—").into();

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
        s.net.primary = default_interface();
        s.net.kind = "Wi-Fi".into();
        s.net_in_hist.push(s.net.rate_in);
        s.net_out_hist.push(s.net.rate_out);
        s.net.peak_in = s.net.peak_in.max(s.net.rate_in);
        s.net.peak_out = s.net.peak_out.max(s.net.rate_out);

        // GPU
        s.gpu = gpu_stats();
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

        // Processes
        let pids = list_pids();
        let enumerated = pids.iter().copied().collect();
        let mut procs = Vec::with_capacity(pids.len());
        for pid in pids {
            let Some(b) = proc_bsd(pid) else { continue };
            let Some(r) = proc_rusage(pid) else { continue };
            let start = b.pbi_start_tvsec as f64 + b.pbi_start_tvusec as f64 / 1e6;
            let cpu_ns = mach_ticks_to_ns(r.ri_user_time.saturating_add(r.ri_system_time));
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
                (
                    cpu_ns as f64 / life / 1e9 * 100.0,
                    r.ri_diskio_bytesread as f64 / life,
                    r.ri_diskio_byteswritten as f64 / life,
                    r.ri_billed_energy as f64 / life / 1e9,
                )
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
                interrupt_wakeups: Some(r.ri_interrupt_wkups),
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
        s.process_groups = self.process_tracker.sample(&procs, &enumerated);
        s.resource_activity = self.resource_tracker.sample(&procs, self.started.elapsed().as_secs_f64(), now_unix());
        s.apps = regroup_apps(&procs, &s.apps);
        s.app_count = s.apps.len();

        // attach per-app network via nettop (every other tick to save CPU)
        if self.tick % 2 == 0 {
            crate::projects::apply_nettop(&mut s.apps);
        }

        // Battery terminal power and per-process estimates are not system power.
        s.power_w = None;
        s.power_hist.pts.clear();

        s.audio = AudioInfo {
            apps: audio_apps,
            ..s.audio.clone()
        };
        let output = crate::audio::output_state();
        s.audio.output_id = output.id;
        s.audio.device_name = output.name;
        s.audio.device_volume = output.volume;
        s.audio.device_muted = output.muted;
        s.audio.can_volume = output.can_volume;
        s.audio.can_mute = output.can_mute;
        if self.tick % 10 == 0 {
            s.audio.devices = crate::audio::output_device_names();
        }
        s.bt = bt;
        s.projects = crate::projects::detect_projects(&procs);
        s.uptime = self.started.elapsed();

        s.neural = None; // No public ANE counter; leave the history empty.
    }
}

// UI readers share only the last completed sample, never the collection lock.
static PUBLISHED: OnceLock<Mutex<Snapshot>> = OnceLock::new();

fn published() -> &'static Mutex<Snapshot> {
    PUBLISHED.get_or_init(|| Mutex::new(Snapshot::default()))
}

pub fn publish(snap: &Snapshot) {
    let snapshot = snap.clone();
    *published().lock().unwrap() = snapshot;
}

pub fn latest() -> Snapshot {
    published().lock().unwrap().clone()
}

pub fn fmt_bytes(n: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1.0 {
        return "0 B".into();
    }
    let mut v = n;
    let mut i = 0;
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", v as u64)
    } else if v >= 100.0 {
        format!("{:.0} {}", v, U[i])
    } else if v >= 10.0 {
        format!("{:.1} {}", v, U[i])
    } else {
        format!("{:.2} {}", v, U[i])
    }
}
pub fn fmt_rate(n: f64) -> String {
    format!("{}/s", fmt_bytes(n))
}
pub fn fmt_watts(w: f64) -> String {
    if w < 0.05 {
        "0.00 W".into()
    } else if w < 10.0 {
        format!("{:.2} W", w)
    } else {
        format!("{:.1} W", w)
    }
}
pub fn fmt_pct(v: f64) -> String {
    format!("{:.0}%", v)
}
pub fn fmt_up(secs: f64) -> String {
    let s = secs as u64;
    if s < 90 {
        let n = s.max(1);
        format!("{} {}", n, if n == 1 { l("second", "sekund") } else { l("seconds", "sekunder") })
    } else if s < 5400 {
        let n = s / 60;
        format!("{} {}", n, if n == 1 { l("minute", "minutt") } else { l("minutes", "minutter") })
    } else if s < 129600 {
        let n = s / 3600;
        format!("{} {}", n, if n == 1 { l("hour", "time") } else { l("hours", "timer") })
    } else {
        let n = s / 86400;
        format!("{} {}", n, if n == 1 { l("day", "døgn") } else { l("days", "dager") })
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
        sampler.sample(Vec::new(), BtInfo::default());
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
