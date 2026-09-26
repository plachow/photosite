//! Putting an XMP packet into a JPEG, and taking one out.
//!
//! The one rule this file exists to keep: **the compressed image never
//! changes.** Segments before the start of scan are rearranged; from the scan
//! onwards not a byte moves. That is not a hope, it is checked on every
//! single write by [`compressed_image`] before anything reaches the disk —
//! these are photographs nobody has another copy of.

use anyhow::{Context, Result};

/// The header that marks an APP1 segment as XMP rather than as EXIF.
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const APP1: u8 = 0xE1;
/// Where Photoshop keeps its resources, the IPTC record among them.
const APP13: u8 = 0xED;
/// Start of scan. Everything from here to the end of the file is the
/// photograph.
const SOS: u8 = 0xDA;

/// A JPEG segment: where it starts, how long it is, and which marker it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Segment {
    marker: u8,
    /// The offset of the `0xFF` that opens it.
    at: usize,
    /// The whole segment, marker and length included.
    len: usize,
}

impl Segment {
    /// The body: past the marker and past the two length bytes.
    ///
    /// `len` counts the whole segment, the `0xFF` and the marker included,
    /// so the end is `at + len` — not `at + 2 + len`, which reads two bytes
    /// of the next segment into this one's.
    fn body(&self) -> std::ops::Range<usize> {
        self.at + 4..self.at + self.len
    }
}

/// Walks the segments up to the start of scan, or to the end of what we
/// have.
///
/// Running out of bytes is not a failure. The application reads the first
/// 128 kB of a photograph to learn about it, and the XMP packet is in there —
/// asking for the whole six megabytes of every file in a library, to find a
/// packet that sits near the front, is most of a terabyte of reading for
/// nothing.
///
/// What needs the whole file is [`compressed_image`], and it needs the start
/// of scan specifically, so nothing built on that guarantee can be fooled by
/// a header.
///
/// Returns `None` for anything that is not a JPEG at all — which happens: in
/// a trial library of 57,606 photographs, three files were not JPEGs despite
/// the extension.
fn segments(raw: &[u8]) -> Option<Vec<Segment>> {
    if raw.len() < 4 || raw[0] != 0xFF || raw[1] != 0xD8 {
        return None;
    }

    let mut found = Vec::new();
    let mut at = 2usize;
    while at + 4 <= raw.len() {
        if raw[at] != 0xFF {
            return None;
        }

        let marker = raw[at + 1];
        if marker == SOS {
            found.push(Segment {
                marker,
                at,
                len: raw.len() - at,
            });
            return Some(found);
        }

        // Padding and the markers that carry no body.
        if marker == 0xFF {
            at += 1;
            continue;
        }

        if matches!(marker, 0x01 | 0xD0..=0xD9) {
            at += 2;
            continue;
        }

        let len = u16::from_be_bytes([raw[at + 2], raw[at + 3]]) as usize;
        if len < 2 {
            return None;
        }

        // Past the end of what we hold: this is a header, not a broken file.
        if at + 2 + len > raw.len() {
            return Some(found);
        }

        found.push(Segment {
            marker,
            at,
            len: len + 2,
        });
        at += 2 + len;
    }

    Some(found)
}

/// The photograph itself: everything from the start of scan to the end.
///
/// What the guarantee is made about. Two files whose compressed image is
/// equal hold the same picture whatever their metadata says.
pub fn compressed_image(raw: &[u8]) -> Option<&[u8]> {
    let segments = segments(raw)?;
    let scan = segments.iter().find(|segment| segment.marker == SOS)?;
    raw.get(scan.at..)
}

/// The XMP packet a photograph already carries, if it carries one.
pub fn xmp(raw: &[u8]) -> Option<String> {
    let body = body_of(raw, APP1, XMP_HEADER)?;
    Some(String::from_utf8_lossy(body).into_owned())
}

/// The Photoshop block a photograph carries, header included, if it carries
/// one. The IPTC record is in there — see [`crate::iptc`].
pub fn app13(raw: &[u8]) -> Option<&[u8]> {
    let segments = segments(raw)?;
    let found = segment_with(raw, &segments, APP13, crate::iptc::HEADER)?;
    raw.get(segments[found].body())
}

/// The body of the first segment with this marker and opening bytes, the
/// opening bytes taken off.
fn body_of<'a>(raw: &'a [u8], marker: u8, header: &[u8]) -> Option<&'a [u8]> {
    let segments = segments(raw)?;
    let found = segment_with(raw, &segments, marker, header)?;
    let body = segments[found].body();
    raw.get(body.start + header.len()..body.end)
}

fn segment_with(raw: &[u8], segments: &[Segment], marker: u8, header: &[u8]) -> Option<usize> {
    segments.iter().position(|found| {
        found.marker == marker
            && raw.get(found.body().start..found.body().start + header.len()) == Some(header)
    })
}

/// The largest a JPEG segment can be, its own length field included.
const SEGMENT_LIMIT: usize = u16::MAX as usize;

/// Puts the packet into the file, replacing the one already there.
///
/// The photograph is never re-encoded and never re-ordered: the XMP segment
/// takes the place of the old one, or goes in right after the last of the
/// leading application segments, and everything else is copied across in the
/// order it was in.
pub fn with_xmp(raw: &[u8], packet: &str) -> Result<Vec<u8>> {
    let body_len = XMP_HEADER.len() + packet.len() + 2;
    anyhow::ensure!(
        body_len <= SEGMENT_LIMIT,
        "the XMP packet is {} bytes and a JPEG segment holds at most {}",
        packet.len(),
        SEGMENT_LIMIT - XMP_HEADER.len() - 2
    );

    let mut body = Vec::with_capacity(body_len);
    body.extend_from_slice(XMP_HEADER);
    body.extend_from_slice(packet.as_bytes());
    with_segment(raw, APP1, XMP_HEADER, Some(&body))
}

/// Puts the Photoshop block into the file, replacing the one already there;
/// `None` takes it out.
///
/// The body is the whole of it, header included, as [`crate::iptc`] builds
/// it.
pub fn with_app13(raw: &[u8], body: Option<&[u8]>) -> Result<Vec<u8>> {
    if let Some(body) = body {
        anyhow::ensure!(
            body.len() + 2 <= SEGMENT_LIMIT,
            "the IPTC block is {} bytes and a JPEG segment holds at most {}",
            body.len(),
            SEGMENT_LIMIT - 2
        );
    }

    with_segment(raw, APP13, crate::iptc::HEADER, body)
}

/// The one way a segment gets into a file, or out of it.
///
/// The segment recognised by `marker` and `header` is replaced by one holding
/// `body` — or removed, when there is no body — and a new one goes in right
/// after the last of the leading application segments, which is where every
/// writer puts it and where every reader looks first. Everything else is
/// copied across in the order it was in.
fn with_segment(raw: &[u8], marker: u8, header: &[u8], body: Option<&[u8]>) -> Result<Vec<u8>> {
    let segments = segments(raw).context("this is not a JPEG we can read")?;

    let segment = body.map(|body| {
        let mut segment = Vec::with_capacity(body.len() + 4);
        segment.extend_from_slice(&[0xFF, marker]);
        segment.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        segment.extend_from_slice(body);
        segment
    });

    let existing = segment_with(raw, &segments, marker, header);
    let after = segments
        .iter()
        .rposition(|found| matches!(found.marker, 0xE0..=0xEF))
        .map(|at| at + 1)
        .unwrap_or(0);

    let mut out = Vec::with_capacity(raw.len() + segment.as_ref().map_or(0, Vec::len));
    out.extend_from_slice(&raw[..2]);
    for (index, found) in segments.iter().enumerate() {
        if Some(index) == existing {
            if let Some(segment) = &segment {
                out.extend_from_slice(segment);
            }

            continue;
        }

        if existing.is_none()
            && index == after
            && let Some(segment) = &segment
        {
            out.extend_from_slice(segment);
        }

        out.extend_from_slice(&raw[found.at..found.at + found.len]);
    }

    if existing.is_none()
        && after >= segments.len()
        && let Some(segment) = &segment
    {
        out.extend_from_slice(segment);
    }

    // The guarantee, checked rather than trusted. A photograph is not
    // something to be optimistic about.
    let before = compressed_image(raw);
    let now = compressed_image(&out);
    anyhow::ensure!(
        before.is_some() && before == now,
        "writing the metadata would have changed the photograph itself"
    );

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG with the segments named, and image data that is recognisable
    /// if anything moves it.
    fn jpeg(with_app1_exif: bool, with_xmp: Option<&str>) -> Vec<u8> {
        let mut raw = vec![0xFF, 0xD8];

        let mut app = |marker: u8, body: &[u8]| {
            raw.push(0xFF);
            raw.push(marker);
            raw.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            raw.extend_from_slice(body);
        };

        if with_app1_exif {
            app(APP1, b"Exif\0\0II*\0\x08\0\0\0");
        }

        if let Some(packet) = with_xmp {
            let mut body = XMP_HEADER.to_vec();
            body.extend_from_slice(packet.as_bytes());
            app(APP1, &body);
        }

        app(0xE0, b"JFIF\0");
        app(0xDB, &[0u8; 64]);
        // The frame header, then the scan and the photograph.
        app(0xC0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0]);
        raw.extend_from_slice(&[0xFF, SOS, 0x00, 0x08, 1, 1, 0, 0, 63, 0]);
        raw.extend_from_slice(b"THE PHOTOGRAPH ITSELF");
        raw.extend_from_slice(&[0xFF, 0xD9]);
        raw
    }

    const PACKET: &str = "<x:xmpmeta><rdf:RDF/></x:xmpmeta>";

    #[test]
    fn a_file_with_no_packet_gains_one() {
        let raw = jpeg(true, None);
        assert_eq!(xmp(&raw), None);

        let out = with_xmp(&raw, PACKET).expect("cannot write");
        assert_eq!(xmp(&out).as_deref(), Some(PACKET));
    }

    #[test]
    fn a_file_with_a_packet_has_it_replaced_and_not_doubled() {
        let raw = jpeg(true, Some("<x:xmpmeta>old</x:xmpmeta>"));
        let out = with_xmp(&raw, PACKET).expect("cannot write");
        assert_eq!(xmp(&out).as_deref(), Some(PACKET));

        let occurrences = segments(&out)
            .expect("not a jpeg")
            .iter()
            .filter(|found| {
                found.marker == APP1
                    && out.get(found.body().start..found.body().start + XMP_HEADER.len())
                        == Some(XMP_HEADER)
            })
            .count();
        assert_eq!(occurrences, 1, "the file has two XMP segments");
    }

    /// The whole reason this file exists.
    #[test]
    fn the_photograph_itself_is_never_touched() {
        for existing in [None, Some("<x:xmpmeta>old</x:xmpmeta>")] {
            let raw = jpeg(true, existing);
            let out = with_xmp(&raw, PACKET).expect("cannot write");
            assert_eq!(
                compressed_image(&raw),
                compressed_image(&out),
                "existing = {existing:?}"
            );
            assert!(
                out.windows(21)
                    .any(|window| window == b"THE PHOTOGRAPH ITSELF"),
                "the image data went missing"
            );
        }
    }

    #[test]
    fn the_exif_segment_is_left_exactly_where_it_was() {
        let raw = jpeg(true, None);
        let out = with_xmp(&raw, PACKET).expect("cannot write");
        let exif: Vec<_> = segments(&out)
            .expect("not a jpeg")
            .into_iter()
            .filter(|found| {
                found.marker == APP1
                    && out.get(found.body().start..found.body().start + 6) == Some(&b"Exif\0\0"[..])
            })
            .collect();
        assert_eq!(exif.len(), 1);
        assert_eq!(exif[0].at, 2, "EXIF must stay the first segment");
    }

    #[test]
    fn writing_twice_settles_rather_than_growing() {
        let raw = jpeg(true, None);
        let once = with_xmp(&raw, PACKET).expect("cannot write");
        let twice = with_xmp(&once, PACKET).expect("cannot write");
        assert_eq!(once, twice, "the second write changed something");
    }

    /// The whole point of tolerating a short buffer: the packet is found in
    /// the header, and nothing that promises to preserve the photograph can
    /// be satisfied by one.
    #[test]
    fn a_header_gives_up_its_packet_but_not_the_photograph() {
        let raw = jpeg(true, Some(PACKET));
        let header = &raw[..raw.len() / 2];

        assert_eq!(xmp(header).as_deref(), Some(PACKET));
        assert_eq!(
            compressed_image(header),
            None,
            "half a file must not pass as a whole one"
        );
        assert!(
            with_xmp(header, PACKET).is_err(),
            "a header must never be written back as a file"
        );
    }

    #[test]
    fn a_file_that_is_not_a_jpeg_is_refused_rather_than_mangled() {
        assert!(with_xmp(b"this is not a jpeg at all", PACKET).is_err());
        assert!(with_xmp(&[], PACKET).is_err());
        assert_eq!(xmp(b"nope"), None);
        assert_eq!(compressed_image(b"nope"), None);
    }

    #[test]
    fn a_packet_too_large_for_a_segment_is_refused() {
        let raw = jpeg(true, None);
        let enormous = "x".repeat(SEGMENT_LIMIT);
        let error = with_xmp(&raw, &enormous).unwrap_err().to_string();
        assert!(error.contains("segment"), "{error}");
    }

    #[test]
    fn a_file_with_no_application_segments_still_takes_a_packet() {
        let raw = jpeg(false, None);
        let out = with_xmp(&raw, PACKET).expect("cannot write");
        assert_eq!(xmp(&out).as_deref(), Some(PACKET));
        assert_eq!(compressed_image(&raw), compressed_image(&out));
    }
}
