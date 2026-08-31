//! Drawing the docks from the tree in [`photosite_core::docks`].
//!
//! This file knows nothing about any particular layout. It is handed a tree,
//! splits the window rectangle by it and passes each pane to its renderer.
//! Putting the details below the preview means changing a string in the
//! settings, not reaching in here.
//!
//! The splitter is the one place a layout changes, and it holds three rules:
//! it drags, a double-click returns it to half and half, and it **will not go
//! below a dock's minimum size**. The last of those is not cosmetic: the
//! preview could be dragged down to eight pixels, that was saved, and nothing
//! brought it back.

use crate::{App, grid, info, theme};
use eframe::egui;
use egui::Sense;
use photosite_core::docks::{Axis, Layout};
use photosite_core::t;
use photosite_core::theme::Palette;
use std::collections::HashSet;

/// A change a splitter made. Written only after drawing, so the tree does not
/// change in the middle of a walk.
struct Moved {
    path: Vec<bool>,
    ratio: f64,
}

/// What holds for the whole walk of the tree. Kept together so it is not
/// dragged down one argument at a time into every storey.
struct Board<'a> {
    palette: &'a Palette,
    hidden: HashSet<&'a str>,
    splitter: f64,
}

/// A splitter along with what dragging needs to know about it.
struct Bar {
    rect: egui::Rect,
    axis: Axis,
    /// How much room both parts have together, the splitter itself aside.
    usable: f64,
    ratio: f64,
}

pub fn show(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    // The tree and the hidden list are pulled out first; drawing borrows all
    // of `app`, and a borrow of its fields would block that.
    let layout = app.layout.clone();
    let closed = app.hidden.clone();
    let board = Board {
        palette,
        hidden: closed.iter().map(String::as_str).collect(),
        splitter: app.settings.window.splitter.clamp(2.0, 16.0),
    };
    let rect = ui.available_rect_before_wrap();

    // Everything hidden would leave an empty window with nothing to bring it
    // back.
    if !layout.visible(&board.hidden) {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(t!("docks-all-hidden")).color(theme::color(palette.dim)));
        });
        return;
    }

    let mut moved = None;
    let mut path = Vec::new();
    draw(app, ui, &board, &layout, rect, &mut path, &mut moved);

    if let Some(moved) = moved {
        app.layout.set_ratio(&moved.path, moved.ratio);
        app.settings.window.layout = app.layout.to_string();
    }
}

fn draw(
    app: &mut App,
    ui: &mut egui::Ui,
    board: &Board,
    node: &Layout,
    rect: egui::Rect,
    path: &mut Vec<bool>,
    moved: &mut Option<Moved>,
) {
    let Layout::Split {
        axis,
        ratio,
        first,
        second,
    } = node
    else {
        let Layout::Pane(id) = node else { return };
        return pane(app, ui, board.palette, id, rect);
    };

    // A hidden half takes no room and charges for no splitter.
    if !first.visible(&board.hidden) {
        return draw(app, ui, board, second, rect, path, moved);
    }

    if !second.visible(&board.hidden) {
        return draw(app, ui, board, first, rect, path, moved);
    }

    let along = match axis {
        Axis::Across => rect.width() as f64,
        Axis::Down => rect.height() as f64,
    };
    let usable = (along - board.splitter).max(0.0);
    let ratio = Layout::clamp_ratio(
        first.min_along(*axis, &board.hidden, board.splitter),
        second.min_along(*axis, &board.hidden, board.splitter),
        usable,
        *ratio,
    );

    let cut = (usable * ratio) as f32;
    let thick = board.splitter as f32;
    let (first_rect, bar, second_rect) = match axis {
        Axis::Across => (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + cut, rect.max.y)),
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x + cut, rect.min.y),
                egui::pos2(rect.min.x + cut + thick, rect.max.y),
            ),
            egui::Rect::from_min_max(egui::pos2(rect.min.x + cut + thick, rect.min.y), rect.max),
        ),
        Axis::Down => (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + cut)),
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.min.y + cut),
                egui::pos2(rect.max.x, rect.min.y + cut + thick),
            ),
            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + cut + thick), rect.max),
        ),
    };

    path.push(false);
    draw(app, ui, board, first, first_rect, path, moved);
    path.pop();

    handle(
        ui,
        board,
        &Bar {
            rect: bar,
            axis: *axis,
            usable,
            ratio,
        },
        path,
        moved,
    );

    path.push(true);
    draw(app, ui, board, second, second_rect, path, moved);
    path.pop();
}

/// The splitter: drag it, or double-click back to half and half.
fn handle(ui: &mut egui::Ui, board: &Board, bar: &Bar, path: &[bool], moved: &mut Option<Moved>) {
    let id = ui.id().with(("splitter", path));
    let response = ui.interact(bar.rect, id, Sense::click_and_drag());
    let cursor = match bar.axis {
        Axis::Across => egui::CursorIcon::ResizeHorizontal,
        Axis::Down => egui::CursorIcon::ResizeVertical,
    };
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(cursor);
    }

    // It lights up only under the mouse — otherwise the window would be cut
    // apart by glowing lines nobody asked to notice.
    let tint = if response.hovered() || response.dragged() {
        board.palette.accent
    } else {
        board.palette.bevel_dark
    };
    ui.painter().rect_filled(bar.rect, 0, theme::color(tint));

    if response.double_clicked() {
        *moved = Some(Moved {
            path: path.to_vec(),
            ratio: 0.5,
        });
        return;
    }

    if response.dragged() && bar.usable > 0.0 {
        let delta = match bar.axis {
            Axis::Across => response.drag_delta().x,
            Axis::Down => response.drag_delta().y,
        };
        if delta != 0.0 {
            *moved = Some(Moved {
                path: path.to_vec(),
                ratio: bar.ratio + delta as f64 / bar.usable,
            });
        }
    }
}

/// One pane. This is the only list that knows what each of them means.
fn pane(app: &mut App, ui: &mut egui::Ui, palette: &Palette, id: &str, rect: egui::Rect) {
    let fill = match id {
        "tree" => palette.panel,
        _ => palette.window,
    };
    ui.painter().rect_filled(rect, 0, theme::color(fill));

    let mut child = child_ui(ui, id, rect.shrink(if id == "tree" { 6.0 } else { 0.0 }));
    child.set_clip_rect(rect);
    match id {
        "tree" => grid::tree(app, &mut child, palette),
        "gallery" => grid::gallery(app, &mut child, palette),
        "preview" => grid::preview(app, &mut child, palette),
        "info" => info::pane(app, &mut child, palette),
        // The layout will not let an unknown pane through; if one ever got
        // here, an empty space beats a panic.
        other => tracing::warn!(pane = other, "pane with no renderer"),
    }
}

/// The space for one pane.
///
/// The `id` key here is not cosmetic. Without it egui gives every child of
/// the same parent the same salt — literally `"child"` — and tells them apart
/// only by the order they were created in. The scroll areas inside then reach
/// for one shared state, and the wheel over the folder tree moves the tiles in
/// the grid. Worse, hiding a single pane would shift the order and swap the
/// states around.
fn child_ui(ui: &mut egui::Ui, id: &str, rect: egui::Rect) -> egui::Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds two panes side by side, a scrolling list in each, sends the
    /// wheel over the left one and returns how far both moved.
    fn wheel_over_left(salt: bool) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut offsets = (0.0, 0.0);
        for _ in 0..2 {
            // Twice: the first pass only creates the panes, the second is
            // the one measured.
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(100.0, 300.0)));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });

            let mut out = ctx.run_ui(input, |ui| {
                let left =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 600.0));
                let right =
                    egui::Rect::from_min_size(egui::pos2(400.0, 0.0), egui::vec2(400.0, 600.0));
                for (name, rect, first) in [("left", left, true), ("right", right, false)] {
                    let mut child = if salt {
                        child_ui(ui, name, rect)
                    } else {
                        ui.new_child(egui::UiBuilder::new().max_rect(rect))
                    };
                    let out = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(&mut child, |ui| {
                            for row in 0..200 {
                                ui.label(format!("{row}"));
                            }
                        });
                    if first {
                        offsets.0 = out.state.offset.y;
                    } else {
                        offsets.1 = out.state.offset.y;
                    }
                }
            });

            // With no renderer the textures are never uploaded, and epaint
            // would point that out with a panic when the output is dropped.
            out.textures_delta.clear();
        }

        offsets
    }

    /// Scrolling happens where the mouse is. Nothing else may move.
    ///
    /// This was broken the very day the docks replaced the fixed panels: the
    /// wheel over the folder tree moved the tiles.
    #[test]
    fn the_wheel_moves_only_the_pane_under_the_mouse() {
        let (left, right) = wheel_over_left(true);
        assert!(left > 0.0, "the pane under the mouse did not move ({left})");
        assert_eq!(right, 0.0, "a pane the mouse was not over moved too");
    }

    /// The gauge itself: without its own key the scrolling really does run
    /// together, otherwise the test above would guard something that can
    /// never fail.
    #[test]
    fn without_its_own_key_the_scrolling_runs_together() {
        let (left, right) = wheel_over_left(false);
        assert_eq!(
            left, right,
            "without keys the panes behaved correctly — the test above then guards nothing"
        );
        assert!(
            right > 0.0,
            "nothing moved at all, so nothing is being measured"
        );
    }
}
