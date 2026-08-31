//! Translations.
//!
//! Everything a person will see comes from here — neither the UI nor the CLI
//! may hold a single hard-written string. Two tests guard it: one checks that
//! every key used in the code exists in the bundle, the other walks the UI
//! sources looking for literals handed to widgets.
//!
//! The format is [Fluent](https://projectfluent.org). It is not the simplest
//! choice, but it is the only one that carries languages with non-trivial
//! plurals — and Czech is exactly that: *1 fotka, 2 fotky, 5 fotek*. A plain
//! key-to-string dictionary would choke on it, and rewriting later means
//! going through every call site.
//!
//! The default language is `en-US` and it doubles as the fallback: whatever
//! is missing in another language is taken from English, so a bare key never
//! stays on screen.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};

/// Re-exported so that calling crates need not know about Fluent. The `t!`
/// macro points here, not at a foreign name in the caller's root.
pub use fluent_bundle::FluentValue;
use std::sync::{OnceLock, RwLock};
use unic_langid::{LanguageIdentifier, langid};

/// The language the source is written in, and the one we fall back to when
/// a translation is missing.
pub const FALLBACK: LanguageIdentifier = langid!("en-US");

/// The languages in the bundle. They will be added one at a time as they are
/// finished; cs-CZ is first in line.
pub fn available() -> Vec<(LanguageIdentifier, &'static str)> {
    vec![(
        langid!("en-US"),
        include_str!("../i18n/en-US/photosite.ftl"),
    )]
}

struct Loaded {
    bundle: FluentBundle<FluentResource>,
    language: LanguageIdentifier,
}

static LOADED: OnceLock<RwLock<Loaded>> = OnceLock::new();

fn build(language: &LanguageIdentifier) -> Loaded {
    // English first as the fallback, then the requested language on top.
    // Fluent returns the first message it finds, which is why the order is
    // this way round.
    let mut bundle = FluentBundle::new_concurrent(vec![language.clone(), FALLBACK]);
    // Without this, Fluent wraps interpolated values in invisible
    // bidirectional-text marks. On a button label that makes a mess.
    bundle.set_use_isolating(false);

    // fluent-bundle does not register `NUMBER()` on its own, and without it
    // the text literally reads `{NUMBER()}`. It is needed so every
    // translation can say how many places numbers are rounded to — that is a
    // decision of the language, not of the code.
    let registered = bundle.add_function("NUMBER", |positional, named| {
        match positional.first() {
            Some(FluentValue::Number(number)) => {
                let mut number = number.clone();
                // fluent-bundle 0.16 will accept `maximumFractionDigits` but
                // will not round by it — `as_string` looks only at the
                // minimum. So we round ourselves, otherwise 4.1667 ms would
                // never become 4 ms.
                if let Some(FluentValue::Number(digits)) = named.get("maximumFractionDigits") {
                    let factor = 10f64.powi(digits.value.max(0.0) as i32);
                    number.value = (number.value * factor).round() / factor;
                }

                number.options.merge(named);
                FluentValue::Number(number)
            }
            Some(other) => other.clone(),
            None => FluentValue::Error,
        }
    });
    if let Err(error) = registered {
        tracing::error!(%error, "NUMBER could not be registered");
    }

    let mut add = |source: &'static str, name: &LanguageIdentifier| {
        match FluentResource::try_new(source.to_owned()) {
            Ok(resource) => {
                if let Err(errors) = bundle.add_resource(resource) {
                    // A key duplicated between the language and the fallback
                    // is normal; anything else is a fault in the bundle and
                    // has to be visible.
                    tracing::debug!(language = %name, ?errors, "part of the translation was not added");
                }
            }
            Err((_, errors)) => {
                tracing::error!(language = %name, ?errors, "the translation is damaged")
            }
        }
    };

    let sources = available();
    if let Some((name, source)) = sources.iter().find(|(id, _)| id == language) {
        add(source, name);
    }

    if language != &FALLBACK
        && let Some((name, source)) = sources.iter().find(|(id, _)| id == &FALLBACK)
    {
        add(source, name);
    }

    Loaded {
        bundle,
        language: language.clone(),
    }
}

/// Picks a language from what the user wants and what is available.
///
/// `wanted` is whatever stands in the settings or arrived on the command
/// line; an unknown or empty language ends at English, not at a panic.
///
/// The negotiation is deliberately hand-written and short: an exact match
/// first, then a match on the language alone without the region (`en-GB` to
/// `en-US`), otherwise the fallback. A library exists for this, but it drags
/// in its own version of `unic-langid`, which will not meet Fluent's.
pub fn negotiate(wanted: Option<&str>) -> LanguageIdentifier {
    let Some(wanted) = wanted.filter(|text| !text.is_empty()) else {
        return FALLBACK;
    };

    let Ok(requested) = wanted.parse::<LanguageIdentifier>() else {
        tracing::warn!(
            language = wanted,
            "unknown language tag, carrying on in English"
        );
        return FALLBACK;
    };

    let supported: Vec<LanguageIdentifier> = available().into_iter().map(|(id, _)| id).collect();
    if let Some(found) = supported.iter().find(|have| *have == &requested) {
        return found.clone();
    }

    if let Some(found) = supported
        .iter()
        .find(|have| have.language == requested.language)
    {
        return found.clone();
    }

    tracing::info!(
        language = wanted,
        "no translation available, carrying on in English"
    );
    FALLBACK
}

/// Sets the language. Called once at startup, and again when the setting
/// changes.
pub fn set_language(language: &LanguageIdentifier) {
    let lock = LOADED.get_or_init(|| RwLock::new(build(language)));
    let mut loaded = lock.write().expect("poisoned lock");
    if &loaded.language != language {
        *loaded = build(language);
    }

    tracing::info!(language = %language, "language set");
}

/// Which language is currently switched on.
pub fn language() -> LanguageIdentifier {
    LOADED
        .get()
        .and_then(|lock| lock.read().ok().map(|loaded| loaded.language.clone()))
        .unwrap_or(FALLBACK)
}

fn lookup(key: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
    let lock = LOADED.get_or_init(|| RwLock::new(build(&FALLBACK)));
    let loaded = lock.read().ok()?;
    let message = loaded.bundle.get_message(key)?;
    let pattern = message.value()?;
    let mut errors = Vec::new();
    let text = loaded.bundle.format_pattern(pattern, args, &mut errors);
    if !errors.is_empty() {
        tracing::warn!(key, ?errors, "the translation could not be assembled");
    }

    Some(text.into_owned())
}

/// Translates a key. A missing key comes back in square brackets — on screen
/// that is visible at a glance, and in a test it fails.
pub fn t(key: &str) -> String {
    lookup(key, None).unwrap_or_else(|| {
        tracing::error!(key, "missing translation");
        format!("[{key}]")
    })
}

/// Translates a key with values interpolated into it.
pub fn t_args(key: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    let mut fluent = FluentArgs::new();
    for (name, value) in args {
        fluent.set(*name, value.clone());
    }

    lookup(key, Some(&fluent)).unwrap_or_else(|| {
        tracing::error!(key, "missing translation");
        format!("[{key}]")
    })
}

/// Does such a key exist? For the tests that guard the bundle's completeness.
pub fn has(key: &str) -> bool {
    let lock = LOADED.get_or_init(|| RwLock::new(build(&FALLBACK)));
    lock.read()
        .ok()
        .map(|loaded| loaded.bundle.get_message(key).is_some())
        .unwrap_or(false)
}

/// `t!("key")` or `t!("key", count = 5)`.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::t($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::t_args(
            $key,
            &[$((stringify!($name), $crate::i18n::FluentValue::from($value))),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_english_bundle_parses() {
        for (language, source) in available() {
            let result = FluentResource::try_new(source.to_owned());
            assert!(result.is_ok(), "the {language} translation failed to load");
        }
    }

    #[test]
    fn an_unknown_language_ends_at_english() {
        assert_eq!(negotiate(None), FALLBACK);
        assert_eq!(negotiate(Some("")), FALLBACK);
        assert_eq!(negotiate(Some("this is not a language")), FALLBACK);
        assert_eq!(negotiate(Some("sv-SE")), FALLBACK);
    }

    #[test]
    fn english_is_found_without_a_region_too() {
        assert_eq!(negotiate(Some("en")), langid!("en-US"));
        assert_eq!(negotiate(Some("en-GB")), langid!("en-US"));
    }

    #[test]
    fn a_missing_key_is_visible_and_does_not_bring_the_app_down() {
        assert_eq!(t("no-such-key"), "[no-such-key]");
        assert!(!has("no-such-key"));
    }

    #[test]
    fn numbers_are_rounded_the_way_the_bundle_asks() {
        // Without NUMBER registered, this would literally read "{NUMBER()}".
        let text = t!("gallery-count", count = 2i64, ms = 4.1667f64);
        assert!(text.contains("4 ms"), "{text}");
        assert!(!text.contains("NUMBER"), "{text}");

        // And to one decimal place, the way the scan output wants it.
        let text = t!(
            "cli-scan-done",
            added = 1i64,
            skipped = 0i64,
            failed = 0i64,
            seconds = 12.3456f64,
            rate = 987.65f64
        );
        assert!(text.contains("12.3 s"), "{text}");
        assert!(text.contains("988 files/s"), "{text}");
    }

    #[test]
    fn interpolating_a_value_works() {
        let text = t!("gallery-count", count = 3);
        assert!(text.contains('3'), "{text}");
        assert!(
            !text.starts_with('['),
            "the gallery-count key is missing from the bundle"
        );
    }
}
