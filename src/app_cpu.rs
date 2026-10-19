//! How an app's CPU is written. The kernel counts one full core as 100 %, so
//! an app on 18 cores can read 163 %. By default that is shown as a share of
//! the whole Mac (9 %); Settings can switch every list to the per-core figure
//! Activity Monitor uses. Sorting always uses the raw figure.

use crate::locale::text as l;

#[derive(Clone, Copy)]
pub struct AppCpu {
    cores: usize,
    per_core: bool,
}

impl AppCpu {
    pub fn new(cores: usize, per_core: bool) -> Self {
        AppCpu { cores, per_core }
    }

    /// The unit the user chose in Settings, for a Mac with `cores` cores.
    pub fn chosen(cores: usize) -> Self {
        Self::new(cores, crate::settings::Settings::load().app_cpu_per_core)
    }

    /// `raw` (100 = one full core) in the chosen unit.
    pub fn value(self, raw: f64) -> f64 {
        if !raw.is_finite() || raw < 0. {
            return 0.;
        }
        if self.per_core {
            raw
        } else {
            raw / self.cores.max(1) as f64
        }
    }

    /// "<1%" for almost nothing, one decimal below 10, whole numbers above.
    pub fn format(self, raw: f64) -> String {
        Self::display(self.value(raw))
    }

    /// `format` for a value already in the chosen unit, as charts hold them.
    pub fn display(v: f64) -> String {
        if v < 0.5 {
            format!("<{}", crate::locale::percent(1., 0))
        } else if v < 9.95 {
            crate::locale::percent(v, 1)
        } else {
            crate::locale::percent(v, 0)
        }
    }

    /// The unit on its own, for a chart's heading.
    pub fn unit(self) -> &'static str {
        if self.per_core {
            l("% of one core", "% av én kjerne")
        } else {
            l("% of whole Mac", "% av hele Macen")
        }
    }

    /// The Apps & processes column header, which names the unit.
    pub fn column_header(self) -> &'static str {
        if self.per_core {
            l("CPU · PER CORE", "CPU · PER KJERNE")
        } else {
            l("CPU · OF MAC", "CPU · AV MAC")
        }
    }

    /// What the percentages mean, for the page's notes.
    pub fn explanation(self) -> &'static str {
        if self.per_core {
            l(
                "CPU: 100% is one full core.",
                "CPU: 100 % er én hel kjerne.",
            )
        } else {
            l("CPU: share of the whole Mac.", "CPU: andel av hele Macen.")
        }
    }

    /// A short qualifier for a list's section header.
    pub fn qualifier(self) -> &'static str {
        if self.per_core {
            l("CPU · 100% per core", "CPU · 100 % per kjerne")
        } else {
            l("CPU · share of the whole Mac", "CPU · andel av hele Macen")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pct(v: f64, decimals: usize) -> String {
        crate::locale::percent(v, decimals)
    }

    #[test]
    fn the_whole_mac_divides_by_the_cores_and_one_core_does_not() {
        let mac = AppCpu::new(18, false);
        let core = AppCpu::new(18, true);
        assert!((mac.value(163.) - 163. / 18.).abs() < 1e-9);
        assert_eq!(core.value(163.), 163.);
        assert_eq!(mac.format(163.), pct(9.1, 1));
        assert_eq!(mac.format(1800.), pct(100., 0));
        assert_eq!(core.format(163.), pct(163., 0));
        assert_eq!(
            AppCpu::new(0, false).value(50.),
            50.,
            "an unknown core count never divides by zero"
        );
    }

    #[test]
    fn almost_nothing_reads_under_one_percent_never_zero() {
        let mac = AppCpu::new(10, false);
        let expected = format!("<{}", pct(1., 0));
        assert_eq!(mac.format(0.), expected);
        assert_eq!(mac.format(4.9), expected, "0.49 % of the Mac");
        assert_eq!(mac.format(5.0), pct(0.5, 1), "0.5 % of the Mac is shown");
        let core = AppCpu::new(10, true);
        assert_eq!(core.format(0.49), expected);
        assert_eq!(core.format(f64::NAN), expected);
        assert_eq!(core.format(-3.), expected);
    }

    #[test]
    fn one_decimal_below_ten_and_whole_numbers_from_there() {
        let core = AppCpu::new(10, true);
        assert_eq!(core.format(1.), pct(1., 1));
        assert_eq!(core.format(9.94), pct(9.9, 1));
        assert_eq!(core.format(9.96), pct(10., 0), "never 10.0");
        assert_eq!(core.format(12.4), pct(12., 0));
        assert_eq!(core.format(380.), pct(380., 0));
    }

    #[test]
    fn charts_hold_converted_values_and_format_them_alike() {
        let mac = AppCpu::new(18, false);
        let converted = mac.value(163.);
        assert_eq!(AppCpu::display(converted), mac.format(163.));
        assert_eq!(
            mac.value(100.),
            100. / 18.,
            "one core, as a chart's lowest top"
        );
        assert_eq!(AppCpu::new(18, true).value(100.), 100.);
    }

    #[test]
    fn the_unit_is_named_in_both_modes() {
        assert_ne!(AppCpu::new(8, false).unit(), AppCpu::new(8, true).unit());
        assert_ne!(
            AppCpu::new(8, false).column_header(),
            AppCpu::new(8, true).column_header()
        );
        assert_ne!(
            AppCpu::new(8, false).explanation(),
            AppCpu::new(8, true).explanation()
        );
        assert_ne!(
            AppCpu::new(8, false).qualifier(),
            AppCpu::new(8, true).qualifier()
        );
    }
}
