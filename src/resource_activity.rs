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
    pub covered: f64,
    pub own: bool,
}

struct Interval {
    start: f64,
    end: f64,
    cpu: f64,
    io: f64,
}
struct Observed {
    process: ProcInfo,
    at: f64,
    wall: f64,
    intervals: VecDeque<Interval>,
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
                }
            }
            while intervals.front().is_some_and(|i| i.end <= at - 60.) {
                intervals.pop_front();
            }
            // Bound memory even if a caller samples much faster than the normal 2-second cadence.
            while intervals.len() > 120 {
                intervals.pop_front();
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
            rows.push(Activity {
                pid: p.pid,
                name: p.comm.clone(),
                path: p.exe_path.clone(),
                cpu: (covered > 0.).then(|| cpu / covered),
                io: (covered > 0.).then(|| io / covered),
                covered,
                own: p.pid == std::process::id() as i32,
            });
            self.previous.insert(
                p.pid,
                Observed {
                    process: p.clone(),
                    at,
                    wall,
                    intervals,
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
}
