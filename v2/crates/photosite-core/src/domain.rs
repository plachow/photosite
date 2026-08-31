//! The words the whole application is about.
//!
//! The vocabulary follows [CONTEXT.md](../../../CONTEXT.md) — it is settled,
//! and inventing new names for the same things buys nothing.

use crate::theme::Color;
use std::path::{Path, PathBuf};

/// The number a photograph lives under in the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhotoId(pub i64);

/// One catalogue row: the file's identity, what was read out of it, and what
/// somebody has said about it.
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
    /// What took it, and with what. Both are what the filter offers, so
    /// they are columns rather than something read back out of the file.
    pub camera: Option<String>,
    pub lens: Option<String>,
    /// What somebody has said about it, as opposed to what was read out of
    /// it. A scan never touches this.
    pub organisation: Organisation,
}

/// What somebody has said about a photograph: the stars, the label, the
/// culling verdict and the words.
///
/// It is separate from the rest of [`Photo`] for one reason that costs
/// nothing to honour now and a great deal later — **a rescan must never
/// touch it.** The file's length and write time come from the disk and are
/// overwritten every time we look; a rating comes from a person and is not
/// ours to overwrite.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Organisation {
    /// Stars, 0..=5. Set it through [`Organisation::with_rating`] or clamp
    /// yourself; the catalogue refuses anything else.
    pub rating: u8,
    pub label: ColorLabel,
    pub flag: Flag,
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
}

impl Organisation {
    /// The highest rating there is. Written once here rather than as a `5`
    /// in every comparison.
    pub const MAX_RATING: u8 = 5;

    pub fn with_rating(mut self, stars: u8) -> Self {
        self.rating = stars.min(Self::MAX_RATING);
        self
    }

    /// Is there anything here at all? Used to decide whether a tile needs a
    /// badge strip drawn over it, which is most tiles in most libraries.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// The Lightroom and Bridge colour labels.
///
/// They are held under the names XMP uses (`xmp:Label`) rather than as bare
/// numbers, because that is what makes a label set here survive a round trip
/// through other software — and because the numbering is ours, while the
/// name is everybody's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub enum ColorLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl ColorLabel {
    /// Every label including `None`, in the order the keys 6..9 and 0 set
    /// them. Menus and the keyboard read from here rather than listing the
    /// variants again.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Red,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
    ];

    /// How XMP names it. The empty string means no label — XMP has no word
    /// for it either.
    pub fn xmp_name(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
        }
    }

    /// Anything unrecognised is no label rather than an error: other
    /// software is free to write its own words there, and a strange one is
    /// no reason to refuse the photograph.
    pub fn from_xmp_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "red" => Self::Red,
            "yellow" => Self::Yellow,
            "green" => Self::Green,
            "blue" => Self::Blue,
            "purple" => Self::Purple,
            _ => Self::None,
        }
    }

    /// What the catalogue stores. Numbering matches v1, so a catalogue
    /// carried across reads the same.
    pub fn as_i64(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Red => 1,
            Self::Yellow => 2,
            Self::Green => 3,
            Self::Blue => 4,
            Self::Purple => 5,
        }
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Red,
            2 => Self::Yellow,
            3 => Self::Green,
            4 => Self::Blue,
            5 => Self::Purple,
            _ => Self::None,
        }
    }

    /// The swatch drawn on a tile.
    ///
    /// These are not part of the theme and deliberately so: a red label has
    /// to look red in every theme, or the word and the colour stop agreeing.
    /// The palette decides what the application looks like; the label
    /// decides what the label means.
    pub fn color(self) -> Option<Color> {
        match self {
            Self::None => None,
            Self::Red => Some(Color::hex(0xE5533D)),
            Self::Yellow => Some(Color::hex(0xE8B84A)),
            Self::Green => Some(Color::hex(0x6FBF5B)),
            Self::Blue => Some(Color::hex(0x4B9BE8)),
            Self::Purple => Some(Color::hex(0xA67BD8)),
        }
    }

    /// A translation key, not text. The core holds nothing that is seen.
    pub fn title_key(self) -> &'static str {
        match self {
            Self::None => "label-none",
            Self::Red => "label-red",
            Self::Yellow => "label-yellow",
            Self::Green => "label-green",
            Self::Blue => "label-blue",
            Self::Purple => "label-purple",
        }
    }
}

/// The culling verdict.
///
/// A rejected photograph stays on disk and in the catalogue — it only dims in
/// the gallery. Deleting is always an explicit second step, and that
/// separation is the whole point of having a verdict at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub enum Flag {
    Rejected,
    #[default]
    None,
    Picked,
}

impl Flag {
    /// Matches v1: rejection is negative on purpose, so ordering by the
    /// stored number puts the rejects at one end.
    pub fn as_i64(self) -> i64 {
        match self {
            Self::Rejected => -1,
            Self::None => 0,
            Self::Picked => 1,
        }
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            i64::MIN..=-1 => Self::Rejected,
            0 => Self::None,
            _ => Self::Picked,
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Rejected => "flag-rejected",
            Self::None => "flag-none",
            Self::Picked => "flag-picked",
        }
    }
}

/// What the gallery is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    TakenAt,
    Name,
    Rating,
    ModifiedAt,
    FileSize,
    Dimensions,
}

impl SortField {
    pub const ALL: [Self; 6] = [
        Self::TakenAt,
        Self::Name,
        Self::Rating,
        Self::ModifiedAt,
        Self::FileSize,
        Self::Dimensions,
    ];

    /// The stable name the settings store. Not the translated one — that may
    /// be rewritten at any time, and a settings file must not depend on it.
    pub fn id(self) -> &'static str {
        match self {
            Self::TakenAt => "taken",
            Self::Name => "name",
            Self::Rating => "rating",
            Self::ModifiedAt => "modified",
            Self::FileSize => "size",
            Self::Dimensions => "dimensions",
        }
    }

    /// An unknown name falls back to the default rather than refusing the
    /// settings file. A key we no longer understand must not cost somebody
    /// the rest of what they configured.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|field| field.id() == id)
            .unwrap_or_default()
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::TakenAt => "sort-taken",
            Self::Name => "sort-name",
            Self::Rating => "sort-rating",
            Self::ModifiedAt => "sort-modified",
            Self::FileSize => "sort-size",
            Self::Dimensions => "sort-dimensions",
        }
    }
}

/// The same names as [`SortField::id`], as a list the settings can offer.
///
/// A const cannot call a method, so this is written out — and
/// `the_offered_sort_fields_are_the_ones_that_exist` is what keeps it from
/// drifting away from the enum. A list that could never be wrong would not
/// need the test; this one could.
pub const SORT_FIELD_IDS: &[&str] = &["taken", "name", "rating", "modified", "size", "dimensions"];

/// How the gallery is ordered: by what, and which way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sort {
    pub field: SortField,
    pub descending: bool,
}

impl Sort {
    pub fn new(field: SortField, descending: bool) -> Self {
        Self { field, descending }
    }

    /// Orders a folder in place.
    ///
    /// Two rules that are worth more than they look:
    ///
    /// * **The path always breaks a tie.** Without it, a thousand photographs
    ///   with the same rating come back in a different order every time the
    ///   sort is redone, and the grid appears to shuffle under the mouse.
    /// * **A photograph with no date sorts last either way.** Reversing the
    ///   order would otherwise open the folder on the files we know least
    ///   about, which is never what somebody meant by "newest first".
    pub fn apply(&self, photos: &mut [Photo]) {
        photos.sort_by(|a, b| {
            let ordering = match self.field {
                SortField::TakenAt => return self.by_option(a.taken_at, b.taken_at, a, b),
                SortField::Name => file_name(&a.path).cmp(&file_name(&b.path)),
                SortField::Rating => a.organisation.rating.cmp(&b.organisation.rating),
                SortField::ModifiedAt => a.modified_at.cmp(&b.modified_at),
                SortField::FileSize => a.file_size.cmp(&b.file_size),
                SortField::Dimensions => {
                    return self.by_option(pixels(a), pixels(b), a, b);
                }
            };

            self.flip(ordering).then_with(|| a.path.cmp(&b.path))
        });
    }

    /// Ordering where the value may be missing. The missing ones go last
    /// whichever way round the rest is.
    fn by_option<T: Ord>(
        &self,
        left: Option<T>,
        right: Option<T>,
        a: &Photo,
        b: &Photo,
    ) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (left, right) {
            (Some(left), Some(right)) => self.flip(left.cmp(&right)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.path.cmp(&b.path))
    }

    fn flip(&self, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
        if self.descending {
            ordering.reverse()
        } else {
            ordering
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// The count of pixels, where both sides are known. Sorting by "dimensions"
/// means by area — a 6000x4000 frame is larger than a 5000x5000 one to
/// nobody, but it is the number everybody means.
fn pixels(photo: &Photo) -> Option<u64> {
    match (photo.width, photo.height) {
        (Some(width), Some(height)) => Some(width as u64 * height as u64),
        _ => None,
    }
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

    fn photo(name: &str) -> Photo {
        Photo {
            id: PhotoId(0),
            path: PathBuf::from(format!("/a/{name}")),
            folder: PathBuf::from("/a"),
            file_size: 1000,
            modified_at: 100,
            taken_at: Some(100),
            width: Some(100),
            height: Some(100),
            orientation: 1,
            camera: None,
            lens: None,
            organisation: Organisation::default(),
        }
    }

    fn names(photos: &[Photo]) -> Vec<String> {
        photos.iter().map(|p| file_name(&p.path)).collect()
    }

    #[test]
    fn extensions_are_recognised_whatever_the_case() {
        assert!(is_photo(Path::new("a/b/C.JPG")));
        assert!(is_photo(Path::new("x.jpeg")));
        assert!(!is_photo(Path::new("x.txt")));
        assert!(!is_photo(Path::new("no_extension")));
    }

    #[test]
    fn a_rating_never_goes_above_five() {
        assert_eq!(Organisation::default().with_rating(9).rating, 5);
        assert_eq!(Organisation::default().with_rating(3).rating, 3);
    }

    #[test]
    fn a_label_survives_the_trip_through_its_xmp_name() {
        for label in ColorLabel::ALL {
            assert_eq!(
                ColorLabel::from_xmp_name(label.xmp_name()),
                label,
                "{label:?}"
            );
        }
    }

    #[test]
    fn a_label_survives_the_trip_through_the_catalogue() {
        for label in ColorLabel::ALL {
            assert_eq!(ColorLabel::from_i64(label.as_i64()), label, "{label:?}");
        }
    }

    #[test]
    fn a_label_written_by_other_software_is_no_label_rather_than_an_error() {
        assert_eq!(ColorLabel::from_xmp_name("Approved"), ColorLabel::None);
        assert_eq!(ColorLabel::from_xmp_name("  RED  "), ColorLabel::Red);
        assert_eq!(ColorLabel::from_i64(77), ColorLabel::None);
    }

    #[test]
    fn a_flag_survives_the_trip_through_the_catalogue() {
        for flag in [Flag::Rejected, Flag::None, Flag::Picked] {
            assert_eq!(Flag::from_i64(flag.as_i64()), flag, "{flag:?}");
        }
    }

    #[test]
    fn a_sort_field_survives_the_trip_through_the_settings() {
        for field in SortField::ALL {
            assert_eq!(SortField::from_id(field.id()), field, "{field:?}");
        }
    }

    #[test]
    fn the_offered_sort_fields_are_the_ones_that_exist() {
        let from_enum: Vec<&str> = SortField::ALL.iter().map(|field| field.id()).collect();
        assert_eq!(from_enum, SORT_FIELD_IDS);
    }

    #[test]
    fn a_sort_field_we_no_longer_understand_falls_back_to_the_default() {
        assert_eq!(SortField::from_id("by-vibes"), SortField::default());
    }

    #[test]
    fn sorting_by_name_ignores_case() {
        let mut photos = vec![photo("b.jpg"), photo("A.jpg"), photo("c.jpg")];
        Sort::new(SortField::Name, false).apply(&mut photos);
        assert_eq!(names(&photos), ["a.jpg", "b.jpg", "c.jpg"]);
    }

    #[test]
    fn the_path_breaks_every_tie_so_the_order_does_not_shuffle() {
        // Every one of these is identical but for its name. Without the
        // tiebreak the order would depend on what came in.
        let mut photos = vec![photo("c.jpg"), photo("a.jpg"), photo("b.jpg")];
        let sort = Sort::new(SortField::Rating, true);
        sort.apply(&mut photos);
        let once = names(&photos);
        photos.reverse();
        sort.apply(&mut photos);
        assert_eq!(names(&photos), once);
    }

    #[test]
    fn a_photograph_with_no_date_sorts_last_either_way_round() {
        let mut dated = photo("dated.jpg");
        dated.taken_at = Some(500);
        let mut undated = photo("undated.jpg");
        undated.taken_at = None;

        for descending in [false, true] {
            let mut photos = vec![undated.clone(), dated.clone()];
            Sort::new(SortField::TakenAt, descending).apply(&mut photos);
            assert_eq!(
                names(&photos),
                ["dated.jpg", "undated.jpg"],
                "descending={descending}"
            );
        }
    }

    #[test]
    fn descending_really_is_the_other_way_round() {
        let mut photos = Vec::new();
        for (index, name) in ["a.jpg", "b.jpg", "c.jpg"].iter().enumerate() {
            let mut one = photo(name);
            one.file_size = 100 * (index as u64 + 1);
            photos.push(one);
        }

        Sort::new(SortField::FileSize, false).apply(&mut photos);
        assert_eq!(names(&photos), ["a.jpg", "b.jpg", "c.jpg"]);
        Sort::new(SortField::FileSize, true).apply(&mut photos);
        assert_eq!(names(&photos), ["c.jpg", "b.jpg", "a.jpg"]);
    }

    #[test]
    fn sorting_by_dimensions_means_by_area() {
        let mut wide = photo("wide.jpg");
        wide.width = Some(6000);
        wide.height = Some(1000);
        let mut square = photo("square.jpg");
        square.width = Some(3000);
        square.height = Some(3000);

        let mut photos = vec![wide, square];
        Sort::new(SortField::Dimensions, false).apply(&mut photos);
        assert_eq!(names(&photos), ["wide.jpg", "square.jpg"]);
    }

    #[test]
    fn an_empty_organisation_knows_it_is_empty() {
        assert!(Organisation::default().is_empty());
        assert!(!Organisation::default().with_rating(1).is_empty());
        assert!(
            !Organisation {
                keywords: vec!["holiday".to_owned()],
                ..Default::default()
            }
            .is_empty()
        );
    }
}
