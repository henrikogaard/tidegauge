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

fn percent(fraction: f64) -> String {
    format!("{:.0}%", (fraction * 100.).round())
}

fn plan_tile(plan: &Plan, now: f64, dark: bool) -> Div {
    let tight = plan.tightest();
    let left = tight.map(|w| 1. - w.used);
    let pace = tight.map(|w| crate::agents::pace(w, now)).unwrap_or(Pace::Unknown);
    let old = plan.stale || plan.fetched_at.is_none_or(|at| now - at > FRESH_SECS);
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
        .map(|w| format!("{} {} {}", w.label, percent(1. - w.used), l("left", "igjen")))
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
                        .text_size(px(13.))
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
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(provider_name(&plan.provider)))
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
                    d.child(sub(format!("{amount:.0} {unit} {}", l("left", "igjen"))).text_color(t::ink_dim(dark)))
                })
                .when(plan.failed, |d| {
                    d.child(sub(l("OpenQuota's last refresh failed", "OpenQuotas siste oppdatering mislyktes")).text_color(t::caution(dark)))
                }),
        )
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Server => l("server", "tjener"),
        Role::Browser => l("browser", "nettleser"),
        Role::ToolServer => l("tool server", "verktøytjener"),
        Role::Command => l("command", "kommando"),
    }
}

fn session_card(session: &Session, now: f64, pending: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let folder_name = session
        .folder
        .as_deref()
        .and_then(|f| std::path::Path::new(f).file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let title = session
        .title
        .clone()
        .or_else(|| folder_name.clone())
        .unwrap_or_else(|| session.agent.name().into());
    let (chip, chip_color) = match session.activity {
        Activity::Working => (l("Working", "Jobber").to_string(), t::ok(dark)),
        Activity::Idle { since: Some(since) } if since < now => (
            format!("{} {}", l("idle", "inaktiv"), duration(now - since)),
            t::ink_dim(dark),
        ),
        Activity::Idle { .. } => (l("idle", "inaktiv").to_string(), t::ink_dim(dark)),
    };
    let mut meta = vec![session.agent.name().to_string()];
    meta.extend(folder_name.filter(|name| Some(name) != session.title.as_ref()));
    if session.root.start_unix > 0. {
        meta.push(format!("{} {}", l("running", "har kjørt"), duration(now - session.root.start_unix)));
    }
    let initial = session.agent.name().chars().next().unwrap_or('?').to_string();
    let mut body = card(dark)
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(22.))
                        .flex_shrink_0()
                        .rounded(px(6.))
                        .bg(t::card_hi(dark))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(initial),
                )
                .child(div().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(title.clone()))
                .child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .rounded(px(6.))
                        .bg(chip_color.opacity(0.14))
                        .text_color(chip_color)
                        .text_xs()
                        .child(chip),
                )
                .child(div().flex_1())
                .when(session.cpu_pct >= 1., |d| {
                    d.child(div().text_size(px(12.)).text_color(t::ink_dim(dark)).child(format!("{:.0}% CPU", session.cpu_pct)))
                })
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(fmt_bytes(session.mem_bytes as f64))),
        )
        .child(sub(meta.join(" · ")).text_color(t::ink_dim(dark)));
    if !session.started.is_empty() {
        body = body.child(
            div()
                .pt_1()
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t::ink_dim(dark))
                .child(format!("{} · {}", l("STARTED", "STARTET"), session.started.len())),
        );
        for started in session.started.iter().take(12) {
            let mut what = role_label(started.role).to_string();
            if let Some(port) = started.port {
                what = format!("{what} · {} {port}", l("port", "port"));
            }
            if started.cpu_pct >= 1. {
                what = format!("{what} · {:.0}% CPU", started.cpu_pct);
            }
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
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
    }
    let targets: Vec<_> = session.tree.iter().filter(|p| control::blocked(p).is_none()).cloned().collect();
    if !targets.is_empty() && !pending {
        let action = Action::Processes { name: title, targets, force: Force::Quit };
        body = body.child(div().pt_1().flex().justify_end().child(button(
            format!("quit-session-{}", session.root.pid),
            l("Quit session…", "Avslutt økten…"),
            false,
            dark,
            move |window, cx| crate::process_panel::request(action.clone(), this.clone(), window, cx),
        )));
    }
    body
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let s = &view.snap;
    let now = now();
    let mut page = div().flex().flex_col().gap_3();
    if let Some(message) = &view.process_message {
        page = page.child(card(dark).child(sub(message.clone()).text_color(if view.process_failed {
            t::warn(dark)
        } else {
            t::ink(dark)
        })));
    }

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
    page = page.child(section_header(
        l("Running agents", "Agenter som kjører"),
        &if s.agents.is_empty() { String::new() } else { format!("{} · {}", s.agents.len(), fmt_bytes(total as f64)) },
        dark,
    ));
    if s.agents.is_empty() {
        return page.child(empty_state(
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
    for session in &s.agents {
        page = page.child(session_card(session, now, view.process_pending, dark, this.clone()));
    }
    page
}
