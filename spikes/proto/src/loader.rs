//! Načítání obrázků na pozadí.
//!
//! Dvě věci, které se sem dostaly až po měření, a obě jsou důležitější než
//! volba jazyka:
//!
//! **Není tu fronta.** Fronta byla první, co jsem zkusil, a byla to chyba: při
//! tažení scrollbarem projedou viewportem tisíce fotek, každá se zařadí, a po
//! puštění handle se těch patnáct viditelných dostane na řadu až za několika
//! tisíci mrtvými požadavky. Čekání vyšlo na sedm sekund. Místo fronty je tu
//! **seznam přání, který hlavní vlákno každý snímek přepíše** na to, co je
//! právě vidět, seřazené od středu ven. Co ze seznamu vypadne, nikdo
//! nedekóduje — zrušení je tím implicitní.
//!
//! **Rychlá dráha z EXIFu.** Změřeno: dekódovat JPEG z foťáku stojí ~36 ms a
//! škálování v DCT doméně z toho ušetří jen 4 %, protože entropické
//! dekódování všech koeficientů se udělá tak jako tak. Skoro každá fotka ale
//! nese v EXIFu vlastní náhled 160×120 — přečíst hlavičku souboru a dekódovat
//! ho stojí zlomek milisekundy. Dlaždice se proto nejdřív naplní tímhle a
//! teprve pak se doostří.

use crossbeam_channel::{Receiver, Sender};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

/// Delší hrana náhledu v mřížce.
pub const THUMB: u32 = 320;
/// Delší hrana plného náhledu v pravém docku.
pub const FULL: u32 = 2560;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// Náhled uložený v EXIFu samotné fotky, obvykle 160×120. Rozmazaný, ale
    /// je hned — a hned je tady víc než ostrý.
    Quick,
    Thumb,
    Full,
}

pub struct Loaded {
    pub path: PathBuf,
    pub kind: Kind,
    pub size: [usize; 2],
    pub rgb: Vec<u8>,
}

#[derive(Default)]
struct Wishes {
    /// Dlaždice, které nemají zatím vůbec nic. Jsou skoro zadarmo, takže jdou
    /// první.
    quick: Vec<PathBuf>,
    /// Dlaždice, které mají dostat ostrou verzi. Pořadí je priorita.
    thumbs: Vec<PathBuf>,
    /// Plný náhled vybrané fotky. Ten se scrollem nezahazuje.
    full: Option<PathBuf>,
    /// Co právě drží nějaké vlákno.
    running: HashSet<(PathBuf, Kind)>,
    /// Co už bylo dekódováno a odesláno. Bez tohohle nastane tohle: vlákno
    /// dokončí dlaždici, hlavní vlákno ji ještě nestihlo převzít, takže ji
    /// příští snímek napíše do přání znovu — a jiné vlákno ji dekóduje podruhé.
    /// Při třiceti vláknech se tím práce znásobí a čekání natáhne o vteřinu.
    done: HashSet<(PathBuf, Kind)>,
    decodes: u64,
    decode_ms: f64,
}

impl Wishes {
    fn free(&self, path: &Path, kind: Kind) -> bool {
        let job = (path.to_path_buf(), kind);
        !self.running.contains(&job) && !self.done.contains(&job)
    }

    /// Vybere další práci: plný náhled, pak rychlé náhledy, pak ostré. Nic
    /// jiného neexistuje — mrtvé požadavky nemají kde přežít.
    fn take(&mut self) -> Option<(PathBuf, Kind)> {
        if let Some(path) = self.full.clone() {
            if self.free(&path, Kind::Full) {
                self.running.insert((path.clone(), Kind::Full));
                return Some((path, Kind::Full));
            }
        }

        let quick = self
            .quick
            .iter()
            .find(|path| self.free(path, Kind::Quick))
            .cloned();
        if let Some(path) = quick {
            self.running.insert((path.clone(), Kind::Quick));
            return Some((path, Kind::Quick));
        }

        let sharp = self
            .thumbs
            .iter()
            .find(|path| self.free(path, Kind::Thumb))
            .cloned();
        if let Some(path) = sharp {
            self.running.insert((path.clone(), Kind::Thumb));
            return Some((path, Kind::Thumb));
        }

        None
    }
}

struct Shared {
    wishes: Mutex<Wishes>,
    wake: Condvar,
}

pub struct Loader {
    shared: Arc<Shared>,
    rx: Receiver<Loaded>,
}

impl Loader {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            wishes: Mutex::new(Wishes::default()),
            wake: Condvar::new(),
        });
        let (done, rx) = crossbeam_channel::unbounded::<Loaded>();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).max(1))
            .unwrap_or(4);
        for _ in 0..threads {
            spawn_worker(shared.clone(), done.clone());
        }

        Self { shared, rx }
    }

    /// Řekne, co je teď vidět. Volá se každý snímek a předchozí přání tím
    /// zanikají — v tom je celé to zrušení.
    pub fn want(&self, quick: Vec<PathBuf>, thumbs: Vec<PathBuf>, full: Option<PathBuf>) {
        let mut wishes = self.shared.wishes.lock().unwrap();
        let changed =
            wishes.quick != quick || wishes.thumbs != thumbs || wishes.full != full;
        wishes.quick = quick;
        wishes.thumbs = thumbs;
        wishes.full = full;
        drop(wishes);
        if changed {
            self.shared.wake.notify_all();
        }
    }

    /// Zapomene, že se něco už dekódovalo — volá se, když textura vypadne
    /// z cache a bude ji potřeba vyrobit znovu.
    pub fn forget(&self, key: &(PathBuf, Kind)) {
        self.shared.wishes.lock().unwrap().done.remove(key);
    }

    /// Kolik dekódů a kolik času celkem, po druzích.
    pub fn stats(&self) -> (u64, f64) {
        let wishes = self.shared.wishes.lock().unwrap();
        (wishes.decodes, wishes.decode_ms)
    }

    pub fn drain(&self, limit: usize) -> Vec<Loaded> {
        let mut out = Vec::new();
        while out.len() < limit {
            match self.rx.try_recv() {
                Ok(loaded) => out.push(loaded),
                Err(_) => break,
            }
        }

        out
    }
}

fn spawn_worker(shared: Arc<Shared>, done: Sender<Loaded>) {
    std::thread::spawn(move || loop {
        let job = {
            let mut wishes = shared.wishes.lock().unwrap();
            loop {
                if let Some(job) = wishes.take() {
                    break job;
                }

                wishes = shared.wake.wait(wishes).unwrap();
            }
        };

        let started = std::time::Instant::now();
        let decoded = match job.1 {
            Kind::Quick => embedded(&job.0),
            Kind::Thumb => decode(&job.0, THUMB),
            Kind::Full => decode(&job.0, FULL),
        };
        let took = started.elapsed().as_secs_f64() * 1000.0;
        {
            let mut wishes = shared.wishes.lock().unwrap();
            wishes.running.remove(&job);
            wishes.done.insert(job.clone());
            wishes.decodes += 1;
            wishes.decode_ms += took;
        }

        if let Some((size, rgb)) = decoded {
            if done.send(Loaded { path: job.0, kind: job.1, size, rgb }).is_err() {
                return;
            }
        }
    });
}

/// Vytáhne náhled uložený v EXIFu. Čte jen hlavičku souboru, ne celou fotku.
fn embedded(path: &Path) -> Option<([usize; 2], Vec<u8>)> {
    use std::io::Read;
    let mut head = vec![0u8; 128 << 10];
    let mut file = std::fs::File::open(path).ok()?;
    let read = file.read(&mut head).ok()?;
    head.truncate(read);

    let (orientation, range) = exif(&head)?;
    let (from, len) = range?;
    let jpeg = head.get(from..from + len)?.to_vec();

    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(&jpeg));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    let rgb = to_rgb(pixels, w, h, info.pixel_format)?;
    let (out, w, h) = rotate(rgb, w, h, orientation);
    Some(([w as usize, h as usize], out))
}

fn decode(path: &Path, max: u32) -> Option<([usize; 2], Vec<u8>)> {
    let raw = std::fs::read(path).ok()?;
    let orientation = exif(&raw).map(|(o, _)| o).unwrap_or(1);
    let swapped = matches!(orientation, 5..=8);

    let (sw, sh, rgb) = if is_jpeg(&raw) {
        let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(&raw));
        // Žádat víc, než je potřeba, nemá smysl: měřením vyšlo, že škálování
        // v DCT doméně ušetří jen 4 % času. Ušetří ale práci Lanczosu.
        let want = max.min(u16::MAX as u32) as u16;
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
        F::L16 if pixels.len() >= n * 2 => {
            Some(pixels.chunks_exact(2).flat_map(|c| [c[1], c[1], c[1]]).collect())
        }
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

/// Najde APP1 a vrátí orientaci z IFD0 a rozsah vloženého náhledu z IFD1.
/// Rozsah je vztažený k celému vstupnímu bufferu, ne k TIFF bloku.
fn exif(raw: &[u8]) -> Option<(u8, Option<(usize, usize)>)> {
    let limit = raw.len().min(128 << 10);
    let mut at = 2usize;
    while at + 4 < limit {
        if raw[at] != 0xFF {
            at += 1;
            continue;
        }

        let marker = raw[at + 1];
        let len = u16::from_be_bytes([raw[at + 2], raw[at + 3]]) as usize;
        let tiff_at = at + 10;
        if marker == 0xE1
            && tiff_at <= raw.len()
            && &raw[at + 4..tiff_at] == b"Exif\x00\x00"
        {
            let tiff = &raw[tiff_at..(at + 2 + len).min(raw.len())];
            let (orientation, thumbnail) = parse_tiff(tiff)?;
            return Some((
                orientation,
                thumbnail.map(|(from, size)| (tiff_at + from, size)),
            ));
        }

        // SOS; dál už jsou jen komprimovaná data.
        if marker == 0xDA {
            return None;
        }

        at += 2 + len;
    }

    None
}

fn parse_tiff(tiff: &[u8]) -> Option<(u8, Option<(usize, usize)>)> {
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

    let ifd0 = u32at(4)? as usize;
    let count = u16at(ifd0)? as usize;
    let mut orientation = 1u8;
    for entry in 0..count {
        let at = ifd0 + 2 + entry * 12;
        if u16at(at) == Some(0x0112) {
            if let Some(value) = u16at(at + 8) {
                if (1..=8).contains(&value) {
                    orientation = value as u8;
                }
            }
        }
    }

    // Za poslední položkou IFD0 stojí ukazatel na IFD1 a v něm bývá náhled.
    let ifd1 = u32at(ifd0 + 2 + count * 12).unwrap_or(0) as usize;
    let mut thumbnail = None;
    if ifd1 != 0 && ifd1 + 2 <= tiff.len() {
        if let Some(entries) = u16at(ifd1) {
            let (mut from, mut size) = (0usize, 0usize);
            for entry in 0..entries as usize {
                let at = ifd1 + 2 + entry * 12;
                match u16at(at) {
                    Some(0x0201) => from = u32at(at + 8).unwrap_or(0) as usize,
                    Some(0x0202) => size = u32at(at + 8).unwrap_or(0) as usize,
                    _ => {}
                }
            }

            if size > 0 && from > 0 && from.saturating_add(size) <= tiff.len() {
                thumbnail = Some((from, size));
            }
        }
    }

    Some((orientation, thumbnail))
}
