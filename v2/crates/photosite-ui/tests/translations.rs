//! Guards two things that otherwise fall apart within a week.
//!
//! First, that every key the code refers to really is in the bundle —
//! otherwise `[some-key]` appears on screen and nobody notices until somebody
//! runs into it.
//!
//! Second, that nobody handed a widget a hard-written string. That is how
//! localisation dies: not by a big decision, but by one `ui.label("Done")`
//! added in a hurry.

use std::path::{Path, PathBuf};

/// Calls whose text ends up on screen.
const WIDGETS: &[&str] = &[
    "label(",
    "button(",
    "monospace(",
    "checkbox(",
    "RichText::new(",
    "Window::new(",
    "on_hover_text(",
    "heading(",
    "selectable_value(",
    // The title of the native dialog. Not egui's, but on screen all the same.
    "set_title(",
];

/// Calls that take a translation key.
const TRANSLATIONS: &[&str] = &["t!(", "i18n::t(", "i18n::t_args("];

/// Strings that never reach the screen.
const ALLOWED: &[&str] = &["PhotoSite"];

fn sources() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for crate_dir in ["photosite-ui", "photosite-cli"] {
        collect(&manifest.join("..").join(crate_dir).join("src"), &mut found);
    }

    assert!(!found.is_empty(), "found no sources at all");
    found.sort();
    found
}

fn collect(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            into.push(path);
        }
    }
}

/// Drops line comments but keeps the lines in place, so line numbers still
/// line up. Comments are full of quotation marks; without this the test
/// reports nonsense.
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            // A quote before the `//` means we are inside a string.
            Some(at) if !line[..at].contains('"') => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Is `at` a call of its own, or merely the tail of a longer name?
/// `format!(` ends in `t!(` and without this check it would be reported over
/// and over.
fn standalone(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .map(|c| !c.is_alphanumeric() && c != '_' && c != '!')
        .unwrap_or(true)
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The literal immediately after the marker, with nothing in between. For
/// `t!("key")`, where the key is always the first argument.
///
/// A looser search — "the first literal up to the semicolon" — was here
/// before and claimed `format!("{error:#}")` from an entirely different line
/// under `i18n::t(variable)`.
fn literal_right_after(text: &str, from: usize) -> Option<(String, usize)> {
    let rest = &text[from..];
    let start = rest.find(|c: char| !c.is_whitespace())?;
    if rest.as_bytes().get(start) != Some(&b'"') {
        return None;
    }

    let tail = &rest[start + 1..];
    let end = tail.find('"')?;
    Some((tail[..end].to_owned(), from + start))
}

/// The first literal inside a call's brackets, along with what precedes it.
/// It tracks nesting, so it never runs past the end of the arguments.
fn literal_in_brackets(text: &str, from: usize) -> Option<(String, String, usize)> {
    let mut depth = 1i32;
    let mut in_string = false;
    let mut start = None;
    for (index, c) in text[from..].char_indices() {
        match c {
            '"' if in_string => {
                let at = start?;
                return Some((
                    text[from + at..from + index].to_owned(),
                    text[from..from + at].to_owned(),
                    from + at,
                ));
            }
            '"' => {
                in_string = true;
                start = Some(index + 1);
            }
            _ if in_string => {}
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return None;
                }
            }
            _ => {}
        }
    }

    None
}

fn occurrences<'a>(text: &'a str, marker: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(marker)
        .map(|(at, _)| at)
        .filter(move |at| standalone(text, *at))
}

/// Once `{…}` is taken out, is there still a word left in the literal? A
/// format string like `"{label}  {value}"` is layout, not text.
fn holds_a_word(literal: &str) -> bool {
    let mut rest = String::new();
    let mut depth = 0i32;
    for c in literal.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => rest.push(c),
            _ => {}
        }
    }

    rest.split(|c: char| !c.is_alphabetic())
        .any(|word| word.chars().count() >= 2)
}

#[test]
fn the_ui_holds_no_hard_written_text() {
    let mut sins = Vec::new();
    for path in sources() {
        let text = without_comments(&std::fs::read_to_string(&path).unwrap());
        for marker in WIDGETS {
            for at in occurrences(&text, marker) {
                let Some((literal, between, where_)) =
                    literal_in_brackets(&text, at + marker.len())
                else {
                    continue;
                };

                // A literal inside `t!()` is a key, not text.
                if TRANSLATIONS
                    .iter()
                    .any(|translation| between.contains(translation))
                {
                    continue;
                }

                if ALLOWED.contains(&literal.as_str()) || !holds_a_word(&literal) {
                    continue;
                }

                sins.push(format!(
                    "{}:{}  {marker}…{literal:?}",
                    path.file_name().unwrap().to_string_lossy(),
                    line_of(&text, where_)
                ));
            }
        }
    }

    assert!(
        sins.is_empty(),
        "text written hard instead of a translation key:\n  {}",
        sins.join("\n  ")
    );
}

#[test]
fn every_key_used_exists_in_the_bundle() {
    let mut missing = Vec::new();
    let mut found = 0;
    for path in sources() {
        let text = without_comments(&std::fs::read_to_string(&path).unwrap());
        for marker in TRANSLATIONS {
            for at in occurrences(&text, marker) {
                let Some((key, where_)) = literal_right_after(&text, at + marker.len()) else {
                    continue;
                };

                found += 1;
                if !photosite_core::i18n::has(&key) {
                    missing.push(format!(
                        "{}:{}  {key}",
                        path.file_name().unwrap().to_string_lossy(),
                        line_of(&text, where_)
                    ));
                }
            }
        }
    }

    assert!(
        found > 20,
        "only {found} keys were found — the search is probably broken"
    );
    assert!(
        missing.is_empty(),
        "keys that are not in the bundle:\n  {}",
        missing.join("\n  ")
    );
}

/// The search itself. Without this it could quietly stop finding anything and
/// nobody would learn of it — exactly the kind of gauge that lies.
#[test]
fn the_search_really_searches() {
    let sample = r#"
        ui.label("Done");
        ui.button(t!("command-file-quit"));
        let x = format!("{a:#}");
        ui.button(i18n::t(variable));
        tracing::error!(error = %format!("{error:#}"), "somethingbroke");
    "#;
    let text = without_comments(sample);

    let mut hits = Vec::new();
    for marker in WIDGETS {
        for at in occurrences(&text, marker) {
            if let Some((literal, between, _)) = literal_in_brackets(&text, at + marker.len())
                && !TRANSLATIONS.iter().any(|t| between.contains(t))
                && holds_a_word(&literal)
            {
                hits.push(literal);
            }
        }
    }

    assert_eq!(hits, vec!["Done".to_owned()], "{hits:?}");
    assert_eq!(occurrences(&text, "t!(").count(), 1, "format! is not t!");

    // A key counts only when it stands right after the bracket. Earlier,
    // `i18n::t(x)` claimed the literal from the following line and the test
    // reported nonsense.
    let keys: Vec<String> = TRANSLATIONS
        .iter()
        .flat_map(|marker| {
            occurrences(&text, marker)
                .filter_map(|at| literal_right_after(&text, at + marker.len()).map(|(key, _)| key))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(keys, vec!["command-file-quit".to_owned()], "{keys:?}");
}
