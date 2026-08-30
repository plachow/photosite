//! Jedno místo, kde jsou vypsané všechny příkazy aplikace.
//!
//! Menu, klávesové zkratky, nabídka „co umíš" i případná paleta příkazů čtou
//! odsud. Dodělat takový registr do hotového UI znamená projít každé tlačítko
//! zvlášť, takže je tu od začátku, i když má zatím pár položek.
//!
//! Zkratky se tu drží v neutrálním tvaru, ne v typech egui — jádro o žádné
//! grafické knihovně neví a nesmí vědět.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Do které skupiny příkaz patří. Určuje pořadí v menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    File,
    View,
    Help,
}

impl Group {
    /// Klíč do překladu, ne text. Jádro nesmí obsahovat nic, co je vidět.
    pub fn title_key(self) -> &'static str {
        match self {
            Group::File => "group-file",
            Group::View => "group-view",
            Group::Help => "group-help",
        }
    }

    /// Přeložený název skupiny.
    pub fn title(self) -> String {
        crate::i18n::t(self.title_key())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// Stabilní klíč. Do konfigurace se ukládá tenhle, ne název — název se
    /// smí kdykoliv přepsat nebo přeložit.
    pub id: &'static str,
    /// Klíč do překladu. V registru nejsou texty, jen odkazy na ně —
    /// jinak by se název nedal přeložit ani přepsat bez zásahu do kódu.
    pub title_key: &'static str,
    pub group: Group,
    pub default_shortcut: Option<&'static str>,
}

pub const COMMANDS: &[Command] = &[
    Command {
        id: "file.open_folder",
        title_key: "command-file-open-folder",
        group: Group::File,
        default_shortcut: Some("Ctrl+O"),
    },
    Command {
        id: "file.rescan",
        title_key: "command-file-rescan",
        group: Group::File,
        default_shortcut: Some("F5"),
    },
    Command {
        id: "file.quit",
        title_key: "command-file-quit",
        group: Group::File,
        default_shortcut: Some("Ctrl+Q"),
    },
    Command {
        id: "view.recursive",
        title_key: "command-view-recursive",
        group: Group::View,
        default_shortcut: Some("Ctrl+R"),
    },
    Command {
        id: "view.bigger_tiles",
        title_key: "command-view-bigger-tiles",
        group: Group::View,
        default_shortcut: Some("Ctrl+Plus"),
    },
    Command {
        id: "view.smaller_tiles",
        title_key: "command-view-smaller-tiles",
        group: Group::View,
        default_shortcut: Some("Ctrl+Minus"),
    },
    Command {
        id: "view.next_theme",
        title_key: "command-view-next-theme",
        group: Group::View,
        default_shortcut: Some("Ctrl+T"),
    },
    Command {
        id: "view.settings",
        title_key: "command-view-settings",
        group: Group::View,
        default_shortcut: Some("Ctrl+Comma"),
    },
    Command {
        id: "help.diagnostics",
        title_key: "command-help-diagnostics",
        group: Group::Help,
        default_shortcut: Some("Ctrl+Shift+D"),
    },
];

impl Command {
    /// Přeložený název příkazu.
    pub fn title(&self) -> String {
        crate::i18n::t(self.title_key)
    }
}

pub fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.id == id)
}

/// Klávesová zkratka nezávislá na toolkitu.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Název klávesy tak, jak ho píšeme: `O`, `F5`, `Plus`, `Escape`.
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
            return Err(format!("zkratka {text:?} nemá klávesu"));
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

/// Co je na co namapované. Výchozí stav plus to, co si člověk přenastavil.
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
                    // Výchozí zkratky jsou v kódu, takže tohle je chyba
                    // programátora — ale ne důvod, aby aplikace nenaběhla.
                    Err(error) => {
                        tracing::error!(command = command.id, %error, "špatná výchozí zkratka")
                    }
                }
            }
        }

        Self { by_command }
    }

    pub fn shortcut(&self, id: &str) -> Option<&Shortcut> {
        self.by_command.get(id)
    }

    /// Který příkaz patří téhle zkratce.
    pub fn command_for(&self, shortcut: &Shortcut) -> Option<&'static Command> {
        self.by_command
            .iter()
            .find(|(_, bound)| *bound == shortcut)
            .and_then(|(id, _)| command(id))
    }

    pub fn rebind(&mut self, id: &'static str, shortcut: Shortcut) {
        self.by_command.insert(id, shortcut);
    }

    /// Zkratky namapované na víc než jeden příkaz. Ať se to pozná při testu,
    /// a ne až tím, že jedna z nich tiše nefunguje.
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
    fn zkratka_tam_a_zpatky() {
        for text in ["Ctrl+O", "F5", "Ctrl+Shift+D", "Alt+Enter"] {
            let shortcut: Shortcut = text.parse().unwrap();
            assert_eq!(shortcut.to_string(), text, "{text}");
        }
    }

    #[test]
    fn zkratka_bez_klavesy_je_chyba() {
        assert!("Ctrl+".parse::<Shortcut>().is_err());
        assert!("".parse::<Shortcut>().is_err());
    }

    #[test]
    fn identifikatory_prikazu_jsou_jedinecne() {
        let mut ids: Vec<_> = COMMANDS.iter().map(|c| c.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "dva příkazy se stejným id");
    }

    #[test]
    fn vychozi_zkratky_se_nebijou() {
        let bindings = Bindings::defaults();
        assert!(
            bindings.conflicts().is_empty(),
            "{:?}",
            bindings.conflicts()
        );
    }

    #[test]
    fn vychozi_zkratky_jsou_vsechny_platne() {
        let bindings = Bindings::defaults();
        for command in COMMANDS.iter().filter(|c| c.default_shortcut.is_some()) {
            assert!(bindings.shortcut(command.id).is_some(), "{}", command.id);
        }
    }

    #[test]
    fn kazdy_prikaz_ma_preklad() {
        for command in COMMANDS {
            assert!(
                crate::i18n::has(command.title_key),
                "příkaz {} odkazuje na chybějící klíč {}",
                command.id,
                command.title_key
            );
        }
    }

    #[test]
    fn kazda_skupina_ma_preklad() {
        for group in [Group::File, Group::View, Group::Help] {
            assert!(crate::i18n::has(group.title_key()), "{:?}", group);
        }
    }

    #[test]
    fn zkratku_lze_najit_zpatky_na_prikaz() {
        let bindings = Bindings::defaults();
        let shortcut: Shortcut = "Ctrl+O".parse().unwrap();
        assert_eq!(
            bindings.command_for(&shortcut).unwrap().id,
            "file.open_folder"
        );
    }
}
