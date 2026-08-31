//! Thumbnails that survive a restart.
//!
//! Decoding a folder's tiles is a tenth of a second per photograph for the
//! eight-megabyte files a full-frame camera writes. Doing it again on every
//! start, for a library nobody has changed, is work that was already done.
//!
//! Measured over three hundred of them: **28.6 s the first time and 0.23 s
//! the second**, for 6.9 MB of disk — twenty-three kilobytes a tile. A whole
//! library browsed through would be a few gigabytes of cache, which is why
//! `loading.cache_thumbnails` exists to turn it off.
//!
//! So a finished tile is written beside the catalogue as a small JPEG, and
//! looked for before anything is decoded. Nothing here is on the drawing
//! thread: this runs where the decoding already ran.
//!
//! **A cached tile is never stale.** The name carries the file's length and
//! its write time along with its path, so a photograph that changed asks for
//! a name that has never been written and gets decoded. The old entry is not
//! deleted, it simply stops being asked for — which is what makes this safe
//! without a single invalidation rule to get wrong.

use crate::decode::{self, Rgb};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// What a tile is written at. High enough that a thumbnail does not look
/// worse than the photograph it stands for, low enough that a library's
/// worth of them is megabytes and not gigabytes.
const QUALITY: u8 = 88;

#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The tile for a photograph, decoded only if it has not been before.
    ///
    /// Every failure here is a miss and never an error: a cache that can
    /// break the thing it is speeding up is worse than no cache. An
    /// unwritable directory, a half-written file, a disk that filled up —
    /// all of them end in the photograph being decoded, which is what would
    /// have happened anyway.
    pub fn thumb(&self, path: &Path, max: u32) -> Result<Rgb> {
        let at = self.entry(path, max);
        if let Some(at) = &at
            && let Some(found) = read(at)
        {
            return Ok(found);
        }

        let decoded = decode::sized(path, max)?;
        if let Some(at) = &at
            && let Err(error) = write(at, &decoded)
        {
            tracing::debug!(path = %at.display(), %error, "the tile could not be kept");
        }

        Ok(decoded)
    }

    /// Where a photograph's tile lives, or nothing when it cannot be worked
    /// out.
    ///
    /// Two hex characters of the name become a folder. A hundred thousand
    /// files in one directory is a directory every tool on the machine
    /// struggles with, this one included.
    fn entry(&self, path: &Path, max: u32) -> Option<PathBuf> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let name = name_for(path, max, meta.len(), modified);
        Some(self.root.join(&name[..2]).join(format!("{name}.jpg")))
    }
}

/// The name of a tile: the path, the size asked for, and what the file was
/// when it was written.
fn name_for(path: &Path, max: u32, length: u64, modified: u64) -> String {
    let mut key = path.to_string_lossy().into_owned();
    // A separator that cannot appear in any of the parts, so two different
    // keys cannot spell the same string.
    key.push('\u{0}');
    key.push_str(&format!("{max}\u{0}{length}\u{0}{modified}"));

    // Two passes with different starts, for a hundred and twenty-eight bits.
    // Not a cryptographic hash and it does not need to be — but it does need
    // to be the same next week, which is why it is written out here rather
    // than taken from the standard library, whose hasher makes no such
    // promise across releases.
    format!(
        "{:016x}{:016x}",
        fnv(key.as_bytes(), 0),
        fnv(key.as_bytes(), 1)
    )
}

/// FNV-1a, with an offset so the same bytes can be hashed twice differently.
fn fnv(bytes: &[u8], salt: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }

    hash
}

fn read(at: &Path) -> Option<Rgb> {
    let raw = std::fs::read(at).ok()?;
    decode::jpeg_rgb(&raw, None)
}

/// Written beside and moved into place, for the same reason a photograph is:
/// a tile half-written by a crash would be read as a tile ever after.
fn write(at: &Path, tile: &Rgb) -> Result<()> {
    let parent = at.parent().context("a tile with no folder")?;
    std::fs::create_dir_all(parent)?;

    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, QUALITY).encode(
        &tile.pixels,
        tile.width,
        tile.height,
        image::ExtendedColorType::Rgb8,
    )?;

    let temporary = at.with_extension("part");
    std::fs::write(&temporary, &encoded)?;
    match std::fs::rename(&temporary, at) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_photograph(dir: &Path, name: &str) -> PathBuf {
        // A real, if very small, JPEG: the cache decodes what it is given.
        let mut pixels = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut pixels)
            .encode(
                &[200u8; 16 * 12 * 3],
                16,
                12,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let path = dir.join(name);
        std::fs::write(&path, pixels).unwrap();
        path
    }

    #[test]
    fn the_second_time_it_comes_off_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("cache"));
        let path = a_photograph(dir.path(), "a.jpg");

        let first = cache.thumb(&path, 16).unwrap();
        let kept: Vec<_> = walk(&dir.path().join("cache"));
        assert_eq!(kept.len(), 1, "nothing was kept");

        // Fill the photograph with rubbish, keeping its length and its write
        // time. It is now undecodable, so a tile can only come from the
        // cache — and the cache has no way of telling that anything changed,
        // which is exactly the state being tested.
        let was = std::fs::metadata(&path).unwrap();
        let length = was.len() as usize;
        std::fs::write(&path, vec![0x7Fu8; length]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(was.modified().unwrap())
            .unwrap();
        assert!(
            decode::sized(&path, 16).is_err(),
            "the fixture still decodes"
        );

        let second = cache.thumb(&path, 16).expect("the tile was not read back");
        assert_eq!((first.width, first.height), (second.width, second.height));
    }

    /// The rule the whole design rests on: a photograph that changed must
    /// not be shown as it was.
    #[test]
    fn a_changed_photograph_asks_for_a_name_that_was_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, b"x").unwrap();

        let before = name_for(&path, 320, 100, 1_000);
        assert_ne!(before, name_for(&path, 320, 101, 1_000), "length ignored");
        assert_ne!(
            before,
            name_for(&path, 320, 100, 1_001),
            "write time ignored"
        );
        assert_ne!(before, name_for(&path, 640, 100, 1_000), "size ignored");
        assert_ne!(
            before,
            name_for(&dir.path().join("b.jpg"), 320, 100, 1_000),
            "the path is ignored"
        );

        // And the same file twice is the same name, or the cache never hits.
        assert_eq!(before, name_for(&path, 320, 100, 1_000));
    }

    /// A cache that can break the thing it speeds up is worse than none.
    #[test]
    fn a_cache_that_cannot_be_written_is_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = a_photograph(dir.path(), "a.jpg");

        // A file where the folder should be, so every write fails.
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"not a folder").unwrap();

        let cache = Cache::new(blocked);
        assert!(cache.thumb(&path, 16).is_ok(), "a bad cache lost the tile");
    }

    #[test]
    fn a_photograph_that_is_not_there_is_still_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("cache"));
        assert!(cache.thumb(&dir.path().join("nothing.jpg"), 16).is_err());
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return found;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            } else {
                found.push(path);
            }
        }

        found
    }
}
