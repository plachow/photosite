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
use little_exif::rational::uR64;
use photosite_core::catalog::NewPhoto;
use photosite_core::domain::Organisation;
use photosite_core::place::Place;
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

    let mut meta = photosite_image::exif::read(&header);
    let said = read_from_header(path, &header);

    // The frame's size, when the header did not reach far enough to hold it.
    // See `frame_in_file`: a large embedded preview pushes the frame header
    // past the 128 KB we read, and a photograph with no dimensions drops out
    // of a sort by size and out of the shape filter without saying why.
    if meta.width.is_none()
        && let Some((width, height)) = photosite_image::exif::frame_in_file(path)
    {
        meta.width = Some(width);
        meta.height = Some(height);
    }

    // Where it was taken, and how much of that to believe. The verdict is
    // reached here, while the evidence is in hand: the error estimate, the
    // method and the age of the fix are in the file and are not kept, so
    // asking again later would mean opening the file again.
    let (place, verdict, reason) = match &meta.gps {
        Some(gps) => {
            let judgement = photosite_core::place::judge(photosite_core::place::Evidence {
                error_metres: gps.error_metres,
                method: gps.method.as_deref(),
                fixed_at: gps.fixed_at,
                taken_at: utc_of(&meta),
            });
            match photosite_core::Place::new(gps.latitude, gps.longitude) {
                Some(place) => (Some(place), judgement.verdict, judgement.because),
                None => (None, photosite_core::Verdict::Nowhere, None),
            }
        }
        None => (None, photosite_core::Verdict::Nowhere, None),
    };
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
            place,
            verdict,
            reason,
        },
        said,
    ))
}

/// When the shutter fired, in UTC, or nothing.
///
/// `DateTimeOriginal` is a wall clock and says nothing about which one, so it
/// becomes a moment in time only when the file also recorded what the camera's
/// clock was set to. Guessing instead read as a one-hour-stale fix on every
/// photograph taken in this country.
fn utc_of(meta: &photosite_image::exif::Exif) -> Option<i64> {
    Some(meta.taken_at? - i64::from(meta.offset_seconds?))
}

/// Makes the file say what the catalogue says.
///
/// Not a change to it but the whole of it: the caller holds one pending write
/// per photograph, so a retry writes the truth as it stands rather than a
/// change that may since have been undone.
///
/// `regions` is the one argument that means something by being absent.
/// `None` says the photograph has never been face-scanned here, and then
/// whatever face frames another program left in it are none of our business.
/// `Some` with an empty list says we looked and nobody on it is named, which
/// is a thing to write: it is how a face taken off somebody comes back out
/// of the file.
pub fn write(
    path: &Path,
    organisation: &Organisation,
    place: Option<Place>,
    regions: Option<xmp::Regions>,
) -> Result<Target> {
    let target = target_for(path);
    let mut wanted = Xmp::from(organisation);
    wanted.place = place;
    wanted.regions = regions;

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

/// Carries a photograph's metadata onto a converted copy of it.
///
/// What a batch conversion needs and nothing else does: the output is
/// freshly encoded pixels and knows nothing about where it came from, so
/// when it was taken, with what, at what exposure and everything anybody has
/// said about it are copied across.
///
/// Three tags are deliberately **not** copied, and every one of them would
/// be a bug that looks like a broken photograph:
///
/// * **Orientation.** The output was rendered the right way up — the decode
///   turned it — so an orientation tag from the source would turn it again.
///   A folder of exported photographs all lying on their side is exactly
///   what that looks like.
/// * **The pixel dimensions.** The output may have been resized, and a tag
///   saying 6000x4000 over a 2048-pixel file is a lie that some readers
///   believe.
/// * **The position**, when asked. That is the whole of `keep_place`, and it
///   is the reason somebody exports at all before putting a photograph of
///   their house on the internet.
///
/// What can be carried depends on the format. EXIF goes into JPEG, PNG, WebP
/// and TIFF; our own XMP is embedded only into JPEG, and **no sidecar is
/// written beside a conversion** — a folder of exported files with a `.xmp`
/// next to each one is not what anybody meant by "export".
pub fn carry(source: &Path, destination: &Path, keep_place: bool) -> Result<()> {
    let Some(kind) = little_exif_kind(destination) else {
        // BMP, and anything else that holds no metadata at all. Not a
        // failure: the pixels are what was asked for.
        tracing::debug!(
            path = %destination.display(),
            "this format holds no metadata; the conversion carries none"
        );
        return Ok(());
    };

    match Metadata::new_from_path(source) {
        Ok(mut metadata) => {
            // The ones that would be wrong on the output. See above.
            for (tag, group) in [
                (0x0112u16, ExifTagGroup::GENERIC),
                (0x0100, ExifTagGroup::GENERIC),
                (0x0101, ExifTagGroup::GENERIC),
                (0xA002, ExifTagGroup::EXIF),
                (0xA003, ExifTagGroup::EXIF),
                // A thumbnail of the original inside a resized copy is both
                // wrong and the largest thing in the block.
                (0x0201, ExifTagGroup::GENERIC),
                (0x0202, ExifTagGroup::GENERIC),
            ] {
                metadata.remove_tag_by_hex_group(tag, group);
            }

            if !keep_place {
                for tag in [
                    0x0001u16, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007, 0x001D, 0x001F,
                ] {
                    metadata.remove_tag_by_hex_group(tag, ExifTagGroup::GPS);
                }
            }

            if let Err(error) = metadata.write_to_file(destination) {
                // A photograph that came out right and lost its date is
                // worth saying so about, and is not worth failing over.
                tracing::warn!(
                    path = %destination.display(),
                    %error,
                    "the metadata could not be carried across"
                );
            }
        }
        Err(error) => tracing::debug!(
            path = %source.display(),
            %error,
            "the original holds no metadata to carry"
        ),
    }

    // And what somebody said about it, where the format can hold it. Read
    // from the source rather than passed in, so that this one function is
    // the whole of "carry the metadata".
    if kind == FileExtension::JPEG {
        let mut said = read(source);
        if !keep_place {
            said.place = None;
        }

        // The face frames belong to the original's own pixel size; a copy
        // has a different one, and a region list measured against the wrong
        // frame draws boxes over nothing.
        said.regions = None;
        if !said.is_empty() {
            let raw = std::fs::read(destination)
                .with_context(|| format!("cannot read {}", destination.display()))?;
            let packet = xmp::merge(jpeg::xmp(&raw).as_deref(), &said);
            match jpeg::with_xmp(&raw, &packet) {
                Ok(out) => replace(destination, &out)?,
                Err(error) => tracing::warn!(
                    path = %destination.display(),
                    error = %format!("{error:#}"),
                    "what was said about it could not be carried across"
                ),
            }
        }
    }

    Ok(())
}

/// What little_exif calls this file's kind, or nothing for a format that
/// holds no metadata.
fn little_exif_kind(path: &Path) -> Option<FileExtension> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => Some(FileExtension::JPEG),
        "png" => Some(FileExtension::PNG {
            as_zTXt_chunk: true,
        }),
        "webp" => Some(FileExtension::WEBP),
        "tif" | "tiff" => Some(FileExtension::TIFF),
        _ => None,
    }
}

/// Everything the file needs, worked out in memory before anything on disk is
/// touched.
fn embed(raw: &[u8], wanted: &Xmp) -> Result<Vec<u8>> {
    // The stars for Windows Explorer, which reads EXIF and not XMP.
    let mut out = with_rating(raw, wanted.rating, wanted.place)?;

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
fn with_rating(raw: &[u8], rating: Option<u8>, place: Option<Place>) -> Result<Vec<u8>> {
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

    // A position, when the catalogue holds one. **Never removed**: a photo
    // whose coordinates we happen not to know is not one whose coordinates
    // are wrong, and clearing what the camera recorded because the catalogue
    // has not caught up would be the worst kind of quiet damage.
    if let Some(place) = place {
        let (degrees, minutes, seconds) = sexagesimal(place.latitude);
        metadata.set_tag(ExifTag::GPSLatitude(vec![degrees, minutes, seconds]));
        metadata.set_tag(ExifTag::GPSLatitudeRef(
            if place.latitude >= 0.0 { "N" } else { "S" }.to_owned(),
        ));

        let (degrees, minutes, seconds) = sexagesimal(place.longitude);
        metadata.set_tag(ExifTag::GPSLongitude(vec![degrees, minutes, seconds]));
        metadata.set_tag(ExifTag::GPSLongitudeRef(
            if place.longitude >= 0.0 { "E" } else { "W" }.to_owned(),
        ));

        // **And the story of how it was arrived at, because it has changed.**
        // A position we write is one somebody typed, so the method is
        // MANUAL — and the camera's error estimate and the time of its fix
        // describe a fix that has just been replaced. Left behind, they
        // would have the photograph marked doubtful again the moment it was
        // read back, and correcting a position would visibly do nothing.
        metadata.set_tag(ExifTag::GPSProcessingMethod(b"ASCII\0\0\0MANUAL".to_vec()));
        metadata.remove_tag_by_hex_group(0x001F, ExifTagGroup::GPS);
        metadata.remove_tag_by_hex_group(0x0007, ExifTagGroup::GPS);
        metadata.remove_tag_by_hex_group(0x001D, ExifTagGroup::GPS);
    }

    metadata
        .write_to_vec(&mut buffer, FileExtension::JPEG)
        .context("the EXIF block could not be written")?;
    Ok(buffer)
}

/// Degrees, whole minutes and seconds, as the three rationals EXIF wants.
///
/// The sign is dropped: EXIF keeps the hemisphere as a letter in a tag of its
/// own, and a negative degree beside a `S` would be south twice over.
/// Seconds are kept to four decimal places, which is a hundredth of a
/// millimetre on the ground and rather more than anybody's GPS knows.
fn sexagesimal(value: f64) -> (uR64, uR64, uR64) {
    let value = value.abs();
    let degrees = value.trunc();
    let minutes = ((value - degrees) * 60.0).trunc();
    let seconds = (((value - degrees) * 60.0 - minutes) * 60.0 * 10_000.0).round();
    (
        uR64 {
            nominator: degrees as u32,
            denominator: 1,
        },
        uR64 {
            nominator: minutes as u32,
            denominator: 1,
        },
        uR64 {
            nominator: seconds as u32,
            denominator: 10_000,
        },
    )
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

        assert_eq!(
            write(&path, &organisation(), None, None).unwrap(),
            Target::Embedded
        );
        let read = read(&path);
        assert_eq!(read.rating, Some(4));
        assert_eq!(read.label.as_deref(), Some("Green"));
        assert_eq!(read.title.as_deref(), Some("Sunrise"));
        assert_eq!(read.keywords, ["Hawaii"]);
    }

    /// The face frames have to reach the file itself, not merely a packet
    /// in a test — and the photograph has to come out of it byte for byte
    /// the same, which is the promise this whole crate is built on.
    #[test]
    fn face_frames_go_into_the_photograph_without_touching_the_photograph() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        let before = jpeg_bytes();
        std::fs::write(&path, &before).unwrap();

        let regions = xmp::Regions {
            width: 6000,
            height: 4000,
            faces: vec![photosite_core::people::Region {
                name: "Jana".to_owned(),
                x: 0.2,
                y: 0.3,
                width: 0.1,
                height: 0.1,
            }],
        };
        write(&path, &organisation(), None, Some(regions)).unwrap();

        let after = std::fs::read(&path).unwrap();
        let packet = jpeg::xmp(&after).expect("no XMP packet in the file");
        assert!(
            packet.contains("<mwg-rs:Name>Jana</mwg-rs:Name>"),
            "{packet}"
        );
        assert_eq!(
            jpeg::compressed_image(&before),
            jpeg::compressed_image(&after),
            "the photograph itself changed"
        );
    }

    /// A RAW cannot be written into, so its faces go beside it — which is
    /// also the only place a RAW's stars have ever lived.
    #[test]
    fn a_raw_keeps_its_face_frames_in_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.nef");
        std::fs::write(&path, b"not really a raw").unwrap();

        write(
            &path,
            &organisation(),
            None,
            Some(xmp::Regions {
                width: 100,
                height: 100,
                faces: vec![photosite_core::people::Region {
                    name: "Jana".to_owned(),
                    x: 0.1,
                    y: 0.1,
                    width: 0.2,
                    height: 0.2,
                }],
            }),
        )
        .unwrap();

        let sidecar = std::fs::read_to_string(dir.path().join("a.xmp")).unwrap();
        assert!(sidecar.contains("Jana"), "{sidecar}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"not really a raw",
            "the file itself was touched"
        );
    }

    /// A corrected position has to survive the trip into the file and back
    /// out of it — the catalogue is overwritten by the next rescan, so the
    /// file is the only place the correction can live.
    #[test]
    fn a_corrected_position_goes_into_the_photograph_and_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        // Prague, and then the other side of the world, where both signs
        // are the other way round.
        for wanted in [
            Place::new(50.0755, 14.4378).unwrap(),
            Place::new(-33.8688, -70.6693).unwrap(),
        ] {
            write(&path, &organisation(), Some(wanted), None).unwrap();

            let raw = std::fs::read(&path).unwrap();
            let gps = photosite_image::exif::read(&raw)
                .gps
                .unwrap_or_else(|| panic!("{wanted:?} did not reach the EXIF block"));
            assert!(
                (gps.latitude - wanted.latitude).abs() < 1e-6,
                "{wanted:?} came back as {gps:?}"
            );
            assert!(
                (gps.longitude - wanted.longitude).abs() < 1e-6,
                "{wanted:?} came back as {gps:?}"
            );

            // And in the XMP packet as well, which is what Lightroom reads.
            let read = read(&path);
            let from_xmp = read.place.expect("the packet holds no position");
            assert!(
                (from_xmp.latitude - wanted.latitude).abs() < 1e-4,
                "{read:?}"
            );
            assert!(
                (from_xmp.longitude - wanted.longitude).abs() < 1e-4,
                "{read:?}"
            );
        }
    }

    /// The one a real photograph taught us. A correction that leaves the
    /// cell-tower fix behind is a correction that undoes itself: the mark
    /// comes straight back the next time the file is read.
    #[test]
    fn correcting_a_position_replaces_the_story_of_how_it_was_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        write(
            &path,
            &organisation(),
            Some(Place::new(50.0755, 14.4378).unwrap()),
            None,
        )
        .unwrap();

        let raw = std::fs::read(&path).unwrap();
        let gps = photosite_image::exif::read(&raw).gps.expect("no position");
        assert_eq!(gps.method.as_deref(), Some("MANUAL"));
        assert_eq!(gps.error_metres, None, "the old error estimate stayed");
        assert_eq!(gps.fixed_at, None, "the old fix time stayed");

        // Which is what makes the mark clear itself.
        let judgement = photosite_core::place::judge(photosite_core::place::Evidence {
            error_metres: gps.error_metres,
            method: gps.method.as_deref(),
            fixed_at: gps.fixed_at,
            taken_at: None,
        });
        assert_eq!(judgement.verdict, photosite_core::Verdict::Precise);
    }

    /// The one that would be quiet and irreversible. A photograph whose
    /// position we happen not to hold is not one whose position is wrong.
    #[test]
    fn writing_without_a_position_leaves_the_one_that_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        let wanted = Place::new(50.0755, 14.4378).unwrap();
        write(&path, &organisation(), Some(wanted), None).unwrap();
        write(&path, &organisation(), None, None).unwrap();

        let raw = std::fs::read(&path).unwrap();
        let gps = photosite_image::exif::read(&raw)
            .gps
            .expect("the position was wiped by a write that knew nothing about it");
        assert!((gps.latitude - wanted.latitude).abs() < 1e-6, "{gps:?}");
    }

    #[test]
    fn the_photograph_is_the_same_photograph_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        let before = jpeg_bytes();
        std::fs::write(&path, &before).unwrap();

        write(&path, &organisation(), None, None).unwrap();
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

        let target = write(&path, &organisation(), None, None).unwrap();
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

        write(&path, &organisation(), None, None).unwrap();
        let once = std::fs::read(&path).unwrap();
        write(&path, &organisation(), None, None).unwrap();
        let twice = std::fs::read(&path).unwrap();
        assert_eq!(once, twice, "the second write changed the file");
    }

    #[test]
    fn clearing_everything_leaves_the_file_saying_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, jpeg_bytes()).unwrap();

        write(&path, &organisation(), None, None).unwrap();
        write(&path, &Organisation::default(), None, None).unwrap();
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

        assert!(write(&path, &organisation(), None, None).is_err());
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
        let _ = write(&path, &organisation(), None, None);

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
