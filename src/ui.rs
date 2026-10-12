//! The Tidegauge panel: top tab bar, card-based content.

#![allow(dead_code)]

use crate::engine::*;
use crate::locale::text as l;
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
    Battery,
    Sound,
    Bluetooth,
    Projects,
    History,
}

impl Tab {
    const ALL: [Tab; 11] = [
        Tab::Overview,
        Tab::Cpu,
        Tab::Memory,
        Tab::Disk,
        Tab::Network,
        Tab::Gpu,
        Tab::Battery,
        Tab::Sound,
        Tab::Bluetooth,
        Tab::Projects,
        Tab::History,
    ];
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Disk => "disk",
            Self::Network => "network",
            Self::Gpu => "gpu",
            Self::Battery => "battery",
            Self::Sound => "sound",
            Self::Bluetooth => "bluetooth",
            Self::Projects => "projects",
            Self::History => "history",
        }
    }
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == key)
    }
    fn visible() -> Vec<Self> {
        std::iter::once(Self::Overview)
            .chain(
                crate::settings::Settings::load()
                    .dashboard_tabs
                    .iter()
                    .filter_map(|k| Self::from_key(k)),
            )
            .collect()
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Tab::Overview => l("Overview", "Oversikt"),
            Tab::Cpu => "CPU",
            Tab::Memory => l("Memory", "Minne"),
            Tab::Disk => "Disk",
            Tab::Network => l("Network", "Nettverk"),
            Tab::Gpu => "GPU",
            Tab::Battery => l("Battery", "Batteri"),
            Tab::Sound => l("Sound", "Lyd"),
            Tab::Bluetooth => "Bluetooth",
            Tab::Projects => l("Projects", "Prosjekter"),
            Tab::History => l("History", "Historikk"),
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
            Tab::Battery => "icons/battery-charging.svg",
            Tab::Sound => "icons/volume-2.svg",
            Tab::Bluetooth => "icons/bluetooth.svg",
            Tab::Projects => "icons/folder-code.svg",
            Tab::History => "icons/clock.svg",
        }
    }
    fn metric(self) -> Option<Metric> {
        match self {
            Tab::Cpu => Some(Metric::Cpu),
            Tab::Memory => Some(Metric::Memory),
            Tab::Disk => Some(Metric::Disk),
            Tab::Network => Some(Metric::Network),
            Tab::Gpu => Some(Metric::Gpu),
            Tab::Battery => Some(Metric::Battery),
            Tab::Sound => Some(Metric::Sound),
            Tab::Bluetooth => Some(Metric::Bluetooth),
            Tab::Projects => Some(Metric::Projects),
            Tab::Overview | Tab::History => None,
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
    pub show_settings: bool,
    pub show_processes: bool,
    pub show_tools: bool,
    pub tools: crate::tools_panel::State,
    tools_scroll: ScrollHandle,
    process_scroll: ScrollHandle,
    pub only_remaining: bool,
    pub expanded_process_group: Option<String>,
    pub process_pending: bool,
    pub process_message: Option<String>,
    pub process_failed: bool,
    pub process_signing: crate::process_signing::State,
    pub settings_error: Option<String>,
    pub selected_disk: Option<String>,
    pub project_error: Option<String>,
    pub project_stopping: bool,
    pub project_stop_failed: bool,
    pub audio_error: Option<String>,
    pub audio_pending: bool,
    pub history_request: crate::history_browser::Request,
    pub history_data: Option<Result<crate::history_browser::Data, String>>,
    pub history_loading: bool,
    pub history_generation: u64,
    pub history_app_picker: bool,
    tab_scrolls: Vec<ScrollHandle>,
    settings_scroll: ScrollHandle,
}

impl TideGaugeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::appearance::apply(window, crate::settings::Settings::load().dark);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let v = TideGaugeView {
            tab: Tab::Overview,
            dark: matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            scroll: ScrollHandle::new(),
            focus,
            snap: latest(),
            window: None,
            show_settings: false,
            show_processes: false,
            show_tools: false,
            tools: crate::tools_panel::State::default(),
            tools_scroll: ScrollHandle::new(),
            process_scroll: ScrollHandle::new(),
            only_remaining: true,
            expanded_process_group: None,
            process_pending: false,
            process_message: None,
            process_failed: false,
            process_signing: crate::process_signing::State::default(),
            settings_error: None,
            selected_disk: None,
            project_error: None,
            project_stopping: false,
            project_stop_failed: false,
            audio_error: None,
            audio_pending: false,
            history_request: crate::history_browser::Request::default(),
            history_data: None,
            history_loading: false,
            history_generation: 0,
            history_app_picker: false,
            tab_scrolls: Tab::ALL.iter().map(|_| ScrollHandle::new()).collect(),
            settings_scroll: ScrollHandle::new(),
        };
        // 1s refresh loop
        cx.spawn(
            async move |this: WeakEntity<TideGaugeView>, cx: &mut AsyncApp| loop {
                smol::Timer::after(std::time::Duration::from_millis(1000)).await;
                let _ = cx.update(|cx: &mut App| {
                    let _ = this.update(
                        cx,
                        |v: &mut TideGaugeView, cx: &mut Context<TideGaugeView>| {
                            v.snap = latest();
                            cx.notify();
                        },
                    );
                });
            },
        )
        .detach();
        v
    }
}

// ---------- small builders ----------

/// Section/field caption: uppercase 10 px semibold in `ink_dim`. Sentences
/// and disclaimers use `sub()` at 12 px instead.
fn caption(text: &str, dark: bool) -> Div {
    div()
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t::ink_dim(dark))
        .child(text.to_uppercase())
}

pub(crate) fn sub(text: impl Into<String>) -> Div {
    div().text_xs().child(text.into())
}

fn icon_img(path: &'static str, size: f32, color: Hsla) -> impl IntoElement {
    svg().path(path).size(px(size)).text_color(color)
}

fn app_icon(app: &AppGroup, dark: bool) -> impl IntoElement {
    if let Some(path) = &app.icon_path {
        if let Some(file) = crate::icons::cached_icon_for_bundle(path) {
            return div()
                .size(px(20.))
                .rounded_sm()
                .overflow_hidden()
                .child(img(file).size(px(20.)).rounded_sm());
        }
        crate::icons::request_icon_for_bundle(path);
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
        .text_color(t::on_accent(dark))
        .bg(t::accent(dark))
        .child(initial)
}

/// Vertical scale for `area_chart`. Values map into a plot inset from the
/// top and bottom edges so a maxed-out or constant series still reads as a
/// stroked line instead of a solid block.
struct ChartScale {
    max: f64,
    top: f32,
    bottom: f32,
}

impl ChartScale {
    /// A `max_hint` pins the scale (e.g. a 0–100 percentage); without one the
    /// scale sits ~10% above the series maximum so the highest point keeps
    /// visible headroom. Non-finite or non-positive hints and data fall back
    /// to a unit scale.
    fn new(data_max: f64, max_hint: Option<f64>) -> Self {
        let data_max = if data_max.is_finite() { data_max.max(0.) } else { 0. };
        match max_hint {
            Some(hint) if hint.is_finite() && hint > 0. => {
                Self { max: hint.max(data_max), top: 7., bottom: 4. }
            }
            _ => Self { max: (data_max * 1.1).max(1.0), top: 5., bottom: 4. },
        }
    }

    /// Y position inside a `height`-pixel plot, measured from its top edge.
    fn y(&self, value: f64, height: f32) -> f32 {
        let inner = (height - self.top - self.bottom).max(1.);
        height - self.bottom - ((value / self.max).clamp(0., 1.) as f32) * inner
    }
}

/// Area+line chart drawn with canvas: low-opacity fill under the series and a
/// full-opacity stroke on top, so a full or flat series stays legible. Empty
/// series draw nothing; a single point draws a short tick; non-finite samples
/// become gaps rather than fabricated values.
fn area_chart(pts: &[f64], height: f32, color: Hsla, max_hint: Option<f64>) -> Canvas<()> {
    let pts = pts.to_vec();
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _| {
            let w: f32 = bounds.size.width.into();
            let h: f32 = bounds.size.height.into();
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            if pts.is_empty() || w < 4.0 {
                return;
            }
            let n = pts.len().min(240);
            let tail = &pts[pts.len() - n..];
            let data_max = tail.iter().copied().filter(|v| v.is_finite()).fold(0.0f64, f64::max);
            let scale = ChartScale::new(data_max, max_hint);
            let y_of = |v: f64| -> f32 { oy + scale.y(v, h) };

            if n == 1 {
                // A lone sample has no interval to span; draw it as a tick.
                if tail[0].is_finite() {
                    let yc = y_of(tail[0]);
                    let xc = ox + w / 2.0;
                    let mut tick = Path::new(point(px(xc - 3.0), px(yc - 0.8)));
                    tick.line_to(point(px(xc + 3.0), px(yc - 0.8)));
                    tick.line_to(point(px(xc + 3.0), px(yc + 0.8)));
                    tick.line_to(point(px(xc - 3.0), px(yc + 0.8)));
                    window.paint_path(tick, color);
                }
                return;
            }

            let px_per = (w - 4.0) / (n - 1).max(1) as f32;

            // Fill each interval with a convex trapezoid. A single concave
            // area path would be triangulated across the chart by GPUI.
            for i in 1..n {
                if !(tail[i - 1].is_finite() && tail[i].is_finite()) {
                    continue;
                }
                let x0 = ox + 2.0 + (i - 1) as f32 * px_per;
                let x1 = ox + 2.0 + i as f32 * px_per;
                let mut area = Path::new(point(px(x0), px(oy + h - 2.0)));
                area.line_to(point(px(x0), px(y_of(tail[i - 1]))));
                area.line_to(point(px(x1), px(y_of(tail[i]))));
                area.line_to(point(px(x1), px(oy + h - 2.0)));
                area.line_to(point(px(x0), px(oy + h - 2.0)));
                window.paint_path(area, color.opacity(0.18));
            }

            // stroke as thin quads (each segment its own sub-path)
            let mut stroke = Path::new(point(px(ox + 2.0), px(y_of(tail[0]))));
            for i in 1..n {
                if !(tail[i - 1].is_finite() && tail[i].is_finite()) {
                    continue;
                }
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
        .px_4()
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
                            format!("{} {}", app.pids.len(), l("processes", "prosesser"))
                        } else {
                            l("1 process", "1 prosess").into()
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

pub(crate) fn card(dark: bool) -> Div {
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
            div()
                .flex()
                .items_center()
                .gap_6()
                .child(
                    div()
                        .w(px(240.))
                        .flex_shrink_0()
                        .child(
                            div()
                                .text_size(px(10.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(color)
                                .child(label.to_uppercase()),
                        )
                        .child(
                            div()
                                .text_size(px(42.))
                                .font_weight(FontWeight::BOLD)
                                .child(value),
                        )
                        .child(sub(sublabel).text_color(t::ink_dim(dark))),
                )
                .when(!hist.pts.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(t::ink_dim(dark))
                                    .child(l("Recent samples", "Siste målinger"))
                                    .child(max_hint.map(|v| format!("0–{v:.0}%")).unwrap_or_else(
                                        || l("Auto scale", "Automatisk skala").into(),
                                    )),
                            )
                            .child(area_chart(&hist.pts, 130.0, color, max_hint))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(t::ink_dim(dark))
                                    .child(l("Older", "Eldre"))
                                    .child(l("Now", "Nå")),
                            ),
                    )
                }),
        )
        .when(!footer.is_empty(), |d| {
            d.child(
                div()
                    .flex()
                    .gap_6()
                    .children(footer.into_iter().map(|(k, v)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(t::ink_dim(dark))
                                    .child(k),
                            )
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(v))
                    })),
            )
        })
}

fn stat_card(label: &str, value: String, color: Option<Hsla>, dark: bool) -> Div {
    card(dark)
        .child(caption(label, dark))
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
        .px_4()
        .child(caption(left, dark))
        .child(
            div()
                .text_size(px(10.))
                .text_color(t::ink_dim(dark))
                .child(right.to_string()),
        )
}

struct FooterAction {
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    tab_index: isize,
    selected: bool,
}

/// One shared control style for the footer actions: icon + label on a
/// bordered, transparent-at-rest button with the metric accent on focus.
fn footer_button(
    action_def: FooterAction,
    accent: Hsla,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let action = std::rc::Rc::new(action);
    let keyboard = action.clone();
    div()
        .id(action_def.id)
        .role(Role::Button)
        .aria_label(SharedString::from(action_def.label))
        .tab_index(action_def.tab_index)
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(6.))
        .px_3()
        .rounded(px(8.))
        .border_1()
        .border_color(t::line(dark))
        .when(action_def.selected, |d| d.bg(accent.opacity(0.12)))
        .text_size(px(12.))
        .text_color(t::ink(dark))
        .cursor_pointer()
        .hover(|d| d.bg(t::card_hi(dark)))
        .focus_visible(|d| d.border_color(accent))
        .child(icon_img(action_def.icon, 14., t::ink_dim(dark)))
        .child(action_def.label)
        .on_click(move |_, window, cx| action(window, cx))
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                keyboard(window, cx);
                cx.stop_propagation();
            }
        })
}

fn list_card(rows: Vec<Div>, dark: bool) -> Div {
    card(dark).p_0().py_2().gap_0().children(rows)
}

// ---------- Render ----------

impl Render for TideGaugeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.dark = crate::settings::Settings::load().dark.unwrap_or(matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ));
        let dark = self.dark;
        let s = &self.snap;
        let tab = self.tab;
        let this = cx.entity();
        self.scroll = if self.show_settings {
            self.settings_scroll.clone()
        } else if self.show_tools {
            self.tools_scroll.clone()
        } else if self.show_processes {
            self.process_scroll.clone()
        } else {
            self.tab_scrolls[tab as usize].clone()
        };

        // Neutral chrome with a quiet metric-colored selection.
        let topbar = div()
            .w_full()
            .h(px(64.))
            .flex_shrink_0()
            .pl(px(92.))
            .pr(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("metric-tabs")
                    .role(Role::TabList)
                    .aria_label(l("Metrics", "Målinger"))
                    .tab_group()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .rounded_full()
                    .bg(t::card(dark))
                    .border_1()
                    .border_color(t::line(dark))
                    .p_1()
                    .children(
                        Tab::visible()
                            .into_iter()
                            .enumerate()
                            .map(|(tab_index, tb)| {
                                let active = tb == tab;
                                let accent = tb.metric().unwrap_or(Metric::Cpu).accent(dark);
                                let this = this.clone();
                                let keyboard_target = this.clone();
                                div()
                                    .id(SharedString::from(format!("tab-{}", tb.label())))
                                    .role(Role::Tab)
                                    .aria_label(tb.label())
                                    .aria_selected(active)
                                    .tab_index(tab_index as isize)
                                    .focus_visible(|d| {
                                        d.border_color(accent).bg(accent.opacity(0.12))
                                    })
                                    .on_key_down(move |event, _, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            select_tab(&keyboard_target, tb, cx);
                                            cx.stop_propagation();
                                        }
                                    })
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .px(px(8.))
                                    .py(px(8.))
                                    .border_1()
                                    .border_color(gpui_kit::transparent_black())
                                    .rounded_full()
                                    .cursor_pointer()
                                    .when(active, |d| d.bg(accent.opacity(0.12)).text_color(accent))
                                    .when(!active, |d| {
                                        d.text_color(t::ink_dim(dark))
                                            .hover(|h| h.text_color(t::ink(dark)))
                                    })
                                    .child(icon_img(
                                        tb.icon(),
                                        13.0,
                                        if active { accent } else { t::ink_dim(dark) },
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .line_height(px(13.))
                                            .child(tb.label()),
                                    )
                                    .on_click(move |_, _w, cx| {
                                        select_tab(&this, tb, cx);
                                    })
                            }),
                    ),
            );

        let topbar = if self.show_settings {
            let back = this.clone();
            let keyboard_back = this.clone();
            div()
                .h(px(64.))
                .flex_shrink_0()
                .pl(px(104.))
                .pr(px(24.))
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .id("back-to-module")
                        .role(Role::Button)
                        .aria_label(SharedString::from(format!(
                            "{} {}",
                            l("Back to", "Tilbake til"),
                            tab.label()
                        )))
                        .tab_index(0)
                        .rounded_md()
                        .p_2()
                        .cursor_pointer()
                        .hover(|d| d.bg(t::card_hi(dark)))
                        .focus_visible(|d| d.bg(t::card_hi(dark)))
                        .child(icon_img("icons/arrow-left.svg", 16., t::ink(dark)))
                        .on_key_down(move |event, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                select_tab(&keyboard_back, tab, cx);
                                cx.stop_propagation();
                            }
                        })
                        .on_click(move |_, _, cx| select_tab(&back, tab, cx)),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(l("Settings", "Innstillinger")),
                )
                .into_any_element()
        } else {
            topbar.into_any_element()
        };

        let accent = tab.metric().unwrap_or(Metric::Cpu).accent(dark);
        let settings_target = this.clone();
        let process_target = this.clone();
        let tools_target = this.clone();
        let footer = div()
            .w_full()
            .px(px(20.))
            .py_2()
            .flex()
            .items_center()
            .child(footer_button(
                FooterAction {
                    id: "dashboard-settings",
                    icon: "icons/settings.svg",
                    label: l("Settings", "Innstillinger"),
                    tab_index: 11,
                    selected: self.show_settings,
                },
                accent,
                dark,
                move |_, cx| open_settings(&settings_target, cx),
            ))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(t::ink_dim(dark))
                            .child(format!(
                                "{} {} · {} {}",
                                s.app_count,
                                l("apps", "apper"),
                                s.proc_count,
                                l("processes", "prosesser")
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(footer_button(
                        FooterAction {
                            id: "open-processes",
                            icon: "icons/list.svg",
                            label: l("Processes", "Prosesser"),
                            tab_index: 12,
                            selected: self.show_processes,
                        },
                        accent,
                        dark,
                        move |_, cx| crate::process_panel::open(&process_target, cx),
                    ))
                    .child(footer_button(
                        FooterAction {
                            id: "open-tools",
                            icon: "icons/wrench.svg",
                            label: l("Tools", "Verktøy"),
                            tab_index: 13,
                            selected: self.show_tools,
                        },
                        accent,
                        dark,
                        move |_, cx| crate::tools_panel::open(&tools_target, cx),
                    )),
            );

        let content = div()
            .id("content")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(px(20.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_tab(dark, this.clone(), cx));

        div()
            .id("dashboard")
            .role(Role::Application)
            .aria_label("Tidegauge")
            .tab_group()
            .tab_stop(false)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.platform && event.keystroke.key == "q" {
                    cx.quit();
                    cx.stop_propagation();
                } else if event.keystroke.modifiers.platform && event.keystroke.key == "," {
                    view.show_settings = true;
                    view.scroll = ScrollHandle::new();
                    view.settings_error = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            }))
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

fn open_settings(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.show_tools = false;
        view.show_processes = false;
        view.show_settings = true;
        view.scroll = ScrollHandle::new();
        view.settings_error = None;
        cx.notify();
    });
}

fn select_tab(this: &Entity<TideGaugeView>, tab: Tab, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.show_tools = false;
        v.tab = tab;
        v.show_processes = false;
        v.show_settings = false;
        v.settings_error = None;
        v.scroll = ScrollHandle::new();
        cx.notify();
    });
    if tab == Tab::History {
        crate::controls::refresh_history(this, cx);
    }
}

impl TideGaugeView {
    fn render_tab(
        &self,
        dark: bool,
        this: Entity<TideGaugeView>,
        cx: &mut Context<TideGaugeView>,
    ) -> AnyElement {
        if self.show_settings {
            return settings_panel(dark, this, self.settings_error.as_deref()).into_any_element();
        }
        if self.show_processes {
            return crate::process_panel::panel(self, dark, this).into_any_element();
        }
        if self.show_tools {
            return crate::tools_panel::panel(self, dark, this).into_any_element();
        }
        let s = &self.snap;
        match self.tab {
            Tab::Overview => overview(s, dark, this).into_any_element(),
            Tab::Cpu => cpu_tab(s, dark).into_any_element(),
            Tab::Memory => memory_tab(s, dark).into_any_element(),
            Tab::Disk => disk_tab(s, dark, self.selected_disk.as_deref(), cx).into_any_element(),
            Tab::Network => network_tab(s, dark).into_any_element(),
            Tab::Gpu => gpu_tab(s, dark).into_any_element(),
            Tab::Battery => battery_tab(s, dark).into_any_element(),
            Tab::Sound => div().flex().flex_col().gap_3()
                .when_some(self.audio_error.as_ref(), |d,error|d.child(card(dark).child(sub(error.clone()).text_color(t::warn(dark)))))
                .child(sound_tab(s, dark, this, self.audio_pending)).into_any_element(),
            Tab::Bluetooth => bt_tab(s, dark).into_any_element(),
            Tab::Projects => div()
                .flex()
                .flex_col()
                .gap_3()
                .when_some(self.project_error.as_ref(), |d, error| {
                    d.child(card(dark).child(sub(error.clone()).text_color(if self.project_stop_failed {t::warn(dark)} else {t::ink_dim(dark)})))
                })
                .child(projects_tab(s, dark, this))
                .into_any_element(),
            Tab::History => crate::controls::history_panel(self, dark, this, cx).into_any_element(),
        }
    }
}

pub(crate) fn update_preference(
    this: &Entity<TideGaugeView>,
    change: impl FnOnce(&mut crate::settings::Settings),
    window: &mut Window,
    cx: &mut App,
) {
    let result = crate::settings::mutate(change);
    if result.is_ok() {
        crate::appearance::apply(window, crate::settings::Settings::load().dark);
    }
    this.update(cx, |view, cx| {
        view.settings_error = result.err().map(|_| {
            l(
                "Settings could not be saved. Try again.",
                "Innstillingene kunne ikke lagres. Prøv igjen.",
            )
            .to_string()
        });
        if !Tab::visible().contains(&view.tab) { view.tab = Tab::Overview; }
        cx.notify();
    });
}

fn settings_choice(
    id: String,
    label: &str,
    description: String,
    active: bool,
    index: isize,
    dark: bool,
    this: Entity<TideGaugeView>,
    change: impl Fn(&mut crate::settings::Settings) + 'static,
) -> impl IntoElement {
    let change = std::rc::Rc::new(change);
    let keyboard_change = change.clone();
    let keyboard_target = this.clone();
    let accent = t::accent(dark);
    div()
        .id(SharedString::from(id))
        .role(Role::RadioButton)
        .aria_label(SharedString::from(description))
        .aria_toggled(if active {
            Toggled::True
        } else {
            Toggled::False
        })
        .tab_index(index)
        .rounded(px(8.))
        .px_3()
        .py_2()
        .border_1()
        .border_color(if active { accent } else { t::line(dark) })
        .bg(if active {
            accent.opacity(0.12)
        } else {
            t::card_hi(dark)
        })
        .text_color(if active { accent } else { t::ink_dim(dark) })
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|d| d.text_color(t::ink(dark)))
        .focus_visible(|d| d.border_color(accent))
        .child(label.to_string())
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                update_preference(
                    &keyboard_target,
                    |settings| keyboard_change(settings),
                    window,
                    cx,
                );
                cx.stop_propagation();
            }
        })
        .on_click(move |_, window, cx| {
            update_preference(&this, |settings| change(settings), window, cx)
        })
}

fn set_launch_at_login(enabled: bool, this: &Entity<TideGaugeView>, cx: &mut App) {
    let error = match crate::login::set_enabled(enabled) {
        Ok(status) => {
            crate::settings::mutate(|settings| settings.launch_at_login = status.is_registered())
                .err()
                .map(|_| {
                    l(
                        "The login setting changed, but its local preference could not be saved.",
                        "Påloggingsinnstillingen ble endret, men kunne ikke lagres lokalt.",
                    )
                    .to_string()
                })
        }
        Err(error) => Some(error.to_string()),
    };
    this.update(cx, |view, cx| {
        view.settings_error = error;
        cx.notify();
    });
}

fn login_control(status: crate::login::LoginStatus, dark: bool, this: Entity<TideGaugeView>) -> AnyElement {
    if !status.can_change() {
        return div().text_sm().text_color(t::ink_dim(dark))
            .child(l("Unavailable", "Utilgjengelig")).into_any_element();
    }
    let active = status.is_registered();
    let keyboard_target = this.clone();
    div().id("launch-at-login").role(Role::Switch)
        .aria_label(l("Launch at login", "Start ved pålogging"))
        .aria_toggled(if active { Toggled::True } else { Toggled::False })
        .tab_index(3).px_4().py_2().rounded(px(8.))
        .bg(t::card_hi(dark)).border_1().border_color(t::line(dark))
        .text_sm().text_color(t::ink(dark)).cursor_pointer()
        .child(status.control_label())
        .on_key_down(move |event, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                set_launch_at_login(!active, &keyboard_target, cx);
                cx.stop_propagation();
            }
        })
        .on_click(move |_, _, cx| set_launch_at_login(!active, &this, cx))
        .into_any_element()
}

fn settings_panel(dark: bool, this: Entity<TideGaugeView>, error: Option<&str>) -> Div {
    use crate::settings::{
        localized_metric_label, localized_style_label, Settings, MENU_METRICS, MENU_STYLES,
    };
    let settings = Settings::load();
    let mut panel = div().flex().flex_col().gap_4().child(
        div()
            .text_size(px(24.))
            .font_weight(FontWeight::BOLD)
            .child(l("Settings", "Innstillinger")),
    );
    if let Some(error) = error {
        panel = panel.child(
            div()
                .text_sm()
                .text_color(t::warn(dark))
                .child(error.to_string()),
        );
    }
    let login = crate::login::status();
    panel = panel
        .child(card(dark).flex_row().items_center().justify_between().gap_4()
            .child(div().flex_1()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("Appearance", "Utseende")))
                .child(div().text_xs().text_color(t::ink_dim(dark)).child(l(
                    "Choose a theme, or follow your Mac.", "Velg et tema, eller følg Macens utseende.",
                ))))
            .child(div().id("appearance-options").role(Role::RadioGroup)
                .aria_label(l("Appearance", "Utseende")).tab_group().flex().gap_2()
                .children([(None, l("System", "System")), (Some(false), l("Light", "Lyst")), (Some(true), l("Dark", "Mørkt"))]
                    .into_iter().enumerate().map(|(i, (value, label))| {
                        settings_choice(format!("appearance-{i}"), label, format!("{}: {label}", l("Appearance", "Utseende")),
                            settings.dark == value, i as isize, dark, this.clone(), move |settings| settings.dark = value)
                    }))))
        .child(card(dark).flex_row().items_center().justify_between().gap_4()
            .child(div().flex_1()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("Launch at login", "Start ved pålogging")))
                .child(div().text_xs().text_color(t::ink_dim(dark)).child(login.message())))
            .child(login_control(login, dark, this.clone())))
        .child(crate::controls::settings(dark, this.clone()))
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("Menu bar", "Menylinje")))
        .child(div().text_xs().text_color(t::ink_dim(dark)).child(l(
            "One compact menu-bar item. Add readings below, or keep just the Tidegauge icon.",
            "Ett kompakt menylinjeelement. Legg til målinger nedenfor, eller behold bare Tidegauge-ikonet.",
        )));
    let icon_name = l("Tidegauge icon", "Tidegauge-ikon");
    panel = panel.child(
        card(dark)
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(icon_name),
                    )
                    .child(div().text_xs().text_color(t::ink_dim(dark)).child(l(
                        "Open the app menu with one click.",
                        "Åpne appmenyen med ett klikk.",
                    ))),
            )
            .child(
                div()
                    .id("launcher-options")
                    .role(Role::RadioGroup)
                    .aria_label(l("Tidegauge icon", "Tidegauge-ikon"))
                    .tab_group()
                    .flex()
                    .gap_2()
                    .children(
                        [(true, l("Shown", "Vises")), (false, l("Hidden", "Skjult"))]
                            .into_iter()
                            .enumerate()
                            .map(|(i, (shown, label))| {
                                settings_choice(
                                    format!("launcher-{shown}"),
                                    label,
                                    format!("{icon_name}: {label}"),
                                    settings.show_app_icon == shown,
                                    i as isize,
                                    dark,
                                    this.clone(),
                                    move |settings| settings.show_app_icon = shown,
                                )
                            }),
                    ),
            ),
    );
    let rows = MENU_METRICS
        .iter()
        .enumerate()
        .map(|(row, &(key, _))| {
            let name = localized_metric_label(key).expect("menu metric has a localized label");
            let selected = settings
                .menu_items
                .iter()
                .find(|item| item.metric == key)
                .map(|item| item.style.as_str())
                .unwrap_or("off");
            let choices = std::iter::once(("off", l("Off", "Av"))).chain(MENU_STYLES.iter().map(
                |(style, _)| {
                    (
                        *style,
                        localized_style_label(style).expect("menu style has a localized label"),
                    )
                },
            ));
            div()
                .id(SharedString::from(format!("settings-{key}")))
                .role(Role::RadioGroup)
                .aria_label(name)
                .tab_group()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .py_2()
                .when(row > 0, |d| d.border_t_1().border_color(t::line(dark)))
                .child(
                    div()
                        .w(px(140.))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(name),
                )
                .child(div().flex().gap_2().children(choices.enumerate().map(
                    |(i, (style, label))| {
                        settings_choice(
                            format!("{key}-{style}"),
                            label,
                            format!("{name}: {label}"),
                            selected == style,
                            i as isize,
                            dark,
                            this.clone(),
                            move |settings| {
                                settings.set_menu_style(
                                    key,
                                    if style == "off" { None } else { Some(style) },
                                )
                            },
                        )
                    },
                )))
        })
        .collect::<Vec<_>>();
    panel.child(card(dark).gap_0().py_1().children(rows))
        .child(div().text_xs().text_color(t::ink_dim(dark))
            .child(l(
                "Click the menu-bar item for Open Dashboard, Settings, or Quit. Changes are saved immediately.",
                "Klikk på menylinjeelementet for å åpne oversikten, endre innstillinger eller avslutte. Endringer lagres med en gang.",
            )))
        .child(div().flex().justify_end().gap_3()
            .child({
                let click_target = this.clone();
                let key_target = this.clone();
                div().id("settings-icon-only").role(Role::Button).aria_label(l("Reset to icon only", "Tilbakestill til bare ikon"))
                    .tab_index(29).rounded_md().px_4().py_2().border_1().border_color(t::line(dark))
                    .text_sm().cursor_pointer().focus_visible(|d| d.border_color(t::accent(dark)))
                    .child(l("Reset to icon only", "Tilbakestill til bare ikon"))
                    .on_key_down(move |event, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            update_preference(&key_target, |settings| { settings.show_app_icon = true; settings.menu_items.clear(); }, window, cx);
                            cx.stop_propagation();
                        }
                    })
                    .on_click(move |_, window, cx| update_preference(&click_target, |settings| { settings.show_app_icon = true; settings.menu_items.clear(); }, window, cx))
            })
            .child(div().id("settings-app-menu").role(Role::Button).aria_label(l("App menu", "Appmeny"))
                .tab_index(30).rounded_md().px_4().py_2().border_1().border_color(t::line(dark))
                .text_sm().cursor_pointer().focus_visible(|d| d.border_color(t::accent(dark)))
                .child(l("App menu", "Appmeny"))
                .on_key_down(|event, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        crate::statusbar::SHOW_CONTEXT_MENU.store(true, std::sync::atomic::Ordering::SeqCst);
                        cx.stop_propagation();
                    }
                })
                .on_click(|_, _, _| crate::statusbar::SHOW_CONTEXT_MENU.store(true, std::sync::atomic::Ordering::SeqCst)))
            .child(div().id("settings-quit").role(Role::Button).aria_label(l("Quit Tidegauge", "Avslutt Tidegauge"))
                .tab_index(31).rounded_md().px_4().py_2().bg(t::card_hi(dark))
                .text_sm().cursor_pointer().focus_visible(|d| d.border_color(t::accent(dark)))
                .child(l("Quit Tidegauge", "Avslutt Tidegauge"))
                .on_key_down(|event, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") { cx.quit(); }
                })
                .on_click(|_, _, cx| cx.quit())))
}

fn metric_heading(tab: Tab, dark: bool) -> Div {
    let color = tab.metric().map(|m| m.accent(dark)).unwrap_or_else(|| t::accent(dark));
    div().flex().items_center().gap_2()
        .child(div().size(px(22.)).rounded(px(7.)).bg(color.opacity(0.14))
            .flex().items_center().justify_center().child(icon_img(tab.icon(), 13., color)))
        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM)
            .text_color(color).child(tab.label()))
}

fn overview_card(
    tab: Tab,
    context: String,
    value: String,
    stats: [(&str, String); 3],
    hist: &Hist,
    max: Option<f64>,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> impl IntoElement {
    let color = tab.metric().unwrap().accent(dark);
    let compact = crate::settings::Settings::load().compact_dashboard;
    let (number, unit) = value.rsplit_once(' ').unwrap_or((&value, ""));
    let keyboard_target = this.clone();
    div()
        .id(SharedString::from(format!("overview-{}", tab.label())))
        .role(Role::Button)
        .aria_label(SharedString::from(format!(
            "{}: {}. {}",
            tab.label(),
            value,
            l("Open details", "Åpne detaljer")
        )))
        .tab_index(20 + tab as isize)
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .focus_visible(|d| d.border_color(color))
        .on_key_down(move |event, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                select_tab(&keyboard_target, tab, cx);
                cx.stop_propagation();
            }
        })
        .min_w_0()
        .h(px(if compact { 154. } else { 208. }))
        .p_4()
        .rounded(px(12.))
        .bg(t::card(dark))
        .flex()
        .flex_col()
        .justify_between()
        .cursor_pointer()
        .hover(|d| d.bg(t::card_hi(dark)))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(metric_heading(tab, dark))
                .child(icon_img("icons/chevron-right.svg", 12., t::ink_dim(dark))),
        )
        .child(
            div()
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(t::ink_dim(dark))
                        .child(context),
                )
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(34.))
                                .line_height(px(42.))
                                .font_weight(FontWeight::BOLD)
                                .child(number.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t::ink_dim(dark))
                                .child(unit.to_string()),
                        ),
                ),
        )
        .when(!compact, |d| {
            d.child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap_2()
                    .children(stats.into_iter().map(|(label, value)| {
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .line_height(px(16.))
                                    .text_color(t::ink_dim(dark))
                                    .child(label.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(17.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(value),
                            )
                    })),
            )
        })
        .when(!hist.pts.is_empty(), |d| {
            d.child(
                div()
                    .h(px(38.))
                    .rounded(px(8.))
                    .px(px(6.))
                    .py(px(5.))
                    .bg(color.opacity(0.09))
                    .child(area_chart(&hist.pts, 28.0, color, max)),
            )
        })
        .when(hist.pts.is_empty(), |d| {
            d.child(div().h(px(38.)).flex_shrink_0())
        })
        .on_click(move |_, _, cx| {
            select_tab(&this, tab, cx);
        })
}

fn history_average(hist: &Hist) -> String {
    if hist.pts.is_empty() { return "—".into(); }
    fmt_pct(hist.pts.iter().sum::<f64>() / hist.pts.len() as f64)
}

fn overview(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let battery_context = s.battery.status();
    let remaining = s
        .battery
        .minutes_remaining
        .filter(|m| s.battery.on_battery && *m >= 0)
        .map(|m| format!("{}h {}m", m / 60, m % 60))
        .unwrap_or("—".into());
    let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
    let settings = crate::settings::Settings::load();
    let favorites = &settings.favorite_metrics;
    let cards = vec![
        (
            Tab::Cpu,
            l("Now", "Nå").into(),
            format!("{:.0} %", s.cpu_pct),
            [
                (l("Load · 1 min", "Last · 1 min"), format!("{:.2}", s.load_avg[0])),
                (l("Cores", "Kjerner"), s.chip.cores.to_string()),
                (l("Average", "Gjennomsnitt"), history_average(&s.cpu_hist)),
            ],
            &s.cpu_hist,
            Some(100.),
        ),
        (
            Tab::Memory,
            format!(
                "{} {}",
                l("In use of", "I bruk av"),
                fmt_bytes(s.mem.total as f64)
            ),
            fmt_bytes(s.mem.used as f64),
            [
                (l("App", "App"), fmt_bytes(s.mem.app as f64)),
                (l("Wired", "Bundet"), fmt_bytes(s.mem.wired as f64)),
                (l("Compressed", "Komprimert"), fmt_bytes(s.mem.compressed as f64)),
            ],
            &s.mem_hist,
            Some(100.),
        ),
        (
            Tab::Gpu,
            if s.gpu.utilization.is_some() {
                l("GPU utilization", "GPU-bruk")
            } else {
                l("Utilization unavailable", "Bruk utilgjengelig")
            }
            .into(),
            s.gpu
                .utilization
                .map(|v| format!("{:.0} %", v))
                .unwrap_or("—".into()),
            [
                (
                    l("Memory", "Minne"),
                    s.gpu
                        .mem_in_use
                        .map(|v| fmt_bytes(v as f64))
                        .unwrap_or("—".into()),
                ),
                (
                    l("Average", "Gjennomsnitt"),
                    if s.gpu.utilization.is_some() {
                        history_average(&s.gpu_hist)
                    } else {
                        "—".into()
                    },
                ),
                (
                    l("Peak", "Topp"),
                    if s.gpu.utilization.is_some() {
                        fmt_pct(s.gpu_hist.pts.iter().copied().fold(0., f64::max))
                    } else {
                        "—".into()
                    },
                ),
            ],
            &s.gpu_hist,
            Some(100.),
        ),
        (
            Tab::Disk,
            format!(
                "{} {}",
                l("Free of", "Ledig av"),
                fmt_bytes(s.disk.total as f64)
            ),
            fmt_bytes(s.disk.free as f64),
            [
                (l("Reading", "Leser"), fmt_rate(s.disk.read_rate)),
                (l("Writing", "Skriver"), fmt_rate(s.disk.write_rate)),
                (l("Used", "Brukt"), fmt_bytes(s.disk.used as f64)),
            ],
            &s.disk_w_hist,
            None,
        ),
        (
            Tab::Network,
            l("Downloading", "Laster ned").into(),
            fmt_rate(s.net.rate_in),
            [
                (l("Uploading", "Laster opp"), fmt_rate(s.net.rate_out)),
                (l("Received¹", "Mottatt¹"), fmt_bytes(s.net.total_in as f64)),
                (l("Sent¹", "Sendt¹"), fmt_bytes(s.net.total_out as f64)),
            ],
            &s.net_in_hist,
            None,
        ),
        (
            Tab::Battery,
            battery_context.into(),
            s.battery
                .percent
                .map(|v| format!("{:.0} %", v))
                .unwrap_or("—".into()),
            [
                (l("Remaining", "Gjenstår"), remaining),
                (
                    crate::locale::text("Battery flow", "Batteristrøm"),
                    s.battery
                        .power_w
                        .map(|w| format!("{:+.1} W", w))
                        .unwrap_or("—".into()),
                ),
                (
                    crate::locale::text("Capacity health", "Kapasitetshelse"),
                    s.battery.health.map(fmt_pct).unwrap_or("—".into()),
                ),
            ],
            &s.batt_hist,
            Some(100.),
        ),
    ];
    let target = this.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(if settings.compact_dashboard { 10. } else { 16. }))
        .when(crate::keepawake::status().active, |d| {
            d.child(crate::controls::keep_awake(dark, this.clone()))
        })
        .when(favorites.is_empty(), |d| {
            d.child(
                card(dark)
                    .child(sub(l(
                        "Choose your Overview favorites in Settings.",
                        "Velg favoritter for oversikten i Innstillinger.",
                    )))
                    .child(crate::controls::button(
                        "choose-favorites",
                        l("Settings", "Innstillinger"),
                        false,
                        dark,
                        move |_, cx| open_settings(&target, cx),
                    )),
            )
        })
        .child(
            div()
                .grid()
                .grid_cols(3)
                .gap(px(if settings.compact_dashboard { 10. } else { 16. }))
                .children(
                    cards
                        .into_iter()
                        .filter(|(tab, ..)| favorites.iter().any(|k| k == tab.key()))
                        .map(|(tab, context, value, stats, hist, max)| {
                            overview_card(tab, context, value, stats, hist, max, dark, this.clone())
                        }),
                ),
        )
        .when(!settings.compact_dashboard && !favorites.is_empty(), |d| {
            d.child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap(px(16.))
                    .when(favorites.iter().any(|key| key == "memory"), |row| row.child(donut_card(
                        l("Memory by Type", "Minne etter type"),
                        Tab::Memory,
                        vec![
                            BreakdownEntry::new(l("App", "App"), s.mem.app as f64, Metric::Cpu.accent(dark)),
                            BreakdownEntry::new(
                                l("Wired", "Bundet"),
                                s.mem.wired as f64,
                                Metric::Memory.accent(dark),
                            ),
                            BreakdownEntry::new(
                                l("Compressed", "Komprimert"),
                                s.mem.compressed as f64,
                                Metric::Gpu.accent(dark),
                            ),
                            BreakdownEntry::new(l("Cached", "Hurtigbufret"), s.mem.cached as f64, t::ink_dim(dark)),
                            BreakdownEntry::new(l("Free", "Ledig"), s.mem.free as f64, t::card_hi(dark)),
                        ],
                        format!("{:.0}%", mem_pct),
                        l("in use", "i bruk"),
                        fmt_bytes,
                        dark,
                    ))
                    .child(top_apps_donut(s, dark)))
                    .when(favorites.iter().any(|key| key == "battery"), |row| row.child(power_donut(s, dark))),
            )
        })
        .child(
            div()
                .text_size(px(10.))
                .text_color(t::ink_dim(dark))
                .child(l(
                "¹ Network totals since boot. Averages and peaks cover this session.",
                "¹ Nettverkstotaler siden oppstart. Gjennomsnitt og topper gjelder denne økten.",
            )),
        )
}

/// Draw a ring from convex quadrilaterals. GPUI triangulates filled paths
/// as a fan, so a concave wedge would incorrectly paint across the center.
fn donut(slices: &[(f64, Hsla)], size: f32) -> Canvas<()> {
    let slices = slices.to_vec();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let w: f32 = bounds.size.width.into();
            let h: f32 = bounds.size.height.into();
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let cx = ox + w / 2.0;
            let cy = oy + h / 2.0;
            let r = w.min(h) / 2.0 - 2.0;
            let r_in = r * 0.70;
            let total: f64 = slices.iter().map(|s| s.0).sum();
            let ring = if total > 0. {
                slices.clone()
            } else {
                vec![(1., t::hex(0x8e8e93))]
            };
            let total = if total > 0. { total } else { 1. };
            let mut angle = -std::f64::consts::FRAC_PI_2;
            let at = |radius: f32, angle: f64| {
                point(
                    px(cx + radius * angle.cos() as f32),
                    px(cy + radius * angle.sin() as f32),
                )
            };
            for (value, color) in &ring {
                let sweep = (*value / total) * std::f64::consts::TAU;
                if sweep <= 0. {
                    continue;
                }
                let steps = (sweep / 0.04).ceil().max(1.) as usize;
                for i in 0..steps {
                    let a = angle + sweep * i as f64 / steps as f64;
                    let b = angle + sweep * (i + 1) as f64 / steps as f64;
                    let mut segment = Path::new(at(r, a));
                    segment.line_to(at(r, b));
                    segment.line_to(at(r_in, b));
                    segment.line_to(at(r_in, a));
                    segment.line_to(at(r, a));
                    window.paint_path(segment, *color);
                }
                angle += sweep;
            }
        },
    )
    .size(px(size))
}

struct BreakdownEntry {
    name: String,
    value: f64,
    color: Hsla,
    app: Option<AppGroup>,
}

impl BreakdownEntry {
    fn new(name: &str, value: f64, color: Hsla) -> Self {
        Self {
            name: name.into(),
            value,
            color,
            app: None,
        }
    }
}

fn donut_card(
    label: &str, tab: Tab, slices: Vec<BreakdownEntry>, center: String,
    center_label: &str, format: fn(f64) -> String, dark: bool,
) -> Div {
    let chart = slices.iter().map(|s| (s.value, s.color)).collect::<Vec<_>>();
    let legend = slices.iter().map(|entry| {
        div().flex().items_center().gap(px(6.)).h(px(23.))
            .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(entry.color))
            .when_some(entry.app.as_ref(), |d, app| d.child(app_icon(app, dark)))
            .child(div().flex_1().min_w_0().text_size(px(11.)).truncate().child(entry.name.clone()))
            .child(div().flex_shrink_0().text_size(px(11.)).text_color(t::ink_dim(dark))
                .child(format(entry.value)))
    }).collect::<Vec<_>>();
    div().min_w_0().h(px(184.)).rounded(px(12.)).bg(t::card(dark)).p(px(16.))
        .flex().flex_col().gap(px(12.))
        .child(div().flex().items_center().gap_2()
            .child(div().size(px(22.)).rounded(px(7.)).bg(tab.metric().unwrap().soft(dark))
                .flex().items_center().justify_center()
                .child(icon_img(tab.icon(), 13., tab.metric().unwrap().accent(dark))))
            .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM)
                .text_color(tab.metric().unwrap().accent(dark)).child(label.to_string())))
        .child(div().flex().items_center().gap(px(16.))
            .child(div().relative().size(px(92.)).flex_shrink_0()
                .child(donut(&chart, 92.))
                .child(div().absolute().inset_0().flex().flex_col().items_center().justify_center()
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(center))
                    .child(div().text_size(px(9.)).text_color(t::ink_dim(dark)).child(center_label.to_string()))))
            .child(div().flex_1().min_w_0().flex().flex_col().children(legend)))
}

fn top_apps_donut(s: &Snapshot, dark: bool) -> Div {
    let color = Metric::Memory.accent(dark);
    let mut top: Vec<&AppGroup> = s.apps.iter().collect();
    top.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    let total: f64 = top.iter().map(|a| a.mem_bytes as f64).sum();
    let mut slices = Vec::new();
    for (i, app) in top.iter().take(4).enumerate() {
        slices.push(BreakdownEntry {
            name: app.name.clone(), value: app.mem_bytes as f64,
            color: color.opacity(1.0 - i as f32 * 0.16), app: Some((*app).clone()),
        });
    }
    let other: f64 = top.iter().skip(4).map(|a| a.mem_bytes as f64).sum();
    if other > 0. { slices.push(BreakdownEntry::new(l("Other", "Annet"), other, color.opacity(0.35))); }
    donut_card(l("Memory by App", "Minne etter app"), Tab::Memory, slices, fmt_bytes(total), l("all apps", "alle apper"), fmt_bytes, dark)
}

fn breakdown_watts(watts: f64) -> String {
    if watts <= 0. { "0 W".into() }
    else if watts < 0.001 { "<1 mW".into() }
    else if watts < 1. { format!("{:.0} mW", watts * 1000.) }
    else { fmt_watts(watts) }
}

fn power_donut(s: &Snapshot, dark: bool) -> Div {
    let color = Metric::Power.accent(dark);
    let mut top: Vec<&AppGroup> = s.apps.iter().filter(|a| a.power_w > 0.).collect();
    if top.is_empty() {
        return card(dark)
            .child(metric_heading(Tab::Battery, dark))
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(l("App energy unavailable", "Appenergi utilgjengelig")))
            .child(sub(l("No per-app energy estimates are currently available. This does not mean zero power consumption.", "Ingen energianslag per app er tilgjengelige nå. Dette betyr ikke null strømforbruk.")).text_color(t::ink_dim(dark)));
    }
    top.sort_by(|a, b| b.power_w.total_cmp(&a.power_w));
    let total: f64 = top.iter().map(|a| a.power_w).sum();
    let mut slices = Vec::new();
    for (i, app) in top.iter().take(4).enumerate() {
        slices.push(BreakdownEntry {
            name: app.name.clone(), value: app.power_w,
            color: color.opacity(1.0 - i as f32 * 0.16), app: Some((*app).clone()),
        });
    }
    let other: f64 = top.iter().skip(4).map(|a| a.power_w).sum();
    if other > 0. { slices.push(BreakdownEntry::new(l("Other", "Annet"), other, color.opacity(0.35))); }
    donut_card(l("Power by App", "Effekt etter app"), Tab::Battery, slices, breakdown_watts(total), l("estimated", "beregnet"), breakdown_watts, dark)
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
            l("CPU usage", "CPU-bruk"),
            format!("{:.0}%", s.cpu_pct),
            format!(
                "{} · {} P + {} E {}",
                s.chip.brand,
                s.chip.p_cores,
                s.chip.e_cores,
                l("cores", "kjerner")
            ),
            &s.cpu_hist,
            m.accent(dark),
            dark,
            vec![
                (
                    l("Load · 1 / 5 / 15 min", "Last · 1 / 5 / 15 min").into(),
                    format!(
                        "{:.2} / {:.2} / {:.2}",
                        s.load_avg[0], s.load_avg[1], s.load_avg[2]
                    ),
                ),
                (
                    l("Top app", "Mest aktive app").into(),
                    rows.first()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    l("Session uptime", "Øktens oppetid").into(),
                    crate::engine::fmt_up(s.uptime.as_secs_f64()),
                ),
            ],
            Some(100.0),
        ))
        .child(section_header(
            l("Apps · highest usage first", "Apper · høyest bruk først"),
            l("CPU · 100% per core", "CPU · 100 % per kjerne"),
            dark,
        ))
        .child(list_card(
            rows.iter()
                .take(30)
                .enumerate()
                .map(|(i, a)| {
                    app_row(
                        i + 1,
                        a,
                        format!("{:.0}%", a.cpu_pct),
                        a.cpu_pct / max,
                        m.accent(dark),
                        dark,
                    )
                })
                .collect(),
            dark,
        ))
        .child(sensor_panel(s, dark))
}

const MAX_DISPLAYED_TEMPERATURE_CHANNELS: usize = 24;

/// Ordered display rows and counts for the temperature card. Real hardware can
/// expose hundreds of channels, so the card renders at most
/// `MAX_DISPLAYED_TEMPERATURE_CHANNELS` rows.
struct TemperatureDisplay {
    rows: Vec<crate::sensors::Reading>,
    total: usize,
    unavailable: usize,
}

/// Order a local copy of the channels by value descending with unavailable
/// channels last, then bound the rendered list. The snapshot is not mutated.
fn bound_temperature_display(values: &[crate::sensors::Reading]) -> TemperatureDisplay {
    let mut rows = values.to_vec();
    rows.sort_by(|a, b| match (a.value, b.value) {
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (None, None) => std::cmp::Ordering::Equal,
    });
    rows.truncate(MAX_DISPLAYED_TEMPERATURE_CHANNELS);
    TemperatureDisplay {
        rows,
        total: values.len(),
        unavailable: values.iter().filter(|r| r.value.is_none()).count(),
    }
}

/// Exact counts when the temperature list is bounded or has unavailable
/// channels; `None` when every channel fits and reported a value. `l` is
/// `locale::text` in the app and a fixed-language selector in tests.
fn temperature_display_caption(
    display: &TemperatureDisplay,
    l: fn(&'static str, &'static str) -> &'static str,
) -> Option<String> {
    if display.total <= MAX_DISPLAYED_TEMPERATURE_CHANNELS && display.unavailable == 0 {
        return None;
    }
    let shown = display.rows.len();
    let total = display.total;
    let mut caption = format!(
        "{} {shown} {} {total} {}",
        l("Showing", "Viser"),
        l("of", "av"),
        l("channels", "kanaler")
    );
    if display.unavailable > 0 {
        caption.push_str(&format!(
            " · {} {}",
            display.unavailable,
            l("unavailable", "utilgjengelige")
        ));
    }
    Some(caption)
}

fn sensor_panel(s: &Snapshot, dark: bool) -> Div {
    use crate::locale::text as l;
    let preview = crate::sensors::preview();
    let sample = preview.clone().unwrap_or_else(crate::sensors::latest);
    let fresh = sample.is_fresh();
    let source = if !fresh && matches!(sample.source, crate::sensors::SourceState::Ready) {
        l(
            "Stale readings · waiting for an update",
            "Utdaterte målinger · venter på oppdatering",
        )
        .into()
    } else {
        sample.source.label()
    };
    let temperature_display = bound_temperature_display(&sample.temperatures);
    let temperature_caption = temperature_display_caption(&temperature_display, l);
    let readings = |title: &'static str,
                    values: &[crate::sensors::Reading],
                    unit: &'static str,
                    empty: &'static str| {
        card(dark)
            .child(section_header(title, unit, dark))
            .when(values.is_empty(), |d| {
                d.child(sub(empty).text_color(t::ink_dim(dark)))
            })
            .children(values.iter().map(|reading| {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(div().text_sm().child(reading.key.clone()))
                    .child(
                        div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
                            reading
                                .value
                                .filter(|_| fresh)
                                .map(|v| {
                                    if unit == "RPM" {
                                        format!("{:.0} RPM", v)
                                    } else {
                                        format!("{:.1} °C", v)
                                    }
                                })
                                .unwrap_or_else(|| l("Unavailable", "Utilgjengelig").into()),
                        ),
                    )
            }))
    };
    div().flex().flex_col().gap_3()
        .child(section_header(l("Temperatures and fans", "Temperaturer og vifter"), l("Read-only", "Kun avlesning"), dark))
        .when(preview.is_some(), |d| d.child(card(dark).child(sub(l("DEVELOPMENT PREVIEW · Sample sensor values, not hardware readings", "UTVIKLINGSVISNING · Eksempelverdier, ikke sensormålinger")).text_color(t::ink_dim(dark)))))
        .child(div().grid().grid_cols(2).gap_3()
            .child(readings(l("Temperature channels", "Temperaturkanaler"), &temperature_display.rows, "°C", if sample.discovering { l("Discovering temperature channels…", "Finner temperaturkanaler…") } else { l("Temperature channels unavailable. This does not mean the Mac is cool.", "Temperaturkanaler utilgjengelige. Dette betyr ikke at Macen er kjølig.") })
                .when_some(temperature_caption, |d, caption| {
                    d.child(sub(caption).text_color(t::ink_dim(dark)))
                }))
            .child(readings(l("Fan speed", "Viftehastighet"), &sample.fans, "RPM", if sample.fan_count == Some(0) && fresh {
                l("The controller reports no fans.", "Kontrolleren rapporterer ingen vifter.")
            } else { l("Fan readings unavailable.", "Viftemålinger er utilgjengelige.") })))
        .child(div().flex().justify_between().gap_3()
            .child(sub(source).text_color(t::ink_dim(dark)))
            .child(sub(format!("{}: {}", l("macOS thermal state", "Termisk tilstand fra macOS"), s.thermal)).text_color(t::ink_dim(dark))))
        .when(sample.discovering, |d| d.child(sub(l("Discovering sensor channels…", "Finner sensorkanaler…")).text_color(t::ink_dim(dark))))
        .when(sample.inventory_limited, |d| d.child(sub(l("Sensor inventory is limited; some channels are not shown.", "Sensoroversikten er begrenset; enkelte kanaler vises ikke.")).text_color(t::ink_dim(dark))))
        .child(sub(l("Native channel identifiers are preserved; locations vary by model. Missing or stale readings are never treated as zero. macOS retains automatic fan control.", "Opprinnelige kanalnavn beholdes; plassering varierer med modell. Manglende eller utdaterte målinger behandles aldri som null. macOS beholder automatisk viftekontroll.")).text_color(t::ink_dim(dark)))
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
            l("Memory", "Minne"),
            format!("{:.0}%", mem_pct),
            format!("{} {} {}", fmt_bytes(s.mem.used as f64), l("used of", "brukt av"), fmt_bytes(s.mem.total as f64)),
            &s.mem_hist,
            m.accent(dark),
            dark,
            vec![
                (l("App memory", "Appminne").into(), fmt_bytes(s.mem.app as f64)),
                (l("Wired", "Reservert").into(), fmt_bytes(s.mem.wired as f64)),
                (l("Compressed", "Komprimert").into(), fmt_bytes(s.mem.compressed as f64)),
                (l("Cached", "Hurtigbufret").into(), fmt_bytes(s.mem.cached as f64)),
                (l("Swap used", "Brukt vekselminne").into(), fmt_bytes(s.mem.swap_used as f64)),
            ],
            Some(100.0),
        ))
        .child(section_header(l("Apps · highest usage first", "Apper · høyest bruk først"), l("Memory", "Minne"), dark))
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
        .child(sub(l("App totals and system-used memory have different accounting scopes and may not reconcile. Utilization is not memory pressure.", "Apptotaler og brukt systemminne har ulikt beregningsgrunnlag og samsvarer ikke nødvendigvis. Utnyttelse er ikke minnepress.")).text_color(t::ink_dim(dark)))
}

fn disk_tab(
    s: &Snapshot,
    dark: bool,
    selected: Option<&str>,
    cx: &mut Context<TideGaugeView>,
) -> Div {
    use crate::locale::text as l;
    let m = Metric::Disk;
    let preview = crate::disks::preview();
    let is_preview = preview.is_some();
    let (inventory, io) =
        preview.unwrap_or_else(|| (crate::disks::inventory(), crate::disks::io()));
    let device = match selected {
        Some(id) => inventory.devices.iter().find(|d| d.id == id),
        None => inventory.devices.first(),
    };
    let selected_id = device.map(|d| d.id.as_str());
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| {
        (b.disk_read_rate + b.disk_write_rate)
            .partial_cmp(&(a.disk_read_rate + a.disk_write_rate))
            .unwrap()
    });
    let max = rows
        .first()
        .map(|a| a.disk_read_rate + a.disk_write_rate)
        .unwrap_or(1.0)
        .max(1.0);
    let mut combined = s.disk_r_hist.pts.clone();
    for (i, v) in combined.iter_mut().enumerate() {
        *v += s.disk_w_hist.pts.get(i).copied().unwrap_or(0.0);
    }
    let io_hist = Hist {
        pts: combined,
        max_len: 360,
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            l("Disk", "Disk"),
            fmt_bytes(s.disk.free as f64),
            format!(
                "{} {}",
                l("Free of", "Ledig av"),
                fmt_bytes(s.disk.total as f64)
            ),
            &io_hist,
            m.accent(dark),
            dark,
            vec![
                (l("Reading", "Leser").into(), fmt_rate(s.disk.read_rate)),
                (l("Writing", "Skriver").into(), fmt_rate(s.disk.write_rate)),
                (l("Used", "Brukt").into(), fmt_bytes(s.disk.used as f64)),
            ],
            None,
        ))
        .child(section_header(l("Storage devices", "Lagringsenheter"), l("Read-only", "Kun avlesning"), dark))
        .when(is_preview, |d| d.child(card(dark).child(sub(l("DEVELOPMENT PREVIEW · Sample disk values, not hardware readings", "UTVIKLINGSVISNING · Eksempelverdier, ikke diskmålinger")).text_color(t::ink_dim(dark)))))
        .child(div().id("disk-device-picker").role(Role::RadioGroup).aria_label(l("Storage devices", "Lagringsenheter")).tab_group().flex().flex_wrap().gap_2().children(inventory.devices.iter().enumerate().map(|(index, device)| {
            let id = device.id.clone();
            let key_id = id.clone();
            div().id(SharedString::from(format!("disk-{}", id)))
                .role(Role::RadioButton).aria_label(SharedString::from(format!("{} · {}", id, device.name)))
                .aria_toggled(if selected_id == Some(id.as_str()) { Toggled::True } else { Toggled::False }).tab_index(20 + index as isize)
                .px_3().py_2().rounded_md().border_1().border_color(t::line(dark))
                .bg(if selected_id == Some(id.as_str()) { t::card_hi(dark) } else { t::card(dark) })
                .hover(|d| d.bg(t::card_hi(dark))).focus_visible(|d| d.bg(t::card_hi(dark)))
                .cursor_pointer().max_w(px(320.))
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(device.id.clone()))
                .child(sub(device.name.clone()).text_color(t::ink_dim(dark)))
                .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        view.selected_disk = Some(key_id.clone()); cx.notify(); cx.stop_propagation();
                    }
                }))
                .on_click(cx.listener(move |view, _, _, cx| { view.selected_disk = Some(id.clone()); cx.notify(); }))
        })))
        .when(!inventory.fresh(), |d| d.child(card(dark).child(sub(if inventory.collected_at.is_none() {
            l("Discovering storage devices…", "Finner lagringsenheter…")
        } else { l("Disk inventory unavailable or stale. No capacity or health is assumed.", "Diskoversikten er utilgjengelig eller utdatert. Ingen kapasitet eller helsetilstand antas.") }).text_color(t::ink_dim(dark)))))
        .when(inventory.partial, |d| d.child(sub(l("Partial inventory · some device information could not be read.", "Delvis oversikt · enkelte enhetsopplysninger kunne ikke leses.")).text_color(t::ink_dim(dark))))
        .when(inventory.fresh() && device.is_none(), |d| d.child(card(dark).child(sub(l("Selected device unavailable, or no supported devices found.", "Valgt enhet er utilgjengelig, eller ingen støttede enheter ble funnet.")).text_color(t::ink_dim(dark)))))
        .when(device.is_some_and(|device| !device.identity_matches(&io)), |d| d.child(card(dark).child(sub(l("Device disconnected, replaced, or temporarily unavailable. Waiting for a fresh inventory.", "Enheten er frakoblet, erstattet eller midlertidig utilgjengelig. Venter på en ny oversikt.")).text_color(t::ink_dim(dark)))))
        .when_some(device.filter(|device| inventory.fresh() && device.identity_matches(&io)), |d, device| d.child(disk_detail(device, io.get(&device.id).filter(|sample| sample.fresh()), dark)))
        .child(sub(l("Inventory refreshes every 30 seconds. Virtual disk images are omitted. No mount, eject, erase, or SMART-setting operations are performed.", "Oversikten oppdateres hvert 30. sekund. Virtuelle diskbilder utelates. Ingen montering, utmating, sletting eller endring av SMART-innstillinger utføres.")).text_color(t::ink_dim(dark)))
        .child(section_header(l("All apps · all devices", "Alle apper · alle enheter"), l("Disk activity", "Diskaktivitet"), dark))
        .child(sub(l("App activity below is system-wide, not attributed to the selected drive.", "Appaktiviteten nedenfor gjelder hele systemet, ikke bare den valgte disken.")).text_color(t::ink_dim(dark)))
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

fn disk_detail(
    device: &crate::disks::Device,
    io: Option<&crate::disks::IoSample>,
    dark: bool,
) -> Div {
    use crate::disks::Health;
    use crate::locale::text as l;
    let unknown = || l("Unavailable", "Utilgjengelig").to_string();
    let bytes = |value: Option<u64>| value.map(|v| fmt_bytes(v as f64)).unwrap_or_else(unknown);
    let count = |value: Option<u64>| value.map(|v| v.to_string()).unwrap_or_else(unknown);
    let health = match device.smart.health {
        Health::Passed => l("Passed", "Bestått"),
        Health::Failed => l("Failing", "Feil oppdaget"),
        Health::Unsupported => l("Not supported", "Ikke støttet"),
        Health::Unavailable => l("Unavailable", "Utilgjengelig"),
    };
    let location = match device.internal {
        Some(true) => l("Internal", "Intern"),
        Some(false) => l("External", "Ekstern"),
        None => l("Location unavailable", "Plassering utilgjengelig"),
    };
    let media = match device.solid_state {
        Some(true) => "SSD",
        Some(false) => l("Rotating disk", "Roterende disk"),
        None => l("Media type unavailable", "Medietype utilgjengelig"),
    };
    let flow = |title: &'static str, rate: Option<f64>, history: &[f64]| {
        card(dark)
            .child(caption(title, dark))
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child(rate.map(fmt_rate).unwrap_or_else(unknown)),
            )
            .when(!history.is_empty(), |d| {
                d.child(area_chart(history, 60.0, Metric::Disk.accent(dark), None))
            })
    };
    div().flex().flex_col().gap_3()
        .child(card(dark)
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(device.name.clone()))
            .child(sub(format!("{} · {} · {} · {}", device.id, location, media, device.protocol.as_deref().unwrap_or(l("Protocol unavailable", "Protokoll utilgjengelig")))).text_color(t::ink_dim(dark))))
        .child(stat_card(l("Device capacity", "Enhetskapasitet"), bytes(device.size), None, dark))
        .when(device.smart.health == Health::Failed, |d| d.child(card(dark).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("SMART reports a failure. Back up important data and inspect this drive.", "SMART rapporterer en feil. Sikkerhetskopier viktige data og undersøk disken.")))))
        .child(section_header(l("Volumes and containers", "Volumer og beholdere"), "diskutil", dark))
        .when(device.volumes.is_empty(), |d| d.child(sub(l("No readable volume information.", "Ingen lesbar voluminformasjon.")).text_color(t::ink_dim(dark))))
        .children(device.volumes.iter().map(|volume| {
            card(dark)
                .child(div().flex().justify_between().gap_3().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(volume.name.clone())).child(sub(volume.id.clone()).text_color(t::ink_dim(dark))))
                .child(sub(volume.mount.clone().unwrap_or_else(|| l("Not mounted", "Ikke montert").into())).text_color(t::ink_dim(dark)))
                .when_some(volume.container.as_ref(), |d, container| d.child(sub(format!("APFS {} · {}", container, volume.stores.join(", "))).text_color(t::ink_dim(dark))))
                .child(div().grid().grid_cols(3).gap_3().children([
                    (if volume.container.is_some() { l("Container capacity", "Beholderkapasitet") } else { l("Volume capacity", "Volumkapasitet") }, bytes(volume.total)),
                    (if volume.container.is_some() { l("Volume footprint", "Volumets plassbruk") } else { l("Used", "Brukt") }, bytes(volume.used)),
                    (if volume.container.is_some() { l("Shared free", "Delt ledig plass") } else { l("Free", "Ledig") }, bytes(volume.free)),
                ].into_iter().map(|(label, value)| div().flex().flex_col().gap_1().child(caption(label, dark)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(value)))))
        }))
        .child(sub(l("APFS volumes share container space. Container capacity and shared free space are repeated for context, never summed as separate disks or per-volume free space.", "APFS-volumer deler beholderplass. Beholderkapasitet og delt ledig plass gjentas som kontekst, og summeres aldri som separate disker eller ledig plass per volum.")).text_color(t::ink_dim(dark)))
        .child(section_header(l("Device activity", "Enhetsaktivitet"), l("Read + write", "Lesing + skriving"), dark))
        .child(div().grid().grid_cols(2).gap_3()
            .child(flow(l("Device read", "Enhetens lesing"), io.and_then(|v| v.read_rate), io.map(|v| v.read_history.as_slice()).unwrap_or(&[])))
            .child(flow(l("Device write", "Enhetens skriving"), io.and_then(|v| v.write_rate), io.map(|v| v.write_history.as_slice()).unwrap_or(&[]))))
        .child(sub(format!("IORegistry · {}: {} · {}: {}", l("Read counter", "Leseteller"), bytes(io.map(|v| v.read_total)), l("Write counter", "Skriveteller"), bytes(io.map(|v| v.write_total)))).text_color(t::ink_dim(dark)))
        .child(sub(l("Device I/O counters reset when the device is reattached; they are not lifetime SMART totals. Rates need two fresh samples.", "Enhetens I/O-tellere nullstilles når enheten kobles til på nytt; de er ikke SMART-totaltall for levetiden. Hastigheter krever to ferske målinger.")).text_color(t::ink_dim(dark)))
        .child(section_header(l("Health and lifetime", "Helse og levetid"), "SMART", dark))
        .child(div().grid().grid_cols(3).gap_3()
            .child(stat_card("SMART", health.to_string(), None, dark))
            .child(stat_card(l("Temperature", "Temperatur"), device.smart.temperature_c.map(|v| format!("{v:.1} °C")).unwrap_or_else(unknown), None, dark))
            .child(stat_card(l("Endurance used", "Brukt skrivelevetid"), device.smart.endurance_used.map(|v| format!("{v}%")).unwrap_or_else(unknown), None, dark)))
        .child(div().grid().grid_cols(4).gap_3()
            .child(stat_card(l("Lifetime read", "Lest i levetiden"), bytes(device.smart.lifetime_read), None, dark))
            .child(stat_card(l("Lifetime write", "Skrevet i levetiden"), bytes(device.smart.lifetime_write), None, dark))
            .child(stat_card(l("Power-on hours", "Driftstimer"), count(device.smart.power_on_hours), None, dark))
            .child(stat_card(l("Power cycles", "Strømsykluser"), count(device.smart.power_cycles), None, dark)))
        .child(sub(if device.smart.extended { l("Extended source: read-only SMART log. Endurance used is a wear indicator, not a health score; it can exceed 100%.", "Utvidet kilde: skrivebeskyttet SMART-logg. Brukt skrivelevetid er en slitasjeindikator, ikke en helsescore; den kan overstige 100 %.") } else { l("SMART status source: macOS. Extended SMART data is unavailable without a supported local reader and device. Missing data never means healthy.", "Kilde for SMART-status: macOS. Utvidede SMART-data er utilgjengelige uten en støttet lokal leser og enhet. Manglende data betyr aldri frisk.") }).text_color(t::ink_dim(dark)))
}

fn network_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Network;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| {
        (b.net_in_rate + b.net_out_rate)
            .partial_cmp(&(a.net_in_rate + a.net_out_rate))
            .unwrap()
    });
    let max = rows
        .first()
        .map(|a| a.net_in_rate + a.net_out_rate)
        .unwrap_or(1.0)
        .max(1.0);
    let mut combined = s.net_in_hist.pts.clone();
    for (i, v) in combined.iter_mut().enumerate() {
        *v += s.net_out_hist.pts.get(i).copied().unwrap_or(0.0);
    }
    let hist = Hist {
        pts: combined,
        max_len: 360,
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(hero(
            l("Network · combined traffic", "Nettverk · samlet trafikk"),
            fmt_rate(s.net.rate_in + s.net.rate_out),
            format!("{} · {}", s.net.kind, s.net.primary),
            &hist,
            m.accent(dark),
            dark,
            vec![
                (l("Download", "Nedlasting").into(), fmt_rate(s.net.rate_in)),
                (l("Upload", "Opplasting").into(), fmt_rate(s.net.rate_out)),
                (
                    l("Received · since boot", "Mottatt · siden oppstart").into(),
                    fmt_bytes(s.net.total_in as f64),
                ),
                (
                    l("Sent · since boot", "Sendt · siden oppstart").into(),
                    fmt_bytes(s.net.total_out as f64),
                ),
            ],
            None,
        ))
        .child(section_header(
            l("Apps · highest usage first", "Apper · høyest bruk først"),
            l("Download + upload", "Nedlasting + opplasting"),
            dark,
        ))
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
        .child(
            sub(format!(
                "{}: {} / {}",
                l(
                    "Peak download / upload · this session",
                    "Høyeste nedlasting / opplasting · denne økten"
                ),
                fmt_rate(s.net.peak_in),
                fmt_rate(s.net.peak_out)
            ))
            .text_color(t::ink_dim(dark)),
        )
}

/// Empty-state card per the detail-page template: a metric-tinted icon tile,
/// a 13 px headline, and at most two lines of `ink_dim` explanation. States
/// what is unavailable and why; never implies a measurement exists.
fn empty_state(icon: &'static str, color: Hsla, headline: String, explanation: String, dark: bool) -> Div {
    div()
        .bg(t::card(dark))
        .rounded(px(12.))
        .border_1()
        .border_color(t::line(dark))
        .p_4()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .size(px(36.))
                .flex_shrink_0()
                .rounded(px(9.))
                .bg(color.opacity(0.16))
                .flex()
                .items_center()
                .justify_center()
                .child(icon_img(icon, 20., color)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(headline),
                )
                .child(sub(explanation).text_color(t::ink_dim(dark))),
        )
}

fn gpu_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Gpu;
    let supported = s.gpu.supported;
    let mut page = div().flex().flex_col().gap_3();
    if s.gpu.utilization.is_none() {
        page = page
            .child(empty_state(
                "icons/gpu.svg",
                m.accent(dark),
                l("Utilization unavailable", "Utnyttelse utilgjengelig").into(),
                l("The current collector does not expose GPU utilization on this Mac. Missing readings are not zero usage.", "Den aktive datakilden viser ikke GPU-utnyttelse på denne Macen. Manglende målinger betyr ikke null bruk.").into(),
                dark,
            ))
            .when_some(s.gpu.mem_in_use, |d, bytes| {
                d.child(stat_card(l("GPU memory", "GPU-minne"), fmt_bytes(bytes as f64), None, dark))
            });
    } else {
        page = page
            .child(hero(
                "GPU",
                s.gpu.utilization.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "—".into()),
                if supported { "Apple GPU".into() } else { l("Not available on this Mac", "Ikke tilgjengelig på denne Macen").into() },
                &s.gpu_hist,
                m.accent(dark),
                dark,
                vec![
                    (l("GPU memory", "GPU-minne").into(), s.gpu.mem_in_use.map(|b| fmt_bytes(b as f64)).unwrap_or("—".into())),
                    (l("Average", "Gjennomsnitt").into(), history_average(&s.gpu_hist)),
                    (l("Peak", "Topp").into(), fmt_pct(s.gpu_hist.pts.iter().copied().fold(0., f64::max))),
                ],
                Some(100.0),
            ))
            .when(!supported, |d| {
                d.child(empty_state(
                    "icons/gpu.svg",
                    m.accent(dark),
                    l("Limited GPU counters", "Begrensede GPU-målere").into(),
                    l("GPU utilization counters aren't exposed in virtual machines. On real Apple Silicon hardware this tab shows utilization and GPU memory.", "GPU-utnyttelse er ikke eksponert i virtuelle maskiner. På ekte Apple Silicon-maskinvare viser denne fanen utnyttelse og GPU-minne.").into(),
                    dark,
                ))
            })
            .child(section_header(
                l("Apps", "Apper"),
                l("GPU attribution", "GPU-tilskrivning"),
                dark,
            ))
            .child(empty_state(
                "icons/gpu.svg",
                m.accent(dark),
                l("Per-app GPU usage unavailable", "GPU-bruk per app er ikke tilgjengelig").into(),
                l("macOS does not attribute GPU utilization to individual apps on this Mac.", "macOS tilskriver ikke GPU-utnyttelse til enkeltapper på denne Macen.").into(),
                dark,
            ));
    }
    page.child(neural_section(s, dark))
}

/// Neural Engine as a supplementary section of the GPU page. Utilization is
/// not exposed by the collector on this Mac, so the section reports its core
/// count honestly rather than implying a live reading.
fn neural_section(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Neural;
    let cores = s.chip.neural_cores;
    let mut section = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(section_header("Neural Engine", l("Read-only", "Kun avlesning"), dark));
    if s.neural.is_some() {
        section = section.child(
            div()
                .grid()
                .grid_cols(3)
                .gap_3()
                .child(stat_card(
                    l("Utilization", "Utnyttelse"),
                    s.neural.map(|v| format!("{:.0}%", v)).unwrap_or("—".into()),
                    Some(m.accent(dark)),
                    dark,
                ))
                .child(stat_card(
                    l("Cores", "Kjerner"),
                    if cores > 0 { format!("{}", cores) } else { "—".into() },
                    None,
                    dark,
                ))
                .child(stat_card(l("Thermal", "Termisk"), s.thermal.clone(), None, dark)),
        );
    } else {
        section = section.child(empty_state(
            "icons/brain.svg",
            m.accent(dark),
            l("Utilization unavailable", "Utnyttelse utilgjengelig").into(),
            format!(
                "{} {}",
                l("Neural Engine utilization needs real Apple Silicon hardware with IOReport access; the current collector does not report it.", "Neural Engine-utnyttelse krever ekte Apple Silicon-maskinvare med IOReport-tilgang; den aktive datakilden rapporterer det ikke."),
                if cores > 0 {
                    format!("{}: {} · {}", l("Reported cores", "Rapporterte kjerner"), cores, l("a core count does not imply live utilization support", "et kjerneantall betyr ikke at direkte måling er støttet"))
                } else {
                    l("A core count does not imply live utilization support.", "Et kjerneantall betyr ikke at direkte måling er støttet.").to_string()
                }
            ),
            dark,
        ));
    }
    section
}

fn battery_tab(s: &Snapshot, dark: bool) -> Div {
    use crate::locale::text as l;
    let m = Metric::Battery;
    let preview = crate::battery::preview();
    let b = preview.as_ref().unwrap_or(&s.battery);
    let empty_history = Hist::default();
    let status = b.status();
    let main = if b.present {
        b.percent.map(|p| format!("{:.0}%", p)).unwrap_or("—".into())
    } else {
        "—".into()
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .when(preview.is_some(), |d| d.child(card(dark).child(sub(l("DEVELOPMENT PREVIEW · Sample battery values, not hardware readings", "UTVIKLINGSVISNING · Eksempelverdier, ikke batterimålinger")).text_color(t::ink_dim(dark)))))
        .when(b.present, |d| d.child(hero(
            l("Battery", "Batteri"),
            main,
            status.into(),
            if preview.is_some() { &empty_history } else { &s.batt_hist },
            m.accent(dark),
            dark,
            vec![
                (l("Capacity health", "Kapasitetshelse").into(), b.health.map(|h| format!("{:.0}%", h)).unwrap_or("—".into())),
                (l("Cycles", "Sykluser").into(), b.cycles.map(|c| c.to_string()).unwrap_or("—".into())),
                (l("Temperature", "Temperatur").into(), b.temperature_c.map(|t| format!("{:.1} °C", t)).unwrap_or("—".into())),
                (if b.is_charging { l("Until full · estimated", "Til fulladet · anslag") } else { l("Time left · estimated", "Tid igjen · anslag") }.into(),
                    (if b.is_charging { b.minutes_to_full } else { b.minutes_remaining }).map(|m| format!("{} h {} min", m / 60, m % 60)).unwrap_or("—".into())),
            ],
            Some(100.0),
        )))
        .when(!b.present, |d| d.child(empty_state(
            "icons/battery-charging.svg",
            m.accent(dark),
            status.into(),
            if b.available { l("This Mac has no internal battery. Battery health and charge readings do not apply.", "Denne Macen har ikke et internt batteri. Batterihelse og lademålinger er ikke relevante.").into() }
            else { l("macOS did not provide battery information. No charge or health values are assumed.", "macOS ga ingen batteriinformasjon. Ingen verdier for ladenivå eller helse blir antatt.").into() },
            dark,
        )))
        .child(section_header(l("App", "App"), l("Estimated power usage", "Anslått strømforbruk"), dark))
        .when(s.apps.iter().any(|a| a.power_w > 0.0), |d| d.child(list_card(
            {
                let mut rows: Vec<&AppGroup> = s.apps.iter().filter(|a| a.power_w > 0.0).collect();
                rows.sort_by(|a, b| b.power_w.partial_cmp(&a.power_w).unwrap());
                let max = rows.first().map(|a| a.power_w).unwrap_or(1.0).max(0.01);
                rows.iter()
                    .take(30)
                    .enumerate()
                    .map(|(i, a)| app_row(i + 1, a, fmt_watts(a.power_w), a.power_w / max, m.accent(dark), dark))
                    .collect()
            },
            dark,
        )))
        .when(!s.apps.iter().any(|a| a.power_w > 0.0), |d| d.child(card(dark).child(sub(l(
            "No per-app energy estimates are currently available.",
            "Ingen energianslag per app er tilgjengelige nå.",
        )).text_color(t::ink_dim(dark)))))
        .when(b.present, |d| d
            .child(section_header(l("Battery health", "Batterihelse"), l("Read-only", "Kun avlesning"), dark))
            .child(div().grid().grid_cols(3).gap_3()
                .child(stat_card(l("Full-charge capacity", "Fulladekapasitet"), b.full_capacity_mah.map(|c| format!("{} mAh", c)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Design capacity", "Designkapasitet"), b.design_capacity_mah.map(|c| format!("{} mAh", c)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("macOS condition", "Tilstand fra macOS"), b.condition_label().into(), None, dark)))
            .child(sub(l("Capacity health compares full-charge and design capacity; it may differ from the macOS health percentage.", "Kapasitetshelse sammenligner fullade- og designkapasitet; den kan avvike fra helseprosenten i macOS.")).text_color(t::ink_dim(dark)))
            .child(section_header(l("Electrical details", "Elektriske detaljer"), l("Live battery readings", "Aktuelle batterimålinger"), dark))
            .child(div().grid().grid_cols(4).gap_3()
                .child(stat_card(l("Battery flow", "Batteristrøm"), b.power_w.map(|w| format!("{:+.1} W", w)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Voltage", "Spenning"), b.voltage_v.map(|v| format!("{:.2} V", v)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Current", "Strømstyrke"), b.current_ma.map(|a| format!("{:+} mA", a)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Adapter rating", "Adapterkapasitet"), b.adapter_rating_w.map(|w| format!("{} W", w)).unwrap_or("—".into()), None, dark)))
            .child(sub(l("Battery flow: + charging, − discharging. Adapter rating is not live consumption. System power is unavailable without a dedicated sensor.", "Batteristrøm: + lading, − utlading. Adapterkapasitet er ikke faktisk forbruk. Systemeffekt er utilgjengelig uten en egen sensor.")).text_color(t::ink_dim(dark))))
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

fn sound_tab(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>, pending: bool) -> Div {
    let m = Metric::Sound;
    let vol = s.audio.device_volume;
    let accent = m.accent(dark);
    let mut rows = Vec::new();
    let mut apps: Vec<_> = s.audio.apps.iter().collect();
    apps.sort_by_key(|a| !(a.running_output || a.running_input));
    for (i, a) in apps.iter().enumerate() {
        let status = if a.running_output && a.running_input {
            l("Output + input active", "Utgang + inngang aktiv")
        } else if a.running_output {
            l("Output active", "Utgang aktiv")
        } else if a.running_input {
            l("Input active", "Inngang aktiv")
        } else {
            l("Idle", "Inaktiv")
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
                .px_4()
                .child(
                    div()
                        .w(px(16.))
                        .text_xs()
                        .text_color(t::ink_dim(dark))
                        .child(format!("{}", i + 1)),
                )
                .child(app_icon(&fake, dark))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(a.name.clone()),
                        )
                        .child(div().text_xs().text_color(t::ink_dim(dark)).child(status)),
                ),
        );
    }
    if rows.is_empty() {
        rows.push(div().p_4().text_sm().text_color(t::ink_dim(dark)).child(l(
            "No app audio activity is reported. Process activity may be unavailable on this Mac.",
            "Ingen lydaktivitet fra apper er rapportert. Prosessaktivitet kan være utilgjengelig på denne Macen.",
        )));
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
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(l("Output device", "Lydutgang")),
                                )
                                .child(
                                    sub(s.audio.device_name.clone()).text_color(t::ink_dim(dark)),
                                ),
                        )
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(
                            if s.audio.device_muted == Some(true) {
                                l("Muted", "Dempet").into()
                            } else {
                                vol.map(|v|format!("{:.0}%",v*100.)).unwrap_or_else(||l("Unavailable", "Utilgjengelig").into())
                            },
                        )),
                )
                .child(crate::controls::audio_output(&s.audio, dark, this, pending)),
        )
        .child(section_header(
            l("Apps using audio", "Apper som bruker lyd"),
            l(
                "Activity · not measured level",
                "Aktivitet · ikke målt lydnivå",
            ),
            dark,
        ))
        .child(list_card(rows, dark))
        .child(sub(l("Per-app volume and mute are unavailable without additional audio routing. Activity is not a measured level. Tidegauge does not record audio.", "Volum og demping per app er utilgjengelig uten ekstra lydruting. Aktivitet er ikke et målt lydnivå. Tidegauge tar ikke opp lyd.")))
}

// ---------- bluetooth ----------

fn bt_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Bluetooth;
    let n = s.bt.devices.len();
    if n == 0 {
        return card(dark)
            .child(metric_heading(Tab::Bluetooth, dark))
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(l("No connected devices reported", "Ingen tilkoblede enheter rapportert")))
            .child(sub(if s.bt.enabled {
                l("Connected devices and supported battery readings appear here. Pair devices in macOS Bluetooth settings.", "Tilkoblede enheter og støttede batterimålinger vises her. Koble sammen enheter i Bluetooth-innstillingene i macOS.")
            } else {
                l("Bluetooth is off or unavailable. Check Bluetooth in macOS System Settings.", "Bluetooth er av eller utilgjengelig. Sjekk Bluetooth i Systeminnstillinger i macOS.")
            }).text_color(t::ink_dim(dark)));
    }
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
                                .child(if n == 0 { l("No devices connected", "Ingen enheter tilkoblet").into() } else { format!("{} {}", n, l("connected", "tilkoblede")) }),
                        )
                        .when_some(lowest, |d, (name, b)| {
                            d.child(sub(format!("{} {}", name, l("has the lowest battery", "har lavest batterinivå"))).text_color(t::ink_dim(dark)))
                        }),
                    )
                    ,
            ),
    );
    if s.bt.devices.is_empty() {
        c = c.child(empty_state(
            "icons/bluetooth.svg",
            m.accent(dark),
            l("No paired devices", "Ingen sammenkoblede enheter").into(),
            if s.bt.enabled {
                l("Paired devices will appear here with their battery level.", "Sammenkoblede enheter vises her med batterinivå.").to_string()
            } else {
                l("Bluetooth is off or unavailable on this Mac.", "Bluetooth er av eller utilgjengelig på denne Macen.").to_string()
            },
            dark,
        ));
    } else {
        c = c
            .child(section_header(
                l("Devices", "Enheter"),
                &format!("{} {}", n, l("connected", "tilkoblede")),
                dark,
            ))
            .child(list_card(
                s.bt.devices
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        let mut kind_line = d.kind.clone();
                        if d.second_battery.is_some() || d.case_battery.is_some() {
                            for (part, value) in [
                                (l("Left", "Venstre"), d.battery),
                                (l("Right", "Høyre"), d.second_battery),
                                (l("Case", "Etui"), d.case_battery),
                            ] {
                                if let Some(b) = value {
                                    kind_line.push_str(&format!(" · {} {:.0}%", part, b));
                                }
                            }
                        }
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .py(px(5.))
                            .px_4()
                            .child(
                                div()
                                    .w(px(16.))
                                    .text_xs()
                                    .text_color(t::ink_dim(dark))
                                    .child(format!("{}", i + 1)),
                            )
                            .child(
                                div()
                                    .size(px(20.))
                                    .rounded_sm()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(m.accent(dark).opacity(0.16))
                                    .child(icon_img("icons/bluetooth.svg", 13., m.accent(dark))),
                            )
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
                                            .child(d.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(t::ink_dim(dark))
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .child(kind_line),
                                    ),
                            )
                            .child(level_meter(
                                (d.battery.unwrap_or(0.0) / 100.0) as f32,
                                m.accent(dark),
                                dark,
                                8,
                            ))
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_right()
                                    .text_sm()
                                    .child(
                                        d.battery
                                            .map(|b| format!("{:.0}%", b))
                                            .unwrap_or("—".into()),
                                    ),
                            )
                    })
                    .collect(),
                dark,
            ));
    }
    c
}

// ---------- projects ----------

fn project_targets(project: &ProjectGroup) -> Vec<crate::projects::StopTarget> {
    let mut targets = std::collections::BTreeMap::new();
    for server in &project.servers {
        let target = targets
            .entry(server.pid)
            .or_insert_with(|| crate::projects::StopTarget {
                server: server.clone(),
                project_dir: project.dir.clone(),
                ports: Vec::new(),
            });
        if !target.ports.contains(&server.port) {
            target.ports.push(server.port);
        }
    }
    targets.into_values().collect()
}

fn request_stop(
    targets: Vec<crate::projects::StopTarget>,
    this: Entity<TideGaugeView>,
    window: &mut Window,
    cx: &mut App,
) {
    if targets.is_empty() || this.read(cx).project_stopping {
        return;
    }
    let details = targets
        .iter()
        .map(|target| {
            format!(
                "{} · PID {} · {}\n{}",
                target.server.name,
                target.server.pid,
                target
                    .ports
                    .iter()
                    .map(|p| format!(":{p}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                target.project_dir
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let detail = format!("{}\n\n{}", details, l("Sends SIGTERM to these processes only. Unsaved work may be lost. Changed or exited processes will not be stopped.", "Sender SIGTERM kun til disse prosessene. Ulagret arbeid kan gå tapt. Endrede eller avsluttede prosesser stoppes ikke."));
    let answer = window.prompt(
        PromptLevel::Warning,
        l("Stop development servers?", "Stoppe utviklingsservere?"),
        Some(&detail),
        &[l("Cancel", "Avbryt"), l("Stop", "Stopp")],
        cx,
    );
    cx.spawn(async move |cx| {
        if answer.await != Ok(1) {
            return;
        }
        this.update(cx, |view, cx| {
            view.project_stopping = true;
                view.project_stop_failed = false;
            view.project_error = Some(
                l(
                    "Stopping confirmed processes…",
                    "Stopper bekreftede prosesser…",
                )
                .into(),
            );
            cx.notify();
        });
        let results = smol::unblock(move || crate::projects::stop_targets(targets)).await;
        this.update(cx, |view, cx| {
            view.project_stopping = false;
                view.project_stop_failed = results.iter().any(|result| !matches!(result.outcome, crate::projects::StopOutcome::Exited | crate::projects::StopOutcome::AlreadyStopped));
            view.project_error = Some(
                results
                    .iter()
                    .map(|r| r.message())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            cx.notify();
        });
    })
    .detach();
}

fn stop_button(
    id: String,
    label: &'static str,
    targets: Vec<crate::projects::StopTarget>,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> impl IntoElement {
    let key_targets = targets.clone();
    let key_this = this.clone();
    div()
        .id(SharedString::from(id))
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(t::line(dark))
        .text_xs()
        .cursor_pointer()
        .hover(|d| d.bg(t::card_hi(dark)))
        .focus_visible(|d| d.bg(t::card_hi(dark)))
        .child(label)
        .on_click(move |_, window, cx| request_stop(targets.clone(), this.clone(), window, cx))
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                request_stop(key_targets.clone(), key_this.clone(), window, cx);
                cx.stop_propagation();
            }
        })
}

fn projects_tab(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let m = Metric::Projects;
    let accent = m.accent(dark);
    let total: usize = s.projects.iter().map(|g| g.servers.len()).sum();
    let all_idle = s.projects.iter().all(|g| g.idle) && total > 0;
    let mut c = div().flex().flex_col().gap_3();
    if total > 0 {
        c = c.child(
            card(dark).child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon_img("icons/folder-code.svg", 20.0, accent))
                    .child(div().flex_1().text_sm().child(if all_idle {
                        format!(
                            "{} {}",
                            total,
                            if total == 1 {
                                l("dev server is running but idle", "utviklingsserver kjører, men er inaktiv")
                            } else {
                                l("dev servers are running but idle", "utviklingsservere kjører, men er inaktive")
                            }
                        )
                    } else {
                        format!(
                            "{} {}",
                            total,
                            if total == 1 {
                                l("dev server running", "utviklingsserver kjører")
                            } else {
                                l("dev servers running", "utviklingsservere kjører")
                            }
                        )
                    }))
                    .child(stop_button(
                        "stop-all".into(),
                        l("Stop all…", "Stopp alle…"),
                        s.projects.iter().flat_map(project_targets).collect(),
                        dark,
                        this.clone(),
                    )),
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
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(g.name.clone()),
                            )
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
                                    .text_color(
                                        if srv.status.starts_with(l("idle", "inaktiv"))
                                            || srv.status.contains(l("barely", "nesten ikke"))
                                        {
                                            t::ink_dim(dark)
                                        } else {
                                            t::ok(dark)
                                        },
                                    )
                                    .child(srv.status.clone()),
                            )
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_right()
                                    .text_xs()
                                    .child(fmt_bytes(srv.mem_bytes as f64)),
                            )
                            .child(stop_button(
                                format!("stop-{}-{}", srv.pid, srv.port),
                                l("Stop…", "Stopp…"),
                                project_targets(g)
                                    .into_iter()
                                    .filter(|target| target.server.pid == srv.pid)
                                    .collect(),
                                dark,
                                this.clone(),
                            ))
                    })),
            );
        }
    } else {
        c = c.child(card(dark).child(
            div()
                .text_sm()
                .text_color(t::ink_dim(dark))
                .child(l("No development servers detected. Local TCP listeners owned by you appear when their working directory is near a Git root or supported project manifest.", "Ingen utviklingsservere funnet. Lokale TCP-lyttere som du eier vises når arbeidsmappen ligger nær en Git-rot eller en støttet prosjektfil.")),
        ));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::{
        ChartScale, MAX_DISPLAYED_TEMPERATURE_CHANNELS, bound_temperature_display,
        temperature_display_caption,
    };
    use crate::sensors::Reading;

    fn reading(key: &str, value: Option<f64>) -> Reading {
        Reading {
            key: key.to_string(),
            value,
        }
    }

    fn english(english: &'static str, _: &'static str) -> &'static str {
        english
    }

    fn norwegian(_: &'static str, norwegian: &'static str) -> &'static str {
        norwegian
    }

    fn channel_pool(channels: usize, unavailable: usize) -> Vec<Reading> {
        let mut values: Vec<Reading> = (0..channels)
            .map(|i| {
                reading(
                    &format!("T{i:03}"),
                    Some(30.0 + (channels - i) as f64 / 10.0),
                )
            })
            .collect();
        for r in values.iter_mut().take(unavailable) {
            r.value = None;
        }
        values
    }

    #[test]
    fn temperature_display_is_bounded_ordered_and_reports_counts() {
        let values = channel_pool(342, 22);
        let display = bound_temperature_display(&values);
        assert_eq!(display.total, 342);
        assert_eq!(display.unavailable, 22);
        assert_eq!(display.rows.len(), MAX_DISPLAYED_TEMPERATURE_CHANNELS);
        assert!(display.rows.iter().all(|r| r.value.is_some()));
        assert!(display
            .rows
            .windows(2)
            .all(|w| w[0].value.unwrap() >= w[1].value.unwrap()));
        // The input order is untouched: sorting happened on a local copy.
        assert_eq!(values[0].key, "T000");
        assert!(values[0].value.is_none());
    }

    #[test]
    fn temperature_display_orders_unavailable_last_and_stays_stable() {
        let values = vec![
            reading("Ta00", Some(40.0)),
            reading("Tb00", None),
            reading("Tc00", Some(50.0)),
            reading("Td00", Some(50.0)),
            reading("Te00", None),
        ];
        let display = bound_temperature_display(&values);
        let keys: Vec<&str> = display.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["Tc00", "Td00", "Ta00", "Tb00", "Te00"]);
    }

    #[test]
    fn temperature_display_caption_is_exact_in_both_languages() {
        let values = channel_pool(342, 22);
        let display = bound_temperature_display(&values);
        assert_eq!(
            temperature_display_caption(&display, english).as_deref(),
            Some("Showing 24 of 342 channels · 22 unavailable")
        );
        assert_eq!(
            temperature_display_caption(&display, norwegian).as_deref(),
            Some("Viser 24 av 342 kanaler · 22 utilgjengelige")
        );
    }

    #[test]
    fn temperature_display_caption_omits_zero_unavailable() {
        let values = channel_pool(30, 0);
        let display = bound_temperature_display(&values);
        assert_eq!(
            temperature_display_caption(&display, english).as_deref(),
            Some("Showing 24 of 30 channels")
        );
    }

    #[test]
    fn temperature_display_caption_is_absent_when_unbounded_and_complete() {
        let values = vec![reading("TC0P", Some(54.5)), reading("TG0P", Some(48.0))];
        let display = bound_temperature_display(&values);
        assert_eq!(display.rows.len(), 2);
        assert!(temperature_display_caption(&display, english).is_none());
    }

    #[test]
    fn chart_scale_leaves_headroom_above_unpinned_maximum() {
        let scale = ChartScale::new(80., None);
        assert!(scale.max > 80., "unpinned scale must exceed the data max");
        let y = scale.y(80., 92.);
        assert!(
            y > scale.top,
            "series maximum must sit below the top inset, got {y}"
        );
    }

    #[test]
    fn chart_scale_pins_to_hint_but_never_below_data() {
        let scale = ChartScale::new(12., Some(100.));
        assert_eq!(scale.max, 100.);
        assert!((scale.y(100., 92.) - scale.top).abs() < 0.01);
        let grown = ChartScale::new(140., Some(100.));
        assert_eq!(grown.max, 140.);
    }

    #[test]
    fn chart_scale_handles_constant_zero_and_degenerate_series() {
        // Constant series: a flat stroke at the mapped height, no NaN, not
        // pinned to an edge.
        let scale = ChartScale::new(42., None);
        let y = scale.y(42., 92.);
        assert!(y.is_finite() && y > scale.top && y < 92. - scale.bottom);
        // All-zero data still gets a finite scale.
        let scale = ChartScale::new(0., None);
        assert!(scale.y(0., 92.).is_finite());
        // NaN/inf inputs fall back rather than propagate into geometry.
        let scale = ChartScale::new(f64::NAN, Some(f64::INFINITY));
        assert!(scale.max.is_finite() && scale.y(0., 92.).is_finite());
        let scale = ChartScale::new(f64::INFINITY, None);
        assert!(scale.max.is_finite());
    }
}
