//! Two to four photographs at once, drawn in place of the docks.
//!
//! In place of, and not over: a comparison wants the whole window, which is
//! the reason for opening one. The toolbar and the trail stay — knowing
//! which folder this is does not stop being useful.
//!
//! The arithmetic is all in [`photosite_core::compare`]. What is left here
//! is turning a wheel notch and a drag into a view, and a view into
//! rectangles.

use crate::{App, Want, theme};
use eframe::egui;
use egui::{Sense, Vec2};
use photosite_core::compare::{self, Compare};
use photosite_core::t;
use photosite_core::theme::Palette;
use std::path::{Path, PathBuf};

/// How much one notch of the wheel zooms.
const NOTCH: f32 = 1.0015;

pub fn show(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    // Taken out and put back, so the drawing can reach the rest of the
    // application while it holds the comparison.
    let Some(mut compare) = app.compare.take() else {
        return;
    };

    let full = ui.available_rect_before_wrap();
    // The strip's height is claimed before the cells are laid out, because
    // the cell size is what a hundred per cent is measured against. Drawing
    // the strip first and asking it afterwards gave a percentage worked out
    // from the whole window — a quarter of the answer, in a two by two.
    let claimed = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0;
    let area = egui::Rect::from_min_max(egui::pos2(full.min.x, full.min.y + claimed), full.max);
    let sizes: Vec<(f32, f32)> = compare
        .photos()
        .iter()
        .map(|path| shown(app, path))
        .collect();
    // The shape the cells are cut to. An average, because there is one
    // arrangement for all of them and a portrait among landscapes has to
    // live somewhere.
    let aspect = sizes.iter().map(|(w, h)| w / h).sum::<f32>() / sizes.len().max(1) as f32;
    let (columns, rows) =
        compare::arrangement(compare.len(), (area.width(), area.height()), aspect);

    let gap = 4.0;
    let cell = Vec2::new(
        area.width() / columns as f32 - gap,
        area.height() / rows as f32 - gap,
    );

    let mut wanted: Vec<PathBuf> = Vec::new();
    let mut moved: Option<compare::View> = None;
    let mut focus: Option<usize> = None;

    for (index, path) in compare.photos().iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            area.min
                + Vec2::new(
                    gap / 2.0 + (index % columns) as f32 * (cell.x + gap),
                    gap / 2.0 + (index / columns) as f32 * (cell.y + gap),
                ),
            cell,
        );
        let frame = compare::frame((rect.width(), rect.height()), sizes[index], compare.view);
        let response = ui.interact(rect, ui.id().with(index), Sense::click_and_drag());
        if response.clicked() || response.drag_started() {
            focus = Some(index);
        }

        cell_background(ui, rect, palette, index == compare.focus());
        wanted.extend(draw(app, ui, rect, palette, path, &frame));
        caption(app, ui, rect, palette, path);

        // The wheel and the drag are read from the cell under the pointer,
        // and land on the one view every cell shares. Reading them from the
        // focused cell instead would mean pointing at one photograph and
        // moving another.
        if let Some(view) = steered(ui, &response, rect, &frame, compare.view) {
            moved = Some(view);
        }
    }

    if let Some(at) = focus {
        compare.focus_on(at);
    }

    if let Some(view) = moved {
        // Settled against the photograph with the focus: the shared view has
        // to be legal for the one somebody is dragging.
        let focused = shown(app, compare.focused());
        compare.view = compare::settled(view, (cell.x, cell.y), focused);
    }

    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("compare-strip")
            .max_rect(egui::Rect::from_min_max(
                full.min,
                egui::pos2(full.max.x, full.min.y + claimed),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    strip(app, &mut bar, palette, &mut compare, cell);

    app.wanted_close = wanted;
    app.wanted_preview.clear();

    // Dismissing what is in front of you is not a command, any more than
    // clicking a window's cross is. It is what Escape means everywhere, and
    // putting it in the registry would be claiming otherwise.
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        return;
    }

    app.compare = Some(compare);
}

/// The row above the photographs: how close we are, and how to change it.
fn strip(app: &mut App, ui: &mut egui::Ui, palette: &Palette, compare: &mut Compare, cell: Vec2) {
    let focused = shown(app, compare.focused());

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !compare.view.is_fitted(),
                egui::Button::new(t!("compare-fit")),
            )
            .clicked()
        {
            compare.view = compare::View::FITTED;
        }

        // A hundred per cent of the photograph as the file holds it, not of
        // whatever has been decoded so far. What is on screen is a decode of
        // at most `loading.compare_size`, so past that size this is an
        // interpolation — an honest one, and the number still means what a
        // photographer means by it.
        if ui.button(t!("compare-actual")).clicked() {
            let at = compare::one_to_one((cell.x, cell.y), focused);
            compare.view = compare::settled(
                compare::View {
                    zoom: at,
                    ..compare.view
                },
                (cell.x, cell.y),
                focused,
            );
        }

        let percent = compare::magnification((cell.x, cell.y), focused, compare.view.zoom) * 100.0;
        ui.label(
            egui::RichText::new(t!(
                "compare-magnification",
                percent = percent.round() as i64
            ))
            .color(theme::color(palette.dim)),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(t!("compare-hint")).color(theme::color(palette.dim)));
        });
    });
}

/// The well, and the ring round whichever one has the focus.
fn cell_background(ui: &mut egui::Ui, rect: egui::Rect, palette: &Palette, focused: bool) {
    let radius = egui::CornerRadius::same(2);
    ui.painter()
        .rect_filled(rect, radius, theme::color(palette.well));
    if focused {
        ui.painter().rect(
            rect,
            radius,
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(2.0, theme::color(palette.accent)),
            egui::StrokeKind::Inside,
        );
    }
}

/// Draws the photograph, and says so if it wants a closer decode than it has.
fn draw(
    app: &mut App,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &Palette,
    path: &Path,
    frame: &compare::Frame,
) -> Option<PathBuf> {
    // Whatever is already decoded is drawn at once and replaced as something
    // better arrives, so a cell is never empty while the disk is being read.
    let chosen = [Want::Close, Want::Preview, Want::Thumb, Want::Quick]
        .into_iter()
        .map(|want| (path.to_path_buf(), want))
        .find(|key| app.texture(key).is_some());

    match chosen {
        Some(key) => {
            if let Some(texture) = app.texture(&key) {
                ui.painter().with_clip_rect(rect).image(
                    texture.id(),
                    egui::Rect::from_min_size(
                        rect.min + Vec2::new(frame.target[0], frame.target[1]),
                        Vec2::new(frame.target[2], frame.target[3]),
                    ),
                    egui::Rect::from_min_size(
                        egui::pos2(frame.source[0], frame.source[1]),
                        Vec2::new(frame.source[2], frame.source[3]),
                    ),
                    egui::Color32::WHITE,
                );
            }

            app.touch(&key);
        }
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                t!("compare-unreadable"),
                egui::FontId::proportional(12.0),
                theme::color(palette.dim),
            );
        }
    }

    (!app.has(path, Want::Close)).then(|| path.to_path_buf())
}

/// The name along the bottom, and what somebody has said about it.
fn caption(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect, palette: &Palette, path: &Path) {
    if let Some(photo) = app.photo_named(path) {
        let organisation = photo.organisation.clone();
        let verdict = photo.verdict;
        let people = photo.people.clone();
        let expressions = photo.expressions;
        theme::badges(
            ui.painter(),
            rect,
            palette,
            &organisation,
            verdict,
            &people,
            expressions,
        );
    }

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let at = egui::pos2(rect.center().x, rect.max.y - 12.0);
    // White on a dark plate, and not the palette's text colour, for the same
    // reason the badges use their own: what is behind this is a photograph,
    // not a theme. Dark text on a dark plate read as "DSC_" and then nothing
    // at all, because the rest of it happened to fall over a dark jacket.
    let galley =
        ui.painter()
            .layout_no_wrap(name, egui::FontId::proportional(12.0), egui::Color32::WHITE);
    let plate = egui::Rect::from_center_size(at, galley.size() + Vec2::new(10.0, 4.0));
    ui.painter().rect_filled(
        plate,
        egui::CornerRadius::same(2),
        egui::Color32::from_black_alpha(160),
    );
    ui.painter().galley(
        plate.center() - galley.size() / 2.0,
        galley,
        egui::Color32::WHITE,
    );
}

/// The wheel and the drag, turned into a view.
///
/// `None` when nothing happened, so that a cell nobody is pointing at cannot
/// quietly hand back the view unchanged and undo the cell that was.
pub(crate) fn steered(
    ui: &mut egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    frame: &compare::Frame,
    view: compare::View,
) -> Option<compare::View> {
    let dragged = response.drag_delta();
    if dragged != Vec2::ZERO {
        return Some(pulled(view, frame, dragged));
    }

    if !response.hovered() {
        return None;
    }

    let wheel = ui.input(|input| input.smooth_scroll_delta.y);
    if wheel == 0.0 {
        return None;
    }

    let pointer = ui.input(|input| input.pointer.latest_pos())?;
    Some(wheeled(view, frame, pointer - rect.min, wheel))
}

/// How much of the photograph one point of the screen covers.
///
/// Everything below is in these units, which is what keeps a drag under the
/// finger and a detail under the pointer whatever the zoom.
fn per_point(frame: &compare::Frame) -> (f32, f32) {
    (
        frame.source[2] / frame.target[2].max(1.0),
        frame.source[3] / frame.target[3].max(1.0),
    )
}

/// A drag of so many points. The photograph follows the hand, so the view
/// goes the other way.
pub(crate) fn pulled(view: compare::View, frame: &compare::Frame, by: Vec2) -> compare::View {
    let per_point = per_point(frame);
    view.pan_by((-by.x * per_point.0, -by.y * per_point.1))
}

/// A turn of the wheel at a point in the cell, measured from its top left.
pub(crate) fn wheeled(
    view: compare::View,
    frame: &compare::Frame,
    at: Vec2,
    wheel: f32,
) -> compare::View {
    let per_point = per_point(frame);
    // Where the pointer is in the photograph, so that what is under it stays
    // under it.
    let inside = at - Vec2::new(frame.target[0], frame.target[1]);
    let point = (
        frame.source[0] + inside.x * per_point.0,
        frame.source[1] + inside.y * per_point.1,
    );
    view.zoom_about(NOTCH.powf(wheel), point)
}

/// The pixel size of a photograph as it appears, from the catalogue.
///
/// The catalogue and not the texture: a texture is a downscale and would
/// make a hundred per cent mean a hundred per cent of the downscale. Falling
/// back on the texture matters for a folder whose headers are still being
/// read, and on a square when there is neither.
pub(crate) fn shown(app: &App, path: &Path) -> (f32, f32) {
    if let Some((width, height)) = app.photo_named(path).and_then(|photo| photo.shown()) {
        return (width as f32, height as f32);
    }

    for want in [Want::Close, Want::Preview, Want::Thumb, Want::Quick] {
        if let Some(texture) = app.texture(&(path.to_path_buf(), want)) {
            let size = texture.size();
            return (size[0].max(1) as f32, size[1].max(1) as f32);
        }
    }

    (1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (f32, f32) = (800.0, 600.0);
    const IMAGE: (f32, f32) = (6000.0, 4000.0);

    /// Turning the wheel over a corner has to walk towards that corner.
    /// Zooming about the middle instead is the difference between examining
    /// a detail and chasing it round the cell.
    #[test]
    fn the_wheel_zooms_towards_where_the_pointer_is() {
        let view = compare::View::FITTED;
        let frame = compare::frame(CELL, IMAGE, view);
        // Near the top left of the drawn photograph, not of the cell: the
        // photograph is letterboxed here and the two are not the same place.
        let at = Vec2::new(frame.target[0] + 20.0, frame.target[1] + 20.0);
        let zoomed = wheeled(view, &frame, at, 400.0);

        assert!(zoomed.zoom > 1.5, "{zoomed:?}");
        assert!(zoomed.centre.0 < 0.5, "it walked away from the pointer");
        assert!(zoomed.centre.1 < 0.5, "it walked away from the pointer");

        // And the other way puts it back.
        let out = wheeled(zoomed, &compare::frame(CELL, IMAGE, zoomed), at, -400.0);
        assert!(out.zoom < zoomed.zoom);
    }

    /// The photograph follows the hand. Getting this backwards is the single
    /// most annoying thing an image viewer can do.
    #[test]
    fn dragging_left_brings_what_was_on_the_right_into_view() {
        let view = compare::View {
            centre: (0.5, 0.5),
            zoom: 4.0,
        };
        let frame = compare::frame(CELL, IMAGE, view);
        let pulled = pulled(view, &frame, Vec2::new(-40.0, 0.0));
        assert!(
            pulled.centre.0 > 0.5,
            "dragging left did not move towards the right of the frame"
        );

        // And the distance is the distance dragged, in the photograph's own
        // units — a drag is under the finger or it is nothing.
        let expected = 0.5 + 40.0 * frame.source[2] / frame.target[2];
        assert!((pulled.centre.0 - expected).abs() < 1e-5, "{pulled:?}");
    }

    #[test]
    fn a_fitted_photograph_does_not_move_when_dragged() {
        let view = compare::View::FITTED;
        let frame = compare::frame(CELL, IMAGE, view);
        let pulled = pulled(view, &frame, Vec2::new(-200.0, 90.0));
        // `pan_by` itself is free; it is settling against the cell that
        // holds it, and that is what the drawing does with the answer.
        let settled = compare::settled(pulled, CELL, IMAGE);
        assert_eq!(settled.centre, (0.5, 0.5));
    }
}
