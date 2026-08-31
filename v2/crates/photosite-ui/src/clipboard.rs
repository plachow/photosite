//! Files on the clipboard.
//!
//! Two clipboards, and it matters which is which.
//!
//! **Ours** is a list of paths held in the application. It works on every
//! platform, it survives whatever else lands on the system clipboard in the
//! meantime, and it is what `Ctrl+V` inside PhotoSite reads.
//!
//! **The system's** is what makes a copy here paste in the file manager, and
//! a copy there paste here. Windows has one agreed way to put files on a
//! clipboard — `CF_HDROP`, with `Preferred DropEffect` alongside to say
//! whether it was a copy or a cut — and every other platform has several
//! disagreeing ones. So the system clipboard is spoken to on Windows and
//! left alone elsewhere, where the paths go on as text instead: pasting them
//! into a terminal or a dialog is a real use, and claiming more than that
//! would be pretending.
//!
//! The system's answer wins when it has one. Somebody who copied a file in
//! Explorer and pressed `Ctrl+V` here means that file, not whatever they
//! copied in PhotoSite ten minutes ago.
//!
//! **The system clipboard is left alone under test.** A test that wrote to it
//! would take it out of the hands of whoever is running the tests, and one
//! that read from it would pass or fail by what they happened to have copied
//! last. Ours is what the tests exercise; the platform code is still compiled
//! and linted, it is simply not called.

use photosite_core::transfer::Mode;
use std::path::{Path, PathBuf};

/// What was last copied here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    pub paths: Vec<PathBuf>,
    /// A cut, rather than a copy. Nothing is moved until it is pasted.
    pub cut: bool,
}

impl Held {
    pub fn mode(&self) -> Mode {
        if self.cut { Mode::Move } else { Mode::Copy }
    }
}

/// Puts the files on both clipboards.
pub fn put(ctx: &egui::Context, paths: &[PathBuf], cut: bool) -> Held {
    system_put(paths, cut);

    // As text as well, on every platform including Windows. It costs nothing
    // and a path one can paste into a terminal is worth having.
    ctx.copy_text(
        paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );

    Held {
        paths: paths.to_vec(),
        cut,
    }
}

/// What to paste: the system's files if it has any, else ours.
pub fn take(held: &Held) -> Held {
    match system_take() {
        Some(from_system) => from_system,
        None => held.clone(),
    }
}

#[cfg(all(windows, not(test)))]
fn system_put(paths: &[PathBuf], cut: bool) {
    use clipboard_win::{Clipboard, Setter, formats};

    // One open, both formats. Opening twice would clear the first write:
    // emptying the clipboard is part of taking it.
    let opened = Clipboard::new_attempts(10);
    let Ok(_clipboard) = opened else {
        tracing::warn!("the clipboard would not open");
        return;
    };

    if let Err(error) = clipboard_win::empty() {
        tracing::warn!(%error, "the clipboard would not empty");
        return;
    }

    let names: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    if let Err(error) = formats::FileList.write_clipboard(&names) {
        tracing::warn!(%error, "the files would not go on the clipboard");
        return;
    }

    // 1 is copy and 2 is move, and without it Explorer assumes a copy. A cut
    // that pastes as a copy leaves the original behind, which is the one
    // outcome nobody would notice until much later.
    let effect: u32 = if cut { 2 } else { 1 };
    match clipboard_win::register_format("Preferred DropEffect") {
        Some(format) => {
            if let Err(error) =
                clipboard_win::raw::set_without_clear(format.get(), &effect.to_le_bytes())
            {
                tracing::warn!(%error, "the drop effect would not go on the clipboard");
            }
        }
        None => tracing::warn!("the drop effect format could not be registered"),
    }
}

#[cfg(all(windows, not(test)))]
fn system_take() -> Option<Held> {
    use clipboard_win::{Clipboard, Getter, formats};

    let _clipboard = Clipboard::new_attempts(10).ok()?;
    let mut names: Vec<String> = Vec::new();
    formats::FileList.read_clipboard(&mut names).ok()?;
    if names.is_empty() {
        return None;
    }

    let cut = clipboard_win::register_format("Preferred DropEffect")
        .and_then(|format| {
            let mut bytes: Vec<u8> = Vec::new();
            clipboard_win::raw::get_vec(format.get(), &mut bytes).ok()?;
            bytes.first().copied()
        })
        // 2 is a move. Anything else, including nothing at all, is a copy —
        // the safe way to be wrong.
        .is_some_and(|effect| effect == 2);

    Some(Held {
        paths: names.into_iter().map(PathBuf::from).collect(),
        cut,
    })
}

#[cfg(any(not(windows), test))]
fn system_put(_: &[PathBuf], _: bool) {}

#[cfg(any(not(windows), test))]
fn system_take() -> Option<Held> {
    None
}

/// The files worth putting on a clipboard: the ones that are really there.
///
/// A path in the catalogue whose file has since been deleted elsewhere would
/// otherwise be pasted as a failure.
pub fn existing(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for path in paths {
        if path.exists() && !found.iter().any(|already| already == path) {
            found.push(path.clone());
        }
    }

    found
}

/// Only what can actually be pasted: files, not folders, that still exist.
pub fn pastable(held: &Held) -> Vec<PathBuf> {
    held.paths
        .iter()
        .filter(|path| path.is_file())
        .filter(|path| photosite_core::is_photo(path) || is_sidecar(path))
        .cloned()
        .collect()
}

/// A sidecar pasted on its own is still worth carrying: somebody who copied
/// a whole folder in Explorer means the lot.
fn is_sidecar(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.eq_ignore_ascii_case("xmp"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_is_a_move_and_a_copy_is_not() {
        assert_eq!(Held::default().mode(), Mode::Copy);
        assert_eq!(
            Held {
                cut: true,
                ..Default::default()
            }
            .mode(),
            Mode::Move
        );
    }

    #[test]
    fn what_is_not_there_does_not_go_on_the_clipboard() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("a.jpg");
        std::fs::write(&real, b"x").unwrap();
        let gone = dir.path().join("b.jpg");

        let kept = existing(&[real.clone(), gone, real.clone()]);
        assert_eq!(kept, [real], "a missing file or a repeat got through");
    }

    #[test]
    fn only_photographs_and_their_sidecars_are_pasted() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.jpg", "a.xmp", "notes.txt", "raw.nef"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }

        let held = Held {
            paths: ["a.jpg", "a.xmp", "notes.txt", "raw.nef", "nothing.jpg"]
                .iter()
                .map(|name| dir.path().join(name))
                .collect(),
            cut: false,
        };
        let names: Vec<String> = pastable(&held)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.jpg", "a.xmp", "raw.nef"]);
    }
}
