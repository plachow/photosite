//! Reading the little of EXIF that is needed straight away: orientation and
//! the embedded thumbnail.
//!
//! This code is handed anything that carries a `.jpg` extension on disk. In a
//! trial library of 57,606 photographs, three files were not JPEGs at all
//! despite saying so. Hence the one hard rule, guarded by a property test:
//! **never panic.** Returning `None` is always acceptable.

/// Where the embedded thumbnail sits in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thumbnail {
    pub offset: usize,
    pub len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exif {
    /// EXIF orientation, 1..8; 1 when none was found.
    pub orientation: u8,
    /// The thumbnail the photograph carries with it. Usually 160x120 and
    /// decoded in a fraction of a millisecond, which makes it the cheapest
    /// way to put something real on screen.
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

/// How many bytes from the start of the file are enough. APP1 sits right
/// behind the header.
pub const HEADER_BYTES: usize = 128 << 10;

/// Walks the JPEG markers, finds APP1 and reads what we need out of it.
///
/// The thumbnail offset is relative to `raw`, so it can be used to slice
/// directly.
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
        // Fill bytes and bodyless markers carry no length.
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

        // Past the start of the image data there are no more markers.
        if marker == 0xDA {
            return None;
        }

        at = end;
    }

    None
}

/// Reads the TIFF block inside APP1. Every access is checked, so a corrupt
/// file ends at `None` rather than at a panic.
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

    /// The thumbnail lives in IFD1, pointed at by the four bytes following
    /// the last IFD0 entry.
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

    /// Builds a minimal JPEG with an APP1 segment and the given TIFF block.
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
    fn anything_that_is_not_a_jpeg_gives_the_default() {
        assert_eq!(read(b""), Exif::NONE);
        assert_eq!(read(b"this really is not a jpeg"), Exif::NONE);
        assert_eq!(read(&[0xFF, 0xD8]), Exif::NONE);
    }

    #[test]
    fn a_truncated_app1_does_not_panic() {
        let mut raw = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
        raw.extend_from_slice(b"Exif\x00\x00II");
        assert_eq!(read(&raw).orientation, 1);
    }

    #[test]
    fn orientation_is_read_from_ifd0() {
        let raw = with_tiff(&tiff_with_orientation(6, 0));
        assert_eq!(read(&raw).orientation, 6);
        assert_eq!(read(&raw).thumbnail, None);
    }

    #[test]
    fn a_nonsense_orientation_is_ignored() {
        let raw = with_tiff(&tiff_with_orientation(77, 0));
        assert_eq!(read(&raw).orientation, 1);
    }

    #[test]
    fn an_ifd1_pointer_outside_the_block_does_not_panic() {
        let raw = with_tiff(&tiff_with_orientation(3, 0xFFFF_FF00));
        assert_eq!(read(&raw).orientation, 3);
        assert_eq!(read(&raw).thumbnail, None);
    }
}
