//! Narrowing a folder down to what somebody is looking for.
//!
//! Two rules carried straight over from v1, because both are what makes a
//! filter usable rather than a puzzle:
//!
//! **An empty facet does not filter.** No labels chosen means "any label",
//! not "no label". So [`Filter::default`] shows the whole folder and every
//! facet added narrows it further — the facets compose by conjunction, the
//! values inside one facet by disjunction. Choosing red and green means
//! *red or green*; choosing red and adding three stars means *red **and**
//! three stars*.
//!
//! **Only offer what is there.** [`Facets`] is collected from the folder in
//! front of you, so the panel never offers a camera that would return an
//! empty gallery. A list of every camera ever owned, most of them matching
//! nothing here, is a list nobody reads.
//!
//! The filter is deliberately **not** saved to the settings. A filter that
//! survived a restart would hide photographs on a later day for a reason
//! nobody remembers setting, and "where did half my folder go" is not a
//! question an application should ever cause.

use crate::domain::{ColorLabel, Flag, Photo};
use crate::i18n::t;
use crate::place::Verdict;
use std::collections::BTreeSet;

/// The shape of the frame, which is not the same as its orientation tag: a
/// photograph rotated by EXIF is portrait however its pixels are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub enum Shape {
    #[default]
    Any,
    Landscape,
    Portrait,
    Square,
}

impl Shape {
    pub const ALL: [Self; 4] = [Self::Any, Self::Landscape, Self::Portrait, Self::Square];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Any => "shape-any",
            Self::Landscape => "shape-landscape",
            Self::Portrait => "shape-portrait",
            Self::Square => "shape-square",
        }
    }

    /// What shape a photograph is, once its orientation tag is taken into
    /// account.
    ///
    /// Orientations 5..8 turn the frame a quarter, so a camera held upright
    /// stores a landscape frame and means a portrait one. Judging by the
    /// stored width alone puts every phone photograph in the wrong pile.
    pub fn of(photo: &Photo) -> Option<Self> {
        let (width, height) = (photo.width?, photo.height?);
        let (width, height) = if (5..=8).contains(&photo.orientation) {
            (height, width)
        } else {
            (width, height)
        };

        Some(match width.cmp(&height) {
            std::cmp::Ordering::Greater => Self::Landscape,
            std::cmp::Ordering::Less => Self::Portrait,
            std::cmp::Ordering::Equal => Self::Square,
        })
    }
}

/// One side of an expression facet.
///
/// Two sides and not one, because a portrait session is culled both ways
/// round: "keep the ones where everybody is smiling" and "show me the ones
/// where somebody blinked" are the same folder looked at from either end,
/// and an application that offers only the first makes the second a hunt.
///
/// [`Self::All`] means every face on the photograph, including the ones no
/// expression model has ever seen; [`Self::Anyone`] counts only the faces
/// that were actually looked at. A face nobody scored is not evidence of a
/// frown, and it is not evidence of a smile either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub enum Expression {
    #[default]
    Any,
    /// Everybody on the photograph passes.
    All,
    /// Somebody who was looked at does not.
    Anyone,
}

/// Everything the gallery can be narrowed by.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// At least this many stars. Nought does not filter.
    pub minimum_rating: u8,
    pub labels: BTreeSet<ColorLabel>,
    pub flags: BTreeSet<Flag>,
    /// Lower-case, without the dot: `jpg`, `png`.
    pub formats: BTreeSet<String>,
    pub cameras: BTreeSet<String>,
    pub lenses: BTreeSet<String>,
    pub shape: Shape,
    /// How much a photograph's position is to be trusted. This is what
    /// collects the doubtful ones for a look, which is the only reason to
    /// mark them in the first place.
    pub places: BTreeSet<Verdict>,
    /// Seconds since the epoch, both ends inclusive.
    pub taken_from: Option<i64>,
    pub taken_to: Option<i64>,
    /// Who has to be on the photograph.
    ///
    /// The one facet that composes by **conjunction**: two names means the
    /// photographs with both of them on. Every other facet widens as values
    /// are added because they are alternatives — a photograph has one label
    /// — but a photograph has any number of people, and picking two people
    /// means the shot they are both in. That is the only reason anybody
    /// picks two.
    pub people: BTreeSet<String>,
    pub smile: Expression,
    pub eyes: Expression,
    pub search: String,
    /// Drops the rejects whatever else is set, so a culling pass visibly
    /// shrinks the folder as it goes. It is its own switch and not a flag
    /// facet, because "show me the picks" and "stop showing me the rejects"
    /// are different questions.
    pub hide_rejected: bool,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        self.minimum_rating > 0
            || !self.labels.is_empty()
            || !self.flags.is_empty()
            || !self.formats.is_empty()
            || !self.cameras.is_empty()
            || !self.lenses.is_empty()
            || self.shape != Shape::Any
            || !self.places.is_empty()
            || !self.people.is_empty()
            || self.smile != Expression::Any
            || self.eyes != Expression::Any
            || self.taken_from.is_some()
            || self.taken_to.is_some()
            || self.hide_rejected
            || !self.search.trim().is_empty()
    }

    /// Does this photograph get through?
    pub fn keeps(&self, photo: &Photo) -> bool {
        let organisation = &photo.organisation;

        if organisation.rating < self.minimum_rating {
            return false;
        }

        if self.hide_rejected && organisation.flag == Flag::Rejected {
            return false;
        }

        if !self.labels.is_empty() && !self.labels.contains(&organisation.label) {
            return false;
        }

        if !self.flags.is_empty() && !self.flags.contains(&organisation.flag) {
            return false;
        }

        if !self.formats.is_empty() {
            match format_of(photo) {
                Some(format) if self.formats.contains(&format) => {}
                _ => return false,
            }
        }

        // A photograph whose camera we do not know is not "any camera" — it
        // is one we cannot answer for, and it does not belong in the results
        // for "taken with the D90".
        if !self.cameras.is_empty() && !matches(&self.cameras, photo.camera.as_deref()) {
            return false;
        }

        if !self.lenses.is_empty() && !matches(&self.lenses, photo.lens.as_deref()) {
            return false;
        }

        if self.shape != Shape::Any && Shape::of(photo) != Some(self.shape) {
            return false;
        }

        if !self.places.is_empty() && !self.places.contains(&photo.verdict) {
            return false;
        }

        for name in &self.people {
            if !photo
                .people
                .iter()
                .any(|tag| tag.name.eq_ignore_ascii_case(name))
            {
                return false;
            }
        }

        let expressions = &photo.expressions;
        match self.smile {
            Expression::Any => {}
            Expression::All if !expressions.all_smiling() => return false,
            Expression::Anyone if !expressions.anyone_not_smiling() => return false,
            _ => {}
        }

        match self.eyes {
            Expression::Any => {}
            Expression::All if !expressions.all_eyes_open() => return false,
            Expression::Anyone if !expressions.anyone_blinking() => return false,
            _ => {}
        }

        // Undated photographs fall outside every date range rather than
        // inside all of them. Asking for "last July" and being handed every
        // file with no date in it is not an answer.
        if self.taken_from.is_some() || self.taken_to.is_some() {
            let Some(taken) = photo.taken_at else {
                return false;
            };

            if self.taken_from.is_some_and(|from| taken < from)
                || self.taken_to.is_some_and(|to| taken > to)
            {
                return false;
            }
        }

        self.matches_search(photo)
    }

    /// One box over the file name, the title, the description and the
    /// keywords — which is what somebody means by typing "iceland" into a
    /// search field.
    ///
    /// Every **word** typed has to appear somewhere, rather than the whole
    /// line appearing as one piece. v1 matched the line, so "iceland
    /// waterfall" found nothing unless those two words stood together;
    /// here it finds a photograph titled *Waterfall* with the keyword
    /// *Iceland*. One word behaves identically either way.
    fn matches_search(&self, photo: &Photo) -> bool {
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }

        let organisation = &photo.organisation;
        let mut haystack = photo
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        for extra in [
            organisation.title.as_deref(),
            organisation.description.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            haystack.push(' ');
            haystack.push_str(&extra.to_lowercase());
        }

        for keyword in &organisation.keywords {
            haystack.push(' ');
            haystack.push_str(&keyword.to_lowercase());
        }

        needle
            .split_whitespace()
            .all(|word| haystack.contains(word))
    }

    /// A short line naming what is set, so an active filter is never
    /// invisible. Half a folder missing with nothing on screen to say why is
    /// the worst thing a filter can do.
    pub fn describe(&self) -> String {
        if !self.is_active() {
            return t("filter-none");
        }

        let mut parts: Vec<String> = Vec::new();
        if self.minimum_rating > 0 {
            parts.push(crate::i18n::t_args(
                "filter-rating",
                &[("count", (self.minimum_rating as i64).into())],
            ));
        }

        if !self.labels.is_empty() {
            parts.push(join(self.labels.iter().map(|label| t(label.title_key()))));
        }

        if !self.flags.is_empty() {
            parts.push(join(self.flags.iter().map(|flag| t(flag.title_key()))));
        }

        if !self.formats.is_empty() {
            parts.push(join(
                self.formats.iter().map(|format| format.to_uppercase()),
            ));
        }

        for (chosen, key) in [
            (&self.cameras, "filter-cameras"),
            (&self.lenses, "filter-lenses"),
        ] {
            match chosen.len() {
                0 => {}
                1 => parts.push(chosen.iter().next().cloned().unwrap_or_default()),
                many => parts.push(crate::i18n::t_args(key, &[("count", (many as i64).into())])),
            }
        }

        if self.shape != Shape::Any {
            parts.push(t(self.shape.title_key()));
        }

        if !self.places.is_empty() {
            parts.push(join(
                self.places.iter().map(|verdict| t(verdict.title_key())),
            ));
        }

        // People are joined with "+" and not "/", because they narrow each
        // other rather than widening: the line has to read the way the
        // filter behaves.
        if !self.people.is_empty() {
            parts.push(self.people.iter().cloned().collect::<Vec<_>>().join(" + "));
        }

        for (facet, keys) in [
            (
                self.smile,
                ["filter-all-smiling", "filter-someone-not-smiling"],
            ),
            (
                self.eyes,
                ["filter-all-eyes-open", "filter-someone-blinking"],
            ),
        ] {
            match facet {
                Expression::Any => {}
                Expression::All => parts.push(t(keys[0])),
                Expression::Anyone => parts.push(t(keys[1])),
            }
        }

        if self.taken_from.is_some() || self.taken_to.is_some() {
            parts.push(t("filter-date"));
        }

        if !self.search.trim().is_empty() {
            parts.push(format!("\u{201c}{}\u{201d}", self.search.trim()));
        }

        if self.hide_rejected {
            parts.push(t("filter-no-rejects"));
        }

        parts.join(" \u{b7} ")
    }
}

fn join(parts: impl Iterator<Item = String>) -> String {
    parts.collect::<Vec<_>>().join("/")
}

fn matches(chosen: &BTreeSet<String>, value: Option<&str>) -> bool {
    value.is_some_and(|value| chosen.contains(value))
}

/// The file's extension, lower-case and without the dot.
pub fn format_of(photo: &Photo) -> Option<String> {
    photo
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
}

/// What this particular folder holds, so the panel offers nothing that would
/// come back empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facets {
    pub formats: Vec<String>,
    pub cameras: Vec<String>,
    pub lenses: Vec<String>,
    pub labels: Vec<ColorLabel>,
    pub flags: Vec<Flag>,
    pub shapes: Vec<Shape>,
    /// Which verdicts this folder actually holds. A folder where every
    /// position is precise has nothing to review, and says so by offering
    /// nothing.
    pub places: Vec<Verdict>,
    /// Everybody who appears anywhere in this folder, each once. What the
    /// panel offers: the people who are actually on these photographs, not
    /// everybody ever named.
    pub people: Vec<String>,
    /// Whether anything here has been scored for expression at all. Without
    /// it the smile and eyes rows would be a heading over two buttons that
    /// can only ever empty the gallery.
    pub expressions: bool,
    /// The highest rating anything here carries. Offering "four stars and up"
    /// in a folder where nothing has more than two is offering an empty
    /// gallery.
    pub highest_rating: u8,
    /// The oldest and the newest capture time, where anything has one.
    pub taken: Option<(i64, i64)>,
}

impl Facets {
    /// One pass over the folder. It is linear and it runs when the folder is
    /// read, not every frame.
    pub fn of(photos: &[Photo]) -> Self {
        let mut formats = BTreeSet::new();
        let mut cameras = BTreeSet::new();
        let mut lenses = BTreeSet::new();
        let mut labels = BTreeSet::new();
        let mut flags = BTreeSet::new();
        let mut shapes = BTreeSet::new();
        let mut places = BTreeSet::new();
        let mut people = BTreeSet::new();
        let mut expressions = false;
        let mut highest_rating = 0u8;
        let mut taken: Option<(i64, i64)> = None;

        for photo in photos {
            if let Some(format) = format_of(photo) {
                formats.insert(format);
            }

            if let Some(camera) = &photo.camera {
                cameras.insert(camera.clone());
            }

            if let Some(lens) = &photo.lens {
                lenses.insert(lens.clone());
            }

            if let Some(shape) = Shape::of(photo) {
                shapes.insert(shape);
            }

            // No label and no verdict are values too: "show me the ones I
            // have not decided about" is the most useful question there is
            // half way through a cull.
            labels.insert(photo.organisation.label);
            flags.insert(photo.organisation.flag);
            places.insert(photo.verdict);
            for tag in &photo.people {
                people.insert(tag.name.clone());
            }

            expressions = expressions || photo.expressions.scored > 0;
            highest_rating = highest_rating.max(photo.organisation.rating);

            if let Some(at) = photo.taken_at {
                taken = Some(match taken {
                    Some((first, last)) => (first.min(at), last.max(at)),
                    None => (at, at),
                });
            }
        }

        Self {
            formats: formats.into_iter().collect(),
            cameras: cameras.into_iter().collect(),
            lenses: lenses.into_iter().collect(),
            labels: labels.into_iter().collect(),
            flags: flags.into_iter().collect(),
            shapes: shapes.into_iter().collect(),
            places: places.into_iter().collect(),
            people: people.into_iter().collect(),
            expressions,
            highest_rating,
            taken,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Organisation, PhotoId};
    use std::path::PathBuf;

    /// Two names means the photograph with both of them on it. Every other
    /// facet widens as values are added; this one narrows, and that is
    /// deliberate.
    #[test]
    fn asking_for_two_people_asks_for_the_shot_they_are_both_in() {
        let tag = |id: i64, name: &str| crate::people::Tag {
            id,
            name: name.to_owned(),
        };
        let mut both = photo("both.jpg");
        both.people = vec![tag(1, "Jana"), tag(2, "Petr")];
        let mut alone = photo("alone.jpg");
        alone.people = vec![tag(1, "Jana")];

        let mut filter = Filter::default();
        filter.people.insert("Jana".to_owned());
        assert!(filter.keeps(&both) && filter.keeps(&alone));

        filter.people.insert("Petr".to_owned());
        assert!(filter.keeps(&both));
        assert!(!filter.keeps(&alone), "one of the two was enough");
    }

    #[test]
    fn a_name_matches_whatever_the_case() {
        let mut photo = photo("a.jpg");
        photo.people = vec![crate::people::Tag {
            id: 1,
            name: "Jana".to_owned(),
        }];
        let mut filter = Filter::default();
        filter.people.insert("jana".to_owned());
        assert!(filter.keeps(&photo));
    }

    /// A portrait session is culled both ways round, so both sides have to
    /// work — and a face nobody has scored must not count as either.
    #[test]
    fn both_sides_of_the_smile_facet_answer_a_real_question() {
        let expressions = |faces, scored, smiling, eyes_open| crate::people::Expressions {
            faces,
            scored,
            smiling,
            eyes_open,
        };

        let mut everyone = photo("everyone.jpg");
        everyone.expressions = expressions(2, 2, 2, 2);
        let mut somebody = photo("somebody.jpg");
        somebody.expressions = expressions(2, 2, 1, 1);
        let mut unlooked = photo("unlooked.jpg");
        unlooked.expressions = expressions(2, 0, 0, 0);

        let mut filter = Filter {
            smile: Expression::All,
            ..Default::default()
        };
        assert!(filter.keeps(&everyone));
        assert!(!filter.keeps(&somebody));
        assert!(!filter.keeps(&unlooked), "nobody has looked at these faces");

        filter.smile = Expression::Anyone;
        assert!(!filter.keeps(&everyone));
        assert!(filter.keeps(&somebody));
        assert!(
            !filter.keeps(&unlooked),
            "an unscored face is not evidence of a frown"
        );
    }

    #[test]
    fn the_eyes_facet_is_the_smile_facet_for_the_other_thing() {
        let mut blinked = photo("blinked.jpg");
        blinked.expressions = crate::people::Expressions {
            faces: 3,
            scored: 3,
            smiling: 3,
            eyes_open: 2,
        };

        let mut filter = Filter {
            eyes: Expression::Anyone,
            ..Default::default()
        };
        assert!(filter.keeps(&blinked));
        filter.eyes = Expression::All;
        assert!(!filter.keeps(&blinked));
        // And it says nothing about smiles.
        filter.eyes = Expression::Any;
        filter.smile = Expression::All;
        assert!(filter.keeps(&blinked));
    }

    /// An active filter is never invisible, and the line has to read the way
    /// the filter behaves — people narrow, so they are joined with a plus.
    #[test]
    fn the_line_says_who_and_how_they_compose() {
        let mut filter = Filter::default();
        filter.people.insert("Jana".to_owned());
        filter.people.insert("Petr".to_owned());
        assert!(
            filter.describe().contains("Jana + Petr"),
            "{}",
            filter.describe()
        );
        assert!(filter.is_active());
    }

    #[test]
    fn a_folder_offers_the_people_that_are_in_it() {
        let tag = |name: &str| crate::people::Tag {
            id: 1,
            name: name.to_owned(),
        };
        let mut one = photo("one.jpg");
        one.people = vec![tag("Petr")];
        let mut two = photo("two.jpg");
        two.people = vec![tag("Jana"), tag("Petr")];
        two.expressions = crate::people::Expressions {
            faces: 2,
            scored: 2,
            smiling: 1,
            eyes_open: 2,
        };

        let facets = Facets::of(&[one, two, photo("three.jpg")]);
        assert_eq!(facets.people, ["Jana", "Petr"]);
        assert!(facets.expressions);
    }

    /// A folder nobody has swept must not offer a smile row: two buttons
    /// that can only ever empty the gallery.
    #[test]
    fn a_folder_nobody_has_swept_offers_no_expressions() {
        let facets = Facets::of(&[photo("a.jpg"), photo("b.jpg")]);
        assert!(facets.people.is_empty());
        assert!(!facets.expressions);
    }

    /// The point of marking a doubtful position is being able to collect
    /// them afterwards. A mark nobody can act on is decoration.
    #[test]
    fn the_doubted_positions_can_be_collected() {
        let mut folder = vec![photo("a.jpg"), photo("b.jpg"), photo("c.jpg")];
        folder[0].verdict = Verdict::Precise;
        folder[0].place = crate::place::Place::new(50.0, 14.0);
        folder[1].verdict = Verdict::Doubtful;
        folder[1].place = crate::place::Place::new(50.0, 14.0);
        // And one that says nothing about where it was.

        let filter = Filter {
            places: [Verdict::Doubtful, Verdict::Approximate]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let kept: Vec<&str> = folder
            .iter()
            .filter(|photo| filter.keeps(photo))
            .map(|photo| photo.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(kept, ["b.jpg"]);

        // And a folder with nothing to review offers nothing to review with.
        let facets = Facets::of(&folder);
        assert!(facets.places.contains(&Verdict::Doubtful));
        assert!(facets.places.contains(&Verdict::Nowhere));
    }

    fn photo(name: &str) -> Photo {
        Photo {
            id: PhotoId(1),
            path: PathBuf::from(format!("/a/{name}")),
            folder: PathBuf::from("/a"),
            file_size: 100,
            modified_at: 0,
            taken_at: Some(1_000),
            width: Some(4000),
            height: Some(3000),
            orientation: 1,
            camera: None,
            lens: None,
            organisation: Organisation::default(),
            people: Vec::new(),
            expressions: Default::default(),
            place: None,
            verdict: crate::place::Verdict::Nowhere,
            reason: None,
        }
    }

    #[test]
    fn an_empty_filter_keeps_everything() {
        let filter = Filter::default();
        assert!(!filter.is_active());
        assert!(filter.keeps(&photo("a.jpg")));
    }

    #[test]
    fn the_stars_are_a_floor_and_not_an_exact_match() {
        let filter = Filter {
            minimum_rating: 3,
            ..Default::default()
        };
        for (rating, kept) in [(0, false), (2, false), (3, true), (5, true)] {
            let mut one = photo("a.jpg");
            one.organisation.rating = rating;
            assert_eq!(filter.keeps(&one), kept, "rating {rating}");
        }
    }

    #[test]
    fn several_labels_mean_any_of_them() {
        let filter = Filter {
            labels: [ColorLabel::Red, ColorLabel::Green].into_iter().collect(),
            ..Default::default()
        };
        for (label, kept) in [
            (ColorLabel::Red, true),
            (ColorLabel::Green, true),
            (ColorLabel::Blue, false),
            (ColorLabel::None, false),
        ] {
            let mut one = photo("a.jpg");
            one.organisation.label = label;
            assert_eq!(filter.keeps(&one), kept, "{label:?}");
        }
    }

    #[test]
    fn two_facets_mean_both_of_them() {
        let filter = Filter {
            minimum_rating: 3,
            labels: [ColorLabel::Red].into_iter().collect(),
            ..Default::default()
        };

        let mut red_and_rated = photo("a.jpg");
        red_and_rated.organisation.label = ColorLabel::Red;
        red_and_rated.organisation.rating = 4;
        assert!(filter.keeps(&red_and_rated));

        let mut red_only = photo("b.jpg");
        red_only.organisation.label = ColorLabel::Red;
        assert!(!filter.keeps(&red_only), "one facet is not enough");
    }

    #[test]
    fn hiding_the_rejects_hides_them_whatever_else_is_asked() {
        let filter = Filter {
            hide_rejected: true,
            ..Default::default()
        };
        let mut rejected = photo("a.jpg");
        rejected.organisation.flag = Flag::Rejected;
        assert!(!filter.keeps(&rejected));
        assert!(filter.keeps(&photo("b.jpg")));
    }

    #[test]
    fn a_shape_is_judged_after_the_orientation_tag() {
        // Stored landscape, turned a quarter by EXIF: an upright frame.
        let mut upright = photo("a.jpg");
        upright.orientation = 6;
        assert_eq!(Shape::of(&upright), Some(Shape::Portrait));

        upright.orientation = 1;
        assert_eq!(Shape::of(&upright), Some(Shape::Landscape));

        let mut square = photo("b.jpg");
        square.height = Some(4000);
        assert_eq!(Shape::of(&square), Some(Shape::Square));
    }

    #[test]
    fn a_frame_of_unknown_size_has_no_shape_and_is_filtered_out() {
        let mut unknown = photo("a.jpg");
        unknown.width = None;
        assert_eq!(Shape::of(&unknown), None);

        let filter = Filter {
            shape: Shape::Landscape,
            ..Default::default()
        };
        assert!(!filter.keeps(&unknown));
    }

    #[test]
    fn the_format_comes_from_the_extension_whatever_its_case() {
        let filter = Filter {
            formats: ["jpg".to_owned()].into_iter().collect(),
            ..Default::default()
        };
        assert!(filter.keeps(&photo("a.JPG")));
        assert!(!filter.keeps(&photo("a.png")));
    }

    #[test]
    fn a_camera_we_do_not_know_is_not_every_camera() {
        let filter = Filter {
            cameras: ["NIKON D90".to_owned()].into_iter().collect(),
            ..Default::default()
        };

        let mut known = photo("a.jpg");
        known.camera = Some("NIKON D90".to_owned());
        assert!(filter.keeps(&known));
        assert!(!filter.keeps(&photo("b.jpg")), "no camera is not a match");
    }

    #[test]
    fn a_date_range_takes_both_ends_with_it() {
        let filter = Filter {
            taken_from: Some(500),
            taken_to: Some(1500),
            ..Default::default()
        };
        for (at, kept) in [
            (499, false),
            (500, true),
            (1000, true),
            (1500, true),
            (1501, false),
        ] {
            let mut one = photo("a.jpg");
            one.taken_at = Some(at);
            assert_eq!(filter.keeps(&one), kept, "at {at}");
        }
    }

    #[test]
    fn an_undated_photograph_is_outside_every_range() {
        let filter = Filter {
            taken_from: Some(0),
            ..Default::default()
        };
        let mut undated = photo("a.jpg");
        undated.taken_at = None;
        assert!(!filter.keeps(&undated));
    }

    #[test]
    fn the_search_covers_the_name_the_words_and_the_keywords() {
        let mut one = photo("IMG_2019.jpg");
        one.organisation.title = Some("Sunrise over Waikiki".to_owned());
        one.organisation.description = Some("The first morning".to_owned());
        one.organisation.keywords = vec!["Hawaii".to_owned()];

        for needle in ["img_2019", "waikiki", "MORNING", "hawaii"] {
            let filter = Filter {
                search: needle.to_owned(),
                ..Default::default()
            };
            assert!(filter.keeps(&one), "{needle}");
        }

        let filter = Filter {
            search: "iceland".to_owned(),
            ..Default::default()
        };
        assert!(!filter.keeps(&one));
    }

    /// The one place this deliberately differs from v1, which matched the
    /// typed line as one piece.
    #[test]
    fn every_word_typed_has_to_appear_but_not_together() {
        let mut one = photo("a.jpg");
        one.organisation.title = Some("Waterfall".to_owned());
        one.organisation.keywords = vec!["Iceland".to_owned()];

        let filter = Filter {
            search: "iceland waterfall".to_owned(),
            ..Default::default()
        };
        assert!(filter.keeps(&one));

        let filter = Filter {
            search: "iceland volcano".to_owned(),
            ..Default::default()
        };
        assert!(!filter.keeps(&one), "every word has to be there");
    }

    #[test]
    fn the_facets_offer_only_what_the_folder_holds() {
        let mut nikon = photo("a.jpg");
        nikon.camera = Some("NIKON D90".to_owned());
        nikon.organisation.rating = 3;
        nikon.organisation.label = ColorLabel::Red;

        let mut phone = photo("b.png");
        phone.camera = Some("HUAWEI VOG-L29".to_owned());
        phone.taken_at = Some(9_000);

        let facets = Facets::of(&[nikon, phone]);
        assert_eq!(facets.formats, ["jpg", "png"]);
        assert_eq!(facets.cameras, ["HUAWEI VOG-L29", "NIKON D90"]);
        assert!(facets.lenses.is_empty(), "no lens here, so none offered");
        assert_eq!(facets.highest_rating, 3);
        assert_eq!(facets.taken, Some((1_000, 9_000)));
        assert_eq!(facets.labels, [ColorLabel::None, ColorLabel::Red]);
    }

    #[test]
    fn an_empty_folder_offers_nothing_and_does_not_panic() {
        let facets = Facets::of(&[]);
        assert!(facets.formats.is_empty());
        assert_eq!(facets.highest_rating, 0);
        assert_eq!(facets.taken, None);
    }

    #[test]
    fn a_filter_that_is_set_says_so() {
        let quiet = Filter::default();
        assert_eq!(quiet.describe(), t("filter-none"));

        let loud = Filter {
            minimum_rating: 3,
            hide_rejected: true,
            ..Default::default()
        };
        let described = loud.describe();
        assert_ne!(described, t("filter-none"));
        assert!(described.contains('3'), "{described}");
    }
}
