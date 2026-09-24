//! Keeping the installed application current.
//!
//! The whole of it is one thread and one line on the status row. The
//! thread starts a few seconds after the window does, asks the feed whether
//! there is a newer release and, when there is, downloads it into Velopack's
//! packages folder. Nothing on the screen changes until the download is
//! complete — a photographer half way through a cull has no use for a
//! progress bar about software. Then the status row says the version is
//! there and offers a restart; whoever ignores it gets the new version the
//! next time PhotoSite starts, because [`startup::first`](crate::startup)
//! applies a downloaded package before anything else.
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

/// What the thread has to say, in the order it says it.
#[derive(Debug)]
enum Word {
    NotInstalled,
    Asking,
    Current,
    Downloading(String),
    /// Downloaded and waiting. The manager comes with it, because it is
    /// what applies the package.
    Ready(Box<Ready>),
    Failed(String),
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

/// Where things stand, for the diagnostics window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The settings say not to ask.
    Off,
    NotInstalled,
    Asking,
    Current,
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
    pub state: State,
    pub ready: Option<Ready>,
}

impl Updates {
    /// Starts the check, when the settings ask for one.
    pub fn start(settings: &Settings, waker: Arc<OnceLock<egui::Context>>) -> Self {
        if !settings.updates.check {
            return Self {
                words: None,
                state: State::Off,
                ready: None,
            };
        }

        let (tx, rx) = crossbeam_channel::unbounded();
        let feed = settings.updates.feed.trim().to_owned();
        std::thread::Builder::new()
            .name("updates".to_owned())
            .spawn(move || ask(&feed, &tx, &waker))
            .ok();

        Self {
            words: Some(rx),
            state: State::Asking,
            ready: None,
        }
    }

    /// Takes in what the thread has said since the last frame.
    pub fn poll(&mut self) {
        let Some(words) = self.words.as_ref() else {
            return;
        };

        while let Ok(word) = words.try_recv() {
            self.state = match word {
                Word::NotInstalled => State::NotInstalled,
                Word::Asking => State::Asking,
                Word::Current => State::Current,
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

/// The thread. Every step is reported, because a support conversation about
/// updates starts with "what did it do".
fn ask(feed: &str, tx: &Sender<Word>, waker: &OnceLock<egui::Context>) {
    let say = |word: Word| {
        let _ = tx.send(word);
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }
    };

    std::thread::sleep(AFTER);

    let manager = match UpdateManager::new(GithubSource::new(feed, None, false), None, None) {
        Ok(manager) => manager,
        Err(velopack::Error::NotInstalled(why)) => {
            tracing::debug!(%why, "not installed, so not asking for updates");
            say(Word::NotInstalled);
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "the updater could not start");
            say(Word::Failed(error.to_string()));
            return;
        }
    };

    say(Word::Asking);
    tracing::info!(
        feed,
        running = %manager.get_current_version_as_string(),
        "asking for a newer release"
    );

    let info = match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(info)) => info,
        Ok(_) => {
            tracing::info!("this is the newest release");
            say(Word::Current);
            return;
        }
        Err(error) => {
            // The network being away is the ordinary reason, and it is
            // asked again on the next start.
            tracing::warn!(%error, "the release feed could not be read");
            say(Word::Failed(error.to_string()));
            return;
        }
    };

    let version = info.TargetFullRelease.Version.clone();
    tracing::info!(%version, "downloading");
    say(Word::Downloading(version.clone()));

    if let Err(error) = manager.download_updates(&info, None) {
        tracing::warn!(%error, %version, "the release could not be downloaded");
        say(Word::Failed(error.to_string()));
        return;
    }

    tracing::info!(%version, "downloaded; it applies on the next start");
    say(Word::Ready(Box::new(Ready {
        version,
        manager,
        info: *info,
    })));
}
