//! The UI language: English or Norwegian Bokmål. Settings can pick one;
//! otherwise Norwegian when the first preferred macOS language is `nb`, `nn`
//! or `no`, else English. It can change while the app runs.
//!
//! Every visible string goes through `text` (or `plural`/`count`), and every
//! visible number goes through `number`/`percent`/`decimal`, so the two
//! languages never mix. `locale_guard` fails the tests when text bypasses
//! these helpers.

use std::sync::atomic::{AtomicU8, Ordering};

/// 0 until decided, then 1 for English or 2 for Norwegian.
static LANGUAGE: AtomicU8 = AtomicU8::new(0);

/// What the first preferred macOS language asks for.
fn system_is_norwegian() -> bool {
    objc2_foundation::NSLocale::preferredLanguages()
        .firstObject()
        .map(|language| {
            matches!(language.to_string().split(['-', '_']).next(), Some("nb" | "nn" | "no"))
        })
        .unwrap_or(false)
}

/// A saved choice ("en" or "nb") wins; otherwise macOS decides.
fn decide(choice: Option<&str>) -> bool {
    match choice {
        Some("nb") => true,
        Some("en") => false,
        _ => system_is_norwegian(),
    }
}

pub fn is_norwegian() -> bool {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            // Lets one test run exercise either language: `TIDEGAUGE_TEST_LANG=nb`.
            #[cfg(test)]
            let norwegian = match std::env::var("TIDEGAUGE_TEST_LANG") {
                Ok(language) => language == "nb",
                Err(_) => system_is_norwegian(),
            };
            #[cfg(not(test))]
            let norwegian = decide(crate::settings::Settings::load().language.as_deref());
            LANGUAGE.store(if norwegian { 2 } else { 1 }, Ordering::Relaxed);
            norwegian
        }
    }
}

/// Switch the language while running: "en", "nb", or `None` for macOS's.
/// Everything drawn afterwards uses it; nothing mixes the two.
pub fn set_language(choice: Option<&str>) {
    LANGUAGE.store(if decide(choice) { 2 } else { 1 }, Ordering::Relaxed);
}

/// The short weekday name in the UI language; 0 is Sunday, 6 Saturday.
pub fn weekday(index: usize) -> &'static str {
    const ENGLISH: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const NORWEGIAN: [&str; 7] = ["søn", "man", "tir", "ons", "tor", "fre", "lør"];
    let names = if is_norwegian() { NORWEGIAN } else { ENGLISH };
    names[index % 7]
}

pub fn text(english: &'static str, norwegian: &'static str) -> &'static str {
    if is_norwegian() {
        norwegian
    } else {
        english
    }
}

fn plural_in(
    norwegian: bool,
    n: u64,
    one_en: &'static str,
    many_en: &'static str,
    one_nb: &'static str,
    many_nb: &'static str,
) -> &'static str {
    match (norwegian, n == 1) {
        (false, true) => one_en,
        (false, false) => many_en,
        (true, true) => one_nb,
        (true, false) => many_nb,
    }
}

/// The noun for `n` things: the singular for exactly one, the plural
/// otherwise (including zero, in both languages).
pub fn plural(
    n: u64,
    one_en: &'static str,
    many_en: &'static str,
    one_nb: &'static str,
    many_nb: &'static str,
) -> &'static str {
    plural_in(is_norwegian(), n, one_en, many_en, one_nb, many_nb)
}

/// "1 app", "3 apps" / "1 app", "3 apper".
pub fn count(
    n: u64,
    one_en: &'static str,
    many_en: &'static str,
    one_nb: &'static str,
    many_nb: &'static str,
) -> String {
    format!("{n} {}", plural(n, one_en, many_en, one_nb, many_nb))
}

fn decimal_in(norwegian: bool, formatted: String) -> String {
    if norwegian {
        formatted.replace('.', ",")
    } else {
        formatted
    }
}

/// Show a number that Rust already formatted ("6.74 GB") with the decimal
/// comma Norwegian uses ("6,74 GB"). Display only: pass text that is built
/// for the screen, never a value that is stored or parsed back.
pub fn decimal(formatted: String) -> String {
    decimal_in(is_norwegian(), formatted)
}

/// `value` with `decimals` fraction digits and the language's decimal mark.
pub fn number(value: f64, decimals: usize) -> String {
    decimal(format!("{value:.decimals$}"))
}

fn whole_in(norwegian: bool, value: f64) -> String {
    let digits = format!("{:.0}", value.abs());
    let separator = if norwegian { '\u{a0}' } else { ',' };
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(separator);
        }
        grouped.push(c);
    }
    if value.round() < 0. {
        grouped.insert(0, '-');
    }
    grouped
}

/// A whole number with the language's thousands separator: "57,602" / "57 602".
pub fn whole(value: f64) -> String {
    whole_in(is_norwegian(), value)
}

fn percent_in(norwegian: bool, value: f64, decimals: usize, signed: bool) -> String {
    let digits = if signed {
        format!("{value:+.decimals$}")
    } else {
        format!("{value:.decimals$}")
    };
    let digits = decimal_in(norwegian, digits);
    // Norwegian writes a space before the percent sign.
    if norwegian {
        format!("{digits} %")
    } else {
        format!("{digits}%")
    }
}

/// "50%" in English, "50 %" in Norwegian.
pub fn percent(value: f64, decimals: usize) -> String {
    percent_in(is_norwegian(), value, decimals, false)
}

/// Like `percent`, with an explicit sign: "+1.5%" / "+1,5 %".
pub fn signed_percent(value: f64, decimals: usize) -> String {
    percent_in(is_norwegian(), value, decimals, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_language_override_is_honoured() {
        if let Ok(language) = std::env::var("TIDEGAUGE_TEST_LANG") {
            assert_eq!(is_norwegian(), language == "nb");
            assert_eq!(text("Yes", "Ja"), if language == "nb" { "Ja" } else { "Yes" });
        }
    }

    #[test]
    fn plural_picks_the_singular_only_for_exactly_one() {
        for (n, en, nb) in [(0, "apps", "apper"), (1, "app", "app"), (2, "apps", "apper")] {
            assert_eq!(plural_in(false, n, "app", "apps", "app", "apper"), en);
            assert_eq!(plural_in(true, n, "app", "apps", "app", "apper"), nb);
        }
    }

    #[test]
    fn decimals_use_a_comma_only_in_norwegian() {
        assert_eq!(decimal_in(false, "6.74 GB".into()), "6.74 GB");
        assert_eq!(decimal_in(true, "6.74 GB".into()), "6,74 GB");
        assert_eq!(decimal_in(true, "1500".into()), "1500");
    }

    #[test]
    fn a_saved_language_wins_over_the_system() {
        assert!(decide(Some("nb")));
        assert!(!decide(Some("en")));
        assert_eq!(decide(None), system_is_norwegian());
        assert_eq!(decide(Some("unknown")), system_is_norwegian());
    }

    #[test]
    fn whole_numbers_group_thousands() {
        assert_eq!(whole_in(false, 57602.), "57,602");
        assert_eq!(whole_in(true, 57602.), "57\u{a0}602");
        assert_eq!(whole_in(false, 999.), "999");
        assert_eq!(whole_in(false, 1234567.), "1,234,567");
        assert_eq!(whole_in(false, -0.2), "0", "no negative zero");
        assert_eq!(whole_in(false, -1500.), "-1,500");
    }

    #[test]
    fn percent_spacing_follows_the_language() {
        assert_eq!(percent_in(false, 50.0, 0, false), "50%");
        assert_eq!(percent_in(true, 50.0, 0, false), "50 %");
        assert_eq!(percent_in(false, 12.34, 1, false), "12.3%");
        assert_eq!(percent_in(true, 12.34, 1, false), "12,3 %");
        assert_eq!(percent_in(false, 1.5, 1, true), "+1.5%");
        assert_eq!(percent_in(true, -1.5, 1, true), "-1,5 %");
    }
}
