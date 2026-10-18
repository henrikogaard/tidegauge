//! Tidegauge design tokens and per-metric accents.

#![allow(dead_code)]

use crate::settings::{ChartStyle, ColorMode, Settings};
use gpui_kit::*;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: u32,
    pub card: u32,
    pub card_hi: u32,
    pub ink: u32,
    pub ink_dim: u32,
    pub line: u32,
    pub accent: u32,
}

pub const LIGHT: Palette = Palette {
    bg: 0xf5f5f7,
    card: 0xffffff,
    card_hi: 0xededf0,
    ink: 0x1d1d1f,
    ink_dim: 0x6e6e73,
    line: 0xd2d2d7,
    accent: 0x1d1d1f,
};

pub const DARK: Palette = Palette {
    bg: 0x161618,
    card: 0x1e1e20,
    card_hi: 0x2c2c2e,
    ink: 0xe5e5ea,
    ink_dim: 0x8e8e93,
    line: 0x38383a,
    accent: 0xf5f5f7,
};

pub fn hex(c: u32) -> Hsla {
    gpui_kit::rgb(c).into()
}

pub fn ink(dark: bool) -> Hsla {
    hex(if dark { DARK.ink } else { LIGHT.ink })
}
pub fn ink_dim(dark: bool) -> Hsla {
    hex(if dark { DARK.ink_dim } else { LIGHT.ink_dim })
}
pub fn card(dark: bool) -> Hsla {
    hex(if dark { DARK.card } else { LIGHT.card })
}
pub fn card_hi(dark: bool) -> Hsla {
    hex(if dark { DARK.card_hi } else { LIGHT.card_hi })
}
pub fn bg(dark: bool) -> Hsla {
    hex(if dark { DARK.bg } else { LIGHT.bg })
}
/// Sidebar ground: one step off `bg` so the navigation reads as chrome.
pub fn sidebar(dark: bool) -> Hsla {
    hex(if dark { 0x1b1b1e } else { 0xececef })
}
pub fn line(dark: bool) -> Hsla {
    hex(if dark { DARK.line } else { LIGHT.line })
}

pub fn accent(dark: bool) -> Hsla {
    hex(if dark { DARK.accent } else { LIGHT.accent })
}

/// Foreground for a neutral filled identity tile.
pub fn on_accent(dark: bool) -> Hsla {
    hex(if dark { 0x1d1d1f } else { 0xffffff })
}

// ---------- Size, spacing and radius tokens ----------
//
// Typography, corner radii and layout spacing come from here, never from raw
// `px(..)` literals (`design_tokens_tests` enforces it). Chart geometry, icon
// sizes and column widths stay literal. Values are documented in DESIGN.md.

/// Uppercase labels and tiny badges.
pub const TEXT_CAPTION: Pixels = px(10.);
/// Secondary readings and list metadata.
pub const TEXT_SMALL: Pixels = px(11.);
/// Default body and control label.
pub const TEXT_BODY: Pixels = px(12.);
/// List-row names and headline lines.
pub const TEXT_ROW: Pixels = px(13.);
/// Emphasised values (units beside a stat, card sub-headings).
pub const TEXT_EMPH: Pixels = px(14.);
/// Page and section titles.
pub const TEXT_TITLE: Pixels = px(20.);
/// Overview card headline numbers.
pub const TEXT_STAT: Pixels = px(34.);
/// Detail-page hero number.
pub const TEXT_HERO: Pixels = px(42.);

/// Small badges, icon tiles and row-highlight bars.
pub const RADIUS_CHIP: Pixels = px(6.);
/// Controls, list rows, toasts and notices.
pub const RADIUS_CONTROL: Pixels = px(8.);
/// Cards and panels.
pub const RADIUS_CARD: Pixels = px(12.);
/// Thin bars and colour swatches.
pub const RADIUS_BAR: Pixels = px(2.);

pub const SPACE_1: Pixels = px(4.);
pub const SPACE_2: Pixels = px(8.);
pub const SPACE_3: Pixels = px(12.);
pub const SPACE_4: Pixels = px(16.);
/// Page gutter.
pub const SPACE_5: Pixels = px(20.);
pub const SPACE_6: Pixels = px(24.);
/// Gap between the sections of a detail page.
pub const SECTION_GAP: Pixels = SPACE_4;

/// Height of buttons, segmented controls and steppers.
pub const CONTROL_H: Pixels = px(28.);
/// Height of a list row.
pub const ROW_H: Pixels = px(36.);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Cpu,
    Memory,
    Gpu,
    Disk,
    Network,
    Battery,
    Sound,
    Bluetooth,
    Projects,
    Power,
}

impl Metric {
    /// Metric identity stays colorful while the surrounding chrome stays
    /// neutral, unless Settings → Colors says otherwise.
    pub fn accent(self, dark: bool) -> Hsla {
        hex(look().metric(self, dark))
    }
    /// The Tidegauge palette's colour for this metric, before any transform.
    pub fn base(self, dark: bool) -> u32 {
        match (self, dark) {
            (Metric::Cpu, true) => 0x5a9cf0,
            (Metric::Memory, true) => 0xa397f4,
            (Metric::Gpu, true) => 0xf18db5,
            (Metric::Disk, true) => 0xe8ad39,
            (Metric::Network, true) => 0x41c79a,
            (Metric::Battery | Metric::Power, true) => 0x63ca6e,
            (Metric::Sound, true) => 0xe87bcc,
            (Metric::Bluetooth, true) => 0x51c4df,
            (Metric::Projects, true) => 0xf28d60,
            (Metric::Cpu, false) => 0x246fc7,
            (Metric::Memory, false) => 0x6550bb,
            (Metric::Gpu, false) => 0xb94378,
            (Metric::Disk, false) => 0x9b6900,
            (Metric::Network, false) => 0x147d59,
            (Metric::Battery | Metric::Power, false) => 0x287c35,
            (Metric::Sound, false) => 0xab338c,
            (Metric::Bluetooth, false) => 0x087d99,
            (Metric::Projects, false) => 0xba4f22,
        }
    }
    /// Softer version for chart fills.
    pub fn soft(self, dark: bool) -> Hsla {
        let c = self.accent(dark);
        c.opacity(0.16)
    }
}

// ---------- Colors and chart style (Settings → Colors, Charts) ----------

/// The appearance choices that change how metrics are drawn: the one place
/// where the base palette and the settings meet. `Metric::accent` asks it for
/// every metric colour; the warning, caution and success colours below never
/// do, so they read the same in every mode.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub colors: ColorMode,
    pub charts: ChartStyle,
    /// The macOS accent colour as 0xRRGGBB, as last read.
    pub system_accent: u32,
}

/// The accent a Mac has until it is read (macOS blue).
const FALLBACK_ACCENT: u32 = 0x007aff;

impl Look {
    pub const DEFAULT: Look = Look {
        colors: ColorMode::Tidegauge,
        charts: ChartStyle::Filled,
        system_accent: FALLBACK_ACCENT,
    };

    /// The colour of `metric`: its own, the macOS accent, or one calm tone.
    pub fn metric(&self, metric: Metric, dark: bool) -> u32 {
        match self.colors {
            ColorMode::Tidegauge => metric.base(dark),
            ColorMode::Macos => legible(self.system_accent, dark),
            ColorMode::Calm => calm(dark),
        }
    }
}

static LOOK: std::sync::Mutex<Look> = std::sync::Mutex::new(Look::DEFAULT);

fn look() -> Look {
    *LOOK.lock().unwrap_or_else(|e| e.into_inner())
}

/// How the chart builders draw a series.
pub fn chart_style() -> ChartStyle {
    look().charts
}

/// Adopt the colour and chart settings. Every window's render calls this
/// before it builds anything, so one change reaches the dashboard and the
/// menu-bar panel alike. The macOS accent is read only while it is in use.
pub fn sync(settings: &Settings) {
    let system = (settings.colors == ColorMode::Macos)
        .then(crate::appearance::system_accent)
        .flatten();
    let mut look = LOOK.lock().unwrap_or_else(|e| e.into_inner());
    look.colors = settings.colors;
    look.charts = settings.charts;
    if let Some(system) = system {
        look.system_accent = system;
    }
}

/// Calm: one muted blue-gray for every metric, readable on both card fills.
fn calm(dark: bool) -> u32 {
    if dark {
        0x93a4b8
    } else {
        0x56677b
    }
}

/// Lightness band (HSL) an accent keeps on a card: lighter on dark cards,
/// darker on light ones, so a pale or a dark system accent stays legible.
fn lightness_band(dark: bool) -> (f32, f32) {
    if dark {
        (0.58, 0.76)
    } else {
        (0.30, 0.48)
    }
}

/// `rgb` with its lightness moved into the card's band; hue and saturation
/// stay, so the colour is still recognisably the user's accent.
fn legible(rgb: u32, dark: bool) -> u32 {
    let (h, s, l) = rgb_to_hsl(rgb);
    let (low, high) = lightness_band(dark);
    hsl_to_rgb(h, s, l.clamp(low, high))
}

fn rgb_to_hsl(rgb: u32) -> (f32, f32, f32) {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.;
    let (r, g, b) = (channel(16), channel(8), channel(0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    let d = max - min;
    if d < 1e-6 {
        return (0., 0., l);
    }
    let s = d / (1. - (2. * l - 1.).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60., s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> u32 {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.) - 1.).abs());
    let (r, g, b) = match (h.rem_euclid(360.) / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = l - c / 2.;
    let byte = |v: f32| ((v + m).clamp(0., 1.) * 255.).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// Opacity of the `step`-th segment of a breakdown ring (0 is the largest):
/// one accent in steps, so a single-accent mode still tells segments apart.
pub fn shade_opacity(step: usize) -> f32 {
    (1. - step as f32 * 0.16).max(0.35)
}

pub fn shade(color: Hsla, step: usize) -> Hsla {
    color.opacity(shade_opacity(step))
}

/// Digits of equal width, so a live number does not shift as it changes.
/// Set on the root of each window; the text below inherits it.
pub fn tabular() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

pub fn warn(dark: bool) -> Hsla {
    hex(if dark { 0xff453a } else { 0xd70015 })
}
/// Caution (amber): something is incomplete or worth a look, not broken.
pub fn caution(dark: bool) -> Hsla {
    hex(if dark { 0xe8ad39 } else { 0x9b6900 })
}
pub fn ok(dark: bool) -> Hsla {
    hex(if dark { 0x30d158 } else { 0x248a3d })
}
/// Track of a switch that is on: the system green of macOS switches.
pub fn switch_on(dark: bool) -> Hsla {
    hex(if dark { 0x30d158 } else { 0x34c759 })
}

#[cfg(test)]
mod design_tokens_tests {
    // Explicit imports: the glob `gpui_kit::*` in the parent also exports a
    // `test` macro that shadows `#[test]`.
    use super::{CONTROL_H, ROW_H, SECTION_GAP, SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_5, SPACE_6};

    /// Source files that build the dashboard UI.
    const UI_FILES: &[&str] = &[
        "ui.rs",
        "controls.rs",
        "process_panel.rs",
        "agents_panel.rs",
        "tools_panel.rs",
        "history_browser.rs",
        "settings_page.rs",
        "menu_panel.rs",
    ];

    /// Builder calls whose literal argument must come from a token.
    const GUARDED: &[&str] = &[
        "text_size", "rounded", "gap", "p", "px", "py", "pt", "pb", "pl", "pr", "mx", "my", "mt",
        "mb", "ml", "mr",
    ];

    /// Literals that are deliberately not tokens: (file, call prefix, exact
    /// number of uses, reason). The count pins each exemption to the call
    /// sites it was written for: a new raw call with the same method and
    /// value in that file fails the test instead of slipping through.
    const ALLOWED: &[(&str, &str, usize, &str)] = &[
        ("agents_panel.rs", ".gap(px(2.", 2, "hairline gap between stacked lines: plan tile text, session list rows"),
        ("ui.rs", ".gap(px(1.", 1, "hairline gap between sidebar items"),
        ("ui.rs", ".gap(px(2.", 1, "gap between level-meter bars (chart geometry)"),
        ("ui.rs", ".py(px(1.", 2, "optical vertical padding of the two runtime badges"),
        ("ui.rs", ".mx(px(3.", 1, "centres the 8 px swatch in the 14 px icon slot"),
        ("agents_panel.rs", ".ml(px(48.", 1, "aligns session detail under icon and name"),
        ("process_panel.rs", ".ml(px(38.", 1, "aligns app detail under icon and name"),
        ("ui.rs", ".ml(px(52.", 1, "aligns a list row's detail under its name (row inset + icon + gap)"),
    ];

    fn read(file: &str) -> String {
        let path = format!("{}/src/{file}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Every `.method(px(<number>` in `source`, as the matched prefix.
    fn literal_calls(source: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        for (index, line) in source.lines().enumerate() {
            for method in GUARDED {
                let needle = format!(".{method}(px(");
                let mut rest = line;
                while let Some(at) = rest.find(&needle) {
                    let after = &rest[at + needle.len()..];
                    let number: String =
                        after.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    if number.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        found.push((index + 1, format!("{needle}{number}")));
                    }
                    rest = after;
                }
            }
        }
        found
    }

    #[test]
    fn ui_uses_tokens_for_typography_radius_and_spacing() {
        let mut offenders = Vec::new();
        for file in UI_FILES {
            let calls = literal_calls(&read(file));
            for (line, call) in &calls {
                let allowed = ALLOWED.iter().find(|(f, prefix, _, _)| f == file && prefix == call);
                match allowed {
                    None => offenders.push(format!("{file}:{line}: {call}")),
                    Some((_, prefix, count, _)) => {
                        let uses = calls.iter().filter(|(_, c)| c == prefix).count();
                        if uses > *count {
                            offenders.push(format!("{file}:{line}: {call} ({uses} uses, {count} allowed)"));
                        }
                    }
                }
            }
        }
        offenders.dedup();
        assert!(
            offenders.is_empty(),
            "raw px literals; use a token from theme.rs or add a commented ALLOWED entry:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn allowlist_has_no_stale_entries() {
        for (file, prefix, count, reason) in ALLOWED {
            assert!(!reason.is_empty());
            let uses = literal_calls(&read(file)).iter().filter(|(_, call)| call == prefix).count();
            assert!(
                uses >= *count,
                "{file} has {uses} of {prefix}, {count} allowed; lower or remove the ALLOWED entry"
            );
        }
    }

    #[test]
    fn scanner_flags_literals_and_ignores_tokens() {
        let sample = "div().text_size(px(13.)).rounded(t::RADIUS_CARD).p(px(8.)).px(t::SPACE_2)";
        let calls: Vec<String> = literal_calls(sample).into_iter().map(|(_, c)| c).collect();
        assert_eq!(calls, vec![".text_size(px(13.", ".p(px(8."]);
    }

    #[test]
    fn spacing_tokens_stay_on_the_four_pixel_grid() {
        for space in [SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_5, SPACE_6, SECTION_GAP, CONTROL_H, ROW_H] {
            assert_eq!(f32::from(space) % 4., 0.);
        }
    }
}

#[cfg(test)]
mod look_tests {
    // Explicit imports for the same reason as above: no glob, so `#[test]`
    // stays the standard attribute.
    use super::{
        caution, hex, hsl_to_rgb, legible, lightness_band, ok, rgb_to_hsl, shade, shade_opacity,
        tabular, warn, ChartStyle, ColorMode, Look, Metric,
    };

    const METRICS: [Metric; 10] = [
        Metric::Cpu,
        Metric::Memory,
        Metric::Gpu,
        Metric::Disk,
        Metric::Network,
        Metric::Battery,
        Metric::Sound,
        Metric::Bluetooth,
        Metric::Projects,
        Metric::Power,
    ];

    fn look(colors: ColorMode, system_accent: u32) -> Look {
        Look {
            colors,
            charts: ChartStyle::Filled,
            system_accent,
        }
    }

    #[test]
    fn tidegauge_mode_keeps_every_metric_colour() {
        let look = look(ColorMode::Tidegauge, 0xff0000);
        for dark in [false, true] {
            for metric in METRICS {
                assert_eq!(look.metric(metric, dark), metric.base(dark));
            }
        }
        assert_eq!(Look::DEFAULT.colors, ColorMode::Tidegauge);
        assert_eq!(Look::DEFAULT.charts, ChartStyle::Filled);
    }

    #[test]
    fn macos_and_calm_give_every_metric_one_colour() {
        for mode in [ColorMode::Macos, ColorMode::Calm] {
            let look = look(mode, 0x007aff);
            for dark in [false, true] {
                let first = look.metric(Metric::Cpu, dark);
                assert!(
                    METRICS.iter().all(|m| look.metric(*m, dark) == first),
                    "{mode:?} dark={dark}"
                );
            }
        }
    }

    #[test]
    fn macos_mode_follows_the_accent_and_stays_legible_on_cards() {
        // Pale, dark, saturated and grey accents, as System Settings offers:
        // each lands in the lightness band of the card.
        for accent in [
            0x007aff, 0xff9500, 0xffd60a, 0x8e8e93, 0x1d1d1f, 0xf5f5f7, 0xa550a7, 0xf74f9e,
        ] {
            for dark in [false, true] {
                let colour = look(ColorMode::Macos, accent).metric(Metric::Cpu, dark);
                let (low, high) = lightness_band(dark);
                let l = rgb_to_hsl(colour).2;
                assert!(
                    l >= low - 0.01 && l <= high + 0.01,
                    "{accent:06x} dark={dark} -> {colour:06x} l={l}"
                );
            }
        }
        assert_ne!(
            look(ColorMode::Macos, 0x007aff).metric(Metric::Cpu, false),
            look(ColorMode::Macos, 0xff9500).metric(Metric::Cpu, false),
            "a different accent gives a different colour"
        );
    }

    #[test]
    fn legible_keeps_the_hue_and_leaves_colours_inside_the_band_alone() {
        let (hue, _, _) = rgb_to_hsl(0x007aff);
        let (moved, _, _) = rgb_to_hsl(legible(0x007aff, true));
        assert!((hue - moved).abs() < 2., "{hue} vs {moved}");
        assert_eq!(legible(0x246fc7, false), 0x246fc7);
    }

    #[test]
    fn hsl_round_trips() {
        for rgb in [0x000000, 0xffffff, 0x246fc7, 0xe8ad39, 0x93a4b8, 0x6550bb] {
            let (h, s, l) = rgb_to_hsl(rgb);
            let back = hsl_to_rgb(h, s, l);
            for shift in [16, 8, 0] {
                let (a, b) = ((rgb >> shift & 0xff) as i32, (back >> shift & 0xff) as i32);
                assert!((a - b).abs() <= 1, "{rgb:06x} -> {back:06x}");
            }
        }
    }

    #[test]
    fn warnings_keep_their_colours_and_stay_distinct_from_every_mode() {
        // warn, caution and ok are not metric colours and never pass through
        // the transform; a calm or accent-coloured chart must not be
        // mistakable for them.
        for dark in [false, true] {
            assert_eq!(warn(dark), hex(if dark { 0xff453a } else { 0xd70015 }));
            assert_eq!(caution(dark), hex(if dark { 0xe8ad39 } else { 0x9b6900 }));
            assert_eq!(ok(dark), hex(if dark { 0x30d158 } else { 0x248a3d }));
            for mode in [ColorMode::Macos, ColorMode::Calm] {
                let colour = hex(look(mode, 0x007aff).metric(Metric::Cpu, dark));
                assert_ne!(colour, warn(dark));
                assert_ne!(colour, caution(dark));
            }
        }
    }

    #[test]
    fn shade_steps_fade_but_stay_visible() {
        assert_eq!(shade_opacity(0), 1.);
        let steps: Vec<f32> = (0..8).map(shade_opacity).collect();
        assert!(steps.windows(2).all(|w| w[0] >= w[1]));
        assert!(steps.iter().all(|o| *o >= 0.35));
        assert!(
            steps[..5].windows(2).all(|w| w[0] > w[1]),
            "the first five steps differ"
        );
        assert_eq!(shade(hex(0x246fc7), 0), hex(0x246fc7));
    }

    #[test]
    fn tabular_figures_ask_for_tnum() {
        assert_eq!(tabular().tag_value_list(), [("tnum".to_string(), 1)]);
    }
}
