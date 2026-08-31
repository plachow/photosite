//! Headless PhotoSite.
//!
//! It exists for two reasons. First, it is useful in its own right — scan a
//! folder, look at what is in a file, find out where the application lives.
//! Second, and this matters more, it **runs the whole pipeline with no window
//! and no GPU**, so it can be tested on all three platforms in CI, where
//! there is no screen at all.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use photosite_core::catalog::NewPhoto;
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
    let catalog = Catalog::open(&paths.catalog())?;

    let started = std::time::Instant::now();
    let files: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| domain::is_photo(path))
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
    let mut catalog = catalog;
    let (mut added, mut skipped, mut failed) = (0u64, 0u64, 0u64);

    // In batches: one write to the catalogue per thousand files, not per
    // file.
    for chunk in files.chunks(1000) {
        let identities: Vec<domain::FileIdentity> = chunk
            .iter()
            .filter_map(|path| match domain::FileIdentity::read(path) {
                Ok(identity) => Some(identity),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "the file cannot be read");
                    None
                }
            })
            .collect();
        failed += (chunk.len() - identities.len()) as u64;

        let known = catalog.unchanged(&identities)?;
        let mut batch = Vec::new();
        for identity in identities {
            if known.contains(&identity.path) {
                skipped += 1;
                continue;
            }

            match read_one(&identity) {
                Ok(photo) => batch.push(photo),
                Err(error) => {
                    // Failing on one file is normal. Saying nothing is not.
                    failed += 1;
                    tracing::warn!(path = %identity.path.display(), error = %format!("{error:#}"), "file skipped");
                }
            }
        }

        added += batch.len() as u64;
        catalog.upsert_many(&batch)?;
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
    println!("{}", t!("cli-catalog-total", count = catalog.count()?));
    Ok(())
}

/// Reads out of one file what belongs in the catalogue. Only the header is
/// read, not the whole photograph — nobody here cares about pixels.
fn read_one(identity: &domain::FileIdentity) -> Result<NewPhoto> {
    use std::io::Read as _;
    let mut head = vec![0u8; photosite_image::exif::HEADER_BYTES];
    let mut file = std::fs::File::open(&identity.path)?;
    let read = file.read(&mut head)?;
    head.truncate(read);

    let meta = photosite_image::exif::read(&head);
    Ok(NewPhoto {
        path: identity.path.clone(),
        file_size: identity.file_size,
        modified_at: identity.modified_at,
        taken_at: meta.taken_at,
        width: meta.width,
        height: meta.height,
        orientation: meta.orientation,
    })
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
