//! Writing what somebody said into the photograph, and reading back what one
//! already says.
//!
//! This crate exists so that PhotoSite needs no exiftool. What PhotoSite
//! writes is five properties in two namespaces plus two numbers for Windows;
//! exiftool's worth is its breadth, and its cost is a 35 MB binary with its
//! own copy of Perl, once per platform.
//!
//! Three rules hold everywhere in here, and every one of them is a thing that
//! goes wrong quietly:
//!
//! 1. **Read before writing.** Handing a metadata writer a fresh, empty set
//!    means "this is the whole of the file's metadata", and writing it throws
//!    away everything already there. On a real photograph that turned 5,484
//!    bytes of EXIF into 48: capture date, camera, exposure and orientation
//!    all gone in one call.
//! 2. **Merge, never replace.** A packet holds other people's properties too.
//!    Ours are taken out of wherever they were and written back in one block;
//!    the rest is copied through.
//! 3. **Prove the photograph is untouched.** The compressed image is compared
//!    before and after, in memory, and the write is refused if it differs.
//!
//! Only JPEG is written into. Everything else — RAW above all — gets a `.xmp`
//! beside it, because a sidecar cannot damage a photograph and we do not open
//! a format we cannot prove we preserve.

pub mod jpeg;
pub mod xmp;

use anyhow::{Context, Result};
use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::ifd::ExifTagGroup;
use little_exif::metadata::Metadata;
use photosite_core::catalog::NewPhoto;
use photosite_core::domain::Organisation;
use std::path::{Path, PathBuf};
use xmp::Xmp;

/// Where a photograph's metadata is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Inside the photograph.
    Embedded,
    /// In a `.xmp` beside it, the photograph left alone entirely.
    Sidecar(PathBuf),
}

/// Which extensions we are prepared to write into.
///
/// Deliberately short. A format we cannot walk segment by segment is a format
/// we cannot promise to preserve, and a sidecar always works.
const EMBEDDABLE: &[&str] = &["jpg", "jpeg"];

pub fn target_for(path: &Path) -> Target {
    let embeddable = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| EMBEDDABLE.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false);

    if embeddable {
        Target::Embedded
    } else {
        Target::Sidecar(sidecar_of(path))
    }
}

/// `photo.nef` keeps its sidecar at `photo.xmp`, which is where every
/// cataloguer looks for it.
pub fn sidecar_of(path: &Path) -> PathBuf {
    // The rule itself lives in the core, with the rest of the vocabulary:
    // file operations have to honour it too, and two copies of it would part
    // company the first time either was touched.
    photosite_core::sidecar_of(path)
}

/// What a photograph already says about itself.
///
/// The embedded packet first, then a sidecar beside it. Nothing at all comes
/// back empty rather than as an error: most photographs have never been
/// spoken about, and that is not a failure.
pub fn read(path: &Path) -> Xmp {
    if let Target::Sidecar(sidecar) = target_for(path) {
        return std::fs::read_to_string(&sidecar)
            .ok()
            .map(|packet| xmp::read(&packet))
            .unwrap_or_default();
    }

    let Ok(raw) = std::fs::read(path) else {
        return Xmp::default();
    };

    if let Some(packet) = jpeg::xmp(&raw) {
        return xmp::read(&packet);
    }

    // A JPEG can still have a sidecar, written by something else.
    std::fs::read_to_string(sidecar_of(path))
        .ok()
        .map(|packet| xmp::read(&packet))
        .unwrap_or_default()
}

/// The same, from a header already in hand.
///
/// The scan reads the first stretch of every photograph to learn about it,
/// and the packet is in there. Asking for the whole of every file to find it
/// again would be most of a terabyte of reading across a large library.
pub fn read_from_header(path: &Path, header: &[u8]) -> Xmp {
    match target_for(path) {
        Target::Embedded => match jpeg::xmp(header) {
            Some(packet) => xmp::read(&packet),
            None => Xmp::default(),
        },
        Target::Sidecar(sidecar) => std::fs::read_to_string(&sidecar)
            .ok()
            .map(|packet| xmp::read(&packet))
            .unwrap_or_default(),
    }
}

/// Everything one read of a photograph's header can tell us: the row for the
/// catalogue, and whatever the photograph already says about itself.
///
/// One read serves both. The EXIF block and the XMP packet both sit within
/// the first stretch of the file, and reading the whole of every photograph
/// to find them would be most of a terabyte across a large library.
///
/// This lives here, in one place, because the window and the headless
/// command must do the same thing. They once decided differently what to
/// skip — one by length and write time, the other by whether the file had
/// been read — and agreed only until a migration added a column.
pub fn scan(path: &Path) -> Option<(NewPhoto, Xmp)> {
    use std::io::Read as _;

    let identity = photosite_core::FileIdentity::read(path).ok()?;
    let mut header = Vec::with_capacity(photosite_image::exif::HEADER_BYTES);
    match std::fs::File::open(path) {
        Ok(file) => {
            let _ = file
                .take(photosite_image::exif::HEADER_BYTES as u64)
                .read_to_end(&mut header);
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "the file cannot be opened");
            return None;
        }
    }

    let meta = photosite_image::exif::read(&header);
    let said = read_from_header(path, &header);
    Some((
        NewPhoto {
            path: identity.path,
            file_size: identity.file_size,
            modified_at: identity.modified_at,
            taken_at: meta.taken_at,
            width: meta.width,
            height: meta.height,
            orientation: meta.orientation,
            camera: meta.camera,
            lens: meta.lens,
        },
        said,
    ))
}

/// Makes the file say what the catalogue says.
///
/// Not a change to it but the whole of it: the caller holds one pending write
/// per photograph, so a retry writes the truth as it stands rather than a
/// change that may since have been undone.
pub fn write(path: &Path, organisation: &Organisation) -> Result<Target> {
    let target = target_for(path);
    let wanted = Xmp::from(organisation);

    match &target {
        Target::Sidecar(sidecar) => {
            let existing = std::fs::read_to_string(sidecar).ok();
            let packet = xmp::merge(existing.as_deref(), &wanted);
            replace(sidecar, packet.as_bytes())
                .with_context(|| format!("cannot write {}", sidecar.display()))?;
        }
        Target::Embedded => {
            let raw =
                std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
            let out = embed(&raw, &wanted)
                .with_context(|| format!("cannot write into {}", path.display()))?;
            replace(path, &out).with_context(|| format!("cannot write {}", path.display()))?;
        }
    }

    Ok(target)
}

/// Everything the file needs, worked out in memory before anything on disk is
/// touched.
fn embed(raw: &[u8], wanted: &Xmp) -> Result<Vec<u8>> {
    // The stars for Windows Explorer, which reads EXIF and not XMP.
    let mut out = with_rating(raw, wanted.rating)?;

    let existing = jpeg::xmp(&out);
    let packet = xmp::merge(existing.as_deref(), wanted);
    out = jpeg::with_xmp(&out, &packet)?;

    // The one that matters. Everything above rearranges segments; if any of
    // it moved a byte of the photograph, the file does not get written.
    let before = jpeg::compressed_image(raw).context("this is not a JPEG we can read")?;
    let after = jpeg::compressed_image(&out).context("what we built is not a readable JPEG")?;
    anyhow::ensure!(
        before == after,
        "writing the metadata would have changed the photograph itself"
    );

    Ok(out)
}

/// EXIF `Rating` and `RatingPercent` in IFD0 — 0x4746 and 0x4749.
///
/// Neither is in little_exif's list of known tags, so they go in by number.
/// **The file is read first**, without exception: the alternative writes an
/// empty metadata set over everything the camera recorded.
fn with_rating(raw: &[u8], rating: Option<u8>) -> Result<Vec<u8>> {
    let mut buffer = raw.to_vec();
    let mut metadata = match Metadata::new_from_vec(&buffer, FileExtension::JPEG) {
        Ok(metadata) => metadata,
        Err(error) => {
            // No EXIF at all is ordinary; start one rather than give up on
            // the stars.
            tracing::debug!(%error, "no EXIF to read; starting a new block");
            Metadata::new()
        }
    };

    match rating {
        Some(stars) => {
            metadata.set_tag(ExifTag::UnknownINT16U(
                vec![stars as u16],
                0x4746,
                ExifTagGroup::GENERIC,
            ));
            metadata.set_tag(ExifTag::UnknownINT16U(
                vec![percent_of(stars)],
                0x4749,
                ExifTagGroup::GENERIC,
            ));
        }
        None => {
            metadata.remove_tag_by_hex_group(0x4746, ExifTagGroup::GENERIC);
            metadata.remove_tag_by_hex_group(0x4749, ExifTagGroup::GENERIC);
        }
    }

    metadata
        .write_to_vec(&mut buffer, FileExtension::JPEG)
        .context("the EXIF block could not be written")?;
    Ok(buffer)
}

/// What Windows puts in `RatingPercent` for each number of stars.
///
/// Not a straight twenty percent apiece: these are the values Explorer itself
/// writes, and writing different ones makes it round to a different number of
/// stars than the one beside it.
fn percent_of(stars: u8) -> u16 {
    match stars {
        1 => 1,
        2 => 25,
        3 => 50,
        4 => 75,
        _ => 99,
    }
}

/// Writes beside the file and moves it into place.
///
/// A photograph half-written is worse than one not written at all, and a
/// crash in the middle of a six megabyte write is not a rare event across a
/// library. The temporary sits in the same folder so the move is a rename and
/// not a copy.
fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!(
        "{}.photosite-part",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("tmp")
    ));

    std::fs::write(&temporary, bytes)
        .with_context(|| format!("cannot write {}", temporary.display()))?;

    if let Err(error) = std::fs::rename(&temporary, path) {
        // Leaving a stray part file behind would be a second failure on top
        // of the first.
        let _ = std::fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("cannot move into {}", path.display()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use photosite_core::domain::ColorLabel;

    fn organisation() -> Organisation {
        Organisation {
            rating: 4,
            label: ColorLabel::Green,
            flag: Default::default(),
            title: Some("Sunrise".to_owned()),
            description: None,
            keywords: vec!["Hawaii".to_owned()],
        }
    }

    /// A JPEG small enough to build here and real enough to write into.
    fn jpeg_bytes() -> Vec<u8> {
        let mut raw = vec![0xFF, 0xD8];
        let app = |marker: u8, body: &[u8], raw: &mut Vec<u8>| {
            raw.push(0xFF);
            raw.push(marker);
            raw.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            raw.extend_from_slice(body);
        };
        app(0xE0, b"JFIF\0", &mut raw);
        app(0xDB, &[0u8; 64], &mut raw);
        app(0xC0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0], &mut raw);
        raw.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 1, 1, 0, 0, 63, 0]);
        raw.extend_from_slice(b"THE PHOTOGRAPH ITSELF");
        raw.extend_from_slice(&[0xFF, 0xD9]);
        raw
    }

    #[test]
    fn a_jpeg_is_written_into_and_anything_else_gets_a_sidecar() {
        assert_eq!(target_for(Path::new("a/b.JPG")), Target::Embedded);
        assert_eq!(target_for(Path::new("a/b.jpeg")), Target::Embedded);
        assert_eq!(
            target_for(Path::new("a/b.nef")),
            Target::Sidecar(PathBuf::from("a/b.xmp"))
        );
        assert_eq!(
            target_for(Path::new("a/b.png")),
            Target::Sidecar(PathBuf::from("a/b.xmp"))
        );
    }

    #[test]
    fn what_goes_into_a_jpeg_comes_back_out_of_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        assert_eq!(write(&path, &organisation()).unwrap(), Target::Embedded);
        let read = read(&path);
        assert_eq!(read.rating, Some(4));
        assert_eq!(read.label.as_deref(), Some("Green"));
        assert_eq!(read.title.as_deref(), Some("Sunrise"));
        assert_eq!(read.keywords, ["Hawaii"]);
    }

    #[test]
    fn the_photograph_is_the_same_photograph_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        let before = jpeg_bytes();
        std::fs::write(&path, &before).unwrap();

        write(&path, &organisation()).unwrap();
        let after = std::fs::read(&path).unwrap();
        assert_eq!(
            jpeg::compressed_image(&before),
            jpeg::compressed_image(&after)
        );
    }

    #[test]
    fn a_raw_file_is_not_opened_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.nef");
        std::fs::write(&path, b"pretend this is a raw file").unwrap();

        let target = write(&path, &organisation()).unwrap();
        assert_eq!(target, Target::Sidecar(dir.path().join("a.xmp")));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"pretend this is a raw file",
            "the photograph was touched"
        );
        assert_eq!(read(&path).rating, Some(4));
    }

    #[test]
    fn writing_twice_settles_rather_than_growing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        write(&path, &organisation()).unwrap();
        let once = std::fs::read(&path).unwrap();
        write(&path, &organisation()).unwrap();
        let twice = std::fs::read(&path).unwrap();
        assert_eq!(once, twice, "the second write changed the file");
    }

    #[test]
    fn clearing_everything_leaves_the_file_saying_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        write(&path, &organisation()).unwrap();
        write(&path, &Organisation::default()).unwrap();
        assert!(read(&path).is_empty(), "{:?}", read(&path));
    }

    #[test]
    fn a_photograph_nobody_has_spoken_about_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();
        assert!(read(&path).is_empty());
        assert!(read(&dir.path().join("missing.jpg")).is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_jpeg_despite_its_name_is_refused_not_mangled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, b"this is not a jpeg at all").unwrap();

        assert!(write(&path, &organisation()).is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"this is not a jpeg at all",
            "a file we could not write was changed anyway"
        );
    }

    #[test]
    fn a_failed_write_leaves_no_part_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, b"not a jpeg").unwrap();
        let _ = write(&path, &organisation());

        let strays: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("photosite-part"))
            .collect();
        assert!(strays.is_empty(), "{strays:?}");
    }

    #[test]
    fn the_stars_windows_reads_are_the_ones_windows_writes() {
        assert_eq!(percent_of(1), 1);
        assert_eq!(percent_of(3), 50);
        assert_eq!(percent_of(5), 99);
    }
}
