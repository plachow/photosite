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
    Go,
    Photo,
    Sort,
    View,
    Editor,
    Help,
}

impl Group {
    /// Every group there is. Listing the variants a second time in a test or
    /// a menu is how one of them ends up forgotten.
    pub const ALL: [Self; 7] = [
        Self::File,
        Self::Go,
        Self::Photo,
        Self::Sort,
        Self::View,
        Self::Editor,
        Self::Help,
    ];

    /// A translation key, not text. The core must hold nothing that is seen.
    pub fn title_key(self) -> &'static str {
        match self {
            Group::File => "group-file",
            Group::Go => "group-go",
            Group::Photo => "group-photo",
            Group::Sort => "group-sort",
            Group::View => "group-view",
            Group::Editor => "group-editor",
            Group::Help => "group-help",
        }
    }

    /// The group's translated name.
    pub fn title(self) -> String {
        crate::i18n::t(self.title_key())
    }
}

/// Where a command can be given.
///
/// The manager and the editor are two places with two sets of keys, and the
/// same key may mean a different thing in each: `Ctrl+F` opens the filter
/// over the grid and fills the screen with the photograph in the editor.
/// A shortcut is a conflict only when both of its commands can be reached
/// from the same place — which is what makes rebinding possible later
/// without every editor key having to avoid every manager key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Manager,
    Editor,
    /// Both — quitting, the settings, the diagnostics.
    Everywhere,
}

impl Scope {
    /// Can a command of this scope be reached from `place`?
    pub fn reaches(self, place: Scope) -> bool {
        self == Scope::Everywhere || place == Scope::Everywhere || self == place
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
    /// Where it can be given. See [`Scope`].
    pub scope: Scope,
}

pub const COMMANDS: &[Command] = &[
    Command {
        id: "file.open_folder",
        title_key: "command-file-open-folder",
        group: Group::File,
        default_shortcut: Some("Ctrl+O"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "file.rescan",
        title_key: "command-file-rescan",
        group: Group::File,
        default_shortcut: Some("F5"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "file.rename",
        title_key: "command-file-rename",
        group: Group::File,
        default_shortcut: Some("F2"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.duplicate",
        title_key: "command-file-duplicate",
        group: Group::File,
        default_shortcut: Some("Ctrl+D"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.delete",
        title_key: "command-file-delete",
        group: Group::File,
        default_shortcut: Some("Delete"),
        toolbar: false,
        scope: Scope::Manager,
    },
    // Copying and moving. `Ctrl+C` and `Ctrl+V` are the file manager's own
    // keys and mean the files themselves, not a list of their names.
    Command {
        id: "file.copy",
        title_key: "command-file-copy",
        group: Group::File,
        default_shortcut: Some("Ctrl+C"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.cut",
        title_key: "command-file-cut",
        group: Group::File,
        default_shortcut: Some("Ctrl+X"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.paste",
        title_key: "command-file-paste",
        group: Group::File,
        default_shortcut: Some("Ctrl+V"),
        toolbar: false,
        scope: Scope::Manager,
    },
    // Somewhere else, chosen now. Alt rather than Ctrl because the clipboard
    // already has Ctrl+C, and these are the same idea without the two steps.
    Command {
        id: "file.copy_to",
        title_key: "command-file-copy-to",
        group: Group::File,
        default_shortcut: Some("Alt+C"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.move_to",
        title_key: "command-file-move-to",
        group: Group::File,
        default_shortcut: Some("Alt+X"),
        toolbar: false,
        scope: Scope::Manager,
    },
    // And to wherever the last one went. Sorting a folder into three piles
    // is three dialogs otherwise, and two of them say the same thing.
    Command {
        id: "file.copy_again",
        title_key: "command-file-copy-again",
        group: Group::File,
        default_shortcut: Some("Ctrl+Shift+C"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.new_folder",
        title_key: "command-file-new-folder",
        group: Group::File,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.reveal",
        title_key: "command-file-reveal",
        group: Group::File,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "file.quit",
        title_key: "command-file-quit",
        group: Group::File,
        default_shortcut: Some("Ctrl+Q"),
        toolbar: false,
        scope: Scope::Everywhere,
    },
    // Where we are, which is not the same group as what is in front of us.
    Command {
        id: "go.back",
        title_key: "command-go-back",
        group: Group::Go,
        default_shortcut: Some("Alt+Left"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "go.forward",
        title_key: "command-go-forward",
        group: Group::Go,
        default_shortcut: Some("Alt+Right"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "go.up",
        title_key: "command-go-up",
        group: Group::Go,
        default_shortcut: Some("Alt+Up"),
        toolbar: false,
        scope: Scope::Manager,
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
        scope: Scope::Manager,
    },
    Command {
        id: "photo.rate_1",
        title_key: "command-photo-rate-1",
        group: Group::Photo,
        default_shortcut: Some("1"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.rate_2",
        title_key: "command-photo-rate-2",
        group: Group::Photo,
        default_shortcut: Some("2"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.rate_3",
        title_key: "command-photo-rate-3",
        group: Group::Photo,
        default_shortcut: Some("3"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.rate_4",
        title_key: "command-photo-rate-4",
        group: Group::Photo,
        default_shortcut: Some("4"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.rate_5",
        title_key: "command-photo-rate-5",
        group: Group::Photo,
        default_shortcut: Some("5"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.label_none",
        title_key: "command-photo-label-none",
        group: Group::Photo,
        default_shortcut: Some("0"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.label_red",
        title_key: "command-photo-label-red",
        group: Group::Photo,
        default_shortcut: Some("6"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.label_yellow",
        title_key: "command-photo-label-yellow",
        group: Group::Photo,
        default_shortcut: Some("7"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.label_green",
        title_key: "command-photo-label-green",
        group: Group::Photo,
        default_shortcut: Some("8"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.label_blue",
        title_key: "command-photo-label-blue",
        group: Group::Photo,
        default_shortcut: Some("9"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.label_purple",
        title_key: "command-photo-label-purple",
        group: Group::Photo,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
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
        scope: Scope::Manager,
    },
    Command {
        id: "photo.reject",
        title_key: "command-photo-reject",
        group: Group::Photo,
        default_shortcut: Some("X"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.select_all",
        title_key: "command-photo-select-all",
        group: Group::Photo,
        default_shortcut: Some("Ctrl+A"),
        toolbar: false,
        scope: Scope::Manager,
    },
    // Two to four photographs at once. It toggles: the same key that opens
    // the comparison closes it, so nobody has to hunt for the way out.
    Command {
        id: "photo.compare",
        title_key: "command-photo-compare",
        group: Group::Photo,
        default_shortcut: Some("Ctrl+K"),
        toolbar: false,
        scope: Scope::Manager,
    },
    // Tab does nothing outside a comparison. It is here rather than read
    // straight off the keyboard because every key this application answers
    // to is in this list — that is what makes the list worth having.
    Command {
        id: "photo.compare_next",
        title_key: "command-photo-compare-next",
        group: Group::Photo,
        default_shortcut: Some("Tab"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "photo.compare_previous",
        title_key: "command-photo-compare-previous",
        group: Group::Photo,
        default_shortcut: Some("Shift+Tab"),
        toolbar: false,
        scope: Scope::Manager,
    },
    // One photograph on its own, in a tab of its own. A double-click on a
    // tile does the same; the command is here so the key can be rebound.
    Command {
        id: "photo.edit",
        title_key: "command-photo-edit",
        group: Group::Photo,
        default_shortcut: Some("Enter"),
        toolbar: true,
        scope: Scope::Manager,
    },
    // Sorting is a choice among six, not six buttons. The commands exist so
    // the choice can be bound to a key and named in one place; the toolbar
    // draws it as one control, the way it already does the recursive
    // checkbox.
    // Converting a lot of them at once. v1's key, because somebody moving
    // between the two should not have to learn a new one.
    Command {
        id: "photo.batch",
        title_key: "command-photo-batch",
        group: Group::Photo,
        default_shortcut: Some("Ctrl+B"),
        toolbar: true,
        scope: Scope::Manager,
    },
    // Asking a model on this machine what is in them. Ctrl+Shift+A, next
    // to Ctrl+A which selects what it will run over.
    Command {
        id: "photo.describe",
        title_key: "command-photo-describe",
        group: Group::Photo,
        default_shortcut: Some("Ctrl+Shift+A"),
        toolbar: true,
        scope: Scope::Manager,
    },
    // Who is in the photographs. It is a window rather than a dock: naming a
    // library is a sitting somebody does once and then rarely, and a pane
    // that is empty nine days in ten is a pane in the way.
    Command {
        id: "photo.people",
        title_key: "command-photo-people",
        group: Group::Photo,
        default_shortcut: Some("Ctrl+Shift+P"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.taken",
        title_key: "sort-taken",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.name",
        title_key: "sort-name",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.rating",
        title_key: "sort-rating",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.modified",
        title_key: "sort-modified",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.size",
        title_key: "sort-size",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.dimensions",
        title_key: "sort-dimensions",
        group: Group::Sort,
        default_shortcut: None,
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "sort.reverse",
        title_key: "command-sort-reverse",
        group: Group::Sort,
        default_shortcut: Some("Ctrl+Shift+R"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "view.recursive",
        title_key: "command-view-recursive",
        group: Group::View,
        default_shortcut: Some("Ctrl+R"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "view.filter",
        title_key: "command-view-filter",
        group: Group::View,
        default_shortcut: Some("Ctrl+F"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "view.clear_filter",
        title_key: "command-view-clear-filter",
        group: Group::View,
        default_shortcut: Some("Ctrl+Shift+F"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "view.toggle_tree",
        title_key: "command-view-toggle-tree",
        group: Group::View,
        default_shortcut: Some("Ctrl+1"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "view.toggle_preview",
        title_key: "command-view-toggle-preview",
        group: Group::View,
        default_shortcut: Some("Ctrl+2"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "view.toggle_info",
        title_key: "command-view-toggle-info",
        group: Group::View,
        default_shortcut: Some("Ctrl+3"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "view.reset_layout",
        title_key: "command-view-reset-layout",
        group: Group::View,
        default_shortcut: Some("Ctrl+0"),
        toolbar: false,
        scope: Scope::Manager,
    },
    Command {
        id: "view.bigger_tiles",
        title_key: "command-view-bigger-tiles",
        group: Group::View,
        default_shortcut: Some("Ctrl+Plus"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "view.smaller_tiles",
        title_key: "command-view-smaller-tiles",
        group: Group::View,
        default_shortcut: Some("Ctrl+Minus"),
        toolbar: true,
        scope: Scope::Manager,
    },
    // A bare F, the way every photo application spells it.
    Command {
        id: "view.as_list",
        title_key: "command-view-as-list",
        group: Group::View,
        default_shortcut: Some("Ctrl+L"),
        toolbar: true,
        scope: Scope::Manager,
    },
    Command {
        id: "view.fullscreen",
        title_key: "command-view-fullscreen",
        group: Group::View,
        default_shortcut: Some("F"),
        toolbar: false,
        scope: Scope::Everywhere,
    },
    Command {
        id: "view.next_theme",
        title_key: "command-view-next-theme",
        group: Group::View,
        default_shortcut: Some("Ctrl+T"),
        toolbar: true,
        scope: Scope::Everywhere,
    },
    Command {
        id: "view.settings",
        title_key: "command-view-settings",
        group: Group::View,
        default_shortcut: Some("Ctrl+Comma"),
        toolbar: true,
        scope: Scope::Everywhere,
    },
    // The editor's own keys. They are commands and not hard-wired keys so
    // that they can be rebound alongside everything else — and they share
    // keys with the manager freely, because nobody is ever in both at once.
    //
    // Closing asks nothing yet: the editor holds no edits. The check for
    // unsaved work belongs at the one place a tab is closed, not here.
    Command {
        id: "editor.close",
        title_key: "command-editor-close",
        group: Group::Editor,
        default_shortcut: Some("Escape"),
        toolbar: false,
        scope: Scope::Editor,
    },
    // Back to the manager standing on this photograph — the folder opened,
    // the tree unfolded to it and the tile chosen.
    Command {
        id: "editor.back",
        title_key: "command-editor-back",
        group: Group::Editor,
        default_shortcut: Some("Enter"),
        toolbar: false,
        scope: Scope::Editor,
    },
    Command {
        id: "editor.fullscreen",
        title_key: "command-editor-fullscreen",
        group: Group::Editor,
        default_shortcut: Some("Ctrl+F"),
        toolbar: false,
        scope: Scope::Editor,
    },
    // The next and the previous photograph of the folder, in the order the
    // manager shows them. The wheel does the same.
    Command {
        id: "editor.next",
        title_key: "command-editor-next",
        group: Group::Editor,
        default_shortcut: Some("PageDown"),
        toolbar: false,
        scope: Scope::Editor,
    },
    Command {
        id: "editor.previous",
        title_key: "command-editor-previous",
        group: Group::Editor,
        default_shortcut: Some("PageUp"),
        toolbar: false,
        scope: Scope::Editor,
    },
    // One pixel per point, and the whole photograph again: `*` and `0` on
    // the numeric keypad, as every viewer since ACDSee has had them. The
    // toolkit has no key called `*` — it arrives as typed text — and does
    // not tell the keypad's 0 from the row's, so both 0s fit. That is no
    // clash: the manager's 0 clears a label, and nobody is in both at once.
    Command {
        id: "editor.actual",
        title_key: "command-editor-actual",
        group: Group::Editor,
        default_shortcut: Some("*"),
        toolbar: false,
        scope: Scope::Editor,
    },
    Command {
        id: "editor.fit",
        title_key: "command-editor-fit",
        group: Group::Editor,
        default_shortcut: Some("0"),
        toolbar: false,
        scope: Scope::Editor,
    },
    // A step closer and a step back, about the middle of what is on
    // screen. The keypad's + and - arrive as the same keys as the row's.
    Command {
        id: "editor.zoom_in",
        title_key: "command-editor-zoom-in",
        group: Group::Editor,
        default_shortcut: Some("Plus"),
        toolbar: false,
        scope: Scope::Editor,
    },
    Command {
        id: "editor.zoom_out",
        title_key: "command-editor-zoom-out",
        group: Group::Editor,
        default_shortcut: Some("Minus"),
        toolbar: false,
        scope: Scope::Editor,
    },
    Command {
        id: "help.diagnostics",
        title_key: "command-help-diagnostics",
        group: Group::Help,
        default_shortcut: Some("Ctrl+Shift+D"),
        toolbar: true,
        scope: Scope::Everywhere,
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

    /// Which command this shortcut belongs to, from where somebody is.
    ///
    /// The place matters: `Enter` opens the editor from the manager and
    /// leaves it from the editor, and both are right.
    pub fn command_for(&self, shortcut: &Shortcut, place: Scope) -> Option<&'static Command> {
        self.by_command
            .iter()
            .filter(|(_, bound)| *bound == shortcut)
            .filter_map(|(id, _)| command(id))
            .find(|command| command.scope.reaches(place))
    }

    pub fn rebind(&mut self, id: &'static str, shortcut: Shortcut) {
        self.by_command.insert(id, shortcut);
    }

    /// Shortcuts bound to more than one command that can be reached from the
    /// same place. Better found by a test than by one of them quietly not
    /// working. The same key in the manager and in the editor is not a
    /// conflict: nobody is in both.
    pub fn conflicts(&self) -> Vec<Shortcut> {
        let mut found: Vec<Shortcut> = Vec::new();
        let bound: Vec<(&Command, &Shortcut)> = self
            .by_command
            .iter()
            .filter_map(|(id, shortcut)| command(id).map(|command| (command, shortcut)))
            .collect();
        for (at, (one, shortcut)) in bound.iter().enumerate() {
            let clashes = bound[at + 1..]
                .iter()
                .any(|(other, theirs)| theirs == shortcut && one.scope.reaches(other.scope));
            if clashes && !found.contains(shortcut) {
                found.push((*shortcut).clone());
            }
        }

        found
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
            bindings.command_for(&shortcut, Scope::Manager).unwrap().id,
            "file.open_folder"
        );
    }

    /// The same key, two places, two meanings — and neither is a conflict.
    #[test]
    fn a_key_means_one_thing_in_the_manager_and_another_in_the_editor() {
        let bindings = Bindings::defaults();
        for (text, manager, editor) in [
            ("Ctrl+F", "view.filter", "editor.fullscreen"),
            ("Enter", "photo.edit", "editor.back"),
        ] {
            let shortcut: Shortcut = text.parse().unwrap();
            assert_eq!(
                bindings.command_for(&shortcut, Scope::Manager).unwrap().id,
                manager,
                "{text} in the manager"
            );
            assert_eq!(
                bindings.command_for(&shortcut, Scope::Editor).unwrap().id,
                editor,
                "{text} in the editor"
            );
        }
    }

    /// Quitting is quitting wherever somebody is.
    #[test]
    fn a_command_for_everywhere_is_reached_from_both() {
        let bindings = Bindings::defaults();
        let shortcut: Shortcut = "Ctrl+Q".parse().unwrap();
        for place in [Scope::Manager, Scope::Editor] {
            assert_eq!(
                bindings.command_for(&shortcut, place).unwrap().id,
                "file.quit"
            );
        }
    }

    /// A manager key is not reachable from the editor: the filter does not
    /// open over a photograph.
    #[test]
    fn a_manager_key_does_nothing_in_the_editor() {
        let bindings = Bindings::defaults();
        let shortcut: Shortcut = "Ctrl+O".parse().unwrap();
        assert!(bindings.command_for(&shortcut, Scope::Editor).is_none());
    }

    /// Binding an editor key to something reachable from everywhere is a
    /// clash, and one from the manager alone is not.
    #[test]
    fn a_conflict_is_a_key_two_reachable_commands_share() {
        let mut bindings = Bindings::defaults();
        bindings.rebind("editor.close", "Ctrl+O".parse().unwrap());
        assert!(
            bindings.conflicts().is_empty(),
            "{:?}",
            bindings.conflicts()
        );

        bindings.rebind("editor.close", "Ctrl+Q".parse().unwrap());
        assert_eq!(
            bindings.conflicts(),
            vec!["Ctrl+Q".parse::<Shortcut>().unwrap()]
        );
    }

    #[test]
    fn every_editor_command_is_reached_only_from_the_editor() {
        for command in COMMANDS.iter().filter(|c| c.id.starts_with("editor.")) {
            assert_eq!(command.scope, Scope::Editor, "{}", command.id);
            assert_eq!(command.group, Group::Editor, "{}", command.id);
        }
    }
}
