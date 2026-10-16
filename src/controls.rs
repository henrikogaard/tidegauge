//! Dashboard controls share persistence and keyboard behavior with Settings.

use crate::locale::text as l;
use crate::theme as t;
use crate::ui::{
    card, empty_state, section_header, sub, TideGaugeView,
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

/// An on/off switch: a 34×20 track whose knob slides right when on. The
/// track takes the accent colour when on; Enter or Space flips it.
pub(crate) fn switch(
    id: impl Into<String>,
    aria: impl Into<String>,
    on: bool,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let action = std::rc::Rc::new(action);
    let keyboard = action.clone();
    let accent = t::switch_on(dark);
    div()
        .id(SharedString::from(id.into()))
        .role(Role::Switch)
        .aria_label(SharedString::from(aria.into()))
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .tab_index(0)
        .flex_shrink_0()
        .w(px(34.))
        .h(px(20.))
        .p_0p5()
        .rounded_full()
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .bg(if on { accent } else { t::card_hi(dark) })
        .focus_visible(move |d| d.border_color(t::accent(dark)))
        .cursor_pointer()
        .flex()
        .items_center()
        .when(on, |d| d.justify_end())
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(gpui_kit::white())
                .shadow_sm(),
        )
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

/// A quiet, unframed action in the warning colour, for the harsher of two
/// choices (Force Quit beside Quit).
pub fn button_destructive(
    id: impl Into<String>,
    label: impl Into<String>,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label: String = label.into();
    let mut s = spec(id, label.clone(), false, dark, t::warn(dark));
    s.framed = false;
    control_with(s, div().text_color(t::warn(dark)).child(label), action)
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

/// An icon-only control for the page header: framed, with the selected tint
/// while what it opens is showing.
pub(crate) fn icon_toggle(
    id: impl Into<String>,
    icon: &'static str,
    aria: impl Into<String>,
    selected: bool,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let color = if selected { t::accent(dark) } else { t::ink_dim(dark) };
    let content = svg().path(icon).size(px(14.)).text_color(color);
    let mut s = spec(id, aria, selected, dark, t::accent(dark));
    s.role = Role::Button;
    control_with(s, content, action)
}

/// While Keep awake is on, Overview says so in one line with a way to stop.
pub fn keep_awake(dark: bool, this: Entity<TideGaugeView>) -> Div {
    let status = crate::keepawake::status();
    let title = match status.remaining {
        Some(remaining) => format!(
            "{} · {}:{:02}",
            l("Keeping awake", "Holder våken"),
            remaining.as_secs() / 60,
            remaining.as_secs() % 60
        ),
        None => l("Keeping awake · until stopped", "Holder våken · frem til stopp").into(),
    };
    card(dark)
        .flex_row()
        .items_center()
        .justify_between()
        .py(t::SPACE_2)
        .child(
            div()
                .flex()
                .items_center()
                .gap(t::SPACE_2)
                .child(svg().path("icons/coffee.svg").size(px(14.)).text_color(t::ink_dim(dark)))
                .child(div().text_size(t::TEXT_ROW).child(title)),
        )
        .child(button("awake-stop", l("Stop", "Stopp"), false, dark, move |_, cx| {
            let result = crate::keepawake::stop();
            this.update(cx, |v, cx| {
                v.settings_error = result.err();
                cx.notify();
            });
        }))
}

pub fn refresh_history(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.history_request.end = crate::metrics::now_unix();
        view.history_generation += 1;
        view.history_loading = true;
        view.history_loaded_at = Some(std::time::Instant::now());
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

/// Saves what History shows, every sample rather than the chart's averages,
/// as a CSV file in Downloads and shows it in Finder.
fn export_history(this: Entity<TideGaugeView>, cx: &mut App) {
    let request = this.read(cx).history_request.clone();
    cx.spawn(async move |cx| {
        let result = smol::unblock(move || crate::history_browser::export(&request)).await;
        if let Ok(path) = &result {
            crate::storage::reveal(path);
        }
        this.update(cx, |v, cx| {
            v.toast = Some(crate::ui::Toast::new(
                match &result {
                    Ok(path) => format!(
                        "{} {}",
                        l("Saved in Downloads:", "Lagret i Nedlastinger:"),
                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    ),
                    Err(error) => error.clone(),
                },
                result.is_err(),
            ));
            cx.notify();
        });
    })
    .detach();
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
        Metric::ChipTemp | Metric::SsdTemp | Metric::BatteryTemp => t::Metric::Cpu,
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
    // Temperatures get their own row, so neither row is wider than the
    // narrowest window allows.
    let temperature = |m: &Metric| matches!(m, Metric::ChipTemp | Metric::SsdTemp | Metric::BatteryTemp);
    let mut metrics = segment_group("history-metrics", l("Metric", "Måling"), dark);
    let mut temperatures = segment_group("history-temperatures", l("Temperature", "Temperatur"), dark);
    for &metric in if app {
        &Metric::APP[..]
    } else {
        &Metric::ALL[..]
    } {
        let target = this.clone();
        let group = if temperature(&metric) { &mut temperatures } else { &mut metrics };
        *group = std::mem::replace(group, segment_group("", "", dark)).child(segment(
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
        .child(
            card(dark)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(row_label(l("Range", "Tidsrom")))
                        .child(ranges),
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
                )
                .when(!app, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(row_label(l("Temperature", "Temperatur")))
                            .child(temperatures),
                    )
                }),
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
                .child(history_chart(&data.values, max, accent, 180.))
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
            // When recording began inside the range, say when: that, not a
            // fault, is why the left of the plot is empty.
            if let Some(at) = data.recording_began {
                plot = plot.child(
                    sub(format!("{} {}", l("Recorded since", "Registrert siden"), crate::history_browser::local_time(at)))
                        .text_color(t::ink_dim(dark)),
                );
            }
            let target = this.clone();
            panel = panel
                .child(section_header(metric.label(app), metric.unit(app), dark))
                .child(plot)
                .child(div().flex().child(button(
                    "history-export",
                    l("Export CSV", "Eksporter CSV"),
                    false,
                    dark,
                    move |_, cx| export_history(target.clone(), cx),
                )));
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
    panel
}

pub(crate) fn history_chart(values: &[Option<f64>], max: f64, color: Hsla, height: f32) -> impl IntoElement {
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
    .h(px(height))
}

pub fn audio_output(
    audio: &crate::engine::AudioInfo,
    volume_slider: &Entity<gpui_kit::component::slider::SliderState>,
    dark: bool,
    this: Entity<TideGaugeView>,
    pending: bool,
) -> Div {
    let id = audio.output_id;
    let mut controls = div().flex().flex_col().gap_2();
    if pending {
        return controls.child(sub(l("Updating output…", "Oppdaterer lydutgang…")));
    }
    let accent = t::Metric::Sound.accent(dark);
    let mut buttons = div().flex().items_center().gap(t::SPACE_4);
    if audio.can_volume {
        if let Some(volume) = audio.device_volume {
            buttons = buttons
                .child(svg().path("icons/volume-1.svg").size(px(14.)).text_color(t::ink_dim(dark)))
                .child(crate::settings_page::slider(volume_slider, 220., accent))
                .child(
                    div()
                        .w(px(44.))
                        .text_size(t::TEXT_ROW)
                        .font_weight(FontWeight::MEDIUM)
                        .child(crate::engine::fmt_pct(volume as f64 * 100.)),
                );
        }
    }
    if audio.can_mute {
        if let Some(muted) = audio.device_muted {
            let this = this.clone();
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
    if audio.outputs.len() > 1 {
        controls = controls.child(device_choice("output", &audio.outputs, id, crate::audio::Direction::Output, dark, accent, this.clone()));
    }
    if !audio.can_volume || !audio.can_mute {
        controls = controls.child(sub(l("This output does not expose all volume/mute controls. Use macOS Sound settings for unsupported controls.", "Denne utgangen støtter ikke alle volum-/dempefunksjoner. Bruk lydinnstillingene i macOS for funksjoner som ikke støttes.")));
    }
    controls
}

/// One button per device; choosing one makes it the default on its side.
fn device_choice(
    key: &'static str,
    devices: &[(u32, String)],
    current: u32,
    direction: crate::audio::Direction,
    dark: bool,
    accent: Hsla,
    this: Entity<TideGaugeView>,
) -> Div {
    div().flex().flex_wrap().gap_2().children(devices.iter().map(|(id, name)| {
        let (id, target) = (*id, this.clone());
        button_accent(format!("{key}-device-{id}"), name.clone(), id == current, dark, accent, move |_, cx| {
            if id != current {
                audio_action(&target, cx, move || crate::audio::set_default_device(direction, id));
            }
        })
    }))
}

/// The input device (microphone): its name, volume, mute and the choice of
/// device. Controls a device lacks are left out and said so.
pub fn audio_input(
    audio: &crate::engine::AudioInfo,
    input_slider: &Entity<gpui_kit::component::slider::SliderState>,
    dark: bool,
    this: Entity<TideGaugeView>,
    pending: bool,
) -> Div {
    use crate::audio::Direction;
    let input = &audio.input;
    let accent = t::Metric::Sound.accent(dark);
    let id = input.id;
    let mut card = crate::ui::card(dark).child(
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(svg().path("icons/mic.svg").size(px(20.)).text_color(accent))
            .child(
                div()
                    .flex_1()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("Input device", "Lydinngang")))
                    .child(sub(input.name.clone()).text_color(t::ink_dim(dark))),
            )
            .when(input.muted == Some(true), |d| d.child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(l("Muted", "Dempet")))),
    );
    if id == 0 {
        return card;
    }
    if pending {
        return card.child(sub(l("Updating input…", "Oppdaterer lydinngang…")));
    }
    let mut row = div().flex().items_center().gap(t::SPACE_4);
    if let (true, Some(volume)) = (input.can_volume, input.volume) {
        row = row
            .child(svg().path("icons/mic.svg").size(px(14.)).text_color(t::ink_dim(dark)))
            .child(crate::settings_page::slider(input_slider, 220., accent))
            .child(div().w(px(44.)).text_size(t::TEXT_ROW).font_weight(FontWeight::MEDIUM).child(crate::engine::fmt_pct(volume as f64 * 100.)));
    }
    if let (true, Some(muted)) = (input.can_mute, input.muted) {
        let target = this.clone();
        row = row.child(button_accent("input-mute", if muted { l("Unmute", "Slå på lyden") } else { l("Mute", "Demp") }, muted, dark, accent, move |_, cx| {
            audio_action(&target, cx, move || crate::audio::toggle_mute(Direction::Input, id));
        }));
    }
    card = card.child(row);
    if audio.inputs.len() > 1 {
        card = card.child(device_choice("input", &audio.inputs, id, Direction::Input, dark, accent, this));
    }
    if !input.can_volume || !input.can_mute {
        card = card.child(sub(l("This input does not expose all volume/mute controls. Use macOS Sound settings for unsupported controls.", "Denne inngangen støtter ikke alle volum-/dempefunksjoner. Bruk lydinnstillingene i macOS for funksjoner som ikke støttes.")).text_color(t::ink_dim(dark)));
    }
    card
}

/// Per-app volume: mute, a step down, the level and a step up, plus a meter
/// of the app's own sound while Tidegauge carries it.
pub fn app_volume_controls(
    app: &crate::engine::AudioApp,
    level: Option<crate::app_volume::Level>,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> Div {
    let accent = t::Metric::Sound.accent(dark);
    let volume = level.map(|l| l.volume).unwrap_or(1.0);
    let object = app.object;
    // Steps read the volume when clicked, so quick clicks each count.
    let step = move |target: Entity<TideGaugeView>, delta: f32| {
        move |_: &mut Window, cx: &mut App| {
            app_volume_action(&target, cx, move || {
                let now = crate::app_volume::levels().get(&object).map(|l| l.volume).unwrap_or(1.0);
                crate::app_volume::set_volume(object, ((now + delta).clamp(0., 1.) * 10.).round() / 10.)
            });
        }
    };
    let set = move |target: Entity<TideGaugeView>, value: f32| {
        move |_: &mut Window, cx: &mut App| {
            app_volume_action(&target, cx, move || crate::app_volume::set_volume(object, value));
        }
    };
    let muted = volume <= 0.;
    let mut row = div().flex().items_center().gap_2();
    if let Some(level) = level.filter(|l| l.live) {
        row = row.child(
            div()
                .w(px(48.))
                .h(px(4.))
                .rounded(t::RADIUS_CHIP)
                .bg(t::card_hi(dark))
                .child(div().h_full().rounded(t::RADIUS_CHIP).bg(accent).w(relative(level.peak.clamp(0., 1.)))),
        );
    }
    row.child(button_accent(format!("app-mute-{object}"), if muted { l("Unmute", "Slå på lyden") } else { l("Mute", "Demp") }, muted, dark, accent, set(this.clone(), if muted { 1.0 } else { 0.0 })))
        .child(button(format!("app-down-{object}"), "−", false, dark, step(this.clone(), -0.1)))
        .child(div().w(px(40.)).flex().justify_center().text_size(t::TEXT_ROW).font_weight(FontWeight::MEDIUM).child(crate::engine::fmt_pct(volume as f64 * 100.)))
        .child(button(format!("app-up-{object}"), "+", false, dark, step(this, 0.1)))
}

fn app_volume_action(this: &Entity<TideGaugeView>, cx: &mut App, action: impl FnOnce() -> Result<(), String> + Send + 'static) {
    this.update(cx, |view, cx| {
        view.audio_error = None;
        cx.spawn(async move |this, cx| {
            let (result, levels) = smol::unblock(move || (action(), crate::app_volume::levels())).await;
            let _ = this.update(cx, |view, cx| {
                view.audio_error = result.err();
                view.snap.audio.levels = levels;
                cx.notify();
            });
        })
        .detach();
    });
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
            let (result, output, input, outputs, inputs) = smol::unblock(move || {
                use crate::audio::Direction;
                (action(), crate::audio::output_state(), crate::audio::device_state(Direction::Input), crate::audio::devices(Direction::Output), crate::audio::devices(Direction::Input))
            })
            .await;
            let _ = this.update(cx, |view, cx| {
                view.audio_pending = false;
                view.audio_error = result.err();
                view.snap.audio.output_id = output.id;
                view.snap.audio.device_name = output.name;
                view.snap.audio.device_volume = output.volume;
                view.snap.audio.device_muted = output.muted;
                view.snap.audio.can_volume = output.can_volume;
                view.snap.audio.can_mute = output.can_mute;
                view.snap.audio.input = input;
                view.snap.audio.outputs = outputs;
                view.snap.audio.inputs = inputs;
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
