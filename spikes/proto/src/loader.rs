//! Načítání obrázků na pozadí.
//!
//! Hlavní vlákno si řekne o cestu a velikost, dostane ji zpátky jako holé RGB.
//! JPEG se dekóduje rovnou v DCT doméně na nejbližší 1/8, 1/4 nebo 1/2, takže
//! plné rozlišení nikdy nevznikne — to je ten důvod, proč se náhledy stíhají
//! vyrábět za běhu a nemusí se nic předpočítávat.

use crossbeam_channel::{Receiver, Sender};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Delší hrana náhledu v mřížce.
pub const THUMB: u32 = 320;
/// Delší hrana plného náhledu v pravém docku. Víc než tohle stejně žádný
/// panel nezobrazí a dekódovat celých 45 Mpx by bylo plýtvání.
pub const FULL: u32 = 2560;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Thumb,
    Full,
}

impl Kind {
    fn max_side(self) -> u32 {
        match self {
            Kind::Thumb => THUMB,
            Kind::Full => FULL,
        }
    }
}

pub struct Loaded {
    pub path: PathBuf,
    pub kind: Kind,
    pub size: [usize; 2],
    pub rgb: Vec<u8>,
}

pub struct Loader {
    tx: Sender<(PathBuf, Kind)>,
    rx: Receiver<Loaded>,
    pending: HashSet<(PathBuf, Kind)>,
}

impl Loader {
    pub fn new() -> Self {
        let (tx, work) = crossbeam_channel::unbounded::<(PathBuf, Kind)>();
        let (done, rx) = crossbeam_channel::unbounded::<Loaded>();
        // O dvě vlákna míň než jader, ať zbyde na UI a na systém.
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).max(1))
            .unwrap_or(4);
        for _ in 0..threads {
            let (work, done) = (work.clone(), done.clone());
            std::thread::spawn(move || {
                while let Ok((path, kind)) = work.recv() {
                    if let Some((size, rgb)) = decode(&path, kind.max_side()) {
                        if done.send(Loaded { path, kind, size, rgb }).is_err() {
                            return;
                        }
                    }
                }
            });
        }

        Self { tx, rx, pending: HashSet::new() }
    }

    /// Požádá o obrázek. Opakovaná žádost o totéž se zahodí.
    pub fn request(&mut self, path: &Path, kind: Kind) {
        let key = (path.to_path_buf(), kind);
        if self.pending.insert(key.clone()) {
            let _ = self.tx.send(key);
        }
    }

    pub fn drain(&mut self) -> Vec<Loaded> {
        let mut out = Vec::new();
        while let Ok(loaded) = self.rx.try_recv() {
            self.pending.remove(&(loaded.path.clone(), loaded.kind));
            out.push(loaded);
        }

        out
    }
}

fn decode(path: &Path, max: u32) -> Option<([usize; 2], Vec<u8>)> {
    let raw = std::fs::read(path).ok()?;
    let orientation = exif_orientation(&raw).unwrap_or(1);
    let swapped = matches!(orientation, 5..=8);

    let (sw, sh, rgb) = if is_jpeg(&raw) {
        let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(&raw));
        // O stupeň víc, než potřebujeme, ať má Lanczos z čeho brát.
        let want = (max * 2).min(u16::MAX as u32) as u16;
        let (w, h) = decoder.scale(want, want).ok()?;
        let pixels = decoder.decode().ok()?;
        let format = decoder.info()?.pixel_format;
        (w as u32, h as u32, to_rgb(pixels, w as u32, h as u32, format)?)
    } else {
        let decoded = image::load_from_memory(&raw).ok()?.to_rgb8();
        let (w, h) = decoded.dimensions();
        (w, h, decoded.into_raw())
    };

    let (dw, dh) = if swapped { (sh, sw) } else { (sw, sh) };
    let (tw, th) = fit(dw, dh, max);
    let (rw, rh) = if swapped { (th, tw) } else { (tw, th) };

    let source = Image::from_vec_u8(sw, sh, rgb, PixelType::U8x3).ok()?;
    let mut scaled = Image::new(rw, rh, PixelType::U8x3);
    Resizer::new().resize(&source, &mut scaled, None).ok()?;
    let (pixels, w, h) = rotate(scaled.into_vec(), rw, rh, orientation);
    Some(([w as usize, h as usize], pixels))
}

fn is_jpeg(raw: &[u8]) -> bool {
    raw.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn to_rgb(
    pixels: Vec<u8>,
    w: u32,
    h: u32,
    format: jpeg_decoder::PixelFormat,
) -> Option<Vec<u8>> {
    use jpeg_decoder::PixelFormat as F;
    let n = (w as usize) * (h as usize);
    match format {
        F::RGB24 if pixels.len() >= n * 3 => Some(pixels),
        F::L8 if pixels.len() >= n => Some(pixels.iter().flat_map(|&v| [v, v, v]).collect()),
        F::L16 if pixels.len() >= n * 2 => Some(
            pixels.chunks_exact(2).flat_map(|c| [c[1], c[1], c[1]]).collect(),
        ),
        F::CMYK32 if pixels.len() >= n * 4 => Some(
            pixels
                .chunks_exact(4)
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

fn fit(w: u32, h: u32, boxed: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }

    let scale = (boxed as f64 / w as f64).min(boxed as f64 / h as f64).min(1.0);
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

/// Otočí RGB8 podle EXIF orientace. Zrcadlené varianty se v praxi skoro
/// nevyskytují, bereme z nich jen rotační složku.
fn rotate(src: Vec<u8>, w: u32, h: u32, orientation: u8) -> (Vec<u8>, u32, u32) {
    let turn = match orientation {
        3 | 4 => 2u32,
        5 | 6 => 1,
        7 | 8 => 3,
        _ => return (src, w, h),
    };
    let (dw, dh) = if turn % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 3];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match turn {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let from = ((y * w + x) * 3) as usize;
            let to = ((ny * dw + nx) * 3) as usize;
            out[to..to + 3].copy_from_slice(&src[from..from + 3]);
        }
    }

    (out, dw, dh)
}

/// Minimální čtečka EXIF orientace: najde APP1, přečte TIFF hlavičku a v IFD0
/// tag 0x0112. Nic víc z EXIFu tady nepotřebujeme.
fn exif_orientation(raw: &[u8]) -> Option<u8> {
    let limit = raw.len().min(128 << 10);
    let mut at = 2usize;
    while at + 4 < limit {
        if raw[at] != 0xFF {
            at += 1;
            continue;
        }

        let marker = raw[at + 1];
        let len = u16::from_be_bytes([raw[at + 2], raw[at + 3]]) as usize;
        if marker == 0xE1 && at + 10 <= raw.len() && &raw[at + 4..at + 10] == b"Exif\x00\x00" {
            return parse_tiff(&raw[at + 10..(at + 2 + len).min(raw.len())]);
        }

        if marker == 0xDA {
            return None;
        }

        at += 2 + len;
    }

    None
}

fn parse_tiff(tiff: &[u8]) -> Option<u8> {
    if tiff.len() < 8 {
        return None;
    }

    let little = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16at = |at: usize| -> Option<u16> {
        let s = tiff.get(at..at + 2)?;
        Some(if little {
            u16::from_le_bytes([s[0], s[1]])
        } else {
            u16::from_be_bytes([s[0], s[1]])
        })
    };
    let u32at = |at: usize| -> Option<u32> {
        let s = tiff.get(at..at + 4)?;
        Some(if little {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    };

    let ifd = u32at(4)? as usize;
    let count = u16at(ifd)? as usize;
    for entry in 0..count {
        let at = ifd + 2 + entry * 12;
        if u16at(at)? == 0x0112 {
            let value = u16at(at + 8)?;
            return (1..=8).contains(&value).then_some(value as u8);
        }
    }

    None
}
