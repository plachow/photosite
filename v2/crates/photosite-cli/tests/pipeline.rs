//! An integration test of the whole path, with no window and no GPU.
//!
//! This is the reason the CLI exists: there is no screen in CI on Linux or on
//! macOS, but the scan, the catalogue and the migrations still have to be
//! tested.

use photosite_core::catalog::NewPhoto;
use photosite_core::{Catalog, Paths, domain};
use std::path::Path;

/// The smallest valid JPEG that header reading can be tried on.
const TINY_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9,
];

fn tree(root: &Path) {
    std::fs::create_dir_all(root.join("2024/january")).unwrap();
    std::fs::write(root.join("a.jpg"), TINY_JPEG).unwrap();
    std::fs::write(root.join("2024/b.jpg"), TINY_JPEG).unwrap();
    std::fs::write(root.join("2024/january/c.JPG"), TINY_JPEG).unwrap();
    std::fs::write(root.join("2024/notes.txt"), b"this is not a photograph").unwrap();
    // A file that claims to be a JPEG and is not. In a real library of
    // 57,606 photographs there were three of them.
    std::fs::write(root.join("2024/liar.jpg"), b"certainly not a jpeg").unwrap();
}

fn gather(root: &Path, recursive: bool) -> Vec<std::path::PathBuf> {
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

fn write(catalog: &mut Catalog, files: &[std::path::PathBuf]) {
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
fn a_scan_finds_photographs_and_leaves_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());

    assert_eq!(
        gather(dir.path(), false).len(),
        1,
        "without recursion, the root only"
    );
    let all = gather(dir.path(), true);
    assert_eq!(all.len(), 4, "three real JPEGs and one liar, no .txt");
}

#[test]
fn a_scan_is_incremental_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let data = tempfile::tempdir().unwrap();
    let paths = Paths::portable(data.path());
    paths.ensure().unwrap();

    let files = gather(dir.path(), true);
    let mut catalog = Catalog::open(&paths.catalog()).unwrap();
    write(&mut catalog, &files);
    assert_eq!(catalog.count().unwrap(), 4);

    // A second pass must add nothing and recognise everything as unchanged.
    let identities: Vec<_> = files
        .iter()
        .map(|path| domain::FileIdentity::read(path).unwrap())
        .collect();
    let known = catalog.unchanged(&identities).unwrap();
    assert_eq!(known.len(), 4, "nothing changed, so everything is known");

    write(&mut catalog, &files);
    assert_eq!(
        catalog.count().unwrap(),
        4,
        "a repeated scan creates no duplicates"
    );
}

#[test]
fn a_changed_file_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let data = tempfile::tempdir().unwrap();
    let paths = Paths::portable(data.path());
    paths.ensure().unwrap();

    let files = gather(dir.path(), true);
    let mut catalog = Catalog::open(&paths.catalog()).unwrap();
    write(&mut catalog, &files);

    let zmeneny = dir.path().join("a.jpg");
    std::fs::write(&zmeneny, [TINY_JPEG, TINY_JPEG].concat()).unwrap();
    let identity = domain::FileIdentity::read(&zmeneny).unwrap();
    assert!(
        !catalog.is_current(&identity).unwrap(),
        "a longer file has to be noticed"
    );
}

#[test]
fn a_lying_jpeg_does_not_stop_the_scan() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let liar = dir.path().join("2024/liar.jpg");

    // It gets into the catalogue — it is a file with a photograph's
    // extension. We simply read nothing out of it, and that must not floor
    // anybody.
    let meta = photosite_image::exif::read(&std::fs::read(&liar).unwrap());
    assert_eq!(meta.orientation, 1);
    assert!(photosite_image::quick(&liar).unwrap().is_none());
    assert!(
        photosite_image::sized(&liar, 320).is_err(),
        "an error is reported, not swallowed"
    );
}

#[test]
fn overriding_the_paths_leaves_the_system_locations_alone() {
    let data = tempfile::tempdir().unwrap();
    let paths = Paths::portable(data.path());
    paths.ensure().unwrap();
    let catalog = Catalog::open(&paths.catalog()).unwrap();
    drop(catalog);

    assert!(
        paths.catalog().starts_with(data.path()),
        "the catalogue has to sit under the overridden root"
    );
    assert!(paths.catalog().exists());
}
