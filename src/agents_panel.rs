//! Agents: what is left of your AI plans (read from OpenQuota) and the
//! coding agents running now, with what each one started.

use crate::agents::{Activity, Pace, Plan, Plans, Role, Session, Window};
use crate::controls::button;
use crate::engine::fmt_bytes;
use crate::locale::text as l;
use crate::process_control::{self as control, Force};
use crate::process_panel::Action;
use crate::theme as t;
use crate::ui::{card, donut, empty_state, section_header, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Usage older than this is shown as "as of", without a pace estimate.
const FRESH_SECS: f64 = 3600.;

pub fn open(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.show_agents = true;
        view.show_processes = false;
        view.show_tools = false;
        view.show_settings = false;
        view.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    });
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.)
}

fn duration(secs: f64) -> String {
    let s = secs.max(0.) as u64;
    let (days, hours, minutes) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    let h = l("h", "t");
    if days > 0 {
        format!("{days} d {hours} {h}")
    } else if hours > 0 {
        format!("{hours} {h} {minutes} min")
    } else {
        format!("{} min", minutes.max(1))
    }
}

fn provider_name(id: &str) -> String {
    match id {
        "claude" => "Claude".into(),
        "codex" => "Codex".into(),
        "copilot" | "github-copilot" => "GitHub Copilot".into(),
        "cursor" => "Cursor".into(),
        "gemini" => "Gemini".into(),
        "devin" => "Devin".into(),
        "grok" => "Grok".into(),
        "opencode" => "OpenCode".into(),
        "openrouter" => "OpenRouter".into(),
        "mistral" => "Mistral".into(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

fn window_title(label: &str) -> String {
    match label.to_lowercase().as_str() {
        "5h" => l("5-hour session", "5-timersøkt").into(),
        "week" | "1w" | "7d" => l("This week", "Denne uken").into(),
        "month" => l("This month", "Denne måneden").into(),
        "24h" | "day" => l("Today", "I dag").into(),
        _ => label.to_string(),
    }
}

/// The short name of a window, for the line listing the other windows.
fn window_short(label: &str) -> String {
    match label.to_lowercase().as_str() {
        "5h" => l("5-hour", "5 timer").into(),
        "week" | "1w" | "7d" => l("Week", "Uke").into(),
        "month" => l("Month", "Måned").into(),
        "24h" | "day" => l("Today", "I dag").into(),
        _ => label.to_string(),
    }
}

fn percent(fraction: f64) -> String {
    crate::locale::percent((fraction * 100.).round(), 0)
}

/// What is left of a plan's credits. Money gets cents; nothing left reads 0.
fn credits_left(amount: f64, unit: &str) -> String {
    let amount = amount.max(0.);
    let is_money = unit.len() == 3 && unit.chars().all(|c| c.is_ascii_uppercase());
    let value = if is_money && amount < 100. {
        crate::locale::number(amount, 2)
    } else {
        crate::locale::whole(amount)
    };
    format!("{value} {unit} {}", l("left", "igjen"))
}

/// Stale, failed or too old to trust: show its age, never a pace.
fn plan_not_updating(plan: &Plan, now: f64) -> bool {
    plan.stale || plan.failed || plan.fetched_at.is_none_or(|at| now - at > FRESH_SECS)
}

/// Plan rings for the Overview: one compact chip per provider, opening Agents.
pub fn plan_strip(plans: &Plans, dark: bool, this: Entity<TideGaugeView>) -> Option<Div> {
    let Plans::Loaded(plans) = plans else { return None };
    if plans.is_empty() {
        return None;
    }
    let now = now();
    let mut strip = div().flex().flex_wrap().gap_2();
    for plan in plans {
        let tight = plan.tightest();
        let left = tight.map(|w| 1. - w.used);
        let old = plan_not_updating(plan, now);
        let pace = tight.map(|w| crate::agents::pace(w, now)).unwrap_or(Pace::Unknown);
        let color = if old {
            t::ink_dim(dark)
        } else {
            match (&pace, left) {
                (Pace::UsedUp, _) => t::warn(dark),
                (Pace::RunsOut { .. }, _) => t::caution(dark),
                (_, Some(left)) if left < 0.15 => t::caution(dark),
                _ => t::ok(dark),
            }
        };
        let ring: Vec<(f64, Hsla)> = left.map(|left| vec![(left, color), (1. - left, t::card_hi(dark))]).unwrap_or_default();
        let note = match (old, &pace) {
            (true, _) => l("not updating", "oppdateres ikke").to_string(),
            (false, Pace::RunsOut { in_secs }) => format!("{} {}", l("out in", "tom om"), duration(*in_secs)),
            (false, Pace::UsedUp) => l("used up", "brukt opp").to_string(),
            (false, Pace::OnPace { .. }) => l("on pace", "i rute").to_string(),
            (false, Pace::Unknown) => tight.map(|w| window_title(&w.label)).unwrap_or_default(),
        };
        let target = this.clone();
        let keyboard = this.clone();
        let provider = plan.provider.clone();
        strip = strip.child(
            div()
                .id(SharedString::from(format!("plan-chip-{provider}")))
                .role(gpui_kit::Role::Button)
                .aria_label(SharedString::from(format!("{} {}", provider_name(&provider), left.map(percent).unwrap_or_default())))
                .tab_index(0)
                .flex()
                .items_center()
                .gap_2()
                .pl_1()
                .pr_3()
                .py_1()
                .rounded(t::RADIUS_CONTROL)
                .border_1()
                .border_color(t::line(dark))
                .bg(t::card(dark))
                .cursor_pointer()
                .hover(|d| d.bg(t::card_hi(dark)))
                .focus_visible(move |d| d.border_color(t::accent(dark)))
                .child(div().size(px(28.)).flex_shrink_0().child(donut(&ring, 28., t::card_hi(dark))))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .text_size(t::TEXT_BODY)
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(provider_name(&plan.provider)))
                                .child(div().text_color(color).child(format!(
                                    "{} {}",
                                    left.map(percent).unwrap_or_else(|| "–".into()),
                                    l("left", "igjen")
                                ))),
                        )
                        .child(div().text_size(t::TEXT_SMALL).text_color(if matches!(pace, Pace::RunsOut { .. }) && !old { color } else { t::ink_dim(dark) }).child(note)),
                )
                .on_click(move |_, _, cx| open(&target, cx))
                .capture_key_down(move |event, window, cx| {
                    if crate::controls::is_activation_key(&event.keystroke) {
                        window.prevent_default();
                        open(&keyboard, cx);
                        cx.stop_propagation();
                    }
                }),
        );
    }
    Some(strip)
}

fn plan_tile(plan: &Plan, now: f64, dark: bool) -> Div {
    let tight = plan.tightest();
    let left = tight.map(|w| 1. - w.used);
    let pace = tight.map(|w| crate::agents::pace(w, now)).unwrap_or(Pace::Unknown);
    let old = plan_not_updating(plan, now);
    let color = if old {
        t::ink_dim(dark)
    } else {
        match (&pace, left) {
            (Pace::UsedUp, _) => t::warn(dark),
            (Pace::RunsOut { .. }, _) => t::caution(dark),
            (_, Some(left)) if left < 0.15 => t::caution(dark),
            _ => t::ok(dark),
        }
    };
    let ring: Vec<(f64, Hsla)> = match left {
        Some(left) => vec![(left, color), (1. - left, t::card_hi(dark))],
        None => Vec::new(),
    };
    let status: Option<(String, Hsla)> = match (old, &pace, tight) {
        (true, _, _) => Some((
            match plan.fetched_at {
                Some(at) => format!(
                    "{} {} · {}",
                    l("As of", "Per"),
                    crate::history_browser::local_time(at),
                    l("not updating", "oppdateres ikke")
                ),
                None => l("Not updating", "Oppdateres ikke").into(),
            },
            t::ink_dim(dark),
        )),
        (false, Pace::OnPace { spare }, _) => Some((
            format!("{} · {} {}", l("On pace", "I rute"), percent(*spare), l("to spare", "til overs")),
            t::ink_dim(dark),
        )),
        (false, Pace::RunsOut { in_secs }, _) => Some((
            format!("{} {} {}", l("Runs out in", "Går tom om"), duration(*in_secs), l("at this rate", "i dette tempoet")),
            t::caution(dark),
        )),
        (false, Pace::UsedUp, Some(Window { resets_at: Some(reset), .. })) => Some((
            format!("{} · {} {}", l("Used up", "Brukt opp"), l("resets in", "nullstilles om"), duration(reset - now)),
            t::warn(dark),
        )),
        (false, _, Some(Window { resets_at: Some(reset), .. })) if *reset > now => Some((
            format!("{} {}", l("Resets in", "Nullstilles om"), duration(reset - now)),
            t::ink_dim(dark),
        )),
        _ => None,
    };
    let others: Vec<String> = plan
        .windows
        .iter()
        .filter(|w| Some(*w) != tight)
        .map(|w| format!("{} {} {}", window_short(&w.label), percent(1. - w.used), l("left", "igjen")))
        .collect();
    card(dark)
        .flex_row()
        .items_center()
        .gap_3()
        .p_3()
        .child(
            div()
                .relative()
                .size(px(56.))
                .flex_shrink_0()
                .child(donut(&ring, 56., t::card_hi(dark)))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(t::TEXT_ROW)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(left.map(percent).unwrap_or_else(|| "–".into())),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(div().text_size(t::TEXT_ROW).font_weight(FontWeight::SEMIBOLD).child(provider_name(&plan.provider)))
                        .when_some(plan.plan.clone(), |d, name| {
                            d.child(div().text_xs().text_color(t::ink_dim(dark)).child(name))
                        }),
                )
                .child(sub(match tight {
                    Some(w) => format!("{} · {} {}", window_title(&w.label), l("left", "igjen"), left.map(percent).unwrap_or_default()),
                    None => l("No usage window reported", "Ingen bruksperiode rapportert").into(),
                }).text_color(t::ink_dim(dark)))
                .when_some(status, |d, (text, color)| d.child(sub(text).text_color(color)))
                .when(!others.is_empty(), |d| d.child(sub(others.join(" · ")).text_color(t::ink_dim(dark))))
                .when_some(plan.credits.clone(), |d, (amount, unit)| {
                    d.child(sub(credits_left(amount, &unit)).text_color(t::ink_dim(dark)))
                })
                .when(plan.failed, |d| {
                    d.child(sub(l("OpenQuota's last refresh failed", "OpenQuotas siste oppdatering mislyktes")).text_color(t::caution(dark)))
                }),
        )
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Server => l("server", "server"),
        Role::Browser => l("browser", "nettleser"),
        Role::ToolServer => l("tool server", "verktøyserver"),
        Role::Command => l("command", "kommando"),
    }
}

fn session_title(session: &Session) -> String {
    let folder_name = session
        .folder
        .as_deref()
        .and_then(|f| std::path::Path::new(f).file_name())
        .map(|n| n.to_string_lossy().into_owned());
    session
        .title
        .clone()
        .or(folder_name)
        .unwrap_or_else(|| session.agent.name().into())
}

/// One line per session; selecting it shows what it started and Quit.
fn session_row(session: &Session, now: f64, expanded: bool, pending: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let title = session_title(&session);
    let working = session.activity == Activity::Working;
    let (chip, chip_color) = match session.activity {
        Activity::Working => (l("Working", "Arbeider").to_string(), t::ok(dark)),
        Activity::Idle { since: Some(since) } if since < now => (
            format!("{} {}", l("idle", "inaktiv i"), duration(now - since)),
            t::ink_dim(dark),
        ),
        Activity::Idle { .. } => (l("Idle", "Inaktiv").to_string(), t::ink_dim(dark)),
    };
    let folder_name = session
        .folder
        .as_deref()
        .and_then(|f| std::path::Path::new(f).file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let mut meta = vec![session.agent.name().to_string()];
    if let Some(host) = &session.host {
        meta.push(format!("{} {host}", l("in", "i")));
    }
    meta.extend(folder_name.filter(|name| *name != title));
    if session.root.start_unix > 0. {
        meta.push(format!("{} {}", l("running", "har kjørt"), duration(now - session.root.start_unix)));
    }
    let pid = session.root.pid;
    let toggle_target = this.clone();
    let keyboard = this.clone();
    let toggle = move |this: &Entity<TideGaugeView>, cx: &mut App| {
        this.update(cx, |v, cx| {
            v.agent_expanded = if expanded { None } else { Some(pid) };
            cx.notify();
        });
    };
    let initial = session.agent.name().chars().next().unwrap_or('?').to_string();
    let row = div()
        .id(SharedString::from(format!("agent-row-{pid}")))
        .role(gpui_kit::Role::Button)
        .aria_label(SharedString::from(title.clone()))
        .aria_expanded(expanded)
        .tab_index(0)
        .h(px(44.))
        .px_3()
        .flex()
        .items_center()
        .gap_3()
        .rounded(t::RADIUS_CONTROL)
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .cursor_pointer()
        .when(expanded, |d| d.bg(t::card_hi(dark)))
        .when(!expanded, |d| d.hover(|h| h.bg(t::card_hi(dark).opacity(0.5))))
        .focus_visible(move |d| d.border_color(t::accent(dark)))
        .on_click(move |_, _, cx| toggle(&toggle_target, cx))
        .capture_key_down(move |event, window, cx| {
            if crate::controls::is_activation_key(&event.keystroke) {
                window.prevent_default();
                toggle(&keyboard, cx);
                cx.stop_propagation();
            }
        })
        .child(
            div()
                .size(px(24.))
                .flex_shrink_0()
                .rounded(t::RADIUS_CHIP)
                .bg(if working { t::ok(dark).opacity(0.16) } else { t::card_hi(dark) })
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .child(initial),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(t::TEXT_ROW)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if working { t::ink(dark) } else { t::ink(dark).opacity(0.75) })
                        .child(title),
                )
                .child(div().truncate().text_xs().text_color(t::ink_dim(dark)).child(meta.join(" · "))),
        )
        .child(
            div()
                .flex_shrink_0()
                .px_2()
                .rounded(t::RADIUS_CHIP)
                .bg(chip_color.opacity(0.14))
                .text_color(chip_color)
                .text_xs()
                .child(chip),
        )
        .child(
            div()
                .w(px(64.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(t::TEXT_BODY)
                .text_color(t::ink_dim(dark))
                .child(if session.cpu_pct >= 1. { crate::locale::percent(session.cpu_pct, 0) } else { String::new() }),
        )
        .child(
            div()
                .w(px(80.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(t::TEXT_ROW)
                .font_weight(FontWeight::SEMIBOLD)
                .child(fmt_bytes(session.mem_bytes as f64)),
        )
        .child(div().w(px(12.)).flex_shrink_0().text_size(t::TEXT_CAPTION).text_color(t::ink_dim(dark)).child(if expanded { "▾" } else { "▸" }));
    let mut block = div().flex().flex_col().child(row);
    if !expanded {
        return block;
    }
    let mut detail = div()
        .ml(px(48.))
        .mr_3()
        .mb_2()
        .pl_3()
        .border_l_1()
        .border_color(t::line(dark))
        .flex()
        .flex_col()
        .gap_1();
    let targets: Vec<_> = session.tree.iter().filter(|p| control::blocked(p).is_none()).cloned().collect();
    let mut actions = div().flex().items_center().gap_2().pt_1().pb_1();
    if !targets.is_empty() && !pending {
        let action = Action::Processes { name: session_title(session), targets, force: Force::Quit };
        let target = this.clone();
        actions = actions.child(button(
            format!("quit-session-{pid}"),
            l("Quit session…", "Avslutt økten…"),
            false,
            dark,
            move |window, cx| crate::process_panel::request(action.clone(), target.clone(), window, cx),
        ));
    }
    if let Some(folder) = session.folder.clone() {
        let path = std::path::PathBuf::from(folder);
        actions = actions.child(button(format!("agent-folder-{pid}"), l("Show in Finder", "Vis i Finder"), false, dark, move |_, _| crate::storage::reveal(&path)));
    }
    detail = detail.child(actions);
    if session.started.is_empty() {
        detail = detail.child(sub(l("Nothing started", "Ingenting startet")).text_color(t::ink_dim(dark)));
    }
    for started in session.started.iter().take(12) {
        let mut what = role_label(started.role).to_string();
        if let Some(port) = started.port {
            what = format!("{what} · {} {port}", l("port", "port"));
        }
        if started.cpu_pct >= 1. {
            what = format!("{what} · {} CPU", crate::locale::percent(started.cpu_pct, 0));
        }
        detail = detail.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(t::TEXT_BODY)
                .child(div().truncate().child(if started.count > 1 {
                    format!("{} ×{}", started.name, started.count)
                } else {
                    started.name.clone()
                }))
                .child(div().flex_shrink_0().text_color(t::ink_dim(dark)).child(what))
                .child(div().flex_1())
                .child(div().flex_shrink_0().text_color(t::ink_dim(dark)).child(fmt_bytes(started.mem_bytes as f64))),
        );
    }
    block = block.child(detail);
    block
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let s = &view.snap;
    let now = now();
    let mut page = div().flex().flex_col().gap(t::SECTION_GAP);
    page = page.child(section_header(l("AI plans", "AI-abonnementer"), l("From OpenQuota", "Fra OpenQuota"), dark));
    page = match &s.plans {
        Plans::Loaded(plans) if !plans.is_empty() => page.child(
            div()
                .grid()
                .grid_cols(3)
                .gap_3()
                .children(plans.iter().map(|plan| plan_tile(plan, now, dark))),
        ),
        Plans::Loaded(_) | Plans::Missing => page.child(empty_state(
            "icons/gauge.svg",
            t::accent(dark),
            l("Plan usage comes from OpenQuota", "Abonnementsbruk kommer fra OpenQuota").into(),
            l(
                "Install OpenQuota and connect your accounts. Tidegauge reads the numbers it saves on this Mac and never your logins.",
                "Installer OpenQuota og koble til kontoene dine. Tidegauge leser tallene den lagrer på denne Macen, aldri innloggingene dine.",
            )
            .into(),
            dark,
        )),
        Plans::Unreadable => page.child(empty_state(
            "icons/gauge.svg",
            t::caution(dark),
            l("OpenQuota's saved usage can't be read", "OpenQuotas lagrede bruk kan ikke leses").into(),
            l(
                "The file may be from a newer OpenQuota or being rewritten. Tidegauge tries again when it changes.",
                "Filen kan være fra en nyere OpenQuota eller holde på å skrives. Tidegauge prøver igjen når den endres.",
            )
            .into(),
            dark,
        )),
    };

    let total: u64 = s.agents.iter().map(|session| session.mem_bytes).sum();
    let working = s.agents.iter().filter(|a| a.activity == Activity::Working).count();
    if s.agents.is_empty() {
        return page.child(section_header(l("Running agents", "Agenter som kjører"), "", dark)).child(empty_state(
            "icons/bot.svg",
            t::accent(dark),
            l("No coding agents running", "Ingen kodeagenter kjører").into(),
            l(
                "Claude Code, Codex, Gemini CLI, Cursor Agent and Copilot CLI sessions appear here while they run.",
                "Økter fra Claude Code, Codex, Gemini CLI, Cursor Agent og Copilot CLI vises her mens de kjører.",
            )
            .into(),
            dark,
        ));
    }
    let summary = if working > 0 {
        format!("{} · {} {} · {}", s.agents.len(), working, l("working", "arbeider"), fmt_bytes(total as f64))
    } else {
        format!("{} · {}", s.agents.len(), fmt_bytes(total as f64))
    };
    let idle: Vec<&Session> = s.agents.iter().filter(|a| a.activity != Activity::Working).collect();
    let idle_targets: Vec<_> = idle
        .iter()
        .flat_map(|session| session.tree.iter())
        .filter(|p| control::blocked(p).is_none())
        .cloned()
        .collect();
    // Caption on the left; summary and the bulk action together on the right.
    let mut right = div()
        .flex()
        .items_center()
        .gap_3()
        .child(div().text_size(t::TEXT_CAPTION).text_color(t::ink_dim(dark)).child(summary));
    if idle.len() > 1 && !idle_targets.is_empty() && !view.process_pending {
        let action = Action::Processes {
            name: crate::locale::count(idle.len() as u64, "idle session", "idle sessions", "inaktiv økt", "inaktive økter"),
            targets: idle_targets,
            force: Force::Quit,
        };
        let target = this.clone();
        right = right.child(button(
            "quit-idle-sessions",
            format!("{} ({})…", l("Quit idle sessions", "Avslutt inaktive økter"), idle.len()),
            false,
            dark,
            move |window, cx| crate::process_panel::request(action.clone(), target.clone(), window, cx),
        ));
    }
    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .child(crate::ui::caption(l("Running agents", "Agenter som kjører"), dark))
            .child(right),
    );
    let mut list = card(dark).p_1().gap(px(2.));
    for session in &s.agents {
        let expanded = view.agent_expanded == Some(session.root.pid);
        list = list.child(session_row(session, now, expanded, view.process_pending, dark, this.clone()));
    }
    page.child(list)
}

#[cfg(test)]
mod tests {
    // Named imports: the UI kit's glob export includes its own `test`.
    use super::{credits_left, plan_not_updating, Plan, FRESH_SECS};
    use crate::locale::{number, text as l, whole};

    #[test]
    fn credits_read_as_people_write_them() {
        let left = l("left", "igjen");
        assert_eq!(credits_left(57602., "credits"), format!("{} credits {left}", whole(57602.)));
        assert_eq!(credits_left(-0.3, "USD"), format!("{} USD {left}", number(0., 2)), "never -0");
        assert_eq!(credits_left(12.5, "USD"), format!("{} USD {left}", number(12.5, 2)));
    }

    #[test]
    fn failed_or_stale_plans_never_show_a_pace() {
        let plan = |stale, failed, fetched_at| Plan {
            provider: "claude".into(),
            plan: None,
            windows: Vec::new(),
            credits: None,
            fetched_at,
            stale,
            failed,
        };
        let now = 10_000.;
        assert!(!plan_not_updating(&plan(false, false, Some(now - 60.)), now));
        assert!(plan_not_updating(&plan(false, true, Some(now - 60.)), now), "a failed refresh is not current");
        assert!(plan_not_updating(&plan(true, false, Some(now - 60.)), now));
        assert!(plan_not_updating(&plan(false, false, Some(now - FRESH_SECS - 1.)), now));
        assert!(plan_not_updating(&plan(false, false, None), now));
    }
}
