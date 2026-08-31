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

#[derive(Debug, Clone, PartialEq)]
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
    /// How the photograph was actually taken: the shutter, the aperture, the
    /// sensitivity and the focal length. Together they are the one thing a
    /// photographer reads off a frame before anything else.
    pub exposure: Exposure,
    /// What the camera's clock was set to, against UTC, in seconds.
    ///
    /// The one thing that turns [`Self::taken_at`] from a wall clock into a
    /// moment. Newer cameras and phones write it; older ones do not, and for
    /// those there is no honest way to work it out.
    pub offset_seconds: Option<i32>,
    /// Where it was taken, and everything the file says about how well the
    /// camera knew that. What to make of it is not decided here — see
    /// [`Gps`].
    pub gps: Option<Gps>,
}

/// The exposure, as the camera recorded it.
///
/// Each piece separately, because cameras write whichever ones they feel
/// like: a phone records the shutter and the aperture and no focal length
/// worth the name, an old scan records nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Exposure {
    /// The shutter, in seconds. Written as a rational, so 1/250 arrives
    /// exactly as it was meant rather than as 0.004.
    pub seconds: Option<f64>,
    /// The f-number.
    pub aperture: Option<f64>,
    /// ISO.
    pub sensitivity: Option<u32>,
    /// The focal length in millimetres, as it was on the lens.
    pub focal_mm: Option<f64>,
    /// And the same in the terms everybody compares by, where the camera
    /// bothered to work it out.
    pub focal_equivalent_mm: Option<u32>,
}

impl Exposure {
    pub const NONE: Self = Self {
        seconds: None,
        aperture: None,
        sensitivity: None,
        focal_mm: None,
        focal_equivalent_mm: None,
    };

    pub fn is_empty(&self) -> bool {
        *self == Self::NONE
    }

    /// The shutter as a photographer says it: `1/250`, or `2.5 s` once it is
    /// long enough to count out loud.
    pub fn shutter(&self) -> Option<String> {
        let seconds = self.seconds?;
        if seconds <= 0.0 || !seconds.is_finite() {
            return None;
        }

        if seconds >= 1.0 {
            return Some(format!("{seconds:.1} s"));
        }

        // Rounded to the nearest whole denominator: a camera writes 1/249 or
        // 10/2500 depending on its mood, and both mean 1/250.
        Some(format!("1/{}", (1.0 / seconds).round() as i64))
    }

    /// `f/2.8`, and `f/8` rather than `f/8.0`.
    pub fn f_number(&self) -> Option<String> {
        let aperture = self.aperture.filter(|value| *value > 0.0)?;
        Some(if (aperture.fract()).abs() < 0.05 {
            format!("f/{aperture:.0}")
        } else {
            format!("f/{aperture:.1}")
        })
    }
}

/// What the file says about where it was taken.
///
/// Raw, and deliberately so: whether a position is to be trusted is a
/// judgement, and judgements belong where they can be read and argued with
/// rather than buried in a parser. This is the evidence; the verdict is
/// reached in the core.
#[derive(Debug, Clone, PartialEq)]
pub struct Gps {
    /// Degrees, north and east positive, as everything from a map to a URL
    /// expects. EXIF keeps the sign in a separate letter.
    pub latitude: f64,
    pub longitude: f64,
    /// Metres above sea level, negative below it.
    pub altitude: Option<f64>,
    /// What the camera itself thought its horizontal error was, in metres.
    /// Phones write it; cameras with a GPS chip mostly do not.
    pub error_metres: Option<f64>,
    /// `GPS`, `CELLID`, `WLAN`, `MANUAL` — how the position was arrived at.
    pub method: Option<String>,
    /// When the fix was taken, which is not always when the shutter fired.
    /// Seconds since the epoch, and genuinely UTC: unlike `DateTimeOriginal`,
    /// the GPS stamp is the satellites' own clock.
    pub fixed_at: Option<i64>,
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
        exposure: Exposure::NONE,
        offset_seconds: None,
        gps: None,
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

/// The size of the frame, for a file whose SOF marker sits past the header.
///
/// The header we read is 128 KB, which holds the EXIF block and the SOF of
/// nearly every photograph — but only nearly. A phone that writes a large
/// embedded preview pushes the frame header past it, and on one real library
/// that happened twenty bytes over the line: the dimensions were missing, the
/// photograph dropped out of a sort by size and out of the shape filter, and
/// nothing said why.
///
/// So when the header does not hold the answer, the file is walked for it —
/// by seeking from segment to segment rather than reading it in. It costs a
/// handful of seeks, and only for the files that need it.
pub fn frame_in_file(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let mut two = [0u8; 2];
    file.read_exact(&mut two).ok()?;
    if two != [0xFF, 0xD8] {
        return None;
    }

    // A generous bound rather than none: a corrupt file must not be walked
    // for ever, and no real JPEG has this many segments before its frame.
    for _ in 0..4_096 {
        let mut marker = [0u8; 2];
        file.read_exact(&mut marker).ok()?;
        if marker[0] != 0xFF {
            return None;
        }

        let kind = marker[1];
        // Fill bytes and bodyless markers carry no length.
        if kind == 0xFF || matches!(kind, 0x01 | 0xD0..=0xD9) {
            file.seek(SeekFrom::Current(-1)).ok()?;
            continue;
        }

        // Past the start of the image data there are no more markers.
        if kind == 0xDA {
            return None;
        }

        let mut length = [0u8; 2];
        file.read_exact(&mut length).ok()?;
        let length = u16::from_be_bytes(length) as i64;
        if length < 2 {
            return None;
        }

        if matches!(kind, 0xC0..=0xCF) && !matches!(kind, 0xC4 | 0xC8 | 0xCC) {
            // Precision, then height, then width.
            let mut frame = [0u8; 5];
            file.read_exact(&mut frame).ok()?;
            let height = u16::from_be_bytes([frame[1], frame[2]]) as u32;
            let width = u16::from_be_bytes([frame[3], frame[4]]) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }

        file.seek(SeekFrom::Current(length - 2)).ok()?;
    }

    None
}

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
        exposure: reader.exposure(),
        offset_seconds: reader.offset_seconds(),
        gps: reader.gps(),
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
                found.exposure = reader.exposure();
                found.offset_seconds = reader.offset_seconds();
                found.gps = reader.gps();
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
    /// The exposure, out of the block the camera keeps it in.
    fn exposure(&self) -> Exposure {
        let Some(ifd) = self.sub_ifd() else {
            return Exposure::NONE;
        };

        Exposure {
            seconds: self.rational(ifd, 0x829A),
            aperture: self.rational(ifd, 0x829D),
            // A SHORT for anything up to 65535 and a LONG above it, and
            // cameras that go past that write both. Either will do.
            sensitivity: self
                .short(ifd, 0x8827)
                .map(u32::from)
                .or_else(|| self.long(ifd, 0x8833)),
            focal_mm: self.rational(ifd, 0x920A),
            focal_equivalent_mm: self.short(ifd, 0xA405).map(u32::from),
        }
    }

    /// What the camera's clock was set to, against UTC.
    ///
    /// `OffsetTimeOriginal` is the one that belongs to the shutter; the other
    /// two are the file's write time and the digitising time, and either is a
    /// better guess than nothing. Written as `+01:00`, or `-05:00`.
    fn offset_seconds(&self) -> Option<i32> {
        let ifd = self.sub_ifd()?;
        let text = self
            .text(ifd, 0x9011)
            .or_else(|| self.text(ifd, 0x9010))
            .or_else(|| self.text(ifd, 0x9012))?;

        let sign = match text.chars().next()? {
            '+' => 1,
            '-' => -1,
            _ => return None,
        };
        let mut parts = text[1..].split(':');
        let hours: i32 = parts.next()?.trim().parse().ok()?;
        let minutes: i32 = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
        let seconds = sign * (hours * 3_600 + minutes * 60);
        (-14 * 3_600..=14 * 3_600)
            .contains(&seconds)
            .then_some(seconds)
    }

    /// Where the photograph says it was taken.
    ///
    /// GPS lives in a block of its own, hung off IFD0 under 0x8825. Nothing
    /// in it is required, and phones and cameras disagree about which parts
    /// they write, so every piece is read on its own and missing ones are
    /// simply missing.
    fn gps(&self) -> Option<Gps> {
        let ifd = self.u32(self.find(self.ifd0()?, 0x8825)? + 8)? as usize;
        let latitude = self.degrees(ifd, 0x0002, self.letter(ifd, 0x0001), b'S')?;
        let longitude = self.degrees(ifd, 0x0004, self.letter(ifd, 0x0003), b'W')?;

        // A camera with no fix writes zeroes rather than nothing at all.
        // Null Island is in the Gulf of Guinea and nobody's holiday was
        // there.
        if latitude == 0.0 && longitude == 0.0 {
            return None;
        }

        // Above or below the sea: 1 means below, and the number itself is
        // never negative.
        let below = self.short(ifd, 0x0005) == Some(1);
        let altitude = self
            .rational(ifd, 0x0006)
            .map(|metres| if below { -metres } else { metres });

        Some(Gps {
            latitude,
            longitude,
            altitude,
            error_metres: self.rational(ifd, 0x001F),
            method: self.method(ifd),
            fixed_at: self.fixed_at(ifd),
        })
    }

    /// One coordinate: degrees, minutes and seconds, and the letter that
    /// says which side of nothing it is on.
    fn degrees(&self, ifd: usize, tag: u16, letter: Option<u8>, negative: u8) -> Option<f64> {
        let parts = self.rationals(ifd, tag)?;
        let degrees = *parts.first()?;
        let minutes = parts.get(1).copied().unwrap_or(0.0);
        let seconds = parts.get(2).copied().unwrap_or(0.0);
        let value = degrees + minutes / 60.0 + seconds / 3600.0;
        if !value.is_finite() || value > 180.0 {
            return None;
        }

        Some(if letter == Some(negative) {
            -value
        } else {
            value
        })
    }

    /// The first letter of a one-character ASCII tag: `N`, `S`, `E`, `W`.
    fn letter(&self, ifd: usize, tag: u16) -> Option<u8> {
        let at = self.find(ifd, tag)?;
        self.ascii_long(at)?
            .iter()
            .copied()
            .find(|byte| byte.is_ascii_alphabetic())
            .map(|byte| byte.to_ascii_uppercase())
    }

    /// How the position was arrived at.
    ///
    /// **Written two different ways in the wild.** The specification says
    /// UNDEFINED with a seven-byte character-set header — `ASCII\0\0\0` for
    /// the only one anybody writes — and phones write a plain ASCII string
    /// instead. Insisting on the specification read nothing at all from four
    /// hundred real photographs, nine of which were fixed off a cell tower
    /// and were being called precise for it.
    fn method(&self, ifd: usize) -> Option<String> {
        let at = self.find(ifd, 0x001B)?;
        let bytes = self.value_bytes(at)?;
        let text = bytes
            .strip_prefix(b"ASCII\0\0\0")
            .or_else(|| bytes.strip_prefix(b"ASCII\0\0"))
            .unwrap_or(bytes);
        let text = std::str::from_utf8(text).ok()?;
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| text.to_ascii_uppercase())
    }

    /// When the fix was taken, from the date and the time, which are two
    /// tags of two different types and neither is worth anything alone.
    fn fixed_at(&self, ifd: usize) -> Option<i64> {
        let date = self.text(ifd, 0x001D)?;
        let mut parts = date.split([':', '-', '/']).filter_map(|p| p.parse().ok());
        let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);

        let time = self.rationals(ifd, 0x0007).unwrap_or_default();
        let hour = time.first().copied().unwrap_or(0.0);
        let minute = time.get(1).copied().unwrap_or(0.0);
        let second = time.get(2).copied().unwrap_or(0.0);

        Some(
            days_from_civil(year, month, day) * 86_400
                + (hour * 3_600.0 + minute * 60.0 + second) as i64,
        )
    }

    /// The RATIONAL values of a tag: pairs of longs, a numerator and then a
    /// denominator, held wherever the entry points.
    fn rationals(&self, ifd: usize, tag: u16) -> Option<Vec<f64>> {
        let at = self.find(ifd, tag)?;
        // Type 5 is RATIONAL and 10 is its signed twin. Nothing else is a
        // number of this shape, and reading one as if it were gives nonsense
        // rather than an error.
        let kind = self.u16(at + 2)?;
        if kind != 5 && kind != 10 {
            return None;
        }

        let count = (self.u32(at + 4)? as usize).min(8);
        let start = self.u32(at + 8)? as usize;
        let mut found = Vec::with_capacity(count);
        for index in 0..count {
            let pair = start.checked_add(index.checked_mul(8)?)?;
            let numerator = self.u32(pair)?;
            let denominator = self.u32(pair.checked_add(4)?)?;
            found.push(if denominator == 0 {
                0.0
            } else if kind == 10 {
                numerator as i32 as f64 / denominator as i32 as f64
            } else {
                numerator as f64 / denominator as f64
            });
        }

        Some(found)
    }

    /// A tag holding one rational.
    fn rational(&self, ifd: usize, tag: u16) -> Option<f64> {
        self.rationals(ifd, tag)?.first().copied()
    }

    /// The bytes of a tag that holds bytes, in the entry or out of it.
    ///
    /// BYTE, ASCII and UNDEFINED are one byte per item and are read the same
    /// way; nothing else is, and reading a rational as text gives rubbish
    /// rather than an error.
    fn value_bytes(&self, at: usize) -> Option<&'a [u8]> {
        if !matches!(self.u16(at.checked_add(2)?)?, 1 | 2 | 7) {
            return None;
        }

        let count = (self.u32(at.checked_add(4)?)? as usize).min(256);
        if count <= 4 {
            return self.bytes.get(at + 8..at + 8 + count);
        }

        let start = self.u32(at.checked_add(8)?)? as usize;
        self.bytes.get(start..start.checked_add(count)?)
    }

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

    /// A TIFF whose IFD0 points at a GPS block holding the given entries.
    /// Each entry is a tag, a type and its bytes, laid out after the blocks.
    fn tiff_with_gps(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let ifd0_at = 8usize;
        let gps_at = ifd0_at + 2 + 12 + 4;
        let data_at = gps_at + 2 + entries.len() * 12 + 4;

        let mut data = Vec::new();
        let mut placed = Vec::new();
        for (tag, kind, bytes) in entries {
            // Four bytes or fewer live in the entry itself; the count is a
            // count of items, and for a rational an item is eight bytes.
            let items = match kind {
                5 | 10 => bytes.len() / 8,
                _ => bytes.len(),
            } as u32;
            if bytes.len() <= 4 {
                let mut inline = bytes.clone();
                inline.resize(4, 0);
                placed.push((
                    *tag,
                    *kind,
                    items,
                    u32::from_le_bytes([inline[0], inline[1], inline[2], inline[3]]),
                ));
            } else {
                placed.push((*tag, *kind, items, (data_at + data.len()) as u32));
                data.extend_from_slice(bytes);
            }
        }

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&(ifd0_at as u32).to_le_bytes());

        // IFD0: one entry, the pointer to the GPS block.
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x8825u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&(gps_at as u32).to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        tiff.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in &placed {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&kind.to_le_bytes());
            tiff.extend_from_slice(&count.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
        }

        tiff.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            tiff.len(),
            data_at,
            "the data does not start where the entries say"
        );
        tiff.extend_from_slice(&data);
        tiff
    }

    /// Degrees, minutes and seconds as three rationals.
    fn dms(degrees: u32, minutes: u32, seconds: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (numerator, denominator) in [(degrees, 1u32), (minutes, 1), (seconds, 1)] {
            bytes.extend_from_slice(&numerator.to_le_bytes());
            bytes.extend_from_slice(&denominator.to_le_bytes());
        }

        bytes
    }

    /// The one a real library taught us: a phone that writes a large
    /// embedded preview pushes the frame header past the 128 KB we read, and
    /// twenty bytes over the line was enough to lose the dimensions.
    #[test]
    fn the_frame_is_found_even_when_it_sits_past_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jpg");

        // A JPEG with a very large APP1 in front of the frame header. The
        // segment length field is sixteen bits, so it takes several.
        let mut raw = vec![0xFF, 0xD8];
        for _ in 0..4 {
            raw.extend_from_slice(&[0xFF, 0xE1]);
            raw.extend_from_slice(&65_535u16.to_be_bytes());
            raw.resize(raw.len() + 65_533, 0x41);
        }

        raw.extend_from_slice(&[0xFF, 0xC0]);
        raw.extend_from_slice(&11u16.to_be_bytes());
        raw.push(8);
        raw.extend_from_slice(&2_736u16.to_be_bytes());
        raw.extend_from_slice(&3_648u16.to_be_bytes());
        raw.extend_from_slice(&[3, 0, 0, 0]);
        std::fs::write(&path, &raw).unwrap();

        assert!(
            raw.len() > HEADER_BYTES,
            "the fixture has to be larger than what is read"
        );
        assert_eq!(read(&raw[..HEADER_BYTES]).width, None, "nothing to guard");
        assert_eq!(frame_in_file(&path), Some((3_648, 2_736)));
    }

    #[test]
    fn walking_a_file_for_a_frame_gives_up_rather_than_hanging() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("empty.jpg", vec![]),
            ("nonsense.jpg", b"not a jpeg at all".to_vec()),
            ("truncated.jpg", vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF]),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(frame_in_file(&path), None, "{name}");
        }

        assert_eq!(frame_in_file(&dir.path().join("nothing.jpg")), None);
    }

    #[test]
    fn a_position_is_read_as_degrees_with_the_letter_for_a_sign() {
        // 50 deg 4' 32" N, 14 deg 26' 16" E, which is Prague.
        let tiff = tiff_with_gps(&[
            (0x0001, 2, b"N\0".to_vec()),
            (0x0002, 5, dms(50, 4, 32)),
            (0x0003, 2, b"E\0".to_vec()),
            (0x0004, 5, dms(14, 26, 16)),
        ]);
        let gps = read(&with_tiff(&tiff)).gps.expect("no position");
        assert!((gps.latitude - 50.075_555).abs() < 1e-5, "{gps:?}");
        assert!((gps.longitude - 14.437_777).abs() < 1e-5, "{gps:?}");

        // And the southern, western half of the world, where the numbers are
        // the same and the letters are not.
        let tiff = tiff_with_gps(&[
            (0x0001, 2, b"S\0".to_vec()),
            (0x0002, 5, dms(50, 4, 32)),
            (0x0003, 2, b"W\0".to_vec()),
            (0x0004, 5, dms(14, 26, 16)),
        ]);
        let gps = read(&with_tiff(&tiff)).gps.expect("no position");
        assert!(gps.latitude < 0.0 && gps.longitude < 0.0, "{gps:?}");
    }

    /// The one real files taught us. The specification says UNDEFINED with a
    /// character-set header; phones write a plain ASCII string. Reading only
    /// the specification called nine cell-tower fixes precise.
    #[test]
    fn the_method_is_read_however_the_camera_spelled_it() {
        let position = |method: (u16, Vec<u8>)| {
            let tiff = tiff_with_gps(&[
                (0x0001, 2, b"N\0".to_vec()),
                (0x0002, 5, dms(50, 0, 0)),
                (0x0003, 2, b"E\0".to_vec()),
                (0x0004, 5, dms(14, 0, 0)),
                (0x001B, method.0, method.1),
            ]);
            read(&with_tiff(&tiff)).gps.expect("no position").method
        };

        assert_eq!(
            position((2, b"CELLID\0".to_vec())),
            Some("CELLID".to_owned())
        );
        assert_eq!(position((2, b"GPS\0".to_vec())), Some("GPS".to_owned()));
        assert_eq!(
            position((7, b"ASCII\0\0\0CELLID".to_vec())),
            Some("CELLID".to_owned()),
            "the character-set header was left in the answer"
        );
    }

    #[test]
    fn a_camera_with_no_fix_is_not_placed_in_the_gulf_of_guinea() {
        let tiff = tiff_with_gps(&[
            (0x0001, 2, b"N\0".to_vec()),
            (0x0002, 5, dms(0, 0, 0)),
            (0x0003, 2, b"E\0".to_vec()),
            (0x0004, 5, dms(0, 0, 0)),
        ]);
        assert_eq!(read(&with_tiff(&tiff)).gps, None);
    }

    #[test]
    fn a_photograph_with_no_gps_block_says_so() {
        assert_eq!(read(&with_tiff(&tiff_with_orientation(1, 0))).gps, None);
    }

    #[test]
    fn a_gps_block_pointing_at_nothing_does_not_panic() {
        let mut tiff = tiff_with_gps(&[
            (0x0001, 2, b"N\0".to_vec()),
            (0x0002, 5, dms(50, 0, 0)),
            (0x0003, 2, b"E\0".to_vec()),
            (0x0004, 5, dms(14, 0, 0)),
        ]);
        tiff.truncate(tiff.len() - 20);
        let _ = read(&with_tiff(&tiff));
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
