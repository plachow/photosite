//! The run log and crash reports.
//!
//! A graphical application that crashes simply vanishes — no window, no
//! message, no trace. And an error somebody swallowed is worse still: the
//! application keeps running, looks like all is well, and does nothing. That
//! is exactly what happened to the prototype, where one discarded exception
//! meant not a single thumbnail was drawn and nowhere was there a word about
//! it.
//!
//! Hence: everything goes to the file, a crash leaves a report behind, and
//! nothing is ever lost in silence.

use crate::paths::Paths;
use std::io::Write;
use std::path::PathBuf;

/// A handle that has to stay alive for the whole run — otherwise the file
/// writer closes and the log ends.
#[derive(Debug)]
pub struct Logging {
    _guard: tracing_appender::non_blocking::WorkerGuard,
    pub file: PathBuf,
}

/// The targets that belong to us. The filter matches them by *crate* name,
/// and for a binary that is not the package name but the target name: the
/// application in `photosite-ui` logs under `photosite`. Until that stood
/// here, not one line from the application itself reached the log — only
/// lines from the core — and there was no way to tell, because warnings and
/// errors fell through the general level at the end.
const OURS: &[&str] = &[
    "photosite",
    "photosite_ui",
    "photosite_cli",
    "photosite_core",
    "photosite_image",
];

/// Default levels when `RUST_LOG` says nothing: our crates in detail,
/// foreign ones from warnings up, so the graphics stack does not flood the
/// log.
pub fn default_filter(verbose: bool) -> String {
    let level = if verbose { "debug" } else { "info" };
    OURS.iter()
        .map(|name| format!("{name}={level}"))
        .chain(std::iter::once("warn".to_owned()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Starts logging, both to the file and to standard error.
///
/// The level can be overridden with `RUST_LOG`; without it, `info` for us
/// and `warn` for foreign crates, so the graphics stack does not flood the
/// log.
pub fn start(paths: &Paths, verbose: bool) -> Logging {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let _ = std::fs::create_dir_all(&paths.logs);
    let appender = tracing_appender::rolling::daily(&paths.logs, "photosite.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter(verbose)));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    Logging {
        _guard: guard,
        file: paths.logs.join("photosite.log"),
    }
}

/// Installs a panic hook that writes a report next to the log.
///
/// Without this, a crash in a graphical application is invisible: the window
/// disappears and nobody ever learns why.
pub fn install_panic_hook(paths: &Paths) {
    let logs = paths.logs.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = logs.join("panic.txt");
        let _ = std::fs::create_dir_all(&logs);
        if let Ok(mut file) = std::fs::File::create(&report) {
            let _ = writeln!(file, "PhotoSite {}", env!("CARGO_PKG_VERSION"));
            let _ = writeln!(file, "{} {}", std::env::consts::OS, std::env::consts::ARCH);
            let _ = writeln!(file, "\n{info}\n");
            let _ = writeln!(file, "{}", std::backtrace::Backtrace::force_capture());
        }

        tracing::error!(report = %report.display(), "crashed");
        eprintln!("PhotoSite crashed. Report: {}", report.display());
        previous(info);
    }));
}

/// What to print when somebody asks what is going on. It is the first
/// question any support conversation opens with, so the answer should be one
/// call away.
///
/// Returns label-value pairs, so the CLI can print them and the UI can lay
/// them out.
pub fn about(paths: &Paths) -> Vec<(String, String)> {
    use crate::i18n::t;
    vec![
        (
            t("diagnostics-version"),
            env!("CARGO_PKG_VERSION").to_owned(),
        ),
        (
            t("diagnostics-platform"),
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        ),
        (
            t("diagnostics-mode"),
            t(if paths.portable {
                "diagnostics-mode-portable"
            } else {
                "diagnostics-mode-system"
            }),
        ),
        (
            t("diagnostics-language"),
            crate::i18n::language().to_string(),
        ),
        (t("diagnostics-data"), paths.data.display().to_string()),
        (
            t("diagnostics-config"),
            paths.config_file().display().to_string(),
        ),
        (t("diagnostics-cache"), paths.cache.display().to_string()),
        (t("diagnostics-logs"), paths.logs.display().to_string()),
        (
            t("diagnostics-catalog"),
            paths.catalog().display().to_string(),
        ),
    ]
}
