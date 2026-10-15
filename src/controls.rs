//! Dashboard controls share persistence and keyboard behavior with Settings.

use crate::locale::text as l;
use crate::settings::Settings;
use crate::theme as t;
use crate::ui::{
    caption, card, empty_state, section_header, sub, update_preference, Tab, TideGaugeView,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub(crate) struct ControlSpec {
    pub id: String,
    pub label: String,
    pub aria: Option<String>,
    pub role: Role,
    pub tab_index: isize,
    pub selected: bool,
    pub accent: Hsla,
    pub dark: bool,
    pub framed: bool,
}

pub(crate) fn control(
    spec: ControlSpec,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = spec.label.clone();
    control_with(spec, label, action)
}

/// Like `control`, with `content` (an icon, say) in place of the text label.
/// `spec.label` still names the control for assistive technology.
fn control_with(
    spec: ControlSpec,
    content: impl IntoElement,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let action = std::rc::Rc::new(action);
    let keyboard = action.clone();
    let toggled = matches!(spec.role, Role::RadioButton | Role::Switch);
    let mut base = div()
        .id(SharedString::from(spec.id))
        .role(spec.role)
        .aria_label(SharedString::from(
            spec.aria.unwrap_or_else(|| spec.label.clone()),
        ))
        .tab_index(spec.tab_index)
        .h(t::CONTROL_H)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .px_3()
        .whitespace_nowrap()
        .text_size(t::TEXT_BODY)
        .text_color(if spec.selected {
            spec.accent
        } else {
            t::ink(spec.dark)
        })
        .cursor_pointer()
        .hover(|d| d.bg(t::card_hi(spec.dark)));
    if toggled {
        base = base.aria_toggled(if spec.selected {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if spec.framed {
        base = base
            .rounded(t::RADIUS_CONTROL)
            .border_1()
            .border_color(if spec.selected {
                spec.accent
            } else {
                t::line(spec.dark)
            })
            .focus_visible(|d| d.border_color(spec.accent));
    } else {
        base = base
            .border_1()
            .border_color(t::line(spec.dark).opacity(0.))
            .focus_visible(|d| d.border_color(spec.accent));
    }
    if spec.selected {
        base = base.bg(spec.accent.opacity(0.12));
    }
    base.child(content)
        .on_click(move |_, window, cx| action(window, cx))
        .capture_key_down(move |event, window, cx| {
            if is_activation_key(&event.keystroke) {
                window.prevent_default();
                keyboard(window, cx);
                cx.stop_propagation();
            }
        })
}

pub(crate) fn is_activation_key(keystroke: &Keystroke) -> bool {
    matches!(keystroke.key.as_str(), "enter" | "space") && !keystroke.modifiers.modified()
}

fn spec(
    id: impl Into<String>,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    accent: Hsla,
) -> ControlSpec {
    ControlSpec {
        id: id.into(),
        label: label.into(),
        aria: None,
        role: Role::Button,
        tab_index: 0,
        selected,
        accent,
        dark,
        framed: true,
    }
}

pub fn button(
    id: impl Into<String>,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    control(spec(id, label, selected, dark, t::accent(dark)), action)
}

pub fn button_accent(
    id: impl Into<String>,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    accent: Hsla,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    control(spec(id, label, selected, dark, accent), action)
}

pub fn button_indexed(
    id: impl Into<String>,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    tab_index: isize,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut s = spec(id, label, selected, dark, t::accent(dark));
    s.tab_index = tab_index;
    control(s, action)
}

pub fn button_labeled(
    id: impl Into<String>,
    label: impl Into<String>,
    aria: impl Into<String>,
    selected: bool,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut s = spec(id, label, selected, dark, t::accent(dark));
    s.aria = Some(aria.into());
    control(s, action)
}

pub fn segment(
    id: impl Into<String>,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    accent: Hsla,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut s = spec(id, label, selected, dark, accent);
    s.role = Role::RadioButton;
    s.framed = false;
    control(s, action)
}

pub fn segment_group(id: impl Into<String>, label: impl Into<String>, dark: bool) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.into()))
        .role(Role::RadioGroup)
        .aria_label(SharedString::from(label.into()))
        .tab_group()
        .flex()
        .items_center()
        .border_1()
        .border_color(t::line(dark))
        .rounded(t::RADIUS_CONTROL)
        .overflow_hidden()
}

pub fn stepper_group(id: impl Into<String>, label: impl Into<String>, dark: bool) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.into()))
        .role(Role::Group)
        .aria_label(SharedString::from(label.into()))
        .flex()
        .items_center()
        .border_1()
        .border_color(t::line(dark))
        .rounded(t::RADIUS_CONTROL)
        .overflow_hidden()
}

pub fn step_button(
    id: impl Into<String>,
    label: impl Into<String>,
    aria: impl Into<String>,
    dark: bool,
    accent: Hsla,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut s = spec(id, label, false, dark, accent);
    s.aria = Some(aria.into());
    s.framed = false;
    control(s, action)
}

pub fn step_value(value: String, dark: bool) -> Div {
    div()
        .min_w(px(56.))
        .h(t::CONTROL_H)
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(t::ink(dark))
        .child(value)
}

/// A framed, icon-only action button (the label is its accessible name).
fn icon_button(
    id: impl Into<String>,
    icon: &'static str,
    aria: impl Into<String>,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let content = svg().path(icon).size(px(14.)).text_color(t::ink(dark));
    control_with(spec(id, aria, false, dark, t::accent(dark)), content, action)
}

/// A compact two-choice switch for a boolean preference: the segment that
/// matches the current value carries the accent tint.
fn on_off(
    id: &str,
    name: &str,
    value: bool,
    labels: (&'static str, &'static str),
    dark: bool,
    this: Entity<TideGaugeView>,
    set: impl Fn(&mut Settings, bool) + Clone + 'static,
) -> impl IntoElement {
    let mut group = segment_group(format!("{id}-group"), name, dark);
    for (choice, label) in [(true, labels.0), (false, labels.1)] {
        let this = this.clone();
        let set = set.clone();
        group = group.child(segment(
            format!("{id}-{}", if choice { "on" } else { "off" }),
            label,
            value == choice,
            dark,
            t::accent(dark),
            move |window, cx| update_preference(&this, |s| set(s, choice), window, cx),
        ));
    }
    group
}

fn preference(
    id: String,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    this: Entity<TideGaugeView>,
    change: impl Fn(&mut Settings) + 'static,
) -> impl IntoElement {
    button(id, label, selected, dark, move |window, cx| {
        update_preference(&this, |s| change(s), window, cx)
    })
}

fn step_preference(
    id: String,
    label: impl Into<String>,
    aria: impl Into<String>,
    dark: bool,
    this: Entity<TideGaugeView>,
    change: impl Fn(&mut Settings) + 'static,
) -> impl IntoElement {
    step_button(id, label, aria, dark, t::accent(dark), move |window, cx| {
        update_preference(&this, |s| change(s), window, cx)
    })
}

pub fn settings(dark: bool, this: Entity<TideGaugeView>) -> Div {
    let s = Settings::load();
    let alerts = [
        ("cpu", "CPU", s.alert_cpu_enabled),
        (
            "mem",
            l("Memory in use", "Minne i bruk"),
            s.alert_mem_enabled,
        ),
        (
            "disk",
            l("Disk · above 400 MB/s", "Disk · over 400 MB/s"),
            s.alert_disk_enabled,
        ),
        (
            "net",
            l("Network · above 80 MB/s", "Nettverk · over 80 MB/s"),
            s.alert_network_enabled,
        ),
    ];
    let mut alert_panel = card(dark).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("Alerts", "Varsler")))
        .child(sub(l("Opt-in. CPU/memory: sustained for 20 seconds; disk/network: 30 seconds. One alert until recovery, with a 10-minute cooldown per metric.", "Valgfritt. CPU/minne: vedvarende i 20 sekunder; disk/nettverk: 30 sekunder. Ett varsel frem til normalisering, med 10 minutters pause per måling.")));
    for (key, label, enabled) in alerts {
        let mut row = div()
            .id(SharedString::from(format!("alert-row-{key}")))
            .role(Role::Group)
            .aria_label(label)
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .child(div().flex_1().text_sm().child(label));
        if key == "cpu" || key == "mem" {
            let value = if key == "cpu" {
                s.alert_cpu_pct
            } else {
                s.alert_mem_pct
            };
            row = row
                .aria_label(SharedString::from(format!("{label} · {}", crate::engine::fmt_pct(value))))
                .child(
                    stepper_group(
                        format!("threshold-{key}-group"),
                        format!("{label} {}", l("threshold", "terskel")),
                        dark,
                    )
                    .child(step_preference(
                        format!("threshold-{key}-down"),
                        l("− 5", "− 5"),
                        format!("{label} · {}", l("Decrease", "Senk")),
                        dark,
                        this.clone(),
                        move |s| {
                            let v = if key == "cpu" {
                                &mut s.alert_cpu_pct
                            } else {
                                &mut s.alert_mem_pct
                            };
                            *v = (*v - 5.).max(1.);
                        },
                    ))
                    .child(step_value(crate::engine::fmt_pct(value), dark))
                    .child(step_preference(
                        format!("threshold-{key}-up"),
                        "+ 5",
                        format!("{label} · {}", l("Increase", "Øk")),
                        dark,
                        this.clone(),
                        move |s| {
                            let v = if key == "cpu" {
                                &mut s.alert_cpu_pct
                            } else {
                                &mut s.alert_mem_pct
                            };
                            *v = (*v + 5.).min(100.);
                        },
                    )),
                );
        }
        row = row.child(preference(
            format!("alert-{key}"),
            if enabled {
                l("On", "På")
            } else {
                l("Off", "Av")
            },
            enabled,
            dark,
            this.clone(),
            move |s| {
                let flag = match key {
                    "cpu" => &mut s.alert_cpu_enabled,
                    "mem" => &mut s.alert_mem_enabled,
                    "disk" => &mut s.alert_disk_enabled,
                    _ => &mut s.alert_network_enabled,
                };
                *flag = !enabled;
            },
        ));
        alert_panel = alert_panel.child(row);
    }
    alert_panel = alert_panel.child(sub(l("Permission is requested with the first alert. macOS notification settings and Focus may suppress delivery. Memory measures usage, not memory pressure.", "Tillatelse forespørres ved første varsel. Varselinnstillinger og Fokus i macOS kan hindre levering. Minnetallet viser bruk, ikke minnepress.")));

    let mut order = s.dashboard_tabs.clone();
    for key in crate::settings::DASHBOARD_TABS {
        if !order.iter().any(|x| x == key) {
            order.push(key.into());
        }
    }
    let mut tabs = div().flex().flex_col().gap_2().child(
        div()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .child(l("Tab order", "Fanerekkefølge")),
    );
    let enabled_count = s.dashboard_tabs.len();
    for key in order {
        let Some(tab) = Tab::from_key(&key) else {
            continue;
        };
        let position = s.dashboard_tabs.iter().position(|x| x == &key);
        let toggle_key = key.clone();
        let up_key = key.clone();
        let down_key = key.clone();
        // Arrows sit in a fixed-width slot so the switches line up; the first
        // row has no up arrow and the last no down arrow.
        let mut arrows = div().w(px(88.)).flex_shrink_0().flex().justify_end().gap_2();
        if let Some(i) = position {
            if i > 0 {
                arrows = arrows.child(icon_button(
                    format!("up-{key}"),
                    "icons/arrow-up.svg",
                    format!("{} {} {}", l("Move", "Flytt"), tab.label(), l("up", "opp")),
                    dark,
                    {
                        let this = this.clone();
                        move |window, cx| {
                            let up_key = up_key.clone();
                            update_preference(&this, move |s| {
                                if let Some(i) = s.dashboard_tabs.iter().position(|x| x == &up_key) {
                                    if i > 0 {
                                        s.dashboard_tabs.swap(i, i - 1);
                                    }
                                }
                            }, window, cx)
                        }
                    },
                ));
            }
            if i + 1 < enabled_count {
                arrows = arrows.child(icon_button(
                    format!("down-{key}"),
                    "icons/arrow-down.svg",
                    format!("{} {} {}", l("Move", "Flytt"), tab.label(), l("down", "ned")),
                    dark,
                    {
                        let this = this.clone();
                        move |window, cx| {
                            let down_key = down_key.clone();
                            update_preference(&this, move |s| {
                                if let Some(i) = s.dashboard_tabs.iter().position(|x| x == &down_key) {
                                    if i + 1 < s.dashboard_tabs.len() {
                                        s.dashboard_tabs.swap(i, i + 1);
                                    }
                                }
                            }, window, cx)
                        }
                    },
                ));
            }
        }
        tabs = tabs.child(
            div()
                .id(SharedString::from(format!("tab-row-{key}")))
                .role(Role::Group)
                .aria_label(tab.label())
                .flex()
                .gap_2()
                .items_center()
                .child(div().flex_1().text_sm().child(tab.label()))
                .child(arrows)
                .child(on_off(
                    &format!("show-{key}"),
                    tab.label(),
                    position.is_some(),
                    (l("Shown", "Vist"), l("Hidden", "Skjult")),
                    dark,
                    this.clone(),
                    move |s, on| {
                        if !on {
                            s.dashboard_tabs.retain(|x| x != &toggle_key);
                        } else if !s.dashboard_tabs.contains(&toggle_key) {
                            s.dashboard_tabs.push(toggle_key.clone());
                        }
                    },
                )),
        );
    }
    let mut favorites = div().flex().flex_col().gap_2().child(
        div()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .child(l("Overview favorites", "Favoritter i oversikten")),
    );
    for key in crate::settings::FAVORITE_METRICS {
        let enabled = s.favorite_metrics.iter().any(|x| x == key);
        let label = Tab::from_key(key).unwrap().label();
        favorites = favorites.child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(div().flex_1().text_sm().child(label))
                .child(on_off(
                    &format!("favorite-{key}"),
                    label,
                    enabled,
                    (l("Shown", "Vist"), l("Hidden", "Skjult")),
                    dark,
                    this.clone(),
                    move |s, on| {
                        if !on {
                            s.favorite_metrics.retain(|x| x != key);
                        } else if !s.favorite_metrics.iter().any(|x| x == key) {
                            s.favorite_metrics.push(key.into());
                        }
                    },
                )),
        );
    }
    favorites = favorites
        .child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(div().flex_1().text_sm().child(l("Compact layout", "Kompakt oppsett")))
                .child(on_off(
                    "compact-dashboard",
                    l("Compact layout", "Kompakt oppsett"),
                    s.compact_dashboard,
                    (l("On", "På"), l("Off", "Av")),
                    dark,
                    this.clone(),
                    |s, on| s.compact_dashboard = on,
                )),
        )
        .child(sub(l(
            "Overview and Settings stay available. Menu-bar readings are independent.",
            "Oversikt og Innstillinger er alltid tilgjengelige. Menylinjemålinger er uavhengige.",
        )))
        .child(preference(
            "reset-dashboard".into(),
            l("Reset dashboard only", "Tilbakestill kun oversikten"),
            false,
            dark,
            this.clone(),
            Settings::reset_dashboard,
        ));
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(keep_awake(dark, this))
        .child(alert_panel)
        .child(
            card(dark)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(l("Dashboard layout", "Oversiktsoppsett")),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_6()
                        .child(tabs)
                        .child(favorites),
                ),
        )
}

pub fn keep_awake(dark: bool, this: Entity<TideGaugeView>) -> Div {
    let status = crate::keepawake::status();
    let title = if status.active {
        status
            .remaining
            .map(|remaining| {
                format!(
                    "{} · {}:{:02}",
                    l("Keeping awake", "Holder våken"),
                    remaining.as_secs() / 60,
                    remaining.as_secs() % 60
                )
            })
            .unwrap_or_else(|| {
                l(
                    "Keeping awake · until stopped",
                    "Holder våken · frem til stopp",
                )
                .into()
            })
    } else {
        l("Keep awake", "Hold våken").into()
    };
    let mut choices = div().flex().gap_2().flex_wrap();
    for (minutes, label) in [
        (Some(15), l("15 min", "15 min")),
        (Some(30), l("30 min", "30 min")),
        (Some(60), l("1 hour", "1 time")),
        (None, l("Until stopped", "Frem til stopp")),
    ] {
        let target = this.clone();
        choices = choices.child(button(
            format!("awake-{minutes:?}"),
            label,
            false,
            dark,
            move |_, cx| {
                let result = crate::keepawake::start(
                    minutes.map(|m| std::time::Duration::from_secs(m * 60)),
                );
                target.update(cx, |v, cx| {
                    v.settings_error = result.err();
                    cx.notify();
                });
            },
        ));
    }
    if status.active {
        let target = this;
        choices = choices.child(button(
            "awake-stop",
            l("Stop", "Stopp"),
            true,
            dark,
            move |_, cx| {
                let result = crate::keepawake::stop();
                target.update(cx, |v, cx| {
                    v.settings_error = result.err();
                    cx.notify();
                });
            },
        ));
    }
    card(dark).child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(sub(l("Prevents idle system sleep only. The display may sleep; closing the lid still sleeps. Off after quitting or restarting Tidegauge.", "Hindrer kun automatisk systemdvale. Skjermen kan sove; lukket lokk gir fortsatt dvale. Av etter avslutning eller omstart av Tidegauge.")))
        .child(choices).when_some(status.error, |d,e| d.child(sub(e).text_color(t::warn(dark))))
}

pub fn refresh_history(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.history_request.end = crate::metrics::now_unix();
        view.history_generation += 1;
        view.history_loading = true;
        let request = view.history_request.clone();
        let generation = view.history_generation;
        cx.spawn(async move |this, cx| {
            let result = smol::unblock(move || crate::history_browser::load(&request)).await;
            let _ = this.update(cx, |view, cx| {
                if view.history_generation == generation {
                    view.history_data = Some(result);
                    view.history_loading = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    });
}

fn change_history(
    this: &Entity<TideGaugeView>,
    cx: &mut App,
    change: impl FnOnce(&mut crate::history_browser::Request),
) {
    this.update(cx, |view, cx| {
        change(&mut view.history_request);
        view.history_app_picker = false;
        cx.notify();
    });
    refresh_history(this, cx);
}

pub fn history_panel(
    view: &TideGaugeView,
    dark: bool,
    this: Entity<TideGaugeView>,
    _cx: &mut Context<TideGaugeView>,
) -> Div {
    use crate::history_browser::{Metric, RANGES};
    let app = view.history_request.app.is_some();
    let accent = match view.history_request.metric {
        Metric::Cpu => t::Metric::Cpu,
        Metric::Memory => t::Metric::Memory,
        Metric::Gpu => t::Metric::Gpu,
        Metric::Battery => t::Metric::Battery,
        Metric::DiskRead | Metric::DiskWrite => t::Metric::Disk,
        _ => t::Metric::Network,
    }
    .accent(dark);
    let row_label = |text: &'static str| {
        div()
            .w(px(72.))
            .flex_shrink_0()
            .text_xs()
            .text_color(t::ink_dim(dark))
            .child(text)
    };
    let mut ranges = segment_group("history-ranges", l("Range", "Tidsrom"), dark);
    for (hours, (en, no)) in RANGES {
        let target = this.clone();
        ranges = ranges.child(segment(
            format!("history-{hours}"),
            l(en, no),
            view.history_request.hours == hours,
            dark,
            t::accent(dark),
            move |_, cx| change_history(&target, cx, |r| r.hours = hours),
        ));
    }
    let refresh = this.clone();
    let mut metrics = segment_group("history-metrics", l("Metric", "Måling"), dark);
    for &metric in if app {
        &Metric::APP[..]
    } else {
        &Metric::ALL[..]
    } {
        let target = this.clone();
        metrics = metrics.child(segment(
            format!("history-{metric:?}"),
            metric.label(app),
            view.history_request.metric == metric,
            dark,
            accent,
            move |_, cx| change_history(&target, cx, |r| r.metric = metric),
        ));
    }
    let picker = this.clone();
    let mut panel = div()
        .flex()
        .flex_col()
        .gap_4()
        .child(section_header(
            l("History", "Historikk"),
            l("Read-only", "Kun avlesning"),
            dark,
        ))
        .child(
            card(dark)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(row_label(l("Range", "Tidsrom")))
                        .child(ranges)
                        .child(div().flex_1())
                        .child(button_accent(
                            "refresh-history",
                            l("Refresh", "Oppdater"),
                            false,
                            dark,
                            accent,
                            move |_, cx| refresh_history(&refresh, cx),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(row_label(l("Source", "Kilde")))
                        .child(button_accent(
                            "history-app-picker",
                            format!(
                                "{} ▾",
                                view.history_request
                                    .app
                                    .as_deref()
                                    .unwrap_or(l("System", "System"))
                            ),
                            view.history_app_picker,
                            dark,
                            accent,
                            move |_, cx| {
                                picker.update(cx, |v, cx| {
                                    v.history_app_picker = !v.history_app_picker;
                                    cx.notify();
                                });
                            },
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(row_label(l("Metric", "Måling")))
                        .child(metrics),
                ),
        );
    if view.history_app_picker {
        let mut choices = vec![None];
        if let Some(Ok(data)) = &view.history_data {
            choices.extend(data.apps.iter().cloned().map(Some));
        }
        panel = panel.child(
            card(dark).child(
                div()
                    .id("history-apps")
                    .h(px(180.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(choices.into_iter().enumerate().map(|(i, name)| {
                        let target = this.clone();
                        button_accent(
                            format!("history-app-{i}"),
                            name.as_deref().unwrap_or(l("System", "System")).to_owned(),
                            view.history_request.app == name,
                            dark,
                            accent,
                            move |_, cx| {
                                change_history(&target, cx, |r| {
                                    r.app = name.clone();
                                    if r.app.is_some() && !Metric::APP.contains(&r.metric) {
                                        r.metric = Metric::Cpu;
                                    }
                                });
                            },
                        )
                    })),
            ),
        );
    }
    if view.history_loading {
        panel = panel.child(empty_state(
            "icons/clock.svg",
            accent,
            l("Loading local history…", "Laster lokal historikk…").into(),
            l(
                "Reading only the local database on this Mac.",
                "Leser kun den lokale databasen på denne Macen.",
            )
            .into(),
            dark,
        ));
    } else if let Some(Ok(data)) = &view.history_data {
        let metric = view.history_request.metric;
        let values: Vec<f64> = data.values.iter().flatten().copied().collect();
        if values.is_empty() {
            panel = panel.child(empty_state(
                "icons/clock.svg",
                accent,
                l(
                    "No recorded measurements in this range.",
                    "Ingen registrerte målinger i dette tidsrommet.",
                )
                .into(),
                l(
                    "Missing intervals are gaps, not zero values.",
                    "Manglende intervaller er hull, ikke nullverdier.",
                )
                .into(),
                dark,
            ));
        } else {
            let max =
                values
                    .iter()
                    .copied()
                    .fold(0., f64::max)
                    .max(if !app && metric.unit(app) == "%" {
                        100.
                    } else {
                        1.
                    });
            let samples = data.counts.iter().sum::<u64>();
            let format_value = |v| match metric.unit(app) {
                "B" => crate::engine::fmt_bytes(v),
                "B/s" => crate::engine::fmt_rate(v),
                "%" => crate::locale::percent(v, 1),
                unit => format!("{} {unit}", crate::locale::number(v, 1)),
            };
            let mut plot = card(dark)
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap_3()
                        .child(sub(crate::locale::count(samples, "sample", "samples", "måling", "målinger")))
                        .child(sub(format_value(max))),
                )
                .child(history_chart(&data.values, max, accent))
                .child(sub(format_value(0.)))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .gap_2()
                        .text_xs()
                        .text_color(t::ink_dim(dark))
                        .child(crate::history_browser::local_time(data.start))
                        .child(l("Local time", "Lokal tid"))
                        .child(crate::history_browser::local_time(data.end)),
                );
            if values.len() * 5 < data.values.len() {
                // Under ~20% of the buckets hold data; say so instead of leaving
                // a mostly blank plot unexplained.
                plot = plot.child(sub(l(
                    "Few recorded measurements in this range — most intervals have no data.",
                    "Få registrerte målinger i dette tidsrommet — de fleste intervallene mangler data.",
                )));
            }
            panel = panel
                .child(section_header(metric.label(app), metric.unit(app), dark))
                .child(plot);
        }
    } else if let Some(Err(error)) = &view.history_data {
        panel = panel.child(empty_state(
            "icons/clock.svg",
            t::warn(dark),
            l("History unavailable", "Historikk utilgjengelig").into(),
            error.clone(),
            dark,
        ));
    }
    panel.child(recording_details(dark))
}

fn recording_details(dark: bool) -> Div {
    let row = |label: &'static str, text: &'static str| {
        div()
            .flex()
            .gap_3()
            .child(
                div()
                    .w(px(112.))
                    .flex_shrink_0()
                    .child(caption(label, dark)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(sub(text).text_color(t::ink_dim(dark))),
            )
    };
    card(dark)
        .child(caption(
            l("Recording details", "Registreringsdetaljer"),
            dark,
        ))
        .child(row(
            l("Retention", "Lagring"),
            l(
                "30-day target, pruned hourly",
                "mål om 30 dager, ryddes hver time",
            ),
        ))
        .child(row(
            l("Values", "Verdier"),
            l(
                "bucket means, never peaks; missing is not zero",
                "intervallgjennomsnitt, aldri topper; manglende er ikke null",
            ),
        ))
        .child(row(
            l("Refresh", "Oppdater"),
            l(
                "manual — include new samples with Refresh",
                "manuell — inkluder nye målinger med Oppdater",
            ),
        ))
        .child(row(
            l("Apps", "Apper"),
            l(
                "top 60 recorded names; absence does not prove inactivity",
                "topp 60 registrerte navn; fravær beviser ikke inaktivitet",
            ),
        ))
        .child(row(
            "CPU",
            l(
                "app CPU can exceed 100% across cores",
                "app-CPU kan overstige 100 % på flere kjerner",
            ),
        ))
        .child(row(
            l("GPU · Battery", "GPU · Batteri"),
            l(
                "legacy zero placeholders are omitted",
                "eldre nullverdier er utelatt",
            ),
        ))
        .child(row(
            l("Power", "Effekt"),
            l(
                "estimates are not shown as measurements",
                "estimater vises ikke som målinger",
            ),
        ))
}

fn history_chart(values: &[Option<f64>], max: f64, color: Hsla) -> impl IntoElement {
    let values = values.to_vec();
    canvas(
        move |_, _, _| (),
        move |bounds, _, window, _| {
            let width: f32 = bounds.size.width.into();
            let height: f32 = bounds.size.height.into();
            let ox: f32 = bounds.origin.x.into();
            let oy: f32 = bounds.origin.y.into();
            let step = width / values.len().max(1) as f32;
            for (index, value) in values.iter().enumerate() {
                let left = ox + index as f32 * step;
                let right = left + (step - 1.).max(1.);
                let bottom = oy + height;
                match value {
                    Some(value) => {
                        let top =
                            bottom - ((*value / max).clamp(0., 1.) as f32 * (height - 2.)).max(1.);
                        let mut bar = Path::new(point(px(left), px(bottom)));
                        bar.line_to(point(px(left), px(top)));
                        bar.line_to(point(px(right), px(top)));
                        bar.line_to(point(px(right), px(bottom)));
                        bar.line_to(point(px(left), px(bottom)));
                        window.paint_path(bar, color.opacity(0.8));
                    }
                    None => {
                        // A faint baseline tick marks the empty slot so missing
                        // data reads as intentional, not as a layout bug.
                        let mut tick = Path::new(point(px(left), px(bottom)));
                        tick.line_to(point(px(left), px(bottom - 2.)));
                        tick.line_to(point(px(right), px(bottom - 2.)));
                        tick.line_to(point(px(right), px(bottom)));
                        tick.line_to(point(px(left), px(bottom)));
                        window.paint_path(tick, color.opacity(0.12));
                    }
                }
            }
        },
    )
    .w_full()
    .h(px(180.))
}

pub fn audio_output(
    audio: &crate::engine::AudioInfo,
    dark: bool,
    this: Entity<TideGaugeView>,
    pending: bool,
) -> Div {
    let id = audio.output_id;
    let mut controls = div().flex().flex_col().gap_2().child(sub(l(
        "Controls the default media output device, not individual apps.",
        "Styrer standard lydutgang, ikke enkeltapper.",
    )));
    if pending {
        return controls.child(sub(l("Updating output…", "Oppdaterer lydutgang…")));
    }
    let accent = t::Metric::Sound.accent(dark);
    let mut buttons = div().flex().gap_3();
    if audio.can_volume {
        if let Some(volume) = audio.device_volume {
            let down = this.clone();
            let up = this.clone();
            buttons = buttons.child(
                stepper_group("output-volume", l("Output volume", "Utgangsvolum"), dark)
                    .child(step_button(
                        "volume-down",
                        l("− 10%", "− 10 %"),
                        format!("{} {}", l("Decrease volume by", "Senk volumet med"), crate::engine::fmt_pct(10.)),
                        dark,
                        accent,
                        move |_, cx| {
                            audio_action(&down, cx, move || {
                                crate::audio::adjust_output_volume(id, -0.1)
                            });
                        },
                    ))
                    .child(step_value(crate::engine::fmt_pct(volume as f64 * 100.), dark))
                    .child(step_button(
                        "volume-up",
                        l("+ 10%", "+ 10 %"),
                        format!("{} {}", l("Increase volume by", "Øk volumet med"), crate::engine::fmt_pct(10.)),
                        dark,
                        accent,
                        move |_, cx| {
                            audio_action(&up, cx, move || {
                                crate::audio::adjust_output_volume(id, 0.1)
                            });
                        },
                    )),
            );
        }
    }
    if audio.can_mute {
        if let Some(muted) = audio.device_muted {
            buttons = buttons.child(button_accent(
                "output-mute",
                if muted {
                    l("Unmute", "Slå på lyden")
                } else {
                    l("Mute", "Demp")
                },
                muted,
                dark,
                accent,
                move |_, cx| {
                    audio_action(&this, cx, move || crate::audio::toggle_output_mute(id));
                },
            ));
        }
    }
    controls = controls.child(buttons);
    if !audio.can_volume || !audio.can_mute {
        controls = controls.child(sub(l("This output does not expose all volume/mute controls. Use macOS Sound settings for unsupported controls.", "Denne utgangen støtter ikke alle volum-/dempefunksjoner. Bruk lydinnstillingene i macOS for funksjoner som ikke støttes.")));
    }
    controls
}

fn audio_action(
    this: &Entity<TideGaugeView>,
    cx: &mut App,
    action: impl FnOnce() -> Result<(), String> + Send + 'static,
) {
    this.update(cx, |view, cx| {
        if view.audio_pending {
            return;
        }
        view.audio_pending = true;
        view.audio_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (result, output) =
                smol::unblock(move || (action(), crate::audio::output_state())).await;
            let _ = this.update(cx, |view, cx| {
                view.audio_pending = false;
                view.audio_error = result.err();
                view.snap.audio.output_id = output.id;
                view.snap.audio.device_name = output.name;
                view.snap.audio.device_volume = output.volume;
                view.snap.audio.device_muted = output.muted;
                view.snap.audio.can_volume = output.can_volume;
                view.snap.audio.can_mute = output.can_mute;
                cx.notify();
            });
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::is_activation_key;
    use gpui_kit::Keystroke;

    #[test]
    fn activation_key_matches_unmodified_enter_and_space() {
        for key in ["enter", "space"] {
            let stroke = Keystroke::parse(key).unwrap();
            assert!(is_activation_key(&stroke), "{key} must activate");
        }
    }

    #[test]
    fn activation_key_rejects_modified_and_other_keys() {
        for key in [
            "shift-enter",
            "shift-space",
            "cmd-enter",
            "ctrl-space",
            "alt-space",
            "tab",
            "a",
            "escape",
        ] {
            let stroke = Keystroke::parse(key).unwrap();
            assert!(!is_activation_key(&stroke), "{key} must not activate");
        }
    }
}
