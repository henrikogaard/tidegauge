//! Tidegauge design tokens and per-metric accents.

#![allow(dead_code)]

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
    Neural,
    Power,
}

impl Metric {
    /// Metric identity stays colorful while the surrounding chrome stays neutral.
    pub fn accent(self, dark: bool) -> Hsla {
        hex(match (self, dark) {
            (Metric::Cpu, true) => 0x5a9cf0,
            (Metric::Memory, true) => 0xa397f4,
            (Metric::Gpu, true) => 0xf18db5,
            (Metric::Disk, true) => 0xe8ad39,
            (Metric::Network, true) => 0x41c79a,
            (Metric::Battery | Metric::Power, true) => 0x63ca6e,
            (Metric::Sound, true) => 0xe87bcc,
            (Metric::Bluetooth, true) => 0x51c4df,
            (Metric::Projects, true) => 0xf28d60,
            (Metric::Neural, true) => 0xc28df2,
            (Metric::Cpu, false) => 0x246fc7,
            (Metric::Memory, false) => 0x6550bb,
            (Metric::Gpu, false) => 0xb94378,
            (Metric::Disk, false) => 0x9b6900,
            (Metric::Network, false) => 0x147d59,
            (Metric::Battery | Metric::Power, false) => 0x287c35,
            (Metric::Sound, false) => 0xab338c,
            (Metric::Bluetooth, false) => 0x087d99,
            (Metric::Projects, false) => 0xba4f22,
            (Metric::Neural, false) => 0x8648b8,
        })
    }
    /// Softer version for chart fills.
    pub fn soft(self, dark: bool) -> Hsla {
        let c = self.accent(dark);
        c.opacity(0.16)
    }
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
    ];

    /// Builder calls whose literal argument must come from a token.
    const GUARDED: &[&str] = &[
        "text_size", "rounded", "gap", "p", "px", "py", "pt", "pb", "pl", "pr", "mx", "my", "mt",
        "mb", "ml", "mr",
    ];

    /// Literals that are deliberately not tokens: (file, call prefix, reason).
    const ALLOWED: &[(&str, &str, &str)] = &[
        ("agents_panel.rs", ".gap(px(2.", "hairline gap between stacked list rows"),
        ("ui.rs", ".gap(px(1.", "hairline gap between sidebar items"),
        ("ui.rs", ".gap(px(2.", "gap between level-meter bars (chart geometry)"),
        ("ui.rs", ".py(px(1.", "optical vertical padding of runtime badges"),
        ("ui.rs", ".mx(px(3.", "centres the 8 px swatch in the 14 px icon slot"),
        ("agents_panel.rs", ".ml(px(48.", "aligns detail text under icon and name"),
        ("process_panel.rs", ".ml(px(38.", "aligns detail text under icon and name"),
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
            for (line, call) in literal_calls(&read(file)) {
                if !ALLOWED.iter().any(|(f, prefix, _)| f == file && *prefix == call) {
                    offenders.push(format!("{file}:{line}: {call}"));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "raw px literals; use a token from theme.rs or add a commented ALLOWED entry:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn allowlist_has_no_stale_entries() {
        for (file, prefix, reason) in ALLOWED {
            assert!(!reason.is_empty());
            assert!(
                literal_calls(&read(file)).iter().any(|(_, call)| call == prefix),
                "{file} no longer contains {prefix}; remove the ALLOWED entry"
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
