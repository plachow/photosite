//! Pojmy, o kterých je celá aplikace.
//!
//! Slovník se drží [CONTEXT.md](../../../CONTEXT.md) — je odladěný a nemá
//! smysl si vymýšlet nová jména pro tytéž věci.

use std::path::{Path, PathBuf};

/// Číslo, pod kterým fotka žije v katalogu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhotoId(pub i64);

/// Jeden řádek katalogu: identita souboru a to, co se z něj přečetlo.
///
/// Neobsahuje pixely a nikdy je obsahovat nebude.
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    pub id: PhotoId,
    pub path: PathBuf,
    /// Složka, ve které soubor leží. Vlastní sloupec, aby šlo listovat složku
    /// bez procházení všech řádků.
    pub folder: PathBuf,
    pub file_size: u64,
    /// Čas zápisu souboru v sekundách od epochy.
    pub modified_at: i64,
    /// Kdy fotka vznikla, pokud to šlo zjistit.
    pub taken_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// EXIF orientace 1..8.
    pub orientation: u8,
}

/// Co o souboru víme z disku, ještě než ho někdo přečte. Podle téhle trojice
/// se pozná, že se soubor nezměnil a nemusí se číst znovu.
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

    /// Odpovídá tenhle soubor tomu, co je v katalogu?
    pub fn matches(&self, photo: &Photo) -> bool {
        photo.file_size == self.file_size && photo.modified_at == self.modified_at
    }
}

/// Přípony, které bereme jako fotku.
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
    fn pripony_se_poznaji_bez_ohledu_na_velikost_pismen() {
        assert!(is_photo(Path::new("a/b/C.JPG")));
        assert!(is_photo(Path::new("x.jpeg")));
        assert!(!is_photo(Path::new("x.txt")));
        assert!(!is_photo(Path::new("bez_pripony")));
    }
}
