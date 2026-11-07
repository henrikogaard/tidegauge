//! The highest value each sensor has reached since Tidegauge started.
//! Nothing is stored: a restart begins again, and the CPU page says so.

use std::collections::HashMap;
use std::sync::Mutex;

/// Running maxima by sensor id. A sensor that stops reporting keeps its
/// maximum; a value that is not a finite number never counts.
#[derive(Default)]
pub struct Tracker {
    max: HashMap<String, f64>,
}

impl Tracker {
    pub fn observe(&mut self, id: &str, value: f64) {
        if !value.is_finite() {
            return;
        }
        match self.max.get_mut(id) {
            Some(m) => *m = m.max(value),
            None => {
                self.max.insert(id.to_string(), value);
            }
        }
    }

    pub fn max(&self, id: &str) -> Option<f64> {
        self.max.get(id).copied()
    }
}

static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);

pub fn named_id(kind: crate::named_sensors::Kind) -> String {
    format!("named:{kind:?}")
}

pub fn smc_id(key: &str) -> String {
    format!("smc:{key}")
}

/// Fold one published sample into the maxima: the hottest named sensor of
/// each part, every SMC temperature channel, and each fan's speed.
pub fn record(sample: &crate::sensors::SensorSnapshot) {
    let mut guard = TRACKER.lock().unwrap();
    let tracker = guard.get_or_insert_with(Tracker::default);
    for part in crate::named_sensors::summarise(&sample.named) {
        tracker.observe(&named_id(part.kind), part.hottest);
    }
    for r in sample.temperatures.iter().chain(&sample.fans) {
        if let Some(v) = r.value {
            tracker.observe(&smc_id(&r.key), v);
        }
    }
}

pub fn get(id: &str) -> Option<f64> {
    TRACKER.lock().unwrap().as_ref().and_then(|t| t.max(id))
}

#[cfg(test)]
mod tests {
    use super::Tracker;

    #[test]
    fn keeps_the_highest_value() {
        let mut t = Tracker::default();
        t.observe("a", 60.);
        t.observe("a", 71.);
        t.observe("a", 65.);
        assert_eq!(t.max("a"), Some(71.));
    }

    #[test]
    fn new_sensors_start_at_their_first_value() {
        let mut t = Tracker::default();
        t.observe("a", 40.);
        assert_eq!(t.max("b"), None);
        t.observe("b", -5.);
        assert_eq!(t.max("b"), Some(-5.), "a first reading below zero still counts");
        assert_eq!(t.max("a"), Some(40.));
    }

    #[test]
    fn a_sensor_that_disappears_keeps_its_maximum() {
        let mut t = Tracker::default();
        t.observe("a", 80.);
        t.observe("b", 50.);
        t.observe("b", 51.);
        assert_eq!(t.max("a"), Some(80.));
    }

    #[test]
    fn garbage_values_are_ignored() {
        let mut t = Tracker::default();
        t.observe("a", 50.);
        t.observe("a", f64::NAN);
        t.observe("a", f64::INFINITY);
        assert_eq!(t.max("a"), Some(50.));
        t.observe("never", f64::NAN);
        assert_eq!(t.max("never"), None);
    }

    #[test]
    fn records_named_and_smc_values_from_a_sample() {
        use crate::named_sensors::Kind;
        use crate::sensors::{Reading, SensorSnapshot};
        let sample = SensorSnapshot {
            named: vec![(Kind::Chip, 70.), (Kind::Chip, 75.)],
            temperatures: vec![Reading { key: "TZ9Z".into(), value: Some(33.) }, Reading { key: "TZ9Y".into(), value: None }],
            fans: vec![Reading { key: "FZAc".into(), value: Some(2400.) }],
            ..Default::default()
        };
        super::record(&sample);
        assert_eq!(super::get(&super::named_id(Kind::Chip)), Some(75.));
        assert_eq!(super::get(&super::smc_id("TZ9Z")), Some(33.));
        assert_eq!(super::get(&super::smc_id("TZ9Y")), None);
        assert_eq!(super::get(&super::smc_id("FZAc")), Some(2400.));
    }
}
