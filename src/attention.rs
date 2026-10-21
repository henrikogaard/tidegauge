//! "Needs attention": what is wrong with the Mac right now, worst first, for
//! the strip at the top of Overview. It reads the latest snapshot and the disk
//! inventory only, reuses the thresholds of "Why is my Mac slow?", and says
//! nothing when all is well. A reading that is missing is never an issue.

use crate::diagnose::{startup_disk_low, throttled, CHIP_HOT_C};
use crate::disks::{Device, Health};
use crate::engine::{fmt_celsius, fmt_pct, Snapshot};
use crate::locale::text as l;
use crate::metrics::Pressure;
use crate::theme as t;
use crate::tools_panel::Page;
use crate::ui::{navigate, Destination, Tab, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// How many issues the strip lists.
pub const MAX_ISSUES: usize = 3;
/// Capacity health (full charge against design) below which the battery is
/// worth a mention.
pub const BATTERY_HEALTH_LOW_PCT: f64 = 80.;
/// macOS thermal state 3 is "critical"; 2 ("serious") is already throttling.
const THERMAL_CRITICAL: u8 = 3;

/// Worst first; the derived order puts `Critical` before `Warning`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Critical,
    Warning,
}

/// What kind of problem, which also fixes the order among equal severities.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    DiskFailing,
    Thermal,
    MemoryPressure,
    DiskFull,
    GrowingMemory,
    Battery,
}

#[derive(Clone, PartialEq)]
pub struct Issue {
    pub kind: Kind,
    pub severity: Severity,
    pub icon: &'static str,
    /// One plain line, already in the user's language.
    pub text: String,
    /// The page that explains it.
    pub to: Destination,
}

/// Up to [`MAX_ISSUES`] problems, worst first. Empty when nothing is wrong.
pub fn attention(s: &Snapshot) -> Vec<Issue> {
    attention_with(s, &crate::disks::inventory().devices)
}

fn attention_with(s: &Snapshot, devices: &[Device]) -> Vec<Issue> {
    let mut issues: Vec<Issue> = [
        disk_failing(devices),
        thermal(s),
        memory_pressure(s),
        disk_full(s),
        growing_memory(s),
        battery(s),
    ]
    .into_iter()
    .flatten()
    .collect();
    issues.sort_by_key(|i| (i.severity, i.kind));
    issues.truncate(MAX_ISSUES);
    issues
}

fn disk_failing(devices: &[Device]) -> Option<Issue> {
    let device = devices
        .iter()
        .find(|d| d.internal == Some(true) && d.smart.health == Health::Failed)?;
    let name = if device.name.is_empty() {
        l("A disk", "En disk")
    } else {
        device.name.as_str()
    };
    Some(Issue {
        kind: Kind::DiskFailing,
        severity: Severity::Critical,
        icon: "icons/hard-drive.svg",
        text: format!(
            "{name} {}",
            l(
                "reports a failing health status: back up your data",
                "melder dårlig helse: ta sikkerhetskopi av dataene"
            )
        ),
        to: Destination::Tab(Tab::Disk),
    })
}

fn thermal(s: &Snapshot) -> Option<Issue> {
    let critical = s
        .thermal_level
        .is_some_and(|level| level >= THERMAL_CRITICAL);
    let chip_hot = s.temps.chip.filter(|c| *c >= CHIP_HOT_C);
    let text = if throttled(s) {
        l(
            "The Mac is too hot and slowing itself down",
            "Macen er for varm og bremser seg selv",
        )
        .to_string()
    } else {
        format!(
            "{} {}",
            l("The chip is very hot:", "Brikken er veldig varm:"),
            fmt_celsius(chip_hot?)
        )
    };
    Some(Issue {
        kind: Kind::Thermal,
        severity: if critical {
            Severity::Critical
        } else {
            Severity::Warning
        },
        icon: "icons/thermometer.svg",
        text,
        to: Destination::Tab(Tab::Cpu),
    })
}

fn memory_pressure(s: &Snapshot) -> Option<Issue> {
    (s.mem.pressure == Some(Pressure::Critical)).then(|| Issue {
        kind: Kind::MemoryPressure,
        severity: Severity::Critical,
        icon: "icons/memory-stick.svg",
        text: l("Memory pressure is critical", "Minnetrykket er kritisk").to_string(),
        to: Destination::Tab(Tab::Memory),
    })
}

fn disk_full(s: &Snapshot) -> Option<Issue> {
    startup_disk_low(s).then(|| Issue {
        kind: Kind::DiskFull,
        severity: Severity::Warning,
        icon: "icons/hard-drive.svg",
        text: l(
            "The startup disk is nearly full",
            "Oppstartsdisken er nesten full",
        )
        .to_string(),
        to: Destination::Tool(Page::Storage),
    })
}

fn growing_memory(s: &Snapshot) -> Option<Issue> {
    let leak = s
        .leaks
        .iter()
        .max_by(|a, b| a.growth.total_cmp(&b.growth))?;
    Some(Issue {
        kind: Kind::GrowingMemory,
        severity: Severity::Warning,
        icon: "icons/memory-stick.svg",
        text: format!(
            "{} {}",
            leak.name,
            l("keeps using more memory", "bruker stadig mer minne")
        ),
        to: Destination::Tab(Tab::Memory),
    })
}

fn battery(s: &Snapshot) -> Option<Issue> {
    let b = &s.battery;
    if !b.present {
        return None;
    }
    let service = matches!(
        b.condition.as_deref(),
        Some("Poor" | "Check Battery" | "Permanent Battery Failure")
    );
    let text = if service {
        l(
            "Battery service is recommended",
            "Batteriet bør til service",
        )
        .to_string()
    } else {
        let health = b.health.filter(|h| *h < BATTERY_HEALTH_LOW_PCT)?;
        format!(
            "{} {}",
            l(
                "Battery capacity is down to",
                "Batterikapasiteten er nede på"
            ),
            fmt_pct(health)
        )
    };
    Some(Issue {
        kind: Kind::Battery,
        severity: Severity::Warning,
        icon: "icons/battery-charging.svg",
        text,
        to: Destination::Tab(Tab::Battery),
    })
}

/// The strip itself: one clickable line per issue, or nothing at all.
pub fn strip(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Option<Div> {
    let issues = attention(s);
    if issues.is_empty() {
        return None;
    }
    let mut strip = div()
        .flex()
        .flex_col()
        .rounded(t::RADIUS_CARD)
        .border_1()
        .border_color(t::line(dark))
        .bg(t::window_card(dark))
        .overflow_hidden();
    for (index, issue) in issues.into_iter().enumerate() {
        let color = match issue.severity {
            Severity::Critical => t::warn(dark),
            Severity::Warning => t::caution(dark),
        };
        let (target, keyboard, to) = (this.clone(), this.clone(), issue.to);
        strip = strip.child(
            div()
                .id(SharedString::from(format!("attention-{index}")))
                .role(gpui_kit::Role::Button)
                .aria_label(SharedString::from(issue.text.clone()))
                .tab_index(0)
                .flex()
                .items_center()
                .gap(t::SPACE_2)
                .min_h(t::ROW_H)
                .px(t::SPACE_3)
                .when(index > 0, |d| d.border_t_1().border_color(t::line(dark)))
                .cursor_pointer()
                .hover(|d| d.bg(t::card_hi(dark)))
                .focus_visible(move |d| d.border_color(t::accent(dark)))
                .child(
                    svg()
                        .path(issue.icon)
                        .size(t::SPACE_4)
                        .flex_shrink_0()
                        .text_color(color),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(t::TEXT_ROW)
                        .text_color(t::ink(dark))
                        .child(issue.text),
                )
                .on_click(move |_, _, cx| navigate(&target, to, cx))
                .capture_key_down(move |event, window, cx| {
                    if crate::controls::is_activation_key(&event.keystroke) {
                        window.prevent_default();
                        navigate(&keyboard, to, cx);
                        cx.stop_propagation();
                    }
                }),
        );
    }
    Some(strip)
}

#[cfg(test)]
mod tests {
    use super::*;
    // The gpui glob in `super` brings its own `test` macro; keep the std one.
    use crate::battery::BatteryStats;
    use crate::disks::Smart;
    use crate::leaks::Leak;
    use core::prelude::v1::test;

    const GIB: u64 = 1 << 30;

    fn healthy() -> Snapshot {
        let mut s = Snapshot::default();
        s.disk.total = 500 * GIB;
        s.disk.free = 200 * GIB;
        s.thermal_level = Some(0);
        s.mem.pressure = Some(Pressure::Normal);
        s.battery = BatteryStats {
            available: true,
            present: true,
            condition: Some("Good".into()),
            health: Some(92.),
            ..Default::default()
        };
        s
    }

    fn disk(internal: bool, health: Health) -> Device {
        Device {
            id: "disk0".into(),
            name: "Apple SSD".into(),
            internal: Some(internal),
            smart: Smart {
                health,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn kinds(s: &Snapshot, devices: &[Device]) -> Vec<Kind> {
        attention_with(s, devices).iter().map(|i| i.kind).collect()
    }

    #[test]
    fn nothing_is_shown_when_all_is_fine_or_unknown() {
        assert!(attention_with(&healthy(), &[disk(true, Health::Passed)]).is_empty());
        // Missing readings are skipped, never reported.
        assert!(attention_with(&Snapshot::default(), &[]).is_empty());
        let unknown = [
            disk(true, Health::Unavailable),
            disk(true, Health::Unsupported),
        ];
        assert!(attention_with(&healthy(), &unknown).is_empty());
    }

    #[test]
    fn a_failing_internal_disk_goes_to_disk() {
        let issues = attention_with(&healthy(), &[disk(true, Health::Failed)]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Critical);
        assert!(issues[0].to == Destination::Tab(Tab::Disk));
        assert!(issues[0].text.starts_with("Apple SSD"));
        // An external drive is not the Mac's own health.
        assert!(attention_with(&healthy(), &[disk(false, Health::Failed)]).is_empty());
    }

    #[test]
    fn thermal_state_or_a_very_hot_chip_goes_to_cpu() {
        let mut s = healthy();
        s.thermal_level = Some(1);
        assert!(kinds(&s, &[]).is_empty());
        s.thermal_level = Some(2);
        let issues = attention_with(&s, &[]);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert!(issues[0].to == Destination::Tab(Tab::Cpu));
        s.thermal_level = Some(3);
        assert_eq!(attention_with(&s, &[])[0].severity, Severity::Critical);
        let mut s = healthy();
        s.temps.chip = Some(CHIP_HOT_C - 1.);
        assert!(kinds(&s, &[]).is_empty());
        s.temps.chip = Some(CHIP_HOT_C);
        assert_eq!(kinds(&s, &[]), vec![Kind::Thermal]);
    }

    #[test]
    fn critical_memory_pressure_goes_to_memory() {
        let mut s = healthy();
        s.mem.pressure = Some(Pressure::Warning);
        assert!(kinds(&s, &[]).is_empty());
        s.mem.pressure = Some(Pressure::Critical);
        let issues = attention_with(&s, &[]);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].to == Destination::Tab(Tab::Memory));
    }

    #[test]
    fn a_nearly_full_startup_disk_goes_to_folder_sizes() {
        let mut s = healthy();
        s.disk.free = 5 * GIB;
        let issues = attention_with(&s, &[]);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].to == Destination::Tool(Page::Storage));
        s.disk.total = 0;
        assert!(kinds(&s, &[]).is_empty());
    }

    #[test]
    fn a_flagged_app_names_the_biggest_grower() {
        let mut s = healthy();
        let leak = |name: &str, growth: f64| Leak {
            key: name.into(),
            name: name.into(),
            growth,
        };
        s.leaks = vec![leak("Small", 1e8), leak("Big", 9e8)];
        let issues = attention_with(&s, &[]);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].text.starts_with("Big"));
        assert!(issues[0].to == Destination::Tab(Tab::Memory));
    }

    #[test]
    fn battery_service_or_worn_capacity_goes_to_battery() {
        let mut s = healthy();
        s.battery.health = Some(BATTERY_HEALTH_LOW_PCT);
        assert!(kinds(&s, &[]).is_empty());
        s.battery.health = Some(79.);
        let issues = attention_with(&s, &[]);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].to == Destination::Tab(Tab::Battery));
        // Service wins over capacity, and it is one issue, not two.
        s.battery.condition = Some("Check Battery".into());
        assert_eq!(kinds(&s, &[]), vec![Kind::Battery]);
        // No battery, no issue; no health reading, no guess.
        s.battery.present = false;
        assert!(kinds(&s, &[]).is_empty());
        let mut s = healthy();
        s.battery.health = None;
        assert!(kinds(&s, &[]).is_empty());
    }

    #[test]
    fn worst_first_and_at_most_three() {
        let mut s = healthy();
        s.disk.free = 5 * GIB;
        s.leaks = vec![Leak {
            key: "a".into(),
            name: "A".into(),
            growth: 1e9,
        }];
        s.battery.health = Some(50.);
        s.mem.pressure = Some(Pressure::Critical);
        s.thermal_level = Some(2);
        let order = kinds(&s, &[disk(true, Health::Failed)]);
        assert_eq!(
            order,
            vec![Kind::DiskFailing, Kind::MemoryPressure, Kind::Thermal]
        );
        // Without the critical ones the warnings follow in a fixed order.
        s.mem.pressure = Some(Pressure::Normal);
        assert_eq!(
            kinds(&s, &[]),
            vec![Kind::Thermal, Kind::DiskFull, Kind::GrowingMemory]
        );
    }
}
