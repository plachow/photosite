//! The one place every command of the application is listed.
//!
//! Menus, keyboard shortcuts, a "what can you do" list and any future command
//! palette all read from here. Retrofitting such a registry into a finished
//! UI means going through every button one at a time, so it is here from the
//! start even while it holds only a handful of entries.
//!
//! Shortcuts are kept in a neutral form, not in egui types — the core knows
//! of no graphics library and must not.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Which group a command belongs to. It decides the order in a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    File,
    Photo,
    Sort,
    View,
    Help,
}

impl Group {
    /// Every group there is. Listing the variants a second time in a test or
    /// a menu is how one of them ends up forgotten.
    pub const ALL: [Self; 5] = [Self::File, Self::Photo, Self::Sort, Self::View, Self::Help];

    /// A translation key, not text. The core must hold nothing that is seen.
    pub fn title_key(self) -> &'static str {
        match self {
            Group::File => "group-file",
            Group::Photo => "group-photo",
            Group::Sort => "group-sort",
            Group::View => "group-view",
            Group::Help => "group-help",
        }
    }

    /// The group's translated name.
    pub fn title(self) -> String {
        crate::i18n::t(self.title_key())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The stable key. This is what the configuration stores, not the name —
    /// the name may be rewritten or translated at any time.
    pub id: &'static str,
    /// A translation key. The registry holds no text, only references to it
    /// — otherwise a name could be neither translated nor rewritten without
    /// touching code.
    pub title_key: &'static str,
    pub group: Group,
    pub default_shortcut: Option<&'static str>,
    /// Does it belong on the toolbar? Quitting the application and the
    /// "include subfolders" checkbox have no business there — and that is the
    /// registry's decision, not the drawing layer's. Otherwise the drawing
    /// layer asks by name ("everything in View except `view.recursive`") and
    /// that rule needs rewriting with every command added after it.
    pub toolbar: bool,
}

pub const COMMANDS: &[Command] = &[
    Command {
        id: "file.open_folder",
        title_key: "command-file-open-folder",
        group: Group::File,
        default_shortcut: Some("Ctrl+O"),
        toolbar: true,
    },
    Command {
        id: "file.rescan",
        title_key: "command-file-rescan",
        group: Group::File,
        default_shortcut: Some("F5"),
        toolbar: true,
    },
    Command {
        id: "file.quit",
        title_key: "command-file-quit",
        group: Group::File,
        default_shortcut: Some("Ctrl+Q"),
        toolbar: false,
    },
    // What somebody says about a photograph. None of these is on the
    // toolbar: twelve buttons for the stars and the labels would crowd out
    // everything else, and the keys are how anybody culls a folder anyway.
    //
    // The keys follow v1 exactly, purple included — it has none there
    // either, because 6..9 and 0 are five keys for five labels only if
    // clearing is not one of them.
    Command {
        id: "photo.rate_0",
        title_key: "command-photo-rate-0",
        group: Group::Photo,
        default_shortcut: Some("Backtick"),
        toolbar: false,
    },
    Command {
        id: "photo.rate_1",
        title_key: "command-photo-rate-1",
        group: Group::Photo,
        default_shortcut: Some("1"),
        toolbar: false,
    },
    Command {
        id: "photo.rate_2",
        title_key: "command-photo-rate-2",
        group: Group::Photo,
        default_shortcut: Some("2"),
        toolbar: false,
    },
    Command {
        id: "photo.rate_3",
        title_key: "command-photo-rate-3",
        group: Group::Photo,
        default_shortcut: Some("3"),
        toolbar: false,
    },
    Command {
        id: "photo.rate_4",
        title_key: "command-photo-rate-4",
        group: Group::Photo,
        default_shortcut: Some("4"),
        toolbar: false,
    },
    Command {
        id: "photo.rate_5",
        title_key: "command-photo-rate-5",
        group: Group::Photo,
        default_shortcut: Some("5"),
        toolbar: false,
    },
    Command {
        id: "photo.label_none",
        title_key: "command-photo-label-none",
        group: Group::Photo,
        default_shortcut: Some("0"),
        toolbar: false,
    },
    Command {
        id: "photo.label_red",
        title_key: "command-photo-label-red",
        group: Group::Photo,
        default_shortcut: Some("6"),
        toolbar: false,
    },
    Command {
        id: "photo.label_yellow",
        title_key: "command-photo-label-yellow",
        group: Group::Photo,
        default_shortcut: Some("7"),
        toolbar: false,
    },
    Command {
        id: "photo.label_green",
        title_key: "command-photo-label-green",
        group: Group::Photo,
        default_shortcut: Some("8"),
        toolbar: false,
    },
    Command {
        id: "photo.label_blue",
        title_key: "command-photo-label-blue",
        group: Group::Photo,
        default_shortcut: Some("9"),
        toolbar: false,
    },
    Command {
        id: "photo.label_purple",
        title_key: "command-photo-label-purple",
        group: Group::Photo,
        default_shortcut: None,
        toolbar: false,
    },
    // Both toggle. Pressing P on a photograph already picked takes the pick
    // off — otherwise there is no way back to undecided without the mouse,
    // and culling is done with one hand.
    Command {
        id: "photo.pick",
        title_key: "command-photo-pick",
        group: Group::Photo,
        default_shortcut: Some("P"),
        toolbar: false,
    },
    Command {
        id: "photo.reject",
        title_key: "command-photo-reject",
        group: Group::Photo,
        default_shortcut: Some("X"),
        toolbar: false,
    },
    Command {
        id: "photo.select_all",
        title_key: "command-photo-select-all",
        group: Group::Photo,
        default_shortcut: Some("Ctrl+A"),
        toolbar: false,
    },
    // Sorting is a choice among six, not six buttons. The commands exist so
    // the choice can be bound to a key and named in one place; the toolbar
    // draws it as one control, the way it already does the recursive
    // checkbox.
    Command {
        id: "sort.taken",
        title_key: "sort-taken",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
    },
    Command {
        id: "sort.name",
        title_key: "sort-name",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
    },
    Command {
        id: "sort.rating",
        title_key: "sort-rating",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
    },
    Command {
        id: "sort.modified",
        title_key: "sort-modified",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
    },
    Command {
        id: "sort.size",
        title_key: "sort-size",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
    },
    Command {
        id: "sort.dimensions",
        title_key: "sort-dimensions",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
    },
    Command {
        id: "sort.reverse",
        title_key: "command-sort-reverse",
        group: Group::Sort,
        default_shortcut: Some("Ctrl+Shift+R"),
        toolbar: false,
    },
    Command {
        id: "view.recursive",
        title_key: "command-view-recursive",
        group: Group::View,
        default_shortcut: Some("Ctrl+R"),
        toolbar: false,
    },
    Command {
        id: "view.filter",
        title_key: "command-view-filter",
        group: Group::View,
        default_shortcut: Some("Ctrl+F"),
        toolbar: false,
    },
    Command {
        id: "view.clear_filter",
        title_key: "command-view-clear-filter",
        group: Group::View,
        default_shortcut: Some("Ctrl+Shift+F"),
        toolbar: false,
    },
    Command {
        id: "view.toggle_tree",
        title_key: "command-view-toggle-tree",
        group: Group::View,
        default_shortcut: Some("Ctrl+1"),
        toolbar: true,
    },
    Command {
        id: "view.toggle_preview",
        title_key: "command-view-toggle-preview",
        group: Group::View,
        default_shortcut: Some("Ctrl+2"),
        toolbar: true,
    },
    Command {
        id: "view.toggle_info",
        title_key: "command-view-toggle-info",
        group: Group::View,
        default_shortcut: Some("Ctrl+3"),
        toolbar: true,
    },
    Command {
        id: "view.reset_layout",
        title_key: "command-view-reset-layout",
        group: Group::View,
        default_shortcut: Some("Ctrl+0"),
        toolbar: false,
    },
    Command {
        id: "view.bigger_tiles",
        title_key: "command-view-bigger-tiles",
        group: Group::View,
        default_shortcut: Some("Ctrl+Plus"),
        toolbar: true,
    },
    Command {
        id: "view.smaller_tiles",
        title_key: "command-view-smaller-tiles",
        group: Group::View,
        default_shortcut: Some("Ctrl+Minus"),
        toolbar: true,
    },
    Command {
        id: "view.next_theme",
        title_key: "command-view-next-theme",
        group: Group::View,
        default_shortcut: Some("Ctrl+T"),
        toolbar: true,
    },
    Command {
        id: "view.settings",
        title_key: "command-view-settings",
        group: Group::View,
        default_shortcut: Some("Ctrl+Comma"),
        toolbar: true,
    },
    Command {
        id: "help.diagnostics",
        title_key: "command-help-diagnostics",
        group: Group::Help,
        default_shortcut: Some("Ctrl+Shift+D"),
        toolbar: true,
    },
];

impl Command {
    /// The command's translated name.
    pub fn title(&self) -> String {
        crate::i18n::t(self.title_key)
    }
}

pub fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.id == id)
}

/// A keyboard shortcut, independent of any toolkit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The key's name as we write it: `O`, `F5`, `Plus`, `Escape`.
    pub key: String,
}

impl FromStr for Shortcut {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut shortcut = Shortcut {
            ctrl: false,
            shift: false,
            alt: false,
            key: String::new(),
        };
        for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" => shortcut.ctrl = true,
                "shift" => shortcut.shift = true,
                "alt" | "option" => shortcut.alt = true,
                _ => shortcut.key = part.to_owned(),
            }
        }

        if shortcut.key.is_empty() {
            return Err(format!("the shortcut {text:?} has no key"));
        }

        Ok(shortcut)
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }

        if self.shift {
            f.write_str("Shift+")?;
        }

        if self.alt {
            f.write_str("Alt+")?;
        }

        f.write_str(&self.key)
    }
}

/// What is bound to what: the defaults plus whatever has been rebound.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    by_command: HashMap<&'static str, Shortcut>,
}

impl Bindings {
    pub fn defaults() -> Self {
        let mut by_command = HashMap::new();
        for command in COMMANDS {
            if let Some(text) = command.default_shortcut {
                match text.parse::<Shortcut>() {
                    Ok(shortcut) => {
                        by_command.insert(command.id, shortcut);
                    }
                    // The default shortcuts live in code, so this is a
                    // programmer's mistake — but no reason for the
                    // application not to start.
                    Err(error) => {
                        tracing::error!(command = command.id, %error, "bad default shortcut")
                    }
                }
            }
        }

        Self { by_command }
    }

    pub fn shortcut(&self, id: &str) -> Option<&Shortcut> {
        self.by_command.get(id)
    }

    /// Which command this shortcut belongs to.
    pub fn command_for(&self, shortcut: &Shortcut) -> Option<&'static Command> {
        self.by_command
            .iter()
            .find(|(_, bound)| *bound == shortcut)
            .and_then(|(id, _)| command(id))
    }

    pub fn rebind(&mut self, id: &'static str, shortcut: Shortcut) {
        self.by_command.insert(id, shortcut);
    }

    /// Shortcuts bound to more than one command. Better found by a test than
    /// by one of them quietly not working.
    pub fn conflicts(&self) -> Vec<Shortcut> {
        let mut seen: HashMap<&Shortcut, usize> = HashMap::new();
        for shortcut in self.by_command.values() {
            *seen.entry(shortcut).or_default() += 1;
        }

        seen.into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(shortcut, _)| shortcut.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_there_and_back() {
        for text in ["Ctrl+O", "F5", "Ctrl+Shift+D", "Alt+Enter"] {
            let shortcut: Shortcut = text.parse().unwrap();
            assert_eq!(shortcut.to_string(), text, "{text}");
        }
    }

    #[test]
    fn a_shortcut_without_a_key_is_an_error() {
        assert!("Ctrl+".parse::<Shortcut>().is_err());
        assert!("".parse::<Shortcut>().is_err());
    }

    #[test]
    fn command_identifiers_are_unique() {
        let mut ids: Vec<_> = COMMANDS.iter().map(|c| c.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "two commands with the same id");
    }

    #[test]
    fn the_default_shortcuts_do_not_clash() {
        let bindings = Bindings::defaults();
        assert!(
            bindings.conflicts().is_empty(),
            "{:?}",
            bindings.conflicts()
        );
    }

    #[test]
    fn every_default_shortcut_is_valid() {
        let bindings = Bindings::defaults();
        for command in COMMANDS.iter().filter(|c| c.default_shortcut.is_some()) {
            assert!(bindings.shortcut(command.id).is_some(), "{}", command.id);
        }
    }

    #[test]
    fn every_command_has_a_translation() {
        for command in COMMANDS {
            assert!(
                crate::i18n::has(command.title_key),
                "command {} points at the missing key {}",
                command.id,
                command.title_key
            );
        }
    }

    #[test]
    fn every_group_has_a_translation() {
        for group in Group::ALL {
            assert!(crate::i18n::has(group.title_key()), "{:?}", group);
        }
    }

    #[test]
    fn a_shortcut_can_be_found_back_to_its_command() {
        let bindings = Bindings::defaults();
        let shortcut: Shortcut = "Ctrl+O".parse().unwrap();
        assert_eq!(
            bindings.command_for(&shortcut).unwrap().id,
            "file.open_folder"
        );
    }
}
