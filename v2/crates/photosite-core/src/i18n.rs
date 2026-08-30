//! Překlady.
//!
//! Všechno, co uvidí člověk, se bere odsud — v UI ani v CLI nesmí být jediný
//! natvrdo napsaný řetězec. Hlídají to dva testy: jeden ověří, že každý klíč
//! použitý v kódu v balíčku existuje, druhý prochází zdrojáky UI a hledá
//! literály předané widgetům.
//!
//! Formát je [Fluent](https://projectfluent.org). Není to nejjednodušší
//! volba, ale je jediná, která unese jazyky s netriviálními plurály — a
//! čeština je přesně takový: *1 fotka, 2 fotky, 5 fotek*. Prostý slovník
//! „klíč → řetězec" by se na tom zasekl a přepisovat to pak znamená projít
//! všechna volání.
//!
//! Výchozí jazyk je `en-US` a je zároveň záložní: co v jiném jazyce chybí,
//! se vezme z angličtiny, aby na obrazovce nikdy nezůstal holý klíč.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};

/// Znovu vystavené, aby volající crate nemusely znát Fluent. Makro `t!`
/// odkazuje sem, ne na cizí jméno v kořeni volajícího.
pub use fluent_bundle::FluentValue;
use std::sync::{OnceLock, RwLock};
use unic_langid::{LanguageIdentifier, langid};

/// Jazyk, ve kterém je napsaný zdroj a na který se spadne, když překlad chybí.
pub const FALLBACK: LanguageIdentifier = langid!("en-US");

/// Jazyky, které jsou v balíčku. Přibývat sem budou po jednom, jak budou
/// hotové; cs-CZ je první na řadě.
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
    // Nejdřív angličtina jako záloha, pak požadovaný jazyk navrch. Fluent
    // vrací první nalezenou zprávu, takže pořadí je právě takhle.
    let mut bundle = FluentBundle::new_concurrent(vec![language.clone(), FALLBACK]);
    // Bez tohohle Fluent obaluje dosazené hodnoty neviditelnými znaky pro
    // obousměrný text. Na štítku tlačítka to dělá nepořádek.
    bundle.set_use_isolating(false);

    // `NUMBER()` si fluent-bundle sám nezaregistruje, a bez ní se v textu
    // objeví doslova `{NUMBER()}`. Je potřeba, aby si mohl každý překlad říct,
    // na kolik míst se čísla zaokrouhlují — to je rozhodnutí jazyka, ne kódu.
    let registered = bundle.add_function("NUMBER", |positional, named| {
        match positional.first() {
            Some(FluentValue::Number(number)) => {
                let mut number = number.clone();
                // fluent-bundle 0.16 umí `maximumFractionDigits` jen přijmout,
                // ne podle něj zaokrouhlit — `as_string` se dívá výhradně na
                // minimum. Zaokrouhlíme tedy sami, jinak by z 4,1667 ms nikdy
                // nebyly 4 ms.
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
        tracing::error!(%error, "NUMBER se nepodařilo zaregistrovat");
    }

    let mut add = |source: &'static str, name: &LanguageIdentifier| {
        match FluentResource::try_new(source.to_owned()) {
            Ok(resource) => {
                if let Err(errors) = bundle.add_resource(resource) {
                    // Duplicitní klíč mezi jazykem a zálohou je normální;
                    // cokoliv jiného je chyba v balíčku a musí být vidět.
                    tracing::debug!(jazyk = %name, ?errors, "část překladu se nepřidala");
                }
            }
            Err((_, errors)) => {
                tracing::error!(jazyk = %name, ?errors, "překlad je poškozený")
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

/// Vybere jazyk podle přání uživatele a toho, co je k dispozici.
///
/// `wanted` je to, co stojí v nastavení nebo přišlo z příkazové řádky;
/// neznámý nebo prázdný jazyk skončí na angličtině, ne na pádu.
///
/// Vyjednávání je schválně ruční a krátké: nejdřív přesná shoda, pak shoda
/// na samotném jazyce bez regionu (`en-GB` → `en-US`), jinak záloha. Knihovna
/// na tohle existuje, ale táhne si vlastní verzi `unic-langid`, která se
/// s tou Fluentovou nepotká.
pub fn negotiate(wanted: Option<&str>) -> LanguageIdentifier {
    let Some(wanted) = wanted.filter(|text| !text.is_empty()) else {
        return FALLBACK;
    };

    let Ok(requested) = wanted.parse::<LanguageIdentifier>() else {
        tracing::warn!(jazyk = wanted, "neznámé označení jazyka, jedeme anglicky");
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

    tracing::info!(jazyk = wanted, "překlad není k dispozici, jedeme anglicky");
    FALLBACK
}

/// Nastaví jazyk. Volá se jednou při startu; podruhé při změně v nastavení.
pub fn set_language(language: &LanguageIdentifier) {
    let lock = LOADED.get_or_init(|| RwLock::new(build(language)));
    let mut loaded = lock.write().expect("otrávený zámek");
    if &loaded.language != language {
        *loaded = build(language);
    }

    tracing::info!(jazyk = %language, "jazyk nastaven");
}

/// Který jazyk je právě zapnutý.
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
        tracing::warn!(klic = key, ?errors, "překlad se nepodařilo složit");
    }

    Some(text.into_owned())
}

/// Přeloží klíč. Chybějící klíč se vrátí v hranatých závorkách — na obrazovce
/// je to vidět na první pohled a v testu to spadne.
pub fn t(key: &str) -> String {
    lookup(key, None).unwrap_or_else(|| {
        tracing::error!(klic = key, "chybějící překlad");
        format!("[{key}]")
    })
}

/// Přeloží klíč s dosazenými hodnotami.
pub fn t_args(key: &str, args: &[(&str, FluentValue<'_>)]) -> String {
    let mut fluent = FluentArgs::new();
    for (name, value) in args {
        fluent.set(*name, value.clone());
    }

    lookup(key, Some(&fluent)).unwrap_or_else(|| {
        tracing::error!(klic = key, "chybějící překlad");
        format!("[{key}]")
    })
}

/// Existuje takový klíč? Pro testy, které hlídají úplnost balíčku.
pub fn has(key: &str) -> bool {
    let lock = LOADED.get_or_init(|| RwLock::new(build(&FALLBACK)));
    lock.read()
        .ok()
        .map(|loaded| loaded.bundle.get_message(key).is_some())
        .unwrap_or(false)
}

/// `t!("klic")` nebo `t!("klic", pocet = 5)`.
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
    fn anglicky_balicek_se_da_prelozit() {
        for (language, source) in available() {
            let result = FluentResource::try_new(source.to_owned());
            assert!(result.is_ok(), "překlad {language} se nepodařilo načíst");
        }
    }

    #[test]
    fn neznamy_jazyk_skonci_na_anglictine() {
        assert_eq!(negotiate(None), FALLBACK);
        assert_eq!(negotiate(Some("")), FALLBACK);
        assert_eq!(negotiate(Some("tohle není jazyk")), FALLBACK);
        assert_eq!(negotiate(Some("sv-SE")), FALLBACK);
    }

    #[test]
    fn anglictina_se_najde_i_bez_regionu() {
        assert_eq!(negotiate(Some("en")), langid!("en-US"));
        assert_eq!(negotiate(Some("en-GB")), langid!("en-US"));
    }

    #[test]
    fn chybejici_klic_je_videt_a_neshodi_aplikaci() {
        assert_eq!(t("takovy-klic-neexistuje"), "[takovy-klic-neexistuje]");
        assert!(!has("takovy-klic-neexistuje"));
    }

    #[test]
    fn cisla_se_zaokrouhluji_podle_balicku() {
        // Bez zaregistrované funkce NUMBER by tu stálo doslova "{NUMBER()}".
        let text = t!("gallery-count", count = 2i64, ms = 4.1667f64);
        assert!(text.contains("4 ms"), "{text}");
        assert!(!text.contains("NUMBER"), "{text}");

        // A na jedno desetinné místo, jak to chce výpis skenu.
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
    fn dosazeni_hodnoty_funguje() {
        let text = t!("gallery-count", count = 3);
        assert!(text.contains('3'), "{text}");
        assert!(!text.starts_with('['), "klíč gallery-count chybí v balíčku");
    }
}
