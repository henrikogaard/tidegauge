//! The "Why is my Mac slow?" card on Overview: the findings from
//! `diagnose`, each with its one action. The check reads the latest reading
//! once when asked and keeps that answer on screen until it is checked again
//! or closed, so a button never moves under the pointer.

use crate::controls::{button, icon_toggle};
use crate::diagnose::{self, Action, Finding, Severity};
use crate::locale::text as l;
use crate::process_control::Force;
use crate::theme as t;
use crate::ui::{caption, card, sub, Destination, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Check the Mac now and show the answer on Overview.
pub fn run(this: &Entity<TideGaugeView>, cx: &mut App) {
    let findings = diagnose::diagnose(&crate::engine::latest());
    this.update(cx, |view, cx| {
        view.slow_check = Some(findings);
        cx.notify();
    });
}

fn close(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.slow_check = None;
        cx.notify();
    });
}

/// The Overview header's button.
pub fn header_button(open: bool, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    button(
        "slow-check",
        l("Why is my Mac slow?", "Hvorfor er Macen min treg?"),
        open,
        dark,
        move |_, cx| run(&this, cx),
    )
}

fn act(action: &Action, dark: bool, this: Entity<TideGaugeView>, index: usize) -> impl IntoElement {
    match action {
        Action::Quit { key, name } => {
            let key = key.clone();
            let label = format!("{} {name}…", l("Quit", "Avslutt"));
            button(
                format!("slow-check-action-{index}"),
                label,
                false,
                dark,
                move |window, cx| {
                    // Look the app up again: it may have quit, or changed, since
                    // the check, and the usual confirmation names what is quit.
                    let request = {
                        let view = this.read(cx);
                        view.snap
                            .apps
                            .iter()
                            .find(|app| app.key == key)
                            .and_then(|app| {
                                crate::process_panel::quit_action(&view.snap, app, Force::Quit)
                            })
                    };
                    match request {
                        Some(request) => {
                            crate::process_panel::request(request, this.clone(), window, cx)
                        }
                        None => this.update(cx, |view, cx| {
                            view.toast = Some(crate::ui::Toast::new(
                                l(
                                    "That app has already quit or can't be quit here.",
                                    "Den appen er allerede avsluttet eller kan ikke avsluttes her.",
                                )
                                .into(),
                                true,
                            ));
                            cx.notify();
                        }),
                    }
                },
            )
            .into_any_element()
        }
        Action::Open { to, label } => {
            let to: Destination = *to;
            button(
                format!("slow-check-action-{index}"),
                *label,
                false,
                dark,
                move |_, cx| {
                    close(&this, cx);
                    crate::ui::navigate(&this, to, cx);
                },
            )
            .into_any_element()
        }
    }
}

fn row(index: usize, finding: &Finding, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let (icon, color) = match finding.severity {
        Severity::Ok => ("icons/circle-check.svg", t::ok(dark)),
        Severity::Notice => ("icons/circle-alert.svg", t::caution(dark)),
        Severity::Warning => ("icons/triangle-alert.svg", t::warn(dark)),
    };
    let apps = finding
        .apps
        .iter()
        .map(|app| format!("{} {}", app.name, app.value))
        .collect::<Vec<_>>()
        .join(" · ");
    div()
        .flex()
        .items_start()
        .gap(t::SPACE_3)
        .when(index > 0, |d| {
            d.border_t_1().border_color(t::line(dark)).pt(t::SPACE_3)
        })
        // As tall as the title's line, so the icon sits level with it.
        .child(
            div()
                .h(px(18.))
                .flex()
                .items_center()
                .flex_shrink_0()
                .child(svg().path(icon).size(px(14.)).text_color(color)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(t::SPACE_1)
                .child(
                    div()
                        .text_size(t::TEXT_ROW)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(finding.title),
                )
                .child(sub(finding.text).text_color(t::ink_dim(dark)))
                .when_some(finding.detail.clone(), |d, detail| {
                    d.child(
                        div()
                            .text_size(t::TEXT_SMALL)
                            .text_color(t::ink_dim(dark))
                            .child(detail),
                    )
                })
                .when(!apps.is_empty(), |d| {
                    d.child(div().text_size(t::TEXT_BODY).child(apps))
                })
                .when_some(finding.action.as_ref(), |d, action| {
                    d.child(
                        div()
                            .pt(t::SPACE_1)
                            .child(act(action, dark, this.clone(), index)),
                    )
                }),
        )
}

/// The card: the findings, worst first, and the ways to check again or close.
pub fn card_view(findings: &[Finding], dark: bool, this: Entity<TideGaugeView>) -> Div {
    let again = this.clone();
    let closing = this.clone();
    card(dark)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(caption(
                    l("Why is my Mac slow?", "Hvorfor er Macen min treg?"),
                    dark,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(t::SPACE_2)
                        .child(button(
                            "slow-check-again",
                            l("Check again", "Sjekk på nytt"),
                            false,
                            dark,
                            move |_, cx| run(&again, cx),
                        ))
                        .child(icon_toggle(
                            "slow-check-close",
                            "icons/x.svg",
                            l("Close", "Lukk"),
                            false,
                            dark,
                            move |_, cx| close(&closing, cx),
                        )),
                ),
        )
        .children(
            findings
                .iter()
                .enumerate()
                .map(|(i, finding)| row(i, finding, dark, this.clone())),
        )
}
