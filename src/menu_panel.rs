//! The menu-bar panel: what a left click on the Tidegauge item opens. Four
//! live readings with their recent curve, your AI plans, the busiest apps,
//! and the way into the full window.

use crate::engine::{fmt_bytes, fmt_pct, fmt_rate, Snapshot};
use crate::locale::text as l;
use crate::processes::AppGroup;
use crate::theme::{self as t, Metric};
use crate::ui::{app_icon, Destination, Tab, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub const WIDTH: f32 = 340.;
pub const HEIGHT: f32 = 600.;

pub struct MenuPanel {
    snap: Snapshot,
    main: Entity<TideGaugeView>,
    focus: FocusHandle,
}

impl MenuPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, main: Entity<TideGaugeView>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        cx.spawn(async move |this: WeakEntity<MenuPanel>, cx: &mut AsyncApp| loop {
            smol::Timer::after(std::time::Duration::from_millis(1000)).await;
            let alive = cx
                .update(|cx: &mut App| {
                    this.update(cx, |panel, cx| {
                        panel.snap = crate::engine::latest();
                        cx.notify();
                    })
                    .is_ok()
                });
            if !alive {
                break;
            }
        })
        .detach();
        Self { snap: crate::engine::latest(), main, focus }
    }
}

/// Close the panel and show the full window on `to`.
fn open(main: &Entity<TideGaugeView>, to: Destination, cx: &mut App) {
    crate::app::close_popover(cx);
    crate::ui::navigate(main, to, cx);
    crate::app::toggle_panel(cx);
}

fn tile(
    tab: Tab,
    value: String,
    detail: String,
    points: &[f64],
    max: Option<f64>,
    dark: bool,
    main: Entity<TideGaugeView>,
) -> impl IntoElement {
    let metric = tab.metric().unwrap_or(Metric::Cpu);
    let color = metric.accent(dark);
    let keyboard = main.clone();
    div()
        .id(SharedString::from(format!("panel-{}", tab.key())))
        .role(Role::Button)
        .aria_label(SharedString::from(format!("{}: {value}", tab.label())))
        .tab_index(0)
        .min_w_0()
        .p(t::SPACE_3)
        .rounded(t::RADIUS_CONTROL)
        .bg(t::card(dark))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .focus_visible(move |d| d.border_color(color))
        .hover(|d| d.bg(t::card_hi(dark)))
        .cursor_pointer()
        .flex()
        .flex_col()
        .gap(t::SPACE_1)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(t::TEXT_SMALL).font_weight(FontWeight::MEDIUM).text_color(color).child(tab.label()))
                .child(div().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).truncate().child(detail)),
        )
        .child(div().text_size(t::TEXT_TITLE).font_weight(FontWeight::BOLD).child(value))
        .child(div().h(px(22.)).child(crate::ui::area_chart(points, 22., color, max)))
        .on_click(move |_, _, cx| open(&main, Destination::Tab(tab), cx))
        .capture_key_down(move |event, window, cx| {
            if crate::controls::is_activation_key(&event.keystroke) {
                window.prevent_default();
                open(&keyboard, Destination::Tab(tab), cx);
                cx.stop_propagation();
            }
        })
}

fn sum(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().enumerate().map(|(i, v)| v + b.get(i).copied().unwrap_or(0.)).collect()
}

fn footer_button(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    dark: bool,
    action: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    footer_control(id, icon, label, true, dark, action)
}

/// A footer action; `show_label` false keeps only the icon on screen (the
/// label still names it for VoiceOver) where the row has no room.
fn footer_control(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    show_label: bool,
    dark: bool,
    action: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let action = std::rc::Rc::new(action);
    let keyboard = action.clone();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .h(t::CONTROL_H)
        .px(t::SPACE_2)
        .flex()
        .items_center()
        .gap(t::SPACE_1)
        .rounded(t::RADIUS_CONTROL)
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .focus_visible(move |d| d.border_color(t::accent(dark)))
        .hover(|d| d.bg(t::card_hi(dark)))
        .cursor_pointer()
        .text_size(t::TEXT_BODY)
        .child(svg().path(icon).size(px(13.)).text_color(t::ink_dim(dark)))
        .when(show_label, |d| d.child(label))
        .on_click(move |_, _, cx| action(cx))
        .capture_key_down(move |event, window, cx| {
            if crate::controls::is_activation_key(&event.keystroke) {
                window.prevent_default();
                keyboard(cx);
                cx.stop_propagation();
            }
        })
}

impl Render for MenuPanel {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let dark = crate::settings::Settings::load().dark.unwrap_or(matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ));
        let s = &self.snap;
        let main = self.main.clone();
        let mem_pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.;
        let mut busiest: Vec<&AppGroup> = s.apps.iter().filter(|a| a.cpu_pct > 0.).collect();
        busiest.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        let now = crate::metrics::now_unix();
        let plans: Vec<crate::agents_panel::PlanGlance> = match &s.plans {
            crate::agents::Plans::Loaded(plans) => plans.iter().take(3).map(|p| crate::agents_panel::plan_glance(p, now, dark)).collect(),
            _ => Vec::new(),
        };
        let caption = |text: &'static str| {
            div()
                .px(t::SPACE_1)
                .text_size(t::TEXT_CAPTION)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t::ink_dim(dark))
                .child(text.to_uppercase())
        };
        div()
            .id("menu-panel")
            .role(Role::Dialog)
            .aria_label("Tidegauge")
            .track_focus(&self.focus)
            .on_key_down(|event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    crate::app::close_popover(cx);
                }
            })
            .size_full()
            .p(t::SPACE_3)
            .flex()
            .flex_col()
            .gap(t::SPACE_3)
            .rounded(t::RADIUS_CARD)
            .border_1()
            .border_color(t::line(dark))
            .bg(t::bg(dark))
            .text_color(t::ink(dark))
            .font_family(".AppleSystemUIFont")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(t::SPACE_1)
                    .child(div().text_size(t::TEXT_EMPH).font_weight(FontWeight::SEMIBOLD).child("Tidegauge"))
                    .child(
                        div()
                            .text_size(t::TEXT_SMALL)
                            .text_color(t::ink_dim(dark))
                            .child(crate::locale::count(s.app_count as u64, "app", "apps", "app", "apper")),
                    ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap(t::SPACE_2)
                    .child(tile(Tab::Cpu, fmt_pct(s.cpu_pct), crate::locale::count(s.chip.cores as u64, "core", "cores", "kjerne", "kjerner"), &s.cpu_hist.pts, Some(100.), dark, main.clone()))
                    .child(tile(Tab::Memory, fmt_bytes(s.mem.used as f64), fmt_pct(mem_pct), &s.mem_hist.pts, Some(100.), dark, main.clone()))
                    .child(tile(
                        Tab::Network,
                        fmt_rate(s.net.rate_in + s.net.rate_out),
                        format!("↓ {}", fmt_rate(s.net.rate_in)),
                        &sum(&s.net_in_hist.pts, &s.net_out_hist.pts),
                        None,
                        dark,
                        main.clone(),
                    ))
                    .child(tile(
                        Tab::Disk,
                        fmt_rate(s.disk.read_rate + s.disk.write_rate),
                        format!("{} {}", fmt_bytes(s.disk.free as f64), l("free", "ledig")),
                        &sum(&s.disk_r_hist.pts, &s.disk_w_hist.pts),
                        None,
                        dark,
                        main.clone(),
                    )),
            )
            .when(!plans.is_empty(), |d| {
                d.child(caption(l("AI plans", "AI-abonnementer"))).child(
                    div().rounded(t::RADIUS_CONTROL).bg(t::card(dark)).py(t::SPACE_1).flex().flex_col().children(plans.into_iter().map(|plan| {
                        div()
                            .h(px(28.))
                            .px(t::SPACE_2)
                            .flex()
                            .items_center()
                            .gap(t::SPACE_2)
                            .child(div().size(px(18.)).flex_shrink_0().child(crate::ui::donut(&plan.ring, 18., t::card_hi(dark))))
                            .child(div().flex_1().min_w_0().text_size(t::TEXT_BODY).truncate().child(plan.name))
                            .child(div().text_size(t::TEXT_SMALL).text_color(plan.note_color).child(plan.note))
                            .child(
                                div()
                                    .w(px(44.))
                                    .flex()
                                    .justify_end()
                                    .text_size(t::TEXT_BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(plan.color)
                                    .child(crate::agents_panel::left_text(plan.left)),
                            )
                    })),
                )
            })
            .child(caption(l("Busiest apps", "Mest aktive apper")))
            .child(
                div()
                    .rounded(t::RADIUS_CONTROL)
                    .bg(t::card(dark))
                    .py(t::SPACE_1)
                    .flex()
                    .flex_col()
                    .children(busiest.iter().take(3).enumerate().map(|(i, app)| {
                        let target = main.clone();
                        let keyboard = main.clone();
                        div()
                            .id(SharedString::from(format!("panel-app-{i}")))
                            .role(Role::Button)
                            .aria_label(SharedString::from(format!("{} {}", app.name, fmt_pct(app.cpu_pct))))
                            .tab_index(0)
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .focus_visible(move |d| d.border_color(t::accent(dark)))
                            .capture_key_down(move |event, window, cx| {
                                if crate::controls::is_activation_key(&event.keystroke) {
                                    window.prevent_default();
                                    open(&keyboard, Destination::Apps, cx);
                                    cx.stop_propagation();
                                }
                            })
                            .h(px(28.))
                            .px(t::SPACE_2)
                            .flex()
                            .items_center()
                            .gap(t::SPACE_2)
                            .cursor_pointer()
                            .hover(|d| d.bg(t::card_hi(dark)))
                            .child(app_icon(app, dark))
                            .child(div().flex_1().min_w_0().text_size(t::TEXT_BODY).truncate().child(app.name.clone()))
                            .child(div().text_size(t::TEXT_BODY).text_color(t::ink_dim(dark)).child(fmt_pct(app.cpu_pct)))
                            .on_click(move |_, _, cx| open(&target, Destination::Apps, cx))
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(t::SPACE_1)
                    .child({
                        let main = main.clone();
                        footer_button("panel-open", "icons/layout-dashboard.svg", l("Open Tidegauge", "Åpne Tidegauge"), dark, move |cx| {
                            open(&main, Destination::Tab(Tab::Overview), cx)
                        })
                    })
                    .child(div().flex_1())
                    .when(matches!(crate::updater::availability(), crate::updater::Availability::Available), |d| {
                        d.child(footer_control("panel-updates", "icons/refresh-cw.svg", crate::updater::check_for_updates_title(), false, dark, |cx| {
                            crate::app::close_popover(cx);
                            crate::updater::ask_for_check();
                        }))
                    })
                    .child({
                        let main = main.clone();
                        footer_button("panel-settings", "icons/settings.svg", l("Settings", "Innstillinger"), dark, move |cx| {
                            open(&main, Destination::Settings, cx)
                        })
                    })
                    .child(footer_button("panel-quit", "icons/power.svg", l("Quit", "Avslutt"), dark, |cx| cx.quit())),
            )
    }
}
