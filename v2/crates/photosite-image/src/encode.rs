//! Writing pixels back out to a file.
//!
//! The other direction from [`crate::decode`], and the whole of what a batch
//! conversion does once the plan says where things go.
//!
//! **WebP is written lossless, and that is a real limitation.** The only
//! pure-Rust WebP encoder there is encodes losslessly; a lossy one means
//! shipping libwebp, which is a C library to build once per platform. A
//! lossless WebP of a photograph is larger than a quality-85 JPEG, not
//! smaller, so this is said out loud in the dialog rather than left for
//! somebody to discover from a folder of unexpectedly large files.

use crate::decode::Rgb;
use anyhow::{Context, Result};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use std::path::Path;

/// The formats a conversion can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    /// Lossless. See the module documentation.
    WebP,
    Tiff,
    Bmp,
}

impl Format {
    /// What an extension means, or nothing for one we do not write.
    pub fn of(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::WebP),
            "tif" | "tiff" => Some(Self::Tiff),
            "bmp" => Some(Self::Bmp),
            _ => None,
        }
    }
}

/// Scales an image, keeping every pixel of the result honest.
///
/// Lanczos in the resizer's own default, which is what the thumbnails
/// already go through — one scaler for the whole application, so a tile and
/// an exported file are made the same way.
pub fn resize(source: &Rgb, width: u32, height: u32) -> Result<Rgb> {
    if source.width == width && source.height == height {
        return Ok(source.clone());
    }

    let from = Image::from_vec_u8(
        source.width,
        source.height,
        source.pixels.clone(),
        PixelType::U8x3,
    )?;
    let mut into = Image::new(width.max(1), height.max(1), PixelType::U8x3);
    Resizer::new().resize(&from, &mut into, None)?;
    Rgb::new(width.max(1), height.max(1), into.into_vec())
}

/// Sharpens an image that has just been made smaller.
///
/// An unsharp mask: blur a copy, and push every pixel away from that blur.
/// **It belongs after the downscale**, which is where the softness it makes
/// up for comes from — sharpening first and then scaling throws the work
/// away and leaves the halos.
///
/// `amount` is 0..=100 and 0 does nothing. The threshold is what keeps a
/// clear sky from turning to noise: a difference this small is grain, not an
/// edge, and grain sharpened is grain louder.
pub fn sharpen(image: &Rgb, amount: u8) -> Rgb {
    const THRESHOLD: i32 = 2;

    if amount == 0 || image.width < 3 || image.height < 3 {
        return image.clone();
    }

    let strength = f32::from(amount) / 100.0;
    let blurred = blur(image);
    let mut out = image.pixels.clone();
    for (at, (value, soft)) in image.pixels.iter().zip(&blurred).enumerate() {
        let difference = i32::from(*value) - i32::from(*soft);
        if difference.abs() < THRESHOLD {
            continue;
        }

        let pushed = f32::from(*value) + strength * difference as f32;
        out[at] = pushed.round().clamp(0.0, 255.0) as u8;
    }

    Rgb {
        width: image.width,
        height: image.height,
        pixels: out,
    }
}

/// A 3x3 binomial blur — the smallest one worth having, and the radius v1
/// sharpened at.
fn blur(image: &Rgb) -> Vec<u8> {
    const WEIGHTS: [i32; 3] = [1, 2, 1];
    let (width, height) = (image.width as usize, image.height as usize);
    let mut out = vec![0u8; image.pixels.len()];
    for y in 0..height {
        for x in 0..width {
            for channel in 0..3 {
                let mut total = 0i32;
                let mut weight = 0i32;
                for (dy, wy) in WEIGHTS.iter().enumerate() {
                    let sy = y as isize + dy as isize - 1;
                    if sy < 0 || sy as usize >= height {
                        continue;
                    }

                    for (dx, wx) in WEIGHTS.iter().enumerate() {
                        let sx = x as isize + dx as isize - 1;
                        if sx < 0 || sx as usize >= width {
                            continue;
                        }

                        let at = (sy as usize * width + sx as usize) * 3 + channel;
                        total += wx * wy * i32::from(image.pixels[at]);
                        weight += wx * wy;
                    }
                }

                let at = (y * width + x) * 3 + channel;
                out[at] = (total / weight.max(1)).clamp(0, 255) as u8;
            }
        }
    }

    out
}

/// Writes an image to a file.
///
/// Through a temporary name and a rename, like everything else here that
/// writes: a crash halfway leaves no half-written photograph where somebody
/// will later find one and wonder why it will not open.
pub fn write(path: &Path, image: &Rgb, format: Format, quality: u8) -> Result<()> {
    let bytes = encode(image, format, quality)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot make {}", parent.display()))?;
    }

    let temporary = path.with_extension("photosite-tmp");
    std::fs::write(&temporary, &bytes)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| {
        let _ = std::fs::remove_file(&temporary);
        format!("cannot put {} in place", path.display())
    })?;
    Ok(())
}

/// The file's bytes, made in memory.
pub fn encode(image: &Rgb, format: Format, quality: u8) -> Result<Vec<u8>> {
    use image::{ExtendedColorType, ImageEncoder};

    anyhow::ensure!(
        image.width > 0 && image.height > 0,
        "an image of no size cannot be written"
    );
    let mut out: Vec<u8> = Vec::new();
    let colour = ExtendedColorType::Rgb8;
    match format {
        Format::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
                .write_image(&image.pixels, image.width, image.height, colour)?;
        }
        Format::Png => {
            image::codecs::png::PngEncoder::new(&mut out).write_image(
                &image.pixels,
                image.width,
                image.height,
                colour,
            )?;
        }
        Format::WebP => {
            image::codecs::webp::WebPEncoder::new_lossless(&mut out).encode(
                &image.pixels,
                image.width,
                image.height,
                colour,
            )?;
        }
        Format::Tiff => {
            // The TIFF encoder writes into a seekable sink, which a bare Vec
            // is not.
            let mut cursor = std::io::Cursor::new(Vec::new());
            image::codecs::tiff::TiffEncoder::new(&mut cursor).write_image(
                &image.pixels,
                image.width,
                image.height,
                colour,
            )?;
            out = cursor.into_inner();
        }
        Format::Bmp => {
            image::codecs::bmp::BmpEncoder::new(&mut out).write_image(
                &image.pixels,
                image.width,
                image.height,
                colour,
            )?;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Rgb {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x * 255 / width.max(1)) as u8).wrapping_add(y as u8);
                pixels.extend_from_slice(&[value, value / 2, 255 - value]);
            }
        }

        Rgb::new(width, height, pixels).unwrap()
    }

    /// An edge with grain on both sides of it: the sharpener must find the
    /// edge and leave the grain alone.
    fn edge(width: u32, height: u32) -> Rgb {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let base = if x < width / 2 { 60u8 } else { 190 };
                let grain = if (x + y) % 2 == 0 { 1 } else { 0 };
                let value = base + grain;
                pixels.extend_from_slice(&[value, value, value]);
            }
        }

        Rgb::new(width, height, pixels).unwrap()
    }

    #[test]
    fn every_format_writes_something_that_reads_back() {
        let source = gradient(40, 30);
        for format in [
            Format::Jpeg,
            Format::Png,
            Format::WebP,
            Format::Tiff,
            Format::Bmp,
        ] {
            let bytes = encode(&source, format, 90)
                .unwrap_or_else(|error| panic!("{format:?} could not be written: {error:#}"));
            let decoded = image::load_from_memory(&bytes)
                .unwrap_or_else(|error| panic!("{format:?} could not be read back: {error:#}"))
                .to_rgb8();
            assert_eq!(
                decoded.dimensions(),
                (40, 30),
                "{format:?} came back the wrong size"
            );
        }
    }

    /// The lossless formats have to come back byte for byte. A conversion
    /// to PNG that quietly lost a bit would be a conversion nobody could
    /// trust for an archive.
    #[test]
    fn the_lossless_formats_come_back_exactly() {
        let source = gradient(32, 24);
        for format in [Format::Png, Format::WebP, Format::Tiff, Format::Bmp] {
            let bytes = encode(&source, format, 100).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
            assert_eq!(
                decoded.into_raw(),
                source.pixels,
                "{format:?} changed the pixels"
            );
        }
    }

    #[test]
    fn quality_decides_how_big_a_jpeg_is() {
        let source = gradient(200, 150);
        let small = encode(&source, Format::Jpeg, 30).unwrap();
        let large = encode(&source, Format::Jpeg, 95).unwrap();
        assert!(
            small.len() < large.len(),
            "{} vs {}",
            small.len(),
            large.len()
        );
    }

    #[test]
    fn an_image_of_no_size_is_refused_rather_than_written() {
        let empty = Rgb::new(0, 0, Vec::new()).unwrap();
        assert!(encode(&empty, Format::Jpeg, 90).is_err());
    }

    #[test]
    fn a_written_file_can_be_opened_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("out.png");
        write(&path, &gradient(10, 10), Format::Png, 90).unwrap();
        assert!(path.is_file(), "the folder was not made for it");
        let back = image::open(&path).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (10, 10));

        // And nothing half-written is left lying about.
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|e| e == "photosite-tmp")
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn resizing_keeps_the_size_it_was_asked_for() {
        let out = resize(&gradient(100, 80), 50, 40).unwrap();
        assert_eq!((out.width, out.height), (50, 40));
        assert_eq!(out.pixels.len(), 50 * 40 * 3);
    }

    #[test]
    fn resizing_to_the_same_size_is_the_same_image() {
        let source = gradient(20, 20);
        assert_eq!(resize(&source, 20, 20).unwrap(), source);
    }

    #[test]
    fn no_sharpening_changes_nothing() {
        let source = gradient(20, 20);
        assert_eq!(sharpen(&source, 0), source);
    }

    /// The point of an unsharp mask: an edge gets steeper.
    #[test]
    fn sharpening_makes_an_edge_steeper() {
        let source = edge(40, 20);
        let sharp = sharpen(&source, 80);
        let across = |image: &Rgb, x: usize| {
            let at = (10 * image.width as usize + x) * 3;
            i32::from(image.pixels[at])
        };
        let before = across(&source, 20) - across(&source, 19);
        let after = across(&sharp, 20) - across(&sharp, 19);
        assert!(
            after > before,
            "the edge did not steepen: {before} -> {after}"
        );
    }

    /// And the point of the threshold: grain is not an edge, and grain
    /// sharpened is grain louder.
    #[test]
    fn sharpening_leaves_grain_where_it_is() {
        let source = edge(40, 20);
        let sharp = sharpen(&source, 80);
        // A column well inside the flat left half, away from the edge.
        let at = (10 * 40 + 5) * 3;
        assert_eq!(
            sharp.pixels[at], source.pixels[at],
            "the flat part was sharpened"
        );
    }

    #[test]
    fn an_image_too_small_to_blur_is_left_alone() {
        let tiny = gradient(2, 2);
        assert_eq!(sharpen(&tiny, 100), tiny);
    }

    #[test]
    fn an_extension_says_what_to_write() {
        assert_eq!(Format::of("JPG"), Some(Format::Jpeg));
        assert_eq!(Format::of("jpeg"), Some(Format::Jpeg));
        assert_eq!(Format::of("tiff"), Some(Format::Tiff));
        assert_eq!(Format::of("nef"), None);
    }
}
