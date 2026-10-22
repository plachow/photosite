//! Keeping the installed application current — when asked to.
//!
//! The whole of it is a thread and one corner of the status bar. The thread
//! starts a few seconds after the window does and asks the feed whether
//! there is a newer release. When there is, the status bar says so, with a
//! link to what changed and an *Update now* button, and that is all that
//! happens: nothing is downloaded and nothing is applied until somebody
//! presses it. A photographer half way through a cull has no use for a
//! program that swaps itself out underneath them, and Velopack applies a
//! package that has been downloaded the next time the application starts
//! whether anybody wanted that or not — which is why the download waits
//! for the button rather than running ahead of it.
//!
//! Pressing the button downloads the release, off the main thread, with
//! the status bar saying so, and then restarts into it at once.
//!
//! A build run from source is not installed — there is no `Update.exe`
//! beside it and no package manifest — and [`UpdateManager::new`] says so.
//! That is the ordinary case on a development machine and it is not an
//! error; it is logged at debug and the thread ends.

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use photosite_core::settings::Settings;
use photosite_core::t;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager};

/// How long after the window opens the question is asked. The first seconds
/// belong to the thumbnails.
const AFTER: Duration = Duration::from_secs(5);

/// What the threads have to say, in the order they say it.
#[derive(Debug)]
enum Word {
    NotInstalled,
    Asking,
    Current,
    /// A newer release exists. The manager comes with it, because it is
    /// what fetches the release when somebody asks for it.
    Available(Box<Available>),
    Downloading(String),
    /// Downloaded and waiting. The manager comes with it, because it is
    /// what applies the package.
    Ready(Box<Ready>),
    Failed(String),
}

/// A release newer than the one running, not yet downloaded.
pub struct Available {
    pub version: String,
    manager: UpdateManager,
    info: UpdateInfo,
}

impl std::fmt::Debug for Available {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Available")
            .field("version", &self.version)
            .finish()
    }
}

/// A downloaded release, and the means to restart into it.
pub struct Ready {
    pub version: String,
    manager: UpdateManager,
    info: UpdateInfo,
}

impl std::fmt::Debug for Ready {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ready")
            .field("version", &self.version)
            .finish()
    }
}

/// Where things stand, for the status bar and the diagnostics window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The settings say not to ask.
    Off,
    NotInstalled,
    Asking,
    Current,
    /// Newer, and waiting for somebody to want it.
    Available(String),
    Downloading(String),
    Ready(String),
    Failed(String),
}

impl State {
    pub fn describe(&self) -> String {
        match self {
            State::Off => t!("updates-off"),
            State::NotInstalled => t!("updates-not-installed"),
            State::Asking => t!("updates-asking"),
            State::Current => t!("updates-current"),
            State::Available(version) => t!("updates-available", version = version.as_str()),
            State::Downloading(version) => t!("updates-downloading", version = version.as_str()),
            State::Ready(version) => t!("updates-ready", version = version.as_str()),
            State::Failed(error) => t!("updates-failed", error = error.as_str()),
        }
    }
}

/// The window's side of it.
#[derive(Debug)]
pub struct Updates {
    words: Option<Receiver<Word>>,
    /// The other end, kept so that the download can report on the same
    /// line the check did.
    tx: Option<Sender<Word>>,
    waker: Arc<OnceLock<egui::Context>>,
    pub state: State,
    /// Found and offered, until somebody asks for it.
    pub available: Option<Available>,
    /// Downloaded, on the way to a restart.
    pub ready: Option<Ready>,
    /// Whether somebody pressed the button. What the status bar goes by
    /// to tell a download they asked for from a check that merely failed.
    pub wanted: bool,
}

impl Updates {
    /// Starts the check, when the settings ask for one.
    pub fn start(settings: &Settings, waker: Arc<OnceLock<egui::Context>>) -> Self {
        if !settings.updates.check {
            return Self {
                words: None,
                tx: None,
                waker,
                state: State::Off,
                available: None,
                ready: None,
                wanted: false,
            };
        }

        let (tx, rx) = crossbeam_channel::unbounded();
        let feed = settings.updates.feed.trim().to_owned();
        let thread_tx = tx.clone();
        let thread_waker = waker.clone();
        std::thread::Builder::new()
            .name("updates".to_owned())
            .spawn(move || ask(&feed, &thread_tx, &thread_waker))
            .ok();

        Self {
            words: Some(rx),
            tx: Some(tx),
            waker,
            state: State::Asking,
            available: None,
            ready: None,
            wanted: false,
        }
    }

    /// Takes in what the threads have said since the last frame.
    pub fn poll(&mut self) {
        let Some(words) = self.words.as_ref() else {
            return;
        };

        while let Ok(word) = words.try_recv() {
            self.state = match word {
                Word::NotInstalled => State::NotInstalled,
                Word::Asking => State::Asking,
                Word::Current => State::Current,
                Word::Available(available) => {
                    let version = available.version.clone();
                    self.available = Some(*available);
                    State::Available(version)
                }
                Word::Downloading(version) => State::Downloading(version),
                Word::Failed(error) => State::Failed(error),
                Word::Ready(ready) => {
                    let version = ready.version.clone();
                    self.ready = Some(*ready);
                    State::Ready(version)
                }
            };
        }
    }

    /// Somebody pressed the button: fetches the release that was offered,
    /// off the main thread. Nothing to fetch — pressed twice, or never
    /// offered — is nothing to do.
    pub fn update_now(&mut self) {
        let (Some(available), Some(tx)) = (self.available.take(), self.tx.clone()) else {
            return;
        };

        self.wanted = true;
        let waker = self.waker.clone();
        std::thread::Builder::new()
            .name("updates-download".to_owned())
            .spawn(move || fetch(available, &tx, &waker))
            .ok();
    }

    /// Whether the release somebody asked for has arrived and it is time
    /// to leave for it.
    pub fn should_restart(&self) -> bool {
        self.wanted && self.ready.is_some()
    }

    /// Leaves for the new version. Does not come back: the process ends
    /// here and Velopack starts the next one. Anything worth saving has to
    /// be saved before the call.
    pub fn restart(&self) {
        let Some(ready) = self.ready.as_ref() else {
            return;
        };

        tracing::info!(version = %ready.version, "restarting into the downloaded release");
        if let Err(error) = ready.manager.apply_updates_and_restart(&ready.info) {
            tracing::error!(%error, "the downloaded release could not be applied");
        }
    }
}

/// Where a release says what changed: its page on the feed. The workflow
/// tags a release `v<version>`, and this is the one place that knowledge
/// is needed outside it.
pub fn notes(feed: &str, version: &str) -> String {
    format!(
        "{}/releases/tag/v{version}",
        feed.trim().trim_end_matches('/')
    )
}

/// Says a word, and wakes the window so that it is seen.
fn say(tx: &Sender<Word>, waker: &OnceLock<egui::Context>, word: Word) {
    let _ = tx.send(word);
    if let Some(ctx) = waker.get() {
        ctx.request_repaint();
    }
}

/// The asking thread. Every step is reported, because a support
/// conversation about updates starts with "what did it do".
fn ask(feed: &str, tx: &Sender<Word>, waker: &OnceLock<egui::Context>) {
    std::thread::sleep(AFTER);

    // A beta is offered the next beta; a plain release is offered the next
    // plain release and never a beta in between. Decided by the version
    // this binary was built as, which is the one thing that cannot be
    // misconfigured.
    let beta = !env!("CARGO_PKG_VERSION_PRE").is_empty();
    let manager = match UpdateManager::new(GithubSource::new(feed, None, beta), None, None) {
        Ok(manager) => manager,
        Err(velopack::Error::NotInstalled(why)) => {
            tracing::debug!(%why, "not installed, so not asking for updates");
            say(tx, waker, Word::NotInstalled);
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "the updater could not start");
            say(tx, waker, Word::Failed(error.to_string()));
            return;
        }
    };

    say(tx, waker, Word::Asking);
    tracing::info!(
        feed,
        running = %manager.get_current_version_as_string(),
        "asking for a newer release"
    );

    let info = match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(info)) => info,
        Ok(_) => {
            tracing::info!("this is the newest release");
            say(tx, waker, Word::Current);
            return;
        }
        Err(error) => {
            // The network being away is the ordinary reason, and it is
            // asked again on the next start.
            tracing::warn!(%error, "the release feed could not be read");
            say(tx, waker, Word::Failed(error.to_string()));
            return;
        }
    };

    let version = info.TargetFullRelease.Version.clone();
    tracing::info!(%version, "a newer release is out; offered, not fetched");
    say(
        tx,
        waker,
        Word::Available(Box::new(Available {
            version,
            manager,
            info: *info,
        })),
    );
}

/// The fetching thread, started by the button.
fn fetch(available: Available, tx: &Sender<Word>, waker: &OnceLock<egui::Context>) {
    let Available {
        version,
        manager,
        info,
    } = available;
    tracing::info!(%version, "downloading, as asked");
    say(tx, waker, Word::Downloading(version.clone()));

    if let Err(error) = manager.download_updates(&info, None) {
        tracing::warn!(%error, %version, "the release could not be downloaded");
        say(tx, waker, Word::Failed(error.to_string()));
        return;
    }

    tracing::info!(%version, "downloaded; restarting into it");
    say(
        tx,
        waker,
        Word::Ready(Box::new(Ready {
            version,
            manager,
            info,
        })),
    );
}
