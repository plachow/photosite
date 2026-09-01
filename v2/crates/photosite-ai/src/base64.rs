//! Base64, encoding only.
//!
//! Fifteen lines with a settled specification and an official set of test
//! vectors, used in exactly one place: a JPEG has to reach a JSON body
//! somehow. A dependency for this would be a dependency to audit, update and
//! explain, and there is nothing here to get wrong that the vectors below do
//! not catch.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Bytes as standard base64, padded.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let block = (u32::from(chunk[0]) << 16)
            | (chunk.get(1).map_or(0, |b| u32::from(*b)) << 8)
            | chunk.get(2).map_or(0, |b| u32::from(*b));
        for at in 0..4 {
            // A block of one byte carries two characters of information, a
            // block of two carries three; the rest is padding rather than a
            // letter, or the decoder reads bytes that were never there.
            if at <= chunk.len() {
                let index = (block >> (18 - at * 6)) & 0x3F;
                out.push(char::from(ALPHABET[index as usize]));
            } else {
                out.push('=');
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors from RFC 4648, which is what makes this fifteen lines
    /// rather than fifteen lines and a hope.
    #[test]
    fn the_official_vectors_come_out_right() {
        for (from, to) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(from.as_bytes()), to, "{from:?}");
        }
    }

    /// A JPEG is bytes, not text, and every one of the 256 has to survive.
    #[test]
    fn every_byte_there_is_survives() {
        let all: Vec<u8> = (0..=255u8).collect();
        let encoded = encode(&all);
        assert_eq!(encoded.len(), 344);
        assert!(
            encoded.ends_with("/w=="),
            "{}",
            &encoded[encoded.len() - 8..]
        );
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
            "something outside the alphabet came out"
        );
    }

    #[test]
    fn the_length_is_always_a_multiple_of_four() {
        for length in 0..40 {
            let bytes = vec![7u8; length];
            assert_eq!(encode(&bytes).len() % 4, 0, "{length} bytes");
        }
    }
}
