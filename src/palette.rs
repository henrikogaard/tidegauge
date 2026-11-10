//! The ⌘K command bar (FR-20): every page and every Cleanup action, from
//! the keyboard. Typing filters, ↑↓ pick, Return runs, Esc closes. It reads
//! keys itself while open, so it needs no text field.

use crate::cleanup::Level;
use crate::locale::text as l;
use crate::theme as t;
use crate::ui::{Destination, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Go(Destination),
    Rescan,
    CleanUp,
    Level(Level),
}

struct Command {
    label: String,
    hint: &'static str,
    action: Action,
}

fn commands(view: &TideGaugeView) -> Vec<Command> {
    let mut out: Vec<Command> = crate::ui::every_page(view)
        .into_iter()
        .map(|d| Command { label: d.title().to_string(), hint: l("Page", "Side"), action: Action::Go(d) })
        .collect();
    let cleanup = l("Cleanup", "Opprydding");
    out.push(Command { label: format!("{cleanup}: {}", l("Rescan", "Skann på nytt")), hint: "", action: Action::Rescan });
    out.push(Command { label: format!("{cleanup}: {}", l("Clean up…", "Rydd opp …")), hint: "⌘⌫", action: Action::CleanUp });
    for lv in [Level::Safe, Level::Balanced, Level::Deep] {
        out.push(Command { label: format!("{cleanup}: {} {}", l("Level", "Nivå"), lv.label()), hint: "", action: Action::Level(lv) });
    }
    out
}

/// Commands whose label holds every typed word, in order of appearance.
fn matching(all: Vec<Command>, query: &str) -> Vec<Command> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    all.into_iter().filter(|c| {
        let label = c.label.to_lowercase();
        words.iter().all(|w| label.contains(w))
    }).collect()
}

pub fn toggle(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.palette = if v.palette.is_some() { None } else { Some(Palette::default()) };
        cx.notify();
    });
}

fn run(this: &Entity<TideGaugeView>, action: Action, window: &mut Window, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.palette = None;
        cx.notify();
    });
    let cleanup = Destination::Tool(crate::tools_panel::Page::Cleanup);
    match action {
        Action::Go(d) => crate::ui::navigate(this, d, cx),
        Action::Rescan => {
            crate::ui::navigate(this, cleanup, cx);
            crate::cleanup_panel::start_scan(this, cx);
        }
        Action::CleanUp => {
            crate::ui::navigate(this, cleanup, cx);
            crate::cleanup_panel::clean_up(this, window, cx);
        }
        Action::Level(lv) => {
            crate::ui::navigate(this, cleanup, cx);
            this.update(cx, |v, cx| {
                v.tools.cleanup.level = lv;
                v.tools.cleanup.overrides.clear();
                cx.notify();
            });
        }
    }
}

/// Handle a key while the bar is open; false when it is closed.
pub fn key(this: &Entity<TideGaugeView>, event: &KeyDownEvent, window: &mut Window, cx: &mut App) -> bool {
    let Some(p) = this.read(cx).palette.clone() else { return false };
    let keys = &event.keystroke;
    if keys.modifiers.platform && keys.key == "k" {
        return false; // the ⌘K handler closes it
    }
    let found = matching(commands(this.read(cx)), &p.query);
    let mut next = p.clone();
    match keys.key.as_str() {
        "escape" => {
            this.update(cx, |v, cx| {
                v.palette = None;
                cx.notify();
            });
            return true;
        }
        "enter" => {
            if let Some(c) = found.get(p.selected.min(found.len().saturating_sub(1))) {
                run(this, c.action, window, cx);
            }
            return true;
        }
        "down" => next.selected = (p.selected + 1).min(found.len().saturating_sub(1)),
        "up" => next.selected = p.selected.saturating_sub(1),
        "backspace" => {
            next.query.pop();
            next.selected = 0;
        }
        _ => {
            let typed = keys.key_char.clone().filter(|_| !keys.modifiers.platform && !keys.modifiers.control);
            match typed {
                Some(text) if !text.chars().any(char::is_control) => {
                    next.query.push_str(&text);
                    next.selected = 0;
                }
                _ => return !keys.modifiers.platform,
            }
        }
    }
    this.update(cx, |v, cx| {
        v.palette = Some(next);
        cx.notify();
    });
    true
}

pub fn overlay(p: &Palette, view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Stateful<Div> {
    let found = matching(commands(view), &p.query);
    let selected = p.selected.min(found.len().saturating_sub(1));
    let close = this.clone();
    let mut list = div().flex().flex_col().py(t::SPACE_1);
    for (i, c) in found.iter().take(12).enumerate() {
        let target = this.clone();
        let action = c.action;
        list = list.child(
            div()
                .id(SharedString::from(format!("palette-{i}")))
                .role(Role::ListBoxOption)
                .aria_selected(i == selected)
                .px(t::SPACE_3)
                .py(t::SPACE_1)
                .mx(t::SPACE_1)
                .rounded(t::RADIUS_CONTROL)
                .flex()
                .justify_between()
                .cursor_pointer()
                .when(i == selected, |d| d.bg(t::card_hi(dark)))
                .child(div().text_size(t::TEXT_ROW).child(c.label.clone()))
                .child(div().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).child(c.hint))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    run(&target, action, window, cx)
                }),
        );
    }
    if found.is_empty() {
        list = list.child(div().px(t::SPACE_4).py(t::SPACE_2).text_size(t::TEXT_BODY).text_color(t::ink_dim(dark)).child(l("No matching command", "Ingen kommando passer")));
    }
    div()
        .id("palette-scrim")
        .absolute()
        .inset_0()
        .bg(gpui_kit::black().opacity(0.25))
        .flex()
        .justify_center()
        .pt(px(80.))
        .on_click(move |_, _, cx| {
            close.update(cx, |v, cx| {
                v.palette = None;
                cx.notify();
            })
        })
        .child(
            div()
                .id("palette")
                .role(Role::Dialog)
                .aria_label(l("Command bar", "Kommandolinje"))
                .w(px(460.))
                .max_h(px(420.))
                .bg(t::window_card(dark))
                .rounded(t::RADIUS_CARD)
                .border_1()
                .border_color(t::line(dark))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(t::SPACE_4)
                        .py(t::SPACE_3)
                        .border_b_1()
                        .border_color(t::line(dark))
                        .text_size(t::TEXT_EMPH)
                        .when(p.query.is_empty(), |d| d.text_color(t::ink_dim(dark)).child(l("Type a page or action…", "Skriv en side eller handling …")))
                        .when(!p.query.is_empty(), |d| d.child(p.query.clone())),
                )
                .child(list),
        )
}

#[cfg(test)]
mod tests {
    use super::{matching, Action, Command};

    fn cmd(label: &str) -> Command {
        Command { label: label.into(), hint: "", action: Action::Rescan }
    }

    #[test]
    fn filters_by_every_word() {
        let all = vec![cmd("Cleanup: Rescan"), cmd("Cleanup: Clean up…"), cmd("CPU")];
        let got: Vec<String> = matching(all, "clean res").into_iter().map(|c| c.label).collect();
        assert_eq!(got, vec!["Cleanup: Rescan"]);
        assert_eq!(matching(vec![cmd("CPU")], "").len(), 1);
    }
}
