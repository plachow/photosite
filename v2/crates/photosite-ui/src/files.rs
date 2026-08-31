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
//!
//! **Nothing at a destination is written over, ever.** A copy that lands on
//! a name already in use takes a number instead. Where every file is going
//! is worked out by [`photosite_core::transfer`] before a byte moves, and
//! that is where the reasoning — and the tests — live.

use crate::App;
use anyhow::{Context, Result};
use photosite_core::transfer::{self, Mode, Planned};
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
///
/// Which is the same question as copying them into their own folder, and is
/// answered in the same place.
pub fn duplicate(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut made = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(folder) = path.parent() else {
            continue;
        };
        for step in transfer::plan(std::slice::from_ref(path), folder, Mode::Copy, &|path| {
            path.exists()
        }) {
            carry(&step, Mode::Copy)?;
            made.push(step.to);
        }
    }

    Ok(made)
}

/// Copies or moves photographs into another folder.
///
/// The catalogue follows a move, the same as it follows a rename: the rating
/// and the words hang off the row's number, and forgetting one row to write
/// another loses them.
pub fn transfer(app: &mut App, files: &[PathBuf], into: &Path, mode: Mode) -> Result<usize> {
    anyhow::ensure!(into.is_dir(), "{} is not a folder", into.display());

    let planned = transfer::plan(files, into, mode, &|path| path.exists());
    let mut done = 0usize;
    for step in &planned {
        carry(step, mode)?;
        done += 1;
        if mode == Mode::Move {
            let (from, to) = (step.from.clone(), step.to.clone());
            app.write_catalog(move |catalog| catalog.moved(&from, &to));
        }
    }

    Ok(done)
}

/// One file and its sidecar, along the way the plan laid out.
fn carry(step: &Planned, mode: Mode) -> Result<()> {
    one(&step.from, &step.to, mode)
        .with_context(|| format!("cannot move {}", step.from.display()))?;

    // The sidecar is part of the photograph as far as anybody is concerned.
    // A failure here is not a failure of the operation — the photograph did
    // arrive — but it is not something to be quiet about either.
    let (from, to) = &step.sidecar;
    if from.exists()
        && let Err(error) = one(from, to, mode)
    {
        tracing::warn!(
            from = %from.display(),
            %error,
            "the photograph arrived but its sidecar did not"
        );
    }

    Ok(())
}

/// A move across volumes is not a rename.
///
/// `std::fs::rename` refuses to cross a drive on Windows and a mount
/// elsewhere, and moving photographs from a card to a library is exactly
/// that. So a refused rename becomes a copy and then a delete of the
/// original — in that order, because the other way round loses the file if
/// the copy fails.
fn one(from: &Path, to: &Path, mode: Mode) -> std::io::Result<()> {
    if mode == Mode::Copy {
        return std::fs::copy(from, to).map(|_| ());
    }

    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
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

/// Hands a web address to whatever the system opens them with.
///
/// The one place this application reaches outside itself, and it is a link
/// somebody clicked. A failure is logged and nothing more: not being able to
/// open a browser is not a reason to interrupt anybody.
pub fn open_link(url: &str) {
    if let Err(error) = browse(url) {
        tracing::warn!(%url, %error, "cannot open the link");
    }
}

#[cfg(target_os = "windows")]
fn browse(url: &str) -> std::io::Result<()> {
    // Through the shell, because a URL is not a program. The empty string is
    // the window title `start` insists on eating first, or an address in
    // quotation marks becomes one.
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn browse(url: &str) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn browse(url: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
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

    /// Numbering is the core's rule and is tested there. What matters here
    /// is that a duplicate takes its sidecar with it: a RAW's stars live in
    /// the sidecar and nowhere else, so a copy without one is a copy with
    /// nothing said about it.
    #[test]
    fn a_duplicate_takes_its_sidecar_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("holiday.nef");
        std::fs::write(&path, b"x").unwrap();
        std::fs::write(dir.path().join("holiday.xmp"), b"<x:xmpmeta/>").unwrap();

        let made = duplicate(&[path]).unwrap();
        assert_eq!(made[0].file_name().unwrap(), "holiday (2).nef");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("holiday (2).xmp")).unwrap(),
            "<x:xmpmeta/>",
            "the copy was made without what was said about it"
        );
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

    #[test]
    fn moving_takes_the_photograph_and_its_sidecar_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (from, into) = (dir.path().join("from"), dir.path().join("into"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&into).unwrap();
        let photo = from.join("holiday.nef");
        std::fs::write(&photo, b"pixels").unwrap();
        std::fs::write(from.join("holiday.xmp"), b"stars").unwrap();

        let (mut app, _data, _photos) = crate::culling::three();
        let count = transfer(&mut app, std::slice::from_ref(&photo), &into, Mode::Move).unwrap();

        assert_eq!(count, 1);
        assert!(!photo.exists(), "the original stayed behind");
        assert!(!from.join("holiday.xmp").exists());
        assert_eq!(std::fs::read(into.join("holiday.nef")).unwrap(), b"pixels");
        assert_eq!(std::fs::read(into.join("holiday.xmp")).unwrap(), b"stars");
    }

    /// The one that would be quiet and unrecoverable.
    #[test]
    fn a_copy_never_writes_over_what_is_already_there() {
        let dir = tempfile::tempdir().unwrap();
        let (from, into) = (dir.path().join("from"), dir.path().join("into"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&into).unwrap();
        std::fs::write(from.join("a.jpg"), b"the new one").unwrap();
        std::fs::write(into.join("a.jpg"), b"the one already there").unwrap();

        let (mut app, _data, _photos) = crate::culling::three();
        transfer(&mut app, &[from.join("a.jpg")], &into, Mode::Copy).unwrap();

        assert_eq!(
            std::fs::read(into.join("a.jpg")).unwrap(),
            b"the one already there",
            "a photograph was written over"
        );
        assert_eq!(
            std::fs::read(into.join("a (2).jpg")).unwrap(),
            b"the new one"
        );
    }

    #[test]
    fn a_destination_that_is_not_a_folder_is_refused_rather_than_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, b"x").unwrap();

        let (mut app, _data, _photos) = crate::culling::three();
        assert!(
            transfer(
                &mut app,
                std::slice::from_ref(&photo),
                &dir.path().join("nowhere"),
                Mode::Copy
            )
            .is_err()
        );
        assert!(transfer(&mut app, std::slice::from_ref(&photo), &photo, Mode::Copy).is_err());
        assert!(photo.exists());
    }
}
