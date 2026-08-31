//! Where files are going, worked out before a single byte moves.
//!
//! The plan is separate from carrying it out for one reason: every way this
//! can go wrong is a question about names, and names can be tested without
//! touching a disk. Two photographs of the same name from different folders
//! landing on each other, a copy quietly overwriting what was already there,
//! a sidecar left behind under the old name — all of it is decided here,
//! where a test can watch.
//!
//! Nothing in this module opens, reads or writes anything. It is told what is
//! already in use and answers with paths.

use crate::domain::sidecar_of;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Copying leaves the original where it is; moving does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Copy,
    Move,
}

/// One file's journey, and its sidecar's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Where the sidecar goes, if the photograph has one. The plan never
    /// looks at a disk, so whether one exists is settled when it is carried
    /// out — but where it would land is decided here, with the photograph,
    /// or the two could part company.
    pub sidecar: (PathBuf, PathBuf),
}

/// Where each of `files` lands in `into`.
///
/// `taken` says whether a path is already in use — the file system in
/// earnest, a set of names in a test.
///
/// A file that is already where it is being moved to is left out rather than
/// moved onto itself. Copying into its own folder is not the same thing: that
/// is what duplicating is, and it gets a free name.
pub fn plan(
    files: &[PathBuf],
    into: &Path,
    mode: Mode,
    taken: &dyn Fn(&Path) -> bool,
) -> Vec<Planned> {
    let mut planned = Vec::with_capacity(files.len());
    // What this very plan has already spoken for. Without it, `a/holiday.jpg`
    // and `b/holiday.jpg` chosen together both land on `holiday.jpg` and the
    // second silently eats the first.
    let mut claimed: BTreeSet<PathBuf> = BTreeSet::new();

    for from in files {
        let Some(name) = from.file_name() else {
            continue;
        };

        let wanted = into.join(name);
        if mode == Mode::Move && wanted == *from {
            continue;
        }

        let free = free_name(&wanted, &|path| {
            claimed.contains(path)
                // A photograph must never land beside a stranger's sidecar:
                // `holiday.xmp` written for somebody else's `holiday.nef`
                // would be read as this one's stars the moment it arrived.
                || taken(path)
                || taken(&sidecar_of(path))
        });

        claimed.insert(free.clone());
        planned.push(Planned {
            sidecar: (sidecar_of(from), sidecar_of(&free)),
            from: from.clone(),
            to: free,
        });
    }

    planned
}

/// `wanted` if nothing is using it, else the same name with a number.
///
/// `holiday.jpg` becomes `holiday (2).jpg`, then `holiday (3).jpg`. The
/// number goes before the extension and not after it, or the copy stops being
/// a photograph as far as everything else is concerned.
pub fn free_name(wanted: &Path, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    if !taken(wanted) {
        return wanted.to_path_buf();
    }

    let parent = wanted.parent().unwrap_or(Path::new(""));
    let stem = wanted
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = wanted
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();

    // Two is where a second copy starts. Nobody calls the second one "1".
    for number in 2..10_000 {
        let candidate = parent.join(format!("{stem} ({number}){extension}"));
        if !taken(&candidate) {
            return candidate;
        }
    }

    parent.join(format!("{stem} (copy){extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing_taken(_: &Path) -> bool {
        false
    }

    fn taken(names: &[&str]) -> impl Fn(&Path) -> bool + use<> {
        let names: BTreeSet<PathBuf> = names.iter().map(PathBuf::from).collect();
        move |path: &Path| names.contains(path)
    }

    #[test]
    fn a_free_destination_keeps_the_name_it_came_with() {
        let plan = plan(
            &[PathBuf::from("/a/holiday.jpg")],
            Path::new("/b"),
            Mode::Copy,
            &nothing_taken,
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].to, PathBuf::from("/b/holiday.jpg"));
        assert_eq!(plan[0].sidecar.0, PathBuf::from("/a/holiday.xmp"));
        assert_eq!(plan[0].sidecar.1, PathBuf::from("/b/holiday.xmp"));
    }

    /// The one that would be quiet and unrecoverable: two photographs of the
    /// same name from different folders, chosen together.
    #[test]
    fn two_of_the_same_name_do_not_land_on_each_other() {
        let plan = plan(
            &[
                PathBuf::from("/a/holiday.jpg"),
                PathBuf::from("/b/holiday.jpg"),
                PathBuf::from("/c/holiday.jpg"),
            ],
            Path::new("/into"),
            Mode::Copy,
            &nothing_taken,
        );
        let landed: Vec<&Path> = plan.iter().map(|one| one.to.as_path()).collect();
        assert_eq!(
            landed,
            [
                Path::new("/into/holiday.jpg"),
                Path::new("/into/holiday (2).jpg"),
                Path::new("/into/holiday (3).jpg")
            ]
        );
    }

    #[test]
    fn nothing_at_the_destination_is_written_over() {
        let already = taken(&["/into/holiday.jpg", "/into/holiday (2).jpg"]);
        let plan = plan(
            &[PathBuf::from("/a/holiday.jpg")],
            Path::new("/into"),
            Mode::Copy,
            &already,
        );
        assert_eq!(plan[0].to, PathBuf::from("/into/holiday (3).jpg"));
        assert_eq!(
            plan[0].sidecar.1,
            PathBuf::from("/into/holiday (3).xmp"),
            "the sidecar was left pointing at the name the photograph did not get"
        );
    }

    /// A stranger's sidecar is as good as the name being taken. Landing on it
    /// would mean reading somebody else's stars as this photograph's own.
    #[test]
    fn a_name_whose_sidecar_belongs_to_somebody_else_is_not_free() {
        let already = taken(&["/into/holiday.xmp"]);
        let plan = plan(
            &[PathBuf::from("/a/holiday.nef")],
            Path::new("/into"),
            Mode::Copy,
            &already,
        );
        assert_eq!(plan[0].to, PathBuf::from("/into/holiday (2).nef"));
    }

    #[test]
    fn moving_a_file_to_where_it_already_is_is_not_a_move() {
        let plan = plan(
            &[
                PathBuf::from("/a/one.jpg"),
                PathBuf::from("/a/two.jpg"),
                PathBuf::from("/b/three.jpg"),
            ],
            Path::new("/a"),
            Mode::Move,
            &nothing_taken,
        );
        let landed: Vec<&Path> = plan.iter().map(|one| one.from.as_path()).collect();
        assert_eq!(landed, [Path::new("/b/three.jpg")]);
    }

    /// Copying into its own folder is a different question, and it has a
    /// different answer: that is what duplicating is.
    #[test]
    fn copying_into_its_own_folder_gets_a_free_name() {
        let already = taken(&["/a/one.jpg"]);
        let plan = plan(
            &[PathBuf::from("/a/one.jpg")],
            Path::new("/a"),
            Mode::Copy,
            &already,
        );
        assert_eq!(plan[0].to, PathBuf::from("/a/one (2).jpg"));
    }

    #[test]
    fn a_file_with_no_extension_still_gets_a_number() {
        let free = free_name(Path::new("/a/holiday"), &taken(&["/a/holiday"]));
        assert_eq!(free, PathBuf::from("/a/holiday (2)"));
    }

    #[test]
    fn a_name_that_can_never_be_free_still_answers() {
        let free = free_name(Path::new("/a/one.jpg"), &|_| true);
        assert_eq!(free, PathBuf::from("/a/one (copy).jpg"));
    }
}
