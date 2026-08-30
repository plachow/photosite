//! Záznam běhu a hlášení o pádech.
//!
//! Grafická aplikace, která spadne, prostě zmizí — bez okna, bez hlášky, bez
//! stopy. A chyba, kterou někdo spolkl, je ještě horší: aplikace běží dál a
//! tváří se, že je všechno v pořádku, jen nic nedělá. Přesně tohle se stalo
//! prototypu, kde jedna zahozená výjimka způsobila, že se nevykreslil jediný
//! náhled a nikde o tom nebylo ani slovo.
//!
//! Proto: všechno jde do souboru, pád nechá po sobě hlášení, a nic se nikdy
//! neztratí mlčky.

use crate::paths::Paths;
use std::io::Write;
use std::path::PathBuf;

/// Držák, který musí zůstat naživu po celou dobu běhu — jinak se zápis do
/// souboru zavře a log skončí.
#[derive(Debug)]
pub struct Logging {
    _guard: tracing_appender::non_blocking::WorkerGuard,
    pub file: PathBuf,
}

/// Zapne záznam do souboru i na standardní chybový výstup.
///
/// Úroveň se dá přebít proměnnou `RUST_LOG`; bez ní je to `info` pro nás a
/// `warn` pro cizí crate, aby log nezaplavila grafika.
pub fn start(paths: &Paths, verbose: bool) -> Logging {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let _ = std::fs::create_dir_all(&paths.logs);
    let appender = tracing_appender::rolling::daily(&paths.logs, "photosite.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let default = if verbose {
        "photosite_core=debug,photosite_image=debug,photosite_ui=debug,photosite_cli=debug,warn"
    } else {
        "photosite_core=info,photosite_image=info,photosite_ui=info,photosite_cli=info,warn"
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));

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

/// Nainstaluje zachytávač pádů, který napíše hlášení vedle logu.
///
/// Bez tohohle je pád v grafické aplikaci neviditelný: okno zmizí a nikdo se
/// nikdy nedozví proč.
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

        tracing::error!(report = %report.display(), "pád");
        eprintln!("PhotoSite spadl. Hlášení: {}", report.display());
        previous(info);
    }));
}

/// Co vypsat, když se někdo ptá „co se u tebe děje". První otázka každé
/// podpory, tak ať je odpověď na jedno zavolání.
///
/// Vrací dvojice popisek–hodnota, aby si je CLI mohlo vypsat a UI vysázet.
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
