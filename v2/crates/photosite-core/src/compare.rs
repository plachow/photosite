//! Two to four photographs side by side.
//!
//! The whole point of a comparison is that the eye does the work: the same
//! part of the same scene, at the same magnification, in every cell at once.
//! So the pan and the zoom are **one thing shared by all of them**, not one
//! each — and they are held as a fraction of the frame rather than in
//! pixels, so that photographs of different sizes still show the same part
//! of the scene.
//!
//! Nothing here draws. It works out how the space is divided, where each
//! photograph goes and which part of it is seen, and hands that back as
//! plain numbers. That is the half worth testing, and it is the half that is
//! wrong in every comparison view that has ever felt slippery.

use std::path::{Path, PathBuf};

/// Fewer than two and there is nothing to compare against.
pub const LEAST: usize = 2;

/// More than four and each is too small to judge — which is the one thing
/// looking at them side by side is for.
pub const MOST: usize = 4;

/// How far in it will go. Past this a photograph is a handful of coloured
/// squares, and no decode we could ask for would help.
pub const CLOSEST: f32 = 64.0;

/// What is being looked at. One of these for the whole comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Where in the frame the middle of the cell is, as a fraction of the
    /// photograph. `(0.5, 0.5)` is the middle of it.
    ///
    /// A fraction and not a pixel, because the photographs need not be the
    /// same size: two frames of one burst are, a phone shot and a RAW are
    /// not, and the same fraction is the same part of the scene either way.
    pub centre: (f32, f32),
    /// How much larger than "the whole of it fits" the photograph is drawn.
    /// `1.0` fits; `2.0` shows a quarter of the frame.
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self::FITTED
    }
}

impl View {
    pub const FITTED: Self = Self {
        centre: (0.5, 0.5),
        zoom: 1.0,
    };

    pub fn is_fitted(&self) -> bool {
        self.zoom <= 1.0
    }

    /// Zooms about a point in the photograph, given as a fraction of it.
    ///
    /// Whatever is under the pointer stays under the pointer. Zooming about
    /// the middle instead is the difference between examining a detail and
    /// chasing it around the cell with the mouse.
    pub fn zoom_about(self, factor: f32, point: (f32, f32)) -> Self {
        let zoom = (self.zoom * factor).clamp(1.0, CLOSEST);
        // What really happened, after the limits had their say. Using the
        // asked-for factor here would drift the centre on every notch of the
        // wheel once the zoom had stopped moving.
        let happened = zoom / self.zoom;
        Self {
            centre: (
                point.0 + (self.centre.0 - point.0) / happened,
                point.1 + (self.centre.1 - point.1) / happened,
            ),
            zoom,
        }
    }

    /// Moves by a fraction of the photograph.
    pub fn pan_by(self, delta: (f32, f32)) -> Self {
        Self {
            centre: (self.centre.0 + delta.0, self.centre.1 + delta.1),
            ..self
        }
    }
}

/// The view with the edges held, so nothing can be dragged out of sight.
///
/// Which is a question about one photograph in one cell: how much of the
/// frame that cell shows. A view that is legal for a wide photograph is not
/// necessarily legal for a tall one, so it is settled against the one that
/// has the focus — the one somebody is actually dragging.
pub fn settled(view: View, cell: (f32, f32), image: (f32, f32)) -> View {
    let zoom = if view.zoom.is_finite() {
        view.zoom.clamp(1.0, CLOSEST)
    } else {
        1.0
    };
    let seen = seen(cell, image, zoom);
    let hold = |middle: f32, seen: f32| {
        if !middle.is_finite() || seen >= 1.0 {
            // The whole width (or height) is on screen, so there is exactly
            // one place it can be: in the middle.
            0.5
        } else {
            middle.clamp(seen / 2.0, 1.0 - seen / 2.0)
        }
    };

    View {
        centre: (hold(view.centre.0, seen.0), hold(view.centre.1, seen.1)),
        zoom,
    }
}

/// What fraction of the photograph one cell shows, along each axis.
fn seen(cell: (f32, f32), image: (f32, f32), zoom: f32) -> (f32, f32) {
    let (cell_w, cell_h) = (cell.0.max(1.0), cell.1.max(1.0));
    let (image_w, image_h) = (image.0.max(1.0), image.1.max(1.0));
    let fit = (cell_w / image_w).min(cell_h / image_h);
    let drawn = (image_w * fit * zoom, image_h * fit * zoom);
    (share(cell_w, drawn.0), share(cell_h, drawn.1))
}

/// One axis of it.
///
/// Rounded up at the top end: `cell / (cell / image * image)` does not come
/// back as exactly one, and a fitted photograph missing its last hairline of
/// pixels is a rounding error that has been left in the picture.
fn share(cell: f32, drawn: f32) -> f32 {
    let share = cell / drawn.max(f32::MIN_POSITIVE);
    if share >= 1.0 - 1e-4 { 1.0 } else { share }
}

/// Where one photograph is drawn, and which part of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The part of the photograph shown, as fractions of it: left, top,
    /// width, height.
    pub source: [f32; 4],
    /// Where it lands in the cell, in the cell's own units and measured from
    /// its top left corner: left, top, width, height.
    pub target: [f32; 4],
}

/// Where a photograph of the given pixel size goes in a cell of the given
/// size, under the given view.
///
/// Fitted, the photograph is inscribed in the middle of the cell and all of
/// it is shown. Zoomed in, the cell is filled and a part of the frame is
/// shown instead. The two are the same arithmetic, which is why there is one
/// function rather than two cases.
pub fn frame(cell: (f32, f32), image: (f32, f32), view: View) -> Frame {
    let view = settled(view, cell, image);
    let (cell_w, cell_h) = (cell.0.max(1.0), cell.1.max(1.0));
    let (image_w, image_h) = (image.0.max(1.0), image.1.max(1.0));
    let fit = (cell_w / image_w).min(cell_h / image_h);
    let drawn = (image_w * fit * view.zoom, image_h * fit * view.zoom);
    let seen = seen(cell, image, view.zoom);

    // Never wider than the cell, and never wider than the photograph: a
    // fitted portrait leaves a margin either side, and that margin is the
    // difference between a photograph and a stretched one.
    let target = (drawn.0.min(cell_w), drawn.1.min(cell_h));
    let left = (view.centre.0 - seen.0 / 2.0).clamp(0.0, (1.0 - seen.0).max(0.0));
    let top = (view.centre.1 - seen.1 / 2.0).clamp(0.0, (1.0 - seen.1).max(0.0));

    Frame {
        source: [left, top, seen.0, seen.1],
        target: [
            (cell_w - target.0) / 2.0,
            (cell_h - target.1) / 2.0,
            target.0,
            target.1,
        ],
    }
}

/// The zoom at which one pixel of the photograph covers one point.
pub fn one_to_one(cell: (f32, f32), image: (f32, f32)) -> f32 {
    let fit = fit(cell, image);
    if fit > 0.0 {
        (1.0 / fit).clamp(1.0, CLOSEST)
    } else {
        1.0
    }
}

/// How large the photograph is drawn against its own pixels: `1.0` is one
/// pixel per point, which is what a photographer means by a hundred per cent.
pub fn magnification(cell: (f32, f32), image: (f32, f32), zoom: f32) -> f32 {
    fit(cell, image) * zoom
}

fn fit(cell: (f32, f32), image: (f32, f32)) -> f32 {
    (cell.0.max(1.0) / image.0.max(1.0)).min(cell.1.max(1.0) / image.1.max(1.0))
}

/// How many columns and rows the space is divided into.
///
/// Not a fixed table. Four portraits in a wide window want a single row;
/// four landscapes want two by two; and three of anything are usually better
/// in a two by two with one cell empty than squeezed into a single row. So
/// every arrangement is tried and the one that draws the photographs largest
/// wins — which is the only thing anybody opened a comparison for.
pub fn arrangement(count: usize, area: (f32, f32), aspect: f32) -> (usize, usize) {
    let count = count.clamp(1, MOST);
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let (width, height) = (area.0.max(1.0), area.1.max(1.0));

    let mut best = (count, 1);
    let mut largest = f32::NEG_INFINITY;
    for columns in 1..=count {
        let rows = count.div_ceil(columns);
        let cell = (width / columns as f32, height / rows as f32);
        // The height a photograph of this shape reaches in such a cell.
        let reach = (cell.0 / aspect).min(cell.1);
        // Not a strict `>`: a tie goes to the arrangement with more columns,
        // because side by side is what somebody asked for.
        if reach >= largest {
            largest = reach;
            best = (columns, rows);
        }
    }

    best
}

/// Two to four photographs, one of which has the focus.
#[derive(Debug, Clone, PartialEq)]
pub struct Compare {
    photos: Vec<PathBuf>,
    focus: usize,
    pub view: View,
}

impl Compare {
    /// Opens on what is chosen, or on nothing when there is not enough to
    /// compare.
    ///
    /// Beyond [`MOST`] the rest are left out rather than crowded in. The
    /// caller can see how many it handed over, and says so.
    pub fn open(chosen: &[PathBuf]) -> Option<Self> {
        if chosen.len() < LEAST {
            return None;
        }

        Some(Self {
            photos: chosen.iter().take(MOST).cloned().collect(),
            focus: 0,
            view: View::FITTED,
        })
    }

    pub fn photos(&self) -> &[PathBuf] {
        &self.photos
    }

    pub fn len(&self) -> usize {
        self.photos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.photos.is_empty()
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    /// The one the rating keys land on.
    pub fn focused(&self) -> &Path {
        // A comparison never holds nothing: `open` refuses fewer than two
        // and `drop_focused` closes rather than empty itself.
        &self.photos[self.focus.min(self.photos.len() - 1)]
    }

    pub fn focus_on(&mut self, at: usize) {
        if at < self.photos.len() {
            self.focus = at;
        }
    }

    /// Tab, and Shift+Tab. It wraps: four photographs are a ring, not a list
    /// with an end to fall off.
    pub fn move_focus(&mut self, by: isize) {
        let count = self.photos.len() as isize;
        if count > 0 {
            self.focus = (self.focus as isize + by).rem_euclid(count) as usize;
        }
    }

    /// Takes the focused photograph out of the comparison.
    ///
    /// The file is untouched: it is the comparison it leaves, not the disk.
    /// `false` when what is left is no longer a comparison at all, and the
    /// whole thing should close.
    pub fn drop_focused(&mut self) -> bool {
        if self.focus < self.photos.len() {
            self.photos.remove(self.focus);
        }

        self.focus = self.focus.min(self.photos.len().saturating_sub(1));
        self.photos.len() >= LEAST
    }

    pub fn holds(&self, path: &Path) -> bool {
        self.photos.iter().any(|held| held == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|n| PathBuf::from(format!("/p/{n}.jpg")))
            .collect()
    }

    #[test]
    fn one_photograph_is_not_a_comparison() {
        assert!(Compare::open(&paths(0)).is_none());
        assert!(Compare::open(&paths(1)).is_none());
        assert!(Compare::open(&paths(2)).is_some());
    }

    #[test]
    fn beyond_four_the_rest_are_left_out() {
        let compare = Compare::open(&paths(9)).unwrap();
        assert_eq!(compare.len(), MOST);
        assert_eq!(compare.focused(), Path::new("/p/0.jpg"));
    }

    #[test]
    fn the_focus_goes_round_rather_than_off_the_end() {
        let mut compare = Compare::open(&paths(3)).unwrap();
        compare.move_focus(1);
        compare.move_focus(1);
        assert_eq!(compare.focus(), 2);
        compare.move_focus(1);
        assert_eq!(compare.focus(), 0, "the focus fell off the end");
        compare.move_focus(-1);
        assert_eq!(compare.focus(), 2, "and off the front");
    }

    #[test]
    fn dropping_one_keeps_the_focus_on_something() {
        let mut compare = Compare::open(&paths(3)).unwrap();
        compare.focus_on(2);
        assert!(compare.drop_focused(), "two left is still a comparison");
        assert_eq!(compare.len(), 2);
        assert_eq!(
            compare.focused(),
            Path::new("/p/1.jpg"),
            "the focus was left pointing past the end"
        );
        assert!(!compare.drop_focused(), "one left is not a comparison");
    }

    /// A fitted photograph is shown whole, in the middle, with the margin on
    /// whichever pair of sides it belongs.
    #[test]
    fn fitted_shows_all_of_it_and_stretches_none_of_it() {
        // A landscape photograph in a square cell: margins above and below.
        let where_ = frame((400.0, 400.0), (3000.0, 2000.0), View::FITTED);
        assert_eq!(
            where_.source,
            [0.0, 0.0, 1.0, 1.0],
            "not all of it is shown"
        );
        assert_eq!(where_.target[2], 400.0);
        assert!((where_.target[3] - 400.0 * 2.0 / 3.0).abs() < 0.01);
        assert_eq!(where_.target[0], 0.0);
        assert!(where_.target[1] > 0.0, "it was not centred");

        // The same photograph turned on its side: margins left and right.
        let where_ = frame((400.0, 400.0), (2000.0, 3000.0), View::FITTED);
        assert_eq!(where_.source, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(where_.target[3], 400.0);
        assert!(where_.target[0] > 0.0);
    }

    #[test]
    fn zooming_in_shows_less_of_the_frame_and_fills_the_cell() {
        let cell = (400.0, 400.0);
        let image = (3000.0, 2000.0);
        let view = View {
            centre: (0.5, 0.5),
            zoom: 3.0,
        };
        let where_ = frame(cell, image, view);
        assert!(where_.source[2] < 0.4, "{:?}", where_.source);
        assert_eq!(
            where_.target,
            [0.0, 0.0, 400.0, 400.0],
            "the cell is not full"
        );
    }

    /// The bug this guards is the one everybody meets: drag hard enough and
    /// the photograph sails off, leaving an empty cell and no way back.
    #[test]
    fn the_photograph_cannot_be_dragged_out_of_the_cell() {
        let cell = (400.0, 400.0);
        let image = (3000.0, 2000.0);
        let view = View {
            centre: (0.5, 0.5),
            zoom: 4.0,
        }
        .pan_by((5.0, -5.0));
        let where_ = frame(cell, image, view);
        assert!(where_.source[0] >= 0.0, "{:?}", where_.source);
        assert!(where_.source[1] >= 0.0, "{:?}", where_.source);
        assert!(where_.source[0] + where_.source[2] <= 1.0 + 1e-4);
        assert!(where_.source[1] + where_.source[3] <= 1.0 + 1e-4);
    }

    #[test]
    fn a_fitted_photograph_has_nowhere_to_go() {
        let settled = settled(
            View::FITTED.pan_by((0.3, 0.3)),
            (400.0, 400.0),
            (3000.0, 2000.0),
        );
        assert_eq!(settled.centre, (0.5, 0.5), "a fitted photograph wandered");
    }

    /// Two photographs of different sizes, one view. They must show the same
    /// part of the scene — that is the entire point of the exercise.
    #[test]
    fn different_sizes_show_the_same_part_of_the_scene() {
        let cell = (400.0, 300.0);
        let view = View {
            centre: (0.3, 0.7),
            zoom: 2.5,
        };
        let small = frame(cell, (3000.0, 2000.0), view);
        let large = frame(cell, (6000.0, 4000.0), view);
        assert_eq!(
            small.source, large.source,
            "the same view showed different parts of the two"
        );
    }

    #[test]
    fn what_is_under_the_pointer_stays_under_the_pointer() {
        let point = (0.25, 0.75);
        let view = View::FITTED.zoom_about(4.0, point).zoom_about(2.0, point);
        // Everything shrinks towards the point, so the point itself is the
        // one place that has not moved: the centre closes on it by exactly
        // the factor zoomed.
        let expected = (
            point.0 + (0.5 - point.0) / 8.0,
            point.1 + (0.5 - point.1) / 8.0,
        );
        assert!((view.centre.0 - expected.0).abs() < 1e-5, "{view:?}");
        assert!((view.centre.1 - expected.1).abs() < 1e-5, "{view:?}");
        assert_eq!(view.zoom, 8.0);
    }

    #[test]
    fn zoom_stops_at_both_ends_without_dragging_the_centre_with_it() {
        let point = (0.2, 0.2);
        let mut view = View::FITTED;
        for _ in 0..40 {
            view = view.zoom_about(2.0, point);
        }

        assert_eq!(view.zoom, CLOSEST);
        let stuck = view.centre;
        view = view.zoom_about(2.0, point);
        assert_eq!(
            view.centre, stuck,
            "the centre drifted after the zoom stopped"
        );

        for _ in 0..40 {
            view = view.zoom_about(0.5, point);
        }

        assert_eq!(view.zoom, 1.0, "it zoomed out past fitting");
    }

    #[test]
    fn a_hundred_per_cent_is_one_pixel_to_a_point() {
        let cell = (400.0, 400.0);
        let image = (3000.0, 2000.0);
        let zoom = one_to_one(cell, image);
        let shown = magnification(cell, image, zoom);
        assert!((shown - 1.0).abs() < 1e-4, "{shown}");

        // And fitted is however much smaller the cell is.
        let fitted = magnification(cell, image, 1.0);
        assert!((fitted - 400.0 / 3000.0).abs() < 1e-4, "{fitted}");
    }

    /// The whole reason the arrangement is worked out rather than looked up.
    #[test]
    fn the_arrangement_is_whichever_draws_them_largest() {
        let wide = (1600.0, 900.0);
        assert_eq!(
            arrangement(2, wide, 1.5),
            (2, 1),
            "two went one above the other"
        );
        assert_eq!(
            arrangement(4, wide, 1.5),
            (2, 2),
            "four landscapes were not squared off"
        );
        assert_eq!(
            arrangement(3, wide, 1.5),
            (2, 2),
            "three were squeezed into a row rather than left a gap"
        );

        // Four portraits in the same window are better in one row: a two by
        // two would waste the height that makes them tall.
        assert_eq!(arrangement(4, wide, 0.4), (4, 1));

        // And a tall window turns it all on its side.
        assert_eq!(arrangement(2, (700.0, 1400.0), 1.5), (1, 2));
    }

    #[test]
    fn nothing_here_divides_by_zero() {
        for cell in [(0.0, 0.0), (400.0, 0.0), (-3.0, 12.0)] {
            for image in [(0.0, 0.0), (4000.0, 0.0)] {
                let where_ = frame(
                    cell,
                    image,
                    View {
                        centre: (0.5, 0.5),
                        zoom: 8.0,
                    },
                );
                assert!(
                    where_.source.iter().all(|value| value.is_finite()),
                    "{where_:?}"
                );
                assert!(
                    where_.target.iter().all(|value| value.is_finite()),
                    "{where_:?}"
                );
            }
        }

        assert!(one_to_one((0.0, 0.0), (0.0, 0.0)).is_finite());
        assert_eq!(arrangement(0, (0.0, 0.0), f32::NAN), (1, 1));
    }
}
