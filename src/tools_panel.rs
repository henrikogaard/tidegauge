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
    /// How many packages the list shows before "Show more".
    pub package_limit: usize,
    /// The package whose actions are open, by source and path.
    pub package_selected: Option<String>,
    /// How many startup entries the list shows before "Show more".
    pub startup_limit: usize,
    /// The startup entry whose details are open, by its plist path.
    pub startup_selected: Option<std::path::PathBuf>,
    /// Whether Apple's own registrations are listed.
    pub startup_show_apple: bool,
    /// The page whose "skipped" reasons are open.
    pub skipped_open: Option<Page>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            page: Page::Startup,
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
            package_limit: LIST_STEP,
            package_selected: None,
            startup_limit: LIST_STEP,
            startup_selected: None,
            startup_show_apple: false,
            skipped_open: None,
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
        v.tools.startup_limit = LIST_STEP;
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
}

/// The header for a tool page: what it is for, its actions, and its notes.
pub(crate) fn chrome(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> crate::ui::PageChrome {
    let page = view.tools.page;
    let (subtitle, actions) = match page {
        Page::Startup => (
            l("Review the legacy launch registrations found on this Mac.", "Se gjennom eldre oppstartsregistreringer funnet på denne Macen."),
            startup_actions(view, dark, this),
        ),
        Page::Activity => (
            l("Recent CPU, disk and wakeup activity for each process.", "Nylig CPU-, disk- og oppvåkningsaktivitet for hver prosess."),
            Vec::new(),
        ),
        Page::Baselines => (
            l("See whether a change made your Mac calmer: measure before, change, measure after.", "Se om en endring gjorde Macen roligere: mål før, endre, mål etter."),
            Vec::new(),
        ),
        Page::Storage => (
            l("Find the largest folders and files inside a folder you choose.", "Finn de største mappene og filene i en mappe du velger."),
            storage_actions(view, dark, this),
        ),
        Page::Packages => (
            l("Local app bundles, Homebrew formulae/casks and global npm packages.", "Lokale app-pakker, Homebrew-formler/-casks og globale npm-pakker."),
            package_actions(view, dark, this),
        ),
    };
    crate::ui::PageChrome { subtitle: Some(subtitle.into()), actions, notes: about_notes(page) }
}

/// The caveats for a page, behind the header's ⓘ so the page leads with
/// what it is for.
pub(crate) fn about_notes(page: Page) -> Vec<&'static str> {
    match page {
        Page::Startup => vec![
            l("Partial inventory of legacy launch registrations. A file is not proof of approval or a running process. Modern app background items and managed items belong in System Settings.", "Delvis oversikt over eldre oppstartsregistreringer. En fil beviser ikke godkjenning eller at prosessen kjører. Moderne bakgrunnselementer og administrerte elementer håndteres i Systeminnstillinger."),
        ],
        Page::Activity => vec![
            l("Time-weighted activity over up to 60 observed seconds per process. No energy score or battery-drain estimate. New, missing or reset counters remain unavailable.", "Tidsvektet aktivitet over inntil 60 observerte sekunder per prosess. Ingen energipoeng eller anslag for batteriforbruk. Nye, manglende eller nullstilte tellere vises som utilgjengelige."),
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


/// The name to show for a registration: its program's file name, unless the
/// program is a shell or interpreter, which says nothing; then the last part
/// of its label.
fn startup_name(label: &str, executable: Option<&str>) -> String {
    const RUNNERS: [&str; 9] = ["bash", "sh", "zsh", "env", "python3", "python", "node", "ruby", "perl"];
    match executable.and_then(|x| std::path::Path::new(x).file_name()).map(|n| n.to_string_lossy().into_owned()) {
        Some(name) if !RUNNERS.contains(&name.as_str()) => name,
        _ => label.rsplit('.').next().filter(|last| !last.is_empty()).unwrap_or(label).to_string(),
    }
}

/// Who a startup registration belongs to, for grouping: the app bundle its
/// program lives in, else the maker named in its reverse-DNS label. The flag
/// says whether it is part of macOS itself.
fn startup_owner(label: &str, executable: Option<&str>) -> (String, bool) {
    // Only the system's own folders count: /usr/local and /opt hold
    // third-party tools.
    const SYSTEM_DIRS: [&str; 6] = ["/System/", "/usr/bin/", "/usr/sbin/", "/usr/libexec/", "/bin/", "/sbin/"];
    let apple = label.starts_with("com.apple.")
        || executable.is_some_and(|exe| SYSTEM_DIRS.iter().any(|dir| exe.starts_with(dir)));
    if apple {
        return ("Apple".into(), true);
    }
    if let Some(bundle) = executable.and_then(crate::processes::bundle_root) {
        if let Some(stem) = std::path::Path::new(&bundle).file_stem() {
            return (stem.to_string_lossy().into_owned(), false);
        }
    }
    let parts: Vec<&str> = label.split('.').collect();
    let owner = match parts.as_slice() {
        // A capitalised third part is usually the product: dev.orbstack.OrbStack…
        [_, _, product, ..] if product.chars().any(char::is_uppercase) => product.to_string(),
        [_, maker, ..] => {
            let mut chars = maker.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
        _ => label.to_string(),
    };
    (owner, false)
}

fn startup_actions(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Vec<AnyElement> {
    let refresh = this.clone();
    let settings = this.clone();
    let mut actions = vec![button(
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
    )
    .into_any_element()];
    if !view.tools.startup_busy {
        actions.push(
            button(
                "startup-refresh",
                l("Refresh", "Oppdater"),
                false,
                dark,
                move |_, cx| refresh_startup(&refresh, cx),
            )
            .into_any_element(),
        );
    }
    let login = crate::login::status();
    let login_target = this.clone();
    if login.can_change() && !view.tools.startup_busy {
        let enabled = login.is_registered();
        actions.push(button("startup-own-login",if enabled {l("Disable Tidegauge at login","Slå av Tidegauge ved pålogging")} else {l("Launch Tidegauge at login","Start Tidegauge ved pålogging")},enabled,dark,move |_,cx| {
            let result=crate::login::set_enabled(!enabled).map_err(str::to_string).and_then(|status| {
                crate::settings::mutate(|s|s.launch_at_login=status.is_registered()).map_err(|_|l("Login registration changed, but the local preference could not be saved.","Påloggingsregistreringen ble endret, men den lokale innstillingen kunne ikke lagres.").to_string())
            });
            login_target.update(cx,|v,cx|{v.tools.message=result.err();cx.notify();});
        }).into_any_element());
    }
    actions
}

fn startup(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut body = div().flex().flex_col().gap(t::SECTION_GAP);
    if view.tools.startup_busy {
        body = body.child(sub(l("Reading startup registrations…", "Leser oppstartsregistreringer…")).text_color(t::ink_dim(dark)));
    }
    let Some(inventory) = &view.tools.startup else {
        return body;
    };
    if !inventory.warnings.is_empty() {
        // A skipped entry is worth knowing, not worth alarming about.
        body = body.child(skipped_note(
            Page::Startup,
            l("Partial list: some registrations were skipped.", "Delvis liste: noen registreringer ble hoppet over.").into(),
            &inventory.warnings,
            view.tools.skipped_open == Some(Page::Startup),
            dark,
            this.clone(),
        ));
    }
    if inventory.entries.is_empty() {
        return body.child(empty_state(
            Page::Startup.icon(),
            t::accent(dark),
            l("No registrations found", "Ingen registreringer funnet").into(),
            l("No registrations found in the inspected locations.", "Ingen registreringer funnet på de undersøkte stedene.").into(),
            dark,
        ));
    }
    let query = view.startup_query.as_str();
    let owned: Vec<(String, bool, &crate::startup::Entry)> = inventory
        .entries
        .iter()
        .map(|e| {
            let (owner, apple) = startup_owner(&e.label, e.executable.as_deref());
            (owner, apple, e)
        })
        .collect();
    let apple_count = owned.iter().filter(|(_, apple, _)| *apple).count();
    let matches = |owner: &str, e: &crate::startup::Entry| {
        query.is_empty()
            || owner.to_lowercase().contains(query)
            || e.label.to_lowercase().contains(query)
            || e.executable.as_deref().is_some_and(|x| x.to_lowercase().contains(query))
    };
    let mut visible: Vec<&(String, bool, &crate::startup::Entry)> = owned
        .iter()
        .filter(|(owner, apple, e)| (view.tools.startup_show_apple || !apple) && matches(owner, e))
        .collect();
    // Third-party groups alphabetically, Apple's own last.
    visible.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())).then_with(|| a.2.label.cmp(&b.2.label)));
    let apple_target = this.clone();
    let show_apple = view.tools.startup_show_apple;
    body = body.child(
        div()
            .flex()
            .items_center()
            .gap(t::SPACE_3)
            .child(
                div()
                    .w(px(260.))
                    .h(t::CONTROL_H)
                    .px(t::SPACE_2)
                    .flex()
                    .items_center()
                    .rounded(t::RADIUS_CONTROL)
                    .border_1()
                    .border_color(t::line(dark))
                    .text_size(t::TEXT_BODY)
                    .child(gpui_kit::base::input::Input::new(&view.startup_filter)),
            )
            .child(div().flex_1())
            .child(sub(format!("{} ({apple_count})", l("Show Apple's own", "Vis Apples egne"))).text_color(t::ink_dim(dark)))
            .child(crate::controls::switch("startup-apple", l("Show Apple's own registrations", "Vis Apples egne registreringer"), show_apple, dark, move |_, cx| {
                apple_target.update(cx, |v, cx| {
                    v.tools.startup_show_apple = !v.tools.startup_show_apple;
                    v.tools.startup_limit = LIST_STEP;
                    cx.notify();
                });
            })),
    );
    if visible.is_empty() {
        return body.child(empty_state(
            Page::Startup.icon(),
            t::accent(dark),
            l("Nothing matches", "Ingenting passer").into(),
            l("Try another name, or show Apple's own registrations.", "Prøv et annet navn, eller vis Apples egne registreringer.").into(),
            dark,
        ));
    }
    let shown = visible.len().min(view.tools.startup_limit);
    let mut i = 0;
    while i < shown {
        let owner = visible[i].0.clone();
        let group_len = visible[i..].iter().take_while(|(o, ..)| *o == owner).count();
        let take = group_len.min(shown - i);
        body = body.child(crate::ui::section_header(&owner, &group_len.to_string(), dark));
        let mut list = card(dark).p_0().py_1().gap_0();
        for (index, (_, _, entry)) in visible[i..i + take].iter().enumerate() {
            list = startup_row(list, view, entry, i + index, dark, this.clone());
        }
        body = body.child(list);
        i += take;
    }
    if visible.len() > shown {
        let target = this.clone();
        body = body.child(show_more("startup-more", visible.len() - shown, dark, move |cx| {
            target.update(cx, |v, cx| {
                v.tools.startup_limit += LIST_STEP;
                cx.notify();
            });
        }));
    }
    body
}

/// One registration: the program's name, its label underneath, its kind on
/// the right; picking it shows the paths and the change action.
fn startup_row(list: Div, view: &TideGaugeView, entry: &crate::startup::Entry, i: usize, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let selected = view.tools.startup_selected.as_ref() == Some(&entry.path);
    let name = startup_name(&entry.label, entry.executable.as_deref());
    let bundle = entry.executable.as_deref().and_then(crate::processes::bundle_root);
    let icon = match bundle {
        Some(path) => crate::ui::bundle_icon(Some(&path), dark),
        None => crate::ui::glyph_tile("icons/power.svg", dark).into_any_element(),
    };
    let mut trailing: Vec<AnyElement> = Vec::new();
    if let Some(disabled) = entry.disabled {
        trailing.push(
            crate::ui::chip(
                if disabled { l("Disabled override", "Overstyring: deaktivert") } else { l("Enabled override", "Overstyring: aktivert") },
                None,
                dark,
            )
            .into_any_element(),
        );
    }
    if !entry.reason.is_empty() {
        trailing.push(svg().path("icons/triangle-alert.svg").size(px(13.)).text_color(t::caution(dark)).into_any_element());
    }
    trailing.push(
        div()
            .w(px(140.))
            .flex_shrink_0()
            .flex()
            .justify_end()
            .text_size(t::TEXT_SMALL)
            .text_color(t::ink_dim(dark))
            .child(entry.source.clone())
            .into_any_element(),
    );
    let path = entry.path.clone();
    let toggle_target = this.clone();
    let mut list = list.child(crate::ui::list_row(
        format!("startup-{i}"),
        icon,
        name,
        Some(entry.label.clone()),
        trailing,
        selected,
        dark,
        Some(std::rc::Rc::new(move |cx: &mut App| {
            toggle_target.update(cx, |v, cx| {
                v.tools.startup_selected = if v.tools.startup_selected.as_ref() == Some(&path) { None } else { Some(path.clone()) };
                cx.notify();
            });
        })),
    ));
    if selected {
        let mut actions = div().flex().flex_wrap().gap(t::SPACE_2);
        let plist = entry.path.clone();
        actions = actions.child(button(format!("startup-reveal-{i}"), l("Reveal", "Vis i Finder"), false, dark, move |_, _| {
            crate::storage::reveal(&plist)
        }));
        if entry.manageable && !view.tools.startup_busy {
            if let Some(disabled) = entry.disabled {
                let target = this.clone();
                let entry = entry.clone();
                actions = actions.child(button(
                    format!("startup-toggle-{i}"),
                    if disabled { l("Enable…", "Aktiver…") } else { l("Disable…", "Deaktiver…") },
                    false,
                    dark,
                    move |window, cx| startup_change(&target, entry.clone(), !disabled, window, cx),
                ));
            }
        }
        list = list.child(
            crate::ui::row_detail(dark)
                .when(!entry.reason.is_empty(), |d| d.child(div().text_color(t::caution(dark)).child(entry.reason.clone())))
                .child(mono(entry.executable.clone().unwrap_or_else(|| l("Executable unknown", "Ukjent kjørbar fil").into()), dark))
                .child(mono(entry.path.display().to_string(), dark))
                .child(actions),
        );
    }
    list
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

pub(crate) fn activity(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
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
    let body = div().flex().flex_col().gap(t::SECTION_GAP).child(div().flex().child(actions));
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
    let cpu = crate::app_cpu::AppCpu::chosen(view.snap.chip.cores);
    let cell = |width: f32, value: String| {
        div()
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .justify_end()
            .text_size(t::TEXT_BODY)
            .child(value)
            .into_any_element()
    };
    // Header cells line up with the row cells: the row's icon and gaps sit
    // in front of the name.
    let mut list = card(dark).p_0().py_1().gap_0().child(
        div()
            .flex()
            .items_center()
            .gap(t::SPACE_3)
            .mx(t::SPACE_2)
            .px(t::SPACE_3)
            .pt(t::SPACE_2)
            .pb(t::SPACE_1)
            .text_size(t::TEXT_CAPTION)
            .text_color(t::ink_dim(dark))
            .child(div().flex_1().child(l("Process", "Prosess").to_uppercase()))
            .child(cell(56., "PID".into()))
            .child(cell(112., cpu.column_header().into()))
            .child(cell(100., l("Disk I/O", "Diskaktivitet").to_uppercase()))
            .child(cell(116., l("Wakeups/s", "Oppvåkninger/s").to_uppercase())),
    );
    for (i, r) in rows.iter().take(80).enumerate() {
        // The kernel's process name stops at 16 characters; the executable's
        // file name is the full name.
        let name = std::path::Path::new(&r.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| r.name.clone());
        let mut meta = crate::processes::path_summary(&r.path);
        // Coverage only matters while it is short of the full minute.
        if r.covered < 59.5 || r.wakeups_covered < 59.5 {
            meta = format!("{meta} · {:.0} {}", r.covered.min(r.wakeups_covered), l("s observed", "s observert"));
        }
        let bundle = crate::processes::bundle_root(&r.path);
        list = list.child(crate::ui::list_row(
            format!("activity-{i}"),
            crate::ui::bundle_icon(bundle.as_deref(), dark),
            if r.own { format!("{name} · Tidegauge") } else { name },
            Some(meta),
            vec![
                cell(56., r.pid.to_string()),
                cell(112., r.cpu.map(|v| cpu.format(v)).unwrap_or_else(dash)),
                cell(100., r.io.map(crate::engine::fmt_rate).unwrap_or_else(dash)),
                cell(116., r.interrupt_wakeups.map(|v| crate::locale::number(v, 1)).unwrap_or_else(dash)),
            ],
            false,
            dark,
            None,
        ));
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

/// The baseline window picker and save button: step one of Before & after.
fn save_controls(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
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
    let mut controls = div().flex().items_center().gap(t::SPACE_2).child(windows);
    if !view.tools.baseline_busy && !view.tools.baseline_read_failed {
        let target = this.clone();
        let replacing = view.tools.baseline.is_some();
        controls = controls.child(button(
            "baseline-save",
            if replacing { l("Save again…", "Lagre på nytt…") } else { l("Save the last minutes", "Lagre de siste minuttene") },
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
    }
    controls
}

/// One numbered step: a number badge, a title, one quiet line, and what
/// to do on the right. Done steps show a check instead of the number.
fn step(number: u8, title: &'static str, detail: String, done: bool, control: Option<AnyElement>, dark: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(t::SPACE_3)
        .px(t::SPACE_4)
        .py(t::SPACE_3)
        .when(number > 1, |d| d.border_t_1().border_color(t::line(dark)))
        .child(
            div()
                .size(px(24.))
                .flex_shrink_0()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(if done { t::ok(dark).opacity(0.16) } else { t::card_hi(dark) })
                .text_size(t::TEXT_BODY)
                .font_weight(FontWeight::SEMIBOLD)
                .when(done, |d| d.child(svg().path("icons/check.svg").size(px(13.)).text_color(t::ok(dark))))
                .when(!done, |d| d.child(number.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(t::TEXT_ROW).font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).child(detail)),
        )
        .children(control)
}

fn baselines(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let now = crate::metrics::now_unix();
    let before = view.tools.baseline.as_ref();
    let minutes = before.map(|b| b.minutes).unwrap_or(view.tools.minutes);
    let waited = before.map(|b| (now - b.end).max(0.)).unwrap_or(0.);
    let ready = before.is_some() && waited >= minutes as f64 * 60.;
    let saved_detail = match before {
        Some(b) => format!(
            "{} {} – {} · {} min",
            l("Saved", "Lagret"),
            crate::history_browser::local_time(b.start),
            crate::history_browser::local_time(b.end),
            b.minutes
        ),
        None => l("Saves the average of the last few minutes as your “before”.", "Lagrer gjennomsnittet av de siste minuttene som «før».").into(),
    };
    let change_detail = match before {
        None => l("Then quit an app, change a setting, or whatever you want to test.", "Deretter avslutter du en app, endrer en innstilling eller det du vil teste.").to_string(),
        Some(_) if ready => l("Enough time has passed to compare.", "Det har gått nok tid til å sammenligne.").to_string(),
        Some(_) => format!(
            "{} {} {}",
            l("Use the Mac as usual for", "Bruk Macen som vanlig i"),
            ((minutes as f64 * 60. - waited) / 60.).ceil().max(1.),
            l("more minutes.", "minutter til."),
        ),
    };
    let compare = (before.is_some() && !view.tools.baseline_busy).then(|| {
        let target = this.clone();
        button("baseline-compare", l("Compare with now", "Sammenlign med nå"), ready, dark, move |_, cx| {
            capture_baseline(&target, false, cx)
        })
        .into_any_element()
    });
    let mut body = div().flex().flex_col().gap(t::SECTION_GAP).child(
        card(dark)
            .p_0()
            .gap_0()
            .child(step(1, l("Measure before", "Mål før"), saved_detail, before.is_some(), Some(save_controls(view, dark, this.clone()).into_any_element()), dark))
            .child(step(2, l("Make your change", "Gjør endringen"), change_detail, ready, None, dark))
            .child(step(
                3,
                l("Compare", "Sammenlign"),
                l("Measures an equally long window now and compares the two.", "Måler et like langt vindu nå og sammenligner de to.").into(),
                view.tools.comparison.is_some(),
                compare,
                dark,
            )),
    );
    if view.tools.baseline_busy {
        body = body.child(sub(l("Reading history…", "Leser historikk…")).text_color(t::ink_dim(dark)));
    }
    let (Some(before), Some(after)) = (before, view.tools.comparison.as_ref()) else {
        return body;
    };
    // Before, after and the change per measure, lined up as a table.
    let cell = |width: f32, text: String, color: Hsla| {
        div().w(px(width)).flex_shrink_0().flex().justify_end().text_size(t::TEXT_BODY).text_color(color).child(text)
    };
    let mut table = card(dark).p_0().py_1().gap_0().child(
        div()
            .flex()
            .px(t::SPACE_4)
            .py(t::SPACE_2)
            .text_size(t::TEXT_CAPTION)
            .text_color(t::ink_dim(dark))
            .child(div().flex_1().child(l("Measure", "Måling").to_uppercase()))
            .child(cell(110., l("Before", "Før").to_uppercase(), t::ink_dim(dark)))
            .child(cell(110., l("After", "Etter").to_uppercase(), t::ink_dim(dark)))
            .child(cell(110., l("Change", "Endring").to_uppercase(), t::ink_dim(dark))),
    );
    let seconds = before.minutes as f64 * 60.;
    for (m, a) in before.metrics.iter().zip(after.metrics.iter()) {
        let fmt = |v: Option<f64>, key: &str| v.map(|v| crate::baselines::format(key, v)).unwrap_or_else(|| "—".into());
        let change = crate::baselines::difference(m, a, seconds);
        // Less use is the calmer direction for every measure here.
        let color = match change {
            Some(c) if c <= -5. => t::ok(dark),
            Some(c) if c >= 5. => t::caution(dark),
            _ => t::ink_dim(dark),
        };
        table = table.child(
            div()
                .flex()
                .items_center()
                .px(t::SPACE_4)
                .h(t::ROW_H)
                .child(div().flex_1().text_size(t::TEXT_ROW).child(crate::baselines::label(&m.key)))
                .child(cell(110., fmt(m.mean, &m.key), t::ink(dark)))
                .child(cell(110., fmt(a.mean, &a.key), t::ink(dark)))
                .child(cell(110., change.map(|c| crate::locale::signed_percent(c, 1)).unwrap_or_else(|| "—".into()), color)),
        );
    }
    body.child(crate::ui::section_header(
        l("Result", "Resultat"),
        &format!("{} {}", l("Compared", "Sammenlignet"), crate::history_browser::local_time(after.end)),
        dark,
    ))
    .child(table)
}

fn storage_actions(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Vec<AnyElement> {
    let mut actions: Vec<AnyElement> = Vec::new();
    let data = view
        .tools
        .scan
        .as_ref()
        .map(|s| s.progress.lock().unwrap().clone());
    if data.as_ref().is_none_or(|d| d.finished) {
        let target = this.clone();
        actions.push(button(
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
        ).into_any_element());
    } else if let Some(scan) = &view.tools.scan {
        let cancel = scan.cancel.clone();
        actions.push(button(
            "storage-cancel",
            l("Cancel scan", "Avbryt skanning"),
            false,
            dark,
            move |_, _| cancel.store(true, std::sync::atomic::Ordering::Relaxed),
        ).into_any_element());
    }
    actions
}

fn scan_folder(this: &Entity<TideGaugeView>, path: std::path::PathBuf, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.tools.scan = Some(crate::storage::start(path));
        cx.notify();
    });
}

/// Folders most people want to look at first, so the page never opens empty.
fn quick_folders() -> Vec<(&'static str, &'static str, std::path::PathBuf)> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    vec![
        ("icons/folder-open.svg", l("Home", "Hjem"), home.clone()),
        ("icons/download.svg", l("Downloads", "Nedlastinger"), home.join("Downloads")),
        ("icons/file-text.svg", l("Documents", "Dokumenter"), home.join("Documents")),
        ("icons/app-window.svg", l("Applications", "Programmer"), std::path::PathBuf::from("/Applications")),
        ("icons/layers.svg", l("Caches", "Bufre"), home.join("Library/Caches")),
    ]
}

/// One found folder or file: its name, where it is, a bar against the
/// largest in the list, and its size.
fn size_row(i: usize, kind: &'static str, item: &crate::storage::Item, largest: u64, dark: bool) -> Div {
    let name = item
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| item.path.display().to_string());
    let parent = item.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let share = item.allocated as f32 / largest.max(1) as f32;
    let path = item.path.clone();
    div()
        .flex()
        .items_center()
        .gap(t::SPACE_3)
        .px(t::SPACE_4)
        .min_h(t::ROW_H)
        .py(t::SPACE_1)
        .child(crate::ui::glyph_tile(if kind == "folder" { "icons/folder-open.svg" } else { "icons/file-text.svg" }, dark))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().text_size(t::TEXT_ROW).truncate().child(name))
                .child(div().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).truncate().child(parent)),
        )
        .child(
            div()
                .w(px(120.))
                .h(px(6.))
                .flex_shrink_0()
                .rounded_full()
                .bg(t::card_hi(dark))
                .child(div().h_full().rounded_full().w(relative(share.clamp(0.02, 1.))).bg(t::Metric::Disk.accent(dark))),
        )
        .child(
            div()
                .w(px(80.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(t::TEXT_ROW)
                .font_weight(FontWeight::MEDIUM)
                .child(crate::engine::fmt_bytes(item.allocated as f64)),
        )
        .child(button(format!("storage-reveal-{kind}-{i}"), l("Reveal", "Vis i Finder"), false, dark, move |_, _| {
            crate::storage::reveal(&path)
        }))
}

fn storage(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let data = view
        .tools
        .scan
        .as_ref()
        .map(|s| s.progress.lock().unwrap().clone());
    let mut body = div().flex().flex_col().gap(t::SECTION_GAP);
    let scanning = data.as_ref().is_some_and(|d| !d.finished);
    if !scanning {
        let mut picks = div().flex().flex_wrap().gap(t::SPACE_2);
        for (i, (icon, label, path)) in quick_folders().into_iter().enumerate() {
            let target = this.clone();
            let selected = data.as_ref().is_some_and(|d| d.root == path);
            picks = picks.child(
                div()
                    .id(SharedString::from(format!("storage-pick-{i}")))
                    .role(Role::Button)
                    .aria_label(label)
                    .tab_index(0)
                    .h(t::CONTROL_H)
                    .px(t::SPACE_3)
                    .flex()
                    .items_center()
                    .gap(t::SPACE_2)
                    .rounded(t::RADIUS_CONTROL)
                    .border_1()
                    .border_color(if selected { t::accent(dark) } else { t::line(dark) })
                    .when(selected, |d| d.bg(t::accent(dark).opacity(0.12)))
                    .hover(|d| d.bg(t::card_hi(dark)))
                    .cursor_pointer()
                    .text_size(t::TEXT_BODY)
                    .child(svg().path(icon).size(px(13.)).text_color(t::ink_dim(dark)))
                    .child(label)
                    .on_click({
                        let (target, path) = (target.clone(), path.clone());
                        move |_, _, cx| scan_folder(&target, path.clone(), cx)
                    })
                    .capture_key_down(move |event, window, cx| {
                        if crate::controls::is_activation_key(&event.keystroke) {
                            window.prevent_default();
                            scan_folder(&target, path.clone(), cx);
                            cx.stop_propagation();
                        }
                    }),
            );
        }
        body = body.child(picks);
    }
    let Some(d) = data else {
        return body.child(empty_state(
            Page::Storage.icon(),
            t::accent(dark),
            l("Pick a folder to look inside", "Velg en mappe å se inn i").into(),
            l("Start with one above, or choose any folder in the header.", "Start med en over, eller velg en hvilken som helst mappe øverst.").into(),
            dark,
        ));
    };
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
        Some((files, allocated, _)) => format!(
            "{} · {} · {}",
            status,
            crate::locale::count(files, "file", "files", "fil", "filer"),
            crate::engine::fmt_bytes(allocated as f64),
        ),
        None => status.to_string(),
    };
    body = body.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(t::SPACE_3)
            .child(div().text_size(t::TEXT_ROW).font_weight(FontWeight::MEDIUM).truncate().child(crate::processes::path_summary(&d.root.display().to_string())))
            .child(div().flex_shrink_0().text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).child(totals)),
    );
    if d.errors > 0 || d.skipped > 0 {
        body = body.child(
            sub(format!(
                "{} · {}",
                crate::locale::count(d.errors, "read error", "read errors", "lesefeil", "lesefeil"),
                crate::locale::count(d.skipped, "skipped entry", "skipped entries", "oppføring hoppet over", "oppføringer hoppet over")
            ))
            .text_color(t::ink_dim(dark)),
        );
    }
    if let Some(error) = &d.error {
        body = body.child(sub(error.clone()).text_color(t::warn(dark)));
    }
    for (kind, heading, items) in [
        ("folder", l("Folders", "Mapper"), &d.folders),
        ("file", l("Largest files", "Største filer"), &d.largest),
    ] {
        if items.is_empty() {
            continue;
        }
        let largest = items.iter().map(|i| i.allocated).max().unwrap_or(1);
        body = body.child(crate::ui::section_header(heading, &items.len().min(50).to_string(), dark));
        let mut list = card(dark).p_0().py_1().gap_0();
        for (i, item) in items.iter().take(50).enumerate() {
            list = list.child(size_row(i, kind, item, largest, dark));
        }
        body = body.child(list);
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
            v.tools.package_limit = LIST_STEP;
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
            v.tools.package_limit = LIST_STEP;
            v.tools.message = Some(match result {
                Ok(s) | Err(s) => s,
            });
            cx.notify();
        });
    })
    .detach();
}

fn package_actions(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Vec<AnyElement> {
    let mut actions: Vec<AnyElement> = Vec::new();
    if !view.tools.packages_busy {
        for (updates, label) in [
            (false, l("Refresh installed", "Oppdater installerte")),
            (true, l("Check for updates", "Se etter oppdateringer")),
        ] {
            let target = this.clone();
            actions.push(button(
                format!("packages-refresh-{updates}"),
                label,
                false,
                dark,
                move |_, cx| refresh_packages(&target, updates, cx),
            ).into_any_element());
        }
    }
    actions
}

/// Rows a long list shows at first and adds with each "Show more".
pub(crate) const LIST_STEP: usize = 60;

fn package_key(p: &crate::packages::Package) -> String {
    format!("{:?}:{}:{}", p.source, p.name, p.path.display())
}

fn package_icon(p: &crate::packages::Package, dark: bool) -> AnyElement {
    use crate::packages::Source;
    match p.source {
        Source::App => crate::ui::bundle_icon(p.path.to_str(), dark),
        Source::BrewFormula => crate::ui::glyph_tile("icons/beer.svg", dark).into_any_element(),
        Source::BrewCask => crate::ui::glyph_tile("icons/package.svg", dark).into_any_element(),
        Source::Npm => crate::ui::glyph_tile("icons/hexagon.svg", dark).into_any_element(),
    }
}

/// A newer version than the one installed, when a check found one.
fn newer_version(p: &crate::packages::Package) -> Option<&str> {
    p.latest.as_deref().filter(|latest| *latest != p.version)
}

/// One quiet line saying some items were skipped; clicking it shows why.
fn skipped_note(page: Page, headline: String, reasons: &[String], open: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let keyboard = this.clone();
    let toggle = move |this: &Entity<TideGaugeView>, cx: &mut App| {
        this.update(cx, |v, cx| {
            v.tools.skipped_open = if v.tools.skipped_open == Some(page) { None } else { Some(page) };
            cx.notify();
        });
    };
    div()
        .flex()
        .flex_col()
        .gap(t::SPACE_1)
        .child(
            div()
                .id(SharedString::from(format!("{}-skipped", page.key())))
                .role(Role::Button)
                .aria_label(SharedString::from(headline.clone()))
                .aria_expanded(open)
                .tab_index(0)
                .flex()
                .items_center()
                .gap(t::SPACE_1)
                .cursor_pointer()
                .text_size(t::TEXT_SMALL)
                .text_color(t::ink_dim(dark))
                .hover(|d| d.text_color(t::ink(dark)))
                .child(svg().path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).size(px(11.)).text_color(t::ink_dim(dark)))
                .child(headline)
                .on_click({
                    let this = this.clone();
                    move |_, _, cx| toggle(&this, cx)
                })
                .capture_key_down(move |event, window, cx| {
                    if crate::controls::is_activation_key(&event.keystroke) {
                        window.prevent_default();
                        toggle(&keyboard, cx);
                        cx.stop_propagation();
                    }
                }),
        )
        .when(open, |d| {
            d.children(reasons.iter().map(|reason| {
                div().pl(t::SPACE_4).text_size(t::TEXT_SMALL).text_color(t::ink_dim(dark)).child(reason.clone())
            }))
        })
}

/// "Show more" under a long list: adds another step of rows.
fn show_more(id: &'static str, hidden: usize, dark: bool, add: impl Fn(&mut App) + 'static) -> Div {
    div().flex().justify_center().pt(t::SPACE_1).child(button(
        id,
        format!("{} ({hidden})", l("Show more", "Vis flere")),
        false,
        dark,
        move |_, cx| add(cx),
    ))
}

fn packages(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    use crate::packages::Source;
    let mut body = div().flex().flex_col().gap(t::SECTION_GAP);
    if view.tools.packages_busy {
        body = body.child(sub(l(
            "Working… Other package actions are locked until this finishes.",
            "Arbeider… Andre pakkehandlinger er låst til dette er ferdig.",
        )).text_color(t::ink_dim(dark)));
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
    // Skipped items are worth knowing, not alarming: one quiet line.
    if !inventory.warnings.is_empty() {
        body = body.child(skipped_note(
            Page::Packages,
            crate::locale::count(
                inventory.warnings.len() as u64,
                "item was skipped",
                "items were skipped",
                "element ble hoppet over",
                "elementer ble hoppet over",
            ),
            &inventory.warnings,
            view.tools.skipped_open == Some(Page::Packages),
            dark,
            this.clone(),
        ));
    }
    let mut sources = segment_group("package-sources", l("Package source", "Pakkekilde"), dark);
    for (i, source) in [None, Some(Source::App), Some(Source::BrewFormula), Some(Source::BrewCask), Some(Source::Npm)]
        .into_iter()
        .enumerate()
    {
        let label = source.map(|s| s.label()).unwrap_or(l("All sources", "Alle kilder"));
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
                    v.tools.package_limit = LIST_STEP;
                    cx.notify();
                });
            },
        ));
    }
    let sort_target = this.clone();
    let by_source = view.tools.package_sort_source;
    let mut rows: Vec<_> = inventory
        .items
        .iter()
        .filter(|p| view.tools.package_source.is_none_or(|s| p.source == s))
        .collect();
    rows.sort_by(|a, b| {
        let order = if by_source { a.source.label().cmp(b.source.label()) } else { std::cmp::Ordering::Equal };
        // Packages with an update come first: that is what the page is for.
        newer_version(b).is_some().cmp(&newer_version(a).is_some())
            .then(order)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
    let updates = rows.iter().filter(|p| newer_version(p).is_some()).count();
    body = body.child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(t::SPACE_3)
            .child(sources)
            .child(div().flex_1())
            .child(button(
                "package-sort",
                if by_source { l("Sorted by source", "Sortert etter kilde") } else { l("Sorted by name", "Sortert etter navn") },
                false,
                dark,
                move |_, cx| {
                    sort_target.update(cx, |v, cx| {
                        v.tools.package_sort_source = !v.tools.package_sort_source;
                        cx.notify();
                    });
                },
            )),
    );
    body = body.child(crate::ui::section_header(
        &crate::locale::count(rows.len() as u64, "item", "items", "element", "elementer"),
        &if updates > 0 {
            crate::locale::count(updates as u64, "update available", "updates available", "oppdatering tilgjengelig", "oppdateringer tilgjengelig")
        } else {
            String::new()
        },
        dark,
    ));
    let shown = rows.len().min(view.tools.package_limit);
    let mut list = card(dark).p_0().py_1().gap_0();
    for (i, p) in rows.iter().take(shown).enumerate() {
        let key = package_key(p);
        let selected = view.tools.package_selected.as_deref() == Some(key.as_str());
        let toggle_target = this.clone();
        let toggle_key = key.clone();
        let mut trailing: Vec<AnyElement> = Vec::new();
        if let Some(latest) = newer_version(p) {
            trailing.push(crate::ui::chip(format!("{} {latest}", l("Update to", "Oppdater til")), Some(t::ok(dark)), dark).into_any_element());
        }
        trailing.push(
            div()
                .w(px(140.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(t::TEXT_SMALL)
                .text_color(t::ink_dim(dark))
                .child(p.source.label())
                .into_any_element(),
        );
        list = list.child(crate::ui::list_row(
            format!("package-{i}"),
            package_icon(p, dark),
            p.name.clone(),
            Some(format!("{} · {}", p.version, crate::processes::path_summary(&p.path.display().to_string()))),
            trailing,
            selected,
            dark,
            Some(std::rc::Rc::new(move |cx: &mut App| {
                toggle_target.update(cx, |v, cx| {
                    v.tools.package_selected =
                        if v.tools.package_selected.as_deref() == Some(toggle_key.as_str()) { None } else { Some(toggle_key.clone()) };
                    cx.notify();
                });
            })),
        ));
        if selected {
            let mut actions = div().flex().flex_wrap().gap(t::SPACE_2);
            if p.path.is_absolute() {
                let path = p.path.clone();
                actions = actions.child(button(format!("package-reveal-{i}"), l("Reveal", "Vis i Finder"), false, dark, move |_, _| {
                    crate::storage::reveal(&path)
                }));
            }
            if !view.tools.packages_busy {
                for (remove, enabled, label) in [
                    (false, p.can_update, l("Update…", "Oppdater…")),
                    (true, p.can_remove, l("Remove…", "Fjern…")),
                ] {
                    if enabled {
                        let target = this.clone();
                        let package = (*p).clone();
                        actions = actions.child(button(format!("package-act-{i}-{remove}"), label, false, dark, move |window, cx| {
                            package_action(&target, package.clone(), remove, window, cx)
                        }));
                    }
                }
            }
            list = list.child(
                crate::ui::row_detail(dark)
                    .when(!p.reason.is_empty(), |d| d.child(p.reason.clone()))
                    .child(actions),
            );
        }
    }
    body = body.child(list);
    if rows.len() > shown {
        let target = this.clone();
        body = body.child(show_more("packages-more", rows.len() - shown, dark, move |cx| {
            target.update(cx, |v, cx| {
                v.tools.package_limit += LIST_STEP;
                cx.notify();
            });
        }));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::{startup_name, startup_owner};

    #[test]
    fn startup_names_skip_shells() {
        assert_eq!(startup_name("com.trycua.spacesd", Some("/bin/bash")), "spacesd");
        assert_eq!(startup_name("com.google.keystone.agent", Some("/Library/Google/GoogleSoftwareUpdate/Agent")), "Agent");
        assert_eq!(startup_name("com.example.thing", None), "thing");
    }

    #[test]
    fn startup_entries_group_by_app_or_maker() {
        let own = |label: &str, exe: Option<&str>| startup_owner(label, exe);
        assert_eq!(
            own("com.microsoft.update.agent", Some("/Library/Application Support/Microsoft/MAU2.0/Microsoft AutoUpdate.app/Contents/MacOS/Microsoft Update Assistant.app/Contents/MacOS/x")),
            ("Microsoft AutoUpdate".into(), false)
        );
        assert_eq!(own("dev.orbstack.OrbStack.privhelper", Some("/Library/PrivilegedHelperTools/dev.orbstack.OrbStack.privhelper")), ("OrbStack".into(), false));
        assert_eq!(own("com.microsoft.update.agent", None), ("Microsoft".into(), false));
        assert_eq!(own("com.apple.AMPArtworkAgent", Some("/System/Library/x")), ("Apple".into(), true));
        assert_eq!(own("helper", Some("/usr/libexec/helper")), ("Apple".into(), true));
        assert_eq!(own("standalone", None), ("standalone".into(), false));
        assert_eq!(
            own("homebrew.mxcl.postgresql", Some("/usr/local/opt/postgresql/bin/postgres")),
            ("Mxcl".into(), false),
            "/usr/local is third-party"
        );
    }
}
