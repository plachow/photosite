//! Turns a path on disk into pixels at the size asked for.
//!
//! Two things here are not what one would expect, and both were measured:
//!
//! * **Scaling in the DCT domain saves almost nothing.** Over a set of sixty
//!   photographs it came to 31.9 ms asking for 640 px against 30.6 ms asking
//!   for 320 px — four times fewer output pixels for 4% less time. The
//!   entropy decoding of every coefficient happens either way and that is the
//!   whole cost. Asking for more than is needed is therefore pointless; it
//!   only saves the resizer some work.
//! * **The embedded thumbnail is two orders of magnitude cheaper.** Reading
//!   the first 128 kB of the file and decoding a 160x120 image costs a
//!   fraction of a millisecond. That is why [`quick`] exists and why tiles
//!   are filled from it first.

use crate::exif;
use anyhow::{Context, Result};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use std::io::Read as _;
use std::path::Path;

/// An image in memory: RGB, three bytes per pixel, no row padding.
#[derive(Clone, PartialEq, Eq)]
pub struct Rgb {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Rgb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rgb")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.pixels.len())
            .finish()
    }
}

impl Rgb {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let expected = width as usize * height as usize * 3;
        anyhow::ensure!(
            pixels.len() >= expected,
            "an image of {width}x{height} needs {expected} bytes, got {}",
            pixels.len()
        );
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

/// The thumbnail out of EXIF. Reads only the file header, not the whole
/// photograph.
///
/// Returns `None` when the photograph has none, which is fine and common for
/// images that did not come out of a camera.
pub fn quick(path: &Path) -> Result<Option<Rgb>> {
    let mut head = vec![0u8; exif::HEADER_BYTES];
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let read = file.read(&mut head)?;
    head.truncate(read);

    let meta = exif::read(&head);
    let Some(thumbnail) = meta.thumbnail else {
        return Ok(None);
    };

    let Some(jpeg) = head.get(thumbnail.offset..thumbnail.offset + thumbnail.len) else {
        return Ok(None);
    };

    let Some(image) = jpeg_rgb(jpeg, None) else {
        return Ok(None);
    };

    // The EXIF thumbnail carries whatever aspect ratio suited the camera,
    // not the ratio of the frame. A Nikon stores a 160x120 thumbnail next to
    // a 6000x4000 file and squeezes the whole scene into it — nothing is
    // cropped, it is simply narrowed. Until the sharp version arrives this is
    // what gets drawn, so the library would look squashed.
    let image = match dimensions(&head).and_then(|full| stretched_to(&image, full)) {
        Some((width, height)) => resize(&image, width, height)?,
        None => image,
    };

    Ok(Some(rotate(image, meta.orientation)))
}

/// The dimensions of the frame from the SOF marker, without decoding pixels.
///
/// The header we have already read is enough: SOF sits right behind the EXIF
/// block.
fn dimensions(raw: &[u8]) -> Option<(u32, u32)> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(raw));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    Some((info.width as u32, info.height as u32))
}

/// What size to rescale the thumbnail to so that it carries the ratio of the
/// frame.
///
/// Keeps the longer edge — this is about shape, not resolution. Returns
/// `None` when the ratio already matches; the overwhelming majority of phones
/// store the thumbnail correctly and rescaling would only blur it for
/// nothing.
fn stretched_to(thumb: &Rgb, full: (u32, u32)) -> Option<(u32, u32)> {
    let (full_w, full_h) = full;
    if thumb.width == 0 || thumb.height == 0 || full_w == 0 || full_h == 0 {
        return None;
    }

    let want = full_w as f64 / full_h as f64;
    let have = thumb.width as f64 / thumb.height as f64;
    // One pixel of rounding in a hundred is not a stretch; under a percent
    // counts as a match.
    if (want - have).abs() / want < 0.01 {
        return None;
    }

    let longer = thumb.width.max(thumb.height);
    Some(fit(full_w, full_h, longer))
}

/// A full decode, scaled down to fit inside a `max` by `max` square.
pub fn sized(path: &Path, max: u32) -> Result<Rgb> {
    let raw = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let meta = exif::read(&raw);
    let swapped = matches!(meta.orientation, 5..=8);

    let source = if raw.starts_with(&[0xFF, 0xD8, 0xFF]) {
        jpeg_rgb(&raw, Some(max)).context("the JPEG cannot be decoded")?
    } else if let Some(found) = crate::raw::preview(&raw) {
        // A RAW. What gets drawn is the JPEG the camera put inside it — the
        // rendering shown on the back of the camera — and not a demosaic of
        // our own, which would need a decoder and a colour pipeline per
        // manufacturer to look no better.
        jpeg_rgb(&raw[found], Some(max)).context("the preview inside the RAW cannot be decoded")?
    } else {
        let decoded = image::load_from_memory(&raw)
            .context("the image cannot be decoded")?
            .to_rgb8();
        let (width, height) = decoded.dimensions();
        Rgb::new(width, height, decoded.into_raw())?
    };

    // The aspect ratio is worked out after rotation, but the scaling happens
    // before it.
    let (shown_w, shown_h) = if swapped {
        (source.height, source.width)
    } else {
        (source.width, source.height)
    };
    let (target_w, target_h) = fit(shown_w, shown_h, max);
    let (resize_w, resize_h) = if swapped {
        (target_h, target_w)
    } else {
        (target_w, target_h)
    };

    let scaled = resize(&source, resize_w, resize_h)?;
    Ok(rotate(scaled, meta.orientation))
}

fn jpeg_rgb(raw: &[u8], max: Option<u32>) -> Option<Rgb> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(raw));
    if let Some(max) = max {
        let want = max.min(u16::MAX as u32) as u16;
        decoder.scale(want, want).ok()?;
    }

    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (info.width as u32, info.height as u32);
    let rgb = to_rgb(pixels, width, height, info.pixel_format)?;
    Rgb::new(width, height, rgb).ok()
}

fn to_rgb(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    format: jpeg_decoder::PixelFormat,
) -> Option<Vec<u8>> {
    use jpeg_decoder::PixelFormat as F;
    let count = width as usize * height as usize;
    match format {
        F::RGB24 if pixels.len() >= count * 3 => Some(pixels),
        F::L8 if pixels.len() >= count => Some(pixels.iter().flat_map(|&v| [v, v, v]).collect()),
        F::L16 if pixels.len() >= count * 2 => Some(
            pixels
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|c| [c[1], c[1], c[1]])
                .collect(),
        ),
        // CMYK out of a JPEG is usually inverted; for a preview this will do.
        F::CMYK32 if pixels.len() >= count * 4 => Some(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|c| {
                    let k = c[3] as u32;
                    [
                        (c[0] as u32 * k / 255) as u8,
                        (c[1] as u32 * k / 255) as u8,
                        (c[2] as u32 * k / 255) as u8,
                    ]
                })
                .collect(),
        ),
        _ => None,
    }
}

fn resize(source: &Rgb, width: u32, height: u32) -> Result<Rgb> {
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

/// The largest size, with the aspect ratio kept, that still fits the square.
/// Never enlarges.
pub fn fit(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width == 0 || height == 0 || max == 0 {
        return (1, 1);
    }

    let scale = (max as f64 / width as f64)
        .min(max as f64 / height as f64)
        .min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

/// Rotates according to the EXIF orientation. The mirrored variants barely
/// occur in practice, so only their rotation component is honoured.
pub fn rotate(source: Rgb, orientation: u8) -> Rgb {
    let turn = match orientation {
        3 | 4 => 2u32,
        5 | 6 => 1,
        7 | 8 => 3,
        _ => return source,
    };

    let (width, height) = (source.width, source.height);
    let (target_w, target_h) = if turn % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let mut pixels = vec![0u8; target_w as usize * target_h as usize * 3];
    for y in 0..height {
        for x in 0..width {
            let (nx, ny) = match turn {
                1 => (height - 1 - y, x),
                2 => (width - 1 - x, height - 1 - y),
                _ => (y, width - 1 - x),
            };
            let from = ((y * width + x) * 3) as usize;
            let to = ((ny * target_w + nx) * 3) as usize;
            pixels[to..to + 3].copy_from_slice(&source.pixels[from..from + 3]);
        }
    }

    Rgb {
        width: target_w,
        height: target_h,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripes(width: u32, height: u32) -> Rgb {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[x as u8, y as u8, 0]);
            }
        }

        Rgb::new(width, height, pixels).unwrap()
    }

    #[test]
    fn fit_keeps_the_ratio_and_never_enlarges() {
        assert_eq!(fit(4000, 3000, 320), (320, 240));
        assert_eq!(fit(3000, 4000, 320), (240, 320));
        assert_eq!(
            fit(100, 50, 320),
            (100, 50),
            "a smaller image is not blown up"
        );
        assert_eq!(fit(0, 0, 320), (1, 1));
    }

    /// An EXIF thumbnail shaped unlike its frame has to be put right.
    ///
    /// A Nikon stores a 160x120 thumbnail next to a 6000x4000 file and
    /// squeezes the whole scene into it. It was then drawn squashed into the
    /// tile until the sharp version finished decoding — which, while
    /// scrolling, is most of the time.
    #[test]
    fn a_stretched_thumbnail_takes_the_shape_of_the_frame() {
        let thumb = stripes(160, 120);
        assert_eq!(
            stretched_to(&thumb, (6000, 4000)),
            Some((160, 107)),
            "a 3:2 frame with a 4:3 thumbnail"
        );

        let thumb = stripes(120, 160);
        assert_eq!(stretched_to(&thumb, (4000, 6000)), Some((107, 160)));
    }

    #[test]
    fn a_correct_thumbnail_is_left_alone() {
        // Phones store the thumbnail at the right ratio. Rescaling it would
        // only blur it for nothing.
        assert_eq!(stretched_to(&stripes(160, 120), (4000, 3000)), None);
        assert_eq!(
            stretched_to(&stripes(159, 120), (4000, 3000)),
            None,
            "rounding is not a stretch"
        );
        assert_eq!(stretched_to(&stripes(160, 160), (4000, 4000)), None);
    }

    #[test]
    fn nonsense_dimensions_rescale_nothing() {
        assert_eq!(stretched_to(&stripes(1, 1), (0, 0)), None);
        let empty = Rgb {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        assert_eq!(stretched_to(&empty, (4000, 3000)), None);
    }

    #[test]
    fn a_quarter_turn_swaps_the_sides() {
        let source = stripes(4, 2);
        let turned = rotate(source.clone(), 6);
        assert_eq!((turned.width, turned.height), (2, 4));
        // Four quarter turns give the original image back.
        let back = rotate(rotate(rotate(turned, 6), 6), 6);
        assert_eq!(back, source);
    }

    #[test]
    fn half_a_turn_twice_is_the_original() {
        let source = stripes(3, 5);
        assert_eq!(rotate(rotate(source.clone(), 3), 3), source);
    }

    #[test]
    fn orientation_one_does_nothing() {
        let source = stripes(3, 3);
        assert_eq!(rotate(source.clone(), 1), source);
    }

    #[test]
    fn a_short_buffer_is_refused_rather_than_panicking() {
        assert!(Rgb::new(10, 10, vec![0; 10]).is_err());
    }
}
