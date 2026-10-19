//! Menu-bar style rules that need no AppKit: which colour a level earns and
//! where the per-core bars sit. `statusbar.rs` draws what this decides.

/// The warning colour a reading earns: orange at caution, red at critical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Caution,
    Critical,
}

/// Memory is tinted while macOS reports warning or critical pressure.
pub fn memory_tint(pressure: Option<crate::metrics::Pressure>) -> Option<Tint> {
    match pressure? {
        crate::metrics::Pressure::Normal => None,
        crate::metrics::Pressure::Warning => Some(Tint::Caution),
        crate::metrics::Pressure::Critical => Some(Tint::Critical),
    }
}

/// CPU turns orange above 80 % and red above 95 %.
pub fn cpu_tint(pct: f64) -> Option<Tint> {
    if pct > 95. {
        Some(Tint::Critical)
    } else if pct > 80. {
        Some(Tint::Caution)
    } else {
        None
    }
}

/// A reading that runs out (battery charge, free disk space) turns orange
/// below 20 % and red below 10 %. A battery that is charging is not in
/// trouble, so `charging` keeps it plain.
pub fn remaining_tint(pct: f64, charging: bool) -> Option<Tint> {
    if charging || !pct.is_finite() {
        None
    } else if pct < 10. {
        Some(Tint::Critical)
    } else if pct < 20. {
        Some(Tint::Caution)
    } else {
        None
    }
}

/// A whole percentage, 0 to 100: what a ring or gauge can show.
pub fn whole_pct(pct: f64) -> u8 {
    if pct.is_finite() {
        pct.round().clamp(0., 100.) as u8
    } else {
        0
    }
}

/// Tallest a per-core bar grows, in pixels.
pub const BAR_MAX_HEIGHT: f64 = 12.;
/// A core's bar height in whole pixels, at least 1 so an idle core still
/// shows. Whole pixels are all the bar can show, so they are also what
/// decides whether the image needs redrawing.
pub fn bar_height(pct: f64) -> u8 {
    let pct = if pct.is_finite() {
        pct.clamp(0., 100.)
    } else {
        0.
    };
    (pct / 100. * BAR_MAX_HEIGHT).round().max(1.) as u8
}

const BAR_GAP: f64 = 1.;
const GROUP_GAP: f64 = 3.;
const MAX_BARS_WIDTH: f64 = 72.;

/// Where each core's bar starts, and how wide the bars are together.
#[derive(Clone, Debug, PartialEq)]
pub struct BarLayout {
    pub bar_width: f64,
    pub xs: Vec<f64>,
    pub width: f64,
}

/// Lay out `cores` bars with a gap between core types. 2 px bars with 1 px
/// between them; a chip with so many cores that this passes 72 px gets 1 px
/// bars. Groups that don't add up to `cores` are ignored: one group of all.
pub fn bar_layout(groups: &[usize], cores: usize) -> BarLayout {
    let single = [cores];
    let groups = if groups.iter().sum::<usize>() == cores && groups.iter().all(|g| *g > 0) {
        groups
    } else {
        &single
    };
    let place = |bar_width: f64| {
        let mut xs = Vec::with_capacity(cores);
        let mut x = 0.;
        for (i, group) in groups.iter().enumerate() {
            if i > 0 {
                x += GROUP_GAP - BAR_GAP;
            }
            for _ in 0..*group {
                xs.push(x);
                x += bar_width + BAR_GAP;
            }
        }
        let width = (x - BAR_GAP).max(0.);
        BarLayout {
            bar_width,
            xs,
            width,
        }
    };
    let wide = place(2.);
    if wide.width > MAX_BARS_WIDTH {
        place(1.)
    } else {
        wide
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pressure;

    #[test]
    fn memory_is_tinted_only_under_pressure() {
        assert_eq!(memory_tint(None), None, "an unknown level is not tinted");
        assert_eq!(memory_tint(Some(Pressure::Normal)), None);
        assert_eq!(memory_tint(Some(Pressure::Warning)), Some(Tint::Caution));
        assert_eq!(memory_tint(Some(Pressure::Critical)), Some(Tint::Critical));
    }

    #[test]
    fn cpu_turns_orange_above_80_and_red_above_95() {
        assert_eq!(cpu_tint(80.), None);
        assert_eq!(cpu_tint(80.1), Some(Tint::Caution));
        assert_eq!(cpu_tint(95.), Some(Tint::Caution));
        assert_eq!(cpu_tint(95.1), Some(Tint::Critical));
        assert_eq!(cpu_tint(0.), None);
    }

    #[test]
    fn running_out_turns_orange_below_20_and_red_below_10_unless_charging() {
        assert_eq!(remaining_tint(20., false), None);
        assert_eq!(remaining_tint(19.9, false), Some(Tint::Caution));
        assert_eq!(remaining_tint(10., false), Some(Tint::Caution));
        assert_eq!(remaining_tint(9.9, false), Some(Tint::Critical));
        assert_eq!(remaining_tint(5., true), None, "a charging battery is fine");
        assert_eq!(remaining_tint(f64::NAN, false), None);
    }

    #[test]
    fn rings_and_bars_round_to_what_can_be_drawn() {
        assert_eq!(whole_pct(49.6), 50);
        assert_eq!(whole_pct(140.), 100);
        assert_eq!(whole_pct(-3.), 0);
        assert_eq!(whole_pct(f64::NAN), 0);
        assert_eq!(bar_height(0.), 1, "an idle core still shows");
        assert_eq!(bar_height(100.), 12);
        assert_eq!(bar_height(50.), 6);
        assert_eq!(bar_height(f64::INFINITY), 1);
        assert_eq!(
            bar_height(50.),
            bar_height(51.),
            "under a pixel apart is the same bar"
        );
    }

    #[test]
    fn bars_are_2_px_with_1_px_between_and_a_wider_gap_between_core_types() {
        let one = bar_layout(&[], 8);
        assert_eq!((one.bar_width, one.width), (2., 8. * 2. + 7.));
        assert_eq!(&one.xs[..3], &[0., 3., 6.]);
        // 6 Super + 12 Performance: 18 bars, 16 one-pixel gaps, one 3 px gap.
        let two = bar_layout(&[6, 12], 18);
        assert_eq!(two.xs.len(), 18);
        assert_eq!(two.width, 18. * 2. + 16. + 3.);
        assert_eq!(two.xs[6] - two.xs[5], 2. + 3., "the gap between core types");
        assert_eq!(two.xs[1] - two.xs[0], 3.);
    }

    #[test]
    fn many_cores_get_1_px_bars_and_bad_groups_are_ignored() {
        let big = bar_layout(&[16, 16], 32);
        assert_eq!(big.bar_width, 1.);
        assert!(big.width <= 72.);
        assert_eq!(
            bar_layout(&[4, 4], 10),
            bar_layout(&[], 10),
            "groups that don't add up"
        );
        assert_eq!(
            bar_layout(&[0, 10], 10),
            bar_layout(&[], 10),
            "an empty group"
        );
        let none = bar_layout(&[], 0);
        assert_eq!((none.width, none.xs.len()), (0., 0));
    }
}
