//! The Tidegauge panel: top tab bar, card-based content.

#![allow(dead_code)]

use crate::engine::*;
use crate::processes::AppGroup;
use crate::theme::{self as t, Metric};
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    Neural,
    Battery,
    Sound,
    Bluetooth,
    Projects,
}

impl Tab {
    const ALL: [Tab; 11] = [
        Tab::Overview, Tab::Cpu, Tab::Memory, Tab::Disk, Tab::Network, Tab::Gpu,
        Tab::Neural, Tab::Battery, Tab::Sound, Tab::Bluetooth, Tab::Projects,
    ];
    fn label(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Cpu => "CPU",
            Tab::Memory => "Memory",
            Tab::Disk => "Disk",
            Tab::Network => "Network",
            Tab::Gpu => "GPU",
            Tab::Neural => "Neural",
            Tab::Battery => "Battery",
            Tab::Sound => "Sound",
            Tab::Bluetooth => "Bluetooth",
            Tab::Projects => "Projects",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Tab::Overview => "icons/layout-dashboard.svg",
            Tab::Cpu => "icons/cpu.svg",
            Tab::Memory => "icons/memory-stick.svg",
            Tab::Disk => "icons/hard-drive.svg",
            Tab::Network => "icons/globe.svg",
            Tab::Gpu => "icons/gpu.svg",
            Tab::Neural => "icons/brain.svg",
            Tab::Battery => "icons/battery-charging.svg",
            Tab::Sound => "icons/volume-2.svg",
            Tab::Bluetooth => "icons/bluetooth.svg",
            Tab::Projects => "icons/folder-code.svg",
        }
    }
    fn metric(self) -> Option<Metric> {
        match self {
            Tab::Cpu => Some(Metric::Cpu),
            Tab::Memory => Some(Metric::Memory),
            Tab::Disk => Some(Metric::Disk),
            Tab::Network => Some(Metric::Network),
            Tab::Gpu => Some(Metric::Gpu),
            Tab::Neural => Some(Metric::Neural),
            Tab::Battery => Some(Metric::Battery),
            Tab::Sound => Some(Metric::Sound),
            Tab::Bluetooth => Some(Metric::Bluetooth),
            Tab::Projects => Some(Metric::Projects),
            Tab::Overview => None,
        }
    }
}

pub struct TideGaugeView {
    pub tab: Tab,
    pub dark: bool,
    pub scroll: ScrollHandle,
    pub focus: FocusHandle,
    pub snap: Snapshot,
    pub window: Option<AnyWindowHandle>,
}

impl TideGaugeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let v = TideGaugeView {
            tab: Tab::Overview,
            dark: matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
            scroll: ScrollHandle::new(),
            focus,
            snap: latest(),
            window: None,
        };
        // 1s refresh loop
        cx.spawn(async move |this: WeakEntity<TideGaugeView>, cx: &mut AsyncApp| {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(1000)).await;
                let _ = cx.update(|cx: &mut App| {
                    let _ = this.update(cx, |v: &mut TideGaugeView, cx: &mut Context<TideGaugeView>| {
                        v.snap = latest();
                        cx.notify();
                    });
                });
            }
        })
        .detach();
        v
    }
}

// ---------- small builders ----------

fn caption(text: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t::ink_dim(true))
        .child(text.to_uppercase())
}

fn sub(text: impl Into<String>) -> Div {
    div().text_xs().child(text.into())
}

fn icon_img(path: &'static str, size: f32, color: Hsla) -> impl IntoElement {
    svg().path(path).size(px(size)).text_color(color)
}

fn app_icon(app: &AppGroup, dark: bool) -> impl IntoElement {
    if let Some(path) = &app.icon_path {
        if let Some(file) = crate::icons::icon_file_for_bundle(path) {
            return div()
                .size(px(20.))
                .rounded_sm()
                .overflow_hidden()
                .child(img(file).size(px(20.)).rounded_sm());
        }
    }
    // letter tile fallback
    let initial = app.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    div()
        .size(px(20.))
        .rounded_sm()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(gpui_kit::white())
        .bg(Metric::Memory.accent(dark))
        .child(initial)
}

/// Area+line chart drawn with canvas: filled path under the series and a
/// thin ribbon stroke on top, plus a soft grid.
fn area_chart(pts: &[f64], height: f32, color: Hsla, max_hint: Option<f64>) -> Canvas<()> {
    let pts = pts.to_vec();
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _| {
            let w: f32 = bounds.size.width.into();
            let h: f32 = bounds.size.height.into();
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            if pts.len() < 2 || w < 4.0 {
                return;
            }
            let n = pts.len().min(240);
            let tail = &pts[pts.len() - n..];
            let data_max = tail.iter().cloned().fold(0.0f64, f64::max);
            let max = max_hint.unwrap_or(data_max).max(data_max).max(1.0);
            let px_per = (w - 4.0) / (n - 1).max(1) as f32;
            let y_of = |v: f64| -> f32 { oy + (h - 4.0) - ((v / max).clamp(0.0, 1.0) as f32) * (h - 8.0) };

            // filled area
            let mut area = Path::new(point(px(ox + 2.0), px(oy + h - 2.0)));
            for (i, v) in tail.iter().enumerate() {
                area.line_to(point(px(ox + 2.0 + i as f32 * px_per), px(y_of(*v))));
            }
            area.line_to(point(px(ox + 2.0 + (n - 1) as f32 * px_per), px(oy + h - 2.0)));
            window.paint_path(area, color.opacity(0.18));

            // stroke as thin quads (each segment its own sub-path)
            let mut stroke = Path::new(point(px(ox + 2.0), px(y_of(tail[0]))));
            for i in 1..n {
                let x0 = ox + 2.0 + (i - 1) as f32 * px_per;
                let y0 = y_of(tail[i - 1]);
                let x1 = ox + 2.0 + i as f32 * px_per;
                let y1 = y_of(tail[i]);
                // perpendicular-ish: just a 1.6px box around the segment
                let thick = 0.8f32;
                stroke.move_to(point(px(x0), px(y0 - thick)));
                stroke.line_to(point(px(x1), px(y1 - thick)));
                stroke.line_to(point(px(x1), px(y1 + thick)));
                stroke.line_to(point(px(x0), px(y0 + thick)));
                stroke.line_to(point(px(x0), px(y0 - thick)));
            }
            window.paint_path(stroke, color);
        },
    )
    .w_full()
    .h(px(height))
}

/// Horizontal usage bar used in ranked rows.
fn usage_bar(frac: f64, color: Hsla) -> Div {
    div()
        .h(px(5.))
        .w(px(90.))
        .rounded_full()
        .bg(color.opacity(0.15))
        .child(
            div()
                .h_full()
                .w(relative(frac.clamp(0.0, 1.0) as f32))
                .rounded_full()
                .bg(color),
        )
}

/// Ranked "app | metric" row: icon, name + "N processes", bar, value.
fn app_row(rank: usize, app: &AppGroup, value: String, frac: f64, color: Hsla, dark: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .py(px(5.))
        .px_3()
        .rounded_md()
        .child(
            div()
                .w(px(16.))
                .text_xs()
                .text_color(t::ink_dim(dark))
                .child(format!("{}", rank)),
        )
        .child(app_icon(app, dark))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(app.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(t::ink_dim(dark))
                        .child(if app.pids.len() > 1 {
                            format!("{} processes", app.pids.len())
                        } else {
                            "1 process".into()
                        }),
                ),
        )
        .child(usage_bar(frac, color))
        .child(
            div()
                .w(px(88.))
                .text_right()
                .text_sm()
                .child(value),
        )
}

fn card(dark: bool) -> Div {
    div()
        .bg(t::card(dark))
        .rounded(px(12.))
        .border_1()
        .border_color(t::line(dark))
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
}

/// Hero card for a metric tab: caption + giant number + chart + footer stats.
fn hero(
    label: &str,
    value: String,
    sublabel: String,
    hist: &Hist,
    color: Hsla,
    dark: bool,
    footer: Vec<(String, String)>,
    max_hint: Option<f64>,
) -> Div {
    card(dark)
        .child(
            div().flex().justify_between().items_start().child(
                div()
                    .child(sub(label.to_uppercase()).text_color(t::ink_dim(dark)).font_weight(FontWeight::SEMIBOLD))
                    .child(
                        div()
                            .text_size(px(42.))
                            .font_weight(FontWeight::BOLD)
                            .child(value),
                    )
                    .child(sub(sublabel).text_color(t::ink_dim(dark))),
            ),
        )
        .child(area_chart(&hist.pts, 130.0, color, max_hint))
        .child(
            div()
                .flex()
                .gap_6()
                .children(footer.into_iter().map(|(k, v)| {
                    div()
                        .child(sub(k).text_color(t::ink_dim(dark)))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(v))
                })),
        )
}

fn stat_card(label: &str, value: String, color: Option<Hsla>, dark: bool) -> Div {
    card(dark)
        .p_3()
        .child(sub(label.to_uppercase()).text_color(t::ink_dim(dark)).font_weight(FontWeight::SEMIBOLD))
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .text_color(color.unwrap_or_else(|| t::ink(dark)))
                .child(value),
        )
}

fn section_header(left: &str, right: &str, dark: bool) -> Div {
    div()
        .flex()
        .justify_between()
        .items_end()
        .px_1()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t::ink_dim(dark))
                .child(left.to_uppercase()),
        )
        .child(
            div()
                .text_xs()
                .text_color(t::ink_dim(dark))
                .child(right.to_string()),
        )
}

fn list_card(rows: Vec<Div>, dark: bool) -> Div {
    card(dark).p_2().gap_0().children(rows)
}

// ---------- Render ----------

impl Render for TideGaugeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.dark = matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let dark = self.dark;
        let s = &self.snap;
        let tab = self.tab;
        let this = cx.entity();

        // Pill tab bar: centered, rounded-full card, icon+label
        // pills; active = metric accent text on accent@15% bg.
        let topbar = div()
            .w_full()
            .border_b_1()
            .border_color(t::line(dark))
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .rounded_full()
                    .bg(t::card(dark))
                    .border_1()
                    .border_color(t::line(dark))
                    .p_1()
                    .children(Tab::ALL.iter().map(|&tb| {
                        let active = tb == tab;
                        let accent = tb
                            .metric()
                            .map(|m| m.accent(dark))
                            .unwrap_or_else(hex_acc);
                        let this = this.clone();
                        div()
                            .id(SharedString::from(format!("tab-{}", tb.label())))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .px(px(9.))
                            .py(px(5.))
                            .rounded_full()
                            .cursor_pointer()
                            .when(active, |d| {
                                d.bg(accent.opacity(0.15)).text_color(accent)
                            })
                            .when(!active, |d| {
                                d.text_color(t::ink_dim(dark))
                                    .hover(|h| h.text_color(t::ink(dark)))
                            })
                            .child(icon_img(tb.icon(), 13.0, if active { accent } else { t::ink_dim(dark) }))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .line_height(px(13.))
                                    .child(tb.label()),
                            )
                            .on_click(move |_, _w, cx| {
                                this.update(cx, |v, cx| {
                                    v.tab = tb;
                                    v.scroll = ScrollHandle::new();
                                    cx.notify();
                                });
                            })
                    })),
            );

        let footer = div()
            .w_full()
            .border_t_1()
            .border_color(t::line(dark))
            .px_3()
            .py_1()
            .flex()
            .justify_end()
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(t::ink_dim(dark))
                    .child(format!("{} apps · {} processes", s.app_count, s.proc_count)),
            );

        let content = div()
            .id("content")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_tab(dark));

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t::bg(dark))
            .text_color(t::ink(dark))
            .font_family(".AppleSystemUIFont")
            .track_focus(&self.focus)
            .child(topbar)
            .child(content)
            .child(footer)
    }
}

fn hex_acc() -> Hsla {
    gpui_kit::rgb(0x007aff).into()
}

impl TideGaugeView {
    fn render_tab(&self, dark: bool) -> AnyElement {
        let s = &self.snap;
        match self.tab {
            Tab::Overview => overview(s, dark).into_any_element(),
            Tab::Cpu => cpu_tab(s, dark).into_any_element(),
            Tab::Memory => memory_tab(s, dark).into_any_element(),
            Tab::Disk => disk_tab(s, dark).into_any_element(),
            Tab::Network => network_tab(s, dark).into_any_element(),
            Tab::Gpu => gpu_tab(s, dark).into_any_element(),
            Tab::Neural => neural_tab(s, dark).into_any_element(),
            Tab::Battery => battery_tab(s, dark).into_any_element(),
            Tab::Sound => sound_tab(s, dark).into_any_element(),
            Tab::Bluetooth => bt_tab(s, dark).into_any_element(),
            Tab::Projects => projects_tab(s, dark).into_any_element(),
        }
    }
}

fn overview(s: &Snapshot, dark: bool) -> Div {
    let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
    let disk_pct = s.disk.used as f64 / s.disk.total.max(1) as f64 * 100.0;
    let batt = if s.battery.present {
        s.battery.percent.map(|p| format!("{:.0}%", p)).unwrap_or("—".into())
    } else {
        "—".into()
    };
    let mut cols = div().flex().flex_col().gap_3();
    // row 1: cpu, memory, gpu
    cols = cols.child(
        div()
            .grid()
            .grid_cols(3)
            .gap_3()
            .child(mini_tile("CPU", format!("{:.0}%", s.cpu_pct), &s.cpu_hist, Metric::Cpu, dark, Some(100.0)))
            .child(mini_tile("Memory", format!("{:.0}%", mem_pct), &s.mem_hist, Metric::Memory, dark, Some(100.0)))
            .child(mini_tile(
                "GPU",
                s.gpu.utilization.map(|u| format!("{:.0}%", u)).unwrap_or("—".into()),
                &s.gpu_hist,
                Metric::Gpu,
                dark,
                Some(100.0),
            )),
    )
    .child(
        div()
            .grid()
            .grid_cols(3)
            .gap_3()
            .child(mini_tile("Disk", format!("{:.0}%", disk_pct), &s.disk_w_hist, Metric::Disk, dark, None))
            .child(mini_tile(
                "Network",
                fmt_rate(s.net.rate_in + s.net.rate_out),
                &s.net_in_hist,
                Metric::Network,
                dark,
                None,
            ))
            .child(mini_tile("Battery", batt, &s.batt_hist, Metric::Battery, dark, Some(100.0))),
    )
    .child(
        div()
            .grid()
            .grid_cols(3)
            .gap_3()
            .child(donut_card(
                "Memory by Type",
                vec![
                    ("Apps".to_string(), s.mem.app as f64, Metric::Memory.accent(dark)),
                    ("Wired".to_string(), s.mem.wired as f64, t::ink_dim(dark)),
                    ("Compressed".to_string(), s.mem.compressed as f64, Metric::Cpu.accent(dark)),
                    ("Cached/Free".to_string(), (s.mem.cached + s.mem.free) as f64, t::card_hi(dark)),
                ],
                format_bytes_mem(s.mem.used, s.mem.total),
                dark,
            ))
            .child(top_apps_donut(s, dark))
            .child(power_donut(s, dark)),
    );
    cols
}

fn format_bytes_mem(u: u64, t: u64) -> String {
    format!("{} / {}", crate::engine::fmt_bytes(u as f64), crate::engine::fmt_bytes(t as f64))
}

fn mini_tile(label: &str, value: String, hist: &Hist, m: Metric, dark: bool, max: Option<f64>) -> Div {
    card(dark)
        .child(
            div()
                .flex()
                .justify_between()
                .child(sub(label.to_uppercase()).text_color(t::ink_dim(dark)).font_weight(FontWeight::SEMIBOLD))
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(value)),
        )
        .child(area_chart(&hist.pts, 54.0, m.accent(dark), max))
}

/// Simple conic donut built from wedge polygons.
fn donut(slices: &[(f64, Hsla)], size: f32) -> Canvas<()> {
    let slices: Vec<(f64, Hsla)> = slices.to_vec();
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _| {
            let w: f32 = bounds.size.width.into();
            let h: f32 = bounds.size.height.into();
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let cx = ox + w / 2.0;
            let cy = oy + h / 2.0;
            let r = w.min(h) / 2.0 - 2.0;
            let r_in = r * 0.62;
            let total: f64 = slices.iter().map(|s| s.0).sum::<f64>().max(1.0);
            let mut angle = -std::f64::consts::FRAC_PI_2;
            for (v, color) in &slices {
                let sweep = (*v / total) * std::f64::consts::TAU;
                if sweep <= 0.001 {
                    continue;
                }
                let steps = (sweep / 0.12).ceil().max(2.0) as usize;
                let mut path = Path::new(point(
                    px((cx + r * angle.cos() as f32)),
                    px((cy + r * angle.sin() as f32)),
                ));
                for i in 1..=steps {
                    let a = angle + sweep * i as f64 / steps as f64;
                    path.line_to(point(
                        px((cx + r * a.cos() as f32)),
                        px((cy + r * a.sin() as f32)),
                    ));
                }
                for i in (0..=steps).rev() {
                    let a = angle + sweep * i as f64 / steps as f64;
                    path.line_to(point(
                        px((cx + r_in * a.cos() as f32)),
                        px((cy + r_in * a.sin() as f32)),
                    ));
                }
                angle += sweep;
                window.paint_path(path, *color);
            }
            let _ = size;
        },
    )
    .size(px(size))
}

fn donut_card(label: &str, slices: Vec<(String, f64, Hsla)>, center: String, dark: bool) -> Div {
    let total: f64 = slices.iter().map(|s| s.1).sum::<f64>().max(1.0);
    let legend = slices
        .iter()
        .map(|(n, v, c)| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().size(px(8.)).rounded_full().bg(*c))
                .child(div().flex_1().text_xs().child(n.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(t::ink_dim(dark))
                        .child(format!("{:.0}%", *v / total * 100.0)),
                )
        })
        .collect::<Vec<_>>();
    card(dark)
        .items_center()
        .child(sub(label.to_uppercase()).text_color(t::ink_dim(dark)).font_weight(FontWeight::SEMIBOLD))
        .child(
            div()
                .relative()
                .child(donut(
                    &slices.iter().map(|(_, v, c)| (*v, *c)).collect::<Vec<_>>(),
                    110.0,
                ))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_center()
                                .child(center),
                        ),
                ),
        )
        .child(div().w_full().flex().flex_col().gap_1().children(legend))
}

fn top_apps_donut(s: &Snapshot, dark: bool) -> Div {
    let palette = [
        Metric::Memory.accent(dark),
        Metric::Cpu.accent(dark),
        Metric::Disk.accent(dark),
        Metric::Network.accent(dark),
        Metric::Gpu.accent(dark),
        t::card_hi(dark),
    ];
    let mut top: Vec<&AppGroup> = s.apps.iter().collect();
    top.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    let mut slices: Vec<(String, f64, Hsla)> = Vec::new();
    for (i, a) in top.iter().take(5).enumerate() {
        slices.push((a.name.clone(), a.mem_bytes as f64, palette[i]));
    }
    let other: u64 = top.iter().skip(5).map(|a| a.mem_bytes).sum();
    if other > 0 {
        slices.push(("Other".to_string(), other as f64, palette[5]));
    }
    donut_card(
        "Memory by App",
        slices,
        fmt_bytes(top.first().map(|a| a.mem_bytes).unwrap_or(0) as f64),
        dark,
    )
}

fn power_donut(s: &Snapshot, dark: bool) -> Div {
    let palette = [
        Metric::Battery.accent(dark),
        Metric::Cpu.accent(dark),
        Metric::Disk.accent(dark),
        Metric::Network.accent(dark),
        Metric::Gpu.accent(dark),
        t::card_hi(dark),
    ];
    let mut top: Vec<&AppGroup> = s.apps.iter().collect();
    top.sort_by(|a, b| b.power_w.partial_cmp(&a.power_w).unwrap());
    let mut slices = Vec::new();
    for (i, a) in top.iter().take(5).enumerate() {
        if a.power_w > 0.01 {
            slices.push((a.name.clone(), a.power_w, palette[i]));
        }
    }
    if slices.is_empty() {
        slices.push(("System".to_string(), s.power_w.max(0.1), palette[5]));
    }
    donut_card("Power by App", slices, fmt_watts(s.power_w), dark)
}

// ---------- metric tabs ----------

fn apps_rows<'a>(
    s: &Snapshot,
    dark: bool,
    accent: Hsla,
    val: impl Fn(&AppGroup) -> (String, f64),
    max_v: f64,
) -> Vec<Div> {
    s.apps
        .iter()
        .take(30)
        .enumerate()
        .map(|(i, a)| {
            let (v, raw) = val(a);
            app_row(i + 1, a, v, raw / max_v.max(1e-6), accent, dark)
        })
        .collect()
}

fn cpu_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Cpu;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap());
    let max = rows.first().map(|a| a.cpu_pct).unwrap_or(1.0).max(1.0);
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "CPU Usage",
            format!("{:.0}%", s.cpu_pct),
            format!("{} · {} P + {} E cores", s.chip.brand, s.chip.p_cores, s.chip.e_cores),
            &s.cpu_hist,
            m.accent(dark),
            dark,
            vec![
                ("Load average".to_string(), format!("{:.2} {:.2} {:.2}", s.load_avg[0], s.load_avg[1], s.load_avg[2])),
                ("Processes".to_string(), format!("{}", s.proc_count)),
                ("Uptime".to_string(), crate::engine::fmt_up(s.uptime.as_secs_f64())),
            ],
            Some(100.0),
        ))
        .child(
            div().grid().grid_cols(4).gap_3()
                .child(stat_card("Load 1m", format!("{:.2}", s.load_avg[0]), None, dark))
                .child(stat_card("Load 5m", format!("{:.2}", s.load_avg[1]), None, dark))
                .child(stat_card("Load 15m", format!("{:.2}", s.load_avg[2]), None, dark))
                .child(stat_card("Cores", format!("{}", s.chip.cores), None, dark)),
        )
        .child(section_header("App", "CPU", dark))
        .child(list_card(
            rows.iter()
                .take(30)
                .enumerate()
                .map(|(i, a)| app_row(i + 1, a, format!("{:.0}%", a.cpu_pct), a.cpu_pct / max, m.accent(dark), dark))
                .collect(),
            dark,
        ))
}

fn memory_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Memory;
    let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    let max = rows.first().map(|a| a.mem_bytes).unwrap_or(1) as f64;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "Memory",
            format!("{:.0}%", mem_pct),
            format!("{} used of {}", fmt_bytes(s.mem.used as f64), fmt_bytes(s.mem.total as f64)),
            &s.mem_hist,
            m.accent(dark),
            dark,
            vec![
                ("App memory".into(), fmt_bytes(s.mem.app as f64)),
                ("Wired".into(), fmt_bytes(s.mem.wired as f64)),
                ("Compressed".into(), fmt_bytes(s.mem.compressed as f64)),
                ("Cached".into(), fmt_bytes(s.mem.cached as f64)),
            ],
            Some(100.0),
        ))
        .child(
            div().grid().grid_cols(4).gap_3()
                .child(stat_card("App", fmt_bytes(s.mem.app as f64), None, dark))
                .child(stat_card("Wired", fmt_bytes(s.mem.wired as f64), None, dark))
                .child(stat_card("Compressed", fmt_bytes(s.mem.compressed as f64), None, dark))
                .child(stat_card(
                    "Swap",
                    if s.mem.swap_total > 0 {
                        format!("{} / {}", fmt_bytes(s.mem.swap_used as f64), fmt_bytes(s.mem.swap_total as f64))
                    } else {
                        "None".into()
                    },
                    None,
                    dark,
                )),
        )
        .child(section_header("App", "Memory", dark))
        .child(list_card(
            rows.iter()
                .take(30)
                .enumerate()
                .map(|(i, a)| {
                    app_row(i + 1, a, fmt_bytes(a.mem_bytes as f64), a.mem_bytes as f64 / max, m.accent(dark), dark)
                })
                .collect(),
            dark,
        ))
}

fn disk_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Disk;
    let used_pct = s.disk.used as f64 / s.disk.total.max(1) as f64 * 100.0;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| (b.disk_read_rate + b.disk_write_rate).partial_cmp(&(a.disk_read_rate + a.disk_write_rate)).unwrap());
    let max = rows.first().map(|a| a.disk_read_rate + a.disk_write_rate).unwrap_or(1.0).max(1.0);
    let mut combined = s.disk_r_hist.pts.clone();
    for (i, v) in combined.iter_mut().enumerate() {
        *v += s.disk_w_hist.pts.get(i).copied().unwrap_or(0.0);
    }
    let hist = Hist { pts: combined, max_len: 360 };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "Disk Activity",
            fmt_rate(s.disk.read_rate + s.disk.write_rate),
            format!("{} used of {} · {} volumes", fmt_bytes(s.disk.used as f64), fmt_bytes(s.disk.total as f64), s.disk.volume_count),
            &hist,
            m.accent(dark),
            dark,
            vec![
                ("Read".into(), fmt_rate(s.disk.read_rate)),
                ("Write".into(), fmt_rate(s.disk.write_rate)),
                ("Free".into(), fmt_bytes(s.disk.free as f64)),
                ("Used".into(), format!("{:.0}%", used_pct)),
            ],
            None,
        ))
        .child(
            div().grid().grid_cols(4).gap_3()
                .child(stat_card("Read", fmt_rate(s.disk.read_rate), None, dark))
                .child(stat_card("Write", fmt_rate(s.disk.write_rate), None, dark))
                .child(stat_card("Free", fmt_bytes(s.disk.free as f64), None, dark))
                .child(stat_card("Volumes", format!("{}", s.disk.volume_count), None, dark)),
        )
        .child(section_header("App", "Disk Activity", dark))
        .child(list_card(
            rows.iter()
                .take(30)
                .enumerate()
                .map(|(i, a)| {
                    let r = a.disk_read_rate + a.disk_write_rate;
                    app_row(i + 1, a, fmt_rate(r), r / max, m.accent(dark), dark)
                })
                .collect(),
            dark,
        ))
}

fn network_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Network;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| (b.net_in_rate + b.net_out_rate).partial_cmp(&(a.net_in_rate + a.net_out_rate)).unwrap());
    let max = rows.first().map(|a| a.net_in_rate + a.net_out_rate).unwrap_or(1.0).max(1.0);
    let mut combined = s.net_in_hist.pts.clone();
    for (i, v) in combined.iter_mut().enumerate() {
        *v += s.net_out_hist.pts.get(i).copied().unwrap_or(0.0);
    }
    let hist = Hist { pts: combined, max_len: 360 };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "Network",
            fmt_rate(s.net.rate_in + s.net.rate_out),
            format!("{} · {}", s.net.kind, s.net.primary),
            &hist,
            m.accent(dark),
            dark,
            vec![
                ("Download".into(), fmt_rate(s.net.rate_in)),
                ("Upload".into(), fmt_rate(s.net.rate_out)),
                ("Peak in".into(), fmt_rate(s.net.peak_in)),
                ("Peak out".into(), fmt_rate(s.net.peak_out)),
            ],
            None,
        ))
        .child(
            div().grid().grid_cols(4).gap_3()
                .child(stat_card("Download", fmt_rate(s.net.rate_in), None, dark))
                .child(stat_card("Upload", fmt_rate(s.net.rate_out), None, dark))
                .child(stat_card("Total in", fmt_bytes(s.net.total_in as f64), None, dark))
                .child(stat_card("Total out", fmt_bytes(s.net.total_out as f64), None, dark)),
        )
        .child(section_header("App", "Network", dark))
        .child(list_card(
            rows.iter()
                .take(30)
                .enumerate()
                .map(|(i, a)| {
                    let r = a.net_in_rate + a.net_out_rate;
                    app_row(i + 1, a, fmt_rate(r), r / max, m.accent(dark), dark)
                })
                .collect(),
            dark,
        ))
}

fn gpu_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Gpu;
    let supported = s.gpu.supported;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "GPU",
            s.gpu.utilization.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "—".into()),
            if supported { "Apple GPU".into() } else { "Not available on this Mac".into() },
            &s.gpu_hist,
            m.accent(dark),
            dark,
            vec![
                ("Utilization".into(), s.gpu.utilization.map(|u| format!("{:.0}%", u)).unwrap_or("—".into())),
                ("GPU memory".into(), s.gpu.mem_in_use.map(|b| fmt_bytes(b as f64)).unwrap_or("—".into())),
                ("Supported".into(), if supported { "Yes".into() } else { "No".into() }),
            ],
            Some(100.0),
        ))
        .when(!supported, |d| {
            d.child(
                card(dark).child(
                    div()
                        .text_sm()
                        .text_color(t::ink_dim(dark))
                        .child("GPU utilization counters aren't exposed in virtual machines. On real Apple Silicon hardware this tab shows utilization and GPU memory."),
                ),
            )
        })
}

fn neural_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Neural;
    let cores = s.chip.neural_cores;
    let supported = cores > 0;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "Neural Engine",
            s.neural.map(|v| format!("{:.0}%", v)).unwrap_or_else(|| "—".into()),
            if supported {
                format!("Apple Neural Engine · {} cores", cores)
            } else {
                "Not available on this Mac".into()
            },
            &s.neural_hist,
            m.accent(dark),
            dark,
            vec![
                ("Cores".into(), if supported { format!("{}", cores) } else { "—".into() }),
                ("Thermal".into(), s.thermal.clone()),
                ("State".into(), if supported { "Ready".into() } else { "Unavailable".into() }),
            ],
            Some(100.0),
        ))
        .when(!supported, |d| {
            d.child(card(dark).child(div().text_sm().text_color(t::ink_dim(dark)).child(
                "Neural Engine utilization needs real Apple Silicon hardware with IOReport access.",
            )))
        })
}

fn battery_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Battery;
    let b = &s.battery;
    let status = if b.is_charging {
        "Charging"
    } else if b.on_battery {
        "On battery"
    } else if b.ac_power {
        "On power adapter"
    } else {
        "No battery"
    };
    let main = if b.present {
        b.percent.map(|p| format!("{:.0}%", p)).unwrap_or("—".into())
    } else {
        "—".into()
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            "Battery",
            main,
            status.into(),
            &s.batt_hist,
            m.accent(dark),
            dark,
            vec![
                ("Power".into(), b.power_w.map(fmt_watts).unwrap_or("—".into())),
                ("Health".into(), b.health.map(|h| format!("{:.0}%", h)).unwrap_or("—".into())),
                ("Cycles".into(), b.cycles.map(|c| format!("{}", c)).unwrap_or("—".into())),
                ("Time left".into(), b.minutes_remaining.map(|m| fmt_up(m as f64 * 60.0)).unwrap_or("—".into())),
            ],
            Some(100.0),
        ))
        .child(
            div().grid().grid_cols(4).gap_3()
                .child(stat_card("Status", status.into(), None, dark))
                .child(stat_card("System power", fmt_watts(s.power_w), None, dark))
                .child(stat_card("Temperature", b.temperature_c.map(|t| format!("{:.0}°C", t)).unwrap_or("—".into()), None, dark))
                .child(stat_card("Cycles", b.cycles.map(|c| format!("{}", c)).unwrap_or("—".into()), None, dark)),
        )
        .child(section_header("App", "Power usage", dark))
        .child(list_card(
            {
                let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
                rows.sort_by(|a, b| b.power_w.partial_cmp(&a.power_w).unwrap());
                let max = rows.first().map(|a| a.power_w).unwrap_or(1.0).max(0.01);
                rows.iter()
                    .take(30)
                    .enumerate()
                    .map(|(i, a)| app_row(i + 1, a, fmt_watts(a.power_w), a.power_w / max, m.accent(dark), dark))
                    .collect()
            },
            dark,
        ))
        .when(!b.present, |d| {
            d.child(card(dark).child(div().text_sm().text_color(t::ink_dim(dark)).child(
                "This Mac has no battery — showing per-app power draw instead.",
            )))
        })
}

// ---------- sound ----------

fn level_meter(frac: f32, color: Hsla, dark: bool, bars: usize) -> Div {
    let mut row = div().flex().items_end().gap(px(2.)).h(px(14.));
    for i in 0..bars {
        let on = (i as f32 / bars as f32) < frac;
        let h = 4.0 + (i as f32 / bars.max(1) as f32) * 10.0;
        row = row.child(
            div()
                .w(px(3.))
                .h(px(h))
                .rounded_sm()
                .bg(if on { color } else { t::card_hi(dark) }),
        );
    }
    row
}

fn sound_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Sound;
    let vol = s.audio.device_volume;
    let accent = m.accent(dark);
    let mut rows = Vec::new();
    for (i, a) in s.audio.apps.iter().enumerate() {
        let status = if a.running_output {
            "Playing"
        } else if a.running_input {
            "Recording"
        } else {
            "Idle"
        };
        let fake = AppGroup {
            name: a.name.clone(),
            icon_path: a.icon_path.clone(),
            pids: vec![a.pid],
            ..Default::default()
        };
        rows.push(
            div()
                .flex()
                .items_center()
                .gap_2()
                .py(px(5.))
                .px_3()
                .child(
                    div().w(px(16.)).text_xs().text_color(t::ink_dim(dark)).child(format!("{}", i + 1)),
                )
                .child(app_icon(&fake, dark))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(a.name.clone()))
                        .child(div().text_xs().text_color(t::ink_dim(dark)).child(status)),
                )
                .child(level_meter(if status == "Playing" { a.level } else { 0.0 }, accent, dark, 8))
                .child(
                    div()
                        .w(px(70.))
                        .text_right()
                        .text_sm()
                        .child(if a.muted { "Muted".into() } else { format!("{:.0}%", vol * 100.0) }),
                ),
        );
    }
    if rows.is_empty() {
        rows.push(
            div()
                .p_4()
                .text_sm()
                .text_color(t::ink_dim(dark))
                .child("No apps are using audio right now."),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            card(dark)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(icon_img("icons/volume-2.svg", 22.0, accent))
                        .child(
                            div()
                                .flex_1()
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Output"))
                                .child(sub(s.audio.device_name.clone()).text_color(t::ink_dim(dark))),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if s.audio.device_muted { "Muted".into() } else { format!("{:.0}%", vol * 100.0) }),
                        ),
                )
                .child(
                    div()
                        .h(px(6.))
                        .w_full()
                        .rounded_full()
                        .bg(t::card_hi(dark))
                        .child(
                            div()
                                .h_full()
                                .w(relative(vol.clamp(0.0, 1.0)))
                                .rounded_full()
                                .bg(accent),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(s.audio.devices.iter().map(|d| {
                            let sel = d == &s.audio.device_name;
                            div()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .text_xs()
                                .bg(if sel { accent.opacity(0.18) } else { t::card_hi(dark) })
                                .child(d.clone())
                        })),
                ),
        )
        .child(section_header("App", "Volume", dark))
        .child(list_card(rows, dark))
}

// ---------- bluetooth ----------

fn bt_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Bluetooth;
    let n = s.bt.devices.len();
    let lowest = s
        .bt
        .devices
        .iter()
        .filter_map(|d| d.battery.map(|b| (d.name.clone(), b)))
        .min_by(|a: &(String, f64), b: &(String, f64)| a.1.partial_cmp(&b.1).unwrap());
    let mut c = div().flex().flex_col().gap_3().child(
        card(dark)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon_img("icons/bluetooth.svg", 22.0, m.accent(dark)))
                    .child(
                        div().flex_1().child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if n == 0 { "No devices connected".into() } else { format!("{} connected", n) }),
                        )
                        .when_some(lowest, |d, (name, b)| {
                            d.child(sub(format!("{} has the lowest battery", name)).text_color(t::ink_dim(dark)))
                        }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(m.accent(dark))
                            .child("Bluetooth Settings"),
                    ),
            ),
    );
    if s.bt.devices.is_empty() {
        c = c.child(card(dark).child(
            div()
                .text_sm()
                .text_color(t::ink_dim(dark))
                .child(if s.bt.enabled {
                    "Paired devices will appear here with their battery level.".to_string()
                } else {
                    "Bluetooth is off or unavailable on this Mac.".to_string()
                }),
        ));
    } else {
        for d in &s.bt.devices {
            c = c.child(
                card(dark).p_3().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(icon_img("icons/bluetooth.svg", 16.0, m.accent(dark)))
                        .child(
                            div()
                                .flex_1()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(d.name.clone()))
                                .child(sub(d.kind.clone()).text_color(t::ink_dim(dark))),
                        )
                        .child(
                            div()
                                .w(px(70.))
                                .text_right()
                                .text_sm()
                                .child(d.battery.map(|b| format!("{:.0}%", b)).unwrap_or("—".into())),
                        )
                        .child(level_meter((d.battery.unwrap_or(0.0) / 100.0) as f32, m.accent(dark), dark, 8)),
                ),
            );
            if d.second_battery.is_some() || d.case_battery.is_some() {
                c = c.child(
                    card(dark).p_2().child(
                        div()
                            .flex()
                            .gap_4()
                            .px_2()
                            .children(
                                [("Left", d.battery), ("Right", d.second_battery), ("Case", d.case_battery)]
                                    .iter()
                                    .filter(|(_, b)| b.is_some())
                                    .map(|(n, b)| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(sub(n.to_string()).text_color(t::ink_dim(dark)))
                                            .child(div().text_xs().child(format!("{:.0}%", b.unwrap())))
                                    }),
                            ),
                    ),
                );
            }
        }
    }
    c
}

// ---------- projects ----------

fn projects_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Projects;
    let accent = m.accent(dark);
    let total: usize = s.projects.iter().map(|g| g.servers.len()).sum();
    let all_idle = s.projects.iter().all(|g| g.idle) && total > 0;
    let mut c = div().flex().flex_col().gap_3();
    if total > 0 {
        c = c.child(
            card(dark)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(icon_img("icons/folder-code.svg", 20.0, accent))
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .child(if all_idle {
                                    format!("{} dev servers are running but idle", total)
                                } else {
                                    format!("{} dev servers running", total)
                                }),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(t::warn(dark).opacity(0.15))
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t::warn(dark))
                                .id("stop-all")
                                .cursor_pointer()
                                .child("Stop All…")
                                .on_click(|_, _, _| {
                                    for g in latest().projects {
                                        for s in g.servers {
                                            crate::projects::kill_pid(s.pid);
                                        }
                                    }
                                }),
                        ),
                ),
        );
        for g in &s.projects {
            c = c.child(
                card(dark)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon_img("icons/folder.svg", 16.0, accent))
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(g.name.clone()))
                            .child(sub(g.dir.clone()).text_color(t::ink_dim(dark))),
                    )
                    .children(g.servers.iter().map(|srv| {
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .py(px(4.))
                            .px_1()
                            .child(
                                div()
                                    .w(px(120.))
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_sm()
                                    .whitespace_nowrap()
                                    .child(srv.name.clone()),
                            )
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded_sm()
                                    .bg(t::card_hi(dark))
                                    .text_xs()
                                    .child(srv.runtime.clone()),
                            )
                            .child(
                                div()
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded_sm()
                                    .bg(accent.opacity(0.15))
                                    .text_xs()
                                    .text_color(accent)
                                    .child(format!(":{}", srv.port)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(if srv.status.starts_with("idle") || srv.status.contains("barely") {
                                        t::ink_dim(dark)
                                    } else {
                                        t::ok(dark)
                                    })
                                    .child(srv.status.clone()),
                            )
                            .child(
                                div().w(px(70.)).text_right().text_xs().child(fmt_bytes(srv.mem_bytes as f64)),
                            )
                            .child({
                                let pid = srv.pid;
                                div()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_md()
                                    .bg(t::card_hi(dark))
                                    .text_xs()
                                    .cursor_pointer()
                                    .id(SharedString::from(format!("stop-{}", pid)))
                                    .child("Stop")
                                    .on_click(move |_, _, _| crate::projects::kill_pid(pid))
                            })
                    })),
            );
        }
    } else {
        c = c.child(card(dark).child(
            div()
                .text_sm()
                .text_color(t::ink_dim(dark))
                .child("No dev servers detected. Processes listening on TCP ports in ~/… project folders show up here."),
        ));
    }
    c
}
