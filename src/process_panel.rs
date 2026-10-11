//! App-scoped process inspection and explicit, confirmed termination requests.

use crate::controls::button;
use crate::engine::fmt_bytes;
use crate::locale::text as l;
use crate::process_activity::{self as activity, ProcessRow};
use crate::processes::ProcInfo;
use crate::theme as t;
use crate::ui::{card, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn open(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
        view.show_processes = true;
        view.show_settings = false;
        view.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    });
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let back = this.clone();
    let mut panel = div().flex().flex_col().gap_3().child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(l("Apps and helpers", "Apper og hjelpeprosesser")),
            )
            .child(button(
                "process-back",
                l("Back", "Tilbake"),
                false,
                dark,
                move |_, cx| {
                    back.update(cx, |v, cx| {
                        v.show_processes = false;
                        cx.notify();
                    });
                },
            )),
    );
    let mut filters = div().flex().gap_2();
    for (remaining, label) in [
        (true, l("Remaining helpers", "Gjenværende hjelpeprosesser")),
        (false, l("All apps", "Alle apper")),
    ] {
        let target = this.clone();
        filters = filters.child(button(
            format!("process-filter-{remaining}"),
            label,
            view.only_remaining == remaining,
            dark,
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.only_remaining = remaining;
                    v.expanded_process_group = None;
                    cx.notify();
                });
            },
        ));
    }
    panel = panel.child(card(dark).child(filters).child(sub(l(
        "Remaining means a process survived an app exit observed during this session—not that it is unwanted. Login items and services may run intentionally or restart. Detached processes seen before monitoring began cannot be attributed reliably.",
        "Gjenværende betyr at en prosess overlevde en appavslutning observert i denne økten—ikke at den er uønsket. Påloggingselementer og tjenester kan kjøre med hensikt eller starte på nytt. Prosesser frakoblet før overvåkingen startet, kan ikke tilordnes sikkert."
    ))));
    if let Some(message) = &view.process_message {
        panel = panel.child(card(dark).child(sub(message.clone()).text_color(
            if view.process_failed {
                t::warn(dark)
            } else {
                t::ink(dark)
            },
        )));
    }
    let groups: Vec<_> = view
        .snap
        .process_groups
        .iter()
        .filter(|g| !view.only_remaining || g.remaining_count() > 0)
        .collect();
    if groups.is_empty() {
        return panel.child(card(dark).child(sub(l("No matching app processes observed. This is not proof that no background services exist.", "Ingen samsvarende approsesser observert. Dette beviser ikke at bakgrunnstjenester ikke finnes."))));
    }
    for group in groups {
        let expanded = view.expanded_process_group.as_ref() == Some(&group.bundle);
        let target = this.clone();
        let key = group.bundle.clone();
        let cpu: f64 = group.rows.iter().map(|r| r.process.cpu_pct).sum();
        let memory: f64 = group.rows.iter().map(|r| r.process.mem_bytes as f64).sum();
        let process_count = group.rows.len();
        let process_label = if process_count == 1 {
            l("process", "prosess")
        } else {
            l("processes", "prosesser")
        };
        let summary = format!(
            "{} · {} {} · {:.1}% CPU · {}",
            group.name,
            process_count,
            process_label,
            cpu,
            fmt_bytes(memory)
        );
        let mut row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().child(button(
                format!("process-group-{}", group.bundle),
                format!("{} {}", if expanded { "▾" } else { "▸" }, summary),
                expanded,
                dark,
                move |_, cx| {
                    target.update(cx, |v, cx| {
                        v.expanded_process_group = if expanded { None } else { Some(key.clone()) };
                        cx.notify();
                    });
                },
            )));
        if let [main] = group.mains.as_slice() {
            if activity::controllable(main, &group.bundle) && !view.process_pending {
                let action = Action::Quit(main.clone(), group.bundle.clone());
                let target = this.clone();
                row = row.child(button(
                    format!("quit-app-{}", main.pid),
                    l("Quit app…", "Avslutt app…"),
                    false,
                    dark,
                    move |window, cx| request(action.clone(), target.clone(), window, cx),
                ));
            }
        }
        let mut content = card(dark).child(row);
        if group.mains.len() > 1 {
            content = content.child(sub(l(
                "Multiple app instances are running; quit them from their own app windows.",
                "Flere appinstanser kjører; avslutt dem fra sine egne appvinduer.",
            )));
        }
        if group.remaining_count() > 0 {
            content = content.child(sub(format!(
                "{} {}",
                group.remaining_count(),
                l(
                    "remaining after an observed app exit",
                    "gjenværende etter en observert appavslutning"
                )
            )));
        }
        if expanded {
            content = content.child(sub(group.bundle.clone()));
            for row in &group.rows {
                let p = &row.process;
                let is_main = group
                    .mains
                    .iter()
                    .any(|main| activity::Identity::of(main) == activity::Identity::of(p));
                let provenance = if is_main {
                    l("Main app process", "Appens hovedprosess").into()
                } else {
                    match &row.origin {
                        Some(o) if o.observed_ancestor => format!(
                            "{} {} · {} {}",
                            l("Previously observed under", "Tidligere observert under"),
                            group.name,
                            l("app PID", "app-PID"),
                            o.main.pid
                        ),
                        Some(_) | None => l(
                            "Associated by bundle path; app lifecycle unknown",
                            "Tilknyttet via appsti; appens livssyklus ukjent",
                        )
                        .into(),
                    }
                };
                let mut details = div().flex().items_center().gap_2().child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(format!(
                                    "{} · PID {} · {:.1}% CPU · {}",
                                    p.comm,
                                    p.pid,
                                    p.cpu_pct,
                                    fmt_bytes(p.mem_bytes as f64)
                                )),
                        )
                        .child(sub(provenance))
                        .child(sub(p.exe_path.clone())),
                );
                if !is_main
                    && row.remaining
                    && activity::controllable(p, &group.bundle)
                    && !view.process_pending
                {
                    let target = this.clone();
                    let action = Action::Stop(row.clone(), group.bundle.clone());
                    details = details.child(button(
                        format!("stop-helper-{}", p.pid),
                        l("Stop…", "Stopp…"),
                        false,
                        dark,
                        move |window, cx| request(action.clone(), target.clone(), window, cx),
                    ));
                }
                content = content.child(
                    div()
                        .border_t_1()
                        .border_color(t::line(dark))
                        .pt_3()
                        .child(details),
                );
            }
            content = content.child(sub(l("Quit asks the app to close normally. Stop is available only for verified remaining helpers owned by you. Protected, unknown, or active-app processes cannot be stopped here.", "Avslutt ber appen lukke normalt. Stopp er kun tilgjengelig for bekreftede gjenværende hjelpeprosesser som tilhører deg. Beskyttede, ukjente eller aktive approsesser kan ikke stoppes her.")));
        }
        panel = panel.child(content);
    }
    panel
}

#[derive(Clone)]
enum Action {
    Quit(ProcInfo, String),
    Stop(ProcessRow, String),
}

fn request(action: Action, this: Entity<TideGaugeView>, window: &mut Window, cx: &mut App) {
    if this.read(cx).process_pending {
        return;
    }
    let (process, bundle, quit) = match &action {
        Action::Quit(p, b) => (p, b, true),
        Action::Stop(r, b) => (&r.process, b, false),
    };
    let detail = format!(
        "{} · PID {}\n{}\n{}\n\n{}",
        process.comm,
        process.pid,
        process.exe_path,
        bundle,
        if quit {
            l("Ask this app to quit normally. It may ask you to save work; helpers may remain. No force quit will be used.", "Be denne appen avslutte normalt. Den kan be deg lagre arbeid; hjelpeprosesser kan bli igjen. Tvungen avslutning brukes ikke.")
        } else {
            l("Send SIGTERM to this process only. Unsaved work may be lost and a service may restart it. No other helpers will be stopped.", "Send SIGTERM kun til denne prosessen. Ulagret arbeid kan gå tapt, og en tjeneste kan starte den på nytt. Ingen andre hjelpeprosesser stoppes.")
        }
    );
    this.update(cx, |v, cx| {
        v.process_pending = true;
        cx.notify();
    });
    let answer = window.prompt(
        PromptLevel::Warning,
        if quit {
            l("Quit this app?", "Avslutte denne appen?")
        } else {
            l(
                "Stop this remaining helper?",
                "Stoppe denne gjenværende hjelpeprosessen?",
            )
        },
        Some(&detail),
        &[
            l("Cancel", "Avbryt"),
            if quit {
                l("Quit app", "Avslutt app")
            } else {
                l("Stop process", "Stopp prosess")
            },
        ],
        cx,
    );
    cx.spawn(async move |cx| {
        if answer.await != Ok(1) {
            this.update(cx, |v, cx| {
                v.process_pending = false;
                cx.notify();
            });
            return;
        }
        let result = smol::unblock(move || match action {
            Action::Quit(p, bundle) => activity::quit_app(&p, &bundle),
            Action::Stop(row, bundle) => activity::stop_remaining(&row, &bundle),
        })
        .await;
        this.update(cx, |v, cx| {
            v.process_pending = false;
            v.process_failed = result.is_err();
            v.process_message = Some(match result {
                Ok(s) | Err(s) => s,
            });
            cx.notify();
        });
    })
    .detach();
}
