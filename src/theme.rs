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
    bg: 0xffffff,
    card: 0xf5f5f7,
    card_hi: 0xe8e8ed,
    ink: 0x1d1d1f,
    ink_dim: 0x6b6b73,
    line: 0x000000, // used with opacity via rgba
    accent: 0x007aff,
};

pub const DARK: Palette = Palette {
    bg: 0x1b1b1e,
    card: 0x202023,
    card_hi: 0x27272b,
    ink: 0xf5f5f7,
    ink_dim: 0x86868b,
    line: 0xffffff,
    accent: 0x0a84ff,
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
    if dark { gpui_kit::rgba(0xffffff1a).into() } else { gpui_kit::rgba(0x00000014).into() }
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
    /// Light and dark accent colors for each metric.
    pub fn accent(self, dark: bool) -> Hsla {
        hex(match (self, dark) {
            (Metric::Cpu, false) => 0x2a78d6,
            (Metric::Cpu, true) => 0x3987e5,
            (Metric::Memory | Metric::Neural, false) => 0x4a3aa7,
            (Metric::Memory | Metric::Neural, true) => 0x9085e9,
            (Metric::Gpu, false) => 0xd55181,
            (Metric::Gpu, true) => 0xe87ba4,
            (Metric::Disk, false) => 0xc98500,
            (Metric::Disk, true) => 0xeda100,
            (Metric::Network, false) => 0x199e70,
            (Metric::Network, true) => 0x1baf7a,
            (Metric::Battery | Metric::Power, false) => 0x008300,
            (Metric::Battery | Metric::Power, true) => 0x2fa83a,
            (Metric::Sound, false) => 0xc2379b,
            (Metric::Sound, true) => 0xd955b3,
            (Metric::Bluetooth, false) => 0x0891b2,
            (Metric::Bluetooth, true) => 0x22a8cc,
            (Metric::Projects, false) => 0xd95926,
            (Metric::Projects, true) => 0xeb6834,
        })
    }
    /// Softer version for chart fills.
    pub fn soft(self, dark: bool) -> Hsla {
        let c = self.accent(dark);
        c.opacity(0.16)
    }
}

pub fn warn(dark: bool) -> Hsla {
    hex(if dark { 0xff5a50 } else { 0xe0352b })
}
pub fn ok(_: bool) -> Hsla {
    hex(0x1baf7a)
}
