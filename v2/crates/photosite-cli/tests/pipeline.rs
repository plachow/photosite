//! Integrační test celé cesty bez okna a bez GPU.
//!
//! Tohle je ten důvod, proč CLI existuje: v CI na Linuxu ani na macOS není
//! obrazovka, ale sken, katalog i migrace se otestovat musí.

use photosite_core::catalog::NewPhoto;
use photosite_core::{Catalog, Paths, domain};
use std::path::Path;

/// Nejmenší platný JPEG, na kterém jde zkoušet čtení hlavičky.
const MALY_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9,
];

fn strom(root: &Path) {
    std::fs::create_dir_all(root.join("2024/leden")).unwrap();
    std::fs::write(root.join("a.jpg"), MALY_JPEG).unwrap();
    std::fs::write(root.join("2024/b.jpg"), MALY_JPEG).unwrap();
    std::fs::write(root.join("2024/leden/c.JPG"), MALY_JPEG).unwrap();
    std::fs::write(root.join("2024/poznamky.txt"), b"tohle neni fotka").unwrap();
    // Soubor, který tvrdí, že je JPEG, a není. V reálné knihovně o 57 606
    // fotkách byly takové tři.
    std::fs::write(root.join("2024/lzivy.jpg"), b"rozhodne ne jpeg").unwrap();
}

fn nasbirej(root: &Path, recursive: bool) -> Vec<std::path::PathBuf> {
    let mut found: Vec<_> = walkdir::WalkDir::new(root)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| domain::is_photo(path))
        .collect();
    found.sort();
    found
}

fn zapis(catalog: &mut Catalog, files: &[std::path::PathBuf]) {
    let batch: Vec<NewPhoto> = files
        .iter()
        .filter_map(|path| domain::FileIdentity::read(path).ok())
        .map(|identity| NewPhoto {
            path: identity.path,
            file_size: identity.file_size,
            modified_at: identity.modified_at,
            taken_at: None,
            width: None,
            height: None,
            orientation: 1,
        })
        .collect();
    catalog.upsert_many(&batch).unwrap();
}

#[test]
fn sken_najde_fotky_a_vynecha_ostatni() {
    let dir = tempfile::tempdir().unwrap();
    strom(dir.path());

    assert_eq!(
        nasbirej(dir.path(), false).len(),
        1,
        "bez rekurze jen kořen"
    );
    let vsechny = nasbirej(dir.path(), true);
    assert_eq!(vsechny.len(), 4, "tři pravé JPEGy a jeden lživý, .txt ne");
}

#[test]
fn sken_je_inkrementalni_a_idempotentni() {
    let dir = tempfile::tempdir().unwrap();
    strom(dir.path());
    let data = tempfile::tempdir().unwrap();
    let paths = Paths::portable(data.path());
    paths.ensure().unwrap();

    let files = nasbirej(dir.path(), true);
    let mut catalog = Catalog::open(&paths.catalog()).unwrap();
    zapis(&mut catalog, &files);
    assert_eq!(catalog.count().unwrap(), 4);

    // Druhý průchod nesmí přidat nic a všechno musí poznat jako beze změny.
    let identities: Vec<_> = files
        .iter()
        .map(|path| domain::FileIdentity::read(path).unwrap())
        .collect();
    let known = catalog.unchanged(&identities).unwrap();
    assert_eq!(known.len(), 4, "nic se nezměnilo, takže je všechno známé");

    zapis(&mut catalog, &files);
    assert_eq!(
        catalog.count().unwrap(),
        4,
        "opakovaný sken nezakládá duplicity"
    );
}

#[test]
fn zmeneny_soubor_se_pozna() {
    let dir = tempfile::tempdir().unwrap();
    strom(dir.path());
    let data = tempfile::tempdir().unwrap();
    let paths = Paths::portable(data.path());
    paths.ensure().unwrap();

    let files = nasbirej(dir.path(), true);
    let mut catalog = Catalog::open(&paths.catalog()).unwrap();
    zapis(&mut catalog, &files);

    let zmeneny = dir.path().join("a.jpg");
    std::fs::write(&zmeneny, [MALY_JPEG, MALY_JPEG].concat()).unwrap();
    let identity = domain::FileIdentity::read(&zmeneny).unwrap();
    assert!(
        !catalog.is_current(&identity).unwrap(),
        "delší soubor musí být poznat"
    );
}

#[test]
fn lzivy_jpeg_sken_nezastavi() {
    let dir = tempfile::tempdir().unwrap();
    strom(dir.path());
    let lzivy = dir.path().join("2024/lzivy.jpg");

    // Do katalogu se dostane — je to soubor s příponou fotky. Jen z něj nic
    // nepřečteme, a to nesmí nikoho položit.
    let meta = photosite_image::exif::read(&std::fs::read(&lzivy).unwrap());
    assert_eq!(meta.orientation, 1);
    assert!(photosite_image::quick(&lzivy).unwrap().is_none());
    assert!(
        photosite_image::sized(&lzivy, 320).is_err(),
        "chyba se má ohlásit, ne spolknout"
    );
}

#[test]
fn prebiti_cest_nesahne_na_systemove_umisteni() {
    let data = tempfile::tempdir().unwrap();
    let paths = Paths::portable(data.path());
    paths.ensure().unwrap();
    let catalog = Catalog::open(&paths.catalog()).unwrap();
    drop(catalog);

    assert!(
        paths.catalog().starts_with(data.path()),
        "katalog musí být pod přebitým kořenem"
    );
    assert!(paths.catalog().exists());
}
