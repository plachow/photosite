//! The menu on a tile: what can be done to the photographs selected, where
//! the pointer already is.
//!
//! Every entry is a command from the registry, so the menu and the keys
//! cannot disagree about what a thing is called, what it does or which key
//! does it too. The recent folders and the programs that open a photograph
//! are the only things here that are not commands, because they are not
//! known until the menu opens.
//!
//! Explorer's rule decides what the menu is about. A right-click on a tile
//! outside the selection makes that tile the selection; a right-click inside
//! it leaves the selection alone, so the menu speaks for all of them. The
//! opposite is how somebody copies forty photographs having meant one.

use crate::{App, shell};
use eframe::egui;
use photosite_core::t;
use photosite_core::transfer::Mode;
use std::path::Path;

/// A right-click on the tile at this position.
pub fn pointed_at(app: &mut App, at: usize) {
    if app.is_selected(at) {
        app.selected = Some(at);
    } else {
        app.select_only(at);
    }

    // Asked again for every menu: a program installed since the last one
    // belongs on it.
    app.open_with = None;
}

/// Whatever a tile's response needs to open the menu, and the menu itself.
///
/// Called after the tiles are drawn, not while they are: what the menu does
/// can reorder the folder, and the loop drawing it must not be the one to
/// find out.
pub fn attach(app: &mut App, at: usize, response: &egui::Response) {
    if response.secondary_clicked() {
        pointed_at(app, at);
    }

    response.context_menu(|ui| show(app, ui));
}

/// Does this tile's response need [`attach`] this frame?
pub fn wanted(response: &egui::Response) -> bool {
    response.secondary_clicked() || response.context_menu_opened()
}

fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // Wide enough for a folder's path without every entry wrapping.
    ui.set_min_width(220.0);

    command(app, ui, &ctx, "file.copy", true);
    destinations(app, ui, &ctx, Mode::Copy);
    destinations(app, ui, &ctx, Mode::Move);

    ui.separator();
    // Greyed rather than left out: a menu whose entries come and go by what
    // is selected is a menu nobody learns.
    let turnable = app.turnable() > 0;
    command(app, ui, &ctx, "photo.rotate_left", turnable);
    command(app, ui, &ctx, "photo.rotate_right", turnable);

    if shell::AVAILABLE {
        ui.separator();
        open_with(app, ui, &ctx);
        command(app, ui, &ctx, "file.system_menu", true);
    }
}

/// One command, named and keyed the way the registry has it.
fn command(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, id: &str, enabled: bool) {
    let Some(command) = photosite_core::commands::command(id) else {
        tracing::error!(command = id, "the menu names a command that does not exist");
        return;
    };

    if ui
        .add_enabled(enabled, button(app, command.title(), id))
        .clicked()
    {
        app.run(id, ctx);
        ui.close();
    }
}

fn button<'a>(app: &App, title: String, id: &str) -> egui::Button<'a> {
    let button = egui::Button::new(title);
    match app.shortcut_label(id) {
        Some(shortcut) => button.shortcut_text(shortcut),
        None => button,
    }
}

/// Copy to, or move to: the folders used lately, and a dialog for anywhere
/// else. With nothing used yet it is the dialog and nothing more, rather
/// than a submenu with one entry in it.
fn destinations(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, mode: Mode) {
    let (id, title) = match mode {
        Mode::Copy => ("file.copy_to", t!("menu-copy-to")),
        Mode::Move => ("file.move_to", t!("menu-move-to")),
    };
    let recent = app.settings.gallery.recent_destinations.clone();
    if recent.is_empty() {
        return command(app, ui, ctx, id, true);
    }

    ui.menu_button(title, |ui| {
        for folder in &recent {
            if ui
                .button(elided(folder, 64))
                .on_hover_text(folder)
                .clicked()
            {
                app.send_to(Path::new(folder), mode);
                ui.close();
            }
        }

        ui.separator();
        if ui.add(button(app, t!("menu-choose-folder"), id)).clicked() {
            app.run(id, ctx);
            ui.close();
        }
    });
}

/// The programs Windows would offer, asked for once when the submenu first
/// opens and kept until the next menu.
fn open_with(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    let title = photosite_core::commands::command("file.open_with")
        .map(|command| command.title())
        .unwrap_or_default();
    ui.menu_button(title, |ui| {
        let Some(extension) = app.menu_extension() else {
            return;
        };

        let stale = app
            .open_with
            .as_ref()
            .is_none_or(|(asked, _)| *asked != extension);
        if stale {
            app.open_with = Some((extension.clone(), shell::handlers(&extension)));
        }

        let handlers = app
            .open_with
            .as_ref()
            .map(|(_, handlers)| handlers.clone())
            .unwrap_or_default();
        if handlers.is_empty() {
            ui.add_enabled(false, egui::Button::new(t!("menu-no-apps")));
        }

        for handler in handlers {
            if ui.button(&handler.title).clicked() {
                app.open_in(&handler, &extension);
                ui.close();
            }
        }

        ui.separator();
        if ui.button(t!("menu-other-app")).clicked() {
            app.run("file.open_with", ctx);
            ui.close();
        }
    });
}

/// A long path with its middle taken out. The end is kept longer than the
/// start: the folder's own name is the part somebody is looking for.
pub(crate) fn elided(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max || max < 8 {
        return text.to_owned();
    }

    let keep = max - 1;
    let head = keep / 3;
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}\u{2026}{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_path_is_left_as_it_is() {
        assert_eq!(elided(r"D:\Foto\Keep", 64), r"D:\Foto\Keep");
    }

    #[test]
    fn a_long_path_keeps_its_ends_and_the_folder_name_whole() {
        let long = format!(r"D:\{}\Holiday on Šumava", "deep\\".repeat(20));
        let short = elided(&long, 40);
        assert_eq!(short.chars().count(), 40);
        assert!(short.starts_with(r"D:\deep"), "{short}");
        assert!(short.ends_with(r"\Holiday on Šumava"), "{short}");
        assert!(short.contains('\u{2026}'));
    }

    /// Right-clicking inside the selection keeps it — the menu is about all
    /// of them — and outside it starts a new one.
    #[test]
    fn a_right_click_inside_the_selection_keeps_it_and_outside_replaces_it() {
        let (mut app, _data, _photos) = crate::culling::three();
        app.select_only(0);
        app.select_also(1);

        pointed_at(&mut app, 1);
        assert_eq!(app.selection.iter().copied().collect::<Vec<_>>(), [0, 1]);
        assert_eq!(app.selected, Some(1));

        pointed_at(&mut app, 2);
        assert_eq!(app.selection.iter().copied().collect::<Vec<_>>(), [2]);
    }

    /// Every piece of text a frame drew.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, into: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, into)),
                _ => {}
            }
        }

        let mut found = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// A frame of the gallery, with whatever the pointer did in it.
    fn a_frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
        let palette = *app.palette();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1200.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| crate::grid::gallery(app, ui, &palette));
        // With no renderer nothing is ever uploaded, and epaint says so with
        // a panic when the output is dropped.
        out.textures_delta.clear();
        texts(&out.shapes)
    }

    fn right_button(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// The whole of it, the way a hand does it: a right-click on a tile
    /// that was not selected selects it and opens the menu over it.
    #[test]
    fn a_right_click_on_a_tile_opens_the_menu_on_that_tile() {
        let (mut app, _data, _photos) = crate::culling::three();
        app.select_only(2);
        let ctx = egui::Context::default();
        // The first tile, well inside it.
        let at = egui::pos2(60.0, 60.0);

        a_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        a_frame(&mut app, &ctx, vec![right_button(at, true)]);
        a_frame(&mut app, &ctx, vec![right_button(at, false)]);
        let drawn = a_frame(&mut app, &ctx, Vec::new());

        assert_eq!(app.selection.iter().copied().collect::<Vec<_>>(), [0]);
        for wanted in [
            "Copy",
            "Copy to…",
            "Move to…",
            "Rotate left",
            "Rotate right",
        ] {
            assert!(
                drawn.iter().any(|text| text == wanted),
                "{wanted} is not on the menu: {drawn:?}"
            );
        }
        assert_eq!(
            drawn.iter().any(|text| text == "Show the system menu"),
            shell::AVAILABLE
        );
    }
}
