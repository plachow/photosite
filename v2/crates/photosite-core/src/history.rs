//! Where somebody has been, so that back and forward mean something.
//!
//! The same shape a browser has, and for the same reason: **going back and
//! then somewhere new throws away the forward trail.** Keeping it would offer
//! a "forward" that leads somewhere nobody was heading, which is worse than
//! offering nothing.

use std::path::{Path, PathBuf};

/// How many places are remembered.
///
/// Somebody browsing a library all afternoon should not accumulate a list
/// without end, and nobody has ever wanted the two hundredth step back.
const REMEMBERED: usize = 128;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    places: Vec<PathBuf>,
    /// Where in the list we are. Only meaningful when the list is not empty.
    at: usize,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where we are now.
    pub fn here(&self) -> Option<&Path> {
        self.places.get(self.at).map(PathBuf::as_path)
    }

    /// Somebody went somewhere.
    ///
    /// Going to where we already are changes nothing — otherwise reloading a
    /// folder would fill the history with the same place over and over, and
    /// "back" would walk through them one at a time.
    pub fn went(&mut self, folder: PathBuf) {
        if self.here() == Some(folder.as_path()) {
            return;
        }

        // Anything ahead of us is a trail nobody is on any more.
        self.places.truncate(self.at + 1);
        self.places.push(folder);

        if self.places.len() > REMEMBERED {
            let over = self.places.len() - REMEMBERED;
            self.places.drain(..over);
        }

        self.at = self.places.len() - 1;
    }

    pub fn can_go_back(&self) -> bool {
        self.at > 0 && !self.places.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        self.at + 1 < self.places.len()
    }

    pub fn back(&mut self) -> Option<&Path> {
        if !self.can_go_back() {
            return None;
        }

        self.at -= 1;
        self.here()
    }

    pub fn forward(&mut self) -> Option<&Path> {
        if !self.can_go_forward() {
            return None;
        }

        self.at += 1;
        self.here()
    }

    /// The folder above this one, where there is one.
    ///
    /// A drive root has no parent, and neither does a bare path with nothing
    /// in front of it. `Some("")` from `Path::parent` is not a folder, and
    /// opening it would land nowhere.
    pub fn up_from(folder: &Path) -> Option<PathBuf> {
        folder
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
    }

    /// The folder and each of its ancestors, outermost first — what the
    /// breadcrumb is drawn from.
    pub fn trail(folder: &Path) -> Vec<PathBuf> {
        let mut trail: Vec<PathBuf> = folder.ancestors().map(Path::to_path_buf).collect();
        trail.retain(|part| !part.as_os_str().is_empty());
        trail.reverse();
        trail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(history: &History) -> Option<String> {
        history
            .here()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn go(history: &mut History, where_to: &str) {
        history.went(PathBuf::from(where_to));
    }

    #[test]
    fn a_fresh_history_is_nowhere_and_goes_nowhere() {
        let mut history = History::new();
        assert_eq!(history.here(), None);
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), None);
    }

    #[test]
    fn back_and_forward_walk_the_trail() {
        let mut history = History::new();
        for place in ["/a", "/b", "/c"] {
            go(&mut history, place);
        }

        assert_eq!(at(&history).as_deref(), Some("/c"));
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        history.back();
        assert_eq!(at(&history).as_deref(), Some("/b"));
        history.back();
        assert_eq!(at(&history).as_deref(), Some("/a"));
        assert!(!history.can_go_back());

        history.forward();
        assert_eq!(at(&history).as_deref(), Some("/b"));
    }

    /// The rule every browser has. A forward that leads somewhere nobody was
    /// heading is worse than no forward at all.
    #[test]
    fn going_somewhere_new_throws_away_what_was_ahead() {
        let mut history = History::new();
        for place in ["/a", "/b", "/c"] {
            go(&mut history, place);
        }

        history.back();
        history.back();
        assert!(history.can_go_forward());

        go(&mut history, "/d");
        assert!(!history.can_go_forward(), "the old trail is still offered");
        assert_eq!(at(&history).as_deref(), Some("/d"));
        history.back();
        assert_eq!(at(&history).as_deref(), Some("/a"));
    }

    /// Reloading a folder must not fill the history with the same place, or
    /// "back" walks through a dozen copies of where you already are.
    #[test]
    fn going_where_we_already_are_changes_nothing() {
        let mut history = History::new();
        go(&mut history, "/a");
        go(&mut history, "/a");
        go(&mut history, "/a");
        assert!(!history.can_go_back());
    }

    #[test]
    fn the_list_does_not_grow_without_end() {
        let mut history = History::new();
        for step in 0..(REMEMBERED * 2) {
            go(&mut history, &format!("/place/{step}"));
        }

        assert_eq!(history.places.len(), REMEMBERED);
        assert_eq!(
            at(&history).as_deref(),
            Some(format!("/place/{}", REMEMBERED * 2 - 1).as_str())
        );
        // And the oldest ones are the ones that went.
        assert!(history.can_go_back());
    }

    #[test]
    fn up_stops_at_the_top() {
        assert_eq!(
            History::up_from(Path::new("/a/b/c")),
            Some(PathBuf::from("/a/b"))
        );
        assert_eq!(History::up_from(Path::new("/")), None);
        assert_eq!(History::up_from(Path::new("a")), None);
    }

    #[test]
    fn the_trail_reads_outermost_first() {
        let trail: Vec<String> = History::trail(Path::new("/a/b/c"))
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect();
        assert_eq!(trail, ["/", "/a", "/a/b", "/a/b/c"]);
    }

    #[test]
    fn the_trail_of_nowhere_is_empty_rather_than_a_blank() {
        assert!(History::trail(Path::new("")).is_empty());
    }
}
