//! The Ports page (D-1, slice 3 of docs/design/ports-and-leftovers.md):
//! every listening port grouped by project, with Stop, Open in browser and
//! Show in Finder, and a warning for dev servers open to the network.

use crate::controls::{button, button_accent};
use crate::engine::{fmt_bytes, Snapshot};
use crate::locale::text as l;
use crate::ports::{Launcher, Listener};
use crate::process_control::{self as control, Force};
use crate::process_panel::Action;
use crate::theme::{self as t, Metric};
use crate::ui::{card, empty_state, icon_img, section_header, sub, TideGaugeView};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::collections::BTreeMap;

/// What Stop does for this listener (D-3).
#[derive(Debug, PartialEq, Eq)]
pub enum StopKind {
    /// A dev server in a project: the guarded dev-server path.
    DevServer,
    /// Another process of yours: SIGTERM after a confirmation.
    Process,
    /// No Stop; the reason is shown instead.
    None(&'static str),
}

pub fn stop_kind(x: &Listener) -> StopKind {
    if let Some(block) = control::blocked(&x.process) {
        return StopKind::None(block.reason());
    }
    match x.launcher {
        Launcher::Docker => StopKind::None(l("Stop it in Docker", "Stopp den i Docker")),
        Launcher::Launchd(_) => StopKind::None(l("launchd starts it again", "launchd starter den igjen")),
        _ if x.dev_server => StopKind::DevServer,
        _ => StopKind::Process,
    }
}

fn open_in_browser(port: u16) {
    if let Ok(mut child) = std::process::Command::new("/usr/bin/open").arg(format!("http://localhost:{port}")).spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn chip(text: String, color: Hsla, dark: bool) -> Div {
    div().px(t::SPACE_2).py(px(1.)).rounded_sm().bg(color.opacity(if dark { 0.22 } else { 0.14 })).text_xs().text_color(color).whitespace_nowrap().child(text)
}

fn row(x: &Listener, dark: bool, this: &Entity<TideGaugeView>) -> Div {
    let accent = Metric::Projects.accent(dark);
    let id = format!("{}-{}", x.process.pid, x.port);
    let port = x.port;
    let mut actions = div().flex().items_center().gap_2().flex_shrink_0().child(button(
        format!("port-open-{id}"),
        l("Open", "Åpne"),
        false,
        dark,
        move |_, _| open_in_browser(port),
    ));
    match stop_kind(x) {
        StopKind::DevServer => {
            let targets = vec![crate::ports::cli::stop_target(x)];
            let view = this.clone();
            actions = actions.child(button_accent(format!("port-stop-{id}"), l("Stop…", "Stopp…"), false, dark, accent, move |window, cx| {
                crate::ui::request_stop(targets.clone(), view.clone(), window, cx)
            }));
        }
        StopKind::Process => {
            let action = Action::Processes { name: format!("{} :{}", x.name, x.port), targets: vec![x.process.clone()], force: Force::Quit };
            let view = this.clone();
            actions = actions.child(button_accent(format!("port-stop-{id}"), l("Stop…", "Stopp…"), false, dark, accent, move |window, cx| {
                crate::process_panel::request(action.clone(), view.clone(), window, cx)
            }));
        }
        StopKind::None(why) => {
            actions = actions.child(div().text_xs().text_color(t::ink_dim(dark)).whitespace_nowrap().child(why));
        }
    }
    let mut title = div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(div().text_sm().whitespace_nowrap().overflow_hidden().child(x.name.clone()))
        .child(chip(format!(":{}", x.port), accent, dark))
        .child(div().text_xs().text_color(t::ink_dim(dark)).whitespace_nowrap().child(x.binds_text()));
    if x.warn_exposed() {
        title = title.child(chip(l("Open to network", "Åpen mot nettverket").into(), t::warn(dark), dark));
    }
    div()
        .flex()
        .items_center()
        .gap_3()
        .py(t::SPACE_1)
        .px_1()
        .child(
            div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(title).child(
                div()
                    .text_xs()
                    .text_color(t::ink_dim(dark))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(format!("{} {} · PID {} · {}", l("Started by", "Startet av"), x.launcher.label(), x.process.pid, fmt_bytes(x.process.mem_bytes as f64))),
            ),
        )
        .child(actions)
}

fn group_card(title: String, folder: Option<&str>, rows: &[&Listener], dark: bool, this: &Entity<TideGaugeView>) -> Div {
    let accent = Metric::Projects.accent(dark);
    let icon = if folder.is_some() { "icons/folder.svg" } else { "icons/globe.svg" };
    let mut head = div().flex().items_center().gap_2().child(icon_img(icon, 16.0, accent)).child(
        div()
            .flex_1()
            .min_w_0()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).whitespace_nowrap().child(title))
            .when_some(folder, |d, f| d.child(div().text_xs().text_color(t::ink_dim(dark)).whitespace_nowrap().overflow_hidden().child(f.to_string()))),
    );
    if let Some(f) = folder {
        let path = std::path::PathBuf::from(f);
        head = head.child(button(format!("port-reveal-{f}"), l("Show in Finder", "Vis i Finder"), false, dark, move |_, _| crate::storage::reveal(&path)));
    }
    card(dark).child(head).children(rows.iter().map(|x| row(x, dark, this)))
}

/// Listeners by project folder (sorted by folder name), then the rest.
pub fn grouped(ports: &[Listener]) -> (Vec<(String, Vec<&Listener>)>, Vec<&Listener>) {
    let mut by: BTreeMap<String, Vec<&Listener>> = BTreeMap::new();
    let mut other = Vec::new();
    for x in ports {
        match &x.project {
            Some(p) => by.entry(p.clone()).or_default().push(x),
            None => other.push(x),
        }
    }
    let mut groups: Vec<(String, Vec<&Listener>)> = by.into_iter().collect();
    groups.sort_by_key(|(dir, _)| std::path::Path::new(dir).file_name().map(|n| n.to_string_lossy().to_lowercase()));
    (groups, other)
}

/// What "Stop all dev servers…" stops: the rows whose own Stop is the
/// dev-server Stop, so a launchd job or Docker is never in it (D-3).
/// `stop_targets` merges a process listed once per port.
pub fn bulk_targets(ports: &[Listener]) -> Vec<crate::projects::StopTarget> {
    ports.iter().filter(|x| stop_kind(x) == StopKind::DevServer).map(crate::ports::cli::stop_target).collect()
}

pub fn panel(s: &Snapshot, dark: bool, this: Entity<TideGaugeView>) -> Div {
    let accent = Metric::Projects.accent(dark);
    let mut c = div().flex().flex_col().gap_4();
    if !s.ports_ready {
        // The first frame after opening the page, before the next pass.
        return c.child(card(dark).child(sub(l("Reading ports…", "Leser porter…")).text_color(t::ink_dim(dark))));
    }
    if s.ports.is_empty() {
        return c.child(empty_state(
            "icons/globe.svg",
            accent,
            l("Nothing is listening", "Ingenting lytter").into(),
            l(
                "Ports your programs listen on appear here, with their project folder and what started them. Ports owned by macOS or other users are not shown.",
                "Porter som programmene dine lytter på vises her, med prosjektmappe og hva som startet dem. Porter som eies av macOS eller andre brukere vises ikke.",
            )
            .into(),
            dark,
        ));
    }
    let exposed: Vec<&Listener> = s.ports.iter().filter(|x| x.warn_exposed()).collect();
    if !exposed.is_empty() {
        let list = exposed.iter().map(|x| format!("{} :{}", x.name, x.port)).collect::<Vec<_>>().join(", ");
        c = c.child(
            card(dark).child(
                div().flex().items_start().gap_3().child(icon_img("icons/triangle-alert.svg", 18.0, t::warn(dark))).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(list))
                        .child(sub(crate::ports::exposed_warning()).text_color(t::ink_dim(dark))),
                ),
            ),
        );
    }
    let dev = bulk_targets(&s.ports);
    c = c.child(
        card(dark).child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(icon_img("icons/globe.svg", 20.0, accent))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(crate::locale::count(s.ports.len() as u64, "port", "ports", "port", "porter")))
                        .child(sub(l("Stop asks first, and stops only the process you chose.", "Stopp spør først, og stopper bare prosessen du valgte.")).text_color(t::ink_dim(dark))),
                )
                .when(!dev.is_empty(), |d| {
                    let view = this.clone();
                    d.child(button_accent("ports-stop-dev", l("Stop all dev servers…", "Stopp alle utviklingsservere…"), false, dark, accent, move |window, cx| {
                        crate::ui::request_stop(dev.clone(), view.clone(), window, cx)
                    }))
                }),
        ),
    );
    let (groups, other) = grouped(&s.ports);
    if !groups.is_empty() {
        c = c.child(section_header(l("By project", "Etter prosjekt"), &crate::locale::count(groups.len() as u64, "project", "projects", "prosjekt", "prosjekter"), dark));
        for (dir, rows) in &groups {
            let name = std::path::Path::new(dir).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.clone());
            c = c.child(group_card(name, Some(dir), rows, dark, &this));
        }
    }
    if !other.is_empty() {
        c = c.child(section_header(l("Other programs", "Andre programmer"), &crate::locale::count(other.len() as u64, "port", "ports", "port", "porter"), dark));
        c = c.child(group_card(l("Not in a project", "Ikke i et prosjekt").into(), None, &other, dark, &this));
    }
    c
}

/// "Left running" on the Agents page (FR-4, FR-5): each ended agent
/// session with what it left, Stop all and Dismiss.
pub fn left_running(ended: &[crate::ports::ended::Tracked], dark: bool, this: Entity<TideGaugeView>) -> Div {
    let accent = Metric::Projects.accent(dark);
    let now = crate::metrics::now_unix() as i64;
    let mut c = div().flex().flex_col().gap(t::SPACE_2).child(section_header(
        l("Left running", "Fortsatt i gang"),
        &crate::locale::count(ended.len() as u64, "ended session", "ended sessions", "avsluttet økt", "avsluttede økter"),
        dark,
    ));
    for t in ended {
        let (pid, start) = t.root_key();
        let folder = t.folder.as_deref().and_then(|f| std::path::Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned());
        let title = match folder {
            Some(f) => format!("{} · {f}", t.agent),
            None => t.agent.clone(),
        };
        let when = t.ended_at.map(|at| crate::cleanup_panel::ago(at as i64, now)).unwrap_or_default();
        let targets: Vec<_> = t.members.iter().map(|m| m.proc_info()).filter(|p| control::blocked(p).is_none()).collect();
        let mut buttons = div().flex().items_center().gap_2().flex_shrink_0();
        if !targets.is_empty() {
            let action = Action::Processes { name: format!("{} ({})", title, l("left running", "fortsatt i gang")), targets, force: Force::Quit };
            let view = this.clone();
            buttons = buttons.child(button_accent(format!("left-stop-{pid}-{start}"), l("Stop all…", "Stopp alle…"), false, dark, accent, move |window, cx| {
                crate::process_panel::request(action.clone(), view.clone(), window, cx)
            }));
        }
        buttons = buttons.child(button(format!("left-dismiss-{pid}-{start}"), l("Dismiss", "Lukk"), false, dark, move |_, _| crate::ports::ended::forget((pid, start))));
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(icon_img("icons/bot.svg", 16.0, accent))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).whitespace_nowrap().child(title))
                    .child(
                        div().text_xs().text_color(t::ink_dim(dark)).child(format!(
                            "{} {when} · {}",
                            l("Ended", "Avsluttet"),
                            crate::locale::count(t.members.len() as u64, "process still running", "processes still running", "prosess kjører fortsatt", "prosesser kjører fortsatt")
                        )),
                    ),
            )
            .child(buttons);
        let rows = t.members.iter().map(|m| {
            let mut r = div().flex().items_center().gap_2().py(px(2.)).px_1().child(div().text_sm().whitespace_nowrap().child(m.name.clone()));
            for port in &m.ports {
                r = r.child(chip(format!(":{port}"), accent, dark));
            }
            r.child(div().text_xs().text_color(t::ink_dim(dark)).child(format!("PID {}", m.pid)))
        });
        c = c.child(card(dark).child(head).children(rows));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::{grouped, stop_kind, StopKind};
    use crate::ports::{Bind, Launcher, Listener};
    use crate::processes::ProcInfo;

    fn listener(pid: i32, port: u16, project: Option<&str>, dev: bool, launcher: Launcher) -> Listener {
        Listener {
            port,
            binds: vec![Bind::of("127.0.0.1".parse().unwrap())],
            process: ProcInfo { pid, ppid: 1, uid: unsafe { libc::getuid() }, comm: "node".into(), exe_path: "/opt/homebrew/bin/node".into(), start_unix: 1_700_000_000., ..Default::default() },
            name: "node".into(),
            project: project.map(str::to_string),
            dev_server: dev,
            launcher,
        }
    }

    #[test]
    fn stop_follows_the_launcher() {
        assert_eq!(stop_kind(&listener(900, 3000, Some("/a"), true, Launcher::Terminal("Terminal".into()))), StopKind::DevServer);
        assert_eq!(stop_kind(&listener(900, 3000, None, false, Launcher::App("Spotify".into()))), StopKind::Process);
        assert!(matches!(stop_kind(&listener(900, 5432, None, false, Launcher::Docker)), StopKind::None(_)));
        assert!(matches!(stop_kind(&listener(900, 6379, None, false, Launcher::Launchd("homebrew.mxcl.redis".into()))), StopKind::None(_)));
        let mut other_user = listener(900, 22, None, false, Launcher::Unknown);
        other_user.process.uid = other_user.process.uid.wrapping_add(1);
        assert!(matches!(stop_kind(&other_user), StopKind::None(_)));
    }

    #[test]
    fn stop_all_skips_rows_that_cannot_be_stopped() {
        let ports = [
            listener(901, 3000, Some("/x/app"), true, Launcher::Terminal("Terminal".into())),
            listener(902, 3001, Some("/x/app"), true, Launcher::Launchd("com.example.web".into())),
            listener(903, 3002, Some("/x/app"), true, Launcher::Docker),
            listener(904, 7000, None, false, Launcher::App("ControlCenter".into())),
        ];
        let pids: Vec<i32> = super::bulk_targets(&ports).iter().map(|t| t.server.pid).collect();
        assert_eq!(pids, vec![901]);
    }

    #[test]
    fn groups_by_project_name_then_the_rest() {
        let ports = [
            listener(1, 3000, Some("/x/zeta"), true, Launcher::Unknown),
            listener(2, 3001, Some("/x/Alpha"), true, Launcher::Unknown),
            listener(3, 3002, Some("/x/zeta"), true, Launcher::Unknown),
            listener(4, 7000, None, false, Launcher::App("ControlCenter".into())),
        ];
        let (groups, other) = grouped(&ports);
        assert_eq!(groups.iter().map(|(d, r)| (d.as_str(), r.len())).collect::<Vec<_>>(), vec![("/x/Alpha", 1), ("/x/zeta", 2)]);
        assert_eq!(other.len(), 1);
    }
}
