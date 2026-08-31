//! The dock layout.
//!
//! The application's panes are not wired side by side in the drawing layer.
//! They are described by a **tree that is data**. Today's arrangement is only
//! that tree's default value; "photo details under the preview" is a change
//! to one string, not a change to how anything is drawn.
//!
//! ```text
//! h(0.16, tree, h(0.66, gallery, v(0.62, preview, info)))
//!  │      │                       └ stacked: preview on top, details below
//!  │      └ side by side: tree on the left, the rest on the right
//!  └ the first part's share; the second gets what is left
//! ```
//!
//! The form is textual on purpose. It goes into the settings on one line, it
//! can be read and corrected by hand, and a diff shows at a glance — nested
//! TOML tables three levels deep would be unreadable.
//!
//! **A dock must not be closable by accident.** Every one has a minimum size
//! and the splitter will not go below it. Before that held, the preview pane
//! could be dragged down to eight pixels, it was saved to the settings, and
//! nothing brought it back: clicking tiles still worked, there was simply
//! nowhere to draw.

use std::collections::HashSet;
use std::fmt;

/// One pane that can be placed into the layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dock {
    /// The stable key. This is what goes into the settings, not the name —
    /// the name may be translated at any time.
    pub id: &'static str,
    /// A translation key. There is no text here either.
    pub title_key: &'static str,
    /// Minimum size in points. The splitter will not go below it.
    pub min: f64,
}

pub const DOCKS: &[Dock] = &[
    Dock {
        id: "tree",
        title_key: "dock-tree",
        min: 120.0,
    },
    Dock {
        id: "gallery",
        title_key: "dock-gallery",
        min: 240.0,
    },
    Dock {
        id: "preview",
        title_key: "dock-preview",
        min: 160.0,
    },
    Dock {
        id: "info",
        title_key: "dock-info",
        min: 90.0,
    },
];

pub fn dock(id: &str) -> Option<&'static Dock> {
    DOCKS.iter().find(|dock| dock.id == id)
}

/// The default layout: tree on the left, grid in the middle, preview and
/// details in a column on the right.
pub const DEFAULT: &str = "h(0.16, tree, h(0.66, gallery, v(0.62, preview, info)))";

/// How the two parts are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side; the splitter is vertical.
    Across,
    /// Stacked; the splitter is horizontal.
    Down,
}

impl Axis {
    fn tag(self) -> char {
        match self {
            Axis::Across => 'h',
            Axis::Down => 'v',
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Pane(String),
    Split {
        axis: Axis,
        /// The first part's share, 0 to 1.
        ratio: f64,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Layout::Pane(id) => f.write_str(id),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => write!(f, "{}({ratio:.2}, {first}, {second})", axis.tag()),
        }
    }
}

impl Layout {
    /// Every pane, left to right and top to bottom.
    pub fn panes(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
    }

    fn collect<'a>(&'a self, into: &mut Vec<&'a str>) {
        match self {
            Layout::Pane(id) => into.push(id),
            Layout::Split { first, second, .. } => {
                first.collect(into);
                second.collect(into);
            }
        }
    }

    /// Is anything in this branch visible? A branch with everything hidden
    /// gets neither space nor a splitter.
    pub fn visible(&self, hidden: &HashSet<&str>) -> bool {
        match self {
            Layout::Pane(id) => !hidden.contains(id.as_str()),
            Layout::Split { first, second, .. } => first.visible(hidden) || second.visible(hidden),
        }
    }

    /// The smallest sensible size along an axis, splitters included.
    ///
    /// Along the same axis it adds up, across it takes the larger — two
    /// docks stacked each need their own height but share the width.
    pub fn min_along(&self, axis: Axis, hidden: &HashSet<&str>, splitter: f64) -> f64 {
        match self {
            Layout::Pane(id) => {
                if hidden.contains(id.as_str()) {
                    0.0
                } else {
                    dock(id).map(|dock| dock.min).unwrap_or(0.0)
                }
            }
            Layout::Split {
                axis: split,
                first,
                second,
                ..
            } => {
                if !first.visible(hidden) {
                    return second.min_along(axis, hidden, splitter);
                }

                if !second.visible(hidden) {
                    return first.min_along(axis, hidden, splitter);
                }

                let a = first.min_along(axis, hidden, splitter);
                let b = second.min_along(axis, hidden, splitter);
                if *split == axis {
                    a + b + splitter
                } else {
                    a.max(b)
                }
            }
        }
    }

    /// A share that keeps both parts above their minimum.
    ///
    /// When there is not room for both, the stored ratio wins — otherwise a
    /// dock would stick to the edge as the window shrank and never let go.
    pub fn clamp_ratio(first_min: f64, second_min: f64, total: f64, ratio: f64) -> f64 {
        let ratio = ratio.clamp(0.0, 1.0);
        if total <= 0.0 {
            return ratio;
        }

        let low = (first_min / total).clamp(0.0, 1.0);
        let high = (1.0 - second_min / total).clamp(0.0, 1.0);
        if low > high {
            return ratio;
        }

        ratio.clamp(low, high)
    }

    /// Overwrites the share at the node on the given path. The path is a
    /// sequence of turns: `false` is the first part, `true` the second.
    pub fn set_ratio(&mut self, path: &[bool], value: f64) {
        let Layout::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return;
        };

        match path.split_first() {
            None => *ratio = value.clamp(0.0, 1.0),
            Some((true, rest)) => second.set_ratio(rest, value),
            Some((false, rest)) => first.set_ratio(rest, value),
        }
    }
}

// -------------------------------------------------------------------- reading

/// Reads a layout. An error carries its reason, so the log shows what is
/// wrong.
pub fn parse(text: &str) -> Result<Layout, String> {
    let mut reader = Reader {
        text: text.as_bytes(),
        at: 0,
    };
    let layout = reader.layout()?;
    reader.space();
    if reader.at < reader.text.len() {
        return Err(format!("trailing text from character {}", reader.at));
    }

    check(&layout)?;
    Ok(layout)
}

/// The layout from the settings, or the default when it is broken.
///
/// An unreadable layout must neither bring the application down nor leave it
/// without a grid. Falling back to the default goes to the log — it must not
/// happen in silence.
pub fn parse_or_default(text: &str) -> Layout {
    let text = if text.trim().is_empty() {
        DEFAULT
    } else {
        text
    };
    match parse(text) {
        Ok(layout) => layout,
        Err(reason) => {
            tracing::warn!(
                layout = text,
                reason,
                "the layout makes no sense, taking the default"
            );
            parse(DEFAULT).expect("the default layout has to be valid")
        }
    }
}

fn check(layout: &Layout) -> Result<(), String> {
    let panes = layout.panes();
    for id in &panes {
        if dock(id).is_none() {
            return Err(format!("unknown pane {id}"));
        }
    }

    let mut seen = HashSet::new();
    for id in &panes {
        if !seen.insert(*id) {
            return Err(format!("pane {id} appears in the layout twice"));
        }
    }

    // The grid is the reason the application exists. A layout without it is
    // a typo.
    if !seen.contains("gallery") {
        return Err("the layout holds no grid".to_owned());
    }

    Ok(())
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.text.get(self.at).copied()
    }

    fn eat(&mut self, want: u8) -> Result<(), String> {
        if self.peek() != Some(want) {
            return Err(format!(
                "character {} is missing a {}",
                self.at, want as char
            ));
        }

        self.at += 1;
        Ok(())
    }

    fn word(&mut self) -> Result<String, String> {
        self.space();
        let from = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_alphanumeric() || self.text[self.at] == b'_')
        {
            self.at += 1;
        }

        if from == self.at {
            return Err(format!("character {} is missing a pane name", self.at));
        }

        Ok(String::from_utf8_lossy(&self.text[from..self.at]).into_owned())
    }

    fn number(&mut self) -> Result<f64, String> {
        self.space();
        let from = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_digit() || self.text[self.at] == b'.')
        {
            self.at += 1;
        }

        String::from_utf8_lossy(&self.text[from..self.at])
            .parse()
            .map_err(|_| format!("character {from} is missing a share"))
    }

    fn layout(&mut self) -> Result<Layout, String> {
        let word = self.word()?;
        let axis = match word.as_str() {
            "h" => Some(Axis::Across),
            "v" => Some(Axis::Down),
            _ => None,
        };

        // `h` and `v` are splits only when a bracket follows them.
        match axis.filter(|_| self.peek() == Some(b'(')) {
            None => Ok(Layout::Pane(word)),
            Some(axis) => {
                self.eat(b'(')?;
                let ratio = self.number()?;
                self.eat(b',')?;
                let first = self.layout()?;
                self.eat(b',')?;
                let second = self.layout()?;
                self.eat(b')')?;
                Ok(Layout::Split {
                    axis,
                    ratio: ratio.clamp(0.0, 1.0),
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }
}

// ------------------------------------------------------------- what is hidden

/// The hidden panes from the settings. An unknown name is dropped rather
/// than taking everything else with it.
pub fn hidden(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter(|id| dock(id).is_some())
        .map(str::to_owned)
        .collect()
}

/// Back into the form the settings use, in registry order — so the file does
/// not change merely because something was switched on and off again.
pub fn hidden_to_text(list: &[String]) -> String {
    DOCKS
        .iter()
        .map(|dock| dock.id)
        .filter(|id| list.iter().any(|hidden| hidden == id))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing() -> HashSet<&'static str> {
        HashSet::new()
    }

    #[test]
    fn the_default_layout_is_valid() {
        let layout = parse(DEFAULT).expect("the default layout has to parse");
        assert_eq!(layout.panes(), vec!["tree", "gallery", "preview", "info"]);
    }

    #[test]
    fn every_pane_in_the_registry_has_a_translation() {
        for dock in DOCKS {
            assert!(
                crate::i18n::has(dock.title_key),
                "pane {} points at the missing key {}",
                dock.id,
                dock.title_key
            );
        }
    }

    #[test]
    fn panes_are_not_named_like_splits() {
        // `h` and `v` are reserved in the notation; a pane with such a name
        // could not be told apart from a split.
        for dock in DOCKS {
            assert!(dock.id != "h" && dock.id != "v", "{}", dock.id);
        }
    }

    #[test]
    fn writing_and_reading_meet() {
        for text in [
            DEFAULT,
            "gallery",
            "v(0.50, gallery, info)",
            "h(0.30, v(0.50, tree, info), gallery)",
        ] {
            let layout = parse(text).unwrap();
            let again = layout.to_string();
            assert_eq!(parse(&again).unwrap(), layout, "{text} -> {again}");
        }
    }

    #[test]
    fn nonsense_neither_crashes_nor_loses_the_grid() {
        for text in [
            "",
            "h(0.5, gallery",
            "h(gallery, info)",
            "neznamo",
            "h(0.5, gallery, gallery)",
            "h(0.5, tree, info)",
            "((((",
        ] {
            let layout = parse_or_default(text);
            assert!(
                layout.panes().contains(&"gallery"),
                "{text} left the layout without a grid"
            );
        }
    }

    #[test]
    fn the_same_pane_twice_is_an_error() {
        assert!(parse("h(0.5, gallery, gallery)").is_err());
    }

    #[test]
    fn a_layout_without_a_grid_is_an_error() {
        assert!(parse("h(0.5, tree, preview)").is_err());
    }

    #[test]
    fn the_minimum_adds_along_the_axis_and_takes_the_larger_across() {
        let layout = parse("h(0.5, tree, gallery)").unwrap();
        let tree = dock("tree").unwrap().min;
        let gallery = dock("gallery").unwrap().min;
        assert_eq!(
            layout.min_along(Axis::Across, &nothing(), 6.0),
            tree + gallery + 6.0
        );
        assert_eq!(
            layout.min_along(Axis::Down, &nothing(), 6.0),
            tree.max(gallery)
        );
    }

    #[test]
    fn a_hidden_pane_holds_no_space() {
        let layout = parse("h(0.5, tree, gallery)").unwrap();
        let hidden = HashSet::from(["tree"]);
        assert_eq!(
            layout.min_along(Axis::Across, &hidden, 6.0),
            dock("gallery").unwrap().min,
            "a hidden dock must take no space and charge for no splitter"
        );
    }

    #[test]
    fn the_splitter_will_not_take_a_dock_below_its_minimum() {
        // This is exactly what used to be possible: the preview dragged to
        // eight pixels, saved to the settings, and nothing bringing it back.
        let ratio = Layout::clamp_ratio(240.0, 160.0, 1000.0, 0.99);
        assert!(ratio * 1000.0 <= 840.0 + 1e-9);
        assert!(1000.0 - ratio * 1000.0 >= 160.0 - 1e-9);

        let ratio = Layout::clamp_ratio(240.0, 160.0, 1000.0, 0.0);
        assert!(ratio * 1000.0 >= 240.0 - 1e-9);
    }

    #[test]
    fn in_a_tight_window_the_ratio_decides_not_the_minimums() {
        // When both will not fit, the share must not jam at the edge.
        let ratio = Layout::clamp_ratio(600.0, 600.0, 500.0, 0.4);
        assert!((ratio - 0.4).abs() < 1e-9);
    }

    #[test]
    fn a_share_can_be_overwritten_by_path() {
        let mut layout = parse(DEFAULT).unwrap();
        layout.set_ratio(&[true, true], 0.25);
        let Layout::Split { second, .. } = &layout else {
            panic!("the default layout should be a split")
        };
        let Layout::Split { second, .. } = second.as_ref() else {
            panic!("the second part should be a split")
        };
        let Layout::Split { ratio, .. } = second.as_ref() else {
            panic!("preview and details should be a split")
        };
        assert_eq!(*ratio, 0.25);
    }

    #[test]
    fn hidden_panes_there_and_back() {
        assert_eq!(hidden("info,preview"), vec!["info", "preview"]);
        assert_eq!(hidden(" info , , neznamo "), vec!["info"]);
        assert_eq!(hidden_to_text(&hidden("info,preview")), "preview,info");
        assert_eq!(hidden_to_text(&[]), "");
    }
}
