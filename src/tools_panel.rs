use crate::controls::{button, segment, segment_group};
use crate::locale::text as l;
use crate::theme as t;
use crate::ui::{card, empty_state, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivitySort {
    #[default]
    Cpu,
    Disk,
    InterruptWakeups,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Startup,
    Activity,
    Baselines,
    Storage,
    Packages,
}
impl Page {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Startup => l("Startup", "Oppstart"),
            Self::Activity => l("Recent activity", "Nylig aktivitet"),
            Self::Baselines => l("Before & after", "Før og etter"),
            Self::Storage => l("Folder sizes", "Mappestørrelser"),
            Self::Packages => l("Apps & packages", "Apper og pakker"),
        }
    }
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Activity => "activity",
            Self::Baselines => "baselines",
            Self::Storage => "storage",
            Self::Packages => "packages",
        }
    }
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Startup => "icons/power.svg",
            Self::Activity => "icons/activity.svg",
            Self::Baselines => "icons/git-compare-arrows.svg",
            Self::Storage => "icons/folder-tree.svg",
            Self::Packages => "icons/package.svg",
        }
    }
}
pub struct State {
    pub page: Page,
    /// The page whose "About this data" notes are expanded.
    pub about: Option<Page>,
    pub startup: Option<crate::startup::Inventory>,
    pub startup_busy: bool,
    pub message: Option<String>,
    pub baseline: Option<crate::baselines::Summary>,
    pub comparison: Option<crate::baselines::Summary>,
    pub baseline_busy: bool,
    pub baseline_read_failed: bool,
    pub minutes: u32,
    pub activity_sort: ActivitySort,
    pub scan: Option<crate::storage::Scan>,
    pub packages: Option<crate::packages::Inventory>,
    pub packages_busy: bool,
    pub package_source: Option<crate::packages::Source>,
    pub package_sort_source: bool,
    pub package_page: usize,
    pub startup_page: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            page: Page::Startup,
            about: None,
            startup: None,
            startup_busy: false,
            message: None,
            baseline: None,
            comparison: None,
            baseline_busy: false,
            baseline_read_failed: false,
            minutes: 5,
            activity_sort: ActivitySort::Cpu,
            scan: None,
            packages: None,
            packages_busy: false,
            package_source: None,
            package_sort_source: false,
            package_page: 0,
            startup_page: 0,
        }
    }
}

pub fn open_page(this: &Entity<TideGaugeView>, page: Page, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.show_tools = true;
        v.show_agents = false;
        v.show_processes = false;
        v.show_settings = false;
        v.tools.page = page;
        v.tools.message = None;
        // A page always opens with its "About this data" closed.
        v.tools.about = None;
        v.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    });
    match page {
        Page::Startup if this.read(cx).tools.startup.is_none() => refresh_startup(this, cx),
        Page::Baselines => load_baseline(this, cx),
        _ => {}
    }
}

pub fn refresh_startup(this: &Entity<TideGaugeView>, cx: &mut App) {
    if this.read(cx).tools.startup_busy {
        return;
    }
    this.update(cx, |v, cx| {
        v.tools.startup_busy = true;
        v.tools.startup_page = 0;
        cx.notify();
    });
    let this = this.clone();
    cx.spawn(async move |cx| {
        let inventory = smol::unblock(crate::startup::scan).await;
        this.update(cx, |v, cx| {
            v.tools.startup = Some(inventory);
            v.tools.startup_busy = false;
            cx.notify();
        });
    })
    .detach();
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut body = div().flex().flex_col().gap(t::SECTION_GAP);
    if let Some(msg) = &view.tools.message {
        body = body.child(card(dark).child(sub(msg.clone())));
    }
    body.child(match view.tools.page {
        Page::Startup => startup(view, dark, this.clone()),
        Page::Activity => activity(view, dark, this.clone()),
        Page::Baselines => baselines(view, dark, this.clone()),
        Page::Storage => storage(view, dark, this.clone()),
        Page::Packages => packages(view, dark, this.clone()),
    })
    .child(about(view.tools.page, view.tools.about == Some(view.tools.page), dark, this))
}

/// The caveats for a page, kept out of its opening card so the page leads
/// with what it is for.
fn about_notes(page: Page) -> Vec<&'static str> {
    match page {
        Page::Startup => vec![
            l("Partial inventory of legacy launch registrations. A file is not proof of approval or a running process. Modern app background items and managed items belong in System Settings.", "Delvis oversikt over eldre oppstartsregistreringer. En fil beviser ikke godkjenning eller at prosessen kjører. Moderne bakgrunnselementer og administrerte elementer håndteres i Systeminnstillinger."),
        ],
        Page::Activity => vec![
            l("Time-weighted activity over up to 60 observed seconds per process. CPU: 100% is one core. No energy score or battery-drain estimate. New, missing or reset counters remain unavailable.", "Tidsvektet aktivitet over inntil 60 observerte sekunder per prosess. CPU: 100 % er én kjerne. Ingen energipoeng eller anslag for batteriforbruk. Nye, manglende eller nullstilte tellere vises som utilgjengelige."),
            l("Interrupt wakeups are reported kernel counts, not energy use. Package-idle wakeups remain unvalidated and unavailable. Denied or new counters show a dash.", "Avbruddsoppvåkninger er rapporterte kjernetellere, ikke energibruk. Oppvåkninger fra pakkehvile er fortsatt uvaliderte og utilgjengelige. Nye tellere eller tellere uten tilgang vises med en strek."),
            l("System processes may deny access. The list is limited to 8,192 sampled processes; the top 80 are shown.", "Systemprosesser kan nekte tilgang. Listen er begrenset til 8 192 målte prosesser; de 80 øverste vises."),
        ],
        Page::Baselines => vec![
            l("Differences are observations, not proof of a speed boost or causation.", "Forskjeller er observasjoner, ikke bevis på høyere hastighet eller årsakssammenheng."),
            l("Samples are held until the next sample only when the gap is at most 30 seconds. No extrapolation through sleep or missing data. A comparison needs 80% coverage on each side. Power-source and thermal context were not recorded; app-level comparisons are not supported.", "Hver måling gjelder frem til neste måling bare når avstanden er høyst 30 sekunder. Ingen fremskriving gjennom dvale eller manglende data. Sammenligning krever 80 % dekning på hver side. Strømkilde og termisk tilstand ble ikke lagret; sammenligning per app støttes ikke."),
        ],
        Page::Storage => vec![
            l("Read-only metadata scan, no deletion or file downloads. Allocated bytes are not reclaimable space: APFS sharing, snapshots and compression affect totals. Links and other volumes are skipped; hard links are counted once. Access failures and cloud placeholders leave incomplete results.", "Skrivebeskyttet metadataskanning, ingen sletting eller filnedlasting. Tildelte byte er ikke frigjørbar plass: APFS-deling, øyeblikksbilder og komprimering påvirker summene. Lenker og andre volumer hoppes over; harde lenker telles én gang. Tilgangsfeil og skyplassholdere gir ufullstendige resultater."),
            l("Bounded to 100,000 entries, depth 64 and 128 queued folders. Large scans may be partial. Folder totals attribute shared hard links to the first path encountered.", "Begrenset til 100 000 oppføringer, dybde 64 og 128 mapper i kø. Store skanninger kan være delvise. Mapper tilordnes delte harde lenker etter første sti som blir funnet."),
        ],
        Page::Packages => vec![
            l("Refresh is explicit; checking updates may contact package registries. Only detected installations are covered, not every runtime manager or project.", "Oppdatering av listen er manuell; søk etter oppdateringer kan kontakte pakkeregistre. Bare oppdagede installasjoner dekkes, ikke alle kjøremiljøbehandlere eller prosjekter."),
            l("Standalone and protected apps use their own updater or system management. Reveal never uninstalls an app. Managed removals do not search for or delete leftover user data.", "Frittstående og beskyttede apper bruker sin egen oppdateringsfunksjon eller systemadministrasjon. Vis i Finder avinstallerer aldri en app. Pakkefjerning søker ikke etter eller sletter gjenværende brukerdata."),
        ],
    }
}

/// A collapsed "About this data" disclosure at the bottom of a page.
fn about(page: Page, open: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let toggle = this.clone();
    let mut section = div().flex().flex_col().gap_2().child(
        div().flex().child(crate::controls::button_labeled(
            format!("{}-about", page.key()),
            l("About this data", "Om disse dataene"),
            if open {
                l("About this data, expanded", "Om disse dataene, utvidet")
            } else {
                l("About this data, collapsed", "Om disse dataene, skjult")
            },
            open,
            dark,
            move |_, cx| {
                toggle.update(cx, |v, cx| {
                    v.tools.about = if v.tools.about == Some(page) { None } else { Some(page) };
                    cx.notify();
                });
            },
        )),
    );
    if open {
        let mut notes = card(dark);
        for note in about_notes(page) {
            notes = notes.child(sub(note).text_color(t::ink_dim(dark)));
        }
        section = section.child(notes);
    }
    section
}

fn title(text: &str) -> Div {
    div()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.to_owned())
}

const STARTUP_PAGE_SIZE: usize = 20;

fn startup_page_range(page: usize, len: usize) -> std::ops::Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let page = page.min(len.saturating_sub(1) / STARTUP_PAGE_SIZE);
    let start = page * STARTUP_PAGE_SIZE;
    let end = (start + STARTUP_PAGE_SIZE).min(len);
    start..end
}

fn startup(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let refresh = this.clone();
    let settings = this.clone();
    let mut actions = div().flex().flex_wrap().gap_2().child(button(
        "startup-settings",
        l(
            "Open Login Items Settings",
            "Åpne innstillinger for påloggingsobjekter",
        ),
        false,
        dark,
        move |_, cx| {
            let result = crate::startup::open_settings();
            settings.update(cx, |v, cx| {
                v.tools.message = result.err();
                cx.notify();
            });
        },
    ));
    if !view.tools.startup_busy {
        actions = actions.child(button(
            "startup-refresh",
            l("Refresh", "Oppdater"),
            false,
            dark,
            move |_, cx| refresh_startup(&refresh, cx),
        ));
    }
    let login = crate::login::status();
    let login_target = this.clone();
    if login.can_change() && !view.tools.startup_busy {
        let enabled = login.is_registered();
        actions=actions.child(button("startup-own-login",if enabled {l("Disable Tidegauge at login","Slå av Tidegauge ved pålogging")} else {l("Launch Tidegauge at login","Start Tidegauge ved pålogging")},enabled,dark,move |_,cx| {
            let result=crate::login::set_enabled(!enabled).map_err(str::to_string).and_then(|status| {
                crate::settings::mutate(|s|s.launch_at_login=status.is_registered()).map_err(|_|l("Login registration changed, but the local preference could not be saved.","Påloggingsregistreringen ble endret, men den lokale innstillingen kunne ikke lagres.").to_string())
            });
            login_target.update(cx,|v,cx|{v.tools.message=result.err();cx.notify();});
        }));
    }
    let mut body=div().flex().flex_col().gap(t::SECTION_GAP).child(card(dark)
        .child(sub(l("Review the legacy launch registrations found on this Mac.","Se gjennom eldre oppstartsregistreringer funnet på denne Macen.")))
        .child(sub(format!("Tidegauge: {}",login.message())))
        .child(actions));
    if view.tools.startup_busy {
        body = body.child(sub(l(
            "Reading startup registrations…",
            "Leser oppstartsregistreringer…",
        )));
    }
    if let Some(inventory) = &view.tools.startup {
        if !inventory.warnings.is_empty() {
            let mut notice = div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded(t::RADIUS_CONTROL)
                .border_1()
                .border_color(t::caution(dark).opacity(0.4))
                .bg(t::caution(dark).opacity(0.08))
                .child(
                    div()
                        .text_size(t::TEXT_BODY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(l(
                            "Partial list: some registrations were skipped",
                            "Delvis liste: noen registreringer ble hoppet over",
                        )),
                );
            for warning in &inventory.warnings {
                notice = notice.child(sub(warning.clone()).text_color(t::ink_dim(dark)));
            }
            body = body.child(notice);
        }
        if inventory.entries.is_empty() {
            body = body.child(empty_state(
                Page::Startup.icon(),
                t::accent(dark),
                l("No registrations found", "Ingen registreringer funnet").into(),
                l(
                    "No registrations found in the inspected locations.",
                    "Ingen registreringer funnet på de undersøkte stedene.",
                )
                .into(),
                dark,
            ));
        } else {
            let len = inventory.entries.len();
            let range = startup_page_range(view.tools.startup_page, len);
            body = body.child(crate::ui::section_header(
                l("Startup registrations", "Oppstartsregistreringer"),
                &format!(
                    "{} {} {} {}",
                    l("Showing", "Viser"),
                    if range.end - range.start == 1 {
                        range.end.to_string()
                    } else {
                        format!("{}–{}", range.start + 1, range.end)
                    },
                    l("of", "av"),
                    len
                ),
                dark,
            ));
            let pages = len.div_ceil(STARTUP_PAGE_SIZE);
            let page = range.start / STARTUP_PAGE_SIZE;
            if pages > 1 {
                let mut paging = div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(sub(format!("{}/{}", page + 1, pages)));
                for (offset, label) in [(-1i32, l("Previous", "Forrige")), (1, l("Next", "Neste"))] {
                    let next = page as i32 + offset;
                    if next >= 0 && (next as usize) < pages {
                        let target = this.clone();
                        paging = paging.child(button(
                            format!("startup-page-{offset}"),
                            label,
                            false,
                            dark,
                            move |_, cx| {
                                target.update(cx, |v, cx| {
                                    v.tools.startup_page = next as usize;
                                    v.scroll.set_offset(point(px(0.), px(0.)));
                                    cx.notify();
                                });
                            },
                        ));
                    }
                }
                body = body.child(paging);
            }
            let mut list = card(dark).p_0().py_1().gap_0();
            for (i, entry) in inventory
                .entries
                .iter()
                .enumerate()
                .skip(range.start)
                .take(range.len())
            {
                let state = match entry.disabled {
                    Some(true) => l("Disabled override", "Overstyring: deaktivert"),
                    Some(false) => l("Enabled override", "Overstyring: aktivert"),
                    None => l(
                        "Effective configuration unknown",
                        "Effektiv konfigurasjon ukjent",
                    ),
                };
                // One table row per registration: label and program on the
                // left, kind and state on the right; the plist path is the
                // small line under the label.
                let mut row = div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .when(i > range.start, |d| d.border_t_1().border_color(t::line(dark)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(t::TEXT_ROW)
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(entry.label.clone()),
                            )
                            .child(mono(
                                entry.executable.clone().unwrap_or_else(|| {
                                    l("Executable unknown", "Ukjent kjørbar fil").into()
                                }),
                                dark,
                            ).truncate())
                            .child(mono(entry.path.display().to_string(), dark).truncate())
                            .when(!entry.reason.is_empty(), |d| {
                                d.child(sub(entry.reason.clone()).text_color(t::caution(dark)))
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_2()
                            .rounded(t::RADIUS_CHIP)
                            .bg(t::card_hi(dark))
                            .text_xs()
                            .child(entry.source.clone()),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(t::ink_dim(dark))
                            .child(state),
                    );
                if entry.manageable && !view.tools.startup_busy {
                    if let Some(disabled) = entry.disabled {
                        let target = this.clone();
                        let entry = entry.clone();
                        row = row.child(button(
                            format!("startup-toggle-{i}"),
                            if disabled {
                                l("Enable…", "Aktiver…")
                            } else {
                                l("Disable…", "Deaktiver…")
                            },
                            false,
                            dark,
                            move |window, cx| {
                                startup_change(&target, entry.clone(), !disabled, window, cx)
                            },
                        ));
                    }
                }
                list = list.child(row);
            }
            body = body.child(list);
        }
    }
    body
}

fn startup_change(
    this: &Entity<TideGaugeView>,
    entry: crate::startup::Entry,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if this.read(cx).tools.startup_busy {
        return;
    }
    this.update(cx, |v, cx| {
        v.tools.startup_busy = true;
        cx.notify();
    });
    let detail=format!("{}\n{}\n\n{}",entry.label,entry.path.display(),l("Change this registration only. This does not stop a running process. Sign-out/login may be needed; the owning app may recreate its settings.","Endre kun denne registreringen. Dette stopper ikke en kjørende prosess. Utlogging/pålogging kan være nødvendig; appen kan gjenopprette innstillingene."));
    let answer = window.prompt(
        PromptLevel::Warning,
        l(
            "Change startup configuration?",
            "Endre oppstartskonfigurasjon?",
        ),
        Some(&detail),
        &[l("Cancel", "Avbryt"), l("Confirm", "Bekreft")],
        cx,
    );
    let this = this.clone();
    cx.spawn(async move |cx| {
        if answer.await != Ok(1) {
            this.update(cx, |v, cx| {
                v.tools.startup_busy = false;
                cx.notify();
            });
            return;
        }
        let result = smol::unblock(move || crate::startup::set_disabled(&entry, disabled)).await;
        let inventory = smol::unblock(crate::startup::scan).await;
        this.update(cx, |v, cx| {
            v.tools.startup_busy = false;
            v.tools.startup = Some(inventory);
            v.tools.message = Some(match result {
                Ok(()) => l(
                    "Configuration verified. No process was stopped.",
                    "Konfigurasjonen er bekreftet. Ingen prosess ble stoppet.",
                )
                .into(),
                Err(e) => e,
            });
            cx.notify();
        });
    })
    .detach();
}

fn activity(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut actions = segment_group("activity-sort", l("Sort by", "Sorter etter"), dark);
    for (i, (sort, label)) in [
        (ActivitySort::Cpu, "CPU"),
        (ActivitySort::Disk, l("Disk I/O", "Diskaktivitet")),
        (ActivitySort::InterruptWakeups, l("Wakeups", "Oppvåkninger")),
    ]
    .into_iter()
    .enumerate()
    {
        let target = this.clone();
        actions = actions.child(segment(
            format!("activity-sort-{i}"),
            label,
            view.tools.activity_sort == sort,
            dark,
            t::accent(dark),
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.tools.activity_sort = sort;
                    cx.notify();
                });
            },
        ));
    }
    let wakeups_label = l("reported interrupt wakeups/s", "rapporterte avbruddsoppvåkninger/s");
    let mut body=div().flex().flex_col().gap(t::SECTION_GAP).child(card(dark)
        .child(sub(l("Recent CPU, disk and wakeup activity for each process.","Nylig CPU-, disk- og oppvåkningsaktivitet for hver prosess.")))
        .child(div().flex().child(actions)));
    let mut rows: Vec<_> = view.snap.resource_activity.iter().collect();
    let metric = |r: &crate::resource_activity::Activity| match view.tools.activity_sort {
        ActivitySort::Cpu => r.cpu,
        ActivitySort::Disk => r.io,
        ActivitySort::InterruptWakeups => r.interrupt_wakeups,
    };
    rows.sort_by(|a, b| {
        metric(b)
            .partial_cmp(&metric(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.pid.cmp(&b.pid))
    });
    if let Some(own) = rows.iter().find(|r| r.own) {
        body = body.child(sub(format!(
            "{}: {} CPU · {} · {} {} · {:.0} {}",
            l(
                "Tidegauge monitoring process",
                "Tidegauges overvåkingsprosess"
            ),
            own.cpu
                .map(|v| crate::locale::percent(v, 1))
                .unwrap_or_else(|| "—".into()),
            own.io
                .map(crate::engine::fmt_rate)
                .unwrap_or_else(|| "—".into()),
            own.interrupt_wakeups
                .map(|v| crate::locale::number(v, 1))
                .unwrap_or_else(|| "—".into()),
            wakeups_label,
            own.wakeups_covered,
            l("s covered", "s dekket")
        )));
    }
    if rows.is_empty() {
        return body.child(empty_state(
            Page::Activity.icon(),
            t::accent(dark),
            l("No process activity yet", "Ingen prosessaktivitet ennå").into(),
            l("Activity appears after the first samples have been collected.", "Aktivitet vises etter at de første målingene er samlet inn.").into(),
            dark,
        ));
    }
    let dash = || String::from("—");
    let mut list = card(dark).gap_0().child(
        activity_row(
            div().child(l("Process", "Prosess")),
            "PID".into(),
            "CPU".into(),
            l("Disk I/O", "Diskaktivitet").into(),
            l("Wakeups/s", "Oppvåkninger/s").into(),
        )
        .text_color(t::ink_dim(dark))
        .pb_2()
        .border_b_1()
        .border_color(t::line(dark)),
    );
    for r in rows.iter().take(80) {
        list = list.child(
            activity_row(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    // The kernel's process name stops at 16 characters; the
                    // executable's file name is the full name.
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(
                        std::path::Path::new(&r.path)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| r.name.clone()),
                    ))
                    .child(mono(r.path.clone(), dark).truncate())
                    // Coverage only matters while it is short of the full minute.
                    .when(r.covered < 59.5 || r.wakeups_covered < 59.5, |d| {
                        d.child(sub(format!(
                            "{:.0} {} / {:.0} {}",
                            r.covered,
                            l("s CPU/I/O", "s CPU/I/O"),
                            r.wakeups_covered,
                            l("s wakeups", "s oppvåkninger")
                        )).text_color(t::ink_dim(dark)))
                    }),
                r.pid.to_string(),
                r.cpu.map(|v| crate::locale::percent(v, 1)).unwrap_or_else(dash),
                r.io.map(crate::engine::fmt_rate).unwrap_or_else(dash),
                r.interrupt_wakeups.map(|v| crate::locale::number(v, 1)).unwrap_or_else(dash),
            )
            .py_2()
            .border_b_1()
            .border_color(t::line(dark)),
        );
    }
    body.child(list)
}

fn mono(text: impl Into<String>, dark: bool) -> Div {
    div()
        .text_size(t::TEXT_CAPTION)
        .font_family("Menlo")
        .text_color(t::ink_dim(dark))
        .child(text.into())
}

/// One row of the activity table: the process on the left, then fixed-width
/// right-aligned columns so the numbers line up from row to row.
fn activity_row(name: Div, pid: String, cpu: String, io: String, wakeups: String) -> Div {
    let cell = |width: f32, value: String| {
        div()
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .justify_end()
            .text_xs()
            .child(value)
    };
    div()
        .flex()
        .items_start()
        .gap_3()
        .text_xs()
        .child(name.flex_1().min_w_0())
        .child(cell(56., pid))
        .child(cell(64., cpu))
        .child(cell(100., io))
        .child(cell(116., wakeups))
}

fn load_baseline(this: &Entity<TideGaugeView>, cx: &mut App) {
    if this.read(cx).tools.baseline_busy {
        return;
    }
    this.update(cx, |v, cx| {
        v.tools.baseline_busy = true;
        cx.notify();
    });
    let this = this.clone();
    cx.spawn(async move |cx| {
        let result = smol::unblock(crate::baselines::load).await;
        this.update(cx, |v, cx| {
            v.tools.baseline_busy = false;
            match result {
                Ok(s) => {
                    v.tools.baseline_read_failed = false;
                    if let Some(s) = &s {
                        v.tools.minutes = s.minutes;
                    }
                    v.tools.baseline = s;
                }
                Err(e) => {
                    v.tools.baseline_read_failed = true;
                    v.tools.message = Some(e);
                }
            }
            cx.notify();
        });
    })
    .detach();
}

fn capture_baseline(this: &Entity<TideGaugeView>, save: bool, cx: &mut App) {
    if this.read(cx).tools.baseline_busy {
        return;
    }
    if this.read(cx).tools.baseline_read_failed {
        return;
    }
    let minutes = if save {
        this.read(cx).tools.minutes
    } else {
        this.read(cx)
            .tools
            .baseline
            .as_ref()
            .map(|s| s.minutes)
            .unwrap_or(5)
    };
    this.update(cx, |v, cx| {
        v.tools.baseline_busy = true;
        v.tools.message = None;
        cx.notify();
    });
    let this = this.clone();
    cx.spawn(async move |cx| {
        let result=smol::unblock(move || {
            let summary=crate::baselines::capture(minutes)?;
            if save {crate::baselines::save(&summary)?;}
            Ok::<_,String>(summary)
        }).await;
        this.update(cx,|v,cx|{v.tools.baseline_busy=false;match result {
            Ok(s) if save=>{v.tools.baseline=Some(s);v.tools.comparison=None;},
            Ok(s)=>{if v.tools.baseline.as_ref().is_some_and(|b|s.start>=b.end) {v.tools.comparison=Some(s);} else {v.tools.message=Some(l("Wait for a full non-overlapping window after the saved baseline before comparing.","Vent på et helt vindu uten overlapp etter det lagrede utgangspunktet før sammenligning.").into());}},
            Err(e)=>v.tools.message=Some(e),
        }cx.notify();});
    }).detach();
}

fn baselines(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut actions = div().flex().flex_wrap().items_center().gap_2();
    let mut windows = segment_group("baseline-windows", l("Window", "Tidsvindu"), dark);
    for minutes in crate::baselines::WINDOWS {
        let target = this.clone();
        windows = windows.child(segment(
            format!("baseline-window-{minutes}"),
            format!("{minutes} min"),
            view.tools.minutes == minutes,
            dark,
            t::accent(dark),
            move |_, cx| {
                target.update(cx, |v, cx| {
                    if !v.tools.baseline_busy {
                        v.tools.minutes = minutes;
                    }
                    cx.notify();
                });
            },
        ));
    }
    actions = actions.child(windows);
    if !view.tools.baseline_busy && !view.tools.baseline_read_failed {
        let target = this.clone();
        actions = actions.child(button(
            "baseline-save",
            l("Save recent window", "Lagre nylig tidsvindu"),
            false,
            dark,
            move |window, cx| {
                if target.read(cx).tools.baseline.is_none() {
                    capture_baseline(&target, true, cx);
                    return;
                }
                let answer = window.prompt(
                    PromptLevel::Warning,
                    l("Replace saved baseline?", "Erstatte lagret utgangspunkt?"),
                    Some(l(
                        "Only the saved summary is replaced. History remains unchanged.",
                        "Kun det lagrede sammendraget erstattes. Historikken forblir uendret.",
                    )),
                    &[l("Cancel", "Avbryt"), l("Replace", "Erstatt")],
                    cx,
                );
                let target = target.clone();
                cx.spawn(async move |cx| {
                    if answer.await == Ok(1) {
                        let _ = cx.update(|cx| capture_baseline(&target, true, cx));
                    }
                })
                .detach();
            },
        ));
        if view.tools.baseline.is_some() {
            let target = this.clone();
            actions = actions.child(button(
                "baseline-compare",
                l("Compare with now", "Sammenlign med nå"),
                false,
                dark,
                move |_, cx| capture_baseline(&target, false, cx),
            ));
        }
    }
    let mut body=div().flex().flex_col().gap(t::SECTION_GAP).child(card(dark)
        .child(sub(l("Save a fixed system baseline, make your change, then compare an equal-length later window.","Lagre et fast systemutgangspunkt, gjør endringen, og sammenlign med et like langt senere tidsvindu.")))
        .child(actions));
    if view.tools.baseline_busy {
        body = body.child(sub(l("Reading history…", "Leser historikk…")));
    }
    let Some(before) = &view.tools.baseline else {
        return body.child(empty_state(
            Page::Baselines.icon(),
            t::accent(dark),
            l("No saved baseline yet", "Ikke noe lagret utgangspunkt ennå").into(),
            l(
                "Save the recent window to have something to compare against later.",
                "Lagre det siste tidsvinduet for å ha noe å sammenligne med senere.",
            )
            .into(),
            dark,
        ));
    };
    body = body.child(sub(format!(
        "{}: {} – {} · {} min",
        l("Saved baseline", "Lagret utgangspunkt"),
        crate::history_browser::local_time(before.start),
        crate::history_browser::local_time(before.end),
        before.minutes
    )));
    if let Some(after) = &view.tools.comparison {
        body = body.child(sub(format!(
            "{}: {} – {}",
            l("Comparison", "Sammenligning"),
            crate::history_browser::local_time(after.start),
            crate::history_browser::local_time(after.end)
        )));
    }
    let mut list = card(dark);
    for (i, m) in before.metrics.iter().enumerate() {
        let mut text = format!(
            "{} · {} · {} {}",
            crate::baselines::label(&m.key),
            m.mean
                .map(|v| crate::baselines::format(&m.key, v))
                .unwrap_or_else(|| "—".into()),
            crate::locale::percent(m.covered / (before.minutes as f64 * 60.) * 100., 0),
            l("coverage", "dekning")
        );
        if let Some(after) = &view.tools.comparison {
            let a = &after.metrics[i];
            text.push_str(&format!(
                " → {} · {} {} · {}",
                a.mean
                    .map(|v| crate::baselines::format(&a.key, v))
                    .unwrap_or_else(|| "—".into()),
                crate::locale::percent(a.covered / (after.minutes as f64 * 60.) * 100., 0),
                l("coverage", "dekning"),
                crate::baselines::difference(m, a, before.minutes as f64 * 60.)
                    .map(|v| crate::locale::signed_percent(v, 1))
                    .unwrap_or_else(|| l(
                        "Change unavailable (gaps or zero baseline)",
                        "Endring utilgjengelig (hull eller nullverdi)"
                    )
                    .into())
            ));
        }
        list = list.child(sub(text).py_2().border_b_1().border_color(t::line(dark)));
    }
    body.child(list)
}

fn storage(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut actions = div().flex().gap_2();
    let data = view
        .tools
        .scan
        .as_ref()
        .map(|s| s.progress.lock().unwrap().clone());
    if data.as_ref().is_none_or(|d| d.finished) {
        let target = this.clone();
        actions = actions.child(button(
            "storage-select",
            l("Choose folder…", "Velg mappe…"),
            false,
            dark,
            move |_, cx| {
                let answer = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some(
                        l("Choose a folder to inspect", "Velg en mappe å undersøke").into(),
                    ),
                });
                let target = target.clone();
                cx.spawn(async move |cx| match answer.await {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            target.update(cx, |v, cx| {
                                v.tools.scan = Some(crate::storage::start(path));
                                cx.notify();
                            });
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => {
                        target.update(cx, |v, cx| {
                            v.tools.message = Some(
                                l(
                                    "The folder picker could not be opened.",
                                    "Mappevelgeren kunne ikke åpnes.",
                                )
                                .into(),
                            );
                            cx.notify();
                        });
                    }
                })
                .detach();
            },
        ));
    } else if let Some(scan) = &view.tools.scan {
        let cancel = scan.cancel.clone();
        actions = actions.child(button(
            "storage-cancel",
            l("Cancel scan", "Avbryt skanning"),
            false,
            dark,
            move |_, _| cancel.store(true, std::sync::atomic::Ordering::Relaxed),
        ));
    }
    let mut body=div().flex().flex_col().gap(t::SECTION_GAP).child(card(dark)
        .child(sub(l("Find the largest folders and files inside a folder you choose.","Finn de største mappene og filene i en mappe du velger.")))
        .child(actions));
    if let Some(d) = data {
        let status = if d.error.is_some() {
            l("Scan unavailable", "Skanning utilgjengelig")
        } else if d.cancelled {
            l("Cancelled — partial", "Avbrutt — delvis")
        } else if d.limited || d.errors > 0 || d.skipped > 0 {
            l("Partial inventory", "Delvis oversikt")
        } else if d.finished {
            l("Scan complete", "Skanning fullført")
        } else {
            l("Scanning…", "Skanner…")
        };
        let totals = match crate::storage::observed_totals(&d) {
            Some((files, allocated, logical)) => format!(
                "{} · {}: {} · {} {} · {} {}",
                status,
                l("Observed totals", "Observerte summer"),
                crate::locale::count(files, "file", "files", "fil", "filer"),
                crate::engine::fmt_bytes(allocated as f64),
                l("allocated", "tildelt"),
                crate::engine::fmt_bytes(logical as f64),
                l("logical", "logisk")
            ),
            None => format!(
                "{} · {}",
                status,
                l(
                    "No size totals are available.",
                    "Ingen størrelsessummer er tilgjengelige."
                )
            ),
        };
        body = body.child(
            card(dark)
                .child(sub(d.root.display().to_string()))
                .child(sub(totals))
                .child(sub(format!(
                    "{} · {}",
                    crate::locale::count(d.errors, "read error", "read errors", "lesefeil", "lesefeil"),
                    crate::locale::count(
                        d.skipped,
                        "skipped entry",
                        "skipped entries",
                        "oppføring hoppet over",
                        "oppføringer hoppet over"
                    )
                ))),
        );
        if let Some(error) = d.error {
            body = body.child(sub(error).text_color(t::warn(dark)));
        }
        for (kind, heading, items) in [
            (
                "folder",
                l(
                    "Top-level folders (root includes direct files)",
                    "Mapper på øverste nivå (roten inkluderer direkte filer)",
                ),
                d.folders,
            ),
            (
                "file",
                l(
                    "Largest files by allocated size",
                    "Største filer etter tildelt størrelse",
                ),
                d.largest,
            ),
        ] {
            let mut list = card(dark).child(title(heading));
            for (i, item) in items.into_iter().take(50).enumerate() {
                let path = item.path.clone();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .py_2()
                        .border_b_1()
                        .border_color(t::line(dark))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(sub(item.path.display().to_string()))
                                .child(sub(format!(
                                    "{} {} · {} {}",
                                    crate::engine::fmt_bytes(item.allocated as f64),
                                    l("allocated", "tildelt"),
                                    crate::engine::fmt_bytes(item.logical as f64),
                                    l("logical", "logisk")
                                ))),
                        )
                        .child(button(
                            format!("storage-reveal-{kind}-{i}"),
                            l("Reveal", "Vis i Finder"),
                            false,
                            dark,
                            move |_, _| crate::storage::reveal(&path),
                        )),
                );
            }
            body = body.child(list);
        }
    }
    body
}

fn refresh_packages(this: &Entity<TideGaugeView>, updates: bool, cx: &mut App) {
    if this.read(cx).tools.packages_busy {
        return;
    }
    this.update(cx, |v, cx| {
        v.tools.packages_busy = true;
        v.tools.message = None;
        cx.notify();
    });
    let this = this.clone();
    cx.spawn(async move |cx| {
        let inventory = smol::unblock(move || crate::packages::scan(updates)).await;
        this.update(cx, |v, cx| {
            v.tools.packages = Some(inventory);
            v.tools.packages_busy = false;
            v.tools.package_page = 0;
            cx.notify();
        });
    })
    .detach();
}

fn package_action(
    this: &Entity<TideGaugeView>,
    package: crate::packages::Package,
    remove: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if this.read(cx).tools.packages_busy {
        return;
    }
    this.update(cx, |v, cx| {
        v.tools.packages_busy = true;
        cx.notify();
    });
    let detail=format!("{} · {} · {}\n{}\n\n{}\n{}",package.name,package.version,package.source.label(),package.path.display(),
        l("The owning package manager will act on this one item. Dependency changes and lifecycle scripts may run. Removal can break tools that depend on this package; there is no automatic rollback.","Den tilhørende pakkebehandleren utfører handlingen på dette ene elementet. Avhengigheter kan endres og livssyklusskript kjøres. Fjerning kan ødelegge verktøy som avhenger av pakken; det finnes ingen automatisk tilbakerulling."),
        l("No elevated privileges, force removal, data cleanup or project dependencies. A failed or interrupted operation may need repair with the package manager.","Ingen forhøyede rettigheter, tvungen fjerning, dataopprydding eller prosjektavhengigheter. En mislykket eller avbrutt handling kan kreve reparasjon med pakkebehandleren."));
    let answer = window.prompt(
        PromptLevel::Warning,
        if remove {
            l("Remove this package?", "Fjerne denne pakken?")
        } else {
            l("Update this package?", "Oppdatere denne pakken?")
        },
        Some(&detail),
        &[
            l("Cancel", "Avbryt"),
            if remove {
                l("Remove", "Fjern")
            } else {
                l("Update", "Oppdater")
            },
        ],
        cx,
    );
    let this = this.clone();
    cx.spawn(async move |cx| {
        if answer.await != Ok(1) {
            this.update(cx, |v, cx| {
                v.tools.packages_busy = false;
                cx.notify();
            });
            return;
        }
        let result = smol::unblock(move || {
            crate::packages::act(
                &package,
                if remove {
                    crate::packages::Action::Remove
                } else {
                    crate::packages::Action::Update
                },
            )
        })
        .await;
        let inventory = smol::unblock(|| crate::packages::scan(false)).await;
        this.update(cx, |v, cx| {
            v.tools.packages_busy = false;
            v.tools.packages = Some(inventory);
            v.tools.package_page = 0;
            v.tools.message = Some(match result {
                Ok(s) | Err(s) => s,
            });
            cx.notify();
        });
    })
    .detach();
}

fn packages(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    use crate::packages::Source;
    let mut actions = div().flex().flex_wrap().gap_2();
    if !view.tools.packages_busy {
        for (updates, label) in [
            (false, l("Refresh installed", "Oppdater installerte")),
            (true, l("Check for updates", "Se etter oppdateringer")),
        ] {
            let target = this.clone();
            actions = actions.child(button(
                format!("packages-refresh-{updates}"),
                label,
                false,
                dark,
                move |_, cx| refresh_packages(&target, updates, cx),
            ));
        }
    }
    let mut body=div().flex().flex_col().gap(t::SECTION_GAP).child(card(dark)
        .child(sub(l("Local app bundles, Homebrew formulae/casks and global npm packages.","Lokale app-pakker, Homebrew-formler/-casks og globale npm-pakker.")))
        .child(actions));
    if view.tools.packages_busy {
        body = body.child(sub(l(
            "Working… Other package actions are locked until this finishes.",
            "Arbeider… Andre pakkehandlinger er låst til dette er ferdig.",
        )));
    }
    let Some(inventory) = &view.tools.packages else {
        return body.child(empty_state(
            Page::Packages.icon(),
            t::accent(dark),
            l("Inventory not loaded", "Oversikten er ikke lastet").into(),
            l(
                "Choose Refresh installed to load the inventory. Nothing has been changed.",
                "Velg Oppdater installerte for å laste oversikten. Ingenting er endret.",
            )
            .into(),
            dark,
        ));
    };
    for warning in &inventory.warnings {
        body = body.child(sub(warning.clone()).text_color(t::warn(dark)));
    }
    let mut filters = div().flex().flex_wrap().items_center().gap_2();
    let mut sources =
        segment_group("package-sources", l("Package source", "Pakkekilde"), dark).flex_wrap();
    for (i, source) in [
        None,
        Some(Source::App),
        Some(Source::BrewFormula),
        Some(Source::BrewCask),
        Some(Source::Npm),
    ]
    .into_iter()
    .enumerate()
    {
        let label = source
            .map(|s| s.label())
            .unwrap_or(l("All sources", "Alle kilder"));
        let target = this.clone();
        sources = sources.child(segment(
            format!("package-source-{i}"),
            label,
            view.tools.package_source == source,
            dark,
            t::accent(dark),
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.tools.package_source = source;
                    v.tools.package_page = 0;
                    cx.notify();
                });
            },
        ));
    }
    filters = filters.child(sources);
    let mut sorts = segment_group("package-sort", l("Sort", "Sortering"), dark);
    for (source, label) in [
        (false, l("Sort: name", "Sorter: navn")),
        (true, l("Sort: source", "Sorter: kilde")),
    ] {
        let target = this.clone();
        sorts = sorts.child(segment(
            format!("package-sort-{source}"),
            label,
            view.tools.package_sort_source == source,
            dark,
            t::accent(dark),
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.tools.package_sort_source = source;
                    v.tools.package_page = 0;
                    cx.notify();
                });
            },
        ));
    }
    filters = filters.child(sorts);
    body = body.child(filters);
    let mut rows: Vec<_> = inventory
        .items
        .iter()
        .filter(|p| view.tools.package_source.is_none_or(|s| p.source == s))
        .collect();
    rows.sort_by(|a, b| {
        let order = if view.tools.package_sort_source {
            a.source.label().cmp(b.source.label())
        } else {
            std::cmp::Ordering::Equal
        };
        order
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
    let pages = rows.len().div_ceil(50).max(1);
    let page = view.tools.package_page.min(pages - 1);
    let mut paging = div().flex().items_center().gap_2().child(sub(format!(
        "{} · {} {}/{}",
        crate::locale::count(rows.len() as u64, "item", "items", "element", "elementer"),
        l("Page", "Side"),
        page + 1,
        pages
    )));
    for (offset, label) in [(-1i32, l("Previous", "Forrige")), (1, l("Next", "Neste"))] {
        let next = page as i32 + offset;
        if next >= 0 && (next as usize) < pages {
            let target = this.clone();
            paging = paging.child(button(
                format!("package-page-{offset}"),
                label,
                false,
                dark,
                move |_, cx| {
                    target.update(cx, |v, cx| {
                        v.tools.package_page = next as usize;
                        v.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    });
                },
            ));
        }
    }
    body = body.child(paging);
    for (i, p) in rows.into_iter().skip(page * 50).take(50).enumerate() {
        let mut row_actions = div().flex().flex_wrap().gap_2();
        if p.path.is_absolute() {
            let path = p.path.clone();
            row_actions = row_actions.child(button(
                format!("package-reveal-{i}"),
                l("Reveal", "Vis i Finder"),
                false,
                dark,
                move |_, _| crate::storage::reveal(&path),
            ));
        }
        if !view.tools.packages_busy {
            for (remove, enabled, label) in [
                (false, p.can_update, l("Update…", "Oppdater…")),
                (true, p.can_remove, l("Remove…", "Fjern…")),
            ] {
                if enabled {
                    let target = this.clone();
                    let package = p.clone();
                    row_actions = row_actions.child(button(
                        format!("package-act-{i}-{remove}"),
                        label,
                        false,
                        dark,
                        move |window, cx| {
                            package_action(&target, package.clone(), remove, window, cx)
                        },
                    ));
                }
            }
        }
        body = body.child(
            card(dark)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{} · {}", p.name, p.version)),
                        )
                        .child(sub(p.source.label())),
                )
                .child(sub(format!(
                    "{}: {}",
                    l("Available version", "Tilgjengelig versjon"),
                    p.latest
                        .as_deref()
                        .unwrap_or(l("Not checked / unknown", "Ikke kontrollert / ukjent"))
                )))
                .child(sub(p.path.display().to_string()))
                .child(sub(p.reason.clone()))
                .child(row_actions),
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use super::{startup_page_range, STARTUP_PAGE_SIZE};

    #[test]
    fn startup_page_range_empty() {
        assert_eq!(startup_page_range(0, 0), 0..0);
        assert_eq!(startup_page_range(usize::MAX, 0), 0..0);
    }

    #[test]
    fn startup_page_range_boundaries() {
        assert_eq!(startup_page_range(0, 1), 0..1);
        assert_eq!(startup_page_range(0, 20), 0..20);
        assert_eq!(startup_page_range(0, 21), 0..20);
        assert_eq!(startup_page_range(1, 21), 20..21);
        assert_eq!(startup_page_range(0, 40), 0..20);
        assert_eq!(startup_page_range(1, 40), 20..40);
        assert_eq!(startup_page_range(0, 41), 0..20);
        assert_eq!(startup_page_range(1, 41), 20..40);
        assert_eq!(startup_page_range(2, 41), 40..41);
    }

    #[test]
    fn startup_page_range_clamps_to_last_page() {
        assert_eq!(startup_page_range(usize::MAX, 41), 40..41);
        assert_eq!(startup_page_range(usize::MAX, 20), 0..20);
        assert_eq!(startup_page_range(usize::MAX, 1), 0..1);
    }

    #[test]
    fn startup_pages_cover_every_entry_exactly_once() {
        for len in [1usize, 19, 20, 21, 39, 40, 41, 100, 255] {
            let mut covered = Vec::new();
            for page in 0..len.div_ceil(STARTUP_PAGE_SIZE) {
                covered.extend(startup_page_range(page, len));
            }
            assert_eq!(covered, (0..len).collect::<Vec<_>>(), "len {len}");
        }
    }
}
