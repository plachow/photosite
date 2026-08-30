//! Headless PhotoSite.
//!
//! Existuje ze dvou důvodů. Zaprvé se hodí sám o sobě — naskenovat složku,
//! podívat se, co je v souboru, zjistit, kde aplikace bydlí. Zadruhé, a to je
//! důležitější, **pustí celou pipeline bez okna a bez GPU**, takže se dá
//! testovat na všech třech platformách v CI, kde žádná obrazovka není.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use photosite_core::catalog::NewPhoto;
use photosite_core::{Catalog, Paths, diagnostics, domain, jobs};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "photosite-cli", version, about = "PhotoSite bez okna")]
struct Cli {
    /// Přesměruje data, nastavení i cache pod jeden kořen. Bez tohohle se
    /// sáhne tam, kam patří podle zvyklostí systému.
    #[arg(long, global = true, value_name = "SLOŽKA")]
    data: Option<PathBuf>,

    #[arg(long, short, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Naindexuje složku do katalogu.
    Scan {
        folder: PathBuf,
        #[arg(long, short)]
        recursive: bool,
    },
    /// Vypíše, co katalog o složce ví.
    List { folder: PathBuf },
    /// Přečte jeden soubor a vypíše, co z něj šlo dostat.
    Info { file: PathBuf },
    /// Kde co leží a na čem to běží. První otázka každé podpory.
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::resolve(cli.data.as_deref())?;
    paths.ensure()?;
    let _logging = diagnostics::start(&paths, cli.verbose);
    diagnostics::install_panic_hook(&paths);

    match cli.command {
        Command::Scan { folder, recursive } => scan(&paths, &folder, recursive),
        Command::List { folder } => list(&paths, &folder),
        Command::Info { file } => info(&file),
        Command::Doctor => {
            print!("{}", diagnostics::about(&paths));
            println!("schéma       {}", photosite_core::catalog::latest_version());
            println!("vláken       {}", jobs::worker_count());
            Ok(())
        }
    }
}

fn scan(paths: &Paths, folder: &Path, recursive: bool) -> Result<()> {
    anyhow::ensure!(folder.is_dir(), "{} není složka", folder.display());
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
        "nalezeno {} fotek za {:.0} ms",
        files.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );

    let started = std::time::Instant::now();
    let mut catalog = catalog;
    let (mut added, mut skipped, mut failed) = (0u64, 0u64, 0u64);

    // Po dávkách: jeden zápis do katalogu na tisíc souborů, ne na každý.
    for chunk in files.chunks(1000) {
        let identities: Vec<domain::FileIdentity> = chunk
            .iter()
            .filter_map(|path| match domain::FileIdentity::read(path) {
                Ok(identity) => Some(identity),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "soubor nelze přečíst");
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
                    // Selhat na jednom souboru je normální. Zamlčet to není.
                    failed += 1;
                    tracing::warn!(path = %identity.path.display(), error = %format!("{error:#}"), "soubor přeskočen");
                }
            }
        }

        added += batch.len() as u64;
        catalog.upsert_many(&batch)?;
    }

    let seconds = started.elapsed().as_secs_f64();
    println!(
        "zapsáno {added}, beze změny {skipped}, nečitelných {failed} za {seconds:.1} s \
         ({:.0} souborů/s)",
        files.len() as f64 / seconds.max(0.001)
    );
    println!("v katalogu celkem {}", catalog.count()?);
    Ok(())
}

/// Přečte z jednoho souboru to, co patří do katalogu. Čte se jen hlavička,
/// ne celá fotka — pixely tady nikoho nezajímají.
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
        taken_at: None,
        width: None,
        height: None,
        orientation: meta.orientation,
    })
}

fn list(paths: &Paths, folder: &Path) -> Result<()> {
    let catalog = Catalog::open(&paths.catalog())?;
    let photos = catalog.in_folder(folder)?;
    for photo in &photos {
        println!(
            "{:>8}  {:>10}  o{}  {}",
            photo.id.0,
            photo.file_size,
            photo.orientation,
            photo.path.display()
        );
    }

    println!("{} fotek", photos.len());
    Ok(())
}

fn info(file: &Path) -> Result<()> {
    anyhow::ensure!(file.is_file(), "{} není soubor", file.display());
    let identity = domain::FileIdentity::read(file)?;
    println!("cesta        {}", file.display());
    println!("velikost     {} B", identity.file_size);
    println!("změněn       {}", identity.modified_at);

    let raw = std::fs::read(file).context("soubor nelze přečíst")?;
    let meta = photosite_image::exif::read(&raw);
    println!("orientace    {}", meta.orientation);
    match meta.thumbnail {
        Some(thumbnail) => println!(
            "náhled EXIF  {} B na offsetu {}",
            thumbnail.len, thumbnail.offset
        ),
        None => println!("náhled EXIF  žádný"),
    }

    match photosite_image::quick(file)? {
        Some(image) => println!("rychlý náhled {}×{}", image.width, image.height),
        None => println!("rychlý náhled nelze vyrobit"),
    }

    let started = std::time::Instant::now();
    let full = photosite_image::sized(file, photosite_image::THUMB)?;
    println!(
        "dlaždice     {}×{} za {:.1} ms",
        full.width,
        full.height,
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}
