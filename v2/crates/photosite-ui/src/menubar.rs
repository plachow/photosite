//! The menu bar under the title bar, the way every desktop program has one.
//!
//! Every entry is a command from the registry, and the registry says where
//! each one goes: which menu (its group), where a line is drawn above it, and
//! which submenu it sits in. Nothing here asks about a command by name to
//! decide where it belongs — a menu drawn that way is rewritten with every
//! command added after it. What is decided here is only what the registry
//! cannot know: which menus come in which order, and whether an entry can be
//! chosen right now.
//!
//! A command that cannot be given from where somebody is — the manager's
//! commands while a photograph is in front — is shown and greyed rather than
//! left out, so the menus do not change shape under the hand.

use crate::App;
use eframe::egui;
use photosite_core::commands::{COMMANDS, Command, Group, Menu};
use photosite_core::t;

/// The menus, in the order every program has them. Sorting is not a menu of
/// its own but a submenu of View.
const MENUS: [Group; 7] = [
    Group::File,
    Group::Edit,
    Group::View,
    Group::Go,
    Group::Photo,
    Group::Editor,
    Group::Help,
];

pub fn bar(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    egui::MenuBar::new().ui(ui, |ui| {
        for group in MENUS {
            ui.menu_button(group.title(), |ui| {
                ui.set_min_width(240.0);
                entries(app, ui, &ctx, group);
                if group == Group::View {
                    ui.separator();
                    ui.menu_button(t!("menu-sort-by"), |ui| {
                        entries(app, ui, &ctx, Group::Sort);
                    });
                }
            });
        }
    });
}

/// One menu's entries, in the registry's order.
fn entries(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, group: Group) {
    let mine: Vec<&'static Command> = COMMANDS
        .iter()
        .filter(|command| command.group == group && command.menu != Menu::Hidden)
        .collect();

    let mut submenus_drawn: Vec<&str> = Vec::new();
    for (at, command) in mine.iter().enumerate() {
        match command.menu {
            Menu::Hidden => {}
            Menu::Item => entry(app, ui, ctx, command),
            Menu::Section => {
                if at > 0 {
                    ui.separator();
                }
                entry(app, ui, ctx, command);
            }
            // The submenu stands where the first of its commands would, and
            // holds all of them.
            Menu::Under(title) => {
                if submenus_drawn.contains(&title) {
                    continue;
                }

                submenus_drawn.push(title);
                let under: Vec<&'static Command> = mine
                    .iter()
                    .copied()
                    .filter(|other| other.menu == Menu::Under(title))
                    .collect();
                ui.menu_button(photosite_core::i18n::t(title), |ui| {
                    for command in under {
                        entry(app, ui, ctx, command);
                    }
                });
            }
        }
    }
}

/// One command: its name, its key, whether it can be chosen, and whether it
/// is switched on, for the ones that are switches.
fn entry(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, command: &Command) {
    let enabled = command.scope.reaches(app.scope()) && app.can_run(command.id);
    let shortcut = app.shortcut_label(command.id).unwrap_or_default();
    let chosen = match app.checked(command.id) {
        Some(mut on) => {
            let atoms = (
                command.title(),
                egui::Atom::grow(),
                egui::RichText::new(shortcut).weak(),
            );
            ui.add_enabled(enabled, egui::Checkbox::new(&mut on, atoms))
                .clicked()
        }
        None => ui
            .add_enabled(
                enabled,
                egui::Button::new(command.title()).shortcut_text(shortcut),
            )
            .clicked(),
    };

    if chosen {
        ui.close();
        app.run(command.id, ctx);
    }
}
