//! IPTC keywords, in the Photoshop block a JPEG carries them in.
//!
//! XMP is where keywords live today, and every cataloguer reads `dc:subject`.
//! But the older world — and a good deal of the current one, Zoner Photo
//! Studio and Windows among it — also reads the IPTC-IIM record that
//! Photoshop put into an APP13 segment thirty years ago, and a file whose
//! two lists disagree shows one program one set of keywords and another
//! program another. So what goes into `dc:subject` goes here too, exactly as
//! v1 had exiftool do, and what is found here is read when the packet is
//! silent.
//!
//! The block is a list of **resources** — `8BIM`, an id, a name, a length
//! and the bytes — of which the IPTC record is the one with id `0x0404`. The
//! record itself is a list of **datasets**: `0x1C`, a record number, a
//! dataset number, a length and the bytes. Keywords are dataset 25 of record
//! 2, one dataset per keyword; the character set is dataset 90 of record 1.
//!
//! **Everything that is not ours is copied through.** Other resources —
//! Photoshop's own, a colour profile reference, a caption written elsewhere
//! — are kept byte for byte, with one deliberate exception: the IPTC digest
//! (`0x0425`), a hash of the record that would no longer be true of what we
//! just wrote, is dropped rather than left to lie.

/// The header that marks an APP13 segment as Photoshop's.
pub const HEADER: &[u8] = b"Photoshop 3.0\0";

const SIGNATURE: &[u8] = b"8BIM";
const IPTC_RESOURCE: u16 = 0x0404;
const IPTC_DIGEST: u16 = 0x0425;

/// The dataset marker every IPTC dataset opens with.
const TAG: u8 = 0x1C;
const ENVELOPE: u8 = 1;
const APPLICATION: u8 = 2;
const CODED_CHARACTER_SET: u8 = 90;
const RECORD_VERSION: u8 = 0;
const KEYWORDS: u8 = 25;

/// What the IIM standard says a keyword may hold. exiftool cuts at the same
/// place; a reader that trusts the length would otherwise read the next
/// dataset's header as the end of this keyword.
const KEYWORD_BYTES: usize = 64;

/// The escape sequence that declares UTF-8.
const UTF8: &[u8] = b"\x1b%G";

/// One Photoshop resource, as found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resource<'a> {
    id: u16,
    name: &'a [u8],
    data: &'a [u8],
}

/// One IPTC dataset, as found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dataset<'a> {
    record: u8,
    number: u8,
    data: &'a [u8],
}

/// The resources of an APP13 body, and whatever follows the last one we
/// could read.
///
/// A resource signed other than `8BIM` has a layout we do not know, so the
/// walk stops there and the rest is carried across as one opaque tail.
fn resources(body: &[u8]) -> (Vec<Resource<'_>>, &[u8]) {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at + 12 <= body.len() {
        if &body[at..at + 4] != SIGNATURE {
            break;
        }

        let id = u16::from_be_bytes([body[at + 4], body[at + 5]]);
        let name_len = body[at + 6] as usize;
        let name_end = at + 7 + name_len;
        // The name is padded to an even length, counting its length byte.
        let after_name = at + 6 + ((1 + name_len + 1) & !1);
        if after_name + 4 > body.len() || name_end > body.len() {
            break;
        }

        let size = u32::from_be_bytes([
            body[after_name],
            body[after_name + 1],
            body[after_name + 2],
            body[after_name + 3],
        ]) as usize;
        let data_at = after_name + 4;
        let Some(data_end) = data_at.checked_add(size) else {
            break;
        };
        if data_end > body.len() {
            break;
        }

        found.push(Resource {
            id,
            name: &body[at + 7..name_end],
            data: &body[data_at..data_end],
        });
        // The data is padded to an even length too.
        at = data_end + (size & 1);
    }

    (found, &body[at.min(body.len())..])
}

fn datasets(record: &[u8]) -> Vec<Dataset<'_>> {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at + 5 <= record.len() {
        if record[at] != TAG {
            break;
        }

        let size = u16::from_be_bytes([record[at + 3], record[at + 4]]) as usize;
        // The high bit marks an extended dataset, whose length is itself of
        // variable length. Nothing we care about is ever that long, and a
        // walk that guesses at the layout reads garbage as keywords.
        if size & 0x8000 != 0 {
            break;
        }

        let data_at = at + 5;
        let Some(data_end) = data_at.checked_add(size) else {
            break;
        };
        if data_end > record.len() {
            break;
        }

        found.push(Dataset {
            record: record[at + 1],
            number: record[at + 2],
            data: &record[data_at..data_end],
        });
        at = data_end;
    }

    found
}

/// Is the record declared to be UTF-8?
fn utf8_declared(datasets: &[Dataset<'_>]) -> bool {
    datasets
        .iter()
        .any(|set| set.record == ENVELOPE && set.number == CODED_CHARACTER_SET && set.data == UTF8)
}

/// The text of a dataset, by the record's declared character set.
///
/// Without a declaration the bytes are read as UTF-8 when they are valid
/// UTF-8 and as Latin-1 otherwise: nearly every undeclared record is one or
/// the other, and a Latin-1 keyword read as UTF-8 would be refused, not
/// misread.
fn text(bytes: &[u8], utf8: bool) -> String {
    if utf8 {
        return String::from_utf8_lossy(bytes).into_owned();
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|byte| char::from(*byte)).collect(),
    }
}

/// The keywords an APP13 body carries.
pub fn keywords(body: &[u8]) -> Vec<String> {
    let Some(body) = body.strip_prefix(HEADER) else {
        return Vec::new();
    };

    let (resources, _) = resources(body);
    let mut found: Vec<String> = Vec::new();
    for resource in resources.iter().filter(|found| found.id == IPTC_RESOURCE) {
        let datasets = datasets(resource.data);
        let utf8 = utf8_declared(&datasets);
        for set in datasets
            .iter()
            .filter(|set| set.record == APPLICATION && set.number == KEYWORDS)
        {
            let keyword = text(set.data, utf8);
            let keyword = keyword.trim();
            if !keyword.is_empty() && !found.iter().any(|had| had.eq_ignore_ascii_case(keyword)) {
                found.push(keyword.to_owned());
            }
        }
    }

    found
}

/// The APP13 body with our keywords in it, or nothing when the block would
/// hold nothing worth a segment.
///
/// `existing` is the body already in the file, if any. Everything in it that
/// is not a keyword is kept; the keywords are replaced wholesale, because
/// appending to the list already there is how keywords accumulate.
pub fn with_keywords(existing: Option<&[u8]>, keywords: &[String]) -> Option<Vec<u8>> {
    let existing = existing.and_then(|body| body.strip_prefix(HEADER));
    let (resources, tail) = existing.map(resources).unwrap_or_default();

    let mut out = Vec::with_capacity(existing.map_or(0, <[u8]>::len) + keywords.len() * 40 + 64);
    out.extend_from_slice(HEADER);

    let mut placed = false;
    for resource in &resources {
        match resource.id {
            IPTC_RESOURCE if !placed => {
                placed = true;
                if let Some(record) = record_with(resource.data, keywords) {
                    resource_into(&mut out, IPTC_RESOURCE, resource.name, &record);
                }
            }
            // A second IPTC resource would be a file already broken; the
            // first is the one every reader takes, and a digest of a record
            // we have just rewritten is no longer true.
            IPTC_RESOURCE | IPTC_DIGEST => {}
            _ => resource_into(&mut out, resource.id, resource.name, resource.data),
        }
    }

    if !placed && let Some(record) = record_with(&[], keywords) {
        resource_into(&mut out, IPTC_RESOURCE, b"", &record);
    }

    out.extend_from_slice(tail);
    (out.len() > HEADER.len()).then_some(out)
}

fn resource_into(out: &mut Vec<u8>, id: u16, name: &[u8], data: &[u8]) {
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&id.to_be_bytes());
    out.push(name.len().min(255) as u8);
    out.extend_from_slice(&name[..name.len().min(255)]);
    if (1 + name.len().min(255)) & 1 == 1 {
        out.push(0);
    }

    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() & 1 == 1 {
        out.push(0);
    }
}

/// The IPTC record with our keywords in it, or nothing when it would be
/// empty.
///
/// The record is declared UTF-8 whatever it was before, and what was in it
/// is carried across in UTF-8 — a record half in Latin-1 and half in UTF-8
/// under one declaration is exactly the mess the declaration exists to
/// prevent.
fn record_with(existing: &[u8], keywords: &[String]) -> Option<Vec<u8>> {
    let datasets = datasets(existing);
    let utf8 = utf8_declared(&datasets);
    let kept: Vec<(u8, u8, Vec<u8>)> = datasets
        .iter()
        .filter(|set| {
            !(set.record == ENVELOPE && set.number == CODED_CHARACTER_SET)
                && !(set.record == APPLICATION && set.number == KEYWORDS)
        })
        .map(|set| {
            let data = if utf8 || set.data.is_ascii() {
                set.data.to_vec()
            } else {
                text(set.data, false).into_bytes()
            };
            (set.record, set.number, data)
        })
        .collect();

    // A record that would hold nothing but its own version number is a
    // record saying nothing, and not worth a segment.
    let substantive = kept
        .iter()
        .any(|(record, number, _)| !(*record == APPLICATION && *number == RECORD_VERSION));
    if keywords.is_empty() && !substantive {
        return None;
    }

    let mut out = Vec::new();
    dataset_into(&mut out, ENVELOPE, CODED_CHARACTER_SET, UTF8);

    // Record 2 opens with its version, and a reader that insists on the
    // order — some do — wants it before the first keyword.
    let has_version = kept
        .iter()
        .any(|(record, number, _)| *record == APPLICATION && *number == RECORD_VERSION);
    let mut placed = keywords.is_empty();
    for (record, number, data) in &kept {
        if !placed && *record == APPLICATION && *number != RECORD_VERSION {
            placed = true;
            keywords_into(&mut out, keywords);
        }

        dataset_into(&mut out, *record, *number, data);
        if !placed && *record == APPLICATION && *number == RECORD_VERSION {
            placed = true;
            keywords_into(&mut out, keywords);
        }
    }

    if !placed {
        if !has_version {
            dataset_into(&mut out, APPLICATION, RECORD_VERSION, &[0, 4]);
        }

        keywords_into(&mut out, keywords);
    }

    Some(out)
}

fn keywords_into(out: &mut Vec<u8>, keywords: &[String]) {
    for keyword in keywords {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            continue;
        }

        // Cut on a character boundary, never inside one.
        let mut end = keyword.len().min(KEYWORD_BYTES);
        while !keyword.is_char_boundary(end) {
            end -= 1;
        }

        dataset_into(out, APPLICATION, KEYWORDS, &keyword.as_bytes()[..end]);
    }
}

fn dataset_into(out: &mut Vec<u8>, record: u8, number: u8, data: &[u8]) {
    out.push(TAG);
    out.push(record);
    out.push(number);
    out.extend_from_slice(&(data.len().min(0x7FFF) as u16).to_be_bytes());
    out.extend_from_slice(&data[..data.len().min(0x7FFF)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn keywords_written_are_keywords_read() {
        let body = with_keywords(None, &words(&["sea", "Šumava"])).unwrap();
        assert_eq!(keywords(&body), words(&["sea", "Šumava"]));
    }

    #[test]
    fn nothing_to_say_is_no_block_at_all() {
        assert_eq!(with_keywords(None, &[]), None);
    }

    /// Photoshop's own resources — here a made-up one and the digest — must
    /// come through a write, the digest excepted.
    #[test]
    fn other_resources_are_kept_and_the_stale_digest_is_not() {
        let mut body = HEADER.to_vec();
        resource_into(&mut body, 0x040F, b"", b"an ICC profile, say");
        resource_into(&mut body, IPTC_DIGEST, b"", &[7u8; 16]);
        let out = with_keywords(Some(&body), &words(&["one"])).unwrap();
        let (resources, tail) = resources(out.strip_prefix(HEADER).unwrap());
        assert!(tail.is_empty());
        let ids: Vec<u16> = resources.iter().map(|found| found.id).collect();
        assert_eq!(ids, vec![0x040F, IPTC_RESOURCE]);
        assert_eq!(resources[0].data, b"an ICC profile, say");
    }

    /// A caption written by another program stays; only the keywords are
    /// ours to replace.
    #[test]
    fn writing_replaces_the_keywords_and_keeps_the_rest_of_the_record() {
        let first = with_keywords(None, &words(&["old"])).unwrap();
        // Add a caption (2:120) by hand to the record.
        let mut record = Vec::new();
        dataset_into(&mut record, APPLICATION, RECORD_VERSION, &[0, 4]);
        dataset_into(&mut record, APPLICATION, KEYWORDS, b"old");
        dataset_into(&mut record, APPLICATION, 120, b"A caption");
        let mut body = HEADER.to_vec();
        resource_into(&mut body, IPTC_RESOURCE, b"", &record);
        let _ = first;

        let out = with_keywords(Some(&body), &words(&["new"])).unwrap();
        assert_eq!(keywords(&out), words(&["new"]));
        let (resources, _) = resources(out.strip_prefix(HEADER).unwrap());
        let sets = datasets(resources[0].data);
        let numbers: Vec<(u8, u8)> = sets.iter().map(|set| (set.record, set.number)).collect();
        assert_eq!(
            numbers,
            vec![
                (ENVELOPE, CODED_CHARACTER_SET),
                (APPLICATION, RECORD_VERSION),
                (APPLICATION, KEYWORDS),
                (APPLICATION, 120)
            ]
        );
        assert_eq!(sets[3].data, b"A caption");
    }

    #[test]
    fn writing_twice_settles_rather_than_growing() {
        let once = with_keywords(None, &words(&["a", "b"])).unwrap();
        let twice = with_keywords(Some(&once), &words(&["a", "b"])).unwrap();
        assert_eq!(once, twice);
    }

    /// A record with no declaration and a Latin-1 keyword is read as
    /// Latin-1, and comes back out as UTF-8 under a declaration.
    #[test]
    fn an_undeclared_latin_1_record_is_read_and_carried_across_as_utf8() {
        let mut record = Vec::new();
        dataset_into(&mut record, APPLICATION, 120, b"caf\xe9");
        dataset_into(&mut record, APPLICATION, KEYWORDS, b"\xe9t\xe9");
        let mut body = HEADER.to_vec();
        resource_into(&mut body, IPTC_RESOURCE, b"", &record);
        assert_eq!(keywords(&body), words(&["été"]));

        let out = with_keywords(Some(&body), &words(&["hiver"])).unwrap();
        let (resources, _) = resources(out.strip_prefix(HEADER).unwrap());
        let sets = datasets(resources[0].data);
        assert!(utf8_declared(&sets));
        let caption = sets.iter().find(|set| set.number == 120).unwrap();
        assert_eq!(caption.data, "café".as_bytes());
    }

    #[test]
    fn a_keyword_longer_than_the_standard_allows_is_cut_on_a_character() {
        let long = "š".repeat(40); // 80 bytes
        let body = with_keywords(None, &[long]).unwrap();
        let read = keywords(&body);
        assert_eq!(read[0].chars().count(), 32);
    }

    #[test]
    fn clearing_the_keywords_of_a_record_holding_nothing_else_removes_it() {
        let body = with_keywords(None, &words(&["gone"])).unwrap();
        assert_eq!(with_keywords(Some(&body), &[]), None);
    }

    #[test]
    fn a_broken_block_gives_up_rather_than_panicking() {
        let mut body = HEADER.to_vec();
        body.extend_from_slice(b"8BIM\x04\x04\x00\x00\xff\xff\xff\xff");
        assert!(keywords(&body).is_empty());
        let _ = with_keywords(Some(&body), &words(&["x"]));
    }
}
