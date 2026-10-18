//! The Tidegauge panel: sidebar navigation, card-based content.

#![allow(dead_code)]

use crate::controls::is_activation_key;
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
    pub(crate) fn metric(self) -> Option<Metric> {
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
    pub show_agents: bool,
    pub agent_expanded: Option<i32>,
    pub show_sensor_channels: bool,
    /// The page whose "About this page" notes are open; any other page
    /// opens with them closed.
    pub(crate) info_for: Option<Destination>,
    pub(crate) sliders: crate::settings_page::Sliders,
    /// How long Keep awake lasts when switched on; `None` until stopped.
    pub(crate) awake_choice: Option<u64>,
    /// Where the window was last drawn, to close notes when that changes.
    last_here: Option<Destination>,
    agents_scroll: ScrollHandle,
    pub tools: crate::tools_panel::State,
    tools_scroll: ScrollHandle,
    process_scroll: ScrollHandle,
    pub only_remaining: bool,
    /// Apps & processes shows the last minute's activity per process.
    pub(crate) process_recent: bool,
    pub expanded_process_group: Option<String>,
    pub process_sort_cpu: bool,
    pub process_show_all: bool,
    pub process_all_helpers: Option<String>,
    pub process_detail: Option<crate::process_activity::Identity>,
    pub process_order: Vec<String>,
    pub process_order_at: Option<std::time::Instant>,
    pub process_table_hovered: bool,
    pub process_filter: Entity<gpui_kit::base::input::InputState>,
    pub process_query: String,
    pub startup_filter: Entity<gpui_kit::base::input::InputState>,
    pub startup_query: String,
    pub process_pending: bool,
    pub toast: Option<Toast>,
    pub process_selected: Option<crate::process_activity::Identity>,
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
    /// When History last loaded; it reloads every minute while it is open.
    pub(crate) history_loaded_at: Option<std::time::Instant>,
    /// The expanded app's last hour, shown in its row in Apps & processes.
    pub(crate) app_history: Option<crate::process_panel::AppHistory>,
    tab_scrolls: Vec<ScrollHandle>,
    settings_scroll: ScrollHandle,
}

impl TideGaugeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::appearance::apply(window, crate::settings::Settings::load().dark);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let process_filter = cx.new(|cx| {
            gpui_kit::base::input::InputState::new(window, cx)
                .placeholder(l("Filter by name or PID", "Filtrer på navn eller PID"))
        });
        cx.subscribe(&process_filter, |view: &mut TideGaugeView, input, event: &gpui_kit::base::input::InputEvent, cx| {
            if matches!(event, gpui_kit::base::input::InputEvent::Change) {
                view.process_query = input.read(cx).value().trim().to_lowercase();
                view.process_show_all = false;
                cx.notify();
            }
        })
        .detach();
        let sliders = crate::settings_page::Sliders::new(cx);
        let startup_filter = cx.new(|cx| {
            gpui_kit::base::input::InputState::new(window, cx)
                .placeholder(l("Filter by app or label", "Filtrer på app eller etikett"))
        });
        cx.subscribe(&startup_filter, |view: &mut TideGaugeView, input, event: &gpui_kit::base::input::InputEvent, cx| {
            if matches!(event, gpui_kit::base::input::InputEvent::Change) {
                view.startup_query = input.read(cx).value().trim().to_lowercase();
                view.tools.startup_limit = crate::tools_panel::LIST_STEP;
                cx.notify();
            }
        })
        .detach();
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
            show_agents: false,
            agent_expanded: None,
            show_sensor_channels: false,
            info_for: None,
            sliders,
            awake_choice: Some(60),
            last_here: None,
            agents_scroll: ScrollHandle::new(),
            tools: crate::tools_panel::State::default(),
            tools_scroll: ScrollHandle::new(),
            process_scroll: ScrollHandle::new(),
            only_remaining: false,
            process_recent: false,
            expanded_process_group: None,
            process_sort_cpu: true,
            process_show_all: false,
            process_all_helpers: None,
            process_detail: None,
            process_order: Vec::new(),
            process_order_at: None,
            process_table_hovered: false,
            process_filter,
            process_query: String::new(),
            startup_filter,
            startup_query: String::new(),
            process_pending: false,
            toast: None,
            process_selected: None,
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
            history_loaded_at: None,
            app_history: None,
            tab_scrolls: Tab::ALL.iter().map(|_| ScrollHandle::new()).collect(),
            settings_scroll: ScrollHandle::new(),
        };
        // 1s refresh loop
        cx.spawn(
            async move |this: WeakEntity<TideGaugeView>, cx: &mut AsyncApp| loop {
                smol::Timer::after(std::time::Duration::from_millis(1000)).await;
                let _ = cx.update(|cx: &mut App| {
                    let mut app_history_due = None;
                    let history_due = this
                        .update(cx, |v: &mut TideGaugeView, cx: &mut Context<TideGaugeView>| {
                            app_history_due = crate::process_panel::app_history_due(v);
                            v.snap = latest();
                            if v.toast.as_ref().is_some_and(Toast::expired) {
                                v.toast = None;
                            }
                            crate::process_panel::refresh_order(v);
                            cx.notify();
                            v.destination() == Destination::Tab(Tab::History)
                                && !v.history_loading
                                && v.history_loaded_at.is_none_or(|at| at.elapsed().as_secs() >= 60)
                        })
                        .unwrap_or(false);
                    // History keeps itself current while it is open.
                    if let (true, Some(this)) = (history_due, this.upgrade()) {
                        crate::controls::refresh_history(&this, cx);
                    }
                    if let (Some(app), Some(this)) = (app_history_due, this.upgrade()) {
                        crate::process_panel::load_app_history(&this, app, cx);
                    }
                });
            },
        )
        .detach();
        // The window can open on a given page, so rendered checks need no
        // clicks: TIDEGAUGE_START=cpu|apps|agents|settings|tool-startup…,
        // and TIDEGAUGE_START_INFO=1 opens its notes. Navigation only.
        if let Some(to) = std::env::var("TIDEGAUGE_START").ok().as_deref().and_then(Destination::from_key) {
            let info = std::env::var("TIDEGAUGE_START_INFO").as_deref() == Ok("1");
            cx.spawn(async move |this: WeakEntity<TideGaugeView>, cx: &mut AsyncApp| {
                let _ = cx.update(|cx: &mut App| {
                    if let Some(this) = this.upgrade() {
                        navigate(&this, to, cx);
                        if info {
                            this.update(cx, |v, cx| {
                                v.info_for = Some(to);
                                cx.notify();
                            });
                        }
                    }
                });
            })
            .detach();
        }
        v
    }
}

/// A short result note shown at the bottom of the window, then faded out.
pub struct Toast {
    pub text: String,
    pub failed: bool,
    shown: std::time::Instant,
}

impl Toast {
    pub fn new(text: String, failed: bool) -> Self {
        Self { text, failed, shown: std::time::Instant::now() }
    }
    fn expired(&self) -> bool {
        // Failures stay a little longer so they can be read.
        self.shown.elapsed().as_secs() >= if self.failed { 10 } else { 5 }
    }
}

fn toast_view(toast: &Toast, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    let color = if toast.failed { t::warn(dark) } else { t::ok(dark) };
    div()
        .absolute()
        .bottom(t::SPACE_4)
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .id("toast")
                .role(Role::Status)
                .aria_label(SharedString::from(toast.text.clone()))
                .max_w(px(560.))
                .flex()
                .items_center()
                .gap_2()
                .pl_3()
                .pr_1()
                .py_1()
                .rounded(t::RADIUS_CONTROL)
                .border_1()
                .border_color(t::line(dark))
                .bg(t::card_hi(dark))
                .text_size(t::TEXT_BODY)
                .child(div().size(px(7.)).flex_shrink_0().rounded_full().bg(color))
                .child(div().min_w_0().child(toast.text.clone()))
                .child(
                    div()
                        .id("toast-close")
                        .role(Role::Button)
                        .aria_label(l("Dismiss", "Lukk"))
                        .tab_index(0)
                        .size(px(22.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(t::RADIUS_CHIP)
                        .cursor_pointer()
                        .text_color(t::ink_dim(dark))
                        .hover(|d| d.bg(t::card(dark)))
                        .child("×")
                        .on_click(move |_, _, cx| {
                            this.update(cx, |v, cx| {
                                v.toast = None;
                                cx.notify();
                            });
                        }),
                ),
        )
}

// ---------- small builders ----------

/// Section/field caption: uppercase 10 px semibold in `ink_dim`. Sentences
/// and disclaimers use `sub()` at 12 px instead.
pub(crate) fn caption(text: &str, dark: bool) -> Div {
    div()
        .text_size(t::TEXT_CAPTION)
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

pub(crate) fn app_icon(app: &AppGroup, dark: bool) -> impl IntoElement {
    bundle_icon(app.icon_path.as_deref(), dark)
}

/// The app's own icon when its bundle has one; otherwise a quiet glyph: a
/// window for an app without an icon, a terminal for a background program.
pub(crate) fn bundle_icon(bundle: Option<&str>, dark: bool) -> AnyElement {
    if let Some(path) = bundle {
        if let Some(file) = crate::icons::cached_icon_for_bundle(path) {
            return div()
                .size(px(20.))
                .flex_shrink_0()
                .rounded_sm()
                .overflow_hidden()
                .child(img(file).size(px(20.)).rounded_sm())
                .into_any_element();
        }
        crate::icons::request_icon_for_bundle(path);
    }
    glyph_tile(if bundle.is_some() { "icons/app-window.svg" } else { "icons/terminal.svg" }, dark)
        .into_any_element()
}

/// A 20 px soft tile holding a small line glyph.
pub(crate) fn glyph_tile(icon: &'static str, dark: bool) -> Div {
    div()
        .size(px(20.))
        .flex_shrink_0()
        .rounded(t::RADIUS_CHIP)
        .bg(t::card_hi(dark))
        .flex()
        .items_center()
        .justify_center()
        .child(icon_img(icon, 12., t::ink_dim(dark)))
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
pub(crate) fn area_chart(pts: &[f64], height: f32, color: Hsla, max_hint: Option<f64>) -> Canvas<()> {
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

            // High water: the highest sample in view, as a dashed line.
            if data_max > 0. {
                let yh = y_of(data_max);
                let end = ox + w - 2.0;
                let mut dash = Path::new(point(px(ox + 2.0), px(yh)));
                let mut x = ox + 2.0;
                while x < end {
                    let x1 = (x + 4.0).min(end);
                    dash.move_to(point(px(x), px(yh - 0.5)));
                    dash.line_to(point(px(x1), px(yh - 0.5)));
                    dash.line_to(point(px(x1), px(yh + 0.5)));
                    dash.line_to(point(px(x), px(yh + 0.5)));
                    dash.line_to(point(px(x), px(yh - 0.5)));
                    x += 8.0;
                }
                window.paint_path(dash, color.opacity(0.6));
            }
            // Now: a dot on the newest sample.
            if let Some(i) = tail.iter().rposition(|v| v.is_finite()) {
                let (cx, cy, r) = (ox + 2.0 + i as f32 * px_per, y_of(tail[i]), 3.0f32);
                let mut dot = Path::new(point(px(cx + r), px(cy)));
                for k in 1..=8 {
                    let a = k as f32 * std::f32::consts::TAU / 8.0;
                    dot.line_to(point(px(cx + r * a.cos()), px(cy + r * a.sin())));
                }
                window.paint_path(dot, color);
            }
        },
    )
    .w_full()
    .h(px(height))
}

/// How far back a chart of `samples` reaches, as its axis's left label.
/// Samples are about two seconds apart; the chart shows at most 240.
fn chart_span(samples: usize) -> String {
    let seconds = samples.min(240) * 2;
    if seconds < 120 {
        format!("−{seconds} s")
    } else {
        format!("−{} min", seconds / 60)
    }
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

/// App row: icon, name (+ process count when more than one), value, with a
/// quiet fill behind the row showing its share of the busiest app.
fn app_row(app: &AppGroup, value: String, frac: f64, color: Hsla, dark: bool) -> Div {
    div()
        .relative()
        .flex()
        .items_center()
        .gap_2()
        .h(t::ROW_H)
        .px_3()
        .mx_2()
        .rounded(t::RADIUS_CONTROL)
        // Under 1% of the busiest app reads as nothing, so draw nothing
        // rather than a sliver.
        .when(frac >= 0.01, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .top(t::SPACE_1)
                    .bottom(t::SPACE_1)
                    .rounded(t::RADIUS_CHIP)
                    .w(relative(frac.clamp(0.0, 1.0) as f32))
                    .bg(color.opacity(0.14)),
            )
        })
        .child(app_icon(app, dark))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_baseline()
                .gap_2()
                .child(div().text_size(t::TEXT_ROW).truncate().child(app.name.clone()))
                .when(app.pids.len() > 1, |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(t::ink_dim(dark))
                            .child(crate::locale::count(app.pids.len() as u64, "process", "processes", "prosess", "prosesser")),
                    )
                }),
        )
        .child(
            div()
                .w(px(88.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(t::TEXT_ROW)
                .font_weight(FontWeight::MEDIUM)
                .child(value),
        )
}

/// The rest of a ranked list, as one line that opens the full sortable table.
fn more_apps(rest: usize, sort_cpu: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .pt_1()
        .child(
            sub(if rest == 0 {
                String::new()
            } else {
                crate::locale::count(rest as u64, "more app", "more apps", "app til", "apper til")
            })
            .text_color(t::ink_dim(dark)),
        )
        .child(crate::controls::button(
            format!("more-apps-{sort_cpu}"),
            l("Open Apps & processes", "Åpne Apper og prosesser"),
            false,
            dark,
            move |_, cx| {
                this.update(cx, |v, _| {
                    v.process_sort_cpu = sort_cpu;
                    v.process_order.clear();
                });
                crate::process_panel::open(&this, cx);
            },
        ))
}

/// Top apps above zero for a metric tab: at most ten rows, then `more_apps`.
fn top_apps<'a>(
    rows: impl Iterator<Item = &'a AppGroup>,
    value: impl Fn(&AppGroup) -> (String, f64),
    max: f64,
    color: Hsla,
    sort_cpu: bool,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> Div {
    let active: Vec<&AppGroup> = rows.filter(|a| value(a).1 > 0.).collect();
    let mut list = card(dark).p_0().py_2().gap_0();
    for app in active.iter().take(10) {
        let (text, raw) = value(app);
        list = list.child(app_row(app, text, raw / max.max(1e-9), color, dark));
    }
    if active.is_empty() {
        list = list.child(
            div().px_4().py_2().child(
                sub(l("No app is using this right now.", "Ingen app bruker dette akkurat nå."))
                    .text_color(t::ink_dim(dark)),
            ),
        );
    }
    list.child(more_apps(active.len().saturating_sub(10), sort_cpu, dark, this))
}

pub(crate) fn card(dark: bool) -> Div {
    div()
        .bg(t::card(dark))
        .rounded(t::RADIUS_CARD)
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
                                .text_size(t::TEXT_CAPTION)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(color)
                                .child(label.to_uppercase()),
                        )
                        .child(
                            div()
                                .text_size(t::TEXT_HERO)
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
                            .child(area_chart(&hist.pts, 130.0, color, max_hint))
                            // A time axis and one legend, nothing more.
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(t::ink_dim(dark))
                                    .child(chart_span(hist.pts.len()))
                                    .child(l("- - high water in view", "- - høyvann i visningen"))
                                    .child(l("now", "nå")),
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
                                    .text_size(t::TEXT_CAPTION)
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

pub(crate) fn section_header(left: &str, right: &str, dark: bool) -> Div {
    div()
        .flex()
        .justify_between()
        .items_end()
        .px_4()
        .child(caption(left, dark))
        .child(
            div()
                .text_size(t::TEXT_CAPTION)
                .text_color(t::ink_dim(dark))
                .child(right.to_string()),
        )
}

/// Where the window is: a metric or work tab, the apps list, a maintenance
/// tool, or Settings. Derived from the view's flags, so the sidebar can
/// never highlight a different place than the content shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Destination {
    Tab(Tab),
    Apps,
    Agents,
    Tool(crate::tools_panel::Page),
    Settings,
}

impl Destination {
    fn from_key(key: &str) -> Option<Self> {
        use crate::tools_panel::Page;
        match key {
            "apps" => Some(Self::Apps),
            "agents" => Some(Self::Agents),
            "settings" => Some(Self::Settings),
            _ => match key.strip_prefix("tool-") {
                Some(page) => [Page::Startup, Page::Activity, Page::Baselines, Page::Storage, Page::Packages]
                    .into_iter()
                    .find(|p| p.key() == page)
                    .map(Self::Tool),
                None => Tab::from_key(key).map(Self::Tab),
            },
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Tab(tab) => tab.label(),
            Self::Apps => l("Apps & processes", "Apper og prosesser"),
            Self::Agents => l("Agents", "Agenter"),
            Self::Tool(page) => page.label(),
            Self::Settings => l("Settings", "Innstillinger"),
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Tab(tab) => tab.icon(),
            Self::Apps => "icons/layout-grid.svg",
            Self::Agents => "icons/bot.svg",
            Self::Tool(page) => page.icon(),
            Self::Settings => "icons/settings.svg",
        }
    }
    /// Metric tabs carry their metric colour; everything else is neutral.
    fn tinted(self) -> bool {
        matches!(self, Self::Tab(tab) if tab.metric().is_some())
    }
    /// Live readings get a colour swatch; work, history and tools get icons.
    fn swatch(self) -> bool {
        self.tinted() && self != Self::Tab(Tab::Projects)
    }
    fn accent(self, dark: bool) -> Hsla {
        match self {
            Self::Tab(tab) => tab.metric().map(|m| m.accent(dark)).unwrap_or(t::accent(dark)),
            _ => t::accent(dark),
        }
    }
    fn id(self) -> SharedString {
        match self {
            Self::Tab(tab) => format!("nav-{}", tab.key()).into(),
            Self::Apps => "nav-apps".into(),
            Self::Agents => "nav-agents".into(),
            Self::Tool(page) => format!("nav-tool-{}", page.key()).into(),
            Self::Settings => "nav-settings".into(),
        }
    }
}

impl TideGaugeView {
    pub(crate) fn destination(&self) -> Destination {
        if self.show_settings {
            Destination::Settings
        } else if self.show_processes {
            Destination::Apps
        } else if self.show_tools {
            Destination::Tool(self.tools.page)
        } else if self.show_agents {
            Destination::Agents
        } else {
            Destination::Tab(self.tab)
        }
    }
}

pub(crate) fn navigate(this: &Entity<TideGaugeView>, to: Destination, cx: &mut App) {
    match to {
        Destination::Tab(tab) => select_tab(this, tab, cx),
        Destination::Apps => crate::process_panel::open(this, cx),
        Destination::Agents => crate::agents_panel::open(this, cx),
        // Recent activity lives in Apps & processes as "Last minute".
        Destination::Tool(crate::tools_panel::Page::Activity) => {
            this.update(cx, |v, _| {
                v.process_recent = true;
                v.only_remaining = false;
            });
            crate::process_panel::open(this, cx);
        }
        Destination::Tool(page) => crate::tools_panel::open_page(this, page, cx),
        Destination::Settings => open_settings(this, cx),
    }
}

/// Short live reading beside a sidebar entry; `None` when the source is
/// unavailable on this Mac, which also dims the entry.
fn sidebar_reading(tab: Tab, s: &Snapshot) -> Option<String> {
    match tab {
        Tab::Cpu => Some(fmt_pct(s.cpu_pct)),
        Tab::Memory => Some(fmt_bytes(s.mem.used as f64)),
        Tab::Disk => Some(fmt_rate(s.disk.read_rate + s.disk.write_rate)),
        Tab::Network => Some(fmt_rate(s.net.rate_in + s.net.rate_out)),
        Tab::Gpu => s.gpu.utilization.map(fmt_pct),
        Tab::Battery => s.battery.percent.map(fmt_pct),
        Tab::Sound => match (s.audio.device_muted, s.audio.device_volume) {
            (Some(true), _) => Some(l("Muted", "Dempet").into()),
            (_, Some(volume)) => Some(fmt_pct(volume as f64 * 100.)),
            _ => None,
        },
        Tab::Bluetooth => s.bt.enabled.then(|| s.bt.devices.len().to_string()),
        Tab::Projects => Some(s.projects.len().to_string()),
        Tab::Overview | Tab::History => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn sidebar_item(
    to: Destination,
    here: Destination,
    reading: Option<String>,
    available: bool,
    tab_index: isize,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> impl IntoElement {
    let selected = to == here;
    let accent = to.accent(dark);
    let keyboard = this.clone();
    let leading = if to.swatch() {
        div()
            .size(px(8.))
            .mx(px(3.))
            .flex_shrink_0()
            .rounded(t::RADIUS_BAR)
            .bg(if available { accent } else { accent.opacity(0.4) })
            .into_any_element()
    } else {
        div()
            .flex_shrink_0()
            .child(icon_img(
                to.icon(),
                14.,
                if selected { t::ink(dark) } else { t::ink_dim(dark) },
            ))
            .into_any_element()
    };
    div()
        .id(to.id())
        .role(Role::Tab)
        .aria_label(to.title())
        .aria_selected(selected)
        .tab_index(tab_index)
        .h(t::CONTROL_H)
        .flex_shrink_0()
        .px(t::SPACE_2)
        .flex()
        .items_center()
        .gap(t::SPACE_2)
        .rounded(t::RADIUS_CONTROL)
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .text_size(t::TEXT_ROW)
        .cursor_pointer()
        .text_color(if available || selected { t::ink(dark) } else { t::ink_dim(dark) })
        .when(selected, |d| {
            d.font_weight(FontWeight::SEMIBOLD)
                .bg(if to.tinted() { accent.opacity(0.14) } else { t::card_hi(dark) })
        })
        .when(!selected, |d| d.hover(|h| h.bg(t::card_hi(dark))))
        .focus_visible(move |d| d.border_color(accent))
        .child(leading)
        .child(div().flex_1().min_w_0().truncate().child(to.title()))
        .when_some(reading, |d, reading| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_size(t::TEXT_SMALL)
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if selected && to.tinted() { accent } else { t::ink_dim(dark) })
                    .child(reading),
            )
        })
        .on_click(move |_, _, cx| navigate(&this, to, cx))
        .capture_key_down(move |event, window, cx| {
            if is_activation_key(&event.keystroke) {
                window.prevent_default();
                navigate(&keyboard, to, cx);
                cx.stop_propagation();
            }
        })
}

fn sidebar_group(label: &str, dark: bool) -> Div {
    div()
        .px(t::SPACE_2)
        .pt(t::SPACE_3)
        .pb(t::SPACE_1)
        .text_size(t::TEXT_CAPTION)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t::ink_dim(dark))
        .child(label.to_uppercase())
}

/// Every place in the app, grouped by what you are doing: watching live
/// readings, looking at your own work, looking back, or tidying up.
fn sidebar(s: &Snapshot, here: Destination, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    use crate::tools_panel::Page;
    let visible = Tab::visible();
    let mut index = 0isize;
    let mut items: Vec<AnyElement> = Vec::new();
    let mut add = |items: &mut Vec<AnyElement>, to: Destination, reading: Option<String>, available: bool| {
        items.push(sidebar_item(to, here, reading, available, index, dark, this.clone()).into_any_element());
        index += 1;
    };

    add(&mut items, Destination::Tab(Tab::Overview), None, true);

    items.push(sidebar_group(l("Live", "Nå"), dark).into_any_element());
    for tab in visible
        .iter()
        .copied()
        .filter(|tab| !matches!(tab, Tab::Overview | Tab::Projects | Tab::History))
        // Battery only where there is one (or a preview of one).
        // Hidden only when macOS confirmed there is none; a failed reading
        // keeps the entry so its page can say so.
        .filter(|tab| *tab != Tab::Battery || !s.battery.confirmed_absent() || crate::battery::preview().is_some())
    {
        let reading = sidebar_reading(tab, s);
        let available = reading.is_some();
        add(&mut items, Destination::Tab(tab), Some(reading.unwrap_or_else(|| "–".into())), available);
    }

    items.push(sidebar_group(l("Work", "Arbeid"), dark).into_any_element());
    add(&mut items, Destination::Apps, Some(s.app_count.to_string()), true);
    // Development servers are part of Agents: what your agents and you
    // started while working.
    add(&mut items, Destination::Agents, Some(s.agents.len().to_string()), true);

    items.push(sidebar_group(l("Over time", "Over tid"), dark).into_any_element());
    if visible.contains(&Tab::History) {
        add(&mut items, Destination::Tab(Tab::History), None, true);
    }
    add(&mut items, Destination::Tool(Page::Baselines), None, true);

    items.push(sidebar_group(l("Maintenance", "Vedlikehold"), dark).into_any_element());
    for page in [Page::Startup, Page::Storage, Page::Packages] {
        add(&mut items, Destination::Tool(page), None, true);
    }
    let settings_index = index;

    div()
        .w(px(224.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(t::sidebar(dark))
        .border_r_1()
        .border_color(t::line(dark))
        // The window's traffic lights sit in this strip.
        .child(div().h(px(52.)).flex_shrink_0())
        .child(
            div()
                .id("sidebar")
                .role(Role::TabList)
                .aria_label(l("Sections", "Seksjoner"))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(t::SPACE_2)
                .pb(t::SPACE_2)
                .flex()
                .flex_col()
                .gap(px(1.))
                .children(items),
        )
        .child(
            div()
                .flex_shrink_0()
                .px(t::SPACE_2)
                .py(t::SPACE_2)
                .border_t_1()
                .border_color(t::line(dark))
                .child(sidebar_item(
                    Destination::Settings,
                    here,
                    Some("⌘,".into()),
                    true,
                    settings_index,
                    dark,
                    this,
                )),
        )
}

/// One row of a list: icon, name, one quiet line, values on the right. A
/// row that can be picked gets a hover, a selected tint, and keyboard
/// activation; its actions go underneath in `row_detail` while selected.
#[allow(clippy::too_many_arguments)]
pub(crate) fn list_row(
    id: impl Into<SharedString>,
    icon: AnyElement,
    title: String,
    meta: Option<String>,
    trailing: Vec<AnyElement>,
    selected: bool,
    dark: bool,
    on_toggle: Option<std::rc::Rc<dyn Fn(&mut App)>>,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let pickable = on_toggle.is_some();
    let keyboard = on_toggle.clone();
    div()
        .id(id)
        .role(if pickable { Role::Button } else { Role::Row })
        .aria_label(SharedString::from(title.clone()))
        .when(pickable, |d| d.aria_selected(selected).tab_index(0))
        .min_h(t::ROW_H)
        .py(t::SPACE_1)
        .px(t::SPACE_3)
        .mx(t::SPACE_2)
        .flex()
        .items_center()
        .gap(t::SPACE_3)
        .rounded(t::RADIUS_CONTROL)
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .when(selected, |d| d.bg(t::card_hi(dark)))
        .when(pickable && !selected, |d| d.hover(|h| h.bg(t::card_hi(dark).opacity(0.5))))
        .when(pickable, |d| d.cursor_pointer().focus_visible(move |d| d.border_color(t::accent(dark))))
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(t::TEXT_ROW).truncate().child(title))
                .when_some(meta, |d, meta| {
                    d.child(div().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).truncate().child(meta))
                }),
        )
        .children(trailing)
        .when_some(on_toggle, |d, toggle| d.on_click(move |_, _, cx| toggle(cx)))
        .when_some(keyboard, |d, toggle| {
            d.capture_key_down(move |event, window, cx| {
                if is_activation_key(&event.keystroke) {
                    window.prevent_default();
                    toggle(cx);
                    cx.stop_propagation();
                }
            })
        })
}

/// The actions and details shown under a selected `list_row`, lined up with
/// its name.
pub(crate) fn row_detail(dark: bool) -> Div {
    div()
        .ml(px(52.))
        .mr(t::SPACE_4)
        .mt(t::SPACE_1)
        .mb(t::SPACE_2)
        .flex()
        .flex_col()
        .gap(t::SPACE_2)
        .text_size(t::TEXT_SMALL)
        .text_color(t::ink_dim(dark))
}

/// A small rounded label beside a row's values.
pub(crate) fn chip(text: impl Into<SharedString>, color: Option<Hsla>, dark: bool) -> Div {
    let color = color.unwrap_or(t::ink_dim(dark));
    div()
        .flex_shrink_0()
        .px(t::SPACE_2)
        .rounded(t::RADIUS_CHIP)
        .bg(color.opacity(0.12))
        .text_color(color)
        .text_size(t::TEXT_SMALL)
        .child(text.into())
}

pub(crate) fn list_card(rows: Vec<Div>, dark: bool) -> Div {
    card(dark).p_0().py_2().gap_0().children(rows)
}

// ---------- Page header ----------

/// What a page shows in its header besides its title: one line saying what
/// the page is for, the page's own actions, and the notes behind ⓘ.
#[derive(Default)]
pub(crate) struct PageChrome {
    pub subtitle: Option<String>,
    pub actions: Vec<AnyElement>,
    pub notes: Vec<&'static str>,
}

fn page_header(
    here: Destination,
    chrome: PageChrome,
    info_open: bool,
    accent: Hsla,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> Div {
    div()
        .min_h(px(52.))
        .flex_shrink_0()
        .px(t::SPACE_5)
        .pt(t::SPACE_3)
        .pb(t::SPACE_2)
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(t::TEXT_TITLE)
                        .font_weight(FontWeight::BOLD)
                        .text_color(if here.tinted() { accent } else { t::ink(dark) })
                        .child(here.title()),
                )
                .when_some(chrome.subtitle, |d, subtitle| {
                    d.child(
                        div()
                            .text_size(t::TEXT_BODY)
                            .text_color(t::ink_dim(dark))
                            .truncate()
                            .child(subtitle),
                    )
                }),
        )
        .child(div().flex().items_center().gap_2().flex_shrink_0().children(chrome.actions))
        .when(!chrome.notes.is_empty(), |d| {
            d.child(crate::controls::icon_toggle(
                "page-info",
                "icons/info.svg",
                l("About this page", "Om denne siden"),
                info_open,
                dark,
                move |_, cx| {
                    this.update(cx, |v, cx| {
                        v.info_for = if v.info_for == Some(here) { None } else { Some(here) };
                        cx.notify();
                    });
                },
            ))
        })
}

impl TideGaugeView {
    fn chrome(
        &self,
        here: Destination,
        dark: bool,
        this: Entity<TideGaugeView>,
        _cx: &mut Context<TideGaugeView>,
    ) -> PageChrome {
        match here {
            Destination::Tab(tab) => PageChrome { notes: tab_notes(tab), ..Default::default() },
            Destination::Apps => crate::process_panel::chrome(self),
            Destination::Agents => PageChrome { notes: crate::agents_panel::notes(), ..Default::default() },
            Destination::Tool(_) => crate::tools_panel::chrome(self, dark, this),
            Destination::Settings => PageChrome { notes: settings_notes(), ..Default::default() },
        }
    }
}

/// What each metric page's numbers mean, kept out of the page itself.
fn tab_notes(tab: Tab) -> Vec<&'static str> {
    match tab {
        Tab::Overview => vec![
            l(
                "Network totals count since the Mac started. Averages and peaks cover this session.",
                "Nettverkstotaler teller siden Macen startet. Gjennomsnitt og topper gjelder denne økten.",
            ),
            shared_memory_note(),
        ],
        Tab::Cpu => vec![
            l(
                "App CPU: 100% is one full core, so a busy app can show more than 100%.",
                "App-CPU: 100 % er én hel kjerne, så en travel app kan vise mer enn 100 %.",
            ),
            l("Chip, SSD and battery come from the sensors macOS names itself. The channel list is the SMC's own; its codes are the hardware's, and some are not temperatures.", "Brikke, SSD og batteri kommer fra sensorene macOS selv navngir. Kanallisten er SMC-ens egen; kodene er maskinvarens, og noen er ikke temperaturer."),
            l("Native channel identifiers are preserved; locations vary by model. Missing or stale readings are never treated as zero. macOS retains automatic fan control.", "Opprinnelige kanalnavn beholdes; plassering varierer med modell. Manglende eller utdaterte målinger behandles aldri som null. macOS beholder automatisk viftekontroll."),
        ],
        Tab::Memory => vec![
            shared_memory_note(),
            l("Memory pressure is macOS's own level: normal, warning or critical. Available is the share of memory macOS itself counts as available. Usage alone does not say whether the Mac is short of memory.", "Minnepress er nivået macOS selv melder: normalt, advarsel eller kritisk. Tilgjengelig er andelen minne macOS selv regner som tilgjengelig. Bruk alene sier ikke om Macen mangler minne."),
        ],
        Tab::Disk => vec![
            l(
                "App activity below is system-wide, not attributed to the selected drive.",
                "Appaktiviteten nedenfor gjelder hele systemet, ikke bare den valgte disken.",
            ),
            l("Inventory refreshes every 30 seconds. Virtual disk images are omitted. No mount, eject, erase, or SMART-setting operations are performed.", "Oversikten oppdateres hvert 30. sekund. Virtuelle diskbilder utelates. Ingen montering, utmating, sletting eller endring av SMART-innstillinger utføres."),
            l("APFS volumes share container space. Shared capacity and free space are shown per container, never summed per volume.", "APFS-volumer deler beholderplass. Delt kapasitet og ledig plass vises per beholder og summeres aldri per volum."),
            l("Device I/O counters reset when the device is reattached; they are not lifetime SMART totals. Rates need two fresh samples.", "Enhetens I/O-tellere nullstilles når enheten kobles til på nytt; de er ikke SMART-totaltall for levetiden. Hastigheter krever to ferske målinger."),
            l(
                "Endurance used is a wear indicator, not a health score, and can exceed 100%. Missing SMART data never means healthy.",
                "Brukt skrivelevetid er en slitasjeindikator, ikke en helsescore, og kan overstige 100 %. Manglende SMART-data betyr aldri frisk.",
            ),
        ],
        Tab::Network => vec![l(
            "Received and sent count since the Mac started.",
            "Mottatt og sendt teller siden Macen startet.",
        )],
        Tab::Gpu => vec![
            l("macOS does not attribute GPU utilization to individual apps on this Mac.", "macOS tilskriver ikke GPU-utnyttelse til enkeltapper på denne Macen."),
            l("GPU power is measured: the change in macOS's GPU energy counter between samples.", "GPU-effekten er målt: endringen i GPU-energitelleren i macOS mellom målingene."),
        ],
        Tab::Battery => vec![
            l("Power per app is the processor energy macOS accounts to each process, averaged over the last two seconds. It leaves out GPU, display, radio and work done for the app by system processes, so graphics-heavy apps read low. It comes from macOS's own energy model, not a meter.", "Effekt per app er prosessorenergien macOS fører på hver prosess, i snitt over de siste to sekundene. GPU, skjerm, radio og arbeid systemprosesser gjør for appen er ikke med, så grafikktunge apper viser for lavt. Den kommer fra macOS sin egen energimodell, ikke en måler."),
            l("Capacity health compares full-charge and design capacity; it may differ from the macOS health percentage.", "Kapasitetshelse sammenligner fullade- og designkapasitet; den kan avvike fra helseprosenten i macOS."),
            l("Battery flow: + charging, − discharging. Adapter rating is not live consumption.", "Batteristrøm: + lading, − utlading. Adapterkapasitet er ikke faktisk forbruk."),
            l("System power is the whole Mac's draw as its power controller measures it, on battery or on the adapter. Macs without a battery do not report it.", "Systemeffekt er hele Macens forbruk slik strømkontrolleren måler det, på batteri eller adapter. Macer uten batteri melder det ikke."),
        ],
        Tab::Sound => vec![
            l("Controls the default output and input devices, not individual apps. Choosing a device here is the same as choosing it in macOS Sound settings.", "Styrer standard lydutgang og -inngang, ikke enkeltapper. Å velge en enhet her er det samme som å velge den i lydinnstillingene i macOS."),
            l("Per-app volume passes that app's sound through Tidegauge (macOS 14.2 or later): macOS asks once for permission to capture audio. Nothing is recorded or saved, and at 100% the app plays on its own again. If Tidegauge quits, every app plays on its own.", "Volum per app sender appens lyd gjennom Tidegauge (macOS 14.2 eller nyere): macOS ber én gang om tillatelse til å fange opp lyd. Ingenting tas opp eller lagres, og på 100 % spiller appen på egen hånd igjen. Avsluttes Tidegauge, spiller alle apper på egen hånd."),
        ],
        Tab::Bluetooth => vec![l(
            "Battery levels show only for devices that report them.",
            "Batterinivå vises bare for enheter som rapporterer det.",
        )],
        Tab::Projects => Vec::new(),
        Tab::History => vec![
            l(
                "History keeps 30 days and is tidied every hour.",
                "Historikken beholdes i 30 dager og ryddes hver time.",
            ),
            l(
                "Values are averages for each interval, never peaks. Missing data is a gap, not zero.",
                "Verdiene er gjennomsnitt per intervall, aldri topper. Manglende data er hull, ikke null.",
            ),
            l(
                "Apps: the 60 using the most memory are recorded, plus the busiest by CPU, disk and network, so an app that is missing may still have been active.",
                "Apper: de 60 som bruker mest minne registreres, pluss de travleste etter CPU, disk og nettverk, så en app som mangler kan likevel ha vært aktiv.",
            ),
            l("Export CSV saves every recorded sample of what is shown, not the averages, to Downloads.", "Eksporter CSV lagrer hver registrerte måling av det som vises, ikke snittene, i Nedlastinger."),
            l("App CPU can exceed 100% across cores. Power is processor energy only, from macOS's energy model, not a meter.", "App-CPU kan overstige 100 % på flere kjerner. Effekt er kun prosessorenergi, fra macOS sin energimodell, ikke en måler."),
        ],
    }
}

fn shared_memory_note() -> &'static str {
    l(
        "Apps share memory, so their sum can be larger than memory in use.",
        "Apper deler minne, så summen kan bli større enn minne i bruk.",
    )
}

fn settings_notes() -> Vec<&'static str> {
    vec![
        l("Settings are saved on this Mac right away.", "Innstillingene lagres på denne Macen med en gang."),
        l("Opt-in. CPU/memory: sustained for 20 seconds; disk/network: 30 seconds. One alert until recovery, with a 10-minute cooldown per metric.", "Valgfritt. CPU/minne: vedvarende i 20 sekunder; disk/nettverk: 30 sekunder. Ett varsel frem til normalisering, med 10 minutters pause per måling."),
        l("One app: its CPU, where 100% is one full core, must stay above the level for 5 minutes. Each app alerts once until it calms down or quits, with its own 10-minute cooldown. One app's memory works the same way, above the chosen GB for 5 minutes.", "Én app: CPU-en, der 100 % er én hel kjerne, må ligge over nivået i 5 minutter. Hver app varsler én gang til den roer seg eller avsluttes, med egen pause på 10 minutter. Minne for én app virker likt, over valgt antall GB i 5 minutter."),
        l("Startup disk space counts the free space macOS reports for the startup volume; purgeable space Finder may also show as available is not included. Overheating follows macOS's own thermal state: serious or critical for 10 seconds.", "Plass på oppstartsdisken teller ledig plass macOS melder for oppstartsvolumet; plass som kan frigjøres og som Finder kan vise som tilgjengelig, er ikke med. Overoppheting følger macOS sin egen termiske tilstand: alvorlig eller kritisk i 10 sekunder."),
        l("Permission is requested with the first alert. macOS notification settings and Focus may suppress delivery. Memory in use measures usage; the memory-pressure alert follows macOS's own level. While pressure is warning or critical, the menu-bar memory reading turns orange or red.", "Tillatelse forespørres ved første varsel. Varselinnstillinger og Fokus i macOS kan hindre levering. Minne i bruk måler bruk; varselet for minnepress følger nivået macOS selv melder. Mens minnepresset er advarsel eller kritisk, blir minnetallet i menylinjen oransje eller rødt."),
        l("Prevents idle system sleep only. The display may sleep; closing the lid still sleeps. Off after quitting or restarting Tidegauge.", "Hindrer kun automatisk systemdvale. Skjermen kan sove; lukket lokk gir fortsatt dvale. Av etter avslutning eller omstart av Tidegauge."),
        l("Overview and Settings stay available. Menu-bar readings are independent.", "Oversikt og Innstillinger er alltid tilgjengelige. Menylinjemålinger er uavhengige."),
    ]
}

/// The notes behind ⓘ: what the numbers mean and where they stop.
fn info_card(notes: &[&'static str], dark: bool, this: Entity<TideGaugeView>) -> Div {
    card(dark)
        .gap_2()
        .bg(t::card_hi(dark).opacity(0.5))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(caption(l("About this page", "Om denne siden"), dark))
                .child(crate::controls::icon_toggle(
                    "page-info-close",
                    "icons/x.svg",
                    l("Close", "Lukk"),
                    false,
                    dark,
                    move |_, cx| {
                        this.update(cx, |v, cx| {
                            v.info_for = None;
                            cx.notify();
                        });
                    },
                )),
        )
        .children(notes.iter().map(|note| sub(*note).text_color(t::ink_dim(dark))))
}

// ---------- Render ----------

impl Render for TideGaugeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.dark = crate::settings::Settings::load().dark.unwrap_or(matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ));
        let dark = self.dark;
        self.sliders.sync(self.snap.audio.device_volume, self.snap.audio.input.volume, window, cx);
        let s = &self.snap;
        let tab = self.tab;
        let this = cx.entity();
        self.scroll = if self.show_settings {
            self.settings_scroll.clone()
        } else if self.show_tools {
            self.tools_scroll.clone()
        } else if self.show_agents {
            self.agents_scroll.clone()
        } else if self.show_processes {
            self.process_scroll.clone()
        } else {
            self.tab_scrolls[tab as usize].clone()
        };

        let here = self.destination();
        // Every page opens with its notes closed: moving elsewhere drops
        // notes opened for another page.
        if self.last_here != Some(here) {
            self.last_here = Some(here);
            if self.info_for != Some(here) {
                self.info_for = None;
            }
        }
        let accent = here.accent(dark);
        let chrome = self.chrome(here, dark, this.clone(), cx);
        let notes = chrome.notes.clone();
        let info_open = self.info_for == Some(here) && !notes.is_empty();
        let header = page_header(here, chrome, info_open, accent, dark, this.clone());
        let info = info_open.then(|| info_card(&notes, dark, this.clone()));

        let content = div()
            .id("content")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(t::SPACE_5)
            .pt(t::SPACE_1)
            .pb(t::SPACE_4)
            .flex()
            .flex_col()
            .gap(t::SECTION_GAP)
            .children(info)
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
            .flex_row()
            .bg(t::bg(dark))
            .text_color(t::ink(dark))
            .font_family(".AppleSystemUIFont")
            .track_focus(&self.focus)
            .child(sidebar(s, here, dark, this.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .relative()
                    .child(header)
                    .child(content)
                    .when_some(self.toast.as_ref(), |d, toast| d.child(toast_view(toast, dark, this.clone()))),
            )
    }
}

fn open_settings(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.show_tools = false;
        view.show_processes = false;
        view.show_agents = false;
        view.show_settings = true;
        view.scroll = ScrollHandle::new();
        view.settings_error = None;
        cx.notify();
    });
}

fn select_tab(this: &Entity<TideGaugeView>, tab: Tab, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.show_tools = false;
        v.show_agents = false;
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
            return crate::settings_page::panel(self, dark, this).into_any_element();
        }
        if self.show_processes {
            return crate::process_panel::panel(self, dark, this).into_any_element();
        }
        if self.show_tools {
            return crate::tools_panel::panel(self, dark, this).into_any_element();
        }
        if self.show_agents {
            // Agents and the development servers running beside them.
            let servers = !self.snap.projects.is_empty() || self.project_error.is_some();
            return div()
                .flex()
                .flex_col()
                .gap(t::SECTION_GAP)
                .child(crate::agents_panel::panel(self, dark, this.clone()))
                .when(servers, |d| {
                    d.child(section_header(l("Development servers", "Utviklingsservere"), "", dark))
                        .when_some(self.project_error.as_ref(), |d, error| {
                            d.child(card(dark).child(sub(error.clone()).text_color(if self.project_stop_failed { t::warn(dark) } else { t::ink_dim(dark) })))
                        })
                        .when(!self.snap.projects.is_empty(), |d| d.child(projects_tab(&self.snap, dark, this.clone())))
                })
                .into_any_element();
        }
        let s = &self.snap;
        match self.tab {
            Tab::Overview => overview(s, dark, this).into_any_element(),
            Tab::Cpu => cpu_tab(s, dark, self.show_sensor_channels, this).into_any_element(),
            Tab::Memory => memory_tab(s, dark, this).into_any_element(),
            Tab::Disk => disk_tab(s, dark, self.selected_disk.as_deref(), this, cx).into_any_element(),
            Tab::Network => network_tab(s, dark, this).into_any_element(),
            Tab::Gpu => gpu_tab(s, dark).into_any_element(),
            Tab::Battery => battery_tab(s, dark).into_any_element(),
            Tab::Sound => div().flex().flex_col().gap(t::SECTION_GAP)
                .when_some(self.audio_error.as_ref(), |d,error|d.child(card(dark).child(sub(error.clone()).text_color(t::warn(dark)))))
                .child(sound_tab(s, &self.sliders.volume, &self.sliders.input_volume, dark, this, self.audio_pending)).into_any_element(),
            Tab::Bluetooth => bt_tab(s, dark).into_any_element(),
            Tab::Projects => div()
                .flex()
                .flex_col()
                .gap(t::SECTION_GAP)
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

fn metric_heading(tab: Tab, dark: bool) -> Div {
    let color = tab.metric().map(|m| m.accent(dark)).unwrap_or_else(|| t::accent(dark));
    div().flex().items_center().gap_2()
        .child(div().size(px(22.)).rounded(t::RADIUS_CONTROL).bg(color.opacity(0.14))
            .flex().items_center().justify_center().child(icon_img(tab.icon(), 13., color)))
        .child(div().text_size(t::TEXT_BODY).font_weight(FontWeight::MEDIUM)
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
    // No reading: say so quietly instead of a big dash, and keep only the
    // supporting values that exist.
    let unavailable = value == "—";
    // An empty value means "nothing to say here", so the reading is left out.
    let stats: Vec<(&str, String)> = stats
        .into_iter()
        .filter(|(_, v)| !v.is_empty() && (!unavailable || v != "—"))
        .collect();
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
        .capture_key_down(move |event, window, cx| {
            if is_activation_key(&event.keystroke) {
                window.prevent_default();
                select_tab(&keyboard_target, tab, cx);
                cx.stop_propagation();
            }
        })
        .min_w_0()
        .h(px(if compact { 154. } else { 208. }))
        .p_4()
        .rounded(t::RADIUS_CARD)
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
                        .text_size(t::TEXT_SMALL)
                        .line_height(px(16.))
                        .text_color(t::ink_dim(dark))
                        .child(context),
                )
                .when(unavailable, |d| {
                    d.child(
                        div()
                            .h(px(42.))
                            .flex()
                            .items_center()
                            .text_size(t::TEXT_EMPH)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t::ink_dim(dark))
                            .child(l("Not available on this Mac", "Ikke tilgjengelig på denne Macen")),
                    )
                })
                .when(!unavailable, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(t::SPACE_2)
                            .child(
                                div()
                                    .text_size(t::TEXT_STAT)
                                    .line_height(px(42.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(number.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(t::TEXT_EMPH)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t::ink_dim(dark))
                                    .child(unit.to_string()),
                            ),
                    )
                }),
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
                                    .text_size(t::TEXT_CAPTION)
                                    .line_height(px(16.))
                                    .text_color(t::ink_dim(dark))
                                    .child(label.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(t::TEXT_BODY)
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
                    .rounded(t::RADIUS_CONTROL)
                    .px(t::SPACE_2)
                    .py(t::SPACE_1)
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
    // Time left on battery, or time to full while charging; on power and
    // full there is nothing to count, so the reading is left out.
    let (remaining_label, remaining) = if s.battery.on_battery {
        (l("Remaining", "Gjenstår"), s.battery.minutes_remaining.filter(|m| *m >= 0).map(fmt_hours_minutes).unwrap_or("—".into()))
    } else if s.battery.is_charging {
        (l("Until full", "Til fulladet"), s.battery.minutes_to_full.filter(|m| *m > 0).map(fmt_hours_minutes).unwrap_or_default())
    } else {
        (l("Remaining", "Gjenstår"), String::new())
    };
    let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
    let settings = crate::settings::Settings::load();
    let favorites = &settings.favorite_metrics;
    let cards = vec![
        (
            Tab::Cpu,
            l("Now", "Nå").into(),
            fmt_pct(s.cpu_pct),
            [
                (l("Load · 1 min", "Last · 1 min"), crate::locale::number(s.load_avg[0], 2)),
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
                (l("Compressed", "Komprimert"), fmt_bytes(s.mem.compressed as f64)),
                (l("Pressure", "Minnepress"), s.mem.pressure.map(|p| p.label().to_string()).unwrap_or("—".into())),
            ],
            &s.mem_hist,
            Some(100.),
        ),
        (
            Tab::Gpu,
            l("GPU utilization", "GPU-utnyttelse").into(),
            s.gpu
                .utilization
                .map(fmt_pct)
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
                    l("Power", "Effekt"),
                    s.gpu.power_w.map(crate::engine::fmt_watts).unwrap_or("—".into()),
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
                (l("Received", "Mottatt"), fmt_bytes(s.net.total_in as f64)),
                (l("Sent", "Sendt"), fmt_bytes(s.net.total_out as f64)),
            ],
            &s.net_in_hist,
            None,
        ),
        (
            Tab::Battery,
            battery_context.into(),
            s.battery
                .percent
                .map(fmt_pct)
                .unwrap_or("—".into()),
            [
                (remaining_label, remaining),
                (
                    crate::locale::text("Battery flow", "Batteristrøm"),
                    // No flow worth showing when nothing goes in or out.
                    match s.battery.power_w {
                        Some(w) if w.abs() >= 0.05 => fmt_signed_watts(w),
                        Some(_) => String::new(),
                        None => "—".into(),
                    },
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
    let cards: Vec<_> = cards
        .into_iter()
        .filter(|(tab, ..)| favorites.iter().any(|k| k == tab.key()))
        // A Mac without a battery gets no battery card.
        .filter(|(tab, ..)| *tab != Tab::Battery || !s.battery.confirmed_absent())
        .collect();
    // A short last row takes the busiest-apps card instead of leaving a
    // hole; otherwise it sits beside the donuts below.
    let fill_grid = cards.len() % 3 != 0;
    // The filler spans whatever the short row has left: one or two columns.
    let fill_span = (3 - cards.len() % 3) as u16;
    let show_power = favorites.iter().any(|key| key == "battery") && s.apps.iter().any(|a| a.power_w > 0.);
    let show_memory = favorites.iter().any(|key| key == "memory");
    let bottom_count = 2 * show_memory as usize + show_power as usize + (!fill_grid) as usize;
    div()
        .flex()
        .flex_col()
        .gap(if settings.compact_dashboard { t::SPACE_2 } else { t::SPACE_4 })
        .when_some(crate::agents_panel::plan_strip(&s.plans, dark, this.clone()), |d, strip| d.child(strip))
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
                .gap(if settings.compact_dashboard { t::SPACE_2 } else { t::SPACE_4 })
                .children(cards.into_iter().map(|(tab, context, value, stats, hist, max)| {
                    overview_card(tab, context, value, stats, hist, max, dark, this.clone()).into_any_element()
                }))
                .when(fill_grid && !favorites.is_empty(), |d| {
                    let compact = settings.compact_dashboard;
                    d.child(div().col_span(fill_span).child(busiest_apps(s, if compact { 154. } else { 208. }, if compact { 4 } else { 6 }, dark, this.clone())))
                }),
        )
        .when(!settings.compact_dashboard && bottom_count > 0, |d| {
            d.child(
                div()
                    .grid()
                    // Four cards in one row cut the legends off, so four
                    // make two rows of two.
                    .grid_cols(bottom_cols(bottom_count))
                    .gap(t::SPACE_4)
                    .when(show_memory, |row| row.child(donut_card(
                        l("Memory by Type", "Minne etter type"),
                        Tab::Memory,
                        vec![
                            BreakdownEntry::new(l("App", "App"), s.mem.app as f64, Metric::Memory.accent(dark)),
                            BreakdownEntry::new(
                                l("Wired", "Bundet"),
                                s.mem.wired as f64,
                                Metric::Memory.accent(dark).opacity(0.62),
                            ),
                            BreakdownEntry::new(
                                l("Compressed", "Komprimert"),
                                s.mem.compressed as f64,
                                Metric::Memory.accent(dark).opacity(0.36),
                            ),
                            BreakdownEntry::new(l("Cached", "Hurtigbufret"), s.mem.cached as f64, t::ink_dim(dark)),
                            BreakdownEntry::new(l("Free", "Ledig"), s.mem.free as f64, t::card_hi(dark)),
                        ],
                        fmt_pct(mem_pct),
                        l("in use", "i bruk"),
                        fmt_bytes,
                        dark,
                    ))
                    .child(top_apps_donut(s, dark)))
                    .when(show_power, |row| row.child(power_donut(s, dark)))
                    .when(!fill_grid, |row| row.child(busiest_apps(s, 184., 5, dark, this.clone()))),
            )
        })
}

/// Draw a ring from convex quadrilaterals. GPUI triangulates filled paths
/// as a fan, so a concave wedge would incorrectly paint across the center.
pub(crate) fn donut(slices: &[(f64, Hsla)], size: f32, empty: Hsla) -> Canvas<()> {
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
                vec![(1., empty)]
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
        div().flex().items_center().gap(t::SPACE_2).h(px(23.))
            .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(entry.color))
            .when_some(entry.app.as_ref(), |d, app| d.child(app_icon(app, dark)))
            .child(div().flex_1().min_w_0().text_size(t::TEXT_SMALL).truncate().child(entry.name.clone()))
            .child(div().flex_shrink_0().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark))
                .child(format(entry.value)))
    }).collect::<Vec<_>>();
    div().min_w_0().h(px(184.)).rounded(t::RADIUS_CARD).bg(t::card(dark)).p(t::SPACE_4)
        .flex().flex_col().gap(t::SPACE_3)
        .child(div().flex().items_center().gap_2()
            .child(div().size(px(22.)).rounded(t::RADIUS_CONTROL).bg(tab.metric().unwrap().soft(dark))
                .flex().items_center().justify_center()
                .child(icon_img(tab.icon(), 13., tab.metric().unwrap().accent(dark))))
            .child(div().text_size(t::TEXT_BODY).font_weight(FontWeight::MEDIUM)
                .text_color(tab.metric().unwrap().accent(dark)).child(label.to_string())))
        .child(div().flex().items_center().gap(t::SPACE_4)
            .child(div().relative().size(px(92.)).flex_shrink_0()
                .child(donut(&chart, 92., t::card_hi(dark)))
                .child(div().absolute().inset_0().flex().flex_col().items_center().justify_center()
                    .child(div().text_size(t::TEXT_BODY).font_weight(FontWeight::SEMIBOLD).child(center))
                    .child(div().text_size(t::TEXT_CAPTION).whitespace_nowrap().text_color(t::ink_dim(dark)).child(center_label.to_string()))))
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

/// The apps using the most CPU right now; fills the Overview's third slot
/// when there is no power chart, and opens the full list on click.
fn busiest_apps(s: &Snapshot, height: f32, rows: usize, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    let color = Metric::Cpu.accent(dark);
    let mut top: Vec<&AppGroup> = s.apps.iter().filter(|a| a.cpu_pct > 0.).collect();
    top.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
    let keyboard = this.clone();
    div()
        .id("overview-busiest")
        .role(Role::Button)
        .aria_label(l("Busiest apps. Open Apps & processes", "Mest aktive apper. Åpne Apper og prosesser"))
        .tab_index(40)
        .min_w_0()
        .h(px(height))
        .rounded(t::RADIUS_CARD)
        .bg(t::card(dark))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .focus_visible(move |d| d.border_color(color))
        .p_4()
        .flex()
        .flex_col()
        .gap(t::SPACE_3)
        .cursor_pointer()
        .hover(|d| d.bg(t::card_hi(dark)))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .size(px(22.))
                                .rounded(t::RADIUS_CONTROL)
                                .bg(Metric::Cpu.soft(dark))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon_img("icons/layout-grid.svg", 13., color)),
                        )
                        .child(
                            div()
                                .text_size(t::TEXT_BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(color)
                                .child(l("Busiest apps", "Mest aktive apper")),
                        ),
                )
                .child(icon_img("icons/chevron-right.svg", 12., t::ink_dim(dark))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .when(top.is_empty(), |d| {
                    d.child(sub(l("No app is using this right now.", "Ingen app bruker dette akkurat nå.")).text_color(t::ink_dim(dark)))
                })
                .children(top.iter().take(rows).map(|app| {
                    div()
                        .flex()
                        .items_center()
                        .gap(t::SPACE_2)
                        .h(px(23.))
                        .child(app_icon(app, dark))
                        .child(div().flex_1().min_w_0().text_size(t::TEXT_SMALL).truncate().child(app.name.clone()))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(t::TEXT_SMALL)
                                .text_color(t::ink_dim(dark))
                                .child(fmt_pct(app.cpu_pct)),
                        )
                })),
        )
        .on_click(move |_, _, cx| {
            this.update(cx, |v, _| {
                v.process_sort_cpu = true;
                v.process_order.clear();
            });
            crate::process_panel::open(&this, cx);
        })
        .capture_key_down(move |event, window, cx| {
            if is_activation_key(&event.keystroke) {
                window.prevent_default();
                crate::process_panel::open(&keyboard, cx);
                cx.stop_propagation();
            }
        })
}

/// Columns for the Overview's bottom row: never more than three cards side by
/// side, and four as two rows of two.
fn bottom_cols(cards: usize) -> u16 {
    match cards {
        4 => 2,
        n => n.clamp(2, 3) as u16,
    }
}

fn breakdown_watts(watts: f64) -> String {
    if watts <= 0. { "0 W".into() }
    else if watts < 0.001 { "<1 mW".into() }
    else if watts < 1. { format!("{} mW", crate::locale::number(watts * 1000., 0)) }
    else { fmt_watts(watts) }
}

fn power_donut(s: &Snapshot, dark: bool) -> Div {
    let color = Metric::Power.accent(dark);
    let mut top: Vec<&AppGroup> = s.apps.iter().filter(|a| a.power_w > 0.).collect();
    if top.is_empty() {
        return empty_state(
            Tab::Battery.icon(),
            color,
            l("App energy unavailable", "Appenergi utilgjengelig").into(),
            l("No per-app energy estimates are currently available. This does not mean zero power consumption.", "Ingen energianslag per app er tilgjengelige nå. Dette betyr ikke null strømforbruk.").into(),
            dark,
        );
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
    donut_card(l("CPU Power by App", "CPU-effekt etter app"), Tab::Battery, slices, breakdown_watts(total), l("CPU only", "kun CPU"), breakdown_watts, dark)
}

// ---------- metric tabs ----------

/// macOS's name for a core type, in the app's language.
fn core_kind(name: &str) -> String {
    match name {
        "Super" => "Super".into(),
        "Performance" => l("Performance", "Ytelse").into(),
        "Efficiency" => l("Efficiency", "Effektivitet").into(),
        other => other.to_string(),
    }
}

/// One card per core type: its clock speed and a small bar for each core.
fn cores_card(s: &Snapshot, dark: bool) -> Option<Div> {
    if s.core_pct.is_empty() || s.chip.groups.is_empty() {
        return None;
    }
    let accent = Metric::Cpu.accent(dark);
    let group = |g: &crate::metrics::CoreGroup| {
        let cores = s.core_pct.get(g.first..g.first + g.count).unwrap_or(&[]);
        let average = if cores.is_empty() { 0. } else { cores.iter().sum::<f64>() / cores.len() as f64 };
        let mut header = format!("{} · {} {}", core_kind(&g.name), g.count, crate::locale::plural(g.count as u64, "core", "cores", "kjerne", "kjerner"));
        if let Some(mhz) = s.clocks.cpu.get(&g.kind) {
            header.push_str(" · ");
            header.push_str(&crate::engine::fmt_clock(*mhz));
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(div().text_size(t::TEXT_ROW).font_weight(FontWeight::SEMIBOLD).child(header))
                    .child(div().text_size(t::TEXT_CAPTION).text_color(t::ink_dim(dark)).child(format!("{} {}", l("Average", "Gjennomsnitt"), fmt_pct(average)))),
            )
            .child(div().flex().flex_wrap().gap(t::SPACE_1).children(cores.iter().map(|pct| {
                div()
                    .w(px(14.))
                    .h(px(40.))
                    .rounded(t::RADIUS_CHIP)
                    .bg(t::card_hi(dark))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .overflow_hidden()
                    .child(div().w_full().h(relative((*pct / 100.).clamp(0.02, 1.) as f32)).bg(accent))
            })))
    };
    Some(
        card(dark)
            .gap_4()
            .child(section_header(l("Cores", "Kjerner"), l("Use per core · clock speed from macOS", "Bruk per kjerne · klokkefrekvens fra macOS"), dark))
            .children(s.chip.groups.iter().rev().map(group)),
    )
}

fn cpu_tab(s: &Snapshot, dark: bool, open_sensors: bool, this: Entity<TideGaugeView>) -> Div {
    let m = Metric::Cpu;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap());
    let max = rows.first().map(|a| a.cpu_pct).unwrap_or(1.0).max(1.0);
    div()
        .flex()
        .flex_col()
        .gap(t::SECTION_GAP)
        .child(hero(
            l("CPU usage", "CPU-bruk"),
            fmt_pct(s.cpu_pct),
            if s.chip.groups.len() > 1 {
                format!(
                    "{} · {} {}",
                    s.chip.brand,
                    s.chip.groups.iter().rev().map(|g| format!("{} {}", g.count, core_kind(&g.name))).collect::<Vec<_>>().join(" + "),
                    crate::locale::plural(s.chip.cores as u64, "core", "cores", "kjerne", "kjerner")
                )
            } else {
                format!(
                    "{} · {} {}",
                    s.chip.brand,
                    s.chip.cores,
                    crate::locale::plural(s.chip.cores as u64, "core", "cores", "kjerne", "kjerner")
                )
            },
            &s.cpu_hist,
            m.accent(dark),
            dark,
            vec![
                (
                    l("Load · 1 / 5 / 15 min", "Last · 1 / 5 / 15 min").into(),
                    format!(
                        "{} / {} / {}",
                        crate::locale::number(s.load_avg[0], 2),
                        crate::locale::number(s.load_avg[1], 2),
                        crate::locale::number(s.load_avg[2], 2)
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
        .when_some(cores_card(s, dark), |d, c| d.child(c))
        .child(section_header(
            l("Apps · highest usage first", "Apper · høyest bruk først"),
            l("CPU · 100% per core", "CPU · 100 % per kjerne"),
            dark,
        ))
        .child(top_apps(rows.iter().copied(), |a| (fmt_pct(a.cpu_pct), a.cpu_pct), max, m.accent(dark), true, dark, this.clone()))
        .child(sensor_panel(s, dark, open_sensors, this))
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
fn bound_temperature_display(
    values: &[crate::sensors::Reading],
    fresh: bool,
) -> TemperatureDisplay {
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
        unavailable: if fresh {
            values.iter().filter(|r| r.value.is_none()).count()
        } else {
            values.len()
        },
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
        if total == 1 { l("channel", "kanal") } else { l("channels", "kanaler") }
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

/// Hottest and average of the channels that reported, or `None` without any.
fn temperature_summary(values: &[crate::sensors::Reading], fresh: bool) -> Option<(f64, f64)> {
    let reported: Vec<f64> = values.iter().filter_map(|r| r.value).filter(|_| fresh).collect();
    let hottest = reported.iter().copied().reduce(f64::max)?;
    Some((hottest, reported.iter().sum::<f64>() / reported.len() as f64))
}

fn sensor_panel(s: &Snapshot, dark: bool, open: bool, this: Entity<TideGaugeView>) -> Div {
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
    let temperature_display = bound_temperature_display(&sample.temperatures, fresh);
    let temperature_caption = temperature_display_caption(&temperature_display, l);
    // macOS's named sensors lead: they are real temperatures of named parts.
    // Without them (a virtual or Intel Mac), the SMC channels stand in.
    let named = if fresh { crate::named_sensors::summarise(&sample.named) } else { Vec::new() };
    let summary = named.is_empty().then(|| temperature_summary(&sample.temperatures, fresh)).flatten();
    let reading_row = move |reading: &crate::sensors::Reading, unit: &'static str| {
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
                        .map(|v| if unit == "RPM" { format!("{:.0} RPM", v) } else { fmt_celsius(v) })
                        .unwrap_or_else(|| l("Unavailable", "Utilgjengelig").into()),
                ),
            )
    };
    let summary_stat = |label: String, value: f64| {
        div()
            .flex()
            .flex_col()
            .gap(t::SPACE_1)
            .child(div().text_size(t::TEXT_CAPTION).text_color(t::ink_dim(dark)).child(label))
            .child(div().text_size(t::TEXT_EMPH).font_weight(FontWeight::SEMIBOLD).child(fmt_celsius(value)))
    };
    // The channel list is long and its names are the hardware's own, so it
    // stays folded behind the hottest and average readings.
    let temperatures = card(dark)
        .child(section_header(l("Temperature channels", "Temperaturkanaler"), "°C", dark))
        .when(sample.temperatures.is_empty(), |d| {
            d.child(sub(if sample.discovering {
                l("Discovering temperature channels…", "Finner temperaturkanaler…")
            } else {
                l("Temperature channels unavailable. This does not mean the Mac is cool.", "Temperaturkanaler utilgjengelige. Dette betyr ikke at Macen er kjølig.")
            }).text_color(t::ink_dim(dark)))
        })
        .when(!named.is_empty(), |d| {
            d.child(div().flex().flex_wrap().gap(t::SPACE_6).children(named.iter().flat_map(|part| {
                // The chip has dozens of sensors: its hottest and average. A
                // part with one or a few reads as its hottest.
                let stats = match part.kind {
                    crate::named_sensors::Kind::Chip => vec![
                        (format!("{} · {}", part.kind.label(), l("hottest", "varmest")), part.hottest),
                        (format!("{} · {}", part.kind.label(), l("average", "snitt")), part.average),
                    ],
                    _ => vec![(part.kind.label().to_string(), part.hottest)],
                };
                stats.into_iter().map(|(label, value)| summary_stat(label, value))
            })))
        })
        .when_some(summary, |d, (hottest, average)| {
            d.child(div().flex().gap(t::SPACE_6)
                .child(summary_stat(l("Hottest", "Varmest").into(), hottest))
                .child(summary_stat(l("Average", "Gjennomsnitt").into(), average)))
        })
        .when(open, |d| {
            d.children(temperature_display.rows.iter().map(|r| reading_row(r, "°C")))
                .when_some(temperature_caption, |d, caption| d.child(sub(caption).text_color(t::ink_dim(dark))))
        })
        .when(!sample.temperatures.is_empty(), |d| {
            d.child(div().flex().child(crate::controls::button(
                "sensor-channels",
                if open {
                    l("Hide channels", "Skjul kanaler").to_string()
                } else {
                    format!("{} ({})", l("Show channels", "Vis kanaler"), sample.temperatures.len())
                },
                false,
                dark,
                move |_, cx| {
                    this.update(cx, |v, cx| {
                        v.show_sensor_channels = !v.show_sensor_channels;
                        cx.notify();
                    });
                },
            )))
        });
    let fans = card(dark)
        .child(section_header(l("Fan speed", "Viftehastighet"), "RPM", dark))
        .when(sample.fans.is_empty(), |d| {
            d.child(sub(if sample.fan_count == Some(0) && fresh {
                l("The controller reports no fans.", "Kontrolleren rapporterer ingen vifter.")
            } else {
                l("Fan readings unavailable.", "Viftemålinger er utilgjengelige.")
            }).text_color(t::ink_dim(dark)))
        })
        .children(sample.fans.iter().enumerate().map(|(i, r)| {
            let (min, max) = sample.fan_limits.get(i).copied().unwrap_or((None, None));
            let share = r.value.filter(|_| fresh).and_then(|rpm| crate::sensors::fan_share(rpm, max));
            div()
                .flex()
                .flex_col()
                .gap(t::SPACE_1)
                .child(reading_row(r, "RPM"))
                .when_some(share, |d, share| {
                    let accent = Metric::Cpu.accent(dark);
                    d.child(
                        div()
                            .h(px(4.))
                            .w_full()
                            .rounded(t::RADIUS_CHIP)
                            .bg(t::card_hi(dark))
                            .child(div().h_full().rounded(t::RADIUS_CHIP).bg(accent).w(relative((share / 100.) as f32))),
                    )
                    .child(
                        div().text_size(t::TEXT_CAPTION).text_color(t::ink_dim(dark)).child(format!(
                            "{} {} · {} {:.0} RPM",
                            fmt_pct(share),
                            l("of top speed", "av toppfart"),
                            l("lowest", "lavest"),
                            min.unwrap_or(0.)
                        )),
                    )
                })
        }));
    let thermal = format!("{}: {}", l("macOS thermal state", "Termisk tilstand fra macOS"), s.thermal);
    div().flex().flex_col().gap_3()
        .child(section_header(l("Temperatures and fans", "Temperaturer og vifter"), &thermal, dark))
        .when(preview.is_some(), |d| d.child(card(dark).child(sub(l("DEVELOPMENT PREVIEW · Sample sensor values, not hardware readings", "UTVIKLINGSVISNING · Eksempelverdier, ikke sensormålinger")).text_color(t::ink_dim(dark)))))
        .child(div().grid().grid_cols(2).gap_3().items_start().child(temperatures).child(fans))
        // Only a source that isn't working is worth a line.
        .when(!fresh || !matches!(sample.source, crate::sensors::SourceState::Ready), |d| {
            d.child(sub(source).text_color(t::ink_dim(dark)))
        })
        .when(sample.inventory_limited, |d| d.child(sub(l("Sensor inventory is limited; some channels are not shown.", "Sensoroversikten er begrenset; enkelte kanaler vises ikke.")).text_color(t::ink_dim(dark))))
}

/// Green, amber, red: the colours Activity Monitor gives the same levels.
fn pressure_color(pressure: crate::metrics::Pressure, dark: bool) -> Hsla {
    match pressure {
        crate::metrics::Pressure::Normal => t::ok(dark),
        crate::metrics::Pressure::Warning => t::caution(dark),
        crate::metrics::Pressure::Critical => t::warn(dark),
    }
}

fn memory_tab(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let m = Metric::Memory;
    let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
    let mut rows: Vec<&AppGroup> = s.apps.iter().collect();
    rows.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    let max = rows.first().map(|a| a.mem_bytes).unwrap_or(1) as f64;
    div()
        .flex()
        .flex_col()
        .gap(t::SECTION_GAP)
        .child(hero(
            l("Memory", "Minne"),
            fmt_pct(mem_pct),
            format!("{} {} {}", fmt_bytes(s.mem.used as f64), l("used of", "brukt av"), fmt_bytes(s.mem.total as f64)),
            &s.mem_hist,
            m.accent(dark),
            dark,
            vec![
                (l("App memory", "Appminne").into(), fmt_bytes(s.mem.app as f64)),
                (l("Wired", "Bundet").into(), fmt_bytes(s.mem.wired as f64)),
                (l("Compressed", "Komprimert").into(), fmt_bytes(s.mem.compressed as f64)),
                (l("Cached", "Hurtigbufret").into(), fmt_bytes(s.mem.cached as f64)),
                (l("Swap used", "Brukt vekselminne").into(), fmt_bytes(s.mem.swap_used as f64)),
            ],
            Some(100.0),
        ))
        .child(
            div()
                .grid()
                .grid_cols(2)
                .gap_3()
                .child(stat_card(
                    l("Memory pressure · from macOS", "Minnepress · fra macOS"),
                    s.mem.pressure.map(|p| p.label().to_string()).unwrap_or("—".into()),
                    s.mem.pressure.map(|p| pressure_color(p, dark)),
                    dark,
                ))
                .child(stat_card(
                    l("Available · from macOS", "Tilgjengelig · fra macOS"),
                    s.mem.available_pct.map(|p| fmt_pct(p as f64)).unwrap_or("—".into()),
                    None,
                    dark,
                )),
        )
        .child(section_header(l("Apps · highest usage first", "Apper · høyest bruk først"), l("Memory", "Minne"), dark))
        .child(top_apps(rows.iter().copied(), |a| (fmt_bytes(a.mem_bytes as f64), a.mem_bytes as f64), max, m.accent(dark), false, dark, this))
        .child(sub(l("App totals and system-used memory have different accounting scopes and may not reconcile. Pressure comes from macOS, not from these totals.", "Apptotaler og brukt systemminne har ulikt beregningsgrunnlag og samsvarer ikke nødvendigvis. Minnepresset kommer fra macOS, ikke fra disse summene.")).text_color(t::ink_dim(dark)))
}

fn disk_tab(
    s: &Snapshot,
    dark: bool,
    selected: Option<&str>,
    this: Entity<TideGaugeView>,
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
        .gap(t::SECTION_GAP)
        .child(hero(
            l("Disk activity · all devices", "Diskaktivitet · alle enheter"),
            fmt_rate(s.disk.read_rate + s.disk.write_rate),
            l("System-wide read + write", "Lesing + skriving i hele systemet").into(),
            &io_hist,
            m.accent(dark),
            dark,
            vec![
                (l("Reading", "Leser").into(), fmt_rate(s.disk.read_rate)),
                (l("Writing", "Skriver").into(), fmt_rate(s.disk.write_rate)),
            ],
            None,
        ))
        .child(section_header(l("Storage devices", "Lagringsenheter"), "", dark))
        .when(is_preview, |d| d.child(card(dark).child(sub(l("DEVELOPMENT PREVIEW · Sample disk values, not hardware readings", "UTVIKLINGSVISNING · Eksempelverdier, ikke diskmålinger")).text_color(t::ink_dim(dark)))))
        // One drive needs no picker: its card below already names it. A
        // selection that has gone away keeps it, so the remaining drive can
        // be picked again.
        .when(inventory.devices.len() > 1 || device.is_none(), |d| d.child(div().id("disk-device-picker").role(Role::RadioGroup).aria_label(l("Storage devices", "Lagringsenheter")).tab_group().flex().flex_wrap().gap_2().children(inventory.devices.iter().enumerate().map(|(index, device)| {
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
                .capture_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                    if is_activation_key(&event.keystroke) {
                        window.prevent_default(); view.selected_disk = Some(key_id.clone()); cx.notify(); cx.stop_propagation();
                    }
                }))
                .on_click(cx.listener(move |view, _, _, cx| { view.selected_disk = Some(id.clone()); cx.notify(); }))
        }))))
        .when(!inventory.fresh(), |d| d.child(if inventory.collected_at.is_none() {
            empty_state(Tab::Disk.icon(), m.accent(dark),
                l("Discovering storage devices…", "Finner lagringsenheter…").into(),
                l("Capacity and health appear once the inventory has been read.", "Kapasitet og helsetilstand vises når oversikten er lest.").into(), dark)
        } else {
            empty_state(Tab::Disk.icon(), m.accent(dark),
                l("Disk inventory unavailable or stale", "Diskoversikten er utilgjengelig eller utdatert").into(),
                l("No capacity or health is assumed.", "Ingen kapasitet eller helsetilstand antas.").into(), dark)
        }))
        .when(inventory.partial, |d| d.child(sub(l("Partial inventory · some device information could not be read.", "Delvis oversikt · enkelte enhetsopplysninger kunne ikke leses.")).text_color(t::ink_dim(dark))))
        .when(inventory.fresh() && device.is_none(), |d| d.child(empty_state(Tab::Disk.icon(), m.accent(dark),
            l("Storage device unavailable", "Lagringsenhet utilgjengelig").into(),
            l("Selected device unavailable, or no supported devices found.", "Valgt enhet er utilgjengelig, eller ingen støttede enheter ble funnet.").into(), dark)))
        .when(device.is_some_and(|device| !device.identity_matches(&io)), |d| d.child(empty_state(Tab::Disk.icon(), m.accent(dark),
            l("Device unavailable", "Enheten er utilgjengelig").into(),
            l("Device disconnected, replaced, or temporarily unavailable. Waiting for a fresh inventory.", "Enheten er frakoblet, erstattet eller midlertidig utilgjengelig. Venter på en ny oversikt.").into(), dark)))
        .when_some(device.filter(|device| inventory.fresh() && device.identity_matches(&io)), |d, device| d.child(disk_detail(device, io.get(&device.id).filter(|sample| sample.fresh()), dark)))
        .child(section_header(l("All apps · all devices", "Alle apper · alle enheter"), l("Disk activity", "Diskaktivitet"), dark))
        .child(top_apps(rows.iter().copied(), |a| { let r = a.disk_read_rate + a.disk_write_rate; (fmt_rate(r), r) }, max, m.accent(dark), true, dark, this))
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
    let flow = |title: &'static str, rate: Option<f64>, history: &[f64], total: Option<u64>| {
        card(dark)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(caption(title, dark))
                    .child(
                        div()
                            .text_size(t::TEXT_CAPTION)
                            .text_color(t::ink_dim(dark))
                            .child(format!("{} {}", l("Counter", "Teller"), bytes(total))),
                    ),
            )
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
            .child(div().flex().items_center().gap_3()
                .child(div().flex_1().min_w_0()
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(device.name.clone()))
                    .child(sub(format!("{} · {} · {} · {}", device.id, location, media, device.protocol.as_deref().unwrap_or(l("Protocol unavailable", "Protokoll utilgjengelig")))).text_color(t::ink_dim(dark))))
                .child(div().flex_shrink_0().flex().flex_col().items_end().gap_1()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(bytes(device.size)))
                    .child(caption(l("Physical capacity", "Fysisk kapasitet"), dark)))))
        .when(device.smart.health == Health::Failed, |d| d.child(card(dark).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("SMART reports a failure. Back up important data and inspect this drive.", "SMART rapporterer en feil. Sikkerhetskopier viktige data og undersøk disken.")))))
        .child(section_header(l("Volumes and containers", "Volumer og beholdere"), "", dark))
        .when(device.volumes.is_empty(), |d| d.child(sub(l("No readable volume information.", "Ingen lesbar voluminformasjon.")).text_color(t::ink_dim(dark))))
        .children(crate::disks::volume_groups(&device.volumes).iter().map(|group| {
            let mut volume_card = card(dark);
            match &group.container {
                Some(container) => {
                    volume_card = volume_card
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(format!("APFS {container}")),
                                        )
                                        .when(!group.stores.is_empty(), |d| {
                                            d.child(
                                                sub(group.stores.join(", "))
                                                    .text_color(t::ink_dim(dark)),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .gap_4()
                                        .children(
                                            [
                                                (
                                                    l("Shared capacity", "Delt kapasitet"),
                                                    bytes(group.total),
                                                ),
                                                (l("Shared free", "Delt ledig"), bytes(group.free)),
                                            ]
                                            .into_iter()
                                            .map(|(label, value)| {
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .items_end()
                                                    .gap_1()
                                                    .child(caption(label, dark))
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .child(value),
                                                    )
                                            }),
                                        ),
                                ),
                        )
                        // How full the shared space is, at a glance.
                        .when_some(group.total.zip(group.free).filter(|(total, _)| *total > 0), |d, (total, free)| {
                            let used = total.saturating_sub(free) as f32 / total as f32;
                            d.child(
                                div()
                                    .h(px(8.))
                                    .rounded_full()
                                    .bg(t::card_hi(dark))
                                    .child(div().h_full().rounded_full().w(relative(used.clamp(0.01, 1.))).bg(Metric::Disk.accent(dark))),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .pt(t::SPACE_1)
                                .child(
                                    div()
                                        .w(px(150.))
                                        .flex_shrink_0()
                                        .child(caption(l("Volume", "Volum"), dark)),
                                )
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .w(px(90.))
                                        .flex_shrink_0()
                                        .text_right()
                                        .child(caption(
                                            l("Volume footprint", "Volumets plassbruk"),
                                            dark,
                                        )),
                                ),
                        )
                        .children(group.volumes.iter().map(|volume| {
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .py(t::SPACE_1)
                                .child(
                                    div()
                                        .w(px(150.))
                                        .flex_shrink_0()
                                        .min_w_0()
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(volume.name.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_xs()
                                        .text_color(t::ink_dim(dark))
                                        .child(format!(
                                            "{} · {}",
                                            volume.id,
                                            volume.mount.clone().unwrap_or_else(|| {
                                                l("Not mounted", "Ikke montert").into()
                                            })
                                        )),
                                )
                                .child(
                                    div()
                                        .w(px(90.))
                                        .flex_shrink_0()
                                        .text_right()
                                        .text_xs()
                                        .text_color(t::ink_dim(dark))
                                        .child(bytes(volume.used)),
                                )
                        }));
                }
                None => {
                    for volume in &group.volumes {
                        volume_card = volume_card
                            .child(div().flex().justify_between().gap_3().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(volume.name.clone())).child(sub(volume.id.clone()).text_color(t::ink_dim(dark))))
                            .child(sub(volume.mount.clone().unwrap_or_else(|| l("Not mounted", "Ikke montert").into())).text_color(t::ink_dim(dark)))
                            .child(div().grid().grid_cols(3).gap_3().children([
                                (l("Volume capacity", "Volumkapasitet"), bytes(volume.total)),
                                (l("Used", "Brukt"), bytes(volume.used)),
                                (l("Free", "Ledig"), bytes(volume.free)),
                            ].into_iter().map(|(label, value)| div().flex().flex_col().gap_1().child(caption(label, dark)).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(value)))));
                    }
                }
            }
            volume_card
        }))
        .child(section_header(l("Device activity", "Enhetsaktivitet"), "", dark))
        .child(div().grid().grid_cols(2).gap_3()
            .child(flow(l("Device read", "Enhetens lesing"), io.and_then(|v| v.read_rate), io.map(|v| v.read_history.as_slice()).unwrap_or(&[]), io.map(|v| v.read_total)))
            .child(flow(l("Device write", "Enhetens skriving"), io.and_then(|v| v.write_rate), io.map(|v| v.write_history.as_slice()).unwrap_or(&[]), io.map(|v| v.write_total))))
        .child(section_header(l("Health and lifetime", "Helse og levetid"), "", dark))
        .child(div().grid().grid_cols(3).gap_3()
            .child(stat_card(if device.smart.extended { "SMART" } else { l("SMART · from macOS", "SMART · fra macOS") }, health.to_string(), None, dark))
            .child(stat_card(l("Temperature", "Temperatur"), device.smart.temperature_c.map(fmt_celsius).unwrap_or_else(unknown), None, dark))
            .child(stat_card(l("Endurance used", "Brukt skrivelevetid"), device.smart.endurance_used.map(|v| fmt_pct(v as f64)).unwrap_or_else(unknown), None, dark)))
        .child(div().grid().grid_cols(4).gap_3()
            .child(stat_card(l("Lifetime read", "Lest i levetiden"), bytes(device.smart.lifetime_read), None, dark))
            .child(stat_card(l("Lifetime write", "Skrevet i levetiden"), bytes(device.smart.lifetime_write), None, dark))
            .child(stat_card(l("Power-on hours", "Driftstimer"), count(device.smart.power_on_hours), None, dark))
            .child(stat_card(l("Power cycles", "Strømsykluser"), count(device.smart.power_cycles), None, dark)))
}

fn network_tab(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
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
        .gap(t::SECTION_GAP)
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
        .child(top_apps(rows.iter().copied(), |a| { let r = a.net_in_rate + a.net_out_rate; (fmt_rate(r), r) }, max, m.accent(dark), true, dark, this))
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
pub(crate) fn empty_state(icon: &'static str, color: Hsla, headline: String, explanation: String, dark: bool) -> Div {
    div()
        .bg(t::card(dark))
        .rounded(t::RADIUS_CARD)
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
                .rounded(t::RADIUS_CONTROL)
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
                        .text_size(t::TEXT_ROW)
                        .font_weight(FontWeight::MEDIUM)
                        .child(headline),
                )
                .child(sub(explanation).text_color(t::ink_dim(dark))),
        )
}

fn gpu_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Gpu;
    let supported = s.gpu.supported;
    let mut page = div().flex().flex_col().gap(t::SECTION_GAP);
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
            })
            .when_some(s.gpu.power_w, |d, w| {
                d.child(stat_card(l("GPU power · measured", "GPU-effekt · målt"), crate::engine::fmt_watts(w), None, dark))
            })
;
    } else {
        page = page
            .child(hero(
                "GPU",
                s.gpu.utilization.map(fmt_pct).unwrap_or_else(|| "—".into()),
                if supported { "Apple GPU".into() } else { l("Not available on this Mac", "Ikke tilgjengelig på denne Macen").into() },
                &s.gpu_hist,
                m.accent(dark),
                dark,
                vec![
                    (l("Power · measured", "Effekt · målt").into(), s.gpu.power_w.map(crate::engine::fmt_watts).unwrap_or("—".into())),
                    (l("Cores", "Kjerner").into(), s.chip.gpu_cores.map(|c| c.to_string()).unwrap_or("—".into())),
                    (l("Clock · from macOS", "Klokke · fra macOS").into(), s.clocks.gpu.map(crate::engine::fmt_clock).unwrap_or("—".into())),
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
            });
    }
    page
}

fn battery_tab(s: &Snapshot, dark: bool) -> Div {
    use crate::locale::text as l;
    let m = Metric::Battery;
    let preview = crate::battery::preview();
    let b = preview.as_ref().unwrap_or(&s.battery);
    let empty_history = Hist::default();
    let status = b.status();
    let main = if b.present {
        b.percent.map(fmt_pct).unwrap_or("—".into())
    } else {
        "—".into()
    };
    div()
        .flex()
        .flex_col()
        .gap(t::SECTION_GAP)
        .when(preview.is_some(), |d| d.child(card(dark).child(sub(l("DEVELOPMENT PREVIEW · Sample battery values, not hardware readings", "UTVIKLINGSVISNING · Eksempelverdier, ikke batterimålinger")).text_color(t::ink_dim(dark)))))
        .when(b.present, |d| d.child(hero(
            l("Battery", "Batteri"),
            main,
            status.into(),
            if preview.is_some() { &empty_history } else { &s.batt_hist },
            m.accent(dark),
            dark,
            vec![
                (l("Capacity health", "Kapasitetshelse").into(), b.health.map(fmt_pct).unwrap_or("—".into())),
                (l("Cycles", "Sykluser").into(), b.cycles.map(|c| c.to_string()).unwrap_or("—".into())),
                (l("Temperature", "Temperatur").into(), b.temperature_c.map(fmt_celsius).unwrap_or("—".into())),
                (if b.is_charging { l("Until full · estimated", "Til fulladet · anslått") } else { l("Time left · estimated", "Tid igjen · anslått") }.into(),
                    (if b.is_charging { b.minutes_to_full } else { b.minutes_remaining }).map(fmt_hours_minutes).unwrap_or("—".into())),
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
        .when(b.present, |d| d
            .child(section_header(l("Apps · highest usage first", "Apper · høyest bruk først"), l("CPU power from macOS", "CPU-effekt fra macOS"), dark))
            // Below a milliwatt reads as zero, so those apps are left out.
            .child(if s.apps.iter().any(|a| a.power_w >= 0.001) {
                list_card(
                    {
                        let mut rows: Vec<&AppGroup> = s.apps.iter().filter(|a| a.power_w >= 0.001).collect();
                        rows.sort_by(|a, b| b.power_w.partial_cmp(&a.power_w).unwrap());
                        let max = rows.first().map(|a| a.power_w).unwrap_or(1.0).max(0.01);
                        rows.iter()
                            .take(10)
                            .map(|a| app_row(a, breakdown_watts(a.power_w), a.power_w / max, m.accent(dark), dark))
                            .collect()
                    },
                    dark,
                )
            } else {
                empty_state(
                    "icons/battery-charging.svg",
                    m.accent(dark),
                    l("No per-app energy estimates", "Ingen energianslag per app").into(),
                    l("No per-app energy estimates are currently available.", "Ingen energianslag per app er tilgjengelige nå.").into(),
                    dark,
                )
            }))
        .when(b.present, |d| d
            .child(section_header(l("Battery health", "Batterihelse"), "", dark))
            .child(div().grid().grid_cols(3).gap_3()
                .child(stat_card(l("Full-charge capacity", "Fulladekapasitet"), b.full_capacity_mah.map(|c| format!("{} mAh", c)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Design capacity", "Designkapasitet"), b.design_capacity_mah.map(|c| format!("{} mAh", c)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("macOS condition", "Tilstand fra macOS"), b.condition_label().into(), None, dark)))
            .when(preview.is_none(), |d| d.child(battery_trend(dark)))
            .child(section_header(l("Electrical details", "Elektriske detaljer"), "", dark))
            .child(div().grid().grid_cols(5).gap_3()
                .child(stat_card(l("System power", "Systemeffekt"), b.system_power_w.map(crate::engine::fmt_watts).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Battery flow", "Batteristrøm"), b.power_w.map(fmt_signed_watts).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Voltage", "Spenning"), b.voltage_v.map(fmt_volts).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Current", "Strømstyrke"), b.current_ma.map(|a| format!("{:+} mA", a)).unwrap_or("—".into()), None, dark))
                .child(stat_card(l("Adapter rating", "Adapterkapasitet"), b.adapter_rating_w.map(|w| format!("{} W", w)).unwrap_or("—".into()), None, dark))))
}

/// Capacity health per day, kept by History beyond its 30 days, so wear
/// shows over months. Days Tidegauge did not run are gaps.
fn battery_trend(dark: bool) -> Div {
    use crate::locale::text as l;
    let days = crate::history_browser::battery_days();
    let accent = Metric::Battery.accent(dark);
    let (Some(first), Some(last)) = (days.first(), days.last()) else {
        return card(dark).child(sub(l("Tidegauge keeps the battery's capacity health once a day while it runs; the trend shows here from the second day.", "Tidegauge tar vare på batteriets kapasitetshelse én gang om dagen mens det kjører; utviklingen vises her fra andre dag.")).text_color(t::ink_dim(dark)));
    };
    if days.len() < 2 {
        return card(dark).child(sub(format!("{} {} · {}", l("Recorded since", "Registrert siden"), first.day, l("the trend shows from the second day.", "utviklingen vises fra andre dag."))).text_color(t::ink_dim(dark)));
    }
    // Wear is a few percent a year, invisible on a scale from zero, so the
    // chart shows a labelled band from a round floor below the lowest day.
    let low = days.iter().map(|d| d.health).fold(f64::MAX, f64::min);
    let peak = days.iter().map(|d| d.health).fold(100., f64::max);
    let floor = health_floor(low);
    let series: Vec<Option<f64>> = crate::history_browser::battery_series(&days)
        .into_iter()
        .map(|v| v.map(|v| v - floor))
        .collect();
    let cycles = match (first.cycles, last.cycles) {
        (Some(a), Some(b)) => format!(" · {} {a} → {b}", l("cycle count", "syklustall")),
        _ => String::new(),
    };
    card(dark)
        .child(
            div()
                .flex()
                .justify_between()
                .child(caption(l("Capacity health · per day", "Kapasitetshelse · per dag"), dark))
                .child(sub(format!("{} → {}{cycles}", fmt_pct(first.health), fmt_pct(last.health)))),
        )
        .child(crate::controls::history_chart(&series, peak - floor, accent, 80.))
        .child(
            div()
                .flex()
                .justify_between()
                .text_xs()
                .text_color(t::ink_dim(dark))
                .child(first.day.clone())
                .child(format!("{} {}–{}", l("Scale", "Skala"), fmt_pct(floor), fmt_pct(peak)))
                .child(last.day.clone()),
        )
}

/// The bottom of the battery chart: ten points or more under the lowest
/// day, on a multiple of ten, never below zero.
fn health_floor(low: f64) -> f64 {
    (((low - 5.) / 10.).floor() * 10.).clamp(0., 90.)
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

fn sound_tab(
    s: &Snapshot,
    volume_slider: &Entity<gpui_kit::component::slider::SliderState>,
    input_slider: &Entity<gpui_kit::component::slider::SliderState>,
    dark: bool,
    this: Entity<TideGaugeView>,
    pending: bool,
) -> Div {
    let m = Metric::Sound;
    let vol = s.audio.device_volume;
    let accent = m.accent(dark);
    let mut rows = Vec::new();
    let active: Vec<_> = s
        .audio
        .apps
        .iter()
        .filter(|a| a.running_output || a.running_input)
        .collect();
    let idle = s.audio.apps.len() - active.len();
    for a in active.iter() {
        let status = if a.running_output && a.running_input {
            l("Output + input active", "Utgang + inngang aktiv")
        } else if a.running_output {
            l("Output active", "Utgang aktiv")
        } else {
            l("Input active", "Inngang aktiv")
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
                .py(t::SPACE_1)
                .px_4()
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
                )
                .when(a.running_output || s.audio.levels.contains_key(&a.object), |d| {
                    d.child(crate::controls::app_volume_controls(a, s.audio.levels.get(&a.object).copied(), dark, this.clone()))
                }),
        );
    }
    let idle_line = (idle > 0).then(|| {
        sub(crate::locale::count(idle as u64, "app connected to audio, silent", "apps connected to audio, silent", "app koblet til lyd, stille", "apper koblet til lyd, stille"))
            .text_color(t::ink_dim(dark))
    });
    // The volume shows once: in the stepper when it can be changed, otherwise
    // as plain text beside the device name.
    let stepper_shows_volume = !pending && s.audio.can_volume && vol.is_some();
    let volume_text: Option<String> = if s.audio.device_muted == Some(true) {
        Some(l("Muted", "Dempet").into())
    } else if stepper_shows_volume {
        None
    } else {
        Some(vol.map(|v| fmt_pct(v as f64 * 100.)).unwrap_or_else(|| l("Unavailable", "Utilgjengelig").into()))
    };
    div()
        .flex()
        .flex_col()
        .gap(t::SECTION_GAP)
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
                        .when_some(volume_text, |d, text| {
                            d.child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(text))
                        }),
                )
                .child(crate::controls::audio_output(&s.audio, volume_slider, dark, this.clone(), pending)),
        )
        .child(crate::controls::audio_input(&s.audio, input_slider, dark, this, pending))
        .child(section_header(
            l("Apps using audio", "Apper som bruker lyd"),
            l(
                "Activity · not measured level",
                "Aktivitet · ikke målt lydnivå",
            ),
            dark,
        ))
        .child(if active.is_empty() {
            empty_state(
                "icons/volume-2.svg",
                accent,
                l("No apps are using audio", "Ingen apper bruker lyd").into(),
                if s.audio.apps.is_empty() {
                    l("No app audio activity is reported. Process activity may be unavailable on this Mac.", "Ingen lydaktivitet fra apper er rapportert. Prosessaktivitet kan være utilgjengelig på denne Macen.").to_string()
                } else {
                    l("Apps that play or record sound are listed here while they do.", "Apper som spiller av eller tar opp lyd vises her mens de gjør det.").to_string()
                },
                dark,
            )
        } else {
            list_card(rows, dark)
        })
        .children(idle_line)
}

// ---------- bluetooth ----------

fn bt_tab(s: &Snapshot, dark: bool) -> Div {
    let m = Metric::Bluetooth;
    let n = s.bt.devices.len();
    if n == 0 {
        return empty_state(
            "icons/bluetooth.svg",
            m.accent(dark),
            l("No connected devices reported", "Ingen tilkoblede enheter rapportert").into(),
            if s.bt.enabled {
                l("Connected devices and supported battery readings appear here. Pair devices in macOS Bluetooth settings.", "Tilkoblede enheter og støttede batterimålinger vises her. Koble sammen enheter i Bluetooth-innstillingene i macOS.")
            } else {
                l("Bluetooth is off or unavailable. Check Bluetooth in macOS System Settings.", "Bluetooth er av eller utilgjengelig. Sjekk Bluetooth i Systeminnstillinger i macOS.")
            }
            .into(),
            dark,
        );
    }
    let lowest = s
        .bt
        .devices
        .iter()
        .filter_map(|d| d.battery.map(|b| (d.name.clone(), b)))
        .min_by(|a: &(String, f64), b: &(String, f64)| a.1.partial_cmp(&b.1).unwrap());
    let mut c = div().flex().flex_col().gap(t::SECTION_GAP).child(
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
                        .when_some(lowest, |d, (name, _)| {
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
                    .map(|d| {
                        let mut kind_line = d.kind.clone();
                        if d.second_battery.is_some() || d.case_battery.is_some() {
                            for (part, value) in [
                                (l("Left", "Venstre"), d.battery),
                                (l("Right", "Høyre"), d.second_battery),
                                (l("Case", "Etui"), d.case_battery),
                            ] {
                                if let Some(b) = value {
                                    kind_line.push_str(&format!(" · {} {}", part, fmt_pct(b)));
                                }
                            }
                        }
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .py(t::SPACE_1)
                            .px_4()
                            .child(
                                div()
                                    .size(px(24.))
                                    .flex_shrink_0()
                                    .rounded(t::RADIUS_CHIP)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(m.accent(dark).opacity(0.16))
                                    .child(icon_img(crate::bluetooth::device_icon(&d.name), 14., m.accent(dark))),
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
                            // Many devices don't report a battery; an empty
                            // meter would read as a flat battery.
                            .when_some(d.battery, |row, battery| {
                                row.child(level_meter((battery / 100.0) as f32, m.accent(dark), dark, 8))
                                    .child(div().w(px(70.)).text_right().text_sm().child(fmt_pct(battery)))
                            })
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
    crate::controls::button_accent(
        id,
        label,
        false,
        dark,
        Metric::Projects.accent(dark),
        move |window, cx| request_stop(targets.clone(), this.clone(), window, cx),
    )
}

pub(crate) fn projects_tab(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let m = Metric::Projects;
    let accent = m.accent(dark);
    let total = s
        .projects
        .iter()
        .flat_map(|g| g.servers.iter().map(|server| server.pid))
        .collect::<std::collections::HashSet<_>>()
        .len();
    let projects = s.projects.len();
    let all_idle = s.projects.iter().all(|g| g.idle) && total > 0;
    let mut c = div().flex().flex_col().gap_4();
    if total == 0 {
        return c.child(empty_state(
            "icons/folder-code.svg",
            accent,
            l("No development servers detected", "Ingen utviklingsservere funnet").into(),
            l(
                "Local TCP listeners owned by you appear here when their working directory sits near a supported project marker.",
                "Lokale TCP-lyttere som du eier vises her når arbeidsmappen ligger nær en støttet prosjektmarkør.",
            )
            .into(),
            dark,
        ));
    }
    c = c
        .child(
            card(dark).child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon_img("icons/folder-code.svg", 20.0, accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!(
                                        "{} · {}",
                                        crate::locale::count(
                                            projects as u64,
                                            "project",
                                            "projects",
                                            "prosjekt",
                                            "prosjekter"
                                        ),
                                        crate::locale::count(
                                            total as u64,
                                            "server",
                                            "servers",
                                            "server",
                                            "servere"
                                        ),
                                    )),
                            )
                            .child(
                                sub(if all_idle {
                                    l(
                                        "All detected servers have low CPU activity.",
                                        "Alle oppdagede servere har lav CPU-aktivitet.",
                                    )
                                } else {
                                    l(
                                        "Stop asks for confirmation before sending SIGTERM to the project's own processes.",
                                        "Stopp ber om bekreftelse før SIGTERM sendes til prosjektets egne prosesser.",
                                    )
                                })
                                .text_color(t::ink_dim(dark)),
                            ),
                    )
                    .child(stop_button(
                        "stop-all".into(),
                        l("Stop all…", "Stopp alle…"),
                        s.projects.iter().flat_map(project_targets).collect(),
                        dark,
                        this.clone(),
                    )),
            ),
        )
        .child(section_header(
            l("Projects", "Prosjekter"),
            &crate::locale::count(total as u64, "server", "servers", "server", "servere"),
            dark,
        ));
    for g in &s.projects {
        let server_count = g
            .servers
            .iter()
            .map(|server| server.pid)
            .collect::<std::collections::HashSet<_>>()
            .len();
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
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .whitespace_nowrap()
                                        .child(g.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(t::ink_dim(dark))
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .child(g.dir.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(t::ink_dim(dark))
                                .child(crate::locale::count(
                                    server_count as u64,
                                    "server",
                                    "servers",
                                    "server",
                                    "servere",
                                )),
                        ),
                )
                .children(g.servers.iter().map(|srv| {
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .py(t::SPACE_1)
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
                                .px(t::SPACE_2)
                                .py(px(1.))
                                .rounded_sm()
                                .bg(t::card_hi(dark))
                                .text_xs()
                                .child(srv.runtime.clone()),
                        )
                        .child(
                            div()
                                .px(t::SPACE_2)
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
                                .text_color(if srv.state.is_idle() {
                                    t::ink_dim(dark)
                                } else {
                                    t::ok(dark)
                                })
                                .child(srv.state.label()),
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
    c
}

#[cfg(test)]
mod tests {
    #[test]
    fn battery_chart_floor_sits_a_round_step_below_the_lowest_day() {
        assert_eq!(super::health_floor(96.), 90.);
        assert_eq!(super::health_floor(100.), 90.);
        assert_eq!(super::health_floor(81.), 70.);
        assert_eq!(super::health_floor(3.), 0.);
    }

    use super::{
        ChartScale, MAX_DISPLAYED_TEMPERATURE_CHANNELS, bound_temperature_display,
        temperature_display_caption, temperature_summary,
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
        let display = bound_temperature_display(&values, true);
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
        let display = bound_temperature_display(&values, true);
        let keys: Vec<&str> = display.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["Tc00", "Td00", "Ta00", "Tb00", "Te00"]);
    }

    #[test]
    fn temperature_display_caption_is_exact_in_both_languages() {
        let values = channel_pool(342, 22);
        let display = bound_temperature_display(&values, true);
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
    fn the_bottom_row_never_squeezes_four_cards_side_by_side() {
        assert_eq!(super::bottom_cols(2), 2);
        assert_eq!(super::bottom_cols(3), 3);
        assert_eq!(super::bottom_cols(4), 2);
        assert_eq!(super::bottom_cols(1), 2);
    }

    #[test]
    fn temperature_summary_uses_only_fresh_reported_channels() {
        let values = vec![reading("Ta00", Some(70.0)), reading("Tb00", Some(80.0)), reading("Tc00", None)];
        assert_eq!(temperature_summary(&values, true), Some((80.0, 75.0)));
        assert_eq!(temperature_summary(&values, false), None, "stale readings are not summarised");
        assert_eq!(temperature_summary(&[reading("Ta00", None)], true), None);
    }

    #[test]
    fn temperature_display_caption_omits_zero_unavailable() {
        let values = channel_pool(30, 0);
        let display = bound_temperature_display(&values, true);
        assert_eq!(
            temperature_display_caption(&display, english).as_deref(),
            Some("Showing 24 of 30 channels")
        );
    }

    #[test]
    fn temperature_display_caption_is_absent_when_unbounded_and_complete() {
        let values = vec![reading("TC0P", Some(54.5)), reading("TG0P", Some(48.0))];
        let display = bound_temperature_display(&values, true);
        assert_eq!(display.rows.len(), 2);
        assert!(temperature_display_caption(&display, english).is_none());
    }

    #[test]
    fn stale_temperature_display_counts_every_channel_unavailable() {
        let values = channel_pool(342, 22);
        let display = bound_temperature_display(&values, false);
        assert_eq!(display.rows.len(), MAX_DISPLAYED_TEMPERATURE_CHANNELS);
        assert_eq!(display.total, 342);
        assert_eq!(display.unavailable, 342);
        assert_eq!(
            temperature_display_caption(&display, english).as_deref(),
            Some("Showing 24 of 342 channels · 342 unavailable")
        );
        assert_eq!(
            temperature_display_caption(&display, norwegian).as_deref(),
            Some("Viser 24 av 342 kanaler · 342 utilgjengelige")
        );
        assert_eq!(values.len(), 342);
        assert_eq!(values.iter().filter(|r| r.value.is_none()).count(), 22);

        let two = vec![reading("TC0P", Some(54.5)), reading("TG0P", Some(48.0))];
        let display = bound_temperature_display(&two, false);
        assert_eq!(display.rows.len(), 2);
        assert_eq!(display.total, 2);
        assert_eq!(display.unavailable, 2);
        assert_eq!(
            temperature_display_caption(&display, english).as_deref(),
            Some("Showing 2 of 2 channels · 2 unavailable")
        );
        let display = bound_temperature_display(&two, true);
        assert!(temperature_display_caption(&display, english).is_none());

        let empty: Vec<Reading> = Vec::new();
        let display = bound_temperature_display(&empty, false);
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
