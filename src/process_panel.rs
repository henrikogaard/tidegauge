//! Apps & processes: every app with its helpers, sortable by CPU or memory,
//! with confirmed Quit and Force Quit for anything you own.

use crate::controls::{button, button_destructive, segment, segment_group};
use crate::engine::{fmt_bytes, Snapshot};
use crate::locale::text as l;
use crate::process_activity::Identity;
use crate::process_control::{self as control, Force};
use crate::process_signing::{Failure, Key};
use crate::processes::{AppGroup, ProcInfo};
use crate::theme::{self as t, Metric};
use crate::ui::{app_icon, card, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Apps shown before "Show all"; the busiest ones are always included.
const FIRST_PAGE: usize = 60;

pub fn open(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.process_order.clear();
        view.show_agents = false;
        view.show_processes = true;
        view.show_tools = false;
        view.show_settings = false;
        view.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    });
    if this.read(cx).tools.startup.is_none() {
        crate::tools_panel::refresh_startup(this, cx);
    }
}

fn sorted_keys(s: &Snapshot, sort_cpu: bool) -> Vec<String> {
    let mut apps: Vec<&AppGroup> = s.apps.iter().collect();
    if sort_cpu {
        apps.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct).then(b.mem_bytes.cmp(&a.mem_bytes)));
    } else {
        apps.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    }
    apps.into_iter().map(|app| app.key.clone()).collect()
}

/// Rows re-sort only when it is safe: never while the pointer is over the
/// list or an app is open, and at most every few seconds. A click then
/// always lands on the row that was under it, which matters next to Quit.
pub fn refresh_order(view: &mut TideGaugeView) {
    if !view.show_processes {
        return;
    }
    let stale = view
        .process_order_at
        .is_none_or(|at| at.elapsed() >= Duration::from_secs(4));
    let held = view.process_table_hovered || view.expanded_process_group.is_some();
    if view.process_order.is_empty() || (stale && !held) {
        view.process_order = sorted_keys(&view.snap, view.process_sort_cpu);
        view.process_order_at = Some(Instant::now());
    }
}

/// Whether the rows on screen are out of date because the order is being
/// held for a safe click. Only then is it worth saying.
fn order_held(view: &TideGaugeView) -> bool {
    (view.process_table_hovered || view.expanded_process_group.is_some())
        && !view.process_order.is_empty()
        && view.process_order != sorted_keys(&view.snap, view.process_sort_cpu)
}

fn held_chip(dark: bool) -> impl IntoElement {
    div()
        .id("order-held")
        .role(Role::Status)
        .aria_label(l(
            "Order held while the pointer is over the list",
            "Rekkefølgen holdes mens pekeren er over listen",
        ))
        .flex()
        .items_center()
        .gap_1()
        .h(px(22.))
        .px_2()
        .rounded(t::RADIUS_CHIP)
        .bg(t::card_hi(dark))
        .text_size(t::TEXT_SMALL)
        .text_color(t::ink_dim(dark))
        .child(svg().path("icons/pause.svg").size(px(11.)).text_color(t::ink_dim(dark)))
        .child(l("Order held", "Rekkefølge holdt"))
}

/// Quit is a framed button; Force Quit is the quieter red action beside it.
fn quit_button(
    id: String,
    label: impl Into<String>,
    force: Force,
    dark: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    match force {
        Force::Quit => button(id, label, false, dark, action).into_any_element(),
        Force::ForceQuit => button_destructive(id, label, dark, action).into_any_element(),
    }
}

fn remaining_pids(s: &Snapshot) -> HashSet<i32> {
    s.process_groups
        .iter()
        .flat_map(|g| g.rows.iter().filter(|r| r.remaining).map(|r| r.process.pid))
        .collect()
}

fn count_label(n: usize) -> String {
    crate::locale::count(n as u64, "process", "processes", "prosess", "prosesser")
}

fn mono(text: impl Into<String>, dark: bool) -> Div {
    div()
        .font_family("Menlo")
        .text_size(t::TEXT_CAPTION)
        .text_color(t::ink_dim(dark))
        .truncate()
        .child(text.into())
}

const COL_PROCS: f32 = 92.;
const COL_CPU: f32 = 72.;
const COL_MEM: f32 = 88.;

fn header_row(sort_cpu: bool, dark: bool) -> Div {
    let cell = |text: &str, width: f32, active: bool| {
        div()
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .justify_end()
            .text_color(if active { t::ink(dark) } else { t::ink_dim(dark) })
            .child(if active { format!("{text} ▾") } else { text.to_string() })
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(t::SPACE_2)
        .pb_2()
        .border_b_1()
        .border_color(t::line(dark))
        .text_size(t::TEXT_CAPTION)
        .font_weight(FontWeight::SEMIBOLD)
        .child(div().flex_1().text_color(t::ink_dim(dark)).child(l("App", "App").to_uppercase()))
        .child(cell(l("PROCESSES", "PROSESSER"), COL_PROCS, false))
        .child(cell("CPU", COL_CPU, sort_cpu))
        .child(cell(l("MEMORY", "MINNE"), COL_MEM, !sort_cpu))
        .child(div().w(px(16.)).flex_shrink_0())
}

/// The header line counts what is running; the notes explain Quit and the
/// held row order.
pub(crate) fn chrome(view: &TideGaugeView) -> crate::ui::PageChrome {
    let s = &view.snap;
    crate::ui::PageChrome {
        subtitle: Some(format!(
            "{} · {}",
            crate::locale::count(s.app_count as u64, "app", "apps", "app", "apper"),
            count_label(s.proc_count)
        )),
        notes: vec![
            l(
                "Quit asks the app to close. Force Quit stops it at once, and unsaved work is lost.",
                "Avslutt ber appen lukke seg. Tving avslutning stopper den med en gang, og ulagret arbeid går tapt.",
            ),
            l(
                "The list keeps its order while the pointer is over it, so a click never lands on the wrong row.",
                "Listen holder rekkefølgen mens pekeren er over den, så et klikk aldri treffer feil rad.",
            ),
            l("CPU: 100% is one full core.", "CPU: 100 % er én hel kjerne."),
        ]
        .into_iter()
        .chain(view.process_recent.then(|| crate::tools_panel::about_notes(crate::tools_panel::Page::Activity)).into_iter().flatten())
        .collect(),
        ..Default::default()
    }
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let s = &view.snap;
    let remaining = remaining_pids(s);
    let sort_cpu = view.process_sort_cpu;

    // Three views of the same processes: every app, what an app left
    // running after it quit, and the last minute's activity per process.
    let mut mode = segment_group("process-mode", l("Show", "Vis"), dark);
    for (i, (only_remaining, recent, label)) in [
        (false, false, l("All apps", "Alle apper").to_string()),
        (true, false, format!("{} · {}", l("Left running", "Fortsatt i gang"), remaining.len())),
        (false, true, l("Last minute", "Siste minutt").to_string()),
    ]
    .into_iter()
    .enumerate()
    {
        let target = this.clone();
        mode = mode.child(segment(
            format!("process-mode-{i}"),
            label,
            view.only_remaining == only_remaining && view.process_recent == recent,
            dark,
            t::accent(dark),
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.only_remaining = only_remaining;
                    v.process_recent = recent;
                    v.expanded_process_group = None;
                    cx.notify();
                });
            },
        ));
    }
    if view.process_recent {
        return div()
            .flex()
            .flex_col()
            .gap(t::SECTION_GAP)
            .child(div().flex().child(mode))
            .child(crate::tools_panel::activity(view, dark, this));
    }
    let mut sort = segment_group("process-sort", l("Sort by", "Sorter etter"), dark);
    for (cpu, label) in [(true, "CPU"), (false, l("Memory", "Minne"))] {
        let target = this.clone();
        sort = sort.child(segment(
            format!("process-sort-{cpu}"),
            label,
            sort_cpu == cpu,
            dark,
            t::accent(dark),
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.process_sort_cpu = cpu;
                    v.process_order = sorted_keys(&v.snap, cpu);
                    v.process_order_at = Some(Instant::now());
                    cx.notify();
                });
            },
        ));
    }
    let page = div().flex().flex_col().gap(t::SECTION_GAP).child(
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(mode)
            .child(
                div()
                    .w(px(220.))
                    .h(t::CONTROL_H)
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(t::RADIUS_CONTROL)
                    .border_1()
                    .border_color(t::line(dark))
                    .text_size(t::TEXT_BODY)
                    .child(gpui_kit::base::input::Input::new(&view.process_filter)),
            )
            .child(div().flex_1())
            .when(order_held(view), |d| d.child(held_chip(dark)))
            .child(sub(l("Sort by", "Sorter etter")).text_color(t::ink_dim(dark)))
            .child(sort),
    );

    let query = view.process_query.as_str();
    let by_pid: HashMap<i32, &ProcInfo> = s.procs.iter().map(|p| (p.pid, p)).collect();
    let matches = |app: &AppGroup| {
        query.is_empty()
            || app.name.to_lowercase().contains(query)
            || app.pids.iter().any(|pid| {
                pid.to_string() == query
                    || by_pid.get(pid).is_some_and(|p| {
                        p.comm.to_lowercase().contains(query) || p.exe_path.to_lowercase().contains(query)
                    })
            })
    };
    let mut apps: Vec<&AppGroup> = s
        .apps
        .iter()
        .filter(|app| !view.only_remaining || app.pids.iter().any(|pid| remaining.contains(pid)))
        .filter(|app| matches(app))
        .collect();
    let position: HashMap<&str, usize> = view
        .process_order
        .iter()
        .enumerate()
        .map(|(i, key)| (key.as_str(), i))
        .collect();
    // New apps join below the held order until the next safe re-sort.
    apps.sort_by(|a, b| {
        let rank = |app: &AppGroup| position.get(app.key.as_str()).copied().unwrap_or(usize::MAX);
        rank(a).cmp(&rank(b)).then_with(|| {
            if sort_cpu {
                b.cpu_pct.total_cmp(&a.cpu_pct)
            } else {
                b.mem_bytes.cmp(&a.mem_bytes)
            }
        })
    });
    if apps.is_empty() {
        return page.child(crate::ui::empty_state(
            "icons/layout-grid.svg",
            t::accent(dark),
            if !query.is_empty() {
                l("No matches", "Ingen treff")
            } else if view.only_remaining {
                l("Nothing left running", "Ingenting er fortsatt i gang")
            } else {
                l("No processes sampled yet", "Ingen prosesser målt ennå")
            }
            .into(),
            if !query.is_empty() {
                l("No app or process name, path or PID contains that text.", "Ingen app- eller prosessnavn, sti eller PID inneholder den teksten.")
            } else if view.only_remaining {
                l(
                    "Helpers that outlive an app you quit during this session appear here.",
                    "Hjelpeprosesser som lever videre etter at du avsluttet en app i denne økten, vises her.",
                )
            } else {
                l("The first sample arrives within a few seconds.", "Den første målingen kommer om noen sekunder.")
            }
            .into(),
            dark,
        ));
    }
    let total = apps.len();
    let shown = if view.process_show_all || !query.is_empty() { total } else { total.min(FIRST_PAGE) };
    let max_value = apps
        .iter()
        .map(|a| if sort_cpu { a.cpu_pct } else { a.mem_bytes as f64 })
        .fold(0., f64::max)
        .max(f64::MIN_POSITIVE);

    let hover_target = this.clone();
    let mut table = card(dark)
        .id("process-table")
        .on_hover(move |hovered, _, cx| {
            let hovered = *hovered;
            hover_target.update(cx, |v, _| v.process_table_hovered = hovered);
        })
        .p_0()
        .pt_3()
        .pb_2()
        .px_2()
        .gap_0()
        .child(header_row(sort_cpu, dark));
    for app in apps.into_iter().take(shown) {
        let expanded = view.expanded_process_group.as_deref() == Some(app.key.as_str());
        let value = if sort_cpu { app.cpu_pct } else { app.mem_bytes as f64 };
        table = table.child(app_row(app, expanded, (value / max_value).clamp(0., 1.), sort_cpu, dark, this.clone()));
        if expanded {
            table = table.child(app_detail(view, app, &remaining, dark, this.clone()));
        }
    }
    if shown < total {
        let target = this.clone();
        table = table.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(t::SPACE_2)
                .pt_2()
                .child(sub(crate::locale::count((total - shown) as u64, "more app", "more apps", "app til", "apper til")).text_color(t::ink_dim(dark)))
                .child(button("process-show-all", l("Show all", "Vis alle"), false, dark, move |_, cx| {
                    target.update(cx, |v, cx| {
                        v.process_show_all = true;
                        cx.notify();
                    });
                })),
        );
    }
    page.child(table)
}

fn app_row(app: &AppGroup, expanded: bool, frac: f64, sort_cpu: bool, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    let key = app.key.clone();
    let keyboard_key = key.clone();
    let keyboard = this.clone();
    let tide = if sort_cpu { Metric::Cpu.accent(dark) } else { Metric::Memory.accent(dark) };
    let toggle = move |this: &Entity<TideGaugeView>, key: &String, cx: &mut App| {
        this.update(cx, |v, cx| {
            v.expanded_process_group = if expanded { None } else { Some(key.clone()) };
            v.process_detail = None;
            cx.notify();
        });
    };
    div()
        .id(SharedString::from(format!("app-row-{}", app.key)))
        .role(Role::Button)
        .aria_label(SharedString::from(app.name.clone()))
        .aria_expanded(expanded)
        .tab_index(0)
        .relative()
        .h(t::ROW_H)
        .flex()
        .items_center()
        .gap_2()
        .px(t::SPACE_2)
        .rounded(t::RADIUS_CONTROL)
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .cursor_pointer()
        .when(expanded, |d| d.bg(t::card_hi(dark)))
        .when(!expanded, |d| d.hover(|h| h.bg(t::card_hi(dark))))
        .focus_visible(move |d| d.border_color(t::accent(dark)))
        .child(
            div()
                .absolute()
                .left_0()
                .top(t::SPACE_1)
                .bottom(t::SPACE_1)
                .rounded(t::RADIUS_CHIP)
                .w(relative(frac as f32))
                .bg(tide.opacity(0.12)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(app_icon(app, dark))
                .child(div().min_w_0().truncate().text_size(t::TEXT_ROW).when(expanded, |d| d.font_weight(FontWeight::SEMIBOLD)).child(app.name.clone())),
        )
        .child(div().w(px(COL_PROCS)).flex_shrink_0().flex().justify_end().text_size(t::TEXT_BODY).text_color(t::ink_dim(dark)).child(app.pids.len().to_string()))
        .child(div().w(px(COL_CPU)).flex_shrink_0().flex().justify_end().text_size(t::TEXT_BODY).when(sort_cpu, |d| d.font_weight(FontWeight::SEMIBOLD)).child(crate::locale::percent(app.cpu_pct, 1)))
        .child(div().w(px(COL_MEM)).flex_shrink_0().flex().justify_end().text_size(t::TEXT_BODY).when(!sort_cpu, |d| d.font_weight(FontWeight::SEMIBOLD)).child(fmt_bytes(app.mem_bytes as f64)))
        .child(div().w(px(16.)).flex_shrink_0().text_size(t::TEXT_CAPTION).text_color(t::ink_dim(dark)).child(if expanded { "▾" } else { "▸" }))
        .on_click(move |_, _, cx| toggle(&this, &key, cx))
        .capture_key_down(move |event, window, cx| {
            if crate::controls::is_activation_key(&event.keystroke) {
                window.prevent_default();
                toggle(&keyboard, &keyboard_key, cx);
                cx.stop_propagation();
            }
        })
}

fn registered_main<'a>(s: &'a Snapshot, app: &AppGroup) -> Option<&'a ProcInfo> {
    let group = s.process_groups.iter().find(|g| g.bundle == app.key)?;
    match group.mains.as_slice() {
        [main] if app.pids.contains(&main.pid) => Some(main),
        _ => None,
    }
}

/// The request that quits `app` the way its row here does, or `None` when
/// nothing of it may be quit from Tidegauge (macOS's own, another user's,
/// or Tidegauge itself).
pub(crate) fn quit_action(s: &Snapshot, app: &AppGroup, force: Force) -> Option<Action> {
    if let Some(main) = registered_main(s, app) {
        return control::blocked(main).is_none().then(|| Action::App {
            main: main.clone(),
            name: app.name.clone(),
            helpers: app.pids.len(),
            memory: app.mem_bytes,
            force,
        });
    }
    let targets: Vec<ProcInfo> = s
        .procs
        .iter()
        .filter(|p| app.pids.contains(&p.pid) && control::blocked(p).is_none())
        .cloned()
        .collect();
    (!targets.is_empty()).then(|| Action::Processes { name: app.name.clone(), targets, force })
}

fn app_detail(view: &TideGaugeView, app: &AppGroup, remaining: &HashSet<i32>, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    let s = &view.snap;
    let mut procs: Vec<&ProcInfo> = s.procs.iter().filter(|p| app.pids.contains(&p.pid)).collect();
    if view.process_sort_cpu {
        procs.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
    } else {
        procs.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    }
    // Left-running helpers first, so the row cutoff never hides them; in the
    // Left running view they are the only rows.
    procs.sort_by_key(|p| !remaining.contains(&p.pid));
    if view.only_remaining {
        procs.retain(|p| remaining.contains(&p.pid));
    }
    let pending = view.process_pending;
    let mut actions = div().flex().items_center().gap_2().flex_wrap();
    let left_running: Vec<ProcInfo> = if view.only_remaining {
        procs.iter().filter(|p| control::blocked(p).is_none()).map(|p| (*p).clone()).collect()
    } else {
        Vec::new()
    };
    if view.only_remaining {
        // Only the leftovers: never offer to quit a relaunched app from here.
        if !left_running.is_empty() && !pending {
            for force in [Force::Quit, Force::ForceQuit] {
                let action = Action::Processes { name: app.name.clone(), targets: left_running.clone(), force };
                let target = this.clone();
                actions = actions.child(quit_button(
                    format!("leftover-{}-{force:?}", app.key),
                    match force {
                        Force::Quit => format!("{} ({})…", l("Quit all", "Avslutt alle"), left_running.len()),
                        Force::ForceQuit => format!("{} ({})…", l("Force quit all", "Tving avslutning av alle"), left_running.len()),
                    },
                    force,
                    dark,
                    move |window, cx| request(action.clone(), target.clone(), window, cx),
                ));
            }
        }
    } else if let Some(main) = registered_main(s, app) {
        if control::blocked(main).is_none() && !pending {
            for force in [Force::Quit, Force::ForceQuit] {
                let action = Action::App { main: main.clone(), name: app.name.clone(), helpers: procs.len(), memory: app.mem_bytes, force };
                let target = this.clone();
                actions = actions.child(quit_button(
                    format!("app-{}-{force:?}", main.pid),
                    match force {
                        Force::Quit => format!("{} {}…", l("Quit", "Avslutt"), app.name),
                        Force::ForceQuit => format!("{} {}…", l("Force quit", "Tving avslutning av"), app.name),
                    },
                    force,
                    dark,
                    move |window, cx| request(action.clone(), target.clone(), window, cx),
                ));
            }
        }
    } else {
        let eligible: Vec<ProcInfo> = procs.iter().filter(|p| control::blocked(p).is_none()).map(|p| (*p).clone()).collect();
        if eligible.len() > 1 && !pending {
            for force in [Force::Quit, Force::ForceQuit] {
                let action = Action::Processes { name: app.name.clone(), targets: eligible.clone(), force };
                let target = this.clone();
                actions = actions.child(quit_button(
                    format!("group-{}-{force:?}", app.key),
                    match force {
                        Force::Quit => format!("{} ({})…", l("Quit all", "Avslutt alle"), eligible.len()),
                        Force::ForceQuit => format!("{} ({})…", l("Force quit all", "Tving avslutning av alle"), eligible.len()),
                    },
                    force,
                    dark,
                    move |window, cx| request(action.clone(), target.clone(), window, cx),
                ));
            }
        }
    }
    if let Some(bundle) = app.icon_path.clone() {
        let path = std::path::PathBuf::from(bundle);
        actions = actions.child(button(format!("app-reveal-{}", app.key), l("Show in Finder", "Vis i Finder"), false, dark, move |_, _| crate::storage::reveal(&path)));
    }

    let all = view.process_all_helpers.as_deref() == Some(app.key.as_str());
    let total = procs.len();
    let mut detail = div()
        .ml(px(38.))
        .mr(t::SPACE_2)
        .mb_2()
        .pl_3()
        .border_l_1()
        .border_color(t::line(dark))
        .flex()
        .flex_col()
        .gap_1()
        .child(div().pt_1().pb_1().child(actions))
        .when(!view.only_remaining, |d| d.child(app_hour(view, app, dark, this.clone())))
        .when_some(app.icon_path.clone(), |d, path| d.child(mono(path, dark)));
    for p in procs.iter().take(if all { total } else { HELPERS_SHOWN }) {
        detail = detail.child(process_row(view, p, app.icon_path.as_deref(), remaining.contains(&p.pid), dark, this.clone()));
    }
    if total > HELPERS_SHOWN {
        let key = app.key.clone();
        let target = this.clone();
        detail = detail.child(div().pt_1().child(button(
            format!("app-helpers-{}", app.key),
            if all {
                l("Show fewer processes", "Vis færre prosesser").to_string()
            } else {
                format!("{} {}", l("Show all", "Vis alle"), count_label(total))
            },
            false,
            dark,
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.process_all_helpers = if all { None } else { Some(key.clone()) };
                    cx.notify();
                });
            },
        )));
    }
    detail
}

/// Processes listed under an expanded app before "Show all".
const HELPERS_SHOWN: usize = 12;

/// The expanded app's last hour from local History.
pub struct AppHistory {
    pub app: String,
    pub loaded_at: Instant,
    pub data: Option<Result<(crate::history_browser::Data, crate::history_browser::Data), String>>,
}

/// The app whose hour should load now: the expanded one, when it has none
/// yet, belongs to another app, or is a minute old.
pub fn app_history_due(view: &TideGaugeView) -> Option<String> {
    if view.destination() != crate::ui::Destination::Apps {
        return None;
    }
    let key = view.expanded_process_group.as_deref()?;
    let name = &view.snap.apps.iter().find(|a| a.key == key)?.name;
    match &view.app_history {
        Some(h) if h.app == *name && (h.data.is_none() || h.loaded_at.elapsed() < Duration::from_secs(60)) => None,
        _ => Some(name.clone()),
    }
}

pub fn load_app_history(this: &Entity<TideGaugeView>, app: String, cx: &mut App) {
    this.update(cx, |view, cx| {
        // A reload keeps the old charts up until the new ones arrive.
        let keep = view.app_history.take().filter(|h| h.app == app).and_then(|h| h.data);
        view.app_history = Some(AppHistory { app: app.clone(), loaded_at: Instant::now(), data: keep });
        cx.spawn(async move |this, cx| {
            let name = app.clone();
            let result = smol::unblock(move || crate::history_browser::app_hour(&name)).await;
            let _ = this.update(cx, |view, cx| {
                if let Some(h) = view.app_history.as_mut().filter(|h| h.app == app) {
                    h.data = Some(result);
                    cx.notify();
                }
            });
        })
        .detach();
    });
}

/// CPU and memory over the last hour, and a way into History for more.
fn app_hour(view: &TideGaugeView, app: &AppGroup, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let data = view.app_history.as_ref().filter(|h| h.app == app.name).and_then(|h| h.data.as_ref());
    let chart = |label: &str, values: &[Option<f64>], color: Hsla, format: &dyn Fn(f64) -> String, floor: f64| {
        let peak = values.iter().flatten().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(t::TEXT_CAPTION)
                    .text_color(t::ink_dim(dark))
                    .child(label.to_string())
                    .child(peak.map(|p| format!("{} {}", l("Highest average", "Høyeste snitt"), format(p))).unwrap_or_default()),
            )
            .child(crate::controls::history_chart(values, peak.unwrap_or(0.).max(floor), color, 44.))
    };
    let body = match data {
        Some(Ok((cpu, mem))) if cpu.values.iter().chain(&mem.values).any(Option::is_some) => div()
            .flex()
            .gap_4()
            .child(chart(l("CPU · last hour", "CPU · siste time"), &cpu.values, Metric::Cpu.accent(dark), &|v| crate::locale::percent(v, 1), 100.))
            .child(chart(l("Memory · last hour", "Minne · siste time"), &mem.values, Metric::Memory.accent(dark), &|v| fmt_bytes(v), 1.)),
        Some(Ok(_)) => sub(l("Nothing recorded for this app in the last hour. History records every ten seconds or so.", "Ingenting registrert for denne appen den siste timen. Historikken registrerer omtrent hvert tiende sekund.")).text_color(t::ink_dim(dark)),
        Some(Err(error)) => sub(error.clone()).text_color(t::ink_dim(dark)),
        None => sub(l("Loading the last hour…", "Laster siste time…")).text_color(t::ink_dim(dark)),
    };
    let name = app.name.clone();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .py_1()
        .child(body)
        .child(div().flex().child(button(format!("app-history-{}", app.key), l("Open in History", "Åpne i Historikk"), false, dark, move |_, cx| {
            this.update(cx, |v, _| {
                v.history_request.app = Some(name.clone());
                v.history_request.hours = 1;
                if !crate::history_browser::Metric::APP.contains(&v.history_request.metric) {
                    v.history_request.metric = crate::history_browser::Metric::Cpu;
                }
            });
            crate::ui::navigate(&this, crate::ui::Destination::Tab(crate::ui::Tab::History), cx);
        })))
}

/// The program's own name: macOS truncates `comm` to 16 characters.
fn program_name(p: &ProcInfo) -> String {
    std::path::Path::new(&p.exe_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| p.comm.clone())
}

/// Path inside the app's bundle when it has one, so helpers do not repeat
/// the whole bundle path on every row.
fn short_path(p: &ProcInfo, bundle: Option<&str>) -> String {
    if let Some(nested) = crate::processes::nested_bundle(&p.exe_path) {
        return format!("› {nested}");
    }
    match bundle.and_then(|b| p.exe_path.strip_prefix(b)) {
        Some(rest) => format!("…{rest}"),
        None => crate::processes::path_summary(&p.exe_path),
    }
}

fn process_row(view: &TideGaugeView, p: &ProcInfo, bundle: Option<&str>, left_running: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let id = Identity::of(p);
    let showing = view.process_detail == Some(id);
    let selected = view.process_selected == Some(id) || showing;
    let blocked = control::blocked(p);
    // Clicks on the row's buttons must not also toggle the row.
    let mut controls = div()
        .id(SharedString::from(format!("proc-controls-{}", p.pid)))
        .on_click(|_, _, cx| cx.stop_propagation())
        .flex()
        .items_center()
        .gap_1()
        .flex_shrink_0();
    match blocked {
        // Unselected rows stay quiet: actions appear on the row you pick.
        _ if !selected => {}
        Some(block) => controls = controls.child(sub(block.reason()).text_color(t::ink_dim(dark))),
        None if !view.process_pending => {
            for force in [Force::Quit, Force::ForceQuit] {
                let action = Action::Processes { name: program_name(p), targets: vec![p.clone()], force };
                let target = this.clone();
                controls = controls.child(quit_button(
                    format!("proc-{}-{force:?}", p.pid),
                    match force {
                        Force::Quit => l("Quit…", "Avslutt…"),
                        Force::ForceQuit => l("Force Quit…", "Tving avslutning…"),
                    },
                    force,
                    dark,
                    move |window, cx| request(action.clone(), target.clone(), window, cx),
                ));
            }
        }
        None => {}
    }
    let target = this.clone();
    if selected {
    controls = controls.child(button(
        format!("proc-detail-{}", p.pid),
        if showing { l("Hide details", "Skjul detaljer") } else { l("Details", "Detaljer") },
        showing,
        dark,
        move |_, cx| {
            target.update(cx, |v, cx| {
                v.process_detail = if showing { None } else { Some(id) };
                cx.notify();
            });
        },
    ));
    }
    let select = this.clone();
    let keyboard = this.clone();
    let toggle = move |this: &Entity<TideGaugeView>, cx: &mut App| {
        this.update(cx, |v, cx| {
            v.process_selected = if selected { None } else { Some(id) };
            if selected {
                v.process_detail = None;
            }
            cx.notify();
        });
    };
    let mut row = div().flex().flex_col().gap_1().py_1().child(
        div()
            .id(SharedString::from(format!("proc-row-{}", p.pid)))
            .role(Role::Button)
            .aria_label(SharedString::from(format!("{} PID {}", program_name(p), p.pid)))
            .aria_selected(selected)
            .tab_index(0)
            .min_h(t::ROW_H)
            .px_2()
            .rounded(t::RADIUS_CONTROL)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .cursor_pointer()
            .when(selected, |d| d.bg(t::card_hi(dark)))
            .when(!selected, |d| d.hover(|h| h.bg(t::card_hi(dark).opacity(0.5))))
            .focus_visible(move |d| d.border_color(t::accent(dark)))
            .on_click(move |_, _, cx| toggle(&select, cx))
            .capture_key_down(move |event, window, cx| {
                if crate::controls::is_activation_key(&event.keystroke) {
                    window.prevent_default();
                    toggle(&keyboard, cx);
                    cx.stop_propagation();
                }
            })
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(t::TEXT_BODY)
                            .child(div().truncate().child(program_name(p)))
                            .child(div().text_color(t::ink_dim(dark)).child(format!("PID {}", p.pid)))
                            .when(left_running, |d| {
                                d.child(
                                    div()
                                        .px_1()
                                        .rounded(t::RADIUS_CHIP)
                                        .bg(t::warn(dark).opacity(0.14))
                                        .text_size(t::TEXT_CAPTION)
                                        .child(l("Left running", "Fortsatt i gang")),
                                )
                            }),
                    )
                    .child(mono(short_path(p, bundle), dark)),
            )
            .child(div().w(px(COL_CPU)).flex_shrink_0().flex().justify_end().text_size(t::TEXT_BODY).child(crate::locale::percent(p.cpu_pct, 1)))
            .child(div().w(px(COL_MEM)).flex_shrink_0().flex().justify_end().text_size(t::TEXT_BODY).child(fmt_bytes(p.mem_bytes as f64)))
            // Same trailing slot as the app row's chevron, so columns line up.
            .child(div().w(px(16.)).flex_shrink_0()),
    );
    // Actions sit on their own line under a selected row, so its numbers
    // never move when the buttons appear.
    if selected {
        row = row.child(div().flex().justify_end().px_2().child(controls));
    }
    if showing {
        row = row.child(process_details(view, p, dark, this));
    }
    row
}

/// The app this process was seen starting under, when Tidegauge observed it.
fn started_by(s: &Snapshot, p: &ProcInfo) -> Option<String> {
    let id = Identity::of(p);
    s.process_groups.iter().find_map(|group| {
        let row = group.rows.iter().find(|row| Identity::of(&row.process) == id)?;
        let origin = row.origin.as_ref().filter(|origin| origin.observed_ancestor && origin.main != id)?;
        Some(format!(
            "{} {} (PID {})",
            l("Started by", "Startet av"),
            group.name,
            origin.main.pid
        ))
    })
}

fn process_details(view: &TideGaugeView, p: &ProcInfo, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let matching: Vec<_> = view
        .tools
        .startup
        .as_ref()
        .map(|inventory| {
            inventory
                .entries
                .iter()
                .filter(|entry| !p.exe_path.is_empty() && entry.executable.as_deref() == Some(p.exe_path.as_str()))
                .map(|entry| entry.label.clone())
                .take(5)
                .collect()
        })
        .unwrap_or_default();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .rounded(t::RADIUS_CONTROL)
        .bg(t::card_hi(dark))
        .child(sub(format!(
            "UID {} · {} {} · {} {}",
            p.uid,
            l("Parent PID", "Forelder-PID"),
            p.ppid,
            l("Started", "Startet"),
            crate::history_browser::local_time(p.start_unix)
        )))
        .child(sub(if matching.is_empty() {
            l("No matching startup item.", "Ingen samsvarende oppstartselement.").to_string()
        } else {
            format!("{}: {}", l("Possible startup item", "Mulig oppstartselement"), matching.join(", "))
        }))
        .when_some(started_by(&view.snap, p), |d, text| d.child(sub(text)))
        .when(control::part_of_macos(p), |d| {
            d.child(sub(l("Part of macOS.", "Del av macOS.")).text_color(t::ink_dim(dark)))
        })
        .child(signing_details(view, p, dark, this))
}

fn signing_details(
    view: &TideGaugeView,
    process: &ProcInfo,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> Div {
    let key = Key::of(process);
    let mut details = div().flex().flex_col().gap_1();
    if view.process_signing.pending.as_ref() == Some(&key) {
        return details.child(sub(l(
            "Reading signing metadata…",
            "Leser signeringsinformasjon…",
        )));
    }
    if let Some(result) = view.process_signing.result(&key) {
        details = details.child(sub(match result {
            Ok(metadata) => format!(
                "{}: {} · {}: {}",
                l("Signing identifier", "Signeringsidentifikator"),
                metadata
                    .identifier
                    .as_deref()
                    .unwrap_or(l("Not reported", "Ikke rapportert")),
                l("Team ID", "Team-ID"),
                metadata
                    .team
                    .as_deref()
                    .unwrap_or(l("Not reported", "Ikke rapportert"))
            ),
            Err(Failure::Changed) => l(
                "The process exited or changed identity. Signing metadata was discarded.",
                "Prosessen ble avsluttet eller endret identitet. Signeringsinformasjonen ble forkastet.",
            )
            .into(),
            Err(Failure::Unavailable) => l(
                "Signing metadata is unavailable. This does not establish that the process is unsigned or unsafe.",
                "Signeringsinformasjon er utilgjengelig. Dette betyr ikke at prosessen er usignert eller utrygg.",
            )
            .into(),
        }))
        .child(sub(l(
            "Lookup snapshot only; some metadata comes from disk. Signature validity and developer identity are not verified. Signed does not mean safe; unsigned does not mean harmful.",
            "Kun et øyeblikksbilde; noe informasjon kommer fra disk. Signaturens gyldighet og utviklerens identitet er ikke bekreftet. Signert betyr ikke trygg; usignert betyr ikke skadelig.",
        )));
    }
    if view.process_signing.pending.is_none() && !process.exe_path.is_empty() {
        details = details.child(div().child(button(
            format!("process-signing-{}", process.pid),
            l("Read signing metadata", "Les signeringsinformasjon"),
            false,
            dark,
            move |_, cx| {
                if this.read(cx).process_signing.pending.is_some() {
                    return;
                }
                let key = key.clone();
                this.update(cx, |v, cx| {
                    v.process_signing.pending = Some(key.clone());
                    cx.notify();
                });
                let target = this.clone();
                cx.spawn(async move |cx| {
                    let lookup = key.clone();
                    let result =
                        smol::unblock(move || crate::process_signing::inspect(&lookup)).await;
                    target.update(cx, |v, cx| {
                        v.process_signing.finish(key, result);
                        cx.notify();
                    });
                })
                .detach();
            },
        )));
    }
    details
}

#[derive(Clone)]
pub(crate) enum Action {
    App { main: ProcInfo, name: String, helpers: usize, memory: u64, force: Force },
    Processes { name: String, targets: Vec<ProcInfo>, force: Force },
}

pub(crate) fn request(action: Action, this: Entity<TideGaugeView>, window: &mut Window, cx: &mut App) {
    if this.read(cx).process_pending {
        return;
    }
    let (title, detail, confirm, level) = match &action {
        Action::App { name, helpers, memory, force: Force::Quit, .. } => (
            format!("{} “{name}”?", l("Do you want to quit", "Vil du avslutte")),
            format!(
                "{}\n{} · {}",
                l("The app may ask you to save changes first.", "Appen kan be deg lagre endringer først."),
                count_label(*helpers),
                fmt_bytes(*memory as f64)
            ),
            l("Quit", "Avslutt"),
            PromptLevel::Warning,
        ),
        Action::App { name, helpers, memory, force: Force::ForceQuit, .. } => (
            format!("{} “{name}”?", l("Do you want to force quit", "Vil du tvinge avslutning av")),
            format!(
                "{}\n{} · {}",
                l("Unsaved changes will be lost.", "Endringer som ikke er lagret, går tapt."),
                count_label(*helpers),
                fmt_bytes(*memory as f64)
            ),
            l("Force Quit", "Tving avslutning"),
            PromptLevel::Critical,
        ),
        Action::Processes { name, targets, force } => {
            let single = targets.len() == 1;
            let macos = targets.iter().any(control::part_of_macos);
            let what = if single {
                format!("“{name}” (PID {})", targets[0].pid)
            } else {
                format!("{} “{name}”", count_label(targets.len()))
            };
            let mut detail = match force {
                Force::Quit => l(
                    "It is asked to stop (SIGTERM). A service may start it again.",
                    "Den blir bedt om å stoppe (SIGTERM). En tjeneste kan starte den igjen.",
                ),
                Force::ForceQuit => l(
                    "It is stopped at once (SIGKILL). Unsaved work is lost.",
                    "Den stoppes umiddelbart (SIGKILL). Ulagret arbeid går tapt.",
                ),
            }
            .to_string();
            if macos {
                detail.push_str("\n\n");
                detail.push_str(l(
                    "This is part of macOS. It usually restarts on its own, and things may misbehave until it does.",
                    "Dette er en del av macOS. Den starter vanligvis på nytt av seg selv, og ting kan oppføre seg rart til den gjør det.",
                ));
            }
            if single {
                detail.push_str("\n\n");
                detail.push_str(&targets[0].exe_path);
            }
            match force {
                Force::Quit => (
                    format!("{} {what}?", l("Do you want to quit", "Vil du avslutte")),
                    detail,
                    l("Quit", "Avslutt"),
                    PromptLevel::Warning,
                ),
                Force::ForceQuit => (
                    format!("{} {what}?", l("Do you want to force quit", "Vil du tvinge avslutning av")),
                    detail,
                    l("Force Quit", "Tving avslutning"),
                    PromptLevel::Critical,
                ),
            }
        }
    };
    this.update(cx, |v, cx| {
        v.process_pending = true;
        cx.notify();
    });
    let answer = window.prompt(level, &title, Some(&detail), &[l("Cancel", "Avbryt"), confirm], cx);
    cx.spawn(async move |cx| {
        if answer.await != Ok(1) {
            this.update(cx, |v, cx| {
                v.process_pending = false;
                cx.notify();
            });
            return;
        }
        let result = smol::unblock(move || match action {
            Action::App { main, name, force, .. } => control::quit_app(&main, &name, force),
            Action::Processes { name, targets, force } => control::signal(&targets, &name, force),
        })
        .await;
        this.update(cx, |v, cx| {
            v.process_pending = false;
            v.process_selected = None;
            v.toast = Some(crate::ui::Toast::new(
                match &result {
                    Ok(s) | Err(s) => s.clone(),
                },
                result.is_err(),
            ));
            cx.notify();
        });
    })
    .detach();
}
