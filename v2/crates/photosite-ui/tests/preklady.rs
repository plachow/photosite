//! Hlídá dvě věci, které se jinak rozpadnou během týdne.
//!
//! Zaprvé že každý klíč, na který se kód odkazuje, v balíčku opravdu je —
//! jinak by se na obrazovce objevil `[nejaky-klic]` a nikdo by si toho nevšiml
//! do chvíle, než na to sáhne uživatel.
//!
//! Zadruhé že do widgetu nikdo nepředal text napsaný natvrdo. Právě takhle
//! lokalizace umírá: ne velkým rozhodnutím, ale jedním `ui.label("Hotovo")`
//! přidaným ve spěchu.

use std::path::{Path, PathBuf};

/// Volání, jejichž text končí na obrazovce.
const WIDGETY: &[&str] = &[
    "label(",
    "button(",
    "monospace(",
    "checkbox(",
    "RichText::new(",
    "Window::new(",
    "on_hover_text(",
    "heading(",
];

/// Volání, která berou klíč do překladu.
const PREKLADY: &[&str] = &["t!(", "i18n::t(", "i18n::t_args("];

/// Řetězce, které na obrazovku nejdou.
const POVOLENE: &[&str] = &["PhotoSite"];

fn zdrojaky() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for crate_dir in ["photosite-ui", "photosite-cli"] {
        sesbirej(&manifest.join("..").join(crate_dir).join("src"), &mut found);
    }

    assert!(!found.is_empty(), "nenašel jsem žádné zdrojáky");
    found.sort();
    found
}

fn sesbirej(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sesbirej(&path, into);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            into.push(path);
        }
    }
}

/// Vyhodí řádkové komentáře, ale nechá řádky na místě, aby seděla čísla.
/// Komentáře jsou česky a plné uvozovek; bez tohohle by test hlásil nesmysly.
fn bez_komentaru(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            // Uvozovka před `//` znamená, že jsme uvnitř řetězce.
            Some(at) if !line[..at].contains('"') => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Je na `at` samostatné volání, nebo jen konec delšího jména?
/// `format!(` končí na `t!(` a bez téhle kontroly by se hlásil pořád.
fn samostatne(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .map(|znak| !znak.is_alphanumeric() && znak != '_' && znak != '!')
        .unwrap_or(true)
}

fn radek(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// Literál hned za značkou, bez ničeho mezi. Pro `t!("klic")`, kde klíč je
/// vždycky první argument.
///
/// Volnější hledání „první literál až po středník" tu bylo dřív a přisvojilo
/// si `format!("{error:#}")` z úplně jiného řádku pod `i18n::t(promenna)`.
fn literal_hned(text: &str, from: usize) -> Option<(String, usize)> {
    let usek = &text[from..];
    let start = usek.find(|znak: char| !znak.is_whitespace())?;
    if usek.as_bytes().get(start) != Some(&b'"') {
        return None;
    }

    let zbytek = &usek[start + 1..];
    let end = zbytek.find('"')?;
    Some((zbytek[..end].to_owned(), from + start))
}

/// První literál uvnitř závorky volání, spolu s tím, co je před ním.
/// Hlídá vnoření, takže se nedostane za konec argumentů.
fn literal_v_zavorce(text: &str, from: usize) -> Option<(String, String, usize)> {
    let mut hloubka = 1i32;
    let mut v_retezci = false;
    let mut zacatek = None;
    for (index, znak) in text[from..].char_indices() {
        match znak {
            '"' if v_retezci => {
                let start = zacatek?;
                return Some((
                    text[from + start..from + index].to_owned(),
                    text[from..from + start].to_owned(),
                    from + start,
                ));
            }
            '"' => {
                v_retezci = true;
                zacatek = Some(index + 1);
            }
            _ if v_retezci => {}
            '(' => hloubka += 1,
            ')' => {
                hloubka -= 1;
                if hloubka == 0 {
                    return None;
                }
            }
            _ => {}
        }
    }

    None
}

fn vyskyty<'a>(text: &'a str, marker: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(marker)
        .map(|(at, _)| at)
        .filter(move |at| samostatne(text, *at))
}

/// Zbude v literálu po vyhození `{…}` ještě nějaké slovo? Formátovací řetězec
/// jako `"{label}  {value}"` je rozvržení, ne text.
fn obsahuje_slovo(literal: &str) -> bool {
    let mut zbytek = String::new();
    let mut hloubka = 0i32;
    for znak in literal.chars() {
        match znak {
            '{' => hloubka += 1,
            '}' => hloubka = (hloubka - 1).max(0),
            _ if hloubka == 0 => zbytek.push(znak),
            _ => {}
        }
    }

    zbytek
        .split(|c: char| !c.is_alphabetic())
        .any(|slovo| slovo.chars().count() >= 2)
}

#[test]
fn v_ui_nejsou_natvrdo_psane_texty() {
    let mut hrichy = Vec::new();
    for path in zdrojaky() {
        let text = bez_komentaru(&std::fs::read_to_string(&path).unwrap());
        for marker in WIDGETY {
            for at in vyskyty(&text, marker) {
                let Some((literal, mezi, kde)) = literal_v_zavorce(&text, at + marker.len()) else {
                    continue;
                };

                // Literál uvnitř `t!()` je klíč, ne text.
                if PREKLADY.iter().any(|preklad| mezi.contains(preklad)) {
                    continue;
                }

                if POVOLENE.contains(&literal.as_str()) || !obsahuje_slovo(&literal) {
                    continue;
                }

                hrichy.push(format!(
                    "{}:{}  {marker}…{literal:?}",
                    path.file_name().unwrap().to_string_lossy(),
                    radek(&text, kde)
                ));
            }
        }
    }

    assert!(
        hrichy.is_empty(),
        "text napsaný natvrdo místo klíče do překladu:\n  {}",
        hrichy.join("\n  ")
    );
}

#[test]
fn kazdy_pouzity_klic_v_balicku_existuje() {
    let mut chybi = Vec::new();
    let mut nalezeno = 0;
    for path in zdrojaky() {
        let text = bez_komentaru(&std::fs::read_to_string(&path).unwrap());
        for marker in PREKLADY {
            for at in vyskyty(&text, marker) {
                let Some((klic, kde)) = literal_hned(&text, at + marker.len()) else {
                    continue;
                };

                nalezeno += 1;
                if !photosite_core::i18n::has(&klic) {
                    chybi.push(format!(
                        "{}:{}  {klic}",
                        path.file_name().unwrap().to_string_lossy(),
                        radek(&text, kde)
                    ));
                }
            }
        }
    }

    assert!(
        nalezeno > 20,
        "našel jsem jen {nalezeno} klíčů — hledání je nejspíš rozbité"
    );
    assert!(
        chybi.is_empty(),
        "klíče, které v balíčku nejsou:\n  {}",
        chybi.join("\n  ")
    );
}

/// Samotný test hledání. Bez něj by mohl mlčky přestat hledat cokoliv a nikdo
/// by se to nedozvěděl — přesně ten druh měřidla, které si lže.
#[test]
fn hledani_opravdu_hleda() {
    let vzorek = r#"
        ui.label("Hotovo");
        ui.button(t!("command-file-quit"));
        let x = format!("{a:#}");
        ui.button(i18n::t(promenna));
        tracing::error!(error = %format!("{error:#}"), "necoseposralo");
    "#;
    let text = bez_komentaru(vzorek);

    let mut nalezene = Vec::new();
    for marker in WIDGETY {
        for at in vyskyty(&text, marker) {
            if let Some((literal, mezi, _)) = literal_v_zavorce(&text, at + marker.len())
                && !PREKLADY.iter().any(|p| mezi.contains(p))
                && obsahuje_slovo(&literal)
            {
                nalezene.push(literal);
            }
        }
    }

    assert_eq!(nalezene, vec!["Hotovo".to_owned()], "{nalezene:?}");
    assert_eq!(vyskyty(&text, "t!(").count(), 1, "format! není t!");

    // Klíč se bere jen když stojí hned za závorkou. Dřív si `i18n::t(x)`
    // přisvojilo literál z následujícího řádku a test hlásil nesmysl.
    let klice: Vec<String> = PREKLADY
        .iter()
        .flat_map(|marker| {
            vyskyty(&text, marker)
                .filter_map(|at| literal_hned(&text, at + marker.len()).map(|(klic, _)| klic))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(klice, vec!["command-file-quit".to_owned()], "{klice:?}");
}
