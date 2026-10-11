//! Dashboard controls share persistence and keyboard behavior with Settings.

use crate::locale::text as l;
use crate::settings::Settings;
use crate::theme as t;
use crate::ui::{card, sub, update_preference, Tab, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn button(
    id: impl Into<String>,
    label: impl Into<String>,
    selected: bool,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let action = std::rc::Rc::new(action);
    let keyboard = action.clone();
    let label = label.into();
    div()
        .id(SharedString::from(id.into()))
        .role(Role::Button)
        .aria_label(SharedString::from(label.clone()))
        .tab_index(0)
        .px_3()
        .py_2()
        .rounded(px(7.))
        .border_1()
        .border_color(t::line(dark))
        .bg(if selected {
            t::line(dark)
        } else {
            t::card_hi(dark)
        })
        .text_size(px(12.))
        .text_color(t::ink(dark))
        .cursor_pointer()
        .hover(|d| d.bg(t::line(dark)))
        .focus_visible(|d| d.border_color(t::ink_dim(dark)))
        .child(label)
        .on_click(move |_, window, cx| action(window, cx))
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                keyboard(window, cx);
                cx.stop_propagation();
            }
        })
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
                .aria_label(SharedString::from(format!("{label} · {value:.0}%")))
                .child(preference(
                    format!("threshold-{key}-down"),
                    l("− 5", "− 5"),
                    false,
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
                .child(
                    div()
                        .w(px(64.))
                        .text_center()
                        .text_sm()
                        .child(format!("{value:.0}%")),
                )
                .child(preference(
                    format!("threshold-{key}-up"),
                    "+ 5",
                    false,
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
                ));
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
    for key in order {
        let Some(tab) = Tab::from_key(&key) else {
            continue;
        };
        let enabled = s.dashboard_tabs.contains(&key);
        let toggle_key = key.clone();
        let up_key = key.clone();
        let down_key = key.clone();
        let mut row = div()
            .id(SharedString::from(format!("tab-row-{key}")))
            .role(Role::Group)
            .aria_label(tab.label())
            .flex()
            .gap_2()
            .items_center()
            .child(div().flex_1().text_sm().child(tab.label()))
            .child(preference(
                format!("show-{key}"),
                if enabled {
                    l("Shown", "Vist")
                } else {
                    l("Hidden", "Skjult")
                },
                enabled,
                dark,
                this.clone(),
                move |s| {
                    if enabled {
                        s.dashboard_tabs.retain(|x| x != &toggle_key);
                    } else {
                        s.dashboard_tabs.push(toggle_key.clone());
                    }
                },
            ));
        if enabled {
            row = row
                .child(preference(
                    format!("up-{key}"),
                    "↑",
                    false,
                    dark,
                    this.clone(),
                    move |s| {
                        if let Some(i) = s.dashboard_tabs.iter().position(|x| x == &up_key) {
                            if i > 0 {
                                s.dashboard_tabs.swap(i, i - 1);
                            }
                        }
                    },
                ))
                .child(preference(
                    format!("down-{key}"),
                    "↓",
                    false,
                    dark,
                    this.clone(),
                    move |s| {
                        if let Some(i) = s.dashboard_tabs.iter().position(|x| x == &down_key) {
                            if i + 1 < s.dashboard_tabs.len() {
                                s.dashboard_tabs.swap(i, i + 1);
                            }
                        }
                    },
                ));
        }
        tabs = tabs.child(row);
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
        favorites = favorites.child(preference(
            format!("favorite-{key}"),
            format!(
                "{} · {}",
                label,
                if enabled {
                    l("Shown", "Vist")
                } else {
                    l("Hidden", "Skjult")
                }
            ),
            enabled,
            dark,
            this.clone(),
            move |s| {
                if enabled {
                    s.favorite_metrics.retain(|x| x != key);
                } else {
                    s.favorite_metrics.push(key.into());
                }
            },
        ));
    }
    favorites = favorites
        .child(preference(
            "compact-dashboard".into(),
            format!(
                "{} · {}",
                l("Compact layout", "Kompakt oppsett"),
                if s.compact_dashboard {
                    l("On", "På")
                } else {
                    l("Off", "Av")
                }
            ),
            s.compact_dashboard,
            dark,
            this.clone(),
            |s| s.compact_dashboard = !s.compact_dashboard,
        ))
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
    let mut ranges = div().flex().gap_2().flex_wrap();
    for (hours, (en, no)) in RANGES {
        let target = this.clone();
        ranges = ranges.child(button(
            format!("history-{hours}"),
            l(en, no),
            view.history_request.hours == hours,
            dark,
            move |_, cx| change_history(&target, cx, |r| r.hours = hours),
        ));
    }
    let refresh = this.clone();
    ranges = ranges.child(button(
        "refresh-history",
        l("Refresh", "Oppdater"),
        false,
        dark,
        move |_, cx| refresh_history(&refresh, cx),
    ));
    let mut metrics = div().flex().gap_2().flex_wrap();
    for &metric in if app {
        &Metric::APP[..]
    } else {
        &Metric::ALL[..]
    } {
        let target = this.clone();
        metrics = metrics.child(button(
            format!("history-{metric:?}"),
            metric.label(app),
            view.history_request.metric == metric,
            dark,
            move |_, cx| change_history(&target, cx, |r| r.metric = metric),
        ));
    }
    let picker = this.clone();
    let mut panel = div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_size(px(24.))
                .font_weight(FontWeight::BOLD)
                .child(l("History", "Historikk")),
        )
        .child(
            card(dark)
                .child(ranges)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(sub(l("Source", "Kilde")))
                        .child(button(
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
                            move |_, cx| {
                                picker.update(cx, |v, cx| {
                                    v.history_app_picker = !v.history_app_picker;
                                    cx.notify();
                                });
                            },
                        )),
                )
                .child(metrics),
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
                        button(
                            format!("history-app-{i}"),
                            name.as_deref().unwrap_or(l("System", "System")).to_owned(),
                            view.history_request.app == name,
                            dark,
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
        panel = panel
            .child(card(dark).child(sub(l("Loading local history…", "Laster lokal historikk…"))));
    } else if let Some(Ok(data)) = &view.history_data {
        let metric = view.history_request.metric;
        let values: Vec<f64> = data.values.iter().flatten().copied().collect();
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
            unit => format!("{v:.1} {unit}"),
        };
        let color = match metric {
            Metric::Cpu => t::Metric::Cpu,
            Metric::Memory => t::Metric::Memory,
            Metric::Gpu => t::Metric::Gpu,
            Metric::Battery => t::Metric::Battery,
            Metric::DiskRead | Metric::DiskWrite => t::Metric::Disk,
            _ => t::Metric::Network,
        }
        .accent(dark);
        let mut plot = card(dark)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} · {}", metric.label(app), metric.unit(app))),
                    )
                    .child(sub(format!("{} {}", samples, l("samples", "målinger")))),
            )
            .child(sub(format_value(max)))
            .child(history_chart(&data.values, max, color))
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
        if values.is_empty() {
            plot = plot.child(sub(l(
                "No recorded measurements in this range.",
                "Ingen registrerte målinger i dette tidsrommet.",
            )));
        }
        panel = panel.child(plot);
    } else if let Some(Err(error)) = &view.history_data {
        panel = panel.child(card(dark).child(sub(error.clone()).text_color(t::warn(dark))));
    }
    panel.child(sub(l("Local history · 30-day retention target, pruned hourly. Bars show means of recorded samples; blank intervals are missing, not zero. Refresh to include new samples.", "Lokal historikk · mål om 30 dagers lagring, ryddes hver time. Stolper viser gjennomsnitt av registrerte målinger; tomme intervaller er manglende data, ikke null. Oppdater for å inkludere nye målinger.")))
        .child(sub(l("Only the top 60 app groups are stored per sample, by recorded name. Missing app rows do not prove inactivity. App CPU can exceed 100% across cores. Legacy GPU/battery zero placeholders are omitted; historical power estimates are not shown as measurements.", "Kun de 60 øverste appgruppene lagres per måling, etter registrert navn. Manglende apprader betyr ikke nødvendigvis inaktivitet. App-CPU kan overstige 100% på flere kjerner. Eldre nullverdier for GPU/batteri utelates; historiske effektestimater vises ikke som målinger.")))
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
                let Some(value) = value else { continue };
                let left = ox + index as f32 * step;
                let right = left + (step - 1.).max(1.);
                let bottom = oy + height;
                let top = bottom - ((*value / max).clamp(0., 1.) as f32 * (height - 2.)).max(1.);
                let mut bar = Path::new(point(px(left), px(bottom)));
                bar.line_to(point(px(left), px(top)));
                bar.line_to(point(px(right), px(top)));
                bar.line_to(point(px(right), px(bottom)));
                bar.line_to(point(px(left), px(bottom)));
                window.paint_path(bar, color.opacity(0.8));
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
    let mut buttons = div().flex().gap_2();
    if audio.can_volume {
        if let Some(_volume) = audio.device_volume {
            for (key, label, delta) in [("down", "− 10%", -0.1), ("up", "+ 10%", 0.1)] {
                let target = this.clone();
                buttons = buttons.child(button(
                    format!("volume-{key}"),
                    label,
                    false,
                    dark,
                    move |_, cx| {
                        audio_action(&target, cx, move || {
                            crate::audio::adjust_output_volume(id, delta)
                        });
                    },
                ));
            }
        }
    }
    if audio.can_mute {
        if let Some(muted) = audio.device_muted {
            buttons = buttons.child(button(
                "output-mute",
                if muted {
                    l("Unmute", "Slå på lyden")
                } else {
                    l("Mute", "Demp")
                },
                muted,
                dark,
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
