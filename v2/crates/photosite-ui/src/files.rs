//! Doing things to the files themselves: renaming, duplicating, deleting,
//! and showing them to the system's own file manager.
//!
//! Two rules run through all of it.
//!
//! **The catalogue follows the file.** A rename moves the row rather than
//! forgetting one and writing another, because the rating, the label and the
//! words all hang off its number. People rename files constantly and would
//! never think to be careful about it.
//!
//! **Deleting goes to the recycle bin.** Never `remove_file`. A photograph is
//! not something to be brave about, and the one place in this application
//! that could destroy one for good is the one place that should not exist.

use crate::App;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A name being typed into a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asking {
    Rename { path: PathBuf, name: String },
    NewFolder { inside: PathBuf, name: String },
}

/// Renames a photograph and takes its row with it.
pub fn rename(app: &mut App, from: &Path, to_name: &str) -> Result<PathBuf> {
    let to_name = to_name.trim();
    anyhow::ensure!(!to_name.is_empty(), "a file needs a name");
    anyhow::ensure!(
        !to_name.contains(['/', '\\', ':']),
        "a name cannot hold a path separator"
    );

    let to = from
        .parent()
        .map(|parent| parent.join(to_name))
        .context("this file is not in a folder")?;
    if to == from {
        return Ok(to);
    }

    anyhow::ensure!(!to.exists(), "{} is already there", to_name);

    std::fs::rename(from, &to).with_context(|| format!("cannot rename {}", from.display()))?;

    // The sidecar is part of the photograph as far as anybody is concerned;
    // leaving it behind under the old name loses everything it holds.
    let from_sidecar = photosite_meta::sidecar_of(from);
    if from_sidecar.exists() {
        let to_sidecar = photosite_meta::sidecar_of(&to);
        if let Err(error) = std::fs::rename(&from_sidecar, &to_sidecar) {
            tracing::warn!(
                from = %from_sidecar.display(),
                %error,
                "the photograph was renamed but its sidecar was not"
            );
        }
    }

    let (from_owned, to_owned) = (from.to_path_buf(), to.clone());
    app.write_catalog(move |catalog| catalog.moved(&from_owned, &to_owned));
    Ok(to)
}

/// Copies photographs beside themselves, under a name that is free.
pub fn duplicate(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut made = Vec::with_capacity(paths.len());
    for path in paths {
        let to = unused_name(path);
        std::fs::copy(path, &to).with_context(|| format!("cannot copy {}", path.display()))?;
        made.push(to);
    }

    Ok(made)
}

/// To the recycle bin, never to nowhere.
///
/// The catalogue forgets them only once the system has taken them: a delete
/// that failed and a row that vanished anyway is the worst of both.
pub fn delete(app: &mut App, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    // Sidecars go with their photographs. One left behind would be picked up
    // as an orphan by every cataloguer that looks.
    let mut all: Vec<PathBuf> = Vec::with_capacity(paths.len() * 2);
    for path in paths {
        all.push(path.clone());
        let sidecar = photosite_meta::sidecar_of(path);
        if sidecar.exists() {
            all.push(sidecar);
        }
    }

    trash::delete_all(&all).context("the recycle bin would not take them")?;

    let gone = paths.to_vec();
    app.write_catalog(move |catalog| catalog.forget(&gone));
    Ok(())
}

pub fn new_folder(inside: &Path, name: &str) -> Result<PathBuf> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "a folder needs a name");
    anyhow::ensure!(
        !name.contains(['/', '\\', ':']),
        "a name cannot hold a path separator"
    );

    let path = inside.join(name);
    std::fs::create_dir(&path).with_context(|| format!("cannot make {}", path.display()))?;
    Ok(path)
}

/// A name beside this one that nothing is using.
///
/// `holiday.jpg` becomes `holiday (2).jpg`, then `holiday (3).jpg`. The
/// number goes before the extension and not after it, or the copy stops being
/// a photograph as far as everything else is concerned.
pub fn unused_name(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();

    // Two is where a second copy starts. Nobody calls the second one "1".
    for number in 2..10_000 {
        let candidate = parent.join(format!("{stem} ({number}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!("{stem} (copy){extension}"))
}

/// Hands the photograph to the system's own file manager.
///
/// Every platform spells this differently and none of them can be relied on,
/// so a failure is logged and nothing more: not being able to open Explorer
/// is not a reason to interrupt somebody.
pub fn reveal(path: &Path) {
    let outcome = show(path);
    if let Err(error) = outcome {
        tracing::warn!(path = %path.display(), %error, "cannot show the file");
    }
}

#[cfg(target_os = "windows")]
fn show(path: &Path) -> std::io::Result<()> {
    // The comma is not a typo and there is no space after it: Explorer takes
    // `/select,<path>` as one argument and does nothing at all otherwise.
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn show(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn show(path: &Path) -> std::io::Result<()> {
    // No agreed way to select a file, so the folder is opened instead. Half
    // an answer beats none, and every desktop has `xdg-open`.
    let folder = path.parent().unwrap_or(path);
    std::process::Command::new("xdg-open")
        .arg(folder)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_is_numbered_before_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("holiday.jpg");
        std::fs::write(&path, b"x").unwrap();

        let second = unused_name(&path);
        assert_eq!(second.file_name().unwrap(), "holiday (2).jpg");
        assert_eq!(
            second.extension().unwrap(),
            "jpg",
            "the copy stopped being a photograph"
        );

        std::fs::write(&second, b"x").unwrap();
        assert_eq!(
            unused_name(&path).file_name().unwrap(),
            "holiday (3).jpg",
            "the second copy took the first one's name"
        );
    }

    #[test]
    fn a_file_with_no_extension_still_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("holiday");
        std::fs::write(&path, b"x").unwrap();
        assert_eq!(unused_name(&path).file_name().unwrap(), "holiday (2)");
    }

    #[test]
    fn duplicating_copies_the_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, b"the photograph").unwrap();

        let made = duplicate(&[path]).unwrap();
        assert_eq!(made.len(), 1);
        assert_eq!(std::fs::read(&made[0]).unwrap(), b"the photograph");
    }

    #[test]
    fn a_folder_cannot_be_named_into_another_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(new_folder(dir.path(), "holiday").is_ok());
        assert!(new_folder(dir.path(), "").is_err());
        assert!(new_folder(dir.path(), "  ").is_err());
        assert!(
            new_folder(dir.path(), "../elsewhere").is_err(),
            "a name with a separator in it is a path, not a name"
        );
        assert!(new_folder(dir.path(), "a/b").is_err());
    }
}
