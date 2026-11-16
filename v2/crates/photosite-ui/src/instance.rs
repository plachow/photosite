//! One PhotoSite per catalogue.
//!
//! Two windows over the same catalogue write the same database and the same
//! settings file, and whichever closes last decides what was kept. Nobody
//! means to do that: the second window is nearly always a double-click on a
//! photograph in Explorer while PhotoSite is already open. So the second one
//! hands over what it was asked to open — the photograph, the folder, or
//! nothing — to the one already running, which comes to the front and opens
//! it, and the second one leaves without ever showing a window.
//!
//! It is one per *catalogue*, not one per machine. A run with a `--data`
//! folder of its own has its own catalogue and settings and nothing to
//! quarrel with; it runs beside the other one as it always has.
//!
//! The lock is an ordinary file lock on a file in the catalogue's folder,
//! which the operating system lets go of however the process ends — a crash
//! included, so a dead PhotoSite never keeps a live one from starting. What
//! is handed over goes as a file in a folder beside it, which the running
//! one looks into a few times a second. No socket, nothing listening, and
//! nothing anybody but the owner of the catalogue's folder can write to.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The file whose lock says the catalogue is taken.
const LOCK: &str = "instance.lock";

/// Where requests from later starts wait for the running one.
const REQUESTS: &str = "requests";

/// How often the running one looks for a request. Often enough that a
/// double-click feels answered; rarely enough to cost nothing.
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// What becomes of this start.
#[derive(Debug)]
pub enum Claim {
    /// Nobody else has the catalogue: this one runs, and holds it.
    First(Instance),
    /// Another PhotoSite has it.
    Taken,
}

/// The catalogue held for as long as this lives, which is as long as the
/// application runs.
#[derive(Debug)]
pub struct Instance {
    _lock: File,
    requests: PathBuf,
}

/// Takes the catalogue in `data` for this run, or finds that another run
/// has it.
pub fn claim(data: &Path) -> std::io::Result<Claim> {
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join(LOCK))?;
    match lock.try_lock() {
        Ok(()) => {
            let requests = data.join(REQUESTS);
            // Anything still here was meant for a run that is over. Opening
            // it now, long after somebody gave up waiting, would be a
            // surprise rather than an answer.
            let _ = std::fs::remove_dir_all(&requests);
            std::fs::create_dir_all(&requests)?;
            Ok(Claim::First(Instance {
                _lock: lock,
                requests,
            }))
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(Claim::Taken),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

/// Leaves what this start was asked to open for the run that has the
/// catalogue: a photograph or a folder, or nothing, which still brings it
/// to the front.
pub fn hand_over(data: &Path, open: Option<&Path>) -> std::io::Result<()> {
    let requests = data.join(REQUESTS);
    std::fs::create_dir_all(&requests)?;
    // A path from the command line may be relative to where this start was
    // made, which is not where the other one runs.
    let open = open.map(std::path::absolute).transpose()?;
    let text = open
        .as_deref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Written under another name and renamed, so the running one never
    // reads half of it.
    let name = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let writing = requests.join(format!("{name}.writing"));
    std::fs::write(&writing, text)?;
    std::fs::rename(&writing, requests.join(format!("{name}.request")))?;

    let_it_come_forward();
    Ok(())
}

/// Windows lets a program take the foreground only when the one the user is
/// working with allows it. This start is that one — it was just launched —
/// so it passes the right on before it goes.
#[cfg(target_os = "windows")]
fn let_it_come_forward() {
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    // SAFETY: no pointers; at worst Windows says no, and the running one
    // flashes in the taskbar instead.
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
}

#[cfg(not(target_os = "windows"))]
fn let_it_come_forward() {}

impl Instance {
    /// Starts looking for requests on a thread of its own. Each one arrives
    /// on the channel as the path to open, or `None` for "come to the
    /// front", and asks the window for a frame to deal with it in.
    pub fn listen(&self, waker: egui::Context) -> crossbeam_channel::Receiver<Option<PathBuf>> {
        let (send, receive) = crossbeam_channel::unbounded();
        let requests = self.requests.clone();
        let spawned = std::thread::Builder::new()
            .name("instance requests".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(LOOK_EVERY);
                    let arrived = take(&requests);
                    if arrived.is_empty() {
                        continue;
                    }

                    for request in arrived {
                        if send.send(request).is_err() {
                            return;
                        }
                    }
                    waker.request_repaint();
                }
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "nothing will hear a second start");
        }

        receive
    }
}

/// Every request waiting, oldest first, each taken away as it is read.
fn take(requests: &Path) -> Vec<Option<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(requests) else {
        return Vec::new();
    };
    let mut waiting: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "request")
        })
        .collect();
    waiting.sort();

    waiting
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            std::fs::remove_file(&path).ok()?;
            let text = text.trim();
            Some((!text.is_empty()).then(|| PathBuf::from(text)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_on_the_same_catalogue_finds_it_taken() {
        let data = tempfile::tempdir().unwrap();
        let first = claim(data.path()).unwrap();
        assert!(matches!(first, Claim::First(_)));
        assert!(matches!(claim(data.path()).unwrap(), Claim::Taken));

        // Another catalogue is another matter.
        let other = tempfile::tempdir().unwrap();
        assert!(matches!(claim(other.path()).unwrap(), Claim::First(_)));

        // And once the first is gone, the catalogue is free again.
        drop(first);
        assert!(matches!(claim(data.path()).unwrap(), Claim::First(_)));
    }

    #[test]
    fn what_a_second_start_hands_over_arrives_once_and_in_order() {
        let data = tempfile::tempdir().unwrap();
        let Claim::First(_held) = claim(data.path()).unwrap() else {
            panic!("the catalogue was taken");
        };

        let photo = data.path().join("holiday.jpg");
        hand_over(data.path(), Some(&photo)).unwrap();
        hand_over(data.path(), None).unwrap();

        let requests = data.path().join(REQUESTS);
        assert_eq!(take(&requests), [Some(photo), None]);
        assert!(take(&requests).is_empty(), "a request was answered twice");
    }

    /// A request left by a start that came while the last run was closing
    /// is not opened by the next run, minutes or days later.
    #[test]
    fn a_request_left_from_an_earlier_run_is_not_answered_by_this_one() {
        let data = tempfile::tempdir().unwrap();
        hand_over(data.path(), Some(&data.path().join("old.jpg"))).unwrap();

        let Claim::First(_held) = claim(data.path()).unwrap() else {
            panic!("the catalogue was taken");
        };
        assert!(take(&data.path().join(REQUESTS)).is_empty());
    }
}
