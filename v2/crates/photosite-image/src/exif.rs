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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exif {
    /// EXIF orientation, 1..8; 1 when none was found.
    pub orientation: u8,
    /// The thumbnail the photograph carries with it. Usually 160x120 and
    /// decoded in a fraction of a millisecond, which makes it the cheapest
    /// way to put something real on screen.
    pub thumbnail: Option<Thumbnail>,
    /// When the shutter fired, in seconds since the epoch.
    ///
    /// **EXIF records no time zone.** `DateTimeOriginal` is the wall clock
    /// where the photographer stood, and this is that clock read as if it
    /// were UTC. For ordering a folder — which is what it is for — that is
    /// right: photographs taken in one place stay in the order they were
    /// taken, whatever zone anybody is in later. It is not a moment in time
    /// and must not be presented as one.
    pub taken_at: Option<i64>,
    /// The size of the frame, from the SOF marker rather than from decoding
    /// it. Sorting a folder by dimensions cannot mean decoding every file
    /// in it.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// What took the photograph, as one name.
    ///
    /// EXIF keeps the maker and the model apart, and putting them back
    /// together is not concatenation: a Nikon writes `NIKON CORPORATION` and
    /// `NIKON Z 6`, which joined naively reads as a stutter. See
    /// [`camera_name`].
    pub camera: Option<String>,
    /// The lens, where the camera bothered to record one. Phones mostly do
    /// not; interchangeable-lens cameras do.
    pub lens: Option<String>,
}

impl Exif {
    pub const NONE: Self = Self {
        orientation: 1,
        thumbnail: None,
        taken_at: None,
        width: None,
        height: None,
        camera: None,
        lens: None,
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
    // A RAW file *is* the TIFF block; a JPEG merely carries one in a
    // segment. Same reader either way, and a JPEG never begins `II` or `MM`,
    // so there is nothing to tell apart beyond the first two bytes.
    if raw.starts_with(b"II") || raw.starts_with(b"MM") {
        return from_tiff(raw).unwrap_or(Exif::NONE);
    }

    parse(raw).unwrap_or(Exif::NONE)
}

/// What a TIFF-based file says about itself.
///
/// The frame comes from the largest image any of its blocks describes: a
/// Nikon's first block describes its 160x120 thumbnail, so taking the first
/// answer reports a thumbnail's dimensions for a twenty-four megapixel
/// photograph.
fn from_tiff(raw: &[u8]) -> Option<Exif> {
    let reader = TiffReader::new(raw)?;
    let (width, height) = crate::raw::frame(raw).unzip();
    Some(Exif {
        orientation: reader.orientation().unwrap_or(1),
        // Not the EXIF thumbnail: a RAW keeps its previews elsewhere, and
        // `crate::raw::preview` is what finds them.
        thumbnail: None,
        taken_at: reader.taken_at(),
        width,
        height,
        camera: camera_name(reader.text(0, 0x010F), reader.text(0, 0x0110)),
        lens: reader.sub_ifd().and_then(|ifd| reader.text(ifd, 0xA434)),
    })
}

/// The same, straight from a file.
///
/// Reads the header and no more. A file that cannot be opened gives the
/// default rather than an error: in a library of tens of thousands one
/// unreadable file is ordinary, and the caller has nothing useful to do with
/// the failure beyond what the log already says.
pub fn read_file(path: &std::path::Path) -> Exif {
    use std::io::Read as _;

    let mut header = Vec::with_capacity(HEADER_BYTES);
    match std::fs::File::open(path) {
        Ok(file) => {
            if let Err(error) = file.take(HEADER_BYTES as u64).read_to_end(&mut header) {
                tracing::warn!(path = %path.display(), %error, "the header cannot be read");
            }
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "the file cannot be opened");
            return Exif::NONE;
        }
    }

    read(&header)
}

fn parse(raw: &[u8]) -> Option<Exif> {
    if raw.len() < 4 || raw[0] != 0xFF || raw[1] != 0xD8 {
        return None;
    }

    // Both are wanted and they sit in different segments: APP1 carries the
    // EXIF block, SOF the size of the frame, and APP1 comes first. So the
    // walk collects rather than returning at the first find — it used to
    // return at APP1, which is why the size had to be got by decoding.
    let mut found = Exif::NONE;
    let mut anything = false;

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
            break;
        }

        let body = at + 4;
        let end = at.checked_add(2)?.checked_add(len)?.min(raw.len());
        if marker == 0xE1
            && body + 6 <= end
            && raw.get(body..body + 6) == Some(&b"Exif\x00\x00"[..])
        {
            let tiff_at = body + 6;
            if let Some(tiff) = raw.get(tiff_at..end)
                && let Some(reader) = TiffReader::new(tiff)
            {
                anything = true;
                found.orientation = reader.orientation().unwrap_or(1);
                found.thumbnail = reader.thumbnail().map(|thumb| Thumbnail {
                    offset: tiff_at + thumb.offset,
                    len: thumb.len,
                });
                found.taken_at = reader.taken_at();
                found.camera = camera_name(reader.text(0, 0x010F), reader.text(0, 0x0110));
                found.lens = reader.sub_ifd().and_then(|ifd| reader.text(ifd, 0xA434));
            }
        }

        // The frame's size: precision, then height, then width. C4, C8 and
        // CC share the range and are not frame headers at all.
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if let Some(bytes) = raw.get(body + 1..body + 5) {
                let height = u16::from_be_bytes([bytes[0], bytes[1]]) as u32;
                let width = u16::from_be_bytes([bytes[2], bytes[3]]) as u32;
                if width > 0 && height > 0 {
                    anything = true;
                    found.width = Some(width);
                    found.height = Some(height);
                }
            }

            // SOF is the last thing worth reading; the scan follows.
            break;
        }

        // Past the start of the image data there are no more markers.
        if marker == 0xDA {
            break;
        }

        at = end;
    }

    anything.then_some(found)
}

/// The maker and the model as one camera name.
///
/// `NIKON CORPORATION` and `NIKON Z 6` are one camera and must read as one:
/// joined as they come they stutter, and a filter list offering both
/// `NIKON Z 6` and `NIKON CORPORATION NIKON Z 6` is worse than useless. So
/// only the maker's first word is used, and it is dropped entirely when the
/// model already begins with it.
///
/// Ported from v1, which learned this the same way.
pub fn camera_name(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (make, None) => make,
        (None, model) => model,
        (Some(make), Some(model)) => {
            let first = make.split_whitespace().next().unwrap_or(&make).to_owned();
            if model.to_lowercase().starts_with(&first.to_lowercase()) {
                Some(model)
            } else {
                Some(format!("{first} {model}"))
            }
        }
    }
}

/// Turns `YYYY:MM:DD HH:MM:SS` into seconds since the epoch.
///
/// Cameras write `0000:00:00 00:00:00` for a date they do not have, so the
/// ranges are checked rather than trusted — an unset date must come back as
/// nothing, not as the year zero sorting first in every folder.
fn parse_datetime(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.trim_end_matches('\0').trim();
    if text.len() < 19 {
        return None;
    }

    let field = |from: usize, to: usize| -> Option<i64> { text.get(from..to)?.trim().parse().ok() };
    let year = field(0, 4)?;
    let month = field(5, 7)?;
    let day = field(8, 10)?;
    let hour = field(11, 13)?;
    let minute = field(14, 16)?;
    let second = field(17, 19)?;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        // A leap second really is written as 60.
        || !(0..=60).contains(&second)
        || !(1826..=9999).contains(&year)
    {
        return None;
    }

    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days from 1970-01-01 to the given date, proleptic Gregorian.
///
/// Howard Hinnant's algorithm, which is exact for every year we could meet
/// and needs no calendar library. Bringing in a date crate for one
/// subtraction would be the more expensive answer.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Reads a TIFF block. Every access is checked, so a corrupt file ends at
/// `None` rather than at a panic.
///
/// Shared with [`crate::raw`], where the whole file is the TIFF block rather
/// than a segment carrying one — which is the only difference between a JPEG
/// and nearly every RAW format there is.
pub(crate) struct TiffReader<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl<'a> TiffReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Option<Self> {
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

    /// Every IFD in the file: the chain from IFD0, and the sub-blocks any of
    /// them point at.
    ///
    /// A RAW keeps its previews in sub-blocks — a Nikon's IFD0 describes only
    /// the 160x120 thumbnail — so looking at IFD0 alone finds a smear where
    /// the photograph should be.
    ///
    /// Bounded twice over. A corrupt file can point an IFD at itself, and a
    /// walk that trusts the offsets never comes back.
    pub(crate) fn every_ifd(&self) -> Vec<usize> {
        const MOST: usize = 32;
        let mut found: Vec<usize> = Vec::new();
        let mut queue = vec![self.ifd0().unwrap_or(0)];

        while let Some(at) = queue.pop() {
            if at == 0 || found.len() >= MOST || found.contains(&at) {
                continue;
            }

            let Some(count) = self.u16(at) else {
                continue;
            };

            if count == 0 || count as usize > 4096 {
                continue;
            }

            found.push(at);

            // The next IFD in the chain sits after the last entry.
            if let Some(end) = self.entry(at, count as usize)
                && let Some(next) = self.u32(end)
            {
                queue.push(next as usize);
            }

            // And any sub-blocks this one names.
            if let Some(entry) = self.entry_for(at, 0x014A) {
                let sub_count = self.u32(entry + 4).unwrap_or(0) as usize;
                if sub_count == 1 {
                    if let Some(offset) = self.u32(entry + 8) {
                        queue.push(offset as usize);
                    }
                } else if sub_count <= 16
                    && let Some(list) = self.u32(entry + 8)
                {
                    for index in 0..sub_count {
                        if let Some(offset) = self.u32(list as usize + index * 4) {
                            queue.push(offset as usize);
                        }
                    }
                }
            }
        }

        found
    }

    /// The value field of an entry, read as a LONG.
    pub(crate) fn value_offset(&self, entry: usize) -> Option<u32> {
        self.u32(entry + 8)
    }

    /// How many items an entry holds.
    pub(crate) fn count(&self, entry: usize) -> Option<u32> {
        self.u32(entry + 4)
    }

    /// A LONG tag in the given IFD.
    pub(crate) fn long(&self, ifd: usize, tag: u16) -> Option<u32> {
        self.u32(self.entry_for(ifd, tag)? + 8)
    }

    /// A SHORT tag. A short stored in the four-byte value field sits in its
    /// first two bytes, whichever way round the file is.
    pub(crate) fn short(&self, ifd: usize, tag: u16) -> Option<u16> {
        self.u16(self.entry_for(ifd, tag)? + 8)
    }

    /// A width or a height, which TIFF allows to be either size of integer.
    pub(crate) fn dimension(&self, ifd: usize, tag: u16) -> Option<u32> {
        let entry = self.entry_for(ifd, tag)?;
        match self.u16(entry + 2)? {
            3 => self.u16(entry + 8).map(u32::from),
            4 => self.u32(entry + 8),
            _ => None,
        }
    }

    /// Where the entry for `tag` sits in the given IFD.
    pub(crate) fn entry_for(&self, ifd: usize, tag: u16) -> Option<usize> {
        self.find(ifd, tag)
    }

    /// Where the entry for `tag` sits in the given IFD.
    ///
    /// The entry count is capped: a corrupt file can claim sixty thousand
    /// entries, and walking them costs real time on every photograph in a
    /// folder for an answer that was never going to come.
    fn find(&self, ifd: usize, tag: u16) -> Option<usize> {
        let count = (self.u16(ifd)? as usize).min(4096);
        (0..count).find_map(|index| {
            let at = self.entry(ifd, index)?;
            (self.u16(at) == Some(tag)).then_some(at)
        })
    }

    /// The ASCII value of an entry, whether it fits in the entry itself or
    /// sits elsewhere in the block.
    fn ascii(&self, at: usize) -> Option<&'a [u8]> {
        // Type 2 is ASCII. A date under any other type is not a date.
        if self.u16(at.checked_add(2)?)? != 2 {
            return None;
        }

        // A timestamp is twenty bytes. A count in the thousands means we are
        // reading something that is not one.
        let count = self.u32(at.checked_add(4)?)? as usize;
        if !(1..=64).contains(&count) {
            return None;
        }

        // Four bytes or fewer live in the entry; anything longer is at an
        // offset from the start of the TIFF block.
        let from = if count <= 4 {
            at.checked_add(8)?
        } else {
            self.u32(at.checked_add(8)?)? as usize
        };

        self.bytes.get(from..from.checked_add(count)?)
    }

    fn orientation(&self) -> Option<u8> {
        let at = self.find(self.ifd0()?, 0x0112)?;
        let value = self.u16(at + 8)?;
        (1..=8).contains(&value).then_some(value as u8)
    }

    /// The EXIF sub-block, where everything about the exposure lives.
    fn sub_ifd(&self) -> Option<usize> {
        let at = self.find(self.ifd0()?, 0x8769)?;
        Some(self.u32(at + 8)? as usize)
    }

    /// An ASCII tag as a tidy string, or nothing.
    ///
    /// `ifd` of nought means IFD0; anything else is the offset of a block
    /// [`Self::sub_ifd`] found. Trailing NULs and padding spaces are stripped
    /// — cameras pad these fields, and `Canon EOS R6      ` and `Canon EOS
    /// R6` would otherwise be two different cameras in the filter list.
    fn text(&self, ifd: usize, tag: u16) -> Option<String> {
        let ifd = if ifd == 0 { self.ifd0()? } else { ifd };
        let at = self.find(ifd, tag)?;
        let bytes = self.ascii_long(at)?;
        let text = std::str::from_utf8(bytes).ok()?;
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// [`Self::ascii`] with room for a name rather than a timestamp.
    ///
    /// A lens is written out in full — `EF24-70mm f/2.8L II USM` — so the
    /// twenty-byte ceiling a date needs would cut it off.
    fn ascii_long(&self, at: usize) -> Option<&'a [u8]> {
        if self.u16(at.checked_add(2)?)? != 2 {
            return None;
        }

        let count = self.u32(at.checked_add(4)?)? as usize;
        if !(1..=256).contains(&count) {
            return None;
        }

        let from = if count <= 4 {
            at.checked_add(8)?
        } else {
            self.u32(at.checked_add(8)?)? as usize
        };

        self.bytes.get(from..from.checked_add(count)?)
    }

    fn taken_at(&self) -> Option<i64> {
        // DateTimeOriginal is when the shutter fired. DateTime is when the
        // file was last written — the same moment straight out of a camera,
        // and the wrong one for anything ever edited. Hence the order, and
        // hence taking the second only when there is no first.
        self.sub_ifd()
            .and_then(|ifd| self.find(ifd, 0x9003))
            .and_then(|at| self.ascii(at))
            .and_then(parse_datetime)
            .or_else(|| {
                self.ifd0()
                    .and_then(|ifd| self.find(ifd, 0x0132))
                    .and_then(|at| self.ascii(at))
                    .and_then(parse_datetime)
            })
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

    /// A TIFF block holding one date, either in the EXIF sub-block under
    /// `DateTimeOriginal` or in IFD0 under `DateTime`.
    fn tiff_with_date(text: &str, in_sub_block: bool) -> Vec<u8> {
        // The header is 8 bytes, an IFD with one entry is 2 + 12 + 4 = 18.
        const IFD0: u32 = 8;
        const SUB: u32 = 26;
        let text_at = if in_sub_block { SUB + 18 } else { SUB };

        let entry = |tag: u16, kind: u16, value: u32| {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
            bytes
        };
        // ASCII entries carry their length, not a count of one.
        let ascii_entry = |tag: u16, at: u32, len: u32| {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&len.to_le_bytes());
            bytes.extend_from_slice(&at.to_le_bytes());
            bytes
        };

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&IFD0.to_le_bytes());

        let len = text.len() as u32 + 1;
        tiff.extend_from_slice(&1u16.to_le_bytes());
        if in_sub_block {
            tiff.extend_from_slice(&entry(0x8769, 4, SUB));
        } else {
            tiff.extend_from_slice(&ascii_entry(0x0132, text_at, len));
        }

        tiff.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(tiff.len() as u32, SUB, "the sub-block moved");

        if in_sub_block {
            tiff.extend_from_slice(&1u16.to_le_bytes());
            tiff.extend_from_slice(&ascii_entry(0x9003, text_at, len));
            tiff.extend_from_slice(&0u32.to_le_bytes());
        }

        assert_eq!(tiff.len() as u32, text_at, "the text moved");
        tiff.extend_from_slice(text.as_bytes());
        tiff.push(0);
        tiff
    }

    /// A frame header, as it sits behind the EXIF block in every JPEG.
    fn sof(width: u16, height: u16) -> Vec<u8> {
        let mut raw = vec![0xFF, 0xC0];
        raw.extend_from_slice(&17u16.to_be_bytes());
        raw.push(8);
        raw.extend_from_slice(&height.to_be_bytes());
        raw.extend_from_slice(&width.to_be_bytes());
        raw.push(3);
        raw.extend_from_slice(&[1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        raw
    }

    #[test]
    fn the_size_of_the_frame_comes_from_the_sof_marker() {
        let mut raw = with_tiff(&tiff_with_orientation(6, 0));
        raw.extend_from_slice(&sof(6000, 4000));
        let meta = read(&raw);
        assert_eq!((meta.width, meta.height), (Some(6000), Some(4000)));
        // Reading on for the size must not lose what came before it.
        assert_eq!(meta.orientation, 6);
    }

    #[test]
    fn a_frame_with_no_sof_simply_has_no_size() {
        let meta = read(&with_tiff(&tiff_with_orientation(1, 0)));
        assert_eq!((meta.width, meta.height), (None, None));
    }

    #[test]
    fn the_date_is_read_from_the_exif_sub_block() {
        let raw = with_tiff(&tiff_with_date("2024:07:14 09:30:00", true));
        // 2024-07-14 09:30:00 UTC.
        assert_eq!(read(&raw).taken_at, Some(1_720_949_400));
    }

    #[test]
    fn without_an_original_the_date_falls_back_to_the_one_in_ifd0() {
        let raw = with_tiff(&tiff_with_date("2024:07:14 09:30:00", false));
        assert_eq!(read(&raw).taken_at, Some(1_720_949_400));
    }

    #[test]
    fn a_date_the_camera_never_set_is_no_date() {
        let raw = with_tiff(&tiff_with_date("0000:00:00 00:00:00", true));
        assert_eq!(read(&raw).taken_at, None);
    }

    #[test]
    fn nonsense_where_a_date_should_be_is_no_date() {
        for text in [
            "not a date at all",
            "2024:13:01 00:00:00",
            "2024:01:32 00:00:00",
            "2024:01:01 25:00:00",
            "1200:01:01 00:00:00",
        ] {
            let raw = with_tiff(&tiff_with_date(text, true));
            assert_eq!(read(&raw).taken_at, None, "{text}");
        }
    }

    #[test]
    fn the_calendar_arithmetic_lands_where_it_should() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(
            days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28),
            2
        );
        assert_eq!(
            days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28),
            1
        );
        assert_eq!(days_from_civil(2000, 1, 1) * 86_400, 946_684_800);
    }

    #[test]
    fn a_stuttering_camera_name_is_said_once() {
        // The case that made v1 grow this rule.
        assert_eq!(
            camera_name(Some("NIKON CORPORATION".into()), Some("NIKON Z 6".into())),
            Some("NIKON Z 6".into())
        );
    }

    #[test]
    fn a_maker_the_model_does_not_name_is_put_in_front() {
        assert_eq!(
            camera_name(Some("Canon".into()), Some("EOS R6".into())),
            Some("Canon EOS R6".into())
        );
        // Only the first word of the maker: nobody wants to read
        // "SONY CORPORATION ILCE-7M3" in a list of six.
        assert_eq!(
            camera_name(Some("SONY CORPORATION".into()), Some("ILCE-7M3".into())),
            Some("SONY ILCE-7M3".into())
        );
    }

    #[test]
    fn either_one_alone_is_the_whole_name() {
        assert_eq!(
            camera_name(Some("Canon".into()), None),
            Some("Canon".into())
        );
        assert_eq!(
            camera_name(None, Some("EOS R6".into())),
            Some("EOS R6".into())
        );
        assert_eq!(camera_name(None, None), None);
    }

    #[test]
    fn a_leap_second_is_a_second_like_any_other() {
        let raw = with_tiff(&tiff_with_date("2016:12:31 23:59:60", true));
        assert!(read(&raw).taken_at.is_some());
    }
}
