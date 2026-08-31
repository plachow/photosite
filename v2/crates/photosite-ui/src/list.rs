//! The folder as a list of rows rather than a wall of tiles.
//!
//! v1 kept a details list beside the grid, in a panel of its own. This is the
//! same folder seen the same way, but as a **switch** rather than a second
//! panel: two views of one thing competing for the same width means both are
//! too narrow, and nobody reads a table four columns wide. `Ctrl+L` goes back
//! and forth, and the choice is remembered.
//!
//! Virtualised by hand, the same as the grid: only the rows on screen are
//! drawn, so the number of photographs does not matter.

use crate::{App, theme};
use eframe::egui;
use egui::{Sense, Vec2};
use photosite_core::domain::Organisation;
use photosite_core::t;
use photosite_core::theme::Palette;

/// One row's height, and the font it is written in.
const ROW: f32 = 22.0;
const TEXT: f32 = 13.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let count = app.count();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let (area, _) =
                ui.allocate_exact_size(Vec2::new(width, count as f32 * ROW), Sense::hover());

            let first = (viewport.min.y / ROW).floor().max(0.0) as usize;
            let last = ((viewport.max.y / ROW).ceil() as usize).min(count);
            let mut clicked: Option<(usize, egui::Modifiers)> = None;

            // The columns, from the right. The name takes whatever is left,
            // because it is the one that can be any length at all and the
            // one somebody is reading.
            let columns = Columns::across(width);

            for index in first..last {
                let rect = egui::Rect::from_min_size(
                    area.min + Vec2::new(0.0, index as f32 * ROW),
                    Vec2::new(width, ROW),
                );
                let response = ui.interact(rect, ui.id().with(index), Sense::click());
                if response.clicked() {
                    clicked = Some((index, ui.input(|input| input.modifiers)));
                }

                let chosen = app.is_selected(index);
                if chosen || response.hovered() {
                    let fill = theme::color(palette.tile).lerp_to_gamma(
                        theme::color(palette.accent),
                        if chosen { 0.32 } else { 0.12 },
                    );
                    ui.painter().rect_filled(rect, 2, fill);
                }

                let Some(photo) = app.photo(index) else {
                    continue;
                };

                row(ui, rect, palette, photo, &columns);
            }

            if let Some((index, modifiers)) = clicked {
                if modifiers.command || modifiers.ctrl {
                    app.select_also(index);
                } else if modifiers.shift {
                    app.select_through(index);
                } else {
                    app.select_only(index);
                }
            }
        });
}

/// Where each column starts, from the left edge of the row.
struct Columns {
    taken: f32,
    camera: f32,
    dimensions: f32,
    size: f32,
    marks: f32,
}

impl Columns {
    /// Laid out from the right, so the name gets what is left over.
    ///
    /// Columns that will not fit are pushed off the right-hand edge rather
    /// than squeezed: a narrow pane shows the name, the stars and the date,
    /// which is the order somebody would give them up in.
    fn across(width: f32) -> Self {
        let marks = width - 84.0;
        let size = marks - 76.0;
        let dimensions = size - 92.0;
        let camera = dimensions - 150.0;
        let taken = camera - 130.0;
        Self {
            taken,
            camera,
            dimensions,
            size,
            marks,
        }
    }
}

fn row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &Palette,
    photo: &photosite_core::Photo,
    columns: &Columns,
) {
    let font = egui::FontId::proportional(TEXT);
    let middle = rect.center().y;
    let write = |at: f32, text: String, dim: bool| {
        if at < 8.0 || text.is_empty() {
            return;
        }

        ui.painter().text(
            egui::pos2(rect.min.x + at, middle),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            theme::color(if dim { palette.dim } else { palette.text }),
        );
    };

    let name = photo
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    write(8.0, name, false);
    write(
        columns.taken,
        photo
            .taken_at
            .map(photosite_core::time::format)
            .unwrap_or_default(),
        true,
    );
    write(
        columns.camera,
        photo.camera.clone().unwrap_or_default(),
        true,
    );
    write(
        columns.dimensions,
        match (photo.width, photo.height) {
            (Some(width), Some(height)) => format!("{width}\u{d7}{height}"),
            _ => String::new(),
        },
        true,
    );
    write(
        columns.size,
        t!(
            "info-size-mb",
            mb = photo.file_size as f64 / (1024.0 * 1024.0)
        ),
        true,
    );

    marks(ui, rect, columns.marks, palette, photo);
}

/// The stars, the label and the position, small and in a fixed place.
///
/// Written as shapes rather than characters for the same reason the tiles
/// draw their own: the default font has no star, and an absent glyph is an
/// empty box where a rating should be.
fn marks(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    at: f32,
    palette: &Palette,
    photo: &photosite_core::Photo,
) {
    if at < 8.0 {
        return;
    }

    let middle = rect.center().y;
    let organisation = &photo.organisation;
    for star in 0..Organisation::MAX_RATING {
        if star >= organisation.rating {
            break;
        }

        theme::star(
            ui.painter(),
            egui::pos2(rect.min.x + at + 5.0 + star as f32 * 11.0, middle),
            4.5,
            egui::Color32::from_rgb(0xF2, 0xC5, 0x4E),
        );
    }

    let right = rect.max.x - 8.0;
    if let Some(swatch) = organisation.label.color() {
        ui.painter().rect_filled(
            egui::Rect::from_center_size(egui::pos2(right - 6.0, middle), Vec2::splat(9.0)),
            2,
            egui::Color32::from_rgb(swatch.r, swatch.g, swatch.b),
        );
    }

    if photo.verdict.is_doubted()
        && let Some(fill) = theme::verdict_color(photo.verdict)
    {
        theme::pin(ui.painter(), egui::pos2(right - 22.0, middle), 5.0, fill);
    }

    if organisation.flag == photosite_core::domain::Flag::Rejected {
        // A struck-through row, which is what a rejected photograph looks
        // like everywhere else that has a list.
        ui.painter().line_segment(
            [
                egui::pos2(rect.min.x + 6.0, middle),
                egui::pos2(rect.min.x + 6.0 + 0.0_f32.max(at - 14.0), middle),
            ],
            egui::Stroke::new(1.0, theme::color(palette.dim)),
        );
    }
}
