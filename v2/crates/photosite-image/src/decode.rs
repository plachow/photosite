//! Z cesty na disku udělá pixely v požadované velikosti.
//!
//! Dvě věci, které jsou tu jinak, než by člověk čekal, a obě jsou naměřené:
//!
//! * **Škálování v DCT doméně skoro nic neušetří.** Na sadě šedesáti fotek
//!   vyšlo 31,9 ms při požadavku na 640 px a 30,6 ms při požadavku na 320 px,
//!   tedy čtyřikrát míň výstupních pixelů za o 4 % kratší čas. Entropické
//!   dekódování všech koeficientů se udělá tak jako tak a to je celá cena.
//!   Žádat víc, než je potřeba, proto nemá smysl — ušetří to jen práci
//!   zvětšovači.
//! * **Vložený náhled je o dva řády levnější.** Přečíst prvních 128 kB
//!   souboru a dekódovat obrázek 160×120 stojí zlomek milisekundy. Proto
//!   [`quick`] existuje a proto se dlaždice plní nejdřív jím.

use crate::exif;
use anyhow::{Context, Result};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use std::io::Read as _;
use std::path::Path;

/// Obrázek v paměti: RGB, tři bajty na pixel, bez výplně řádků.
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
            .field("bajtů", &self.pixels.len())
            .finish()
    }
}

impl Rgb {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let expected = width as usize * height as usize * 3;
        anyhow::ensure!(
            pixels.len() >= expected,
            "obrázek {width}×{height} potřebuje {expected} bajtů, dostal {}",
            pixels.len()
        );
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

/// Náhled z EXIFu. Čte jen hlavičku souboru, ne celou fotku.
///
/// Vrátí `None`, když ho fotka nemá — což je v pořádku a časté u obrázků,
/// které nevznikly ve fotoaparátu.
pub fn quick(path: &Path) -> Result<Option<Rgb>> {
    let mut head = vec![0u8; exif::HEADER_BYTES];
    let mut file =
        std::fs::File::open(path).with_context(|| format!("nelze otevřít {}", path.display()))?;
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

    Ok(Some(rotate(image, meta.orientation)))
}

/// Plnohodnotné dekódování zmenšené tak, aby se vešlo do čtverce `max`.
pub fn sized(path: &Path, max: u32) -> Result<Rgb> {
    let raw = std::fs::read(path).with_context(|| format!("nelze přečíst {}", path.display()))?;
    let meta = exif::read(&raw);
    let swapped = matches!(meta.orientation, 5..=8);

    let source = if raw.starts_with(&[0xFF, 0xD8, 0xFF]) {
        jpeg_rgb(&raw, Some(max)).context("JPEG nelze dekódovat")?
    } else {
        let decoded = image::load_from_memory(&raw)
            .context("obrázek nelze dekódovat")?
            .to_rgb8();
        let (width, height) = decoded.dimensions();
        Rgb::new(width, height, decoded.into_raw())?
    };

    // Poměr stran se počítá po otočení, zmenšuje se ale před ním.
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
        // CMYK z JPEGu bývá invertovaný; pro náhled tohle stačí.
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

/// Největší rozměr se zachovaným poměrem stran, který se vejde do čtverce.
/// Nikdy nezvětšuje.
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

/// Otočí podle EXIF orientace. Zrcadlené varianty se v praxi skoro
/// nevyskytují, bere se z nich jen rotační složka.
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
    fn fit_zachova_pomer_a_nezvetsuje() {
        assert_eq!(fit(4000, 3000, 320), (320, 240));
        assert_eq!(fit(3000, 4000, 320), (240, 320));
        assert_eq!(fit(100, 50, 320), (100, 50), "menší obrázek se nenafukuje");
        assert_eq!(fit(0, 0, 320), (1, 1));
    }

    #[test]
    fn otoceni_o_devadesat_prohodi_strany() {
        let source = stripes(4, 2);
        let turned = rotate(source.clone(), 6);
        assert_eq!((turned.width, turned.height), (2, 4));
        // Čtyři otočení po devadesáti stupních vrátí původní obrázek.
        let back = rotate(rotate(rotate(turned, 6), 6), 6);
        assert_eq!(back, source);
    }

    #[test]
    fn otoceni_o_sto_osmdesat_dvakrat_je_puvodni() {
        let source = stripes(3, 5);
        assert_eq!(rotate(rotate(source.clone(), 3), 3), source);
    }

    #[test]
    fn orientace_jedna_nic_nedela() {
        let source = stripes(3, 3);
        assert_eq!(rotate(source.clone(), 1), source);
    }

    #[test]
    fn kratky_buffer_se_odmitne_misto_paniky() {
        assert!(Rgb::new(10, 10, vec![0; 10]).is_err());
    }
}
