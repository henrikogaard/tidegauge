//! Agents: what is left of your AI plans (read from OpenQuota) and the
//! coding agents running now, with what each one started.

use crate::agents::{Activity, Pace, Plan, Plans, Role, Session, Spent, Window};
use crate::app_cpu::AppCpu;
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
        "openai" => "OpenAI".into(),
        "opencode-console" => "OpenCode".into(),
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

/// The provider, plus the account's own name when there are two of it:
/// "Claude · Desktop".
pub(crate) fn plan_name(plan: &Plan) -> String {
    let provider = provider_name(&plan.provider);
    match plan.account.as_deref() {
        Some(account) if !account.eq_ignore_ascii_case(&provider) => format!("{provider} · {account}"),
        _ => provider,
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

/// A currency code, or no unit at all (OpenQuota's money windows).
fn is_money(unit: &str) -> bool {
    unit.is_empty() || (unit.len() == 3 && unit.chars().all(|c| c.is_ascii_uppercase()))
}

/// The amount as it will be shown, so its sign and colour agree with the text.
fn rounded(amount: f64, unit: &str) -> f64 {
    let scale = if is_money(unit) { 100. } else { 1. };
    (amount * scale).round() / scale
}

/// An amount as people write it: money gets cents, other credits whole
/// numbers, a real debt a minus sign, and a rounding never reads "-0".
fn amount_text(amount: f64, unit: &str) -> String {
    let rounded = rounded(amount, unit);
    let digits = if is_money(unit) {
        crate::locale::number(rounded.abs(), 2)
    } else {
        crate::locale::whole(rounded.abs())
    };
    if rounded < 0. {
        format!("\u{2212}{digits}")
    } else {
        digits
    }
}

/// An amount small enough for the inside of a ring: no cents from 100 up.
fn short_amount(amount: f64, unit: &str) -> String {
    if amount.abs() >= 100. {
        amount_text(amount.trunc(), "credits")
    } else {
        amount_text(amount, unit)
    }
}

/// What is left of a plan's credits: "9.17 USD left", or "−0.07 USD" when
/// the balance is overdrawn.
fn credits_left(amount: f64, unit: &str) -> String {
    let value = amount_text(amount, unit);
    if value.starts_with('\u{2212}') {
        format!("{value} {unit}")
    } else {
        format!("{value} {unit} {}", l("left", "igjen"))
    }
}

/// OpenQuota's period name ("Spend 30d") in words: "last 30 days".
fn spent_period(spent: &Spent) -> String {
    let label = spent.label.to_lowercase();
    if label.contains("30d") || label.contains("30 d") {
        l("last 30 days", "siste 30 dager").into()
    } else if label.contains("7d") || label.contains("7 d") || label.contains("week") {
        l("last 7 days", "siste 7 dager").into()
    } else {
        spent.label.clone()
    }
}

/// "60.24 USD"
fn spent_amount(spent: &Spent) -> String {
    format!("{} {}", amount_text(spent.amount, &spent.unit), spent.unit).trim_end().to_string()
}

/// What a plan without a usage window shows instead of a percentage.
enum Money<'a> {
    Balance(f64, &'a str),
    Spent(&'a Spent),
}

fn money(plan: &Plan) -> Option<Money<'_>> {
    if plan.tightest().is_some() {
        return None;
    }
    match (&plan.credits, &plan.spent) {
        (Some((amount, unit)), _) => Some(Money::Balance(rounded(*amount, unit), unit)),
        (None, Some(spent)) => Some(Money::Spent(spent)),
        (None, None) => None,
    }
}

/// Stale, failed or too old to trust: show its age, never a pace.
fn plan_not_updating(plan: &Plan, now: f64) -> bool {
    plan.stale || plan.failed || plan.fetched_at.is_none_or(|at| now - at > FRESH_SECS)
}

/// A plan fetched longer ago than this, or never, is left off the glance.
const HIDE_SECS: f64 = 86_400.;

/// Whether a plan appears at a glance: fetched at some point in the last 24
/// hours, even if it is not updating now (it is then greyed out).
fn plan_shown(plan: &Plan, now: f64) -> bool {
    plan.fetched_at.is_some_and(|at| now - at <= HIDE_SECS)
}

/// The plans worth showing at a glance (the Overview strip and the menu-bar
/// panel). The Agents page still lists every plan.
pub(crate) fn current_plans(plans: &[Plan], now: f64) -> Vec<&Plan> {
    plans.iter().filter(|plan| plan_shown(plan, now)).collect()
}

/// "as of 12:01" for a reading from today, "as of Tue 12:01" for an older one.
fn as_of(at: f64, now: f64) -> String {
    use crate::history_browser::{local_clock, local_day, local_weekday};
    let when = if local_day(at) == local_day(now) {
        local_clock(at)
    } else {
        format!(
            "{} {}",
            crate::locale::weekday(local_weekday(at)),
            local_clock(at)
        )
    };
    format!("{} {when}", l("as of", "per"))
}

/// Plan rings for the Overview: one compact chip per provider, opening Agents.
/// A plan at a glance: how much is left, in which colour, and one word on
/// how it is going. Shared by the Overview strip and the menu-bar panel.
pub(crate) struct PlanGlance {
    pub name: String,
    pub left: Option<f64>,
    /// The chip's reading: "54% left", "9.17 USD left" or "60.24 USD".
    pub headline: String,
    /// The same, short, for the menu-bar panel's right-hand column.
    pub value: String,
    pub color: Hsla,
    pub ring: Vec<(f64, Hsla)>,
    pub note: String,
    /// The provider name: dimmed along with the ring when not updating.
    pub name_color: Hsla,
    /// The note stands out only when the plan is running out.
    pub note_color: Hsla,
}

pub(crate) fn plan_glance(plan: &Plan, now: f64, dark: bool) -> PlanGlance {
    let tight = plan.tightest();
    let left = tight.map(|w| 1. - w.used);
    let old = plan_not_updating(plan, now);
    let pace = tight.map(|w| crate::agents::pace(w, now)).unwrap_or(Pace::Unknown);
    let money = money(plan);
    let color = if old {
        t::ink_dim(dark)
    } else {
        match (&money, &pace, left) {
            (Some(Money::Balance(amount, _)), _, _) if *amount <= 0. => t::warn(dark),
            (Some(Money::Balance(..)), _, _) => t::ok(dark),
            (Some(Money::Spent(_)), _, _) => t::ink(dark),
            (None, Pace::UsedUp, _) => t::warn(dark),
            (None, Pace::RunsOut { .. }, _) => t::caution(dark),
            (None, _, Some(left)) if left < 0.15 => t::caution(dark),
            _ => t::ok(dark),
        }
    };
    let ring = match (&money, left) {
        // A balance or a spend has no share to draw: a full ring in its colour.
        (Some(Money::Balance(..)), _) => vec![(1., color)],
        (Some(Money::Spent(_)), _) => vec![(1., t::ink_dim(dark))],
        (None, Some(left)) => vec![(left, color), (1. - left, t::card_hi(dark))],
        (None, None) => Vec::new(),
    };
    let note = match (old, &money, &pace) {
        (true, _, _) => plan.fetched_at.map(|at| as_of(at, now)).unwrap_or_default(),
        (false, Some(Money::Balance(amount, _)), _) if *amount < 0. => l("overdrawn", "overtrukket").to_string(),
        (false, Some(Money::Balance(..)), _) => l("balance", "saldo").to_string(),
        (false, Some(Money::Spent(spent)), _) => format!("{} · {}", l("spent", "brukt"), spent_period(spent)),
        (false, None, Pace::RunsOut { in_secs }) => format!("{} {}", l("out in", "tom om"), duration(*in_secs)),
        (false, None, Pace::UsedUp) => l("used up", "brukt opp").to_string(),
        (false, None, Pace::OnPace { .. }) => l("on pace", "i rute").to_string(),
        (false, None, Pace::Unknown) => tight.map(|w| window_title(&w.label)).unwrap_or_default(),
    };
    let (headline, value) = match (&money, left) {
        (Some(Money::Balance(amount, unit)), _) => {
            (credits_left(*amount, unit), format!("{} {unit}", amount_text(*amount, unit)).trim_end().to_string())
        }
        (Some(Money::Spent(spent)), _) => (spent_amount(spent), spent_amount(spent)),
        (None, left) => (
            format!("{} {}", left.map(percent).unwrap_or_else(|| "–".into()), l("left", "igjen")),
            left_text(left),
        ),
    };
    let note_color = if matches!(pace, Pace::RunsOut { .. }) && !old { color } else { t::ink_dim(dark) };
    let name_color = if old { t::ink_dim(dark) } else { t::ink(dark) };
    PlanGlance { name: plan_name(plan), left, headline, value, color, ring, note, name_color, note_color }
}

pub(crate) fn left_text(left: Option<f64>) -> String {
    left.map(percent).unwrap_or_default()
}

/// What a screen reader says for a plan: a plan that is not updating also
/// says "as of …", so its percentage is not heard as current.
fn plan_label(name: &str, headline: &str, note: &str, old: bool) -> String {
    let base = format!("{name} {headline}");
    if old && !note.is_empty() {
        format!("{base}, {note}")
    } else {
        base
    }
}

pub fn plan_strip(plans: &Plans, dark: bool, this: Entity<TideGaugeView>) -> Option<Div> {
    let Plans::Loaded(plans) = plans else { return None };
    let now = now();
    let plans = current_plans(plans, now);
    if plans.is_empty() {
        return None;
    }
    let mut strip = div().flex().flex_wrap().gap_2();
    for (index, plan) in plans.into_iter().enumerate() {
        let PlanGlance { name, headline, color, ring, note, name_color, note_color, .. } = plan_glance(plan, now, dark);
        let old = plan_not_updating(plan, now);
        let target = this.clone();
        let keyboard = this.clone();
        let provider = plan.provider.clone();
        strip = strip.child(
            div()
                // Two accounts of one provider each need their own id.
                .id(SharedString::from(format!("plan-chip-{provider}-{index}")))
                .role(gpui_kit::Role::Button)
                .aria_label(SharedString::from(plan_label(&name, &headline, &note, old)))
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
                .bg(t::window_card(dark))
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
                                .child(div().font_weight(FontWeight::SEMIBOLD).text_color(name_color).child(name))
                                .child(div().text_color(color).child(headline)),
                        )
                        .child(div().text_size(t::TEXT_SMALL).text_color(note_color).child(note)),
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
    let pace = tight.map(|w| crate::agents::pace(w, now)).unwrap_or(Pace::Unknown);
    let old = plan_not_updating(plan, now);
    let money = money(plan);
    let PlanGlance { left, color, ring, .. } = plan_glance(plan, now, dark);
    // Inside the ring: the share left, or the amount without its unit.
    let centre = match &money {
        Some(Money::Balance(amount, unit)) => short_amount(*amount, unit),
        Some(Money::Spent(spent)) => short_amount(spent.amount, &spent.unit),
        None => left.map(percent).unwrap_or_else(|| "–".into()),
    };
    let status: Option<(String, Hsla)> = match (old, &pace, tight) {
        (false, _, None) => match &money {
            Some(Money::Balance(amount, unit)) => Some((credits_left(*amount, unit), color)),
            Some(Money::Spent(spent)) => Some((spent_amount(spent), t::ink(dark))),
            None => None,
        },
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
                        .child(centre),
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
                        .child(div().text_size(t::TEXT_ROW).font_weight(FontWeight::SEMIBOLD).child(plan_name(plan)))
                        .when_some(plan.plan.clone(), |d, name| {
                            d.child(div().text_xs().text_color(t::ink_dim(dark)).child(name))
                        }),
                )
                // Three lines at most, none wrapping: which window, how it is
                // going, and everything else in one quiet line.
                .child(sub(match (tight, &money) {
                    (Some(w), _) => window_title(&w.label),
                    (None, Some(Money::Balance(..))) => l("Balance", "Saldo").into(),
                    (None, Some(Money::Spent(spent))) => format!("{}, {}", l("Spent", "Brukt"), spent_period(spent)),
                    (None, None) => l("No usage window reported", "Ingen bruksperiode rapportert").into(),
                }).truncate().text_color(t::ink_dim(dark)))
                .when_some(status, |d, (text, color)| d.child(sub(text).truncate().text_color(color)))
                .map(|d| {
                    let mut rest = others.clone();
                    // Whatever the headline did not already say.
                    if tight.is_some() {
                        rest.extend(plan.credits.clone().map(|(amount, unit)| credits_left(amount, &unit)));
                    }
                    if !matches!(money, Some(Money::Spent(_))) {
                        rest.extend(plan.spent.as_ref().map(|spent| format!("{} {}", spent_period(spent), spent_amount(spent))));
                    }
                    d.when(!rest.is_empty(), |d| d.child(sub(rest.join(" · ")).truncate().text_color(t::ink_dim(dark))))
                })
                .when(plan.failed, |d| {
                    d.child(sub(l("OpenQuota's last refresh failed", "OpenQuotas siste oppdatering mislyktes")).text_color(t::caution(dark)))
                }),
        )
}

/// What the Agents page's numbers mean, behind the header's ⓘ.
pub(crate) fn notes() -> Vec<&'static str> {
    vec![
        l(
            "Plan usage is read from OpenQuota's saved data on this Mac. Tidegauge never signs in to your accounts.",
            "Abonnementsbruk leses fra OpenQuotas lagrede data på denne Macen. Tidegauge logger aldri inn på kontoene dine.",
        ),
        l(
            "Pace compares how much you have used with how much of the period has passed.",
            "Tempo sammenligner hvor mye du har brukt med hvor mye av perioden som har gått.",
        ),
        l(
            "A session counts as working while it is busy or using CPU.",
            "En økt regnes som i arbeid mens den er opptatt eller bruker CPU.",
        ),
    ]
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
fn session_row(session: &Session, now: f64, expanded: bool, pending: bool, cpu: AppCpu, dark: bool, this: Entity<TideGaugeView>) -> Div {
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
        // A bot glyph; a green dot on its corner while the session works.
        .child(
            div()
                .relative()
                .size(px(24.))
                .flex_shrink_0()
                .rounded(t::RADIUS_CHIP)
                .bg(t::card_hi(dark))
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path("icons/bot.svg").size(px(14.)).text_color(if working { t::ink(dark) } else { t::ink_dim(dark) }))
                .when(working, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(-2.))
                            .bottom(px(-2.))
                            .size(px(9.))
                            .rounded_full()
                            .border_2()
                            .border_color(t::card(dark))
                            .bg(t::ok(dark)),
                    )
                }),
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
        // Working stands out as a chip; idle is plain text.
        .child(
            div()
                .flex_shrink_0()
                .px_2()
                .rounded(t::RADIUS_CHIP)
                .when(working, |d| d.bg(chip_color.opacity(0.14)))
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
                .child(if cpu.value(session.cpu_pct) >= 0.5 { cpu.format(session.cpu_pct) } else { String::new() }),
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
        if cpu.value(started.cpu_pct) >= 0.5 {
            what = format!("{what} · {} CPU", cpu.format(started.cpu_pct));
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
    let cpu = AppCpu::chosen(s.chip.cores);
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
        list = list.child(session_row(session, now, expanded, view.process_pending, cpu, dark, this.clone()));
    }
    page.child(list)
}

#[cfg(test)]
mod tests {
    // Named imports: the UI kit's glob export includes its own `test`.
    use super::{as_of, credits_left, current_plans, plan_not_updating, Plan, FRESH_SECS};
    use crate::locale::{number, text as l, whole};

    #[test]
    fn credits_read_as_people_write_them() {
        let left = l("left", "igjen");
        assert_eq!(credits_left(57602., "credits"), format!("{} credits {left}", whole(57602.)));
        assert_eq!(credits_left(-0.3, "USD"), format!("\u{2212}{} USD", number(0.3, 2)), "a debt keeps its sign, without 'left'");
        assert_eq!(credits_left(-0.001, "USD"), format!("{} USD {left}", number(0., 2)), "never -0");
        assert_eq!(credits_left(12.5, "USD"), format!("{} USD {left}", number(12.5, 2)));
        assert_eq!(credits_left(123.45, "USD"), format!("{} USD {left}", number(123.45, 2)), "cents at any size");
    }

    #[test]
    fn failed_or_stale_plans_never_show_a_pace() {
        let plan = |stale, failed, fetched_at| Plan {
            provider: "claude".into(),
            plan: None,
            windows: Vec::new(),
            credits: None,
            spent: None,
            account: None,
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

    #[test]
    fn plans_are_hidden_only_when_never_fetched_or_over_a_day_old() {
        let plan = |provider: &str, stale, failed, fetched_at| Plan {
            provider: provider.into(),
            plan: None,
            windows: Vec::new(),
            credits: None,
            spent: None,
            account: None,
            fetched_at,
            stale,
            failed,
        };
        let now = 200_000.;
        let plans = vec![
            plan("fresh", false, false, Some(now - 60.)),
            plan("stale", true, false, Some(now - 60.)),
            plan("idle", false, false, Some(now - FRESH_SECS - 1.)),
            plan("broken", false, true, Some(now - 60.)),
            plan("day", false, false, Some(now - 86_400.)),
            plan("ancient", false, false, Some(now - 86_401.)),
            plan("never", true, false, None),
        ];
        let shown: Vec<&str> = current_plans(&plans, now).iter().map(|p| p.provider.as_str()).collect();
        assert_eq!(shown, ["fresh", "stale", "idle", "broken", "day"]);
        let greyed: Vec<&str> =
            current_plans(&plans, now).iter().filter(|p| plan_not_updating(p, now)).map(|p| p.provider.as_str()).collect();
        assert_eq!(greyed, ["stale", "idle", "broken", "day"], "only the fresh one keeps its colours");
        assert!(current_plans(&plans[5..], now).is_empty(), "nothing recent: no section");
        assert!(current_plans(&[], now).is_empty());
    }

    #[test]
    fn money_accounts_show_their_balance_or_spend_instead_of_a_dash() {
        use crate::agents::Spent;
        let now = 10_000.;
        let plan = |provider: &str, credits: Option<(f64, &str)>, spent: Option<Spent>| Plan {
            provider: provider.into(),
            plan: None,
            windows: Vec::new(),
            credits: credits.map(|(a, u)| (a, u.to_string())),
            spent,
            account: None,
            fetched_at: Some(now - 60.),
            stale: false,
            failed: false,
        };
        let left = l("left", "igjen");
        let openrouter = super::plan_glance(&plan("openrouter", Some((9.17, "USD")), None), now, true);
        assert_eq!(openrouter.headline, format!("{} USD {left}", number(9.17, 2)));
        assert_eq!(openrouter.value, format!("{} USD", number(9.17, 2)));
        assert_eq!(openrouter.ring.len(), 1, "a full ring, not an empty one");
        let spend = Spent { amount: 60.24, unit: "USD".into(), label: "Spend 30d".into() };
        let openai = super::plan_glance(&plan("openai", None, Some(spend)), now, true);
        assert_eq!(openai.name, "OpenAI");
        assert_eq!(openai.headline, format!("{} USD", number(60.24, 2)));
        assert!(openai.note.contains(l("last 30 days", "siste 30 dager")), "{}", openai.note);
        let devin = super::plan_glance(&plan("devin", Some((-0.07, "USD")), None), now, true);
        assert_eq!(devin.note, l("overdrawn", "overtrukket"));
        assert!(!devin.headline.contains(left));
        let crumb = super::plan_glance(&plan("devin", Some((-0.001, "USD")), None), now, true);
        assert_eq!(crumb.note, l("balance", "saldo"), "rounds to zero: not overdrawn");
        let unitless = Spent { amount: 60.24, unit: String::new(), label: "Spend 30d".into() };
        assert_eq!(super::spent_amount(&unitless), number(60.24, 2), "cents kept, no trailing space");
        let nothing = super::plan_glance(&plan("grok", None, None), now, true);
        assert_eq!(nothing.headline, format!("– {left}"));
    }

    #[test]
    fn a_second_account_is_named_after_its_label() {
        let named = |provider: &str, account: Option<&str>| Plan {
            provider: provider.into(),
            plan: None,
            windows: Vec::new(),
            credits: None,
            spent: None,
            account: account.map(Into::into),
            fetched_at: None,
            stale: false,
            failed: false,
        };
        assert_eq!(super::plan_name(&named("claude", Some("Desktop"))), "Claude · Desktop");
        assert_eq!(super::plan_name(&named("claude", Some("Claude"))), "Claude", "no 'Claude · Claude'");
        assert_eq!(super::plan_name(&named("opencode-console", None)), "OpenCode");
    }

    #[test]
    fn weekday_names_follow_the_ui_language() {
        let names: Vec<&str> = (0..7).map(crate::locale::weekday).collect();
        if crate::locale::is_norwegian() {
            assert_eq!(names, ["søn", "man", "tir", "ons", "tor", "fre", "lør"]);
        } else {
            assert_eq!(names, ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]);
        }
        // 2025-10-14 12:00 UTC is a Tuesday in every time zone.
        assert_eq!(crate::history_browser::local_weekday(1_760_443_200.), 2);
    }

    #[test]
    fn a_plan_not_updating_says_so_to_a_screen_reader() {
        let fresh = super::plan_label("Claude", "50% left", "on pace", false);
        assert!(!fresh.contains("on pace"));
        let old = super::plan_label("Claude", "50% left", "as of 12:01", true);
        assert!(old.ends_with(", as of 12:01"), "{old}");
    }

    #[test]
    fn as_of_gives_the_clock_today_and_the_weekday_on_older_days() {
        use crate::history_browser::{local_clock, local_weekday};
        // Noon-ish UTC keeps "today" and "two days ago" apart in any zone.
        let now = 1_760_000_000.;
        let earlier = now - 2. * 86_400.;
        assert_eq!(as_of(now, now), format!("{} {}", l("as of", "per"), local_clock(now)));
        let old = as_of(earlier, now);
        let when = format!("{} {}", crate::locale::weekday(local_weekday(earlier)), local_clock(earlier));
        assert_eq!(old, format!("{} {when}", l("as of", "per")));
        assert!(old.len() > as_of(now, now).len(), "weekday adds a word: {old}");
    }
}
