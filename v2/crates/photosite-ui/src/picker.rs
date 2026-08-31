//! The native folder picker.
//!
//! The whole module exists because of two things there is no way around.
//!
//! **The dialog is created on the main thread, but awaited elsewhere.** macOS
//! will only show the panel pinned to the window when the main thread asks
//! for it; from anywhere else it falls back to a modal window in the middle
//! of the screen, or straight to a panic. But we must not wait for the answer
//! there: somebody browses a disk for a minute at a time, and for all of it
//! not one tile would be redrawn — Windows marks such a window unresponsive
//! after a few seconds and greys out its title bar. So the future is born
//! here and finished on a thread alongside.
//!
//! **At most one may be open.** A second Ctrl+O would otherwise stack another
//! dialog on top of the first, and one of them would be left hanging even
//! after a folder was chosen.

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

/// A dialog that is currently open.
#[derive(Debug)]
pub struct Picker {
    from_dialog: Receiver<Option<PathBuf>>,
}

/// What the dialog has said so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Still open.
    Waiting,
    /// Closed without a choice. Nothing happened and nothing is reported —
    /// cancelling a dialog is an ordinary answer, not a failure.
    Cancelled,
    Picked(PathBuf),
}

/// Opens the dialog. Call only from the main thread, that is from inside the
/// render pass.
pub fn ask(ctx: &egui::Context, title: String, start: Option<PathBuf>) -> Picker {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }

    // Here, on the main thread. Moving this line into the thread below looks
    // like a simplification and breaks macOS.
    let opened = dialog.pick_folder();
    let (to_ui, from_dialog) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let picked = pollster::block_on(opened);
        let path = picked.map(|handle| handle.path().to_path_buf());
        tracing::info!(chosen = ?path, "dialog closed");
        // A failure here means one thing only: the window has closed in the
        // meantime and the answer has nowhere to go.
        let _ = to_ui.send(path);
        // Without this the answer would sit in the channel until the next
        // repaint, which by `loading.idle_repaint_ms` can be a quarter of a
        // second after somebody clicked Select.
        ctx.request_repaint();
    });

    Picker { from_dialog }
}

impl Picker {
    pub fn answer(&self) -> Answer {
        match self.from_dialog.try_recv() {
            Ok(Some(folder)) => Answer::Picked(folder),
            Ok(None) => Answer::Cancelled,
            Err(TryRecvError::Empty) => Answer::Waiting,
            // The thread ended without answering. Treating that as "still
            // open" would mean Ctrl+O never works again for the rest of the
            // run.
            Err(TryRecvError::Disconnected) => {
                tracing::warn!("the dialog ended without an answer");
                Answer::Cancelled
            }
        }
    }
}

/// Where to open the dialog.
///
/// Best is the folder being looked at; when it has been deleted in the
/// meantime, or the drive unplugged, then the nearest ancestor that still
/// exists. A floor above is still closer than wherever the dialog would land
/// on its own. When no folder is open, the last one from the settings is
/// used — after a restart it is the only trace of the person we have.
pub fn start_dir(current: Option<&Path>, last: Option<&str>) -> Option<PathBuf> {
    let wanted = current
        .map(Path::to_path_buf)
        .or_else(|| last.filter(|text| !text.is_empty()).map(PathBuf::from))?;

    wanted
        .ancestors()
        .find(|path| path.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_starts_where_we_are() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            start_dir(Some(dir.path()), None).as_deref(),
            Some(dir.path())
        );
    }

    #[test]
    fn a_deleted_folder_moves_up_one_floor() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("2019").join("summer");
        assert_eq!(start_dir(Some(&gone), None).as_deref(), Some(dir.path()));
    }

    #[test]
    fn with_no_folder_open_the_last_one_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let last = dir.path().to_string_lossy().into_owned();
        assert_eq!(start_dir(None, Some(&last)).as_deref(), Some(dir.path()));
    }

    #[test]
    fn an_open_folder_beats_the_last_one() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let last = elsewhere.path().to_string_lossy().into_owned();
        assert_eq!(
            start_dir(Some(dir.path()), Some(&last)).as_deref(),
            Some(dir.path())
        );
    }

    #[test]
    fn with_nothing_at_all_the_dialog_decides_for_itself() {
        // `None` means "tell it nothing", not "start at the root of the disk".
        assert_eq!(start_dir(None, None), None);
        assert_eq!(start_dir(None, Some("")), None);
        assert_eq!(start_dir(None, Some("Q:/no such drive/photos")), None);
    }
}
