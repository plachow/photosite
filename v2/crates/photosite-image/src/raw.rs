//! Showing a RAW file, and nothing more.
//!
//! **No demosaicing, no white balance, no colour science.** Every camera puts
//! a finished JPEG inside the RAW file it writes — that is what the back of
//! the camera shows you — and that is what gets drawn here. It is the
//! photographer's own camera's rendering, which is a better answer than a
//! half-built pipeline of ours would give, and it costs a file read rather
//! than a decoder per manufacturer.
//!
//! What that buys: a folder straight off a card is never a grid of grey
//! tiles. What it does not buy: editing a RAW. That is a different feature
//! and it would need `rawler` and a colour pipeline, and it is deliberately
//! not here.
//!
//! Nearly every RAW format is TIFF underneath — the file *is* the TIFF block,
//! where a JPEG merely carries one in a segment — so the same reader serves
//! both. The formats that are not (Canon's CR3, which is ISO base media, and
//! Fuji's RAF) are not handled, and say so rather than guessing.
//!
//! Nothing here looks at a file's name. Whether a file is a RAW is a question
//! its first two bytes answer, and a list of extensions kept in two places is
//! a list that comes apart. The one list of what counts as a photograph is in
//! the core, where the vocabulary lives.

use crate::exif::TiffReader;
use std::ops::Range;

/// Where the largest embedded JPEG is.
///
/// Largest and not first: a RAW carries several, from a 160x120 thumbnail up
/// to something near the full frame, and the small ones are listed first as
/// often as not. Drawing a 160-pixel thumbnail into a 400-pixel tile is the
/// difference between a photograph and a smear.
pub fn preview(raw: &[u8]) -> Option<Range<usize>> {
    let reader = TiffReader::new(raw)?;
    let mut best: Option<Range<usize>> = None;

    for ifd in reader.every_ifd() {
        for (offset, length) in candidates(&reader, ifd) {
            let Some(end) = offset.checked_add(length) else {
                continue;
            };

            // It has to actually be a JPEG. A tag pair that points at raw
            // sensor data would otherwise be handed to the decoder.
            if end > raw.len() || length < 4 || raw.get(offset..offset + 2) != Some(&[0xFF, 0xD8]) {
                continue;
            }

            if best.as_ref().map(|found| found.len()).unwrap_or(0) < length {
                best = Some(offset..end);
            }
        }
    }

    best
}

/// Every place in one IFD that might name an embedded JPEG.
fn candidates(reader: &TiffReader<'_>, ifd: usize) -> Vec<(usize, usize)> {
    let mut found = Vec::new();

    // The ordinary pair, used by Nikon, Canon, Sony and DNG.
    if let (Some(offset), Some(length)) = (reader.long(ifd, 0x0201), reader.long(ifd, 0x0202)) {
        found.push((offset as usize, length as usize));
    }

    // Panasonic keeps its preview in a tag of its own, and nowhere else.
    if let Some(entry) = reader.entry_for(ifd, 0x002E)
        && let (Some(offset), Some(length)) = (reader.value_offset(entry), reader.count(entry))
    {
        found.push((offset as usize, length as usize));
    }

    // Some DNGs store the preview as an ordinary image with JPEG
    // compression: 6 is the old form, 7 the current one.
    if matches!(reader.short(ifd, 0x0103), Some(6 | 7))
        && let (Some(offset), Some(length)) = (reader.long(ifd, 0x0111), reader.long(ifd, 0x0117))
    {
        found.push((offset as usize, length as usize));
    }

    found
}

/// The size of the frame, from the largest image any of the IFDs describes.
///
/// IFD0 of a Nikon file describes its 160x120 thumbnail, so taking the first
/// answer gives a thumbnail's dimensions for a twenty-four megapixel
/// photograph. The largest is the one somebody means.
pub fn frame(raw: &[u8]) -> Option<(u32, u32)> {
    let reader = TiffReader::new(raw)?;
    let mut best: Option<(u32, u32)> = None;
    for ifd in reader.every_ifd() {
        // The ordinary tags first, then Panasonic's, which uses neither:
        // 0x0007 is the width and 0x0006 the height, in that order round.
        // Without this an RW2 has no size at all, and sorting a folder by
        // dimensions quietly leaves them out.
        let (Some(width), Some(height)) = reader
            .dimension(ifd, 0x0100)
            .zip(reader.dimension(ifd, 0x0101))
            .or_else(|| {
                reader
                    .dimension(ifd, 0x0007)
                    .zip(reader.dimension(ifd, 0x0006))
            })
            .unzip()
        else {
            continue;
        };

        if width == 0 || height == 0 {
            continue;
        }

        let area = width as u64 * height as u64;
        if best
            .map(|(w, h)| (w as u64 * h as u64) < area)
            .unwrap_or(true)
        {
            best = Some((width, height));
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How a preview is named. The two forms that occur in the wild.
    enum As {
        /// An offset under one tag and a length under the next, as Nikon,
        /// Canon and DNG write it.
        Pair,
        /// One undefined blob whose count *is* the byte length, as Panasonic
        /// writes it under 0x002E.
        Blob,
    }

    /// A little-endian TIFF with one IFD naming the given previews.
    fn tiff_with_preview(previews: &[(u16, As, Vec<u8>)]) -> Vec<u8> {
        let entries: u16 = previews
            .iter()
            .map(|(_, form, _)| match form {
                As::Pair => 2u16,
                As::Blob => 1,
            })
            .sum();
        let ifd_at = 8usize;
        let data_at = ifd_at + 2 + entries as usize * 12 + 4;

        let mut data = Vec::new();
        let mut placed = Vec::new();
        for (tag, form, bytes) in previews {
            let offset = (data_at + data.len()) as u32;
            data.extend_from_slice(bytes);
            placed.push((*tag, form, offset, bytes.len() as u32));
        }

        let mut raw = Vec::new();
        raw.extend_from_slice(b"II");
        raw.extend_from_slice(&42u16.to_le_bytes());
        raw.extend_from_slice(&(ifd_at as u32).to_le_bytes());
        raw.extend_from_slice(&entries.to_le_bytes());

        let entry = |tag: u16, kind: u16, count: u32, value: u32, raw: &mut Vec<u8>| {
            raw.extend_from_slice(&tag.to_le_bytes());
            raw.extend_from_slice(&kind.to_le_bytes());
            raw.extend_from_slice(&count.to_le_bytes());
            raw.extend_from_slice(&value.to_le_bytes());
        };

        for (tag, form, offset, length) in &placed {
            match form {
                As::Pair => {
                    entry(*tag, 4, 1, *offset, &mut raw);
                    entry(tag + 1, 4, 1, *length, &mut raw);
                }
                // Type 7 is UNDEFINED, and its count is a count of bytes.
                As::Blob => entry(*tag, 7, *length, *offset, &mut raw),
            }
        }

        raw.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            raw.len(),
            data_at,
            "the data does not start where the entries say"
        );
        raw.extend_from_slice(&data);
        raw
    }

    fn jpeg(size: usize) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.resize(size, 0x41);
        bytes
    }

    #[test]
    fn the_largest_preview_wins_and_not_the_first() {
        // A thumbnail listed before the real preview, which is what a Nikon
        // file looks like.
        let raw = tiff_with_preview(&[(0x0201, As::Pair, jpeg(64))]);
        let found = preview(&raw).expect("no preview");
        assert_eq!(found.len(), 64);

        let raw =
            tiff_with_preview(&[(0x0201, As::Pair, jpeg(64)), (0x002E, As::Blob, jpeg(4096))]);
        let found = preview(&raw).expect("no preview");
        assert_eq!(
            found.len(),
            4096,
            "the thumbnail was taken over the preview"
        );
        assert_eq!(raw[found.start..found.start + 2], [0xFF, 0xD8]);
    }

    #[test]
    fn something_that_is_not_a_jpeg_is_not_offered_as_one() {
        // The tags are there and point somewhere real, but at sensor data.
        let mut sensor = vec![0u8; 4096];
        sensor[0] = 0x12;
        let raw = tiff_with_preview(&[(0x0201, As::Pair, sensor)]);
        assert_eq!(preview(&raw), None);
    }

    #[test]
    fn a_tag_pointing_past_the_end_does_not_panic() {
        let mut raw = tiff_with_preview(&[(0x0201, As::Pair, jpeg(64))]);
        raw.truncate(raw.len() - 32);
        // Either it finds nothing or it finds something inside the buffer;
        // what it must not do is reach past it.
        if let Some(found) = preview(&raw) {
            assert!(found.end <= raw.len());
        }
    }

    #[test]
    fn anything_that_is_not_a_tiff_gives_nothing() {
        assert_eq!(preview(b""), None);
        assert_eq!(preview(b"this is not a tiff"), None);
        assert_eq!(frame(b"nor is this"), None);
    }
}
