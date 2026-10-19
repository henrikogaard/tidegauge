//! The About section at the end of Settings: version, update check and links.

use crate::controls::{button_indexed, control, ControlSpec};
use crate::locale::text as l;
use crate::settings_page::{group, row};
use crate::theme as t;
use gpui_kit::prelude::*;
use gpui_kit::*;

const REPOSITORY_URL: &str = "https://github.com/henrikogaard/tidegauge";
const AUTHOR_URL: &str = "https://github.com/henrikogaard";
const COFFEE_URL: &str = "https://buymeacoffee.com/henrikogaard";
const LICENSE_URL: &str = "https://github.com/henrikogaard/tidegauge/blob/main/LICENSE";

/// The only hosts Tidegauge opens.
const ALLOWED_HOSTS: &[&str] = &["github.com", "buymeacoffee.com"];

/// "0.4.0 (12)", "0.4.0", or "dev" when the app runs outside a bundle.
fn version_label(version: Option<&str>, build: Option<&str>) -> String {
    match (version, build) {
        (None, _) => "dev".into(),
        (Some(version), Some(build)) if build != version => format!("{version} ({build})"),
        (Some(version), _) => version.into(),
    }
}

fn is_allowed(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| ALLOWED_HOSTS.contains(&host))
}

/// Open one of the fixed addresses above in the default browser.
fn open(url: &'static str) {
    if !is_allowed(url) {
        return;
    }
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

fn link_row(
    id: &'static str,
    icon: &'static str,
    name: &'static str,
    url: &'static str,
    detail: &'static str,
    dark: bool,
) -> Div {
    row(
        name,
        Some(detail.to_string()),
        div()
            .flex()
            .items_center()
            .gap(t::SPACE_3)
            .child(svg().path(icon).size(px(14.)).text_color(t::ink_dim(dark)))
            .child(control(
                ControlSpec {
                    id: id.into(),
                    label: l("Open", "Åpne").into(),
                    aria: Some(format!("{} {name}", l("Open", "Åpne"))),
                    role: Role::Button,
                    tab_index: 0,
                    selected: false,
                    accent: t::accent(dark),
                    dark,
                    framed: true,
                },
                move |_, _| open(url),
            )),
        false,
        dark,
    )
}

/// Reached from the bottom of Settings and from the menu's "About Tidegauge".
pub fn section(dark: bool) -> Div {
    let version = version_label(
        crate::updater::app_version().as_deref(),
        crate::updater::app_build().as_deref(),
    );
    let (update_note, can_update) = match crate::updater::availability() {
        crate::updater::Availability::Available => (
            l(
                "Checks for updates automatically.",
                "Ser etter oppdateringer automatisk.",
            )
            .to_string(),
            true,
        ),
        crate::updater::Availability::Disabled { reason }
        | crate::updater::Availability::Unavailable { reason } => (reason, false),
    };
    let identity = div()
        .px(t::SPACE_4)
        .py(t::SPACE_3)
        .flex()
        .items_center()
        .gap(t::SPACE_4)
        .child(
            img("tidegauge/logo.png")
                .size(px(48.))
                .flex_shrink_0()
                .rounded(t::RADIUS_CARD),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(t::TEXT_EMPH)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Tidegauge"),
                )
                .child(
                    div()
                        .text_size(t::TEXT_SMALL)
                        .text_color(t::ink_dim(dark))
                        .child(format!("{} {version}", l("Version", "Versjon"))),
                )
                .child(
                    div()
                        .text_size(t::TEXT_BODY)
                        .text_color(t::ink_dim(dark))
                        .child(l(
                            "A menu-bar system monitor for macOS.",
                            "En systemmonitor for macOS i menylinjen.",
                        )),
                )
                .child(
                    div()
                        .text_size(t::TEXT_SMALL)
                        .text_color(t::ink_dim(dark))
                        .child(update_note),
                ),
        )
        .when(can_update, |d| {
            d.child(button_indexed(
                "about-check-updates",
                crate::updater::check_for_updates_title(),
                false,
                dark,
                0,
                |_, _| crate::updater::ask_for_check(),
            ))
        });
    group(
        l("About", "Om"),
        vec![
            identity,
            link_row(
                "about-repo",
                "icons/github.svg",
                l("Tidegauge on GitHub", "Tidegauge på GitHub"),
                REPOSITORY_URL,
                "github.com/henrikogaard/tidegauge",
                dark,
            ),
            link_row(
                "about-author",
                "icons/github.svg",
                l("Henrik on GitHub", "Henrik på GitHub"),
                AUTHOR_URL,
                "github.com/henrikogaard",
                dark,
            ),
            link_row(
                "about-coffee",
                "icons/coffee.svg",
                l("Buy me a coffee", "Spander en kaffe"),
                COFFEE_URL,
                "buymeacoffee.com/henrikogaard",
                dark,
            ),
            link_row(
                "about-license",
                "icons/scale.svg",
                l("License", "Lisens"),
                LICENSE_URL,
                "MIT",
                dark,
            ),
        ],
        dark,
    )
}

#[cfg(test)]
mod tests {
    use super::{is_allowed, version_label, AUTHOR_URL, COFFEE_URL, LICENSE_URL, REPOSITORY_URL};

    #[test]
    fn every_link_is_https_on_an_allowed_host() {
        for url in [REPOSITORY_URL, AUTHOR_URL, COFFEE_URL, LICENSE_URL] {
            assert!(is_allowed(url), "{url}");
        }
        assert!(!is_allowed("http://github.com/x"));
        assert!(!is_allowed("https://github.com.evil.example/x"));
        assert!(!is_allowed("file:///etc/passwd"));
    }

    #[test]
    fn version_shows_build_only_when_it_adds_something() {
        assert_eq!(version_label(Some("0.4.0"), Some("12")), "0.4.0 (12)");
        assert_eq!(version_label(Some("0.4.0"), Some("0.4.0")), "0.4.0");
        assert_eq!(version_label(Some("0.4.0"), None), "0.4.0");
        assert_eq!(version_label(None, Some("12")), "dev");
    }
}
