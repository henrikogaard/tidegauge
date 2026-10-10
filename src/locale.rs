use std::sync::OnceLock;

pub fn text(english: &'static str, norwegian: &'static str) -> &'static str {
    static NORWEGIAN: OnceLock<bool> = OnceLock::new();
    if *NORWEGIAN.get_or_init(|| {
        objc2_foundation::NSLocale::preferredLanguages()
            .firstObject()
            .map(|language| {
                matches!(
                    language.to_string().split(['-', '_']).next(),
                    Some("nb" | "nn" | "no")
                )
            })
            .unwrap_or(false)
    }) {
        norwegian
    } else {
        english
    }
}
