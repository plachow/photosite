//! Dragging photographs out of the window: into a folder, a letter, a photo
//! book, anything that takes a file let go over it.
//!
//! The drag itself is the system's (see [`crate::shell::drag_out`]); what is
//! here is deciding when one has begun and what it is of.
//!
//! **What is dragged follows Explorer's rule**, the same as the menu's: a
//! drag from a tile in the selection takes the whole selection, a drag from
//! one outside it makes that tile the selection and takes only it.
//!
//! **The system keeps the mouse while it drags.** The button is let go inside
//! the system's own loop and never reaches the window, so the toolkit would
//! go on believing it held down — and the next movement over a tile would
//! be a drag nobody started. The release is put back in by hand at the start
//! of the next frame; see [`App::let_go`].

use crate::{App, shell};
use eframe::egui;

/// How far the pointer must travel with the button down before a press is a
/// drag, in points. Windows' own distance, so a drag starts here when it
/// would in Explorer: no sooner, which would turn a careless click into a
/// drag, and no later, which feels like something stuck.
const DISTANCE: f32 = 4.0;

/// Has this tile just been dragged far enough to take it out of the window?
///
/// Only where the system can take it: elsewhere, a drag on a tile stays
/// what it was before, nothing at all.
pub fn wanted(ui: &egui::Ui, response: &egui::Response) -> bool {
    shell::AVAILABLE
        && response.dragged_by(egui::PointerButton::Primary)
        && ui.input(|input| {
            input
                .pointer
                .press_origin()
                .zip(input.pointer.latest_pos())
                .is_some_and(|(from, to)| from.distance(to) >= DISTANCE)
        })
}

/// Drags the photographs out, starting from the tile at this position.
/// Returns once they have been let go, wherever that was.
///
/// Called after the tiles are drawn, like the menu: the drag holds the
/// window until it ends, and the frame it holds should be a whole one.
pub fn start(app: &mut App, at: usize) {
    if !app.is_selected(at) {
        app.select_only(at);
    }

    let paths = app.chosen_paths();
    if paths.is_empty() {
        return;
    }

    let outcome = shell::drag_out(&paths);
    app.let_go = true;
    if let Err(error) = outcome {
        let error = format!("{error:#}");
        tracing::warn!(%error, "the photographs could not be dragged out");
        app.status = error;
    }
}

/// The release the system kept, given back to the toolkit before it reads
/// anything else of the frame.
pub fn give_back_release(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let Some((pos, modifiers)) =
        ctx.input(|input| Some((input.pointer.latest_pos()?, input.modifiers)))
    else {
        return;
    };
    // First, so that whatever the pointer did after the drag is read as
    // happening with the button up.
    raw.events.insert(
        0,
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
    );
}
