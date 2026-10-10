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
    /// Neutral chart tones. Labels and values identify metrics without relying on color.
    pub fn accent(self, dark: bool) -> Hsla {
        hex(match (self, dark) {
            (Metric::Cpu, true) => 0xd1d1d6,
            (Metric::Memory | Metric::Neural, true) => 0xaeaeb2,
            (Metric::Gpu | Metric::Sound, true) => 0xc7c7cc,
            (Metric::Disk | Metric::Projects, true) => 0x8e8e93,
            (Metric::Network | Metric::Bluetooth, true) => 0xd1d1d6,
            (Metric::Battery | Metric::Power, true) => 0xb8b8bd,
            (Metric::Cpu, false) => 0x3a3a3c,
            (Metric::Memory | Metric::Neural, false) => 0x6e6e73,
            (Metric::Gpu | Metric::Sound, false) => 0x48484a,
            (Metric::Disk | Metric::Projects, false) => 0x6e6e73,
            (Metric::Network | Metric::Bluetooth, false) => 0x3a3a3c,
            (Metric::Battery | Metric::Power, false) => 0x48484a,
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
pub fn ok(dark: bool) -> Hsla {
    hex(if dark { 0x30d158 } else { 0x248a3d })
}
