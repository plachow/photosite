//! Čtení toho mála z EXIFu, co je potřeba hned: orientace a vložený náhled.
//!
//! Tenhle kód dostane na vstup cokoliv, co má na disku příponu `.jpg`. Ve
//! zkušební knihovně o 57 606 fotkách byly tři soubory, které JPEG nebyly,
//! přestože to tvrdily. Proto platí jediná tvrdá podmínka, kterou hlídá
//! property test: **nikdy nespadnout.** Vrátit `None` je v pořádku vždycky.

/// Kde v souboru leží vložený náhled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thumbnail {
    pub offset: usize,
    pub len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exif {
    /// EXIF orientace 1..8; 1, když se nenašla.
    pub orientation: u8,
    /// Náhled, který si fotka nese s sebou. Bývá 160×120 a dekóduje se za
    /// zlomek milisekundy, takže je to nejlevnější způsob, jak dostat na
    /// obrazovku něco skutečného.
    pub thumbnail: Option<Thumbnail>,
}

impl Exif {
    pub const NONE: Self = Self {
        orientation: 1,
        thumbnail: None,
    };
}

impl Default for Exif {
    fn default() -> Self {
        Self::NONE
    }
}

/// Kolik bajtů od začátku souboru stačí přečíst. APP1 je hned za hlavičkou.
pub const HEADER_BYTES: usize = 128 << 10;

/// Projde JPEG značky, najde APP1 a přečte z něj, co potřebujeme.
///
/// Offset náhledu je vztažený k `raw`, takže se dá rovnou použít na řez.
pub fn read(raw: &[u8]) -> Exif {
    parse(raw).unwrap_or(Exif::NONE)
}

fn parse(raw: &[u8]) -> Option<Exif> {
    if raw.len() < 4 || raw[0] != 0xFF || raw[1] != 0xD8 {
        return None;
    }

    let limit = raw.len().min(HEADER_BYTES);
    let mut at = 2usize;
    while at + 4 <= limit {
        if raw[at] != 0xFF {
            at += 1;
            continue;
        }

        let marker = raw[at + 1];
        // Výplňové bajty a značky bez těla nemají délku.
        if marker == 0xFF || matches!(marker, 0x01 | 0xD0..=0xD9) {
            at += 2;
            continue;
        }

        let len = u16::from_be_bytes([raw[at + 2], raw[at + 3]]) as usize;
        if len < 2 {
            return None;
        }

        let body = at + 4;
        let end = at.checked_add(2)?.checked_add(len)?.min(raw.len());
        if marker == 0xE1 && body + 6 <= end && &raw[body..body + 6] == b"Exif\x00\x00" {
            let tiff_at = body + 6;
            let tiff = raw.get(tiff_at..end)?;
            let reader = TiffReader::new(tiff)?;
            return Some(Exif {
                orientation: reader.orientation().unwrap_or(1),
                thumbnail: reader.thumbnail().map(|found| Thumbnail {
                    offset: tiff_at + found.offset,
                    len: found.len,
                }),
            });
        }

        // Za začátkem obrazových dat už žádné značky nejsou.
        if marker == 0xDA {
            return None;
        }

        at = end;
    }

    None
}

/// Čte TIFF blok uvnitř APP1. Každý přístup je kontrolovaný, takže poškozený
/// soubor skončí na `None` a ne na panice.
struct TiffReader<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl<'a> TiffReader<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        let little = match bytes.get(0..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        Some(Self { bytes, little })
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let slice = self.bytes.get(at..at.checked_add(2)?)?;
        Some(if self.little {
            u16::from_le_bytes([slice[0], slice[1]])
        } else {
            u16::from_be_bytes([slice[0], slice[1]])
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let slice = self.bytes.get(at..at.checked_add(4)?)?;
        Some(if self.little {
            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]])
        } else {
            u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]])
        })
    }

    fn ifd0(&self) -> Option<usize> {
        self.u32(4).map(|at| at as usize)
    }

    fn entry(&self, ifd: usize, index: usize) -> Option<usize> {
        ifd.checked_add(2)?.checked_add(index.checked_mul(12)?)
    }

    fn orientation(&self) -> Option<u8> {
        let ifd0 = self.ifd0()?;
        let count = self.u16(ifd0)? as usize;
        for index in 0..count {
            let at = self.entry(ifd0, index)?;
            if self.u16(at) == Some(0x0112) {
                let value = self.u16(at + 8)?;
                if (1..=8).contains(&value) {
                    return Some(value as u8);
                }
            }
        }

        None
    }

    /// Náhled bydlí v IFD1, na které ukazuje čtveřice bajtů za poslední
    /// položkou IFD0.
    fn thumbnail(&self) -> Option<Thumbnail> {
        let ifd0 = self.ifd0()?;
        let count = self.u16(ifd0)? as usize;
        let next = self.entry(ifd0, count)?;
        let ifd1 = self.u32(next)? as usize;
        if ifd1 == 0 {
            return None;
        }

        let entries = self.u16(ifd1)? as usize;
        let (mut offset, mut len) = (0usize, 0usize);
        for index in 0..entries {
            let at = self.entry(ifd1, index)?;
            match self.u16(at) {
                Some(0x0201) => offset = self.u32(at + 8)? as usize,
                Some(0x0202) => len = self.u32(at + 8)? as usize,
                _ => {}
            }
        }

        if len == 0 || offset == 0 || offset.checked_add(len)? > self.bytes.len() {
            return None;
        }

        Some(Thumbnail { offset, len })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Poskládá minimální JPEG s APP1 a daným TIFF blokem.
    fn with_tiff(tiff: &[u8]) -> Vec<u8> {
        let mut raw = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let len = (tiff.len() + 6 + 2) as u16;
        raw.extend_from_slice(&len.to_be_bytes());
        raw.extend_from_slice(b"Exif\x00\x00");
        raw.extend_from_slice(tiff);
        raw
    }

    fn tiff_with_orientation(value: u16, next_ifd: u32) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&value.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&next_ifd.to_le_bytes());
        tiff
    }

    #[test]
    fn co_neni_jpeg_vraci_vychozi_stav() {
        assert_eq!(read(b""), Exif::NONE);
        assert_eq!(read(b"tohle fakt neni jpeg"), Exif::NONE);
        assert_eq!(read(&[0xFF, 0xD8]), Exif::NONE);
    }

    #[test]
    fn useknuty_app1_nespadne() {
        let mut raw = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
        raw.extend_from_slice(b"Exif\x00\x00II");
        assert_eq!(read(&raw).orientation, 1);
    }

    #[test]
    fn orientace_se_precte_z_ifd0() {
        let raw = with_tiff(&tiff_with_orientation(6, 0));
        assert_eq!(read(&raw).orientation, 6);
        assert_eq!(read(&raw).thumbnail, None);
    }

    #[test]
    fn nesmyslna_orientace_se_ignoruje() {
        let raw = with_tiff(&tiff_with_orientation(77, 0));
        assert_eq!(read(&raw).orientation, 1);
    }

    #[test]
    fn ukazatel_na_ifd1_mimo_blok_nespadne() {
        let raw = with_tiff(&tiff_with_orientation(3, 0xFFFF_FF00));
        assert_eq!(read(&raw).orientation, 3);
        assert_eq!(read(&raw).thumbnail, None);
    }
}
