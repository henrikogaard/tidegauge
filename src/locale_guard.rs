//! Source-level guard that keeps the English and Norwegian Bokmål UI text
//! isolated.
//!
//! The test parses every `src/*.rs` file (skipping `#[cfg(test)]` items) and
//! checks two things:
//!
//! 1. Every `locale::text(english, norwegian)` pair (through any of its import
//!    aliases) is well formed: no Norwegian letters on the English side, a
//!    real translation on the Norwegian side, and one English string maps to
//!    one Norwegian string everywhere.
//! 2. No string literal with English words reaches a UI text sink without
//!    going through the locale function.
//!
//! When it fails, translate the text with `l(english, norwegian)` (or the
//! `count`/`plural` helpers) rather than extending an allowlist. Allowlists
//! are for proper nouns, units and symbols only.

use std::collections::BTreeMap;
use std::path::Path;

use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprMacro, ExprMethodCall, Lit, Token};

/// Names under which `crate::locale::text` is imported or called.
const LOCALE_FNS: &[&str] = &["l", "localized", "t", "text"];

/// Norwegian text may equal its English source when the English has no word
/// outside `ALLOWED_BARE_WORDS` (units, symbols, technical terms), or when it
/// is one of these words that Norwegian keeps as is.
const SAME_IN_BOTH: &[&str] = &[
    "App", "app", "Bluetooth", "Disk", "Server", "server", "System",
    // Language names are written in their own language in both UIs.
    "English", "Norsk",
    // A network port number: "port 8765" in both languages.
    "port",
];

/// One English string may map to several Norwegian strings only here, each
/// with the reason (a different sense in context).
const ENGLISH_WITH_SEVERAL_NORWEGIAN: &[&str] = &[
    // "up for 3 hours" (uptime) versus "Move X up" (direction).
    "up",
];

/// Words that may appear in bare UI literals: proper nouns, technical terms
/// and unit symbols that read the same in both languages.
const ALLOWED_BARE_WORDS: &[&str] = &[
    "APFS", "CPU", "GPU", "PID", "UID", "RPM", "SMART", "SSD", "HDD", "RAM", "USB", "Tidegauge",
    "Apple", "NVMe", "IORegistry", "diskutil", "KiB", "MiB", "GiB", "TiB", "KB", "MB", "GB", "TB", "MHz", "GHz",
    "Mbps", "mAh", "mW", "min",
];

/// Multi-word proper nouns that may appear in bare UI literals.
const ALLOWED_PHRASES: &[&str] = &["Neural Engine", "Apple GPU"];

/// Struct fields that hold visible text (`FooterAction { label: "..." }`).
const TEXT_FIELDS: &[&str] = &["label", "title", "text", "headline", "explanation", "message"];

/// Helpers that take an English and a Norwegian string as separate
/// arguments: `(function name, [(english index, norwegian index)])`.
const PAIR_FNS: &[(&str, &[(usize, usize)])] = &[
    ("refused", &[(1, 2)]),
    ("refusal", &[(0, 1)]),
    ("error_message", &[(0, 1)]),
];

/// `locale::count(n, one_en, many_en, one_nb, many_nb)` and `locale::plural`.
const COUNT_PAIRS: &[(usize, usize)] = &[(1, 3), (2, 4)];

/// `(function or method name, argument indexes that become visible text)`.
const SINKS: &[(&str, &[usize])] = &[
    ("child", &[0]),
    ("aria_label", &[0]),
    ("sub", &[0]),
    ("caption", &[0]),
    ("section_header", &[0, 1]),
    ("stat_card", &[0]),
    ("empty_state", &[2, 3]),
    ("button", &[1]),
    ("button_accent", &[1]),
    ("button_indexed", &[1]),
    ("button_labeled", &[1, 2]),
    ("segment", &[1]),
    ("segment_group", &[1]),
    ("stepper_group", &[1]),
    ("step_button", &[1, 2]),
    ("preference", &[1]),
    ("preference_labeled", &[1, 2]),
    ("step_preference", &[1, 2]),
];

#[derive(Default)]
struct Findings {
    /// `(file, line, english, norwegian)`
    pairs: Vec<(String, usize, String, String)>,
    /// `(file, line, what)`
    bare: Vec<(String, usize, String)>,
}

struct Scan<'a> {
    file: &'a str,
    out: &'a mut Findings,
}

fn is_test_item(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

fn str_lit(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(s) => Some(s.value()),
            _ => None,
        },
        Expr::Group(g) => str_lit(&g.expr),
        Expr::Paren(p) => str_lit(&p.expr),
        _ => None,
    }
}

fn last_segment(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// Is this call path the locale function (any alias, or `locale::text`)?
fn is_locale_call(func: &Expr) -> bool {
    let Expr::Path(p) = func else { return false };
    let segments = &p.path.segments;
    let Some(last) = segments.last() else {
        return false;
    };
    let name = last.ident.to_string();
    if !LOCALE_FNS.contains(&name.as_str()) {
        return false;
    }
    if segments.len() == 1 {
        return true;
    }
    // `crate::locale::text`, `locale::text`.
    segments.len() >= 2 && segments[segments.len() - 2].ident == "locale"
}

/// Words (3+ ASCII letters) in a literal once `{...}` placeholders are gone.
fn english_words(text: &str) -> Vec<String> {
    let mut stripped = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                stripped.push(' ');
            }
            '{' => {
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
                stripped.push(' ');
            }
            _ => stripped.push(c),
        }
    }
    let mut words = Vec::new();
    let mut current = String::new();
    for c in stripped.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphabetic() {
            current.push(c);
        } else {
            if current.len() >= 3 {
                words.push(std::mem::take(&mut current));
            }
            current.clear();
        }
    }
    words
}

fn offending_words(text: &str) -> Vec<String> {
    let mut text = text.to_string();
    for phrase in ALLOWED_PHRASES {
        text = text.replace(phrase, " ");
    }
    english_words(&text)
        .into_iter()
        .filter(|w| !ALLOWED_BARE_WORDS.contains(&w.as_str()))
        .collect()
}

impl Scan<'_> {
    fn line(&self, span: proc_macro2::Span) -> usize {
        span.start().line
    }

    fn record_pairs(&mut self, call: &ExprCall, specs: &[(usize, usize)]) {
        for &(en, nb) in specs {
            let (Some(en), Some(nb)) = (
                call.args.get(en).and_then(str_lit),
                call.args.get(nb).and_then(str_lit),
            ) else {
                continue;
            };
            let line = self.line(syn::spanned::Spanned::span(call));
            self.out.pairs.push((self.file.to_string(), line, en, nb));
        }
    }

    /// Check an expression that becomes visible text.
    fn check_text(&mut self, expr: &Expr, sink: &str) {
        match expr {
            Expr::Lit(_) => {
                if let Some(text) = str_lit(expr) {
                    self.flag(expr, &text, sink);
                }
            }
            Expr::Group(g) => self.check_text(&g.expr, sink),
            Expr::Paren(p) => self.check_text(&p.expr, sink),
            Expr::Reference(r) => self.check_text(&r.expr, sink),
            Expr::Block(b) => {
                if let Some(syn::Stmt::Expr(last, None)) = b.block.stmts.last() {
                    self.check_text(last, sink);
                }
            }
            Expr::If(i) => {
                if let Some(syn::Stmt::Expr(last, None)) = i.then_branch.stmts.last() {
                    self.check_text(last, sink);
                }
                if let Some((_, other)) = &i.else_branch {
                    self.check_text(other, sink);
                }
            }
            Expr::Match(m) => {
                for arm in &m.arms {
                    self.check_text(&arm.body, sink);
                }
            }
            // `"literal".to_string()`, `.into()`, `.to_uppercase()` ...
            Expr::MethodCall(m)
                if matches!(
                    m.method.to_string().as_str(),
                    "to_string" | "into" | "to_owned" | "to_uppercase" | "to_lowercase"
                ) =>
            {
                self.check_text(&m.receiver, sink)
            }
            // `String::from("literal")`, `SharedString::from("literal")`.
            Expr::Call(c) if c.args.len() == 1 && last_segment(&c.func).as_deref() == Some("from") => {
                self.check_text(&c.args[0], sink)
            }
            Expr::Macro(m) if m.mac.path.is_ident("format") => {
                if let Ok(args) = m
                    .mac
                    .parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
                {
                    if let Some(template) = args.first().and_then(str_lit) {
                        self.flag(expr, &template, sink);
                    }
                }
            }
            _ => {}
        }
    }

    fn flag(&mut self, at: &Expr, text: &str, sink: &str) {
        // NSString also carries identifiers, keys and paths; only a literal
        // with spaces is prose.
        if sink == "NSString::from_str" && !text.contains(char::is_whitespace) {
            return;
        }
        let words = offending_words(text);
        if words.is_empty() {
            return;
        }
        let line = self.line(syn::spanned::Spanned::span(at));
        self.out.bare.push((
            self.file.to_string(),
            line,
            format!("{sink}: {text:?} (words: {})", words.join(", ")),
        ));
    }

    fn sink_args<'e>(
        &mut self,
        name: &str,
        args: impl Iterator<Item = &'e Expr>,
        is_method: bool,
    ) {
        // `.child` and `.aria_label` are methods; the others are plain
        // functions. A same-named function of the other kind is not a sink.
        let Some((_, indexes)) = SINKS.iter().find(|(sink, _)| {
            *sink == name && matches!(*sink, "child" | "aria_label") == is_method
        }) else {
            return;
        };
        for (i, arg) in args.enumerate() {
            if indexes.contains(&i) {
                self.check_text(arg, name);
            }
        }
    }

    /// Visit the expressions inside a macro body when they parse as a comma
    /// separated list (`format!`, `vec!`, `assert_eq!`, ...). Otherwise look
    /// for locale calls in the raw tokens.
    fn macro_body(&mut self, mac: &syn::Macro) {
        if let Ok(args) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            // Any `format!` template with prose words is UI text in waiting:
            // it usually lands in a `let` that feeds a sink later.
            if mac.path.is_ident("format") {
                if let Some(template) = args.first().and_then(str_lit) {
                    // SQL and other machine text is not UI text.
                    let machine = ["SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE ", "PRAGMA "]
                        .iter()
                        .any(|keyword| template.starts_with(keyword));
                    if template.contains(char::is_whitespace) && !machine {
                        let words = offending_words(&template);
                        if !words.is_empty() {
                            let line = self.line(syn::spanned::Spanned::span(&mac.path));
                            self.out.bare.push((
                                self.file.to_string(),
                                line,
                                format!("format!: {template:?} (words: {})", words.join(", ")),
                            ));
                        }
                    }
                }
            }
            for arg in &args {
                self.visit_expr(arg);
            }
        } else {
            self.scan_tokens(mac.tokens.clone());
        }
    }

    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        use proc_macro2::{Delimiter, TokenTree};
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut i = 0;
        while i < trees.len() {
            if let TokenTree::Group(group) = &trees[i] {
                if let (Some(TokenTree::Ident(name)), true) = (
                    i.checked_sub(1).map(|p| &trees[p]),
                    group.delimiter() == Delimiter::Parenthesis,
                ) {
                    if LOCALE_FNS.contains(&name.to_string().as_str()) {
                        if let Ok(args) = syn::parse::Parser::parse2(
                            Punctuated::<Expr, Token![,]>::parse_terminated,
                            group.stream(),
                        ) {
                            if args.len() == 2 {
                                if let (Some(en), Some(nb)) = (str_lit(&args[0]), str_lit(&args[1])) {
                                    self.out.pairs.push((
                                        self.file.to_string(),
                                        name.span().start().line,
                                        en,
                                        nb,
                                    ));
                                }
                            }
                        }
                    }
                }
                self.scan_tokens(group.stream());
            }
            i += 1;
        }
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = match item {
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::Use(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            _ => &[],
        };
        if is_test_item(attrs) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attrs: &[syn::Attribute] = match item {
            syn::ImplItem::Fn(f) => &f.attrs,
            syn::ImplItem::Const(c) => &c.attrs,
            _ => &[],
        };
        if is_test_item(attrs) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if is_locale_call(&call.func) {
            if call.args.len() == 2 {
                self.record_pairs(call, &[(0, 1)]);
                if let Some(en) = call.args.first().and_then(str_lit) {
                    if looks_like_plural_noun(&en) {
                        let line = self.line(syn::spanned::Spanned::span(call));
                        self.out.bare.push((
                            self.file.to_string(),
                            line,
                            format!(
                                "locale text: {en:?} is a plural noun; use locale::count or \
                                 locale::plural so that one item reads \"1 item\""
                            ),
                        ));
                    }
                }
            }
            // The arguments are literals; nothing below to check.
            return;
        }
        if let Expr::Path(p) = &*call.func {
            let segments: Vec<String> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
            let name = segments.last().map(String::as_str).unwrap_or_default();
            let in_locale = segments.len() >= 2 && segments[segments.len() - 2] == "locale";
            if in_locale && matches!(name, "count" | "plural") {
                self.record_pairs(call, COUNT_PAIRS);
            } else if let Some((_, specs)) = PAIR_FNS.iter().find(|(f, _)| *f == name) {
                self.record_pairs(call, specs);
            }
        }
        if let Some(name) = last_segment(&call.func) {
            self.sink_args(&name, call.args.iter(), false);
        }
        // Native menu titles, tooltips and notification text.
        if let Expr::Path(p) = &*call.func {
            let n = p.path.segments.len();
            if n >= 2
                && p.path.segments[n - 2].ident == "NSString"
                && p.path.segments[n - 1].ident == "from_str"
            {
                if let Some(arg) = call.args.first() {
                    self.check_text(arg, "NSString::from_str");
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let name = call.method.to_string();
        self.sink_args(&name, call.args.iter(), true);
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        for field in &expr.fields {
            if let syn::Member::Named(name) = &field.member {
                if TEXT_FIELDS.contains(&name.to_string().as_str()) {
                    self.check_text(&field.expr, &format!("field `{name}`"));
                }
            }
        }
        visit::visit_expr_struct(self, expr);
    }

    fn visit_expr_macro(&mut self, mac: &'ast ExprMacro) {
        self.macro_body(&mac.mac);
    }

    fn visit_stmt_macro(&mut self, mac: &'ast syn::StmtMacro) {
        self.macro_body(&mac.mac);
    }
}

fn source_files() -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read src")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        // This file quotes the patterns it forbids.
        .filter(|path| path.file_name().is_some_and(|n| n != "locale_guard.rs"))
        .collect();
    files.sort();
    files
}

fn scan_source(file: &str, source: &str) -> Findings {
    let mut findings = Findings::default();
    let parsed = syn::parse_file(source).unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut scan = Scan {
        file,
        out: &mut findings,
    };
    scan.visit_file(&parsed);
    findings
}

fn scan_tree() -> Findings {
    let mut all = Findings::default();
    for path in source_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).expect("read source");
        let mut found = scan_source(&name, &source);
        all.pairs.append(&mut found.pairs);
        all.bare.append(&mut found.bare);
    }
    all
}

/// A lone lowercase plural noun ("apps", "samples") is a count label that
/// needs a singular form.
fn looks_like_plural_noun(english: &str) -> bool {
    // `temperature_display_caption` takes the language as a parameter so that
    // tests can fix it; it picks the singular itself.
    english != "channels"
        && english.len() >= 4
        && english.chars().all(|c| c.is_ascii_lowercase())
        && english.ends_with('s')
        && !english.ends_with("ss")
        && !english.ends_with("us")
        && !english.ends_with("is")
}

fn has_digit_before(text: &str, mark: &str) -> bool {
    text.match_indices(mark)
        .any(|(i, _)| text[..i].chars().next_back().is_some_and(|c| c.is_ascii_digit()))
}

fn has_decimal_point(text: &str) -> bool {
    // A macOS version ("macOS 14.2") is a version, not a decimal.
    let text = text.replace("macOS 14.2", "macOS");
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == '.' && w[2].is_ascii_digit())
}

fn has_norwegian_letter(text: &str) -> bool {
    text.chars().any(|c| "æøåÆØÅ".contains(c))
}

#[test]
fn locale_pairs_are_well_formed() {
    let found = scan_tree();
    assert!(
        found.pairs.len() > 500,
        "the scan found only {} locale pairs; the guard is probably not parsing the tree",
        found.pairs.len()
    );

    let mut problems = Vec::new();
    let mut by_english: BTreeMap<&str, Vec<(&str, &str, usize)>> = BTreeMap::new();
    for (file, line, en, nb) in &found.pairs {
        if has_norwegian_letter(en) {
            problems.push(format!("{file}:{line}: English side has a Norwegian letter: {en:?}"));
        }
        if en == nb && !SAME_IN_BOTH.contains(&en.as_str()) && !offending_words(en).is_empty() {
            problems.push(format!(
                "{file}:{line}: Norwegian equals English and is not allowlisted: {en:?}"
            ));
        }
        // Norwegian puts a space before the percent sign and uses a decimal
        // comma; English does neither.
        if has_digit_before(nb, "%") {
            problems.push(format!("{file}:{line}: Norwegian percent needs a space: {nb:?}"));
        }
        if has_digit_before(en, " %") {
            problems.push(format!("{file}:{line}: English percent has a space: {en:?}"));
        }
        if has_decimal_point(nb) {
            problems.push(format!("{file}:{line}: Norwegian decimal point: {nb:?}"));
        }
        by_english.entry(en).or_default().push((nb, file, *line));
    }
    for (en, uses) in &by_english {
        let mut distinct: Vec<&str> = uses.iter().map(|(nb, _, _)| *nb).collect();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() > 1 && !ENGLISH_WITH_SEVERAL_NORWEGIAN.contains(en) {
            let places: Vec<String> = uses
                .iter()
                .map(|(nb, file, line)| format!("{file}:{line} -> {nb:?}"))
                .collect();
            problems.push(format!(
                "{en:?} has several Norwegian translations: {}",
                places.join("; ")
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

/// One message per source line (a `format!` inside a sink is found twice).
fn bare_lines(found: &Findings) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    found
        .bare
        .iter()
        .filter(|(file, line, _)| seen.insert((file.clone(), *line)))
        .map(|(file, line, what)| format!("{file}:{line}: bare text in {what}"))
        .collect()
}

#[test]
fn ui_text_goes_through_the_locale_function() {
    let lines = bare_lines(&scan_tree());
    assert!(
        lines.is_empty(),
        "\nWrap these in l(english, norwegian) instead of showing one language:\n{}\n",
        lines.join("\n")
    );
}

#[test]
fn the_guard_detects_the_patterns_it_forbids() {
    let source = r#"
        fn view() {
            div().child("Processes");
            sub(format!("{} samples", n));
            caption("Memory", dark);
            div().child(if on { "Online" } else { l("Off", "Av") });
            let _ = l("Håndtert", "Handled");
            let _ = l("Same", "Same");
            let _ = l("apps", "apper");
            let _ = crate::locale::count(n, "item", "items", "element", "elementer");
            let _ = refused(&target, "Blocked", "Blokkert");
            let _ = vec![l("Inside macro", "I makro")];
            let _ = FooterAction { id: "open-tools", label: "Tools" };
            item.setTitle(&NSString::from_str("Quit now"));
            let _ = serde_json::from_str("not shown to anyone");
            #[cfg(test)]
            fn ignored() { div().child("Ignored words"); }
        }
    "#;
    let found = scan_source("sample.rs", source);
    let lines = bare_lines(&found);
    assert_eq!(lines.len(), 7, "{lines:#?}");
    for (line, expected) in [
        (3, "Processes"),
        (4, "samples"),
        (5, "Memory"),
        (6, "Online"),
        (9, "plural noun"),
        (14, "Quit now"),
        (13, "field `label`"),
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with(&format!("sample.rs:{line}:")) && l.contains(expected)),
            "line {line} should report {expected}: {lines:#?}"
        );
    }
    let pairs: Vec<(&str, &str)> = found
        .pairs
        .iter()
        .map(|(_, _, en, nb)| (en.as_str(), nb.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("Off", "Av"),
            ("Håndtert", "Handled"),
            ("Same", "Same"),
            ("apps", "apper"),
            ("item", "element"),
            ("items", "elementer"),
            ("Blocked", "Blokkert"),
            ("Inside macro", "I makro"),
        ]
    );
    assert!(has_norwegian_letter(pairs[1].0));
}

#[test]
fn placeholders_and_symbols_are_not_words() {
    assert!(offending_words("{} · {:.1}%").is_empty());
    assert!(offending_words("{value} GB").is_empty());
    assert!(offending_words("{} h {} min").is_empty());
    assert!(offending_words("Neural Engine").is_empty());
    assert_eq!(offending_words("{{braces}}"), ["braces"]);
    assert_eq!(offending_words("Wired memory"), ["Wired", "memory"]);
}

#[test]
fn percent_and_decimal_checks_follow_the_language_rules() {
    assert!(has_digit_before("50%", "%"));
    assert!(!has_digit_before("50 %", "%"));
    assert!(has_digit_before("50 %", " %"));
    assert!(has_decimal_point("6.74 GB"));
    assert!(!has_decimal_point("Version. 2"));
}
