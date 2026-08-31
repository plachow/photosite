//! Headless PhotoSite.
//!
//! It exists for two reasons. First, it is useful in its own right — scan a
//! folder, look at what is in a file, find out where the application lives.
//! Second, and this matters more, it **runs the whole pipeline with no window
//! and no GPU**, so it can be tested on all three platforms in CI, where
//! there is no screen at all.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use photosite_core::{Catalog, Paths, diagnostics, domain, i18n, jobs, t};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "photosite-cli", version, about = "PhotoSite with no window")]
struct Cli {
    /// Redirects data, settings and cache under a single root. Without it,
    /// each goes where the system's conventions put it.
    #[arg(long, global = true, value_name = "FOLDER")]
    data: Option<PathBuf>,

    #[arg(long, short, global = true)]
    verbose: bool,

    /// The language of the output; `en-US` without it.
    #[arg(long, global = true, value_name = "LANGUAGE")]
    lang: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Indexes a folder into the catalogue.
    Scan {
        folder: PathBuf,
        #[arg(long, short)]
        recursive: bool,
    },
    /// Prints what the catalogue knows about a folder.
    List {
        folder: PathBuf,
        #[arg(long, short)]
        recursive: bool,
    },
    /// Reads one file and prints what could be got out of it.
    Info { file: PathBuf },
    /// Where everything lives and what it runs on. The first question any
    /// support conversation opens with.
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::resolve(cli.data.as_deref())?;
    paths.ensure()?;
    let _logging = diagnostics::start(&paths, cli.verbose);
    diagnostics::install_panic_hook(&paths);
    i18n::set_language(&i18n::negotiate(cli.lang.as_deref()));

    match cli.command {
        Command::Scan { folder, recursive } => scan(&paths, &folder, recursive),
        Command::List { folder, recursive } => list(&paths, &folder, recursive),
        Command::Info { file } => info(&file),
        Command::Doctor => {
            let mut rows = diagnostics::about(&paths);
            rows.push((
                t!("diagnostics-schema"),
                photosite_core::catalog::latest_version().to_string(),
            ));
            rows.push((t!("diagnostics-workers"), jobs::worker_count().to_string()));
            let width = rows
                .iter()
                .map(|(label, _)| label.chars().count())
                .max()
                .unwrap_or(0);
            for (label, value) in rows {
                println!("{label:width$}  {value}");
            }

            Ok(())
        }
    }
}

fn scan(paths: &Paths, folder: &Path, recursive: bool) -> Result<()> {
    anyhow::ensure!(
        folder.is_dir(),
        "{}",
        t!("error-not-a-folder", path = folder.display().to_string())
    );
    let mut catalog = Catalog::open(&paths.catalog())?;

    // The same two steps the application takes, in the same order and
    // through the same catalogue methods: a row for every file first, then
    // the headers of whatever has not given one up yet.
    //
    // It used to decide what to skip by length and write time alone, which
    // is a different question from "has this file been read". They agreed
    // until a migration added a column, and then the window went back over
    // the library while this did not.
    let started = std::time::Instant::now();
    let files: Vec<domain::FileIdentity> = walkdir::WalkDir::new(folder)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| domain::is_photo(entry.path()))
        .filter_map(|entry| match domain::FileIdentity::read(entry.path()) {
            Ok(identity) => Some(identity),
            Err(error) => {
                tracing::warn!(path = %entry.path().display(), %error, "the file cannot be read");
                None
            }
        })
        .collect();
    println!(
        "{}",
        t!(
            "cli-scan-found",
            count = files.len() as i64,
            ms = started.elapsed().as_secs_f64() * 1000.0
        )
    );

    let started = std::time::Instant::now();
    catalog.upsert_identities(&files)?;
    let waiting = catalog.unindexed(folder, recursive)?;
    let skipped = files.len().saturating_sub(waiting.len()) as u64;
    let (mut added, mut failed, mut seeded) = (0u64, 0u64, 0u64);

    // In batches: one write to the catalogue per thousand files, not per
    // file.
    for chunk in waiting.chunks(1000) {
        let mut batch = Vec::with_capacity(chunk.len());
        let mut said = Vec::with_capacity(chunk.len());
        for path in chunk {
            match photosite_meta::scan(path) {
                Some((photo, what)) => {
                    said.push((photo.path.clone(), what));
                    batch.push(photo);
                }
                None => {
                    // Failing on one file is normal. Saying nothing is not.
                    failed += 1;
                    tracing::warn!(path = %path.display(), "file skipped");
                }
            }
        }

        added += batch.len() as u64;
        catalog.upsert_many(&batch)?;

        // What the files already say fills in what the catalogue does not.
        for (path, what) in said {
            if what.is_empty() {
                continue;
            }

            if let Some(id) = catalog.id_of(&path)?
                && catalog.seed(id, &what.as_organisation())?
            {
                seeded += 1;
            }
        }
    }

    let seconds = started.elapsed().as_secs_f64();
    println!(
        "{}",
        t!(
            "cli-scan-done",
            added = added as i64,
            skipped = skipped as i64,
            failed = failed as i64,
            seconds = seconds,
            rate = files.len() as f64 / seconds.max(0.001)
        )
    );
    if seeded > 0 {
        println!("{}", t!("cli-scan-seeded", count = seeded as i64));
    }

    println!("{}", t!("cli-catalog-total", count = catalog.count()?));
    Ok(())
}

fn list(paths: &Paths, folder: &Path, recursive: bool) -> Result<()> {
    let catalog = Catalog::open(&paths.catalog())?;
    let photos = catalog.in_folder(folder, recursive)?;
    for photo in &photos {
        println!(
            "{:>8}  {:>10}  o{}  {}",
            photo.id.0,
            photo.file_size,
            photo.orientation,
            photo.path.display()
        );
    }

    println!("{}", t!("cli-list-total", count = photos.len() as i64));
    Ok(())
}

fn info(file: &Path) -> Result<()> {
    anyhow::ensure!(
        file.is_file(),
        "{}",
        t!("error-not-a-file", path = file.display().to_string())
    );
    let identity = domain::FileIdentity::read(file)?;
    let mut rows = vec![
        (t!("cli-info-path"), file.display().to_string()),
        (t!("cli-info-size"), format!("{} B", identity.file_size)),
        (t!("cli-info-modified"), identity.modified_at.to_string()),
    ];

    let raw = std::fs::read(file).context("the file cannot be read")?;
    let meta = photosite_image::exif::read(&raw);
    rows.push((t!("cli-info-orientation"), meta.orientation.to_string()));
    rows.push((
        t!("cli-info-taken"),
        match meta.taken_at {
            Some(seconds) => seconds.to_string(),
            None => t!("cli-info-taken-none"),
        },
    ));
    rows.push((
        t!("cli-info-camera"),
        meta.camera.clone().unwrap_or_else(|| t!("cli-info-none")),
    ));
    rows.push((
        t!("cli-info-lens"),
        meta.lens.clone().unwrap_or_else(|| t!("cli-info-none")),
    ));
    rows.push((
        t!("cli-info-place"),
        match &meta.gps {
            Some(gps) => {
                let judgement = photosite_core::place::judge(photosite_core::place::Evidence {
                    error_metres: gps.error_metres,
                    method: gps.method.as_deref(),
                    fixed_at: gps.fixed_at,
                    taken_at: meta
                        .taken_at
                        .zip(meta.offset_seconds)
                        .map(|(taken, offset)| taken - i64::from(offset)),
                });
                match photosite_core::Place::new(gps.latitude, gps.longitude) {
                    Some(place) => format!(
                        "{place}  [{}]{}",
                        photosite_core::i18n::t(judgement.verdict.title_key()),
                        judgement
                            .because
                            .map(|reason| format!(
                                "  {}",
                                photosite_core::i18n::t(reason.title_key())
                            ))
                            .unwrap_or_default()
                    ),
                    None => t!("cli-info-none"),
                }
            }
            None => t!("cli-info-none"),
        },
    ));
    rows.push((
        t!("cli-info-frame"),
        match (meta.width, meta.height) {
            (Some(width), Some(height)) => format!("{width}x{height}"),
            _ => t!("cli-info-frame-none"),
        },
    ));
    rows.push((
        t!("cli-info-embedded"),
        match meta.thumbnail {
            Some(thumbnail) => t!(
                "cli-info-embedded-at",
                bytes = thumbnail.len as i64,
                offset = thumbnail.offset as i64
            ),
            None => t!("cli-info-embedded-none"),
        },
    ));
    rows.push((
        t!("cli-info-quick"),
        match photosite_image::quick(file)? {
            Some(image) => format!("{}x{}", image.width, image.height),
            None => t!("cli-info-quick-none"),
        },
    ));

    let started = std::time::Instant::now();
    let full = photosite_image::sized(file, photosite_image::THUMB)?;
    rows.push((
        t!("cli-info-tile"),
        t!(
            "cli-info-size-px",
            width = full.width as i64,
            height = full.height as i64,
            ms = started.elapsed().as_secs_f64() * 1000.0
        ),
    ));

    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    for (label, value) in rows {
        println!("{label:width$}  {value}");
    }

    Ok(())
}
