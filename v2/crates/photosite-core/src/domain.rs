//! The words the whole application is about.
//!
//! The vocabulary follows [CONTEXT.md](../../../CONTEXT.md) — it is settled,
//! and inventing new names for the same things buys nothing.

use std::path::{Path, PathBuf};

/// The number a photograph lives under in the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhotoId(pub i64);

/// One catalogue row: the file's identity and what was read out of it.
///
/// Holds no pixels and never will.
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    pub id: PhotoId,
    pub path: PathBuf,
    /// The folder the file sits in. Its own column, so that listing a folder
    /// does not mean walking every row.
    pub folder: PathBuf,
    pub file_size: u64,
    /// The file's write time, in seconds since the epoch.
    pub modified_at: i64,
    /// When the photograph was taken, where that could be established.
    pub taken_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// EXIF orientation, 1..8.
    pub orientation: u8,
}

/// What the disk tells us about a file before anyone reads it. These three
/// values are how we know a file has not changed and need not be read again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    pub path: PathBuf,
    pub file_size: u64,
    pub modified_at: i64,
}

impl FileIdentity {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        let modified_at = meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Ok(Self {
            path: path.to_path_buf(),
            file_size: meta.len(),
            modified_at,
        })
    }

    /// Does this file still match what the catalogue holds?
    pub fn matches(&self, photo: &Photo) -> bool {
        photo.file_size == self.file_size && photo.modified_at == self.modified_at
    }
}

/// Extensions we treat as a photograph.
pub const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tif", "tiff", "gif"];

pub fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let lower = extension.to_ascii_lowercase();
            PHOTO_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_recognised_whatever_the_case() {
        assert!(is_photo(Path::new("a/b/C.JPG")));
        assert!(is_photo(Path::new("x.jpeg")));
        assert!(!is_photo(Path::new("x.txt")));
        assert!(!is_photo(Path::new("no_extension")));
    }
}
