//! App-scoped process inspection and explicit, confirmed termination requests.

use crate::controls::{button, segment, segment_group};
use crate::engine::fmt_bytes;
use crate::locale::text as l;
use crate::process_activity::{self as activity, ProcessRow};
use crate::processes::ProcInfo;
use crate::process_signing::{Failure, Key};
use crate::theme as t;
use crate::ui::{card, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn open(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |view, cx| {
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

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut panel = div().flex().flex_col().gap_3();
    let mut filters =
        segment_group("process-filters", l("Process filter", "Prosessfilter"), dark);
    for (remaining, label) in [
        (true, l("Remaining helpers", "Gjenværende hjelpeprosesser")),
        (false, l("All apps", "Alle apper")),
    ] {
        let target = this.clone();
        filters = filters.child(segment(
            format!("process-filter-{remaining}"),
            label,
            view.only_remaining == remaining,
            dark,
            t::accent(dark),
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
        "Gjenværende betyr at en prosess overlevde en appavslutning observert i denne økten—ikke at den er uønsket. Påloggingsobjekter og tjenester kan kjøre med hensikt eller starte på nytt. Prosesser frakoblet før overvåkingen startet, kan ikke tilordnes sikkert."
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
        let summary = format!(
            "{} · {} · {} CPU · {}",
            group.name,
            crate::locale::count(process_count as u64, "process", "processes", "prosess", "prosesser"),
            crate::locale::percent(cpu, 1),
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
                                    "{} · PID {} · {} CPU · {}",
                                    p.comm,
                                    p.pid,
                                    crate::locale::percent(p.cpu_pct, 1),
                                    fmt_bytes(p.mem_bytes as f64)
                                )),
                        )
                        .child(sub(provenance))
                        .child(sub(format!(
                            "UID {} · {} {} · {} {}",
                            p.uid,
                            l("Current parent PID", "Nåværende forelder-PID"),
                            p.ppid,
                            l("Started", "Startet"),
                            crate::history_browser::local_time(p.start_unix)
                        )))
                        .child(sub(if p.ppid == 1 {
                            l("Parent PID 1 does not establish an orphan or unwanted process.", "Forelder-PID 1 beviser ikke en foreldreløs eller uønsket prosess.")
                        } else {
                            l("The current parent is a sampled fact, not necessarily the original launching app.", "Nåværende forelder er en målt opplysning, ikke nødvendigvis appen som opprinnelig startet prosessen.")
                        }))
                        .child(sub(p.exe_path.clone())),
                );
                let matching: Vec<_> = view.tools.startup.as_ref().map(|inventory| {
                    inventory.entries.iter().filter(|entry| !p.exe_path.is_empty() && entry.executable.as_deref()==Some(p.exe_path.as_str()))
                        .map(|entry|entry.label.clone()).take(5).collect()
                }).unwrap_or_default();
                let association = if matching.is_empty() {
                    l("No exact startup executable match in the available inventory. This does not rule out other launch mechanisms.", "Ingen nøyaktig samsvarende oppstartsfil i den tilgjengelige oversikten. Dette utelukker ikke andre oppstartsmekanismer.").to_string()
                } else {
                    format!("{}: {}. {}", l("Possible startup association", "Mulig oppstartstilknytning"), matching.join(", "), l("Matching paths do not prove how this process was launched.", "Samsvarende stier beviser ikke hvordan prosessen ble startet."))
                };
                if !p.exe_path.is_empty() {
                    let path=std::path::PathBuf::from(&p.exe_path);
                    details=details.child(button(format!("process-reveal-{}",p.pid),l("Reveal", "Vis i Finder"),false,dark,move |_,_|crate::storage::reveal(&path)));
                }
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
                        .child(details)
                        .child(signing_details(view, p, dark, this.clone()))
                        .child(sub(association).text_color(t::ink_dim(dark))),
                );
            }
            content = content.child(sub(l("Quit asks the app to close normally. Stop is available only for verified remaining helpers owned by you. Protected, unknown, or active-app processes cannot be stopped here.", "Avslutt ber appen lukke normalt. Stopp er kun tilgjengelig for bekreftede gjenværende hjelpeprosesser som tilhører deg. Beskyttede, ukjente eller aktive approsesser kan ikke stoppes her.")));
        }
        panel = panel.child(content);
    }
    panel
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
