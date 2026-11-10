//! The Cleanup page (FR-1, FR-9–FR-12, FR-15, FR-18): free space, what is
//! ready to free, levels, one Clean up button. The finders and the safety
//! rules live in `cleanup/`; this file only draws and asks.

use crate::cleanup::agent::AgentKind;
use crate::cleanup::{self, remove::Report, AgentNote, Candidate, Category, Level, Method, Phase, ScanState, Tier};
use crate::controls::{button, button_accent, segment, segment_group};
use crate::locale::text as l;
use crate::theme as t;
use crate::ui::{caption, card, chip, empty_state, glyph_tile, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Size,
    LastUsed,
    Name,
}

pub struct Cleaning {
    pub total: usize,
    pub done: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct State {
    pub scan: Option<Arc<Mutex<ScanState>>>,
    pub cancel: Arc<AtomicBool>,
    pub level: Level,
    /// Manual ticks by candidate id; they win over the level until it changes.
    pub overrides: HashMap<String, bool>,
    pub open: HashSet<Category>,
    pub kept_open: bool,
    pub sort: Sort,
    pub cleaning: Option<Cleaning>,
    pub report: Option<Report>,
    pub skipped_open: bool,
    pub reviewing: bool,
    pub notes: HashMap<String, AgentNote>,
    pub message: Option<String>,
}

impl State {
    fn snapshot(&self) -> Option<ScanState> {
        self.scan.as_ref().map(|s| s.lock().unwrap().clone())
    }
    pub fn scanning(&self) -> bool {
        self.scan.as_ref().is_some_and(|s| s.lock().unwrap().phase != Phase::Done)
    }
    /// Opening the page scans only the first time, or again after a scan
    /// was stopped; otherwise the last result stays until Rescan or a cleanup.
    pub fn needs_scan(&self) -> bool {
        self.scan.is_none() || self.cancel.load(Ordering::Relaxed)
    }
}

fn settings() -> crate::settings::Settings {
    crate::settings::Settings::load()
}

fn ticked(state: &State, c: &Candidate, idle_days: u32, now: i64) -> bool {
    c.selectable()
        && state
            .overrides
            .get(&c.id)
            .copied()
            .unwrap_or_else(|| state.level.ticks(c, idle_days, now) && !state.notes.get(&c.id).is_some_and(|n| n.keep))
}

/// What a cleanup would remove: the ticked items, minus any that sit inside
/// another ticked item (orphaned build data inside all of DerivedData), so
/// sizes are counted once and nothing is reported as skipped for being gone.
fn plan<'a>(st: &State, cands: &'a [Candidate], idle_days: u32, now: i64) -> Vec<&'a Candidate> {
    cleanup::without_nested(cands.iter().filter(|c| ticked(st, c, idle_days, now)).collect())
}

/// Start a scan unless one is running, and redraw while it runs.
pub fn start_scan(this: &Entity<TideGaugeView>, cx: &mut App) {
    if this.read(cx).tools.cleanup.scanning() || this.read(cx).tools.cleanup.cleaning.is_some() {
        return;
    }
    let agent_folders: Vec<std::path::PathBuf> = this
        .read(cx)
        .snap
        .agents
        .iter()
        .filter_map(|s| s.folder.as_ref())
        .map(std::path::PathBuf::from)
        .collect();
    let cfg = cleanup::ScanConfig::from_settings(&settings(), agent_folders);
    let cancel = Arc::new(AtomicBool::new(false));
    let scan = cleanup::scan(cfg, cancel.clone());
    this.update(cx, |v, cx| {
        v.tools.cleanup.scan = Some(scan);
        v.tools.cleanup.cancel = cancel;
        cx.notify();
    });
    poll(this, cx);
}

/// Redraw every 300 ms while a scan or a cleanup runs.
fn poll(this: &Entity<TideGaugeView>, cx: &mut App) {
    let this = this.clone();
    cx.spawn(async move |cx| loop {
        smol::Timer::after(std::time::Duration::from_millis(300)).await;
        let busy = this.update(cx, |v, cx| {
            cx.notify();
            v.tools.cleanup.scanning() || v.tools.cleanup.cleaning.is_some()
        });
        if !busy {
            break;
        }
    })
    .detach();
}

fn stop_scan(this: &Entity<TideGaugeView>, cx: &mut App) {
    this.update(cx, |v, cx| {
        v.tools.cleanup.cancel.store(true, Ordering::Relaxed);
        if let Some(c) = &v.tools.cleanup.cleaning {
            c.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    });
}

fn size(bytes: u64) -> String {
    crate::engine::fmt_disk_bytes(bytes as f64)
}

/// "just now", "3 hours ago", "2 months ago".
pub fn ago(when: i64, now: i64) -> String {
    let s = (now - when).max(0);
    let (n, one_en, many_en, one_nb, many_nb) = match s {
        0..=89 => return l("just now", "nettopp").into(),
        90..=3599 => (s / 60, "minute", "minutes", "minutt", "minutter"),
        3600..=129_599 => (s / 3600, "hour", "hours", "time", "timer"),
        129_600..=5_183_999 => (s / 86_400, "day", "days", "dag", "dager"),
        5_184_000..=47_303_999 => (s / 2_592_000, "month", "months", "måned", "måneder"),
        _ => (s / 31_536_000, "year", "years", "år", "år"),
    };
    format!("{} {}", crate::locale::count(n as u64, one_en, many_en, one_nb, many_nb), l("ago", "siden"))
}

/// "◐ Looking for worktrees and leftovers… · 83 found so far", with a
/// turning mark so a running scan is plain to see.
pub fn scan_status(phase: Phase, found: usize, frame: usize) -> String {
    let spin = ["◐", "◓", "◑", "◒"][frame % 4];
    let what = match phase {
        Phase::Finding => l("Looking for worktrees and leftovers…", "Ser etter arbeidstrær og rester …"),
        _ => l("Measuring sizes…", "Måler størrelser …"),
    };
    format!("{spin} {what} · {found} {}", l("found so far", "funnet så langt"))
}

/// "A balanced cleanup frees 52 GB: Agent worktrees, Xcode build data and 3 more."
pub fn summary_line(level: Level, total: u64, cats: &[(Category, u64)]) -> String {
    if total == 0 {
        return l("Nothing to free at this level.", "Ingenting å frigjøre på dette nivået.").into();
    }
    let mut names: Vec<&str> = cats.iter().take(2).map(|(c, _)| c.label()).collect();
    let more = cats.len().saturating_sub(2);
    let list = match (names.len(), more) {
        (1, _) => names.remove(0).to_string(),
        (_, 0) => format!("{} {} {}", names[0], l("and", "og"), names[1]),
        _ => format!("{}, {} {} {more} {}", names[0], names[1], l("and", "og"), l("more", "til")),
    };
    format!(
        "{} {} {} {}: {list}.",
        l("A", "En"),
        level.label().to_lowercase(),
        l("cleanup frees", "opprydding frigjør"),
        size(total)
    )
}

/// The checkbox state of a category: none, some or all of its items ticked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    Mixed,
    On,
}

fn checkbox(id: String, aria: String, state: Check, enabled: bool, dark: bool, action: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
    let action = std::rc::Rc::new(action);
    let keyboard = action.clone();
    let accent = t::accent(dark);
    let on = state != Check::Off;
    div()
        .id(SharedString::from(id))
        .role(Role::CheckBox)
        .aria_label(SharedString::from(aria))
        .aria_toggled(match state {
            Check::On => Toggled::True,
            Check::Off => Toggled::False,
            Check::Mixed => Toggled::Mixed,
        })
        .when(enabled, |d| d.tab_index(0).cursor_pointer())
        .flex_shrink_0()
        .size(px(14.))
        .rounded(t::RADIUS_BAR)
        .border_1()
        .border_color(if on && enabled { accent } else { t::line(dark) })
        .bg(if on && enabled { accent } else { gpui_kit::transparent_black() })
        .when(!enabled, |d| d.opacity(0.4))
        .focus_visible(move |d| d.border_color(t::accent(dark)))
        .flex()
        .items_center()
        .justify_center()
        .when(state == Check::On && enabled, |d| d.child(svg().path("icons/check.svg").size(px(10.)).text_color(t::on_accent(dark))))
        .when(state == Check::Mixed && enabled, |d| d.child(div().w(px(7.)).h(px(2.)).rounded_full().bg(t::on_accent(dark))))
        .when(enabled, |d| {
            d.on_click(move |_, window, cx| {
                cx.stop_propagation();
                action(window, cx)
            })
            .capture_key_down(move |event, window, cx| {
                if crate::controls::is_activation_key(&event.keystroke) {
                    window.prevent_default();
                    keyboard(window, cx);
                    cx.stop_propagation();
                }
            })
        })
        .into_any_element()
}

/// Used, ready and free in one bar: the ready part is lighter.
fn stacked_bar(used: u64, ready: u64, total: u64, dark: bool) -> Div {
    let disk = t::hex(t::metric_rgb(crate::theme::Metric::Disk, dark));
    let total = total.max(1) as f32;
    let ready = ready.min(used);
    div()
        .h(px(10.))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .flex()
        .bg(disk.opacity(0.15))
        .child(div().h_full().w(relative((used - ready) as f32 / total)).bg(disk))
        .child(div().h_full().w(relative(ready as f32 / total)).bg(disk.opacity(0.45)))
}

fn title_actions(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Vec<AnyElement> {
    let st = &view.tools.cleanup;
    let mut out = Vec::new();
    if st.scanning() || st.cleaning.is_some() {
        let target = this.clone();
        out.push(button("cleanup-stop", l("Stop", "Stopp"), false, dark, move |_, cx| stop_scan(&target, cx)).into_any_element());
    } else {
        let target = this.clone();
        out.push(button("cleanup-rescan", l("Rescan", "Skann på nytt"), false, dark, move |_, cx| start_scan(&target, cx)).into_any_element());
    }
    if let Some(kind) = settings().cleanup_agent.as_deref().and_then(AgentKind::from_key) {
        if kind.find().is_some() && st.cleaning.is_none() && st.scan.is_some() && !st.scanning() {
            let target = this.clone();
            let label = if st.reviewing {
                format!("{} {}…", l("Asking", "Spør"), kind.label())
            } else {
                format!("{} {}", l("Ask", "Spør"), kind.label())
            };
            out.push(
                button("cleanup-ask-agent", label, st.reviewing, dark, move |window, cx| ask_agent(&target, kind, window, cx))
                    .into_any_element(),
            );
        }
    }
    out
}

pub fn chrome_parts(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> (&'static str, Vec<AnyElement>) {
    (
        l(
            "Find what agent work and developer tools left behind, and free it safely.",
            "Finn det agentarbeid og utviklerverktøy har lagt igjen, og frigjør det trygt.",
        ),
        title_actions(view, dark, this),
    )
}

pub fn about_notes() -> Vec<&'static str> {
    vec![
        l(
            "Worktrees are removed with git, never forced: git refuses one with changes or new files, and its branch stays. A worktree counts as removable only when it is merged into the default branch (as last fetched, or confirmed by gh when that is on), has no uncommitted, unpushed or local-only files, and no app is using it.",
            "Arbeidstrær fjernes med git, aldri med tvang: git nekter hvis det finnes endringer eller nye filer, og grenen blir værende. Et arbeidstre kan bare fjernes når det er flettet inn i standardgrenen (slik den sist ble hentet, eller bekreftet av gh når det er slått på), ikke har filer som ikke er committet, pushet eller bare finnes lokalt, og ingen app bruker det.",
        ),
        l(
            "Folders go to the Trash, where you can put them back; space is freed when you empty it. Homebrew, Go and simulator cleanups run the tools' own commands and can't be undone. Every item is checked again right before it is removed, and anything that changed is skipped.",
            "Mapper flyttes til papirkurven, hvor du kan legge dem tilbake; plassen frigjøres når du tømmer den. Opprydding i Homebrew, Go og simulatorer kjører verktøyenes egne kommandoer og kan ikke angres. Hvert element sjekkes på nytt rett før det fjernes, og alt som er endret hoppes over.",
        ),
        l(
            "Tidegauge contacts no service. gh and agent review run only when you switch them on in Settings → Cleanup; agent review sends the listed paths to your agent's provider.",
            "Tidegauge kontakter ingen tjeneste. gh og agentgjennomgang kjøres bare når du slår dem på i Innstillinger → Opprydding; agentgjennomgang sender de oppførte stiene til agentens leverandør.",
        ),
    ]
}

pub fn panel(view: &TideGaugeView, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let st = &view.tools.cleanup;
    let s = settings();
    let now = cleanup::now();
    let idle = s.cleanup_build_idle_days.max(1);
    let mut body = div().flex().flex_col().gap(t::SECTION_GAP);
    let Some(scan) = st.snapshot() else {
        return body.child(sub(l("Starting…", "Starter…")).text_color(t::ink_dim(dark)));
    };

    // Messages: an agent problem, then the last cleanup's report.
    if let Some(m) = &st.message {
        body = body.child(card(dark).child(sub(m.clone())));
    }
    if let Some(r) = &st.report {
        body = body.child(report_card(r, st.skipped_open, dark, this.clone()));
    }

    let ready = plan(st, &scan.candidates, idle, now);
    let ready_bytes: u64 = ready.iter().filter_map(|c| c.bytes).sum();

    // Header: free space, what is ready, and the bar.
    let disk = &view.snap.disk;
    body = body.child(
        card(dark)
            .gap(t::SPACE_3)
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(t::SPACE_2)
                            .child(div().text_size(t::TEXT_STAT).font_weight(FontWeight::SEMIBOLD).child(size(disk.free)))
                            .child(sub(l("available", "ledig")).text_color(t::ink_dim(dark))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(t::SPACE_2)
                            .child(div().text_size(t::TEXT_TITLE).font_weight(FontWeight::SEMIBOLD).text_color(t::ok(dark)).child(size(ready_bytes)))
                            .child(sub(l("ready", "klar")).text_color(t::ink_dim(dark))),
                    ),
            )
            .child(stacked_bar(disk.used, ready_bytes, disk.total, dark))
            .child(level_row(st.level, dark, this.clone()))
            .child(if scan.phase == Phase::Done {
                sub(summary_line(st.level, ready_bytes, &by_category(&ready))).text_color(t::ink_dim(dark))
            } else {
                let frame = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) / 300) as usize;
                sub(scan_status(scan.phase, scan.candidates.len(), frame)).text_color(t::accent(dark))
            }),
    );

    if !scan.git {
        body = body.child(empty_state(
            "icons/git-compare-arrows.svg",
            t::accent(dark),
            l("git isn't available", "git er ikke tilgjengelig").into(),
            l("Install the Command Line Tools (xcode-select --install) to find agent worktrees. Caches are still listed.", "Installer Command Line Tools (xcode-select --install) for å finne agentenes arbeidstrær. Bufre vises fortsatt.").into(),
            dark,
        ));
    }

    if let Some(p) = &scan.pressure {
        if !p.is_empty() {
            body = body.child(pressure_card(p, dark));
        }
    }

    // Ready to free, by category.
    let mut cats: Vec<(Category, Vec<&Candidate>)> = Category::ALL
        .iter()
        .map(|cat| (*cat, scan.candidates.iter().filter(|c| c.category == *cat && c.tier != Tier::Keep).collect::<Vec<_>>()))
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let cat_bytes = |v: &Vec<&Candidate>| cleanup::category_bytes(v.iter().copied());
    match st.sort {
        Sort::Size => cats.sort_by_key(|(_, v)| std::cmp::Reverse(cat_bytes(v))),
        Sort::LastUsed => cats.sort_by_key(|(_, v)| std::cmp::Reverse(v.iter().filter_map(|c| c.last_used).max())),
        Sort::Name => cats.sort_by_key(|(c, _)| c.label()),
    }
    let mut list = card(dark).p_0().py_2().gap_0();
    list = list.child(
        div()
            .px(t::SPACE_4)
            .pb(t::SPACE_1)
            .flex()
            .justify_between()
            .items_center()
            .child(caption(l("Ready to free", "Klar til å frigjøres"), dark))
            .child(sort_row(st.sort, dark, this.clone())),
    );
    if cats.is_empty() && scan.phase == Phase::Done {
        list = list.child(div().px(t::SPACE_4).py(t::SPACE_2).child(sub(l("Nothing to free right now.", "Ingenting å frigjøre akkurat nå.")).text_color(t::ink_dim(dark))));
    }
    let busy = st.cleaning.is_some();
    for (cat, items) in &cats {
        list = list.child(category_row(st, *cat, items, idle, now, busy, dark, this.clone()));
        if st.open.contains(cat) {
            let mut items = items.clone();
            items.sort_by_key(|c| std::cmp::Reverse(c.bytes.unwrap_or(0)));
            for c in items.iter().take(200) {
                list = list.child(item_row(st, c, idle, now, busy, dark, this.clone()));
            }
        }
    }
    body = body.child(list);

    // Kept: shown, never ticked.
    let kept: Vec<&Candidate> = scan.candidates.iter().filter(|c| c.tier == Tier::Keep).collect();
    if !kept.is_empty() {
        body = body.child(kept_card(st, &kept, now, busy, dark, this.clone()));
    }

    // Footer: when it was checked, what was skipped, and the one button.
    let checked = match scan.finished {
        Some(f) => format!("{} {}", l("Checked", "Sjekket"), ago(now - f.elapsed().as_secs() as i64, now)),
        None => l("Scanning…", "Skanner…").into(),
    };
    let mut footer = div().flex().items_center().justify_between().gap(t::SPACE_3).px(t::SPACE_4);
    let mut left = div().flex().items_center().gap(t::SPACE_2).child(sub(checked).text_color(t::ink_dim(dark)));
    if !scan.skipped.is_empty() {
        let target = this.clone();
        left = left.child(button(
            "cleanup-skipped",
            crate::locale::count(scan.skipped.len() as u64, "place skipped", "places skipped", "sted hoppet over", "steder hoppet over"),
            st.skipped_open,
            dark,
            move |_, cx| {
                target.update(cx, |v, cx| {
                    v.tools.cleanup.skipped_open = !v.tools.cleanup.skipped_open;
                    cx.notify();
                })
            },
        ));
    }
    footer = footer.child(left);
    if let Some(c) = &st.cleaning {
        footer = footer.child(sub(format!(
            "{} {} {} {}…",
            l("Cleaning", "Rydder"),
            c.done.load(Ordering::Relaxed).min(c.total),
            l("of", "av"),
            c.total
        )));
    } else if ready_bytes > 0 || !ready.is_empty() {
        let target = this.clone();
        footer = footer.child(button_accent(
            "cleanup-run",
            format!("{} {}", l("Clean up", "Rydd opp"), size(ready_bytes)),
            true,
            dark,
            t::accent(dark),
            move |window, cx| clean_up(&target, window, cx),
        ));
    }
    body = body.child(footer);
    if st.skipped_open {
        let mut s = card(dark).gap(t::SPACE_1);
        for (place, why) in scan.skipped.iter().take(60) {
            s = s.child(sub(format!("{} — {why}", crate::processes::path_summary(place))).text_color(t::ink_dim(dark)));
        }
        body = body.child(s);
    }
    body
}

fn by_category(ready: &[&Candidate]) -> Vec<(Category, u64)> {
    let mut out: Vec<(Category, u64)> = Vec::new();
    for c in ready {
        match out.iter_mut().find(|(cat, _)| *cat == c.category) {
            Some((_, b)) => *b += c.bytes.unwrap_or(0),
            None => out.push((c.category, c.bytes.unwrap_or(0))),
        }
    }
    out.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
    out
}

fn level_row(level: Level, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    let mut g = segment_group("cleanup-level", l("Cleanup level", "Oppryddingsnivå"), dark);
    for lv in [Level::Safe, Level::Balanced, Level::Deep] {
        let target = this.clone();
        g = g.child(segment(format!("cleanup-level-{:?}", lv), lv.label(), lv == level, dark, t::accent(dark), move |_, cx| {
            target.update(cx, |v, cx| {
                v.tools.cleanup.level = lv;
                v.tools.cleanup.overrides.clear();
                cx.notify();
            })
        }));
    }
    div().flex().child(g)
}

fn sort_row(sort: Sort, dark: bool, this: Entity<TideGaugeView>) -> impl IntoElement {
    let mut g = segment_group("cleanup-sort", l("Sort", "Sorter"), dark);
    for (s, label) in [(Sort::Size, l("Size", "Størrelse")), (Sort::LastUsed, l("Last used", "Sist brukt")), (Sort::Name, l("Name", "Navn"))] {
        let target = this.clone();
        g = g.child(segment(format!("cleanup-sort-{label}"), label, s == sort, dark, t::accent(dark), move |_, cx| {
            target.update(cx, |v, cx| {
                v.tools.cleanup.sort = s;
                cx.notify();
            })
        }));
    }
    g
}

fn set_ticks(this: &Entity<TideGaugeView>, ids: Vec<String>, on: bool, cx: &mut App) {
    this.update(cx, |v, cx| {
        for id in ids {
            v.tools.cleanup.overrides.insert(id, on);
        }
        cx.notify();
    });
}

fn size_cell(bytes: Option<u64>, dark: bool) -> AnyElement {
    div()
        .w(px(80.))
        .flex_shrink_0()
        .flex()
        .justify_end()
        .text_size(t::TEXT_ROW)
        .font_weight(FontWeight::MEDIUM)
        .when(bytes.is_none(), |d| d.text_color(t::ink_dim(dark)))
        .child(match bytes {
            Some(b) => size(b),
            None => l("Measuring…", "Måler …").into(),
        })
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn category_row(
    st: &State,
    cat: Category,
    items: &[&Candidate],
    idle: u32,
    now: i64,
    busy: bool,
    dark: bool,
    this: Entity<TideGaugeView>,
) -> Div {
    let selectable: Vec<&&Candidate> = items.iter().filter(|c| c.selectable()).collect();
    let on = selectable.iter().filter(|c| ticked(st, c, idle, now)).count();
    let check = match on {
        0 => Check::Off,
        n if n == selectable.len() => Check::On,
        _ => Check::Mixed,
    };
    let ids: Vec<String> = selectable.iter().map(|c| c.id.clone()).collect();
    let target = this.clone();
    let tick = checkbox(
        format!("cleanup-cat-{cat:?}"),
        cat.label().into(),
        check,
        !selectable.is_empty() && !busy,
        dark,
        move |_, cx| set_ticks(&target, ids.clone(), check != Check::On, cx),
    );
    let last = items.iter().filter_map(|c| c.last_used).max();
    let mut meta = crate::locale::count(items.len() as u64, "item", "items", "element", "elementer");
    if let Some(t) = last {
        meta = format!("{meta} · {} {}", l("used", "brukt"), ago(t, now));
    }
    let locks = items.iter().filter_map(|c| c.lock.clone()).next();
    let open = st.open.contains(&cat);
    let target = this.clone();
    let mut trailing = Vec::new();
    if let Some(lock) = locks {
        trailing.push(chip(lock, Some(t::caution(dark)), dark).into_any_element());
    }
    let measured = items.iter().all(|c| c.bytes.is_some());
    trailing.push(size_cell(measured.then(|| cleanup::category_bytes(items.iter().copied())), dark));
    trailing.push(
        svg()
            .path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" })
            .size(px(12.))
            .text_color(t::ink_dim(dark))
            .into_any_element(),
    );
    div().flex().items_center().pl(t::SPACE_4).child(tick).child(
        crate::ui::list_row(
            format!("cleanup-row-{cat:?}"),
            glyph_tile(cat.icon(), dark).into_any_element(),
            cat.label().to_string(),
            Some(meta),
            trailing,
            open,
            dark,
            Some(std::rc::Rc::new(move |cx: &mut App| {
                target.update(cx, |v, cx| {
                    let o = &mut v.tools.cleanup.open;
                    if !o.remove(&cat) {
                        o.insert(cat);
                    }
                    cx.notify();
                })
            })),
        )
        .flex_1(),
    )
}

fn item_chips(c: &Candidate, dark: bool) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if let Some(state) = c.state {
        use cleanup::WorktreeState::*;
        let color = match state {
            Removable => Some(t::ok(dark)),
            Review | Orphan => Some(t::caution(dark)),
            Active | InUse => None,
        };
        out.push(chip(state.label(), color, dark).into_any_element());
    }
    if let Some(lock) = &c.lock {
        out.push(chip(lock.clone(), Some(t::caution(dark)), dark).into_any_element());
    }
    if let Some(n) = &c.note {
        let (label, color) = if n.keep { (l("Agent: keep", "Agent: behold"), t::caution(dark)) } else { (l("Agent: clean", "Agent: rydd"), t::ok(dark)) };
        out.push(crate::controls::tipped(&n.reason, dark, chip(label, Some(color), dark)));
    }
    out
}

fn item_row(st: &State, c: &Candidate, idle: u32, now: i64, busy: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let on = ticked(st, c, idle, now);
    let id = c.id.clone();
    let target = this.clone();
    let tick = checkbox(
        format!("cleanup-item-{}", c.id),
        c.title.clone(),
        if on { Check::On } else { Check::Off },
        c.selectable() && !busy,
        dark,
        move |_, cx| set_ticks(&target, vec![id.clone()], !on, cx),
    );
    let mut meta = c.detail.clone();
    if let Some(t) = c.last_used {
        meta = if meta.is_empty() { ago(t, now) } else { format!("{meta} · {}", ago(t, now)) };
    }
    let mut trailing = item_chips(c, dark);
    trailing.push(size_cell(c.bytes, dark));
    if let Some(p) = c.paths.first().cloned() {
        trailing.push(
            button(format!("cleanup-show-{}", c.id), l("Show", "Vis"), false, dark, move |_, _| crate::storage::reveal(&p))
                .into_any_element(),
        );
    }
    div().flex().items_center().pl(px(40.)).child(tick).child(
        crate::ui::list_row(format!("cleanup-it-{}", c.id), div().into_any_element(), c.title.clone(), Some(meta), trailing, false, dark, None).flex_1(),
    )
}

fn kept_card(st: &State, kept: &[&Candidate], now: i64, busy: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let total = cleanup::category_bytes(kept.iter().copied());
    let target = this.clone();
    let mut c = card(dark).p_0().py_2().gap_0().opacity(0.85);
    c = c.child(crate::ui::list_row(
        "cleanup-kept",
        glyph_tile("icons/info.svg", dark).into_any_element(),
        l("Kept", "Beholdes").to_string(),
        Some(format!(
            "{} · {}",
            crate::locale::count(kept.len() as u64, "item", "items", "element", "elementer"),
            l("work in progress or real data; never removed here", "pågående arbeid eller ekte data; fjernes aldri her")
        )),
        vec![
            size_cell(Some(total), dark),
            svg()
                .path(if st.kept_open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" })
                .size(px(12.))
                .text_color(t::ink_dim(dark))
                .into_any_element(),
        ],
        st.kept_open,
        dark,
        Some(std::rc::Rc::new(move |cx: &mut App| {
            target.update(cx, |v, cx| {
                v.tools.cleanup.kept_open = !v.tools.cleanup.kept_open;
                cx.notify();
            })
        })),
    ));
    if st.kept_open {
        let mut kept = kept.to_vec();
        kept.sort_by_key(|c| (c.category, std::cmp::Reverse(c.bytes.unwrap_or(0))));
        for k in kept.iter().take(200) {
            let mut meta = format!("{} · {}", k.category.label(), k.detail);
            if let Some(t) = k.last_used {
                meta = format!("{meta} · {}", ago(t, now));
            }
            let mut trailing = item_chips(k, dark);
            trailing.push(size_cell(k.bytes, dark));
            if let Method::Tool(cleanup::ToolAction::ShutdownSimulator(_)) = &k.method {
                if !busy {
                    let target = this.clone();
                    let cand = (*k).clone();
                    trailing.push(
                        button(format!("cleanup-sim-{}", k.id), l("Shut down…", "Slå av…"), false, dark, move |window, cx| {
                            shut_down(&target, cand.clone(), window, cx)
                        })
                        .into_any_element(),
                    );
                }
            } else if let Some(p) = k.paths.first().cloned() {
                trailing.push(
                    button(format!("cleanup-show-{}", k.id), l("Show", "Vis"), false, dark, move |_, _| crate::storage::reveal(&p))
                        .into_any_element(),
                );
            }
            c = c.child(crate::ui::list_row(format!("cleanup-kept-{}", k.id), glyph_tile(k.category.icon(), dark).into_any_element(), k.title.clone(), Some(meta), trailing, false, dark, None));
        }
    }
    c
}

fn pressure_card(p: &cleanup::pressure::Pressure, dark: bool) -> Div {
    let mut lines = Vec::new();
    if let Some(swap) = p.swap_used.filter(|s| *s >= 512 << 20) {
        lines.push(format!(
            "{} {} — {}",
            l("Swap:", "Veksling:"),
            size(swap),
            l("memory spilled onto the disk; quitting apps frees it.", "minne som er lagt over på disken; å avslutte apper frigjør det.")
        ));
    }
    if let Some(n) = p.snapshots.filter(|n| *n > 0) {
        let mut line = crate::locale::count(n as u64, "local snapshot", "local snapshots", "lokalt øyeblikksbilde", "lokale øyeblikksbilder");
        line = format!("{line} — {}", l("they hold deleted files until macOS removes them.", "de holder på slettede filer til macOS fjerner dem."));
        if p.update_snapshots > 0 {
            line = format!("{line} {}", l("A staged macOS update holds space until it installs.", "En nedlastet macOS-oppdatering holder på plass til den installeres."));
        }
        lines.push(line);
    }
    if !p.simulators.is_empty() {
        lines.push(format!(
            "{} — {}",
            crate::locale::count(p.simulators.len() as u64, "booted simulator", "booted simulators", "startet simulator", "startede simulatorer"),
            l("shut them down under Kept to free memory.", "slå dem av under Beholdes for å frigjøre minne.")
        ));
    }
    let mut c = card(dark).gap(t::SPACE_1).child(caption(l("Is it really files?", "Er det egentlig filer?"), dark));
    for line in lines {
        c = c.child(sub(line).text_color(t::ink_dim(dark)));
    }
    c
}

fn report_card(r: &Report, open: bool, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let mut parts = vec![format!("{} {}", l("Freed", "Frigjorde"), size(r.freed_bytes))];
    if r.trashed_bytes > 0 {
        parts.push(format!("{} {}", size(r.trashed_bytes), l("in the Trash", "i papirkurven")));
    }
    let skipped = r.skipped.len() + r.errors.len();
    if skipped > 0 {
        parts.push(crate::locale::count(skipped as u64, "skipped", "skipped", "hoppet over", "hoppet over"));
    }
    let target = this.clone();
    let mut c = card(dark).gap(t::SPACE_2).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(sub(parts.join(" · ")).font_weight(FontWeight::MEDIUM))
            .child(button("cleanup-report-close", l("Close", "Lukk"), false, dark, move |_, cx| {
                target.update(cx, |v, cx| {
                    v.tools.cleanup.report = None;
                    cx.notify();
                })
            })),
    );
    if r.trashed_bytes > 0 {
        c = c.child(sub(l("Empty the Trash to free that space too.", "Tøm papirkurven for å frigjøre den plassen også.")).text_color(t::ink_dim(dark)));
    }
    for (what, why) in r.skipped.iter().chain(&r.errors).take(if open { 60 } else { 5 }) {
        c = c.child(sub(format!("{what} — {why}")).text_color(t::ink_dim(dark)));
    }
    c
}

/// Ask once, then remove every ticked item (FR-12).
pub fn clean_up(this: &Entity<TideGaugeView>, window: &mut Window, cx: &mut App) {
    let st = &this.read(cx).tools.cleanup;
    if st.cleaning.is_some() || st.scanning() {
        return;
    }
    let Some(scan) = st.snapshot() else { return };
    let s = settings();
    let now = cleanup::now();
    let plan: Vec<Candidate> = plan(st, &scan.candidates, s.cleanup_build_idle_days.max(1), now).into_iter().cloned().collect();
    if plan.is_empty() {
        return;
    }
    let total: u64 = plan.iter().filter_map(|c| c.bytes).sum();
    let mut lines: Vec<String> = by_category(&plan.iter().collect::<Vec<_>>())
        .into_iter()
        .map(|(cat, b)| {
            let n = plan.iter().filter(|c| c.category == cat).count() as u64;
            format!("{} · {} · {}", cat.label(), crate::locale::count(n, "item", "items", "element", "elementer"), size(b))
        })
        .collect();
    lines.push(String::new());
    if plan.iter().any(|c| matches!(c.method, Method::GitWorktree { .. })) {
        lines.push(l("Worktrees are removed with git; their branches stay.", "Arbeidstrær fjernes med git; grenene blir værende.").into());
    }
    if plan.iter().any(|c| matches!(c.method, Method::Trash) || matches!(&c.method, Method::KnownCache(k) if k.kind.method() == crate::caches::Method::Trash)) {
        lines.push(l("Folders go to the Trash.", "Mapper flyttes til papirkurven.").into());
    }
    if plan.iter().any(|c| matches!(c.method, Method::Tool(_)) || matches!(&c.method, Method::KnownCache(k) if k.kind.method() == crate::caches::Method::Tool)) {
        lines.push(l("Homebrew, Go and simulator cleanups can't be undone.", "Opprydding i Homebrew, Go og simulatorer kan ikke angres.").into());
    }
    lines.push(l("Each item is checked again first; anything that changed is skipped.", "Hvert element sjekkes på nytt først; alt som er endret hoppes over.").into());
    let title = format!("{} {}?", l("Clean up", "Rydd opp"), size(total));
    let answer = window.prompt(PromptLevel::Warning, &title, Some(&lines.join("\n")), &[l("Cancel", "Avbryt"), l("Clean up", "Rydd opp")], cx);
    run_plan(this, plan, answer, cx);
}

fn run_plan<E: 'static>(
    this: &Entity<TideGaugeView>,
    plan: Vec<Candidate>,
    answer: impl std::future::Future<Output = Result<usize, E>> + 'static,
    cx: &mut App,
) {
    let this = this.clone();
    cx.spawn(async move |cx| {
        if !matches!(answer.await, Ok(1)) {
            return;
        }
        let done = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (d, k) = (done.clone(), cancel.clone());
        let total = plan.len();
        this.update(cx, |v, cx| {
            v.tools.cleanup.cleaning = Some(Cleaning { total, done, cancel });
            v.tools.cleanup.report = None;
            cx.notify();
        });
        let poller = this.clone();
        cx.update(|cx| poll(&poller, cx));
        let report = smol::unblock(move || cleanup::remove::run(plan, &k, &d)).await;
        this.update(cx, |v, cx| {
            v.tools.cleanup.cleaning = None;
            v.tools.cleanup.report = Some(report);
            v.tools.cleanup.overrides.clear();
            v.tools.cleanup.notes.clear();
            cx.notify();
        });
        let rescan = this.clone();
        cx.update(|cx| start_scan(&rescan, cx));
    })
    .detach();
}

fn shut_down(this: &Entity<TideGaugeView>, c: Candidate, window: &mut Window, cx: &mut App) {
    let title = format!("{} {}?", l("Shut down", "Slå av"), c.title.split(" · ").next().unwrap_or(""));
    let answer = window.prompt(
        PromptLevel::Warning,
        &title,
        Some(l("The simulator stops; its apps and data stay.", "Simulatoren stopper; appene og dataene blir værende.")),
        &[l("Cancel", "Avbryt"), l("Shut down", "Slå av")],
        cx,
    );
    let mut c = c;
    // Shutting down frees memory, not disk; it is never part of a level.
    c.tier = Tier::Safe;
    run_plan(this, vec![c], answer, cx);
}

/// Agent review (FR-18): ask once for consent, then run the agent on the
/// ticked items off the main thread.
fn ask_agent(this: &Entity<TideGaugeView>, kind: AgentKind, window: &mut Window, cx: &mut App) {
    let st = &this.read(cx).tools.cleanup;
    if st.reviewing {
        return;
    }
    let Some(scan) = st.snapshot() else { return };
    let s = settings();
    let now = cleanup::now();
    let cands: Vec<Candidate> = plan(st, &scan.candidates, s.cleanup_build_idle_days.max(1), now).into_iter().cloned().collect();
    if cands.is_empty() {
        this.update(cx, |v, cx| {
            v.tools.cleanup.message = Some(l("Tick something first; the agent reviews ticked items.", "Kryss av noe først; agenten går gjennom avkryssede elementer.").into());
            cx.notify();
        });
        return;
    }
    let consent = if s.cleanup_agent_consent.is_none() {
        let text = format!(
            "{} {}. {}",
            l("Your agent reads the ticked paths, sizes and git states, and the files it opens are sent to", "Agenten leser de avkryssede stiene, størrelsene og git-tilstandene, og filene den åpner sendes til"),
            kind.provider(),
            l("It can only read, and its answer is advice: you still confirm Clean up.", "Den kan bare lese, og svaret er et råd: du bekrefter fortsatt Rydd opp."),
        );
        Some(window.prompt(PromptLevel::Info, &format!("{} {}?", l("Ask", "Spør"), kind.label()), Some(&text), &[l("Cancel", "Avbryt"), l("Continue", "Fortsett")], cx))
    } else {
        None
    };
    let this = this.clone();
    cx.spawn(async move |cx| {
        if let Some(answer) = consent {
            if !matches!(answer.await, Ok(1)) {
                return;
            }
            let _ = crate::settings::mutate(|s| s.cleanup_agent_consent = Some(cleanup::now()));
        }
        this.update(cx, |v, cx| {
            v.tools.cleanup.reviewing = true;
            v.tools.cleanup.message = None;
            cx.notify();
        });
        let result = smol::unblock(move || {
            let refs: Vec<&Candidate> = cands.iter().collect();
            cleanup::agent::review(kind, &refs)
        })
        .await;
        this.update(cx, |v, cx| {
            let st = &mut v.tools.cleanup;
            st.reviewing = false;
            match result {
                Ok(notes) => {
                    for (id, note) in notes {
                        // "keep" unticks; "clean" only adds a mark.
                        if note.keep {
                            st.overrides.insert(id.clone(), false);
                        }
                        st.notes.insert(id, note);
                    }
                    if let Some(scan) = &st.scan {
                        let mut s = scan.lock().unwrap();
                        for c in s.candidates.iter_mut() {
                            c.note = st.notes.get(&c.id).cloned();
                        }
                    }
                }
                Err(e) => st.message = Some(e),
            }
            cx.notify();
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::{ago, plan, scan_status, summary_line, ticked, State};
    use crate::cleanup::{self, AgentNote, Candidate, Category, Level, Tier};

    fn cand(id: &str, cat: Category, tier: Tier, bytes: u64, days_idle: i64) -> Candidate {
        let mut c = Candidate::folder(cat, std::path::PathBuf::from(id), id.into(), tier);
        c.bytes = Some(bytes);
        c.last_used = Some(cleanup::now() - days_idle * 86_400);
        c
    }

    #[test]
    fn opening_the_page_scans_only_once() {
        let mut st = State::default();
        assert!(st.needs_scan());
        let mut done = cleanup::ScanState::new();
        done.phase = cleanup::Phase::Done;
        done.finished = Some(std::time::Instant::now() - std::time::Duration::from_secs(3600));
        st.scan = Some(std::sync::Arc::new(std::sync::Mutex::new(done)));
        assert!(!st.needs_scan(), "an hour-old result is kept, not rescanned");
        st.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(st.needs_scan(), "a stopped scan is only partial");
    }

    #[test]
    fn a_running_scan_says_so_and_turns() {
        let a = scan_status(cleanup::Phase::Finding, 83, 0);
        let b = scan_status(cleanup::Phase::Finding, 83, 1);
        assert!(a.contains("83"), "{a}");
        assert_ne!(a, b, "the mark turns between frames");
        assert_ne!(a, scan_status(cleanup::Phase::Measuring, 83, 0));
    }

    #[test]
    fn levels_tick_expected_tiers() {
        let now = cleanup::now();
        let safe = cand("/s", Category::PackageCaches, Tier::Safe, 1, 0);
        let fresh = cand("/r1", Category::BuildOutput, Tier::Rebuild, 1, 10);
        let old = cand("/r2", Category::BuildOutput, Tier::Rebuild, 1, 40);
        let keep = cand("/k", Category::Archives, Tier::Keep, 1, 400);
        let mut st = State { level: Level::Safe, ..Default::default() };
        let picks = |st: &State| [&safe, &fresh, &old, &keep].map(|c| ticked(st, c, 30, now));
        assert_eq!(picks(&st), [true, false, false, false]);
        st.level = Level::Balanced;
        assert_eq!(picks(&st), [true, false, true, false]);
        st.level = Level::Deep;
        assert_eq!(picks(&st), [true, true, true, false]);
    }

    #[test]
    fn manual_tick_wins_and_keep_never_ticks() {
        let now = cleanup::now();
        let fresh = cand("/r1", Category::BuildOutput, Tier::Rebuild, 1, 1);
        let keep = cand("/k", Category::Archives, Tier::Keep, 1, 400);
        let mut st = State::default();
        st.overrides.insert("/r1".into(), true);
        st.overrides.insert("/k".into(), true);
        assert!(ticked(&st, &fresh, 30, now));
        assert!(!ticked(&st, &keep, 30, now), "Keep is never ticked");
        let mut locked = fresh.clone();
        locked.lock = Some("Waits for Xcode".into());
        assert!(!ticked(&st, &locked, 30, now), "a lock wins");
    }

    #[test]
    fn nested_items_count_once() {
        let now = cleanup::now();
        let all = cand("/u/DerivedData", Category::XcodeBuild, Tier::Rebuild, 100, 40);
        let orphan = cand("/u/DerivedData/Gone-abc", Category::XcodeBuild, Tier::Safe, 30, 40);
        let other = cand("/u/.npm/_cacache", Category::PackageCaches, Tier::Safe, 5, 0);
        let st = State::default();
        let both = [all.clone(), orphan.clone(), other.clone()];
        let ids: Vec<&str> = plan(&st, &both, 30, now).iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["/u/DerivedData", "/u/.npm/_cacache"], "the orphan is inside the whole folder");
        let mut st = State::default();
        st.overrides.insert("/u/DerivedData".into(), false);
        let ids: Vec<&str> = plan(&st, &both, 30, now).iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["/u/DerivedData/Gone-abc", "/u/.npm/_cacache"], "only the safe orphan");
    }

    #[test]
    fn agent_keep_unticks_clean_does_not_tick() {
        let now = cleanup::now();
        let safe = cand("/s", Category::PackageCaches, Tier::Safe, 1, 0);
        let fresh = cand("/r1", Category::BuildOutput, Tier::Rebuild, 1, 1);
        let mut st = State::default();
        st.notes.insert("/s".into(), AgentNote { keep: true, reason: "x".into() });
        st.notes.insert("/r1".into(), AgentNote { keep: false, reason: "y".into() });
        assert!(!ticked(&st, &safe, 30, now));
        assert!(!ticked(&st, &fresh, 30, now), "clean does not tick");
    }

    #[test]
    fn summary_and_meta_lines() {
        use crate::locale::{count, text as l};
        let cats = vec![(Category::AgentWorktrees, 40 << 30), (Category::XcodeBuild, 9 << 30), (Category::Logs, 1 << 30), (Category::PackageCaches, 1 << 30)];
        let s = summary_line(Level::Balanced, 51 << 30, &cats);
        let tail = format!("{}, {} {} 2 {}.", Category::AgentWorktrees.label(), Category::XcodeBuild.label(), l("and", "og"), l("more", "til"));
        assert!(s.ends_with(&tail), "{s}");
        assert!(s.contains(&Level::Balanced.label().to_lowercase()), "{s}");
        assert_eq!(summary_line(Level::Safe, 0, &[]), l("Nothing to free at this level.", "Ingenting å frigjøre på dette nivået."));
        let now = 1_000_000_000;
        let ago_s = l("ago", "siden");
        assert_eq!(ago(now - 30, now), l("just now", "nettopp"));
        assert_eq!(ago(now - 3 * 86_400, now), format!("{} {ago_s}", count(3, "day", "days", "dag", "dager")));
        assert_eq!(ago(now - 3600, now), format!("{} {ago_s}", count(1, "hour", "hours", "time", "timer")));
        assert_eq!(ago(now - 59 * 60, now), format!("{} {ago_s}", count(59, "minute", "minutes", "minutt", "minutter")));
    }

}
