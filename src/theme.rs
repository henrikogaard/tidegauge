//! Tidegauge design tokens and per-metric accents.

#![allow(dead_code)]

use crate::settings::{ChartStyle, ColorMode, Settings, Theme};
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: u32,
    /// Sidebar ground: one step off `bg` so the navigation reads as chrome.
    pub sidebar: u32,
    pub card: u32,
    pub card_hi: u32,
    pub ink: u32,
    pub ink_dim: u32,
    pub line: u32,
    pub accent: u32,
}

/// Graphite, light: Tidegauge's original look.
pub const LIGHT: Palette = Palette {
    bg: 0xf5f5f7,
    sidebar: 0xececef,
    card: 0xffffff,
    card_hi: 0xededf0,
    ink: 0x1d1d1f,
    // A shade darker than macOS's secondary label, so dim text keeps 4.5:1
    // on the sidebar too.
    ink_dim: 0x67676c,
    line: 0xd2d2d7,
    accent: 0x1d1d1f,
};

/// Graphite, dark.
pub const DARK: Palette = Palette {
    bg: 0x161618,
    sidebar: 0x1b1b1e,
    card: 0x1e1e20,
    card_hi: 0x2c2c2e,
    ink: 0xe5e5ea,
    // A shade lighter than macOS's secondary label, so dim text keeps 4.5:1
    // on a hovered row too.
    ink_dim: 0x98989d,
    line: 0x38383a,
    accent: 0xf5f5f7,
};

/// The palette of `theme` in light or dark.
pub fn theme_palette(theme: Theme, dark: bool) -> Palette {
    let p = |bg, sidebar, card, card_hi, ink, ink_dim, line, accent| Palette {
        bg, sidebar, card, card_hi, ink, ink_dim, line, accent,
    };
    match (theme, dark) {
        (Theme::Graphite, false) => LIGHT,
        (Theme::Graphite, true) => DARK,
        (Theme::Ocean, false) => p(0xf2f5f9, 0xe7edf4, 0xffffff, 0xe6ecf3, 0x15202b, 0x56667a, 0xcfd8e3, 0x15202b),
        (Theme::Ocean, true) => p(0x0f1620, 0x131c27, 0x16202c, 0x22303f, 0xe3eaf2, 0x8fa0b2, 0x2b3a4b, 0xf0f5fa),
        (Theme::Sand, false) => p(0xf7f3ec, 0xefe9df, 0xfffdf9, 0xefe8dc, 0x2a2118, 0x6f6252, 0xddd2c2, 0x2a2118),
        (Theme::Sand, true) => p(0x1a1612, 0x1f1a15, 0x231e18, 0x302921, 0xede4d8, 0xa39684, 0x3d342a, 0xf7efe4),
        (Theme::Forest, false) => p(0xf1f5f1, 0xe6eee6, 0xffffff, 0xe4ece4, 0x17221a, 0x58685b, 0xcdd9ce, 0x17221a),
        (Theme::Forest, true) => p(0x101611, 0x141b15, 0x172019, 0x233025, 0xe2ece3, 0x8d9d8f, 0x2c3a2e, 0xeff6f0),
        (Theme::Contrast, false) => p(0xffffff, 0xf2f2f2, 0xffffff, 0xe6e6e6, 0x000000, 0x333333, 0x767676, 0x000000),
        (Theme::Contrast, true) => p(0x000000, 0x0a0a0a, 0x0d0d0d, 0x262626, 0xffffff, 0xcccccc, 0x8c8c8c, 0xffffff),
    }
}

/// The palette in use: the theme chosen in Settings, light or dark.
fn palette(dark: bool) -> Palette {
    theme_palette(look().theme, dark)
}

pub fn hex(c: u32) -> Hsla {
    gpui_kit::rgb(c).into()
}

pub fn ink(dark: bool) -> Hsla {
    hex(palette(dark).ink)
}
/// Secondary text. On a see-through window it moves towards the main text
/// just enough to keep 4.5:1 over what is assumed behind it (see
/// `readable_dim`).
pub fn ink_dim(dark: bool) -> Hsla {
    hex(readable_dim(palette(dark), text_backing()))
}

/// WCAG relative luminance of an sRGB colour.
fn luminance(rgb: u32) -> f64 {
    let channel = |shift: u32| {
        let c = f64::from((rgb >> shift) & 0xff) / 255.;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

fn contrast(a: u32, b: u32) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `a` moved towards `b` by `t` (0–1), per sRGB channel.
fn mix(a: u32, b: u32, t: f64) -> u32 {
    [16u32, 8, 0].iter().fold(0, |out, shift| {
        let (x, y) = (f64::from((a >> shift) & 0xff), f64::from((b >> shift) & 0xff));
        out | (((x + (y - x) * t).round() as u32) << shift)
    })
}

/// What a see-through window is assumed to sit on when its secondary text is
/// made readable: the worst case for the theme (white behind a dark theme,
/// black behind a light one), or mid-grey once the ground is clearer than
/// "Whole window" allows, as only "Everything" makes it.
pub(crate) fn assumed_backdrop(p: Palette, a: Alphas) -> u32 {
    if a.bg < MIN_GROUND_ALPHA - 1e-4 {
        0x808080
    } else if luminance(p.bg) < 0.2 {
        0xffffff
    } else {
        0x000000
    }
}

/// The secondary text colour for `p` on surfaces faded to `a`. Dim text is
/// moved towards the main text only as far as needed for 4.5:1 on the
/// ground, sidebar and cards over `assumed_backdrop`; opaque windows keep the
/// palette's own dim text.
pub(crate) fn readable_dim(p: Palette, a: Alphas) -> u32 {
    if a == Alphas::OPAQUE {
        return p.ink_dim;
    }
    let behind = assumed_backdrop(p, a);
    let surfaces = [mix(behind, p.bg, f64::from(a.bg)), mix(behind, p.sidebar, f64::from(a.sidebar)), mix(behind, p.card, f64::from(a.card))];
    let readable = |text: u32| surfaces.iter().all(|s| contrast(text, *s) >= 4.5);
    if readable(p.ink_dim) {
        return p.ink_dim;
    }
    // Smallest step towards the main text that reads; at worst the main text.
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let mid = (low + high) / 2.;
        if readable(mix(p.ink_dim, p.ink, mid)) { high = mid } else { low = mid }
    }
    mix(p.ink_dim, p.ink, high)
}
pub fn card(dark: bool) -> Hsla {
    hex(palette(dark).card)
}
pub fn card_hi(dark: bool) -> Hsla {
    hex(palette(dark).card_hi)
}
pub fn bg(dark: bool) -> Hsla {
    hex(palette(dark).bg)
}
/// Sidebar ground: one step off `bg` so the navigation reads as chrome.
pub fn sidebar(dark: bool) -> Hsla {
    hex(palette(dark).sidebar)
}
// ---------- Transparency ----------
//
// With transparency on, the window is a frosted-glass surface and its
// background layers fade; text, charts and borders stay fully opaque. The
// alpha math lives here, so the views only ask for a surface colour.

/// Least alpha of the window ground, at full amount, for "Whole window". The
/// ground keeps most of its own colour, so text stays readable even over a
/// white desktop; "Everything" is the mode for a clearer window.
pub const MIN_GROUND_ALPHA: f32 = 0.78;
/// Least alpha of the sidebar, at full amount: a little more see-through than
/// the ground, as in Finder, and still dark enough for its labels.
pub const MIN_SIDEBAR_ALPHA: f32 = 0.70;
/// Least alpha of a card, at full amount: cards carry the numbers, so they
/// stay all but solid.
pub const MIN_CARD_ALPHA: f32 = 0.92;
/// "Everything": the least alpha of the ground, sidebar and cards at full
/// amount. Cards fade with the rest; text, charts and lines stay solid. This
/// is the clearest mode, so its floors only promise readable text when what
/// is behind the window is near mid-grey: a dark theme over a very light
/// desktop, or a light theme over a very dark one, loses contrast at the top
/// of the slider (the other modes promise readable text over white behind a
/// dark theme and black behind a light one). Secondary text moves towards
/// the main text colour as the window clears.
pub const MIN_EVERYTHING_GROUND_ALPHA: f32 = 0.30;
pub const MIN_EVERYTHING_SIDEBAR_ALPHA: f32 = 0.30;
pub const MIN_EVERYTHING_CARD_ALPHA: f32 = 0.40;

/// The alpha of each background surface, 1 for opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alphas {
    /// The window's ground, behind the content.
    pub bg: f32,
    pub sidebar: f32,
    pub card: f32,
}

impl Alphas {
    pub const OPAQUE: Alphas = Alphas {
        bg: 1.,
        sidebar: 1.,
        card: 1.,
    };

    /// Whether anything shows through, so the window needs the blur.
    pub fn is_see_through(self) -> bool {
        self != Alphas::OPAQUE
    }
}

/// Fade from opaque towards `floor` as `amount` (a fraction) goes 0 to 1.
fn fade(amount: f64, floor: f32) -> f32 {
    let amount = if amount.is_nan() {
        0.
    } else {
        amount.clamp(0., 1.) as f32
    };
    1. - (1. - floor) * amount
}

/// The dashboard's surfaces for a mode and an amount in percent. The content
/// ground stays opaque with "Sidebar only"; with "Whole window" the ground and
/// sidebar fade fully and cards fade less.
pub fn window_alphas(mode: crate::settings::Transparency, amount_percent: f64) -> Alphas {
    use crate::settings::Transparency;
    let amount = amount_percent / 100.;
    match mode {
        Transparency::Off => Alphas::OPAQUE,
        Transparency::Sidebar => Alphas {
            sidebar: fade(amount, MIN_SIDEBAR_ALPHA),
            ..Alphas::OPAQUE
        },
        Transparency::Window => Alphas {
            bg: fade(amount, MIN_GROUND_ALPHA),
            sidebar: fade(amount, MIN_SIDEBAR_ALPHA),
            card: fade(amount, MIN_CARD_ALPHA),
        },
        Transparency::Everything => Alphas {
            bg: fade(amount, MIN_EVERYTHING_GROUND_ALPHA),
            sidebar: fade(amount, MIN_EVERYTHING_SIDEBAR_ALPHA),
            card: fade(amount, MIN_EVERYTHING_CARD_ALPHA),
        },
    }
}

/// The menu-bar panel's surfaces: its ground and its tiles, with the window's
/// caps.
pub fn panel_alphas(on: bool, amount_percent: f64) -> Alphas {
    if !on {
        return Alphas::OPAQUE;
    }
    let amount = amount_percent / 100.;
    Alphas {
        bg: fade(amount, MIN_GROUND_ALPHA),
        sidebar: 1.,
        card: fade(amount, MIN_CARD_ALPHA),
    }
}

/// `color` with its alpha scaled to `alpha`.
pub fn faded(color: Hsla, alpha: f32) -> Hsla {
    color.opacity(alpha)
}

/// What the dashboard window currently shows through, set by its view each
/// frame; the menu-bar panel passes its own `Alphas` instead.
static WINDOW_ALPHAS: std::sync::RwLock<Alphas> = std::sync::RwLock::new(Alphas::OPAQUE);

pub fn set_window_alphas(alphas: Alphas) {
    *WINDOW_ALPHAS.write().unwrap() = alphas;
    set_text_backing(alphas);
}

/// The surfaces the window being drawn puts its text on: the dashboard's or
/// the menu-bar panel's. Each window sets it at the start of its render.
static TEXT_BACKING: std::sync::RwLock<Alphas> = std::sync::RwLock::new(Alphas::OPAQUE);

pub fn set_text_backing(alphas: Alphas) {
    *TEXT_BACKING.write().unwrap() = alphas;
}

fn text_backing() -> Alphas {
    *TEXT_BACKING.read().unwrap()
}

fn window_alphas_now() -> Alphas {
    *WINDOW_ALPHAS.read().unwrap()
}

/// The dashboard's ground, faded by the window's transparency.
pub fn window_bg(dark: bool) -> Hsla {
    faded(bg(dark), window_alphas_now().bg)
}
pub fn window_sidebar(dark: bool) -> Hsla {
    faded(sidebar(dark), window_alphas_now().sidebar)
}
/// A dashboard card's fill.
pub fn window_card(dark: bool) -> Hsla {
    faded(card(dark), window_alphas_now().card)
}

pub fn line(dark: bool) -> Hsla {
    hex(palette(dark).line)
}

pub fn accent(dark: bool) -> Hsla {
    hex(palette(dark).accent)
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
/// do, so they read the same in every mode but the colour-blind safe one,
/// where they come from the same palette.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub colors: ColorMode,
    pub theme: Theme,
    pub charts: ChartStyle,
    /// The macOS accent colour as 0xRRGGBB, as last read.
    pub system_accent: u32,
}

/// The accent a Mac has until it is read (macOS blue).
const FALLBACK_ACCENT: u32 = 0x007aff;

impl Look {
    pub const DEFAULT: Look = Look {
        colors: ColorMode::Tidegauge,
        theme: Theme::Graphite,
        charts: ChartStyle::Filled,
        system_accent: FALLBACK_ACCENT,
    };

    /// The colour of `metric`: its own, the macOS accent, or one calm tone.
    pub fn metric(&self, metric: Metric, dark: bool) -> u32 {
        match self.colors {
            ColorMode::Tidegauge => metric.base(dark),
            ColorMode::Colorblind => legible(okabe_ito(metric), dark),
            ColorMode::Macos => legible(self.system_accent, dark),
            ColorMode::Calm => calm(dark),
        }
    }
}

static LOOK: std::sync::Mutex<Look> = std::sync::Mutex::new(Look::DEFAULT);

fn look() -> Look {
    *LOOK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The colour of `metric` as 0xRRGGBB under the colour setting last adopted,
/// for the desktop widgets.
pub fn metric_rgb(metric: Metric, dark: bool) -> u32 {
    look().metric(metric, dark)
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
    look.theme = settings.theme;
    look.charts = settings.charts;
    if let Some(system) = system {
        look.system_accent = system;
    }
}

/// Whether the colour-blind safe palette is in use: status colours and the
/// menu bar's warning tints follow it too.
pub fn colorblind() -> bool {
    look().colors == ColorMode::Colorblind
}

/// The Okabe–Ito palette (Okabe & Ito, 2008), whose colours stay apart
/// with every common colour-vision deficiency. Its orange and vermillion
/// mean caution and warning here, and its yellow darkens toward that
/// orange on light cards, so metrics share the other hues and a gray.
fn okabe_ito(metric: Metric) -> u32 {
    match metric {
        Metric::Cpu => 0x0072b2,                                        // blue
        Metric::Memory | Metric::Sound => 0xcc79a7,                     // reddish purple
        Metric::Gpu | Metric::Bluetooth => 0x56b4e9,                    // sky blue
        Metric::Network | Metric::Battery | Metric::Power => 0x009e73, // bluish green
        Metric::Disk | Metric::Projects => 0x8c8c8c,                    // gray
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

/// Good, caution and warning as 0xRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    Caution,
    Warn,
}

fn status_rgb(status: Status, colorblind: bool, dark: bool) -> u32 {
    match (status, colorblind, dark) {
        // Okabe–Ito blue, orange and vermillion, tuned per appearance so
        // caution is clearly brighter than warning as well as another hue.
        (Status::Ok, true, true) => 0x56b4e9,
        (Status::Ok, true, false) => 0x0072b2,
        (Status::Caution, true, true) => 0xe69f00,
        (Status::Caution, true, false) => 0x9b6900,
        (Status::Warn, true, true) => 0xf07030,
        (Status::Warn, true, false) => 0xa63000,
        (Status::Ok, false, true) => 0x30d158,
        (Status::Ok, false, false) => 0x248a3d,
        (Status::Caution, false, true) => 0xe8ad39,
        (Status::Caution, false, false) => 0x9b6900,
        (Status::Warn, false, true) => 0xff453a,
        (Status::Warn, false, false) => 0xd70015,
    }
}

pub fn warn(dark: bool) -> Hsla {
    hex(status_rgb(Status::Warn, colorblind(), dark))
}
/// Caution (amber): something is incomplete or worth a look, not broken.
pub fn caution(dark: bool) -> Hsla {
    hex(status_rgb(Status::Caution, colorblind(), dark))
}
/// Good (green), or blue in the colour-blind palette, so good and bad never
/// differ by red against green alone.
pub fn ok(dark: bool) -> Hsla {
    hex(status_rgb(Status::Ok, colorblind(), dark))
}
/// Track of a switch that is on: the system green of macOS switches.
pub fn switch_on(dark: bool) -> Hsla {
    hex(if dark { 0x30d158 } else { 0x34c759 })
}

#[cfg(test)]
mod theme_tests {
    use super::{theme_palette, DARK, LIGHT};
    use crate::settings::Theme;

    const ALL: [Theme; 5] = [Theme::Graphite, Theme::Ocean, Theme::Sand, Theme::Forest, Theme::Contrast];

    /// WCAG relative luminance of an sRGB colour.
    fn luminance(rgb: u32) -> f64 {
        let channel = |shift: u32| {
            let c = f64::from((rgb >> shift) & 0xff) / 255.;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_theme_keeps_text_readable_in_light_and_dark() {
        for theme in ALL {
            for dark in [false, true] {
                let p = theme_palette(theme, dark);
                for (surface, name) in [(p.card, "card"), (p.bg, "ground"), (p.sidebar, "sidebar"), (p.card_hi, "raised")] {
                    assert!(contrast(p.ink, surface) >= 7., "{theme:?} dark={dark}: text on {name} {:.1}", contrast(p.ink, surface));
                    assert!(contrast(p.ink_dim, surface) >= 4.5, "{theme:?} dark={dark}: dim text on {name} {:.1}", contrast(p.ink_dim, surface));
                }
                // Dark themes are dark and light ones light, whatever their hue.
                assert_eq!(luminance(p.bg) < 0.2, dark, "{theme:?} dark={dark}");
                assert_ne!(p.card, p.card_hi, "{theme:?}: hover must show");
            }
        }
    }

    #[test]
    fn graphite_is_the_original_look_and_themes_differ() {
        assert_eq!(theme_palette(Theme::Graphite, false), LIGHT);
        assert_eq!(theme_palette(Theme::Graphite, true), DARK);
        for dark in [false, true] {
            let grounds: Vec<u32> = ALL.iter().map(|t| theme_palette(*t, dark).bg).collect();
            let mut unique = grounds.clone();
            unique.dedup();
            assert_eq!(unique.len(), grounds.len(), "dark={dark}: every theme has its own ground");
        }
    }
}

#[cfg(test)]
mod transparency_tests {
    use super::{
        faded, hex, panel_alphas, window_alphas, Alphas, LIGHT, MIN_CARD_ALPHA, MIN_GROUND_ALPHA,
        MIN_SIDEBAR_ALPHA,
    };
    use crate::settings::Transparency::{Off, Sidebar, Window};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn off_changes_nothing_at_any_amount() {
        for amount in [0., 40., 100.] {
            assert_eq!(window_alphas(Off, amount), Alphas::OPAQUE);
        }
        assert_eq!(panel_alphas(false, 100.), Alphas::OPAQUE);
        assert!(!Alphas::OPAQUE.is_see_through());
        let base = hex(LIGHT.card);
        assert_eq!(faded(base, Alphas::OPAQUE.card), base);
    }

    #[test]
    fn sidebar_only_fades_the_sidebar_alone() {
        let a = window_alphas(Sidebar, 40.);
        assert!(close(a.sidebar, 1. - 0.4 * (1. - MIN_SIDEBAR_ALPHA)));
        assert_eq!((a.bg, a.card), (1., 1.));
        assert!(a.is_see_through());
    }

    #[test]
    fn whole_window_fades_the_ground_more_than_cards() {
        let a = window_alphas(Window, 40.);
        assert!(close(a.bg, 1. - 0.4 * (1. - MIN_GROUND_ALPHA)));
        assert!(close(a.sidebar, 1. - 0.4 * (1. - MIN_SIDEBAR_ALPHA)));
        assert!(close(a.card, 1. - 0.4 * (1. - MIN_CARD_ALPHA)));
        assert!(a.card > a.bg && a.bg > a.sidebar, "sidebar clearest, cards near solid");
    }

    #[test]
    fn the_caps_hold_at_and_beyond_full_amount() {
        for amount in [100., 250., f64::INFINITY] {
            let a = window_alphas(Window, amount);
            assert!(close(a.bg, MIN_GROUND_ALPHA), "ground {amount}");
            assert!(close(a.sidebar, MIN_SIDEBAR_ALPHA));
            assert!(close(a.card, MIN_CARD_ALPHA));
            assert!(close(
                window_alphas(Sidebar, amount).sidebar,
                MIN_SIDEBAR_ALPHA
            ));
        }
        for amount in [0., -30., f64::NAN] {
            assert_eq!(window_alphas(Window, amount), Alphas::OPAQUE, "{amount}");
        }
    }

    #[test]
    fn the_ground_keeps_most_of_its_colour_even_at_full_amount() {
        // "Whole window" promises readable text over any desktop: over pure
        // white, the dark ground at full amount must stay well below mid-grey.
        // Lightness 0–1 of an sRGB colour, the mean of its channels: enough
        // for these near-greys.
        let lightness = |rgb: u32| {
            [16, 8, 0].iter().map(|shift| f32::from(((rgb >> shift) & 0xff) as u8)).sum::<f32>() / (3. * 255.)
        };
        let over_white = |rgb: u32, alpha: f32| lightness(rgb) * alpha + (1. - alpha);
        let ground = over_white(super::DARK.bg, MIN_GROUND_ALPHA);
        assert!(ground < 0.3, "dark ground over white reads {ground}");
        let sidebar = over_white(super::DARK.sidebar, MIN_SIDEBAR_ALPHA);
        assert!(sidebar < 0.4, "dark sidebar over white reads {sidebar}");
        assert!(MIN_CARD_ALPHA >= 0.9, "cards stay all but solid");
    }

    #[test]
    fn everything_fades_the_cards_too() {
        use super::{MIN_EVERYTHING_CARD_ALPHA, MIN_EVERYTHING_GROUND_ALPHA, MIN_EVERYTHING_SIDEBAR_ALPHA};
        let all = window_alphas(crate::settings::Transparency::Everything, 100.);
        let window = window_alphas(Window, 100.);
        assert!(close(all.bg, MIN_EVERYTHING_GROUND_ALPHA));
        assert!(close(all.sidebar, MIN_EVERYTHING_SIDEBAR_ALPHA));
        assert!(close(all.card, MIN_EVERYTHING_CARD_ALPHA));
        assert!(all.card < window.card && all.bg < window.bg && all.sidebar < window.sidebar, "clearer than Whole window everywhere");
        assert!(all.card > all.bg || close(all.card, all.bg), "cards never fall below the ground");
        // Every dark theme's main and secondary text keeps 4.5:1 on every
        // surface at every amount: over white for Sidebar only, Whole window
        // and the menu-bar panel, and over mid-grey for Everything.
        use super::{contrast, readable_dim, theme_palette};
        use crate::settings::{Theme, Transparency};
        for theme in [Theme::Graphite, Theme::Ocean, Theme::Sand, Theme::Forest, Theme::Contrast] {
            let p = theme_palette(theme, true);
            for amount in (0..=100).step_by(10).map(f64::from) {
                let over_white_modes = [Transparency::Sidebar, Transparency::Window]
                    .map(|mode| (window_alphas(mode, amount), 0xffffff_u32));
                let checks = over_white_modes
                    .into_iter()
                    .chain([(panel_alphas(true, amount), 0xffffff)])
                    .chain([(window_alphas(Transparency::Everything, amount), 0x808080)]);
                for (a, behind) in checks {
                    let dim = readable_dim(p, a);
                    for (surface, alpha) in [(p.bg, a.bg), (p.sidebar, a.sidebar), (p.card, a.card)] {
                        let shown = super::mix(behind, surface, f64::from(alpha));
                        assert!(contrast(p.ink, shown) >= 4.5, "{theme:?} {amount}% over {behind:06x}: text {:.2}", contrast(p.ink, shown));
                        assert!(contrast(dim, shown) >= 4.5, "{theme:?} {amount}% over {behind:06x}: dim {:.2}", contrast(dim, shown));
                    }
                }
            }
            assert_eq!(readable_dim(p, Alphas::OPAQUE), p.ink_dim, "{theme:?}: solid windows keep their dim text");
            // Light themes: over black for the calmer modes, mid-grey for
            // Everything.
            let light = theme_palette(theme, false);
            for amount in (0..=100).step_by(10).map(f64::from) {
                let checks = [Transparency::Sidebar, Transparency::Window]
                    .map(|mode| (window_alphas(mode, amount), 0x000000_u32))
                    .into_iter()
                    .chain([(panel_alphas(true, amount), 0x000000)])
                    .chain([(window_alphas(Transparency::Everything, amount), 0x808080)]);
                for (a, behind) in checks {
                    let dim = readable_dim(light, a);
                    for (surface, alpha) in [(light.bg, a.bg), (light.sidebar, a.sidebar), (light.card, a.card)] {
                        let shown = super::mix(behind, surface, f64::from(alpha));
                        assert!(contrast(light.ink, shown) >= 4.5, "{theme:?} light {amount}% over {behind:06x}: text {:.2}", contrast(light.ink, shown));
                        assert!(contrast(dim, shown) >= 4.5, "{theme:?} light {amount}% over {behind:06x}: dim {:.2}", contrast(dim, shown));
                    }
                }
            }
            assert_eq!(readable_dim(light, Alphas::OPAQUE), light.ink_dim);
        }
        assert_eq!(window_alphas(crate::settings::Transparency::Everything, 0.), Alphas::OPAQUE);
    }

    #[test]
    fn alpha_falls_as_the_amount_rises() {
        let mut last = Alphas::OPAQUE;
        for amount in (0..=100).step_by(10) {
            let a = window_alphas(Window, f64::from(amount));
            assert!(a.bg <= last.bg && a.card <= last.card);
            last = a;
        }
    }

    #[test]
    fn the_panel_uses_the_window_caps_for_ground_and_tiles() {
        let a = panel_alphas(true, 100.);
        assert!(close(a.bg, MIN_GROUND_ALPHA));
        assert!(close(a.card, MIN_CARD_ALPHA));
        let a = panel_alphas(true, 40.);
        assert_eq!(
            a,
            Alphas {
                sidebar: 1.,
                ..window_alphas(Window, 40.)
            }
        );
    }

    #[test]
    fn fading_keeps_the_colour_and_scales_only_alpha() {
        let base = hex(LIGHT.bg);
        let f = faded(base, 0.5);
        assert_eq!((f.h, f.s, f.l), (base.h, base.s, base.l));
        assert!(close(f.a, 0.5));
    }
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
        "slow_check.rs",
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
    use super::{status_rgb, Status,
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
            theme: crate::settings::Theme::Graphite,
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
    fn the_colour_blind_palette_keeps_status_and_metrics_apart() {
        for dark in [false, true] {
            let [good, caution, bad] =
                [Status::Ok, Status::Caution, Status::Warn].map(|status| status_rgb(status, true, dark));
            // Good is blue, never green: hue well away from red's.
            let (good_hue, _, _) = rgb_to_hsl(good);
            assert!((180. ..=240.).contains(&good_hue), "good is blue: {good_hue}");
            assert!(good != caution && caution != bad && good != bad);
            // Caution and warning also differ in brightness as the eye sees
            // it (relative luminance), not only in hue.
            let luminance = |rgb: u32| {
                let channel = |shift: u32| {
                    let c = f64::from((rgb >> shift) & 0xff) / 255.;
                    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
                };
                0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
            };
            assert!(luminance(caution) - luminance(bad) > 0.05, "{dark}: {caution:06x} vs {bad:06x}");
            let palette = look(ColorMode::Colorblind, 0x007aff);
            let (low, high) = lightness_band(dark);
            for metric in [Metric::Cpu, Metric::Memory, Metric::Gpu, Metric::Disk, Metric::Network, Metric::Battery, Metric::Bluetooth] {
                let (_, _, l) = rgb_to_hsl(palette.metric(metric, dark));
                assert!(l >= low - 0.01 && l <= high + 0.01, "{dark}: readable lightness");
            }
            assert_ne!(palette.metric(Metric::Cpu, dark), palette.metric(Metric::Memory, dark));
            // No metric is mistakable for caution or warning: a gray, or a
            // hue at least 30° from both.
            for metric in [Metric::Cpu, Metric::Memory, Metric::Gpu, Metric::Disk, Metric::Network, Metric::Battery, Metric::Sound, Metric::Bluetooth, Metric::Projects, Metric::Power] {
                let (hue, saturation, _) = rgb_to_hsl(palette.metric(metric, dark));
                for status in [caution, bad] {
                    let (status_hue, _, _) = rgb_to_hsl(status);
                    let apart = (hue - status_hue).abs().min(360. - (hue - status_hue).abs());
                    assert!(saturation < 0.1 || apart >= 30., "{dark}: metric hue {hue} near status {status_hue}");
                }
            }
            // Off the colour-blind palette, status colours are unchanged.
            assert_eq!(status_rgb(Status::Ok, false, dark), if dark { 0x30d158 } else { 0x248a3d });
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
