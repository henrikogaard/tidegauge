//! Settings as a grouped list: a name on the left, one control on the right,
//! and at most one quiet line under the name. Explanations live behind the
//! header's ⓘ.

use crate::controls::{button_indexed, segment, segment_group, switch};
use crate::locale::text as l;
use crate::settings::{
    localized_metric_label, localized_style_label, Settings, MENU_METRICS, MENU_STYLES,
};
use crate::theme as t;
use crate::ui::{caption, card, update_preference, Tab, TideGaugeView};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Slider state the settings and sound pages keep between renders.
pub struct Sliders {
    pub cpu: Entity<SliderState>,
    pub mem: Entity<SliderState>,
    /// One app's CPU, which can pass 100 % across cores.
    pub app: Entity<SliderState>,
    pub volume: Entity<SliderState>,
    /// When the volume was last dragged; the device's reading lags, so the
    /// slider isn't pulled back to it for a moment.
    pub volume_touched: Option<std::time::Instant>,
}

impl Sliders {
    pub fn new(cx: &mut Context<TideGaugeView>) -> Self {
        let settings = Settings::load();
        let threshold = |value: f64| SliderState::new().min(10.).max(100.).step(5.).default_value(value as f32);
        let cpu = cx.new(|_| threshold(settings.alert_cpu_pct));
        let mem = cx.new(|_| threshold(settings.alert_mem_pct));
        let (low, high) = crate::settings::APP_CPU_RANGE;
        let app = cx.new(|_| SliderState::new().min(low as f32).max(high as f32).step(10.).default_value(settings.alert_app_cpu_pct as f32));
        let volume = cx.new(|_| SliderState::new().min(0.).max(100.).step(1.).default_value(50.));
        let setters: [(&Entity<SliderState>, fn(&mut Settings, f64)); 3] = [
            (&cpu, |s, v| s.alert_cpu_pct = v),
            (&mem, |s, v| s.alert_mem_pct = v),
            (&app, |s, v| s.alert_app_cpu_pct = v),
        ];
        for (state, set) in setters {
            cx.subscribe(state, move |view: &mut TideGaugeView, _, event: &SliderEvent, cx| {
                let value = match event {
                    SliderEvent::Change(value) | SliderEvent::Release(value) => value.end() as f64,
                };
                let saved = crate::settings::mutate(|s| set(s, value));
                view.settings_error = saved.err().map(|_| {
                    l("Settings could not be saved. Try again.", "Innstillingene kunne ikke lagres. Prøv igjen.").to_string()
                });
                cx.notify();
            })
            .detach();
        }
        cx.subscribe(&volume, |view: &mut TideGaugeView, _, event: &SliderEvent, cx| {
            let value = match event {
                SliderEvent::Change(value) | SliderEvent::Release(value) => value.end(),
            };
            view.sliders.volume_touched = Some(std::time::Instant::now());
            let id = view.snap.audio.output_id;
            view.audio_error = crate::audio::set_output_volume(id, value / 100.).err();
            view.snap.audio.device_volume = Some(value / 100.);
            cx.notify();
        })
        .detach();
        Self { cpu, mem, app, volume, volume_touched: None }
    }

    /// Bring the sliders back in line with what is saved or reported, unless
    /// the person is moving them right now.
    pub fn sync(&self, view_volume: Option<f32>, window: &mut Window, cx: &mut App) {
        let settings = Settings::load();
        for (state, value) in [
            (&self.cpu, settings.alert_cpu_pct as f32),
            (&self.mem, settings.alert_mem_pct as f32),
            (&self.app, settings.alert_app_cpu_pct as f32),
        ] {
            if (state.read(cx).value().end() - value).abs() > 0.5 {
                state.update(cx, |s, cx| s.set_value(value, window, cx));
            }
        }
        let settling = self.volume_touched.is_some_and(|at| at.elapsed().as_millis() < 1500);
        if let (Some(volume), false) = (view_volume, settling) {
            let value = volume * 100.;
            if (self.volume.read(cx).value().end() - value).abs() > 0.5 {
                self.volume.update(cx, |s, cx| s.set_value(value, window, cx));
            }
        }
    }
}

/// A slider drawn in Tidegauge's colours, `width` wide.
pub fn slider(state: &Entity<SliderState>, width: f32, color: Hsla) -> impl IntoElement {
    div().w(px(width)).flex_shrink_0().child(Slider::new(state).bg(color).text_color(gpui_kit::white()))
}

/// One settings row: the name and an optional quiet line on the left, the
/// control on the right.
fn row(name: impl Into<SharedString>, detail: Option<String>, control: impl IntoElement, first: bool, dark: bool) -> Div {
    div()
        .min_h(t::ROW_H)
        .px(t::SPACE_4)
        .py(t::SPACE_2)
        .flex()
        .items_center()
        .gap(t::SPACE_4)
        .when(!first, |d| d.border_t_1().border_color(t::line(dark)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(t::TEXT_ROW).child(name.into()))
                .when_some(detail, |d, detail| {
                    d.child(div().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).child(detail))
                }),
        )
        .child(div().flex_shrink_0().flex().items_center().gap(t::SPACE_3).child(control))
}

/// A titled group of rows, like a section of macOS System Settings.
fn group(title: &str, rows: Vec<Div>, dark: bool) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(t::SPACE_2)
        .child(div().px(t::SPACE_4).child(caption(title, dark)))
        .child(card(dark).p_0().gap_0().children(rows))
}

fn choice(
    id: String,
    label: &'static str,
    aria: String,
    active: bool,
    dark: bool,
    this: Entity<TideGaugeView>,
    change: impl Fn(&mut Settings) + 'static,
) -> impl IntoElement {
    crate::controls::control(
        crate::controls::ControlSpec {
            id,
            label: label.to_string(),
            aria: Some(aria),
            role: Role::RadioButton,
            tab_index: 0,
            selected: active,
            accent: t::accent(dark),
            dark,
            framed: false,
        },
        move |window, cx| update_preference(&this, |settings| change(settings), window, cx),
    )
}

fn preference_switch(
    id: String,
    aria: String,
    on: bool,
    dark: bool,
    this: Entity<TideGaugeView>,
    change: impl Fn(&mut Settings, bool) + 'static,
) -> impl IntoElement {
    switch(id, aria, on, dark, move |window, cx| {
        update_preference(&this, |settings| change(settings, !on), window, cx)
    })
}

/// Save the language and switch to it at once: the next frame, the filter
/// fields and the menu-bar menu all use it.
fn set_language(this: &Entity<TideGaugeView>, value: Option<&'static str>, window: &mut Window, cx: &mut App) {
    let saved = crate::settings::mutate(|s| s.language = value.map(str::to_string));
    crate::locale::set_language(value);
    this.update(cx, |view, cx| {
        view.settings_error = saved.err().map(|_| {
            l("Settings could not be saved. Try again.", "Innstillingene kunne ikke lagres. Prøv igjen.").to_string()
        });
        view.process_filter.update(cx, |input, cx| {
            input.set_placeholder(l("Filter by name or PID", "Filtrer på navn eller PID"), window, cx)
        });
        view.startup_filter.update(cx, |input, cx| {
            input.set_placeholder(l("Filter by app or label", "Filtrer på app eller etikett"), window, cx)
        });
        cx.notify();
    });
}

fn set_launch_at_login(enabled: bool, this: &Entity<TideGaugeView>, cx: &mut App) {
    let error = match crate::login::set_enabled(enabled) {
        Ok(status) => crate::settings::mutate(|settings| settings.launch_at_login = status.is_registered())
            .err()
            .map(|_| {
                l(
                    "The login setting changed, but its local preference could not be saved.",
                    "Påloggingsinnstillingen ble endret, men kunne ikke lagres lokalt.",
                )
                .to_string()
            }),
        Err(error) => Some(error.to_string()),
    };
    this.update(cx, |view, cx| {
        view.settings_error = error;
        cx.notify();
    });
}

fn general(view: &TideGaugeView, settings: &Settings, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let appearance = segment_group("appearance-options", l("Appearance", "Utseende"), dark).children(
        [(None, l("System", "System")), (Some(false), l("Light", "Lyst")), (Some(true), l("Dark", "Mørkt"))]
            .into_iter()
            .enumerate()
            .map(|(i, (value, label))| {
                choice(
                    format!("appearance-{i}"),
                    label,
                    format!("{}: {label}", l("Appearance", "Utseende")),
                    settings.dark == value,
                    dark,
                    this.clone(),
                    move |settings| settings.dark = value,
                )
            }),
    );
    let mut language = segment_group("language-options", l("Language", "Språk"), dark);
    for (i, (value, label)) in [(None, l("System", "System")), (Some("en"), l("English", "English")), (Some("nb"), l("Norsk", "Norsk"))]
        .into_iter()
        .enumerate()
    {
        let target = this.clone();
        language = language.child(segment(
            format!("language-{i}"),
            label,
            settings.language.as_deref() == value,
            dark,
            t::accent(dark),
            move |window, cx| set_language(&target, value, window, cx),
        ));
    }
    let login = crate::login::status();
    let login_control: AnyElement = if login.can_change() {
        let on = login.is_registered();
        let target = this.clone();
        let aria = format!("{}: {}", l("Launch at login", "Start ved pålogging"), login.control_label());
        switch("launch-at-login", aria, on, dark, move |_, cx| {
            set_launch_at_login(!on, &target, cx)
        })
        .into_any_element()
    } else {
        div()
            .text_size(t::TEXT_BODY)
            .text_color(t::ink_dim(dark))
            .child(l("Unavailable", "Utilgjengelig"))
            .into_any_element()
    };
    group(
        l("General", "Generelt"),
        vec![
            row(l("Appearance", "Utseende"), None, appearance, true, dark),
            row(l("Language", "Språk"), None, language, false, dark),
            row(
                l("Launch at login", "Start ved pålogging"),
                (!login.can_change() || login.needs_approval()).then(|| login.message().to_string()),
                login_control,
                false,
                dark,
            ),
            keep_awake_row(view, dark, this),
            updates_row(dark),
        ],
        dark,
    )
}

/// The installed version and a way to look for a newer one. Sparkle also
/// checks on its own; the reason shows when updates are off for this copy.
fn updates_row(dark: bool) -> Div {
    let version = crate::updater::app_version()
        .map(|v| format!("{} {v}", l("Version", "Versjon")))
        .unwrap_or_else(|| l("Version unknown", "Ukjent versjon").into());
    let (detail, available) = match crate::updater::availability() {
        crate::updater::Availability::Available => (
            format!("{version} · {}", l("checks for updates automatically", "ser etter oppdateringer automatisk")),
            true,
        ),
        crate::updater::Availability::Disabled { reason } | crate::updater::Availability::Unavailable { reason } => {
            (format!("{version} · {reason}"), false)
        }
    };
    let control: AnyElement = if available {
        button_indexed("check-updates", crate::updater::check_for_updates_title(), false, dark, 0, |_, _| {
            crate::updater::ask_for_check()
        })
        .into_any_element()
    } else {
        div().into_any_element()
    };
    row(l("Updates", "Oppdateringer"), Some(detail), control, false, dark)
}

/// Keep awake: a switch, and how long it lasts once on.
fn keep_awake_row(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let status = crate::keepawake::status();
    let detail = if status.active {
        Some(match status.remaining {
            Some(remaining) => format!(
                "{} {}:{:02}",
                l("On · ends in", "På · slutter om"),
                remaining.as_secs() / 60,
                remaining.as_secs() % 60
            ),
            None => l("On until you turn it off", "På til du slår den av").into(),
        })
    } else {
        Some(l("Stops the Mac from going to sleep on its own.", "Hindrer at Macen går i dvale av seg selv.").into())
    };
    let start = |minutes: Option<u64>| {
        crate::keepawake::start(minutes.map(|m| std::time::Duration::from_secs(m * 60)))
    };
    let mut durations = segment_group("awake-durations", l("Keep awake for", "Hold våken i"), dark);
    for (minutes, label) in [
        (Some(15), l("15 min", "15 min")),
        (Some(30), l("30 min", "30 min")),
        (Some(60), l("1 hour", "1 time")),
        (None, l("Until stopped", "Frem til stopp")),
    ] {
        let target = this.clone();
        durations = durations.child(segment(
            format!("awake-{minutes:?}"),
            label,
            view.awake_choice == minutes,
            dark,
            t::accent(dark),
            move |_, cx| {
                let active = crate::keepawake::status().active;
                let result = if active { start(minutes) } else { Ok(()) };
                target.update(cx, |v, cx| {
                    v.awake_choice = minutes;
                    v.settings_error = result.err();
                    cx.notify();
                });
            },
        ));
    }
    let on = status.active;
    let choice = view.awake_choice;
    let toggle = switch("keep-awake", l("Keep awake", "Hold våken"), on, dark, move |_, cx| {
        let result = if on { crate::keepawake::stop() } else { start(choice) };
        this.update(cx, |v, cx| {
            v.settings_error = result.err();
            cx.notify();
        });
    });
    row(
        l("Keep awake", "Hold våken"),
        detail,
        div().flex().items_center().gap(t::SPACE_3).child(durations).child(toggle),
        false,
        dark,
    )
    .when_some(status.error, |d, error| d.child(div().text_size(t::TEXT_SMALL).text_color(t::warn(dark)).child(error)))
}

fn alerts(view: &TideGaugeView, settings: &Settings, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let pct = crate::engine::fmt_pct;
    let rows = vec![
        (
            "cpu",
            "CPU",
            format!("{} {} {}", l("Above", "Over"), pct(settings.alert_cpu_pct), l("for 20 seconds", "i 20 sekunder")),
            Some(&view.sliders.cpu),
            settings.alert_cpu_enabled,
        ),
        (
            "mem",
            l("Memory in use", "Minne i bruk"),
            format!("{} {} {}", l("Above", "Over"), pct(settings.alert_mem_pct), l("for 20 seconds", "i 20 sekunder")),
            Some(&view.sliders.mem),
            settings.alert_mem_enabled,
        ),
        ("disk", "Disk", l("Above 400 MB/s for 30 seconds", "Over 400 MB/s i 30 sekunder").to_string(), None, settings.alert_disk_enabled),
        (
            "net",
            l("Network", "Nettverk"),
            l("Above 80 MB/s for 30 seconds", "Over 80 MB/s i 30 sekunder").to_string(),
            None,
            settings.alert_network_enabled,
        ),
        (
            "app",
            l("Any one app", "Én enkelt app"),
            format!("{} {} {}", l("CPU above", "CPU over"), pct(settings.alert_app_cpu_pct), l("for 5 minutes", "i 5 minutter")),
            Some(&view.sliders.app),
            settings.alert_app_enabled,
        ),
    ];
    group(
        l("Alerts", "Varsler"),
        rows.into_iter()
            .enumerate()
            .map(|(i, (key, name, detail, threshold, on))| {
                let control = div()
                    .flex()
                    .items_center()
                    .gap(t::SPACE_4)
                    .when_some(threshold, |d, state| {
                        let metric = if key == "mem" { t::Metric::Memory } else { t::Metric::Cpu };
                        d.child(slider(state, 140., metric.accent(dark)))
                    })
                    .child(preference_switch(
                        format!("alert-{key}"),
                        format!("{} {name}", l("Alert for", "Varsel for")),
                        on,
                        dark,
                        this.clone(),
                        move |s, value| match key {
                            "cpu" => s.alert_cpu_enabled = value,
                            "mem" => s.alert_mem_enabled = value,
                            "disk" => s.alert_disk_enabled = value,
                            "app" => s.alert_app_enabled = value,
                            _ => s.alert_network_enabled = value,
                        },
                    ));
                row(name, Some(detail), control, i == 0, dark)
            })
            .collect(),
        dark,
    )
}

fn menu_bar(settings: &Settings, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let icon_name = l("Tidegauge icon", "Tidegauge-ikon");
    let mut rows = vec![row(
        icon_name,
        Some(l("Shown even when every reading is off.", "Vises også når alle målinger er av.").into()),
        preference_switch("launcher".into(), icon_name.into(), settings.show_app_icon, dark, this.clone(), |s, on| {
            s.show_app_icon = on
        }),
        true,
        dark,
    )];
    for &(key, _) in MENU_METRICS.iter() {
        let name = localized_metric_label(key).expect("menu metric has a localized label");
        let selected = settings
            .menu_items
            .iter()
            .find(|item| item.metric == key)
            .map(|item| item.style.as_str())
            .unwrap_or("off");
        let choices = std::iter::once(("off", l("Off", "Av"))).chain(
            MENU_STYLES
                .iter()
                .map(|(style, _)| (*style, localized_style_label(style).expect("menu style has a localized label"))),
        );
        let group = segment_group(format!("settings-{key}"), name, dark).children(choices.enumerate().map(
            |(i, (style, label))| {
                choice(
                    format!("{key}-{style}-{i}"),
                    label,
                    format!("{name}: {label}"),
                    selected == style,
                    dark,
                    this.clone(),
                    move |settings| settings.set_menu_style(key, if style == "off" { None } else { Some(style) }),
                )
            },
        ));
        rows.push(row(name, None, group, false, dark));
    }
    let target = this.clone();
    rows.push(row(
        l("Readings", "Målinger"),
        None,
        button_indexed(
            "settings-icon-only",
            l("Reset to icon only", "Tilbakestill til bare ikon"),
            false,
            dark,
            0,
            move |window, cx| {
                update_preference(
                    &target,
                    |settings| {
                        settings.show_app_icon = true;
                        settings.menu_items.clear();
                    },
                    window,
                    cx,
                )
            },
        ),
        false,
        dark,
    ));
    group(l("Menu bar", "Menylinje"), rows, dark)
}

fn overview(settings: &Settings, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut rows = Vec::new();
    for (i, key) in crate::settings::FAVORITE_METRICS.iter().enumerate() {
        let enabled = settings.favorite_metrics.iter().any(|x| x == key);
        let label = Tab::from_key(key).unwrap().label();
        rows.push(row(
            label,
            None,
            preference_switch(format!("favorite-{key}"), format!("{} {label}", l("Show on Overview:", "Vis i oversikten:")), enabled, dark, this.clone(), move |s, on| {
                if !on {
                    s.favorite_metrics.retain(|x| x != key);
                } else if !s.favorite_metrics.iter().any(|x| x == key) {
                    s.favorite_metrics.push((*key).into());
                }
            }),
            i == 0,
            dark,
        ));
    }
    rows.push(row(
        l("Compact layout", "Kompakt oppsett"),
        Some(l("Smaller cards without the extra rows.", "Mindre kort uten de ekstra radene.").into()),
        preference_switch("compact-dashboard".into(), l("Compact layout", "Kompakt oppsett").into(), settings.compact_dashboard, dark, this.clone(), |s, on| {
            s.compact_dashboard = on
        }),
        false,
        dark,
    ));
    let target = this.clone();
    rows.push(row(
        l("Overview", "Oversikt"),
        None,
        button_indexed("reset-dashboard", l("Reset dashboard only", "Tilbakestill kun oversikten"), false, dark, 0, move |window, cx| {
            update_preference(&target, Settings::reset_dashboard, window, cx)
        }),
        false,
        dark,
    ));
    group(l("Overview", "Oversikt"), rows, dark)
}

/// The sidebar entries in their saved order, without Projects, which shows
/// inside Agents now. Positions and moves count only these.
fn sidebar_order(tabs: &[String]) -> Vec<String> {
    tabs.iter().filter(|key| *key != "projects").cloned().collect()
}

/// Move `key` one place up or down among the visible entries, swapping it
/// with its visible neighbour wherever that sits in the saved list.
fn move_sidebar_entry(tabs: &mut [String], key: &str, up: bool) {
    let order = sidebar_order(tabs);
    let Some(at) = order.iter().position(|x| x == key) else {
        return;
    };
    let Some(neighbour) = (if up { at.checked_sub(1) } else { (at + 1 < order.len()).then_some(at + 1) }) else {
        return;
    };
    let a = tabs.iter().position(|x| *x == order[at]);
    let b = tabs.iter().position(|x| *x == order[neighbour]);
    if let (Some(a), Some(b)) = (a, b) {
        tabs.swap(a, b);
    }
}

fn sidebar(settings: &Settings, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let shown = sidebar_order(&settings.dashboard_tabs);
    let mut order = shown.clone();
    for key in crate::settings::DASHBOARD_TABS {
        if key != "projects" && !order.iter().any(|x| x == key) {
            order.push(key.into());
        }
    }
    let enabled_count = shown.len();
    let mut rows = Vec::new();
    for (row_index, key) in order.into_iter().enumerate() {
        let Some(tab) = Tab::from_key(&key) else {
            continue;
        };
        let position = shown.iter().position(|x| x == &key);
        // Arrows sit in a fixed-width slot so the switches line up; the first
        // row has no up arrow and the last no down arrow.
        let mut arrows = div().w(px(64.)).flex_shrink_0().flex().justify_end().gap(t::SPACE_1);
        if let Some(i) = position {
            for (up, icon, word) in [(true, "icons/arrow-up.svg", l("up", "opp")), (false, "icons/arrow-down.svg", l("down", "ned"))] {
                if (up && i == 0) || (!up && i + 1 >= enabled_count) {
                    continue;
                }
                let move_key = key.clone();
                let target = this.clone();
                arrows = arrows.child(crate::controls::icon_toggle(
                    format!("{}-{key}", if up { "up" } else { "down" }),
                    icon,
                    format!("{} {} {word}", l("Move", "Flytt"), tab.label()),
                    false,
                    dark,
                    move |window, cx| {
                        let move_key = move_key.clone();
                        update_preference(&target, move |s| move_sidebar_entry(&mut s.dashboard_tabs, &move_key, up), window, cx)
                    },
                ));
            }
        }
        let toggle_key = key.clone();
        rows.push(row(
            tab.label(),
            None,
            div().flex().items_center().gap(t::SPACE_3).child(arrows).child(preference_switch(
                format!("show-{key}"),
                format!("{} {}", l("Show in sidebar:", "Vis i sidefeltet:"), tab.label()),
                position.is_some(),
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
            row_index == 0,
            dark,
        ));
    }
    group(l("Sidebar order", "Rekkefølge i sidefeltet"), rows, dark)
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let settings = Settings::load();
    div()
        .flex()
        .flex_col()
        .gap(t::SPACE_6)
        .when_some(view.settings_error.clone(), |d, error| {
            d.child(div().text_size(t::TEXT_BODY).text_color(t::warn(dark)).child(error))
        })
        .child(general(view, &settings, dark, this.clone()))
        .child(alerts(view, &settings, dark, this.clone()))
        .child(menu_bar(&settings, dark, this.clone()))
        .child(div().grid().grid_cols(2).gap(t::SPACE_4).items_start()
            .child(overview(&settings, dark, this.clone()))
            .child(sidebar(&settings, dark, this.clone())))
        .child(
            div()
                .flex()
                .justify_end()
                .gap(t::SPACE_3)
                .child(button_indexed("settings-app-menu", l("App menu", "Appmeny"), false, dark, 0, |_, _| {
                    crate::statusbar::SHOW_CONTEXT_MENU.store(true, std::sync::atomic::Ordering::SeqCst)
                }))
                .child(button_indexed("settings-quit", l("Quit Tidegauge", "Avslutt Tidegauge"), false, dark, 0, |_, cx| cx.quit())),
        )
}

#[cfg(test)]
mod tests {
    use super::move_sidebar_entry;

    #[test]
    fn moving_skips_the_hidden_projects_entry() {
        let mut tabs: Vec<String> = ["cpu", "projects", "history", "bluetooth"].map(String::from).to_vec();
        move_sidebar_entry(&mut tabs, "history", true);
        assert_eq!(tabs, ["history", "projects", "cpu", "bluetooth"], "history passes cpu, not projects");
        move_sidebar_entry(&mut tabs, "bluetooth", false);
        assert_eq!(tabs, ["history", "projects", "cpu", "bluetooth"], "the last visible entry stays last");
    }
}
