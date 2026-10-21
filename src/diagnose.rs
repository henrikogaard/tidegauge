//! "Why is my Mac slow?": reads the latest snapshot and its recent history and
//! says, in plain words, what is slowing the Mac and what to do about it.
//! It measures nothing itself and never guesses: a check whose reading is
//! missing is skipped, not reported as fine or as a problem.

use crate::engine::{fmt_bytes, fmt_disk_bytes, fmt_celsius, fmt_pct, fmt_rate, fmt_up, Hist, Snapshot};
use crate::locale::text as l;
use crate::metrics::Pressure;
use crate::process_control::Force;
use crate::processes::AppGroup;
use crate::tools_panel::Page;
use crate::ui::Destination;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// Swap in use at which the Mac is said to lean on the disk for memory, and
/// where that becomes a warning. Swap is kept after the pressure that caused
/// it has passed, so it alone is a notice until it is large.
pub const SWAP_NOTICE_BYTES: u64 = GIB;
pub const SWAP_WARNING_BYTES: u64 = 4 * GIB;
/// The recent history the sustained checks average: 15 samples, about half a
/// minute at one sample every two seconds. Fewer than the minimum is too
/// little to call anything sustained.
pub const SUSTAINED_SAMPLES: usize = 15;
pub const MIN_SUSTAINED_SAMPLES: usize = 10;
/// Whole-Mac CPU use (0-100 %) that, sustained, leaves other work waiting.
pub const CPU_HIGH_PCT: f64 = 80.;
/// A chip temperature at which Apple silicon is close to slowing itself down.
pub const CHIP_HOT_C: f64 = 95.;
/// The startup disk counts as nearly full below this share or amount free.
pub const DISK_LOW_FRACTION: f64 = 0.10;
pub const DISK_LOW_BYTES: u64 = 10 * GIB;
/// Whole-Mac disk reads plus writes that, sustained, count as heavy.
pub const DISK_IO_HIGH_BPS: f64 = 50. * MIB as f64;
/// On battery at or below this charge is a low battery.
pub const LOW_BATTERY_PCT: f64 = 20.;
/// Time since the last restart after which a restart is worth suggesting.
pub const LONG_UPTIME_SECS: f64 = 14. * 86_400.;
/// How many apps are named, and the least each must use to be named.
pub const NAMED_APPS: usize = 3;
pub const APP_MEM_MIN_BYTES: u64 = 256 * MIB;
pub const APP_CPU_MIN_PCT: f64 = 20.;
pub const APP_DISK_MIN_BPS: f64 = 5. * MIB as f64;
/// The least CPU an app needs to count among the busiest when all is well.
pub const BUSY_APP_MIN_PCT: f64 = 1.;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Ok,
    Notice,
    Warning,
}

/// An app and what it uses, already formatted for the screen.
#[derive(Clone, PartialEq)]
pub struct AppUse {
    pub name: String,
    pub value: String,
}

#[derive(Clone, PartialEq)]
pub enum Action {
    /// Quit the app with this key, through the usual confirmation.
    Quit { key: String, name: String },
    /// Go to another page.
    Open {
        to: Destination,
        label: &'static str,
    },
}

#[derive(Clone, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    pub title: &'static str,
    /// One plain sentence; any technical term is explained in it.
    pub text: &'static str,
    /// A measured fact that backs the sentence up.
    pub detail: Option<String>,
    pub apps: Vec<AppUse>,
    pub action: Option<Action>,
}

impl Finding {
    fn new(severity: Severity, title: &'static str, text: &'static str) -> Self {
        Self {
            severity,
            title,
            text,
            detail: None,
            apps: Vec::new(),
            action: None,
        }
    }
    fn detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }
    fn apps(mut self, apps: Vec<AppUse>) -> Self {
        self.apps = apps;
        self
    }
    fn action(mut self, action: Option<Action>) -> Self {
        self.action = action;
        self
    }
}

/// What is slowing the Mac, worst first (equal severities keep the order of
/// impact the checks run in). When nothing is, a single friendly finding.
pub fn diagnose(s: &Snapshot) -> Vec<Finding> {
    let mut findings: Vec<Finding> = [
        memory(s),
        cpu(s),
        heat(s),
        disk_space(s),
        disk_activity(s),
        low_power_mode(s),
        low_battery(s),
        long_uptime(s),
    ]
    .into_iter()
    .flatten()
    .collect();
    findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
    if findings.is_empty() {
        let busiest = top(s, |a| a.cpu_pct, BUSY_APP_MIN_PCT);
        let unchecked = unchecked(s);
        let finding = if unchecked.is_empty() {
            Finding::new(
                Severity::Ok,
                l(
                    "Nothing is slowing your Mac down right now",
                    "Ingenting gjør Macen din treg akkurat nå",
                ),
                l(
                    "Memory, processor, heat and disk space all look normal.",
                    "Minne, prosessor, varme og diskplass ser normale ut.",
                ),
            )
        } else {
            // Some checks never ran, so "all normal" would be a claim about
            // readings nobody looked at.
            Finding::new(
                Severity::Ok,
                l("No problem found so far", "Ingen problemer funnet så langt"),
                l(
                    "Nothing wrong was found, but some checks could not run yet: they need a few more seconds of readings, or this Mac does not report them.",
                    "Ingenting galt ble funnet, men noen sjekker kunne ikke kjøres ennå: de trenger noen sekunder til med målinger, eller så melder ikke denne Macen dem.",
                ),
            )
            .detail(format!(
                "{} {}",
                l("Not checked:", "Ikke sjekket:"),
                unchecked.join(", ")
            ))
        };
        findings.push(finding.apps(uses(&busiest, |a| fmt_pct(a.cpu_pct))));
    }
    findings
}

/// The checks named in the all-clear sentence that had no usable reading:
/// too little history yet, or a value this Mac does not report.
fn unchecked(s: &Snapshot) -> Vec<&'static str> {
    let has_history = |h: &Hist| h.pts.len() >= MIN_SUSTAINED_SAMPLES;
    [
        (s.mem.pressure.is_some(), l("memory", "minne")),
        (has_history(&s.cpu_hist), l("CPU", "CPU")),
        (s.thermal_level.is_some(), l("heat", "varme")),
        (s.disk.total > 0, l("disk space", "diskplass")),
        (
            has_history(&s.disk_r_hist) && has_history(&s.disk_w_hist),
            l("disk activity", "diskaktivitet"),
        ),
    ]
    .into_iter()
    .filter_map(|(checked, name)| (!checked).then_some(name))
    .collect()
}

/// The mean of the last `SUSTAINED_SAMPLES` readings, or `None` while there
/// are too few to call anything sustained.
fn sustained(history: &Hist) -> Option<f64> {
    let points = &history.pts;
    if points.len() < MIN_SUSTAINED_SAMPLES {
        return None;
    }
    let recent = &points[points.len().saturating_sub(SUSTAINED_SAMPLES)..];
    Some(recent.iter().sum::<f64>() / recent.len() as f64)
}

/// The apps using the most of something, busiest first, at least `min`.
fn top(s: &Snapshot, measure: impl Fn(&AppGroup) -> f64, min: f64) -> Vec<&AppGroup> {
    let mut apps: Vec<&AppGroup> = s.apps.iter().filter(|a| measure(a) >= min).collect();
    apps.sort_by(|a, b| measure(b).total_cmp(&measure(a)));
    apps.truncate(NAMED_APPS);
    apps
}

fn uses(apps: &[&AppGroup], value: impl Fn(&AppGroup) -> String) -> Vec<AppUse> {
    apps.iter()
        .map(|a| AppUse {
            name: a.name.clone(),
            value: value(a),
        })
        .collect()
}

/// Quit the first of `apps` that Tidegauge may quit.
fn quit_first(s: &Snapshot, apps: &[&AppGroup]) -> Option<Action> {
    apps.iter()
        .find(|a| crate::process_panel::quit_action(s, a, Force::Quit).is_some())
        .map(|a| Action::Quit {
            key: a.key.clone(),
            name: a.name.clone(),
        })
}

fn memory(s: &Snapshot) -> Option<Finding> {
    let m = &s.mem;
    let swap = m.swap_used;
    let finding = match m.pressure {
        Some(Pressure::Critical) => Finding::new(
            Severity::Warning,
            l("Memory is almost full", "Minnet er nesten fullt"),
            l(
                "The Mac is out of room for open apps, so it keeps moving data to the disk (called swap), which is much slower.",
                "Macen går tom for plass til åpne apper, så den flytter hele tiden data til disken (kalt swap), som er mye tregere.",
            ),
        ),
        Some(Pressure::Warning) => Finding::new(
            Severity::Warning,
            l("Memory is running low", "Minnet begynner å bli lite"),
            l(
                "Open apps use most of the memory, so the Mac squeezes data together and starts using the disk as extra memory (called swap).",
                "Åpne apper bruker det meste av minnet, så Macen presser data sammen og begynner å bruke disken som ekstra minne (kalt swap).",
            ),
        ),
        _ if swap >= SWAP_NOTICE_BYTES => Finding::new(
            if swap >= SWAP_WARNING_BYTES { Severity::Warning } else { Severity::Notice },
            l("The Mac is using the disk as extra memory (swap)", "Macen bruker disken som ekstra minne (swap)"),
            l(
                "Apps have asked for more memory than the Mac has, which makes everything slower.",
                "Apper har bedt om mer minne enn Macen har, og det gjør alt tregere.",
            ),
        ),
        _ => return None,
    };
    let apps = top(s, |a| a.mem_bytes as f64, APP_MEM_MIN_BYTES as f64);
    let finding = finding
        .apps(uses(&apps, |a| fmt_bytes(a.mem_bytes as f64)))
        .action(quit_first(s, &apps));
    Some(if swap > 0 {
        finding.detail(format!(
            "{} {}",
            l("Swap in use:", "Swap i bruk:"),
            fmt_bytes(swap as f64)
        ))
    } else {
        finding
    })
}

fn cpu(s: &Snapshot) -> Option<Finding> {
    let mean = sustained(&s.cpu_hist).filter(|mean| *mean >= CPU_HIGH_PCT)?;
    let apps = top(s, |a| a.cpu_pct, APP_CPU_MIN_PCT);
    Some(
        Finding::new(
            Severity::Warning,
            l("The processor is working flat out", "Prosessoren jobber for fullt"),
            l(
                "The CPU (the Mac's main chip) has been close to full for the last half minute, so other work has to wait.",
                "CPU-en (Macens hovedbrikke) har vært nesten full det siste halve minuttet, så annet arbeid må vente.",
            ),
        )
        .detail(format!("{} {}", l("Average:", "Snitt:"), fmt_pct(mean)))
        .apps(uses(&apps, |a| fmt_pct(a.cpu_pct)))
        .action(quit_first(s, &apps)),
    )
}

fn heat(s: &Snapshot) -> Option<Finding> {
    let throttled = s.thermal_level.is_some_and(|level| level >= 2);
    let chip_hot = s.temps.chip.filter(|c| *c >= CHIP_HOT_C);
    if !throttled && chip_hot.is_none() {
        return None;
    }
    let finding = if throttled {
        Finding::new(
            Severity::Warning,
            l("The Mac is slowing itself down to cool off", "Macen bremser seg selv for å kjøle seg ned"),
            l(
                "It is too hot, so macOS holds the chip back: keep the lid open, keep the vents clear, and quit heavy apps.",
                "Den er for varm, så macOS holder brikken igjen: ha lokket åpent, hold luftehullene frie og avslutt tunge apper.",
            ),
        )
    } else {
        Finding::new(
            Severity::Notice,
            l("The chip is very hot", "Brikken er veldig varm"),
            l(
                "macOS may soon slow it down to cool off, so keep the lid open, keep the vents clear, and quit heavy apps.",
                "macOS kan snart bremse den for å kjøle den ned, så ha lokket åpent, hold luftehullene frie og avslutt tunge apper.",
            ),
        )
    };
    let apps = top(s, |a| a.cpu_pct, APP_CPU_MIN_PCT);
    let finding = finding.apps(uses(&apps, |a| fmt_pct(a.cpu_pct)));
    Some(match chip_hot.or(s.temps.chip) {
        Some(c) => finding.detail(format!("{} {}", l("Chip:", "Brikke:"), fmt_celsius(c))),
        None => finding,
    })
}

fn disk_space(s: &Snapshot) -> Option<Finding> {
    let d = &s.disk;
    // Without a total there is nothing to compare.
    if d.total == 0 {
        return None;
    }
    if d.free as f64 >= d.total as f64 * DISK_LOW_FRACTION && d.free >= DISK_LOW_BYTES {
        return None;
    }
    Some(
        Finding::new(
            Severity::Warning,
            l("The startup disk is nearly full", "Oppstartsdisken er nesten full"),
            l(
                "macOS needs free space for swap (disk used as extra memory) and for updates, so a full disk slows everything down.",
                "macOS trenger ledig plass til swap (disk brukt som ekstra minne) og til oppdateringer, så en full disk gjør alt tregere.",
            ),
        )
        .detail(format!(
            "{} {} {} {}",
            fmt_disk_bytes(d.free as f64),
            l("free of", "ledig av"),
            fmt_disk_bytes(d.total as f64),
            l("(as macOS reports it)", "(slik macOS melder det)")
        ))
        .action(Some(Action::Open {
            to: Destination::Tool(Page::Storage),
            label: l("Open folder sizes", "Åpne mappestørrelser"),
        })),
    )
}

fn disk_activity(s: &Snapshot) -> Option<Finding> {
    let total: Vec<f64> = s
        .disk_r_hist
        .pts
        .iter()
        .zip(&s.disk_w_hist.pts)
        .map(|(r, w)| r + w)
        .collect();
    let mean = sustained(&Hist {
        pts: total,
        max_len: 0,
    })
    .filter(|mean| *mean >= DISK_IO_HIGH_BPS)?;
    let apps = top(
        s,
        |a| a.disk_read_rate + a.disk_write_rate,
        APP_DISK_MIN_BPS,
    );
    Some(
        Finding::new(
            Severity::Notice,
            l("The disk is very busy", "Disken er veldig travel"),
            l(
                "Something has been reading or writing a lot of data for the last half minute, which can make other apps wait.",
                "Noe har lest eller skrevet mye data det siste halve minuttet, og det kan få andre apper til å vente.",
            ),
        )
        .detail(format!("{} {}", l("Average:", "Snitt:"), fmt_rate(mean)))
        .apps(uses(&apps, |a| fmt_rate(a.disk_read_rate + a.disk_write_rate)))
        .action(quit_first(s, &apps)),
    )
}

fn low_power_mode(s: &Snapshot) -> Option<Finding> {
    s.low_power.then(|| {
        Finding::new(
            Severity::Notice,
            l("Low Power Mode is on", "Lavstrømmodus er på"),
            l(
                "macOS limits the Mac's speed to save battery, and you can turn it off in System Settings under Battery.",
                "macOS begrenser hastigheten for å spare batteri, og du kan slå det av i Systeminnstillinger under Batteri.",
            ),
        )
    })
}

fn low_battery(s: &Snapshot) -> Option<Finding> {
    let b = &s.battery;
    let percent = b
        .percent
        .filter(|p| b.present && b.on_battery && *p <= LOW_BATTERY_PCT)?;
    Some(
        Finding::new(
            Severity::Notice,
            l("The battery is low", "Batteriet er lavt"),
            l(
                "Plug in the charger, because on battery macOS favours saving power over speed.",
                "Koble til laderen, for på batteri prioriterer macOS strømsparing fremfor fart.",
            ),
        )
        .detail(format!("{} {}", l("Charge:", "Ladning:"), fmt_pct(percent))),
    )
}

fn long_uptime(s: &Snapshot) -> Option<Finding> {
    let up = s.at - s.boot_unix?;
    (up >= LONG_UPTIME_SECS).then(|| {
        Finding::new(
            Severity::Notice,
            l("A restart can help", "En omstart kan hjelpe"),
            l(
                "The Mac has been on for a long time, and a restart clears out memory and background work that has built up.",
                "Macen har vært på lenge, og en omstart rydder bort minne og bakgrunnsarbeid som har hopet seg opp.",
            ),
        )
        .detail(format!("{} {} {}", l("Last restart:", "Siste omstart:"), fmt_up(up), l("ago", "siden")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MemStats;
    use crate::processes::ProcInfo;

    const NOW: f64 = 1_800_000_000.;

    /// A healthy Mac: nothing here should raise a finding.
    fn healthy() -> Snapshot {
        let mut s = Snapshot {
            at: NOW,
            ..Snapshot::default()
        };
        s.mem = MemStats {
            total: 16 * GIB,
            used: 8 * GIB,
            pressure: Some(Pressure::Normal),
            ..Default::default()
        };
        s.disk.total = 500 * GIB;
        s.disk.free = 200 * GIB;
        s.thermal_level = Some(0);
        s.boot_unix = Some(NOW - 86_400.);
        s.cpu_hist = history(&[5.; SUSTAINED_SAMPLES]);
        s.disk_r_hist = history(&[0.; SUSTAINED_SAMPLES]);
        s.disk_w_hist = history(&[0.; SUSTAINED_SAMPLES]);
        s
    }

    fn app(key: &str, cpu: f64, mem: u64) -> AppGroup {
        AppGroup {
            key: key.into(),
            name: key.into(),
            pids: vec![pid_of(key)],
            cpu_pct: cpu,
            mem_bytes: mem,
            ..Default::default()
        }
    }

    /// A stable pid per test app name.
    fn pid_of(key: &str) -> i32 {
        100 + key.bytes().map(|b| b as i32).sum::<i32>()
    }

    /// The process behind `app`, owned by the user running the test (or not).
    fn process(key: &str, mine: bool) -> ProcInfo {
        let me = unsafe { libc::getuid() };
        ProcInfo {
            pid: pid_of(key),
            uid: if mine { me } else { me + 1 },
            exe_path: format!("/Applications/{key}.app/Contents/MacOS/{key}"),
            comm: key.into(),
            start_unix: 1_700_000_000.,
            ..Default::default()
        }
    }

    fn with_apps(mut s: Snapshot, apps: &[(&str, f64, u64, bool)]) -> Snapshot {
        for (key, cpu, mem, mine) in apps {
            s.apps.push(app(key, *cpu, *mem));
            s.procs.push(process(key, *mine));
        }
        s
    }

    fn history(values: &[f64]) -> Hist {
        Hist {
            pts: values.to_vec(),
            max_len: 360,
        }
    }

    fn only(s: &Snapshot) -> Finding {
        let mut findings = diagnose(s);
        assert_eq!(findings.len(), 1, "expected one finding");
        findings.remove(0)
    }

    fn quit_name(f: &Finding) -> Option<&str> {
        match &f.action {
            Some(Action::Quit { name, .. }) => Some(name),
            _ => None,
        }
    }

    #[test]
    fn a_healthy_mac_gets_one_friendly_all_clear_naming_the_busiest_apps() {
        let s = with_apps(
            healthy(),
            &[
                ("Mail", 3., GIB, true),
                ("Safari", 40., 2 * GIB, true),
                ("Xcode", 12., 3 * GIB, true),
                ("Notes", 7., GIB, true),
                ("Idle", 0.2, GIB, true),
            ],
        );
        let f = only(&s);
        assert_eq!(f.severity, Severity::Ok);
        assert_eq!(
            f.title,
            l(
                "Nothing is slowing your Mac down right now",
                "Ingenting gjør Macen din treg akkurat nå"
            )
        );
        let names: Vec<&str> = f.apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            ["Safari", "Xcode", "Notes"],
            "the three busiest, busiest first"
        );
        assert!(f.action.is_none());
    }

    #[test]
    fn missing_readings_are_not_problems() {
        // A fresh start: no history, no pressure level, no thermal state,
        // no disk total, no boot time.
        let s = Snapshot {
            at: NOW,
            ..Snapshot::default()
        };
        assert_eq!(only(&s).severity, Severity::Ok);
    }

    fn full_all_clear_title() -> &'static str {
        l(
            "Nothing is slowing your Mac down right now",
            "Ingenting gjør Macen din treg akkurat nå",
        )
    }

    #[test]
    fn too_little_history_is_not_a_full_all_clear() {
        let mut s = healthy();
        s.cpu_hist = history(&[5.; MIN_SUSTAINED_SAMPLES - 1]);
        s.disk_r_hist = history(&[0.; MIN_SUSTAINED_SAMPLES - 1]);
        s.disk_w_hist = history(&[0.; MIN_SUSTAINED_SAMPLES - 1]);
        let f = only(&s);
        assert_eq!(f.severity, Severity::Ok);
        assert_ne!(f.title, full_all_clear_title());
        let detail = f.detail.expect("the unchecked checks are named");
        assert!(detail.contains(l("CPU", "CPU")), "{detail}");
        assert!(
            detail.contains(l("disk activity", "diskaktivitet")),
            "{detail}"
        );
        assert!(!detail.contains(l("memory", "minne")), "{detail}");
        assert!(!detail.contains(l("heat", "varme")), "{detail}");
    }

    #[test]
    fn missing_readings_are_named_instead_of_called_normal() {
        let mut s = healthy();
        s.mem.pressure = None;
        s.thermal_level = None;
        s.disk.total = 0;
        let f = only(&s);
        assert_ne!(f.title, full_all_clear_title());
        let detail = f.detail.expect("the unchecked checks are named");
        for name in [
            l("memory", "minne"),
            l("heat", "varme"),
            l("disk space", "diskplass"),
        ] {
            assert!(detail.contains(name), "{name} missing from {detail}");
        }
        assert!(!detail.contains(l("CPU", "CPU")), "{detail}");
    }

    #[test]
    fn every_reading_present_gives_the_full_all_clear() {
        let f = only(&healthy());
        assert_eq!(f.title, full_all_clear_title());
        assert!(f.detail.is_none());
    }

    #[test]
    fn memory_pressure_is_a_warning_that_names_the_top_memory_apps() {
        for pressure in [Pressure::Warning, Pressure::Critical] {
            let mut s = with_apps(
                healthy(),
                &[
                    ("Small", 1., 300 * MIB, true),
                    ("Big", 1., 6 * GIB, true),
                    ("Medium", 1., 2 * GIB, true),
                    ("Tiny", 1., 10 * MIB, true),
                ],
            );
            s.mem.pressure = Some(pressure);
            let f = only(&s);
            assert_eq!(f.severity, Severity::Warning);
            let names: Vec<&str> = f.apps.iter().map(|a| a.name.as_str()).collect();
            assert_eq!(names, ["Big", "Medium", "Small"], "tiny apps are not named");
            assert_eq!(quit_name(&f), Some("Big"));
        }
        let mut warning = healthy();
        warning.mem.pressure = Some(Pressure::Warning);
        let mut critical = healthy();
        critical.mem.pressure = Some(Pressure::Critical);
        assert_ne!(only(&warning).title, only(&critical).title);
    }

    #[test]
    fn the_quit_action_skips_apps_that_cannot_be_quit() {
        let mut s = with_apps(
            healthy(),
            &[("Root", 1., 6 * GIB, false), ("Mine", 1., 2 * GIB, true)],
        );
        s.mem.pressure = Some(Pressure::Warning);
        let f = only(&s);
        assert_eq!(f.apps[0].name, "Root", "the biggest is still named");
        assert_eq!(
            quit_name(&f),
            Some("Mine"),
            "but the quit goes to the first one that may be quit"
        );
        let mut s = with_apps(healthy(), &[("Root", 1., 6 * GIB, false)]);
        s.mem.pressure = Some(Pressure::Warning);
        assert!(only(&s).action.is_none());
    }

    #[test]
    fn swap_alone_is_a_notice_and_a_warning_when_large() {
        let mut s = healthy();
        s.mem.swap_used = GIB - 1;
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "under 1 GB of swap is fine"
        );
        s.mem.swap_used = GIB + GIB / 2;
        let f = only(&s);
        assert_eq!(f.severity, Severity::Notice);
        assert!(f.detail.is_some(), "the swap amount backs it up");
        s.mem.swap_used = SWAP_WARNING_BYTES;
        assert_eq!(only(&s).severity, Severity::Warning);
    }

    #[test]
    fn sustained_cpu_is_a_warning_but_a_spike_is_not() {
        let mut s = with_apps(
            healthy(),
            &[("Render", 350., GIB, true), ("Quiet", 2., GIB, true)],
        );
        s.cpu_hist = history(&[92.; SUSTAINED_SAMPLES]);
        let f = only(&s);
        assert_eq!(f.severity, Severity::Warning);
        assert_eq!(f.apps.len(), 1, "only apps above the floor are named");
        assert_eq!(quit_name(&f), Some("Render"));

        let mut spike = vec![10.; SUSTAINED_SAMPLES - 1];
        spike.push(100.);
        s.cpu_hist = history(&spike);
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "one busy sample is not sustained"
        );

        s.cpu_hist = history(&[100.; MIN_SUSTAINED_SAMPLES - 1]);
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "too little history to call it sustained"
        );

        // Only the recent window counts: a busy past that has calmed is fine.
        let mut calmed = vec![100.; 100];
        calmed.extend([5.; SUSTAINED_SAMPLES]);
        s.cpu_hist = history(&calmed);
        assert_eq!(only(&s).severity, Severity::Ok);
    }

    #[test]
    fn heat_is_reported_from_macos_thermal_state_or_a_very_hot_chip() {
        let mut s = healthy();
        s.thermal_level = Some(1);
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "fair is not slowing anything"
        );
        for level in [2, 3] {
            s.thermal_level = Some(level);
            let f = only(&s);
            assert_eq!(f.severity, Severity::Warning);
            assert_eq!(
                f.title,
                l(
                    "The Mac is slowing itself down to cool off",
                    "Macen bremser seg selv for å kjøle seg ned"
                )
            );
        }
        s.thermal_level = Some(0);
        s.temps.chip = Some(CHIP_HOT_C - 1.);
        assert_eq!(only(&s).severity, Severity::Ok);
        s.temps.chip = Some(CHIP_HOT_C);
        let f = only(&s);
        assert_eq!(f.severity, Severity::Notice, "very hot but not yet slowed");
        assert!(f.detail.is_some());
        s.thermal_level = None;
        s.temps.chip = None;
        assert_eq!(only(&s).severity, Severity::Ok, "no reading, no finding");
    }

    #[test]
    fn a_nearly_full_disk_points_to_folder_sizes() {
        let mut s = healthy();
        s.disk.free = 49 * GIB;
        assert_eq!(
            only(&s).severity,
            Severity::Warning,
            "9.8 % free is nearly full"
        );
        let f = only(&s);
        assert!(
            matches!(f.action, Some(Action::Open { to, .. }) if to == Destination::Tool(Page::Storage))
        );
        s.disk.free = 50 * GIB;
        assert_eq!(only(&s).severity, Severity::Ok, "10 % free is enough");
        s.disk.total = 100 * GIB;
        s.disk.free = 9 * GIB;
        assert_eq!(only(&s).severity, Severity::Warning, "under 10 GB free");
        s.disk.total = 0;
        s.disk.free = 0;
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "no total, nothing to compare"
        );
    }

    #[test]
    fn sustained_heavy_disk_activity_names_the_busy_app() {
        let mut s = healthy();
        let mut busy = app("Backup", 5., GIB);
        busy.disk_write_rate = 120. * MIB as f64;
        let mut quiet = app("Mail", 5., GIB);
        quiet.disk_read_rate = MIB as f64;
        s.apps = vec![quiet, busy];
        s.procs = vec![process("Backup", true), process("Mail", true)];
        s.disk_r_hist = history(&[10. * MIB as f64; SUSTAINED_SAMPLES]);
        s.disk_w_hist = history(&[90. * MIB as f64; SUSTAINED_SAMPLES]);
        let f = only(&s);
        assert_eq!(f.severity, Severity::Notice);
        assert_eq!(f.apps.len(), 1);
        assert_eq!(f.apps[0].name, "Backup");

        // A short burst in an otherwise quiet minute is not sustained.
        let mut quiet_minute = vec![MIB as f64; SUSTAINED_SAMPLES - 1];
        quiet_minute.push(300. * MIB as f64);
        s.disk_r_hist = history(&quiet_minute);
        s.disk_w_hist = history(&[0.; SUSTAINED_SAMPLES]);
        assert_eq!(only(&s).severity, Severity::Ok);
    }

    #[test]
    fn low_power_mode_and_a_low_battery_are_notes() {
        let mut s = healthy();
        s.low_power = true;
        let f = only(&s);
        assert_eq!(f.severity, Severity::Notice);
        assert_eq!(f.title, l("Low Power Mode is on", "Lavstrømmodus er på"));

        let mut s = healthy();
        s.battery.present = true;
        s.battery.on_battery = true;
        s.battery.percent = Some(LOW_BATTERY_PCT);
        assert_eq!(only(&s).severity, Severity::Notice);
        s.battery.percent = Some(LOW_BATTERY_PCT + 1.);
        assert_eq!(only(&s).severity, Severity::Ok);
        s.battery.percent = Some(5.);
        s.battery.on_battery = false;
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "a low battery on the charger is fine"
        );
        s.battery.on_battery = true;
        s.battery.present = false;
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "no battery, no battery note"
        );
    }

    #[test]
    fn a_long_uptime_suggests_a_restart() {
        let mut s = healthy();
        s.boot_unix = Some(NOW - LONG_UPTIME_SECS + 1.);
        assert_eq!(only(&s).severity, Severity::Ok);
        s.boot_unix = Some(NOW - LONG_UPTIME_SECS);
        let f = only(&s);
        assert_eq!(f.severity, Severity::Notice);
        assert_eq!(f.title, l("A restart can help", "En omstart kan hjelpe"));
        s.boot_unix = None;
        assert_eq!(
            only(&s).severity,
            Severity::Ok,
            "unknown boot time is not a long uptime"
        );
    }

    #[test]
    fn findings_are_ordered_by_severity_then_by_impact() {
        let mut s = healthy();
        s.boot_unix = Some(NOW - 30. * 86_400.); // notice, last in impact
        s.low_power = true; // notice
        s.mem.swap_used = 2 * GIB; // notice, first in impact
        s.thermal_level = Some(2); // warning
        s.disk.free = GIB; // warning
        let findings = diagnose(&s);
        let severities: Vec<Severity> = findings.iter().map(|f| f.severity).collect();
        assert_eq!(
            severities,
            [
                Severity::Warning,
                Severity::Warning,
                Severity::Notice,
                Severity::Notice,
                Severity::Notice
            ]
        );
        let titles: Vec<&str> = findings.iter().map(|f| f.title).collect();
        assert_eq!(
            titles,
            [
                l(
                    "The Mac is slowing itself down to cool off",
                    "Macen bremser seg selv for å kjøle seg ned"
                ),
                l(
                    "The startup disk is nearly full",
                    "Oppstartsdisken er nesten full"
                ),
                l(
                    "The Mac is using the disk as extra memory (swap)",
                    "Macen bruker disken som ekstra minne (swap)"
                ),
                l("Low Power Mode is on", "Lavstrømmodus er på"),
                l("A restart can help", "En omstart kan hjelpe"),
            ]
        );
        assert!(
            findings.iter().all(|f| f.severity != Severity::Ok),
            "no all-clear beside real findings"
        );
    }
}
