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
    pub show_settings: bool,
    pub settings_error: Option<String>,
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
            show_settings: false,
            settings_error: None,
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

            // Fill each interval with a convex trapezoid. A single concave
            // area path would be triangulated across the chart by GPUI.
            for i in 1..n {
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
                    .aria_label("Metrics")
                    .tab_group()
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
                        let accent = tb.metric().unwrap_or(Metric::Cpu).accent(dark);
                        let this = this.clone();
                        let keyboard_target = this.clone();
                        div()
                            .id(SharedString::from(format!("tab-{}", tb.label())))
                            .role(Role::Tab)
                            .aria_label(tb.label())
                            .aria_selected(active)
                            .tab_index(tb as isize)
                            .focus_visible(|d| d.border_color(accent).bg(accent.opacity(0.12)))
                            .on_key_down(move |event, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    select_tab(&keyboard_target, tb, cx);
                                    cx.stop_propagation();
                                }
                            })
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .px(px(10.))
                            .py(px(8.))
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .rounded_full()
                            .cursor_pointer()
                            .when(active, |d| {
                                d.bg(accent.opacity(0.12)).text_color(accent)
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
                                select_tab(&this, tb, cx);
                            })
                    })),
            );

        let topbar = if self.show_settings {
            let back = this.clone();
            let keyboard_back = this.clone();
            div().h(px(64.)).flex_shrink_0().pl(px(104.)).pr(px(24.))
                .flex().items_center().gap_3()
                .child(div().id("back-to-overview").role(Role::Button).aria_label("Back to Overview")
                    .tab_index(0).rounded_md().p_2().cursor_pointer()
                    .hover(|d| d.bg(t::card_hi(dark))).focus_visible(|d| d.bg(t::card_hi(dark)))
                    .child(icon_img("icons/arrow-left.svg", 16., t::ink(dark)))
                    .on_key_down(move |event, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            select_tab(&keyboard_back, Tab::Overview, cx);
                            cx.stop_propagation();
                        }
                    })
                    .on_click(move |_, _, cx| select_tab(&back, Tab::Overview, cx)))
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Settings"))
                .into_any_element()
        } else { topbar.into_any_element() };

        let footer = div()
            .w_full()
            .px_3()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .child(div().flex().items_center().gap_2()
                .child({
                    let click_target = this.clone();
                    let key_target = this.clone();
                    div().id("dashboard-settings").role(Role::Button).aria_label("Settings")
                        .tab_index(11).flex().items_center().gap_1().px_2().py_1().rounded_md()
                        .cursor_pointer().text_xs().text_color(t::ink_dim(dark))
                        .hover(|d| d.bg(t::card_hi(dark)).text_color(t::ink(dark)))
                        .focus_visible(|d| d.bg(t::card_hi(dark)).text_color(t::ink(dark)))
                        .child(icon_img("icons/settings.svg", 13., t::ink_dim(dark)))
                        .child("Settings")
                        .on_click(move |_, _, cx| open_settings(&click_target, cx))
                        .on_key_down(move |event, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                open_settings(&key_target, cx);
                                cx.stop_propagation();
                            }
                        })
                })
            )
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
            .px(px(20.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_tab(dark, this.clone()));

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
                    if event.keystroke.modifiers.shift { window.focus_prev(cx); }
                    else { window.focus_next(cx); }
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
        view.show_settings = true;
        view.scroll = ScrollHandle::new();
        view.settings_error = None;
        cx.notify();
    });
}

fn select_tab(this: &Entity<TideGaugeView>, tab: Tab, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.tab = tab;
        v.show_settings = false;
        v.settings_error = None;
        v.scroll = ScrollHandle::new();
        cx.notify();
    });
}

impl TideGaugeView {
    fn render_tab(&self, dark: bool, this: Entity<TideGaugeView>) -> AnyElement {
        if self.show_settings {
            return settings_panel(dark, this, self.settings_error.as_deref()).into_any_element();
        }
        let s = &self.snap;
        match self.tab {
            Tab::Overview => overview(s, dark, this).into_any_element(),
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

fn update_preference(
    this: &Entity<TideGaugeView>, change: impl FnOnce(&mut crate::settings::Settings), cx: &mut App,
) {
    let result = crate::settings::mutate(change);
    this.update(cx, |view, cx| {
        view.settings_error = result.err().map(|error| format!("Could not save settings: {error}"));
        cx.notify();
    });
}

fn settings_choice(
    id: String, label: &str, description: String, active: bool, index: isize,
    dark: bool, this: Entity<TideGaugeView>, change: impl Fn(&mut crate::settings::Settings) + 'static,
) -> impl IntoElement {
    let change = std::rc::Rc::new(change);
    let keyboard_change = change.clone();
    let keyboard_target = this.clone();
    let accent = t::accent(dark);
    div().id(SharedString::from(id)).role(Role::RadioButton)
        .aria_label(SharedString::from(description))
        .aria_toggled(if active { Toggled::True } else { Toggled::False })
        .tab_index(index).rounded(px(8.)).px_3().py_2().border_1()
        .border_color(if active { accent } else { t::line(dark) })
        .bg(if active { accent.opacity(0.12) } else { t::card_hi(dark) })
        .text_color(if active { accent } else { t::ink_dim(dark) })
        .text_size(px(12.)).cursor_pointer()
        .hover(|d| d.text_color(t::ink(dark)))
        .focus_visible(|d| d.border_color(accent))
        .child(label.to_string())
        .on_key_down(move |event, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                update_preference(&keyboard_target, |settings| keyboard_change(settings), cx);
                cx.stop_propagation();
            }
        })
        .on_click(move |_, _, cx| update_preference(&this, |settings| change(settings), cx))
}

fn settings_panel(dark: bool, this: Entity<TideGaugeView>, error: Option<&str>) -> Div {
    use crate::settings::{Settings, MENU_METRICS, MENU_STYLES};
    let settings = Settings::load();
    let mut panel = div().flex().flex_col().gap_4()
        .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).child("Menu bar"))
        .child(div().text_sm().text_color(t::ink_dim(dark))
            .child("One compact menu-bar item. Add readings below, or keep just the Tidegauge icon."));
    if let Some(error) = error {
        panel = panel.child(div().text_sm().text_color(t::warn(dark)).child(error.to_string()));
    }
    panel = panel.child(card(dark).flex_row().items_center().justify_between()
        .child(div().flex_1()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Tidegauge icon"))
            .child(div().text_xs().text_color(t::ink_dim(dark)).child("Open the app menu with one click.")))
        .child(div().id("launcher-options").role(Role::RadioGroup).aria_label("Tidegauge icon")
            .tab_group().flex().gap_2()
            .children([(true, "Shown"), (false, "Hidden")].into_iter().enumerate().map(|(i, (shown, label))| {
                settings_choice(format!("launcher-{shown}"), label, format!("Tidegauge icon: {label}"),
                    settings.show_app_icon == shown, i as isize, dark, this.clone(),
                    move |settings| settings.show_app_icon = shown)
            }))));
    let rows = MENU_METRICS.iter().enumerate().map(|(row, &(key, name))| {
        let selected = settings.menu_items.iter().find(|item| item.metric == key)
            .map(|item| item.style.as_str()).unwrap_or("off");
        let choices = std::iter::once(("off", "Off")).chain(MENU_STYLES);
        div().id(SharedString::from(format!("settings-{key}")))
            .role(Role::RadioGroup).aria_label(name).tab_group()
            .flex().items_center().justify_between().gap_4().py_2()
            .when(row > 0, |d| d.border_t_1().border_color(t::line(dark)))
            .child(div().w(px(140.)).text_sm().font_weight(FontWeight::MEDIUM).child(name))
            .child(div().flex().gap_2().children(choices.enumerate().map(|(i, (style, label))| {
                settings_choice(format!("{key}-{style}"), label, format!("{name}: {label}"),
                    selected == style, i as isize, dark, this.clone(),
                    move |settings| settings.set_menu_style(key, if style == "off" { None } else { Some(style) }))
            })))
    }).collect::<Vec<_>>();
    panel.child(card(dark).gap_0().py_1().children(rows))
        .child(div().text_xs().text_color(t::ink_dim(dark))
            .child("Click the menu-bar item for Open Dashboard, Settings, or Quit. Changes are saved immediately."))
        .child(div().flex().justify_end().gap_3()
            .child({
                let click_target = this.clone();
                let key_target = this.clone();
                div().id("settings-icon-only").role(Role::Button).aria_label("Reset to icon only")
                    .tab_index(29).rounded_md().px_4().py_2().border_1().border_color(t::line(dark))
                    .text_sm().cursor_pointer().focus_visible(|d| d.border_color(t::accent(dark)))
                    .child("Reset to icon only")
                    .on_key_down(move |event, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            update_preference(&key_target, |settings| { settings.show_app_icon = true; settings.menu_items.clear(); }, cx);
                            cx.stop_propagation();
                        }
                    })
                    .on_click(move |_, _, cx| update_preference(&click_target, |settings| { settings.show_app_icon = true; settings.menu_items.clear(); }, cx))
            })
            .child(div().id("settings-app-menu").role(Role::Button).aria_label("App menu")
                .tab_index(30).rounded_md().px_4().py_2().border_1().border_color(t::line(dark))
                .text_sm().cursor_pointer().focus_visible(|d| d.border_color(t::accent(dark)))
                .child("App menu")
                .on_key_down(|event, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        crate::statusbar::SHOW_CONTEXT_MENU.store(true, std::sync::atomic::Ordering::SeqCst);
                        cx.stop_propagation();
                    }
                })
                .on_click(|_, _, _| crate::statusbar::SHOW_CONTEXT_MENU.store(true, std::sync::atomic::Ordering::SeqCst)))
            .child(div().id("settings-quit").role(Role::Button).aria_label("Quit Tidegauge")
                .tab_index(31).rounded_md().px_4().py_2().bg(t::card_hi(dark))
                .text_sm().cursor_pointer().focus_visible(|d| d.border_color(t::accent(dark)))
                .child("Quit Tidegauge")
                .on_key_down(|event, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") { cx.quit(); }
                })
                .on_click(|_, _, cx| cx.quit())))
}

/// Compact bars use the latest 48 real samples, aligned to the right while
/// the history fills. Empty slots are left blank rather than inventing activity.
fn bar_chart(pts: &[f64], color: Hsla, max_hint: Option<f64>) -> Canvas<()> {
    let pts = pts.to_vec();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let w: f32 = bounds.size.width.into();
            let h: f32 = bounds.size.height.into();
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let n = pts.len().min(48);
            let tail = &pts[pts.len() - n..];
            let data_max = tail.iter().copied().fold(0.0f64, f64::max);
            let max = max_hint.unwrap_or(data_max).max(data_max).max(1.0);
            let slot = w / 48.0;
            for (i, value) in tail.iter().enumerate() {
                let height = ((*value / max).clamp(0.0, 1.0) as f32 * h).max(1.0);
                let x = ox + (48 - n + i) as f32 * slot;
                let width = (slot - 2.0).max(1.0);
                let mut bar = Path::new(point(px(x), px(oy + h)));
                bar.line_to(point(px(x), px(oy + h - height)));
                bar.line_to(point(px(x + width), px(oy + h - height)));
                bar.line_to(point(px(x + width), px(oy + h)));
                bar.line_to(point(px(x), px(oy + h)));
                window.paint_path(bar, color);
            }
        },
    ).w_full().h_full()
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
    tab: Tab, context: String, value: String,
    stats: [(&str, String); 3], hist: &Hist, max: Option<f64>,
    dark: bool, this: Entity<TideGaugeView>,
) -> impl IntoElement {
    let color = tab.metric().unwrap().accent(dark);
    let (number, unit) = value.rsplit_once(' ').unwrap_or((&value, ""));
    let keyboard_target = this.clone();
    div().id(SharedString::from(format!("overview-{}", tab.label())))
        .role(Role::Button)
        .aria_label(SharedString::from(format!("{}: {}. Open details", tab.label(), value)))
        .tab_index(20 + tab as isize)
        .border_1().border_color(gpui_kit::transparent_black())
        .focus_visible(|d| d.border_color(color))
        .on_key_down(move |event, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                select_tab(&keyboard_target, tab, cx);
                cx.stop_propagation();
            }
        })
        .min_w_0().h(px(222.)).p(px(18.)).rounded(px(12.))
        .bg(t::card(dark)).flex().flex_col().justify_between()
        .cursor_pointer().hover(|d| d.bg(t::card_hi(dark)))
        .child(div().flex().items_center().justify_between()
            .child(metric_heading(tab, dark))
            .child(icon_img("icons/chevron-right.svg", 12., t::ink_dim(dark))))
        .child(div()
            .child(div().text_size(px(11.)).line_height(px(16.))
                .text_color(t::ink_dim(dark)).child(context))
            .child(div().flex().items_baseline().gap(px(6.))
                .child(div().text_size(px(34.)).line_height(px(42.))
                    .font_weight(FontWeight::BOLD).child(number.to_string()))
                .child(div().text_size(px(15.)).font_weight(FontWeight::MEDIUM)
                    .text_color(t::ink_dim(dark)).child(unit.to_string()))))
        .child(div().grid().grid_cols(3).gap_2().children(stats.into_iter().map(|(label, value)| {
            div().min_w_0()
                .child(div().text_size(px(10.)).line_height(px(16.))
                    .text_color(t::ink_dim(dark)).child(label.to_string()))
                .child(div().text_size(px(12.)).line_height(px(17.))
                    .font_weight(FontWeight::MEDIUM).child(value))
        })))
        .child(div().h(px(38.)).rounded(px(8.)).px(px(6.)).py(px(5.))
            .bg(color.opacity(0.09)).child(bar_chart(&hist.pts, color, max)))
        .on_click(move |_, _, cx| {
            select_tab(&this, tab, cx);
        })
}

fn history_average(hist: &Hist) -> String {
    if hist.pts.is_empty() { return "—".into(); }
    fmt_pct(hist.pts.iter().sum::<f64>() / hist.pts.len() as f64)
}

fn overview(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let battery_context = if !s.battery.present { "No battery" }
        else if s.battery.is_charging { "Charging" }
        else if s.battery.on_battery { "On battery" } else { "On AC power" };
    let remaining = s.battery.minutes_remaining.filter(|m| s.battery.on_battery && *m >= 0)
        .map(|m| format!("{}h {}m", m / 60, m % 60)).unwrap_or("—".into());
    let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.0;
    div().flex().flex_col().gap(px(16.))
        .child(div().grid().grid_cols(3).gap(px(16.))
            .child(overview_card(Tab::Cpu, "Now".into(), format!("{:.0} %", s.cpu_pct), [
                ("Load · 1 min", format!("{:.2}", s.load_avg[0])),
                ("Cores", s.chip.cores.to_string()),
                ("Average", history_average(&s.cpu_hist)),
            ], &s.cpu_hist, Some(100.), dark, this.clone()))
            .child(overview_card(Tab::Memory, format!("In use of {}", fmt_bytes(s.mem.total as f64)),
                fmt_bytes(s.mem.used as f64), [
                    ("App", fmt_bytes(s.mem.app as f64)),
                    ("Wired", fmt_bytes(s.mem.wired as f64)),
                    ("Compressed", fmt_bytes(s.mem.compressed as f64)),
                ], &s.mem_hist, Some(100.), dark, this.clone()))
            .child(overview_card(Tab::Gpu, if s.gpu.utilization.is_some() { "GPU utilization" } else { "Utilization unavailable" }.into(),
                s.gpu.utilization.map(|v| format!("{:.0} %", v)).unwrap_or("—".into()), [
                    ("Memory", s.gpu.mem_in_use.map(|v| fmt_bytes(v as f64)).unwrap_or("—".into())),
                    ("Average", if s.gpu.utilization.is_some() { history_average(&s.gpu_hist) } else { "—".into() }),
                    ("Peak", if s.gpu.utilization.is_some() { fmt_pct(s.gpu_hist.pts.iter().copied().fold(0., f64::max)) } else { "—".into() }),
                ], &s.gpu_hist, Some(100.), dark, this.clone())))
        .child(div().grid().grid_cols(3).gap(px(16.))
            .child(overview_card(Tab::Disk, format!("Free of {}", fmt_bytes(s.disk.total as f64)),
                fmt_bytes(s.disk.free as f64), [
                    ("Reading", fmt_rate(s.disk.read_rate)),
                    ("Writing", fmt_rate(s.disk.write_rate)),
                    ("Used", fmt_bytes(s.disk.used as f64)),
                ], &s.disk_w_hist, None, dark, this.clone()))
            .child(overview_card(Tab::Network, "Downloading".into(), fmt_rate(s.net.rate_in), [
                ("Uploading", fmt_rate(s.net.rate_out)),
                ("Received¹", fmt_bytes(s.net.total_in as f64)),
                ("Sent¹", fmt_bytes(s.net.total_out as f64)),
            ], &s.net_in_hist, None, dark, this.clone()))
            .child(overview_card(Tab::Battery, battery_context.into(),
                s.battery.percent.map(|v| format!("{:.0} %", v)).unwrap_or("—".into()), [
                    ("Remaining", remaining),
                    ("Power draw", s.battery.power_w.map(fmt_watts).unwrap_or("—".into())),
                    ("Health", s.battery.health.map(fmt_pct).unwrap_or("—".into())),
                ], &s.batt_hist, Some(100.), dark, this)))
        .child(div().grid().grid_cols(3).gap(px(16.))
            .child(donut_card("Memory by Type", Tab::Memory, vec![
                BreakdownEntry::new("App", s.mem.app as f64, Metric::Cpu.accent(dark)),
                BreakdownEntry::new("Wired", s.mem.wired as f64, Metric::Memory.accent(dark)),
                BreakdownEntry::new("Compressed", s.mem.compressed as f64, Metric::Gpu.accent(dark)),
                BreakdownEntry::new("Cached", s.mem.cached as f64, t::ink_dim(dark)),
                BreakdownEntry::new("Free", s.mem.free as f64, t::card_hi(dark)),
            ], format!("{:.0}%", mem_pct), "in use", fmt_bytes, dark))
            .child(top_apps_donut(s, dark))
            .child(power_donut(s, dark)))
        .child(div().text_size(px(10.)).text_color(t::ink_dim(dark))
            .child("¹ Network totals since boot. Averages and peaks cover this session."))
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
            let ring = if total > 0. { slices.clone() }
                else { vec![(1., t::hex(0x8e8e93))] };
            let total = if total > 0. { total } else { 1. };
            let mut angle = -std::f64::consts::FRAC_PI_2;
            let at = |radius: f32, angle: f64| point(
                px(cx + radius * angle.cos() as f32),
                px(cy + radius * angle.sin() as f32),
            );
            for (value, color) in &ring {
                let sweep = (*value / total) * std::f64::consts::TAU;
                if sweep <= 0. { continue; }
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
    ).size(px(size))
}

struct BreakdownEntry {
    name: String,
    value: f64,
    color: Hsla,
    app: Option<AppGroup>,
}

impl BreakdownEntry {
    fn new(name: &str, value: f64, color: Hsla) -> Self {
        Self { name: name.into(), value, color, app: None }
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
    div().min_w_0().h(px(184.)).rounded(px(12.)).bg(t::card(dark)).p(px(18.))
        .flex().flex_col().gap(px(14.))
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
    if other > 0. { slices.push(BreakdownEntry::new("Other", other, color.opacity(0.35))); }
    donut_card("Memory by App", Tab::Memory, slices, fmt_bytes(total), "all apps", fmt_bytes, dark)
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
    if other > 0. { slices.push(BreakdownEntry::new("Other", other, color.opacity(0.35))); }
    donut_card("Power by App", Tab::Battery, slices, breakdown_watts(total), "estimated", breakdown_watts, dark)
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
                ("Session uptime".to_string(), crate::engine::fmt_up(s.uptime.as_secs_f64())),
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
                ("State".into(), if s.neural.is_some() { "Ready".into() } else { "Unavailable".into() }),
            ],
            Some(100.0),
        ))
        .when(s.neural.is_none(), |d| {
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
                                    format!("{} dev {} {} running but idle", total, if total == 1 { "server" } else { "servers" }, if total == 1 { "is" } else { "are" })
                                } else {
                                    format!("{} dev {} running", total, if total == 1 { "server" } else { "servers" })
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
                                .child("Stop All")
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
