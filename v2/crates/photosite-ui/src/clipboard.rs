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
//! **One photograph copied is its picture as well.** A clipboard holds the
//! same thing in several forms at once and whoever pastes takes the form
//! they understand: Explorer and a mail attachment take the file, Paint and
//! a letter take the picture. So there is one Copy, not a "copy file" and a
//! "copy image" for somebody to choose between. The picture is decoded on a
//! thread of its own — a full-size photograph is a quarter of a second, and
//! `Ctrl+C` must not stall the window — and added to what is already there
//! only if nothing else has been copied in the meantime.
//!
//! **The toolkit is kept away from the clipboard on Windows.** Its way of
//! putting text there empties it first, at the end of the frame — after the
//! files have gone on — so a copy here pasted in Explorer as nothing at all.
//! The paths go on as text alongside the files instead, in the same breath,
//! except beside a picture: a program offered text and a picture takes the
//! text, and a letter full of file names is not what anybody copied a
//! photograph for.
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
    // As text as well. It costs nothing, and a path one can paste into a
    // terminal is worth having.
    let text = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let picture = match paths {
        [one] if !cut => Some(one.as_path()),
        _ => None,
    };

    if !system_put(paths, cut, &text, picture) {
        ctx.copy_text(text);
    }

    Held {
        paths: paths.to_vec(),
        cut,
    }
}

/// A picture as Windows keeps one on the clipboard: `CF_DIB`, which is a
/// `BITMAPINFOHEADER` followed by the rows, blue first, each padded to four
/// bytes.
///
/// The rows go **bottom to top**. The format allows top to bottom by giving
/// a negative height, and Word refuses to paste a picture that does. The
/// system makes every other bitmap format out of this one on request.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn dib(picture: &photosite_image::Rgb) -> Vec<u8> {
    let (width, height) = (picture.width as usize, picture.height as usize);
    let line = width * 3;
    let stride = line.div_ceil(4) * 4;

    let mut out = Vec::with_capacity(40 + stride * height);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    // One plane, twenty-four bits, uncompressed.
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((stride * height) as u32).to_le_bytes());
    // No resolution and no palette.
    out.extend_from_slice(&[0u8; 16]);

    for row in (0..height).rev() {
        let pixels = &picture.pixels[row * line..(row + 1) * line];
        for pixel in pixels.as_chunks::<3>().0 {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        out.extend(std::iter::repeat_n(0u8, stride - line));
    }

    out
}

/// What to paste: the system's files if it has any, else ours.
pub fn take(held: &Held) -> Held {
    match system_take() {
        Some(from_system) => from_system,
        None => held.clone(),
    }
}

/// Speaks to the system's clipboard itself, and says whether it did. When
/// it did not, the toolkit is left to put the text there.
#[cfg(all(windows, not(test)))]
fn system_put(paths: &[PathBuf], cut: bool, text: &str, picture: Option<&Path>) -> bool {
    {
        use clipboard_win::{Clipboard, Setter, formats, options};

        // One open, every format. Opening twice would clear the first write:
        // emptying the clipboard is part of taking it.
        let opened = Clipboard::new_attempts(10);
        let Ok(_clipboard) = opened else {
            tracing::warn!("the clipboard would not open");
            return false;
        };

        if let Err(error) = clipboard_win::empty() {
            tracing::warn!(%error, "the clipboard would not empty");
            return false;
        }

        let names: Vec<String> = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        if let Err(error) = formats::FileList.write_clipboard(&names) {
            tracing::warn!(%error, "the files would not go on the clipboard");
            return false;
        }

        drop_effect(cut);
        if picture.is_none()
            && let Err(error) = clipboard_win::raw::set_string_with(text, options::NoClear)
        {
            tracing::warn!(%error, "the paths would not go on the clipboard as text");
        }
    }

    // Counted once the clipboard is closed again, so the picture can tell
    // whether anybody has copied anything since.
    if let Some(path) = picture {
        picture_later(path.to_path_buf(), clipboard_win::raw::seq_num());
    }

    true
}

/// Whether the files are to be moved or copied when pasted.
#[cfg(all(windows, not(test)))]
fn drop_effect(cut: bool) {
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

/// Decodes the photograph whole and adds it to the clipboard as a picture,
/// beside the file already there.
///
/// Only if the clipboard is still the one this copy left: a picture landing
/// on top of whatever somebody copied in the meantime would be a picture of
/// the wrong thing in the wrong place.
#[cfg(all(windows, not(test)))]
fn picture_later(path: PathBuf, since: Option<std::num::NonZeroU32>) {
    let spawned = std::thread::Builder::new()
        .name("clipboard picture".to_owned())
        .spawn(move || {
            let picture = match photosite_image::sized(&path, u32::MAX) {
                Ok(picture) => picture,
                Err(error) => {
                    // The file is on the clipboard and pastes as a file; a
                    // photograph that cannot be decoded here has no picture
                    // to add, and that is all.
                    tracing::debug!(path = %path.display(), error = %format!("{error:#}"), "no picture to put beside the file");
                    return;
                }
            };
            let bytes = dib(&picture);
            drop(picture);

            let Ok(_clipboard) = clipboard_win::Clipboard::new_attempts(10) else {
                tracing::warn!("the clipboard would not open for the picture");
                return;
            };
            if clipboard_win::raw::seq_num() != since {
                tracing::debug!("something else was copied meanwhile; the picture stays out");
                return;
            }

            if let Err(error) =
                clipboard_win::raw::set_without_clear(clipboard_win::formats::CF_DIB, &bytes)
            {
                tracing::warn!(%error, "the picture would not go on the clipboard");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "the picture could not be prepared");
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
fn system_put(_: &[PathBuf], _: bool, _: &str, _: Option<&Path>) -> bool {
    false
}

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

    /// Two by two: a red and a green pixel over a blue and a white one.
    #[test]
    fn a_picture_goes_on_bottom_row_first_blue_first_and_padded() {
        let picture =
            photosite_image::Rgb::new(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
                .unwrap();

        let bytes = dib(&picture);
        assert_eq!(&bytes[0..4], &40u32.to_le_bytes(), "the header size");
        assert_eq!(&bytes[4..8], &2i32.to_le_bytes());
        assert_eq!(
            &bytes[8..12],
            &2i32.to_le_bytes(),
            "a negative height is one Word will not paste"
        );
        assert_eq!(&bytes[14..16], &24u16.to_le_bytes());
        // Six bytes a row, padded to eight.
        assert_eq!(bytes.len(), 40 + 8 * 2);
        assert_eq!(
            &bytes[40..48],
            &[255, 0, 0, 255, 255, 255, 0, 0],
            "the bottom row, blue then white"
        );
        assert_eq!(
            &bytes[48..56],
            &[0, 0, 255, 0, 255, 0, 0, 0],
            "then red and green"
        );
    }

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
