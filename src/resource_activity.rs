//! Recent observed process activity, never an energy or battery-drain score.
use crate::processes::ProcInfo;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug)]
pub struct Activity {
    pub pid: i32,
    pub name: String,
    pub path: String,
    pub cpu: Option<f64>,
    pub io: Option<f64>,
    pub interrupt_wakeups: Option<f64>,
    pub covered: f64,
    pub wakeups_covered: f64,
    pub own: bool,
}

struct Interval {
    start: f64,
    end: f64,
    cpu: f64,
    io: f64,
}
struct WakeupInterval {
    start: f64,
    end: f64,
    rate: f64,
}
struct Observed {
    process: ProcInfo,
    at: f64,
    wall: f64,
    intervals: VecDeque<Interval>,
    wakeups: VecDeque<WakeupInterval>,
}

#[derive(Default)]
pub struct Tracker {
    previous: HashMap<i32, Observed>,
}

impl Tracker {
    pub fn sample(&mut self, procs: &[ProcInfo], at: f64, wall: f64) -> Vec<Activity> {
        let mut previous = std::mem::take(&mut self.previous);
        let mut rows = Vec::new();
        for p in procs.iter().take(8192) {
            let mut intervals = VecDeque::new();
            let mut wakeups = VecDeque::new();
            if let Some(old) = previous.remove(&p.pid) {
                let dt = at - old.at;
                let wall_dt = wall - old.wall;
                if p.start_unix > 0.
                    && p.start_unix == old.process.start_unix
                    && p.exe_path == old.process.exe_path
                    && dt > 0.
                    && dt <= 10.
                    && wall_dt > 0.
                    && wall_dt <= 10.
                    && (wall_dt - dt).abs() <= 1.
                    && p.cpu_ns >= old.process.cpu_ns
                    && p.disk_read >= old.process.disk_read
                    && p.disk_write >= old.process.disk_write
                {
                    let cpu = (p.cpu_ns - old.process.cpu_ns) as f64 / dt / 1e9 * 100.;
                    let io = ((p.disk_read - old.process.disk_read) as f64
                        + (p.disk_write - old.process.disk_write) as f64)
                        / dt;
                    intervals = old.intervals;
                    intervals.push_back(Interval {
                        start: old.at,
                        end: at,
                        cpu,
                        io,
                    });
                    if let Some(delta) = p.interrupt_wakeups.and_then(|current| {
                        old.process
                            .interrupt_wakeups
                            .and_then(|before| current.checked_sub(before))
                    }) {
                        wakeups = old.wakeups;
                        wakeups.push_back(WakeupInterval {
                            start: old.at,
                            end: at,
                            rate: delta as f64 / dt,
                        });
                    }
                }
            }
            while intervals.front().is_some_and(|i| i.end <= at - 60.) {
                intervals.pop_front();
            }
            while wakeups.front().is_some_and(|i| i.end <= at - 60.) {
                wakeups.pop_front();
            }
            // Bound memory even if a caller samples much faster than the normal 2-second cadence.
            while intervals.len() > 120 {
                intervals.pop_front();
            }
            while wakeups.len() > 120 {
                wakeups.pop_front();
            }
            let mut covered = 0.;
            let mut cpu = 0.;
            let mut io = 0.;
            for i in &intervals {
                let span = i.end - i.start.max(at - 60.);
                covered += span;
                cpu += i.cpu * span;
                io += i.io * span;
            }
            let mut wakeups_covered = 0.;
            let mut interrupt_wakeups = 0.;
            for i in &wakeups {
                let span = i.end - i.start.max(at - 60.);
                wakeups_covered += span;
                interrupt_wakeups += i.rate * span;
            }
            rows.push(Activity {
                pid: p.pid,
                name: p.comm.clone(),
                path: p.exe_path.clone(),
                cpu: (covered > 0.).then(|| cpu / covered),
                io: (covered > 0.).then(|| io / covered),
                interrupt_wakeups: (wakeups_covered > 0.)
                    .then(|| interrupt_wakeups / wakeups_covered),
                covered,
                wakeups_covered,
                own: p.pid == std::process::id() as i32,
            });
            self.previous.insert(
                p.pid,
                Observed {
                    process: p.clone(),
                    at,
                    wall,
                    intervals,
                    wakeups,
                },
            );
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn proc(cpu: u64, io: u64) -> ProcInfo {
        ProcInfo {
            pid: 123,
            start_unix: 1.,
            cpu_ns: cpu,
            disk_read: io,
            ..Default::default()
        }
    }
    #[test]
    fn recent_rates_do_not_use_lifetime_or_survive_identity_reset() {
        let mut t = Tracker::default();
        assert_eq!(t.sample(&[proc(1_000_000_000, 100)], 0., 100.)[0].cpu, None);
        let rows = t.sample(&[proc(2_000_000_000, 300)], 2., 102.);
        assert_eq!(rows[0].cpu, Some(50.));
        assert_eq!(rows[0].io, Some(100.));
        let mut p = proc(3_000_000_000, 400);
        p.start_unix = 2.;
        assert_eq!(t.sample(&[p], 4., 104.)[0].cpu, None);
    }
    #[test]
    fn resets_gaps_sleep_and_missing_processes_clear_evidence() {
        let mut t = Tracker::default();
        t.sample(&[proc(100, 100)], 0., 100.);
        assert_eq!(t.sample(&[proc(50, 50)], 2., 102.)[0].cpu, None);
        assert_eq!(t.sample(&[proc(100, 100)], 4., 202.)[0].cpu, None);
        t.sample(&[], 6., 204.);
        assert_eq!(t.sample(&[proc(200, 200)], 8., 206.)[0].cpu, None);
    }
    #[test]
    fn only_last_sixty_seconds_contribute() {
        let mut t = Tracker::default();
        for i in 0..=40 {
            t.sample(
                &[proc(i * 1_000_000_000, i * 200)],
                i as f64 * 2.,
                100. + i as f64 * 2.,
            );
        }
        let r = t.sample(&[proc(41_000_000_000, 8200)], 82., 182.);
        assert_eq!(r[0].covered, 60.);
        assert_eq!(r[0].cpu, Some(50.));
    }

    fn waking(cpu: u64, count: Option<u64>) -> ProcInfo {
        ProcInfo {
            interrupt_wakeups: count,
            ..proc(cpu, 0)
        }
    }

    #[test]
    fn interrupt_rates_are_time_weighted_not_lifetime_estimates() {
        let mut t = Tracker::default();
        let first = t.sample(&[waking(0, Some(1000))], 0., 100.);
        assert_eq!(first[0].interrupt_wakeups, None);
        let second = t.sample(&[waking(1_000_000_000, Some(1090))], 2., 102.);
        assert_eq!(second[0].interrupt_wakeups, Some(45.));
        assert_eq!(second[0].wakeups_covered, 2.);
        let third = t.sample(&[waking(2_000_000_000, Some(1150))], 5., 105.);
        assert_eq!(third[0].interrupt_wakeups, Some(30.));
        assert_eq!(third[0].wakeups_covered, 5.);
    }

    #[test]
    fn interrupt_reset_and_missing_counter_clear_only_wakeup_evidence() {
        let mut t = Tracker::default();
        t.sample(&[waking(0, Some(1000))], 0., 100.);
        t.sample(&[waking(1_000_000_000, Some(1100))], 2., 102.);
        let reset = t.sample(&[waking(2_000_000_000, Some(5))], 4., 104.);
        assert_eq!(reset[0].interrupt_wakeups, None);
        assert_eq!(reset[0].wakeups_covered, 0.);
        assert_eq!(reset[0].cpu, Some(50.));
        t.sample(&[waking(3_000_000_000, None)], 6., 106.);
        assert_eq!(
            t.sample(&[waking(4_000_000_000, Some(7))], 8., 108.)[0].interrupt_wakeups,
            None
        );
        assert_eq!(
            t.sample(&[waking(5_000_000_000, Some(27))], 10., 110.)[0].interrupt_wakeups,
            Some(10.)
        );
    }

    #[test]
    fn interrupt_identity_sleep_and_clock_gaps_clear_evidence() {
        let mut t = Tracker::default();
        t.sample(&[waking(0, Some(0))], 0., 100.);
        t.sample(&[waking(1_000_000_000, Some(10))], 2., 102.);
        assert_eq!(
            t.sample(&[waking(2_000_000_000, Some(20))], 4., 202.)[0].interrupt_wakeups,
            None
        );
        let mut reused = waking(3_000_000_000, Some(30));
        reused.start_unix = 2.;
        assert_eq!(t.sample(&[reused], 6., 204.)[0].interrupt_wakeups, None);
        t.sample(&[], 8., 206.);
        assert_eq!(
            t.sample(&[waking(4_000_000_000, Some(40))], 10., 208.)[0].interrupt_wakeups,
            None
        );
    }

    #[test]
    fn interrupt_window_is_bounded_and_real_zero_is_preserved() {
        let mut t = Tracker::default();
        for i in 0..=40 {
            t.sample(
                &[waking(i * 1_000_000_000, Some(i * 10))],
                i as f64 * 2.,
                100. + i as f64 * 2.,
            );
        }
        let rows = t.sample(&[waking(41_000_000_000, Some(410))], 82., 182.);
        assert_eq!(rows[0].wakeups_covered, 60.);
        assert_eq!(rows[0].interrupt_wakeups, Some(5.));
        let mut zero = Tracker::default();
        zero.sample(&[waking(0, Some(0))], 0., 100.);
        assert_eq!(
            zero.sample(&[waking(1, Some(0))], 2., 102.)[0].interrupt_wakeups,
            Some(0.)
        );
        let mut unavailable = Tracker::default();
        unavailable.sample(&[waking(0, None)], 0., 100.);
        assert_eq!(
            unavailable.sample(&[waking(1, None)], 2., 102.)[0].interrupt_wakeups,
            None
        );
    }
}
