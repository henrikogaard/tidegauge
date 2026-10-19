//! The menu-bar panel: what a left click on the Tidegauge item opens. Four
//! live readings with their recent curve, your AI plans, the busiest apps,
//! and the way into the full window.

use crate::app_cpu::AppCpu;
use crate::engine::{fmt_bytes, fmt_pct, fmt_rate, Snapshot};
use crate::locale::text as l;
use crate::processes::AppGroup;
use crate::theme as t;
use crate::ui::{app_icon, Card, Destination, Tab, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub const WIDTH: f32 = 340.;
/// The panel's height with the default two rows of tiles.
const BASE_HEIGHT: f32 = 600.;
/// One row of two tiles, with the gap below it.
const TILE_ROW: f32 = 108.;

/// A section's caption, its card of three rows and the gaps around them.
const SECTION: f32 = 128.;

/// What the panel shows, decided once when it opens and kept until it
/// closes. The window's size is fixed at that moment, so the tiles and
/// sections must not come or go while it is open: a section appearing would
/// clip the footer, one vanishing would leave a gap.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    tiles: Vec<String>,
    plans: bool,
    apps: bool,
}

impl Layout {
    /// The layout for a panel opened now.
    pub fn now() -> Self {
        Self::new(&crate::settings::Settings::load(), &crate::engine::latest().plans, crate::metrics::now_unix())
    }

    fn new(settings: &crate::settings::Settings, plans: &crate::agents::Plans, now: f64) -> Self {
        Layout {
            tiles: settings.panel_tiles.clone(),
            plans: settings.panel_show_plans && !current_plans(plans, now).is_empty(),
            apps: settings.panel_show_apps,
        }
    }

    /// The panel grows or shrinks by a row for every two tiles beyond or
    /// short of the default four, and by a section for each of AI plans and
    /// Busiest apps that it leaves out.
    pub fn height(&self) -> f32 {
        let rows = self.tiles.len().max(1).div_ceil(2) as f32;
        let missing = [self.plans, self.apps].iter().filter(|shown| !**shown).count() as f32;
        BASE_HEIGHT + (rows - 2.) * TILE_ROW - missing * SECTION
    }

    /// The plans to draw now: none unless the section was open for, else the
    /// first three that are being updated at this moment.
    fn plans<'a>(&self, plans: &'a crate::agents::Plans, now: f64) -> Vec<&'a crate::agents::Plan> {
        if self.plans {
            current_plans(plans, now)
        } else {
            Vec::new()
        }
    }
}

/// Whether the tile at `index` of `count` takes both columns: the last of an
/// odd number, which would otherwise leave a gap beside it.
fn spans_both_columns(index: usize, count: usize) -> bool {
    count % 2 == 1 && index + 1 == count
}

/// The first three plans that are being updated.
fn current_plans(plans: &crate::agents::Plans, now: f64) -> Vec<&crate::agents::Plan> {
    match plans {
        crate::agents::Plans::Loaded(plans) => crate::agents_panel::current_plans(plans, now).into_iter().take(3).collect(),
        _ => Vec::new(),
    }
}

pub struct MenuPanel {
    layout: Layout,
    snap: Snapshot,
    main: Entity<TideGaugeView>,
    focus: FocusHandle,
}

impl MenuPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, main: Entity<TideGaugeView>, layout: Layout) -> Self {
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
        Self { layout, snap: crate::engine::latest(), main, focus }
    }
}

/// Close the panel and show the full window on `to`.
fn open(main: &Entity<TideGaugeView>, to: Destination, cx: &mut App) {
    crate::app::close_popover(cx);
    crate::ui::navigate(main, to, cx);
    crate::app::toggle_panel(cx);
}

/// Whether the panel shows through to the desktop, which macOS's "Reduce
/// transparency" overrides.
pub fn see_through(settings: &crate::settings::Settings) -> bool {
    settings.panel_is_transparent(crate::appearance::reduce_transparency())
}

fn tile(
    card: Card,
    value: String,
    detail: String,
    (points, max): (&[f64], Option<f64>),
    full_width: bool,
    (dark, fill): (bool, Hsla),
    main: Entity<TideGaugeView>,
) -> impl IntoElement {
    let color = card.color(dark);
    let tab = card.opens();
    let keyboard = main.clone();
    div()
        .id(SharedString::from(format!("panel-{}", card.key())))
        .role(Role::Button)
        .aria_label(SharedString::from(format!("{}: {value}", card.label())))
        .tab_index(0)
        .min_w_0()
        .when(full_width, |d| d.col_span(2))
        .p(t::SPACE_3)
        .rounded(t::RADIUS_CONTROL)
        .bg(fill)
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
                .child(div().text_size(t::TEXT_SMALL).font_weight(FontWeight::MEDIUM).text_color(color).child(card.label()))
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

/// One tile's value, side note, curve and curve top.
fn tile_data(card: Card, s: &Snapshot) -> (String, String, Vec<f64>, Option<f64>) {
    let dash = || "—".to_string();
    match card {
        Card::Tab(Tab::Cpu) => (fmt_pct(s.cpu_pct), crate::locale::count(s.chip.cores as u64, "core", "cores", "kjerne", "kjerner"), s.cpu_hist.pts.clone(), Some(100.)),
        Card::Tab(Tab::Memory) => {
            let pct = s.mem.used as f64 / s.mem.total.max(1) as f64 * 100.;
            (fmt_bytes(s.mem.used as f64), fmt_pct(pct), s.mem_hist.pts.clone(), Some(100.))
        }
        // Download is the reading and its curve (as on Overview); upload is
        // the side note. A total would sit beside "↓" and read as a second,
        // conflicting number.
        Card::Tab(Tab::Network) => (
            format!("↓ {}", fmt_rate(s.net.rate_in)),
            format!("↑ {}", fmt_rate(s.net.rate_out)),
            s.net_in_hist.pts.clone(),
            None,
        ),
        Card::Tab(Tab::Disk) => (
            fmt_rate(s.disk.read_rate + s.disk.write_rate),
            format!("{} {}", fmt_bytes(s.disk.free as f64), l("free", "ledig")),
            sum(&s.disk_r_hist.pts, &s.disk_w_hist.pts),
            None,
        ),
        Card::Tab(Tab::Gpu) => (
            s.gpu.utilization.map(fmt_pct).unwrap_or_else(dash),
            s.gpu.power_w.map(crate::engine::fmt_watts).unwrap_or_default(),
            s.gpu_hist.pts.clone(),
            Some(100.),
        ),
        Card::Tab(Tab::Battery) => (
            s.battery.percent.map(fmt_pct).unwrap_or_else(dash),
            s.battery.health.map(|h| format!("{} {}", l("health", "helse"), fmt_pct(h))).unwrap_or_default(),
            s.batt_hist.pts.clone(),
            Some(100.),
        ),
        Card::Temperature => (
            s.temps.chip.map(crate::engine::fmt_celsius).unwrap_or_else(dash),
            s.temps.ssd.map(|c| format!("SSD {}", crate::engine::fmt_celsius(c))).unwrap_or_default(),
            s.temp_hist.pts.clone(),
            None,
        ),
        Card::Fans => (
            s.fans.iter().filter_map(|f| f.1).reduce(f64::max).map(fmt_pct).unwrap_or_else(dash),
            s.fans.iter().map(|f| f.0).reduce(f64::max).map(|rpm| format!("{rpm:.0} RPM")).unwrap_or_default(),
            s.fan_hist.pts.clone(),
            Some(100.),
        ),
        Card::Power => (
            s.power_w.map(crate::engine::fmt_watts).unwrap_or_else(dash),
            l("whole Mac", "hele Macen").into(),
            s.power_hist.pts.clone(),
            None,
        ),
        Card::Tab(_) => (dash(), String::new(), Vec::new(), None),
    }
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
        let settings = crate::settings::Settings::load();
        let alphas = t::panel_alphas(see_through(&settings), settings.transparency_amount);
        let ground = t::faded(t::bg(dark), alphas.bg);
        let fill = t::faded(t::card(dark), alphas.card);
        let s = &self.snap;
        let main = self.main.clone();
        let layout = &self.layout;
        let cpu = AppCpu::chosen(s.chip.cores);
        let mut busiest: Vec<&AppGroup> = s.apps.iter().filter(|a| a.cpu_pct > 0.).collect();
        busiest.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        let now = crate::metrics::now_unix();
        let plans: Vec<crate::agents_panel::PlanGlance> =
            layout.plans(&s.plans, now).into_iter().map(|p| crate::agents_panel::plan_glance(p, now, dark)).collect();
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
            .bg(ground)
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
                    .children({
                        let cards: Vec<Card> = layout.tiles.iter().filter_map(|key| Card::from_key(key)).collect();
                        let count = cards.len();
                        let main = main.clone();
                        cards.into_iter().enumerate().map(move |(i, card)| {
                            let (value, detail, points, max) = tile_data(card, s);
                            tile(card, value, detail, (&points, max), spans_both_columns(i, count), (dark, fill), main.clone())
                        })
                    })
            )
            .when(!plans.is_empty(), |d| {
                d.child(caption(l("AI plans", "AI-abonnementer"))).child(
                    div().rounded(t::RADIUS_CONTROL).bg(fill).py(t::SPACE_1).flex().flex_col().children(plans.into_iter().map(|plan| {
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
            .when(layout.apps, |d| d.child(caption(l("Busiest apps", "Mest aktive apper"))).child(
                div()
                    .rounded(t::RADIUS_CONTROL)
                    .bg(fill)
                    .py(t::SPACE_1)
                    .flex()
                    .flex_col()
                    .children(busiest.iter().take(3).enumerate().map(|(i, app)| {
                        let target = main.clone();
                        let keyboard = main.clone();
                        div()
                            .id(SharedString::from(format!("panel-app-{i}")))
                            .role(Role::Button)
                            .aria_label(SharedString::from(format!("{} {}", app.name, cpu.format(app.cpu_pct))))
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
                            .child(div().text_size(t::TEXT_BODY).text_color(t::ink_dim(dark)).child(cpu.format(app.cpu_pct)))
                            .on_click(move |_, _, cx| open(&target, Destination::Apps, cx))
                    })),
            ))
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
                    .child({
                        let main = main.clone();
                        footer_control("panel-slow-check", "icons/stethoscope.svg", l("Why is my Mac slow?", "Hvorfor er Macen min treg?"), false, dark, move |cx| {
                            crate::slow_check::run(&main, cx);
                            open(&main, Destination::Tab(Tab::Overview), cx)
                        })
                    })
                    .when(matches!(crate::updater::availability(), crate::updater::Availability::Available), |d| {
                        d.child(footer_control("panel-updates", "icons/refresh-cw.svg", crate::updater::check_for_updates_title(), false, dark, |cx| {
                            crate::app::close_popover(cx);
                            crate::updater::ask_for_check();
                        }))
                    })
                    .child({
                        let main = main.clone();
                        // Icons only beside the labelled Open button: the
                        // footer has no room for four labels.
                        footer_control("panel-settings", "icons/settings.svg", l("Settings", "Innstillinger"), false, dark, move |cx| {
                            open(&main, Destination::Settings, cx)
                        })
                    })
                    .child(footer_control("panel-quit", "icons/power.svg", l("Quit Tidegauge", "Avslutt Tidegauge"), false, dark, |cx| cx.quit())),
            )
    }
}

#[cfg(test)]
mod tests {
    // Named imports: the UI kit's glob export includes its own `test`.
    use super::{spans_both_columns, Layout, BASE_HEIGHT, SECTION, TILE_ROW};
    use crate::agents::{Plan, Plans};
    use crate::settings::Settings;

    const NOW: f64 = 100_000.;

    fn plan(provider: &str, age: f64) -> Plan {
        Plan {
            provider: provider.into(),
            plan: None,
            windows: Vec::new(),
            credits: None,
            fetched_at: Some(NOW - age),
            stale: false,
            failed: false,
        }
    }

    fn settings(tiles: usize, plans: bool, apps: bool) -> Settings {
        Settings {
            panel_tiles: crate::settings::PANEL_TILES.iter().take(tiles).map(|s| (*s).into()).collect(),
            panel_show_plans: plans,
            panel_show_apps: apps,
            ..Settings::default()
        }
    }

    #[test]
    fn only_the_last_tile_of_an_odd_count_takes_the_full_width() {
        for count in 1..=9usize {
            let full: Vec<usize> = (0..count).filter(|i| spans_both_columns(*i, count)).collect();
            if count % 2 == 1 {
                assert_eq!(full, [count - 1], "{count} tiles");
            } else {
                assert!(full.is_empty(), "{count} tiles pair up");
            }
        }
    }

    #[test]
    fn the_height_follows_tile_rows_and_the_sections_left_out() {
        let fresh = Plans::Loaded(vec![plan("claude", 60.)]);
        let height = |tiles, plans, apps| Layout::new(&settings(tiles, plans, apps), &fresh, NOW).height();
        let full = height(4, true, true);
        assert_eq!(full, BASE_HEIGHT);
        assert_eq!(height(5, true, true), height(6, true, true), "an odd last tile shares its row");
        assert_eq!(height(0, true, true), height(1, true, true), "at least one row");
        assert_eq!(height(6, true, true), full + TILE_ROW);
        assert_eq!(height(4, false, true), full - SECTION);
        assert_eq!(height(4, false, false), full - 2. * SECTION);
    }

    #[test]
    fn a_plan_that_is_not_updating_leaves_no_section() {
        let stale = Plans::Loaded(vec![plan("claude", 7200.)]);
        let layout = Layout::new(&settings(4, true, true), &stale, NOW);
        assert!(!layout.plans);
        assert_eq!(layout.height(), BASE_HEIGHT - SECTION);
        assert!(Layout::new(&settings(4, true, true), &Plans::Loaded(Vec::new()), NOW).height() < BASE_HEIGHT);
    }

    #[test]
    fn the_layout_stays_as_decided_when_plans_change_while_open() {
        let fresh = Plans::Loaded(vec![plan("claude", 60.), plan("codex", 60.)]);
        let stale = Plans::Loaded(vec![plan("claude", 7200.)]);

        // Opened with a plan: the section keeps its room and draws nothing
        // once the plan goes stale; the height never changes.
        let open_with = Layout::new(&settings(4, true, true), &fresh, NOW);
        let height = open_with.height();
        assert_eq!(open_with.plans(&fresh, NOW).len(), 2);
        assert!(open_with.plans(&stale, NOW + 1.).is_empty());
        assert_eq!(open_with.height(), height);

        // Opened without one: a plan appearing later is not drawn, so the
        // footer is never pushed out of the smaller window.
        let open_without = Layout::new(&settings(4, true, true), &stale, NOW);
        assert!(open_without.plans(&fresh, NOW + 1.).is_empty());
        assert_eq!(open_without.height(), BASE_HEIGHT - SECTION);

        // Switched off in Settings: never drawn, whatever the plans do.
        let off = Layout::new(&settings(4, false, true), &fresh, NOW);
        assert!(off.plans(&fresh, NOW).is_empty());

        // At most three, as before.
        let many = Plans::Loaded((0..5).map(|i| plan(&format!("p{i}"), 1.)).collect());
        assert_eq!(Layout::new(&settings(4, true, true), &many, NOW).plans(&many, NOW).len(), 3);
    }
}
