//! How the parsers hold up against rubbish.
//!
//! A real library is full of files that are not what their extension claims —
//! in a trial set of 57,606 photographs there were three. This test does not
//! check that anything is read correctly. It checks the one thing that
//! matters: **that it does not panic.**

use photosite_image::exif;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Arbitrary bytes. The commonest real case: the file is something else
    /// entirely.
    #[test]
    fn anything_at_all_leaves_the_reader_standing(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = exif::read(&bytes);
    }

    /// Bytes that start out looking like a JPEG. The parser gets further in
    /// here, which makes this the more interesting of the two.
    #[test]
    fn pretends_to_be_a_jpeg(rest in prop::collection::vec(any::<u8>(), 0..4096)) {
        let mut raw = vec![0xFF_u8, 0xD8, 0xFF, 0xE1];
        raw.extend_from_slice(&rest);
        let _ = exif::read(&raw);
    }

    /// A damaged APP1: correct header, random body. The parser walks all the
    /// way in here and reads offsets out of data it must not trust.
    #[test]
    fn damaged_app1(len in any::<u16>(), body in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut raw = vec![0xFF_u8, 0xD8, 0xFF, 0xE1];
        raw.extend_from_slice(&len.to_be_bytes());
        raw.extend_from_slice(b"Exif\x00\x00");
        raw.extend_from_slice(&body);
        let meta = exif::read(&raw);
        // When the parser reports a thumbnail, it must lie inside the file —
        // otherwise the caller panics on it instead of the parser.
        if let Some(thumbnail) = meta.thumbnail {
            prop_assert!(thumbnail.offset + thumbnail.len <= raw.len());
        }

        prop_assert!((1..=8).contains(&meta.orientation));
    }
}
