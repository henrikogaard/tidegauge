//! Apps whose memory keeps growing for hours: a likely leak, and a restart
//! frees it. Each app's memory is sampled once a minute into a small
//! in-memory ring and judged every five minutes, so the check costs next to
//! nothing and needs no database query. The ring starts empty with
//! Tidegauge, so a warning needs three hours of watching.

use crate::processes::AppGroup;
use std::collections::{HashMap, VecDeque};

/// How far back the climb is judged.
pub const WINDOW: f64 = 3. * 3600.;
/// One memory sample per app this often.
const SAMPLE_EVERY: f64 = 60.;
/// Judge the samples this often.
const EVALUATE_EVERY: f64 = 300.;
/// The window is split into ten-minute buckets; each needs a sample, so a
/// sleep or a gap longer than that breaks the run.
const BUCKET: f64 = 600.;
const BUCKETS: usize = (WINDOW / BUCKET) as usize;
const GIB: f64 = 1024. * 1024. * 1024.;
/// Smaller apps are not tracked at all.
const MIN_TRACKED: u64 = 64 * 1024 * 1024;
/// Memory now, and the least growth over the window. The growth must also be
/// half of where the app started.
const MIN_NOW: f64 = GIB;
const MIN_GROWTH: f64 = GIB;
const MIN_RELATIVE_GROWTH: f64 = 0.5;
/// How well a straight line has to fit the ten-minute medians (R squared).
const MIN_FIT: f64 = 0.9;
/// The most the memory may fall back from an earlier peak, as a share of the
/// total growth: a sawtooth that frees memory again is not a leak.
const MAX_DRAWDOWN: f64 = 0.25;
/// Each third of the window has to add at least this share of the growth, so
/// a climb that has flattened out is not flagged.
const MIN_THIRD: f64 = 0.15;
/// A flagged app stays flagged through this many failed judgements in a row,
/// so one borderline reading does not make the warning flicker.
const MISSES_ALLOWED: u8 = 3;

/// One app whose memory has climbed steadily over the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Leak {
    pub key: String,
    pub name: String,
    /// Bytes gained over the window (end minus start, as bucket medians).
    pub growth: f64,
}

/// Whether `samples` (time, bytes; oldest first) show a steady climb over
/// the window ending at `now`; returns the bytes gained.
pub fn detect(samples: &VecDeque<(f64, u64)>, now: f64) -> Option<f64> {
    let start = now - WINDOW;
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); BUCKETS];
    for &(at, bytes) in samples {
        let index = ((at - start) / BUCKET).floor();
        if (0. ..BUCKETS as f64).contains(&index) {
            buckets[index as usize].push(bytes as f64);
        }
    }
    let mut medians = Vec::with_capacity(BUCKETS);
    for bucket in &mut buckets {
        if bucket.is_empty() {
            return None;
        }
        bucket.sort_by(f64::total_cmp);
        medians.push(bucket[bucket.len() / 2]);
    }
    let (first, last) = (medians[0], medians[BUCKETS - 1]);
    let growth = last - first;
    let now_bytes = samples.back()?.1 as f64;
    if now_bytes < MIN_NOW || growth < MIN_GROWTH.max(first * MIN_RELATIVE_GROWTH) {
        return None;
    }
    if freed(samples, growth) || !steady(&medians, growth) {
        return None;
    }
    Some(growth)
}

/// Whether the latest sample has fallen back from the peak by more than the
/// allowed drawdown: the app has freed the memory, so it is not a leak.
fn freed(samples: &VecDeque<(f64, u64)>, growth: f64) -> bool {
    let peak = samples.iter().map(|&(_, bytes)| bytes).max().unwrap_or(0) as f64;
    let now = samples.back().map_or(0., |&(_, bytes)| bytes as f64);
    peak - now > growth * MAX_DRAWDOWN
}

fn steady(medians: &[f64], growth: f64) -> bool {
    let n = medians.len() as f64;
    let mean_x = (n - 1.) / 2.;
    let mean_y = medians.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx, mut syy) = (0., 0., 0.);
    for (i, y) in medians.iter().enumerate() {
        let (dx, dy) = (i as f64 - mean_x, y - mean_y);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxy <= 0. || syy <= 0. || sxy * sxy / (sxx * syy) < MIN_FIT {
        return false;
    }
    let mut peak = medians[0];
    for &y in medians {
        peak = peak.max(y);
        if peak - y > growth * MAX_DRAWDOWN {
            return false;
        }
    }
    let third = medians.len() / 3;
    [0, third, 2 * third, medians.len() - 1]
        .windows(2)
        .all(|pair| medians[pair[1]] - medians[pair[0]] >= growth * MIN_THIRD)
}

struct Series {
    name: String,
    pids: Vec<i32>,
    samples: VecDeque<(f64, u64)>,
    /// The current leak and how many judgements in a row have not found it.
    flag: Option<(Leak, u8)>,
}

#[derive(Default)]
pub struct Tracker {
    series: HashMap<String, Series>,
    last_sample: f64,
    last_judged: f64,
    flagged: Vec<Leak>,
}

impl Tracker {
    /// Called every tick with the app list; does work once a minute.
    pub fn update(&mut self, now: f64, apps: &[AppGroup]) {
        if !now.is_finite() {
            return;
        }
        if now < self.last_sample {
            *self = Self::default();
        }
        if now - self.last_sample < SAMPLE_EVERY {
            return;
        }
        self.last_sample = now;
        let mut seen = std::collections::HashSet::new();
        let mut relaunched = Vec::new();
        for app in apps.iter().filter(|a| a.mem_bytes >= MIN_TRACKED) {
            seen.insert(app.key.as_str());
            let series = self
                .series
                .entry(app.key.clone())
                .or_insert_with(|| Series {
                    name: String::new(),
                    pids: Vec::new(),
                    samples: VecDeque::new(),
                    flag: None,
                });
            // No process in common with a minute ago: the app was quit and
            // opened again, and its memory starts over.
            if !series.pids.is_empty() && !app.pids.iter().any(|pid| series.pids.contains(pid)) {
                series.samples.clear();
                series.flag = None;
                relaunched.push(app.key.clone());
            }
            series.name.clone_from(&app.name);
            series.pids.clone_from(&app.pids);
            series.samples.push_back((now, app.mem_bytes));
            while series
                .samples
                .front()
                .is_some_and(|(at, _)| *at < now - WINDOW - BUCKET)
            {
                series.samples.pop_front();
            }
        }
        // An app that is gone, or shrank below the floor, starts over too.
        self.series.retain(|key, _| seen.contains(key.as_str()));
        // Take them off the published list now, not at the next judgement,
        // so a fresh instance is never offered to quit as a leak.
        self.flagged
            .retain(|leak| self.series.contains_key(&leak.key) && !relaunched.contains(&leak.key));
        if now - self.last_judged >= EVALUATE_EVERY {
            self.last_judged = now;
            self.judge(now);
        }
    }

    fn judge(&mut self, now: f64) {
        for (key, series) in &mut self.series {
            match detect(&series.samples, now) {
                Some(growth) => {
                    series.flag = Some((
                        Leak {
                            key: key.clone(),
                            name: series.name.clone(),
                            growth,
                        },
                        0,
                    ));
                }
                None => {
                    series.flag = match series.flag.take() {
                        // Memory given back clears the flag at once.
                        Some((leak, _)) if freed(&series.samples, leak.growth) => None,
                        Some((leak, misses)) if misses + 1 < MISSES_ALLOWED => {
                            Some((leak, misses + 1))
                        }
                        _ => None,
                    };
                }
            }
        }
        self.flagged = self
            .series
            .values()
            .filter_map(|s| s.flag.as_ref().map(|(leak, _)| leak.clone()))
            .collect();
        self.flagged.sort_by(|a, b| {
            b.growth
                .total_cmp(&a.growth)
                .then_with(|| a.key.cmp(&b.key))
        });
    }

    /// The apps flagged now, biggest gain first.
    pub fn flagged(&self) -> &[Leak] {
        &self.flagged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_800_000_000.;

    /// One sample a minute for the whole window, `bytes` of the fraction of
    /// the window elapsed (0 to 1).
    fn series(bytes: impl Fn(f64) -> f64) -> VecDeque<(f64, u64)> {
        (0..=180)
            .map(|minute| {
                let fraction = minute as f64 / 180.;
                (NOW - WINDOW + minute as f64 * 60., bytes(fraction) as u64)
            })
            .collect()
    }

    /// A small deterministic wobble of up to +-`amount`.
    fn wobble(fraction: f64, amount: f64) -> f64 {
        (fraction * 977.).sin() * amount
    }

    #[test]
    fn a_steady_climb_is_flagged() {
        let samples = series(|f| 1.5 * GIB + 2.5 * GIB * f + wobble(f, 0.03 * GIB));
        let growth = detect(&samples, NOW).expect("flagged");
        assert!((growth / GIB - 2.5).abs() < 0.2, "{growth}");
    }

    #[test]
    fn a_sawtooth_that_frees_memory_is_not_flagged() {
        // Climbs 1 to 3 GB every 45 minutes, then collects.
        let samples = series(|f| GIB + 2. * GIB * ((f * 4.).fract()));
        assert_eq!(detect(&samples, NOW), None);
        // A few big drops on top of an overall climb.
        let samples = series(|f| {
            2. * GIB + 3. * GIB * f
                - if (0.4..0.55).contains(&f) {
                    1.5 * GIB
                } else {
                    0.
                }
        });
        assert_eq!(detect(&samples, NOW), None);
    }

    #[test]
    fn a_high_or_swinging_number_is_not_a_climb() {
        assert_eq!(detect(&series(|_| 8. * GIB), NOW), None, "big but flat");
        assert_eq!(
            detect(&series(|f| 6. * GIB + 3. * GIB * (f * 31.).sin()), NOW),
            None,
            "swinging"
        );
        assert_eq!(
            detect(&series(|f| 3. * GIB - 2. * GIB * f), NOW),
            None,
            "falling"
        );
    }

    #[test]
    fn a_climb_that_stopped_is_not_flagged() {
        // Grows for the first hour, then stays.
        let samples = series(|f| 1. * GIB + 3. * GIB * (f * 3.).min(1.));
        assert_eq!(detect(&samples, NOW), None);
    }

    #[test]
    fn growth_has_to_be_big_in_bytes_and_in_share() {
        assert_eq!(
            detect(&series(|f| 4. * GIB + 0.9 * GIB * f), NOW),
            None,
            "under 1 GB gained"
        );
        assert_eq!(
            detect(&series(|f| 4. * GIB + 1.4 * GIB * f), NOW),
            None,
            "over 1 GB but under half again"
        );
        assert!(
            detect(&series(|f| 2. * GIB + 1.1 * GIB * f), NOW).is_some(),
            "1 GB and half again"
        );
    }

    #[test]
    fn a_small_app_is_not_flagged() {
        assert_eq!(
            detect(&series(|f| 0.1 * GIB + 0.7 * GIB * f), NOW),
            None,
            "under 1 GB now"
        );
        assert_eq!(
            detect(&series(|f| 0.1 * GIB + 0.9 * GIB * f), NOW),
            None,
            "grew under 1 GB"
        );
    }

    #[test]
    fn a_short_lived_app_or_a_gap_is_not_flagged() {
        let hour: VecDeque<_> = series(|f| 1. * GIB + 4. * GIB * f)
            .into_iter()
            .filter(|(at, _)| *at > NOW - 3600.)
            .collect();
        assert_eq!(detect(&hour, NOW), None, "only an hour old");
        let gap: VecDeque<_> = series(|f| 1. * GIB + 4. * GIB * f)
            .into_iter()
            .filter(|(at, _)| !(NOW - 7200. ..NOW - 5000.).contains(at))
            .collect();
        assert_eq!(
            detect(&gap, NOW),
            None,
            "a sleep in the middle breaks the run"
        );
        assert_eq!(detect(&VecDeque::new(), NOW), None);
    }

    fn app(key: &str, pids: &[i32], gib: f64) -> AppGroup {
        AppGroup {
            key: key.into(),
            name: key.into(),
            pids: pids.to_vec(),
            mem_bytes: (gib * GIB) as u64,
            ..Default::default()
        }
    }

    /// Run a tracker once a minute for the window and a bit more, the app
    /// growing from 1.5 to 4 GB.
    fn climbing(tracker: &mut Tracker, from: usize, to: usize, pids: &[i32]) {
        for minute in from..=to {
            let gib = 1.5 + 2.5 * minute as f64 / 180.;
            tracker.update(
                NOW - WINDOW + minute as f64 * 60.,
                &[
                    app("com.example.leaky", pids, gib),
                    app("com.example.flat", &[9], 2.),
                ],
            );
        }
    }

    #[test]
    fn the_tracker_flags_after_the_window_and_only_the_climbing_app() {
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 120, &[1, 2]);
        assert!(tracker.flagged().is_empty(), "two hours is not enough");
        climbing(&mut tracker, 121, 185, &[1, 2]);
        let flagged = tracker.flagged();
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].key, "com.example.leaky");
        assert!(flagged[0].growth > GIB);
    }

    #[test]
    fn a_relaunch_starts_the_run_over() {
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 150, &[1, 2]);
        // Quit and opened again at minute 151: new processes.
        climbing(&mut tracker, 151, 185, &[7, 8]);
        assert!(
            tracker.flagged().is_empty(),
            "the new run is half an hour old"
        );
        // Helpers coming and going while the main process stays is not a relaunch.
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 90, &[1, 2]);
        climbing(&mut tracker, 91, 185, &[1, 3]);
        assert_eq!(tracker.flagged().len(), 1);
    }

    #[test]
    fn a_flagged_app_survives_a_borderline_judgement_but_not_a_long_miss() {
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 185, &[1]);
        assert_eq!(tracker.flagged().len(), 1);
        // Its memory levels off at the top: the climb no longer passes, but
        // the flag holds for two more judgements and clears later.
        let mut at = NOW - WINDOW + 185. * 60.;
        let mut judged_flags = Vec::new();
        for _ in 0..120 {
            at += 60.;
            tracker.update(at, &[app("com.example.leaky", &[1], 4.05)]);
            judged_flags.push(tracker.flagged().len());
        }
        assert_eq!(judged_flags[0], 1);
        assert_eq!(*judged_flags.last().unwrap(), 0);
    }

    #[test]
    fn memory_given_back_clears_the_flag_at_the_next_judgement() {
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 185, &[1]);
        assert_eq!(tracker.flagged().len(), 1);
        let mut at = NOW - WINDOW + 185. * 60.;
        for _ in 0..5 {
            at += 60.;
            tracker.update(at, &[app("com.example.leaky", &[1], 1.)]);
        }
        assert!(tracker.flagged().is_empty());
    }

    #[test]
    fn a_climb_that_ends_in_a_big_drop_is_not_flagged() {
        let mut samples = series(|f| 1.5 * GIB + 2.5 * GIB * f);
        samples.back_mut().unwrap().1 = GIB as u64;
        assert_eq!(detect(&samples, NOW), None);
    }

    #[test]
    fn a_relaunch_clears_the_published_flag_at_once() {
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 185, &[1, 2]);
        assert_eq!(tracker.flagged().len(), 1);
        // One minute later, new processes: no judgement is due yet.
        tracker.update(
            NOW - WINDOW + 186. * 60.,
            &[app("com.example.leaky", &[7], 1.5)],
        );
        assert!(tracker.flagged().is_empty());
    }

    #[test]
    fn apps_that_quit_or_shrink_are_forgotten_and_a_backwards_clock_resets() {
        let mut tracker = Tracker::default();
        climbing(&mut tracker, 0, 185, &[1]);
        tracker.update(NOW + 3600., &[]);
        assert!(tracker.series.is_empty());
        climbing(&mut tracker, 0, 10, &[1]);
        assert!(!tracker.series.is_empty());
        tracker.update(NOW - 10. * WINDOW, &[app("x", &[1], 1.)]);
        assert_eq!(tracker.series.len(), 1, "the earlier readings are dropped");
        tracker.update(f64::NAN, &[]);
    }

    #[test]
    fn the_tracker_samples_once_a_minute() {
        let mut tracker = Tracker::default();
        for second in 0..600 {
            tracker.update(NOW + second as f64, &[app("a", &[1], 1.)]);
        }
        assert_eq!(tracker.series["a"].samples.len(), 10);
    }
}
