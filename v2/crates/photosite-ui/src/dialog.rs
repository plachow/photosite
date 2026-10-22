//! The one way a dialog stands.
//!
//! Every dialog — Describe, People, Convert, the filter, the settings — is
//! made here, so that they all behave the same: the first time, in the
//! middle of the application; after that, wherever it was left, even in a
//! run before this one, unless that place has since fallen outside the
//! window. The same margins, the same spacing between the rows, the same
//! footer for the one button a dialog is about.

use crate::theme;
use eframe::egui;
use photosite_core::settings::Window as Remembered;
use photosite_core::theme::Palette;

/// The space between a dialog's edge and its contents.
const MARGIN: i8 = 16;

/// A dialog, placed. `name` is its key in the settings and must not change
/// between versions, or everybody's dialogs go back to the middle.
///
/// Placed by its middle every frame rather than by a corner: a dialog grows
/// and shrinks with what is in it, and one placed by its middle one frame
/// and by its corner the next jumps by half its size.
pub fn window<'a>(
    ctx: &egui::Context,
    remembered: &Remembered,
    name: &'static str,
    title: impl Into<egui::WidgetText>,
) -> egui::Window<'a> {
    let content = ctx.content_rect();
    let middle = remembered
        .dialog_at(name)
        .map(|(x, y)| egui::pos2(x as f32, y as f32))
        // Left on a screen that is no longer there, or in a corner the
        // window has since shrunk away from: the middle, then.
        .filter(|at| content.contains(*at))
        .unwrap_or_else(|| content.center());
    egui::Window::new(title)
        .id(egui::Id::new(name))
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(middle)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(MARGIN)))
}

/// Keeps where a dialog is, every frame it is shown, so that wherever it is
/// closed from, the next one opens there.
pub fn remember<R>(
    remembered: &mut Remembered,
    name: &str,
    shown: &Option<egui::InnerResponse<Option<R>>>,
) {
    if let Some(shown) = shown {
        let middle = shown.response.rect.center();
        remembered.remember_dialog(name, f64::from(middle.x), f64::from(middle.y));
    }
}

/// Room between the rows. Called first thing inside a dialog.
pub fn spacious(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 5.0);
}

/// A form: a label down the left, the controls down the right, the rows
/// lined up. Each row is `ui.label(..)`, the control, `ui.end_row()`.
pub fn form<R>(ui: &mut egui::Ui, name: &str, rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Grid::new(name)
        .num_columns(2)
        .spacing([18.0, 12.0])
        .show(ui, rows)
        .inner
}

/// A quiet line under a form: an explanation, a path, what will happen. At
/// the ordinary size, because a hint nobody can read is not a hint.
pub fn hint(ui: &mut egui::Ui, palette: &Palette, text: impl Into<String>) -> egui::Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text.into()).color(theme::color(palette.dim))).wrap(),
    )
}

/// The strip along the bottom where the dialog's one real button lives,
/// set off from the form above it by a line and its own background. It
/// runs edge to edge, through the dialog's own margin.
pub fn footer<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.add_space(6.0);
    let line = egui::Stroke::new(1.0, theme::color(palette.bevel_light));
    egui::Frame::new()
        .fill(theme::color(palette.panel))
        .inner_margin(egui::Margin::symmetric(MARGIN, 12))
        .outer_margin(egui::Margin {
            left: -MARGIN,
            right: -MARGIN,
            top: 0,
            bottom: -MARGIN,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let inside = ui.max_rect();
            ui.painter().hline(
                egui::Rangef::new(
                    inside.left() - f32::from(MARGIN),
                    inside.right() + f32::from(MARGIN),
                ),
                inside.top() - 12.0,
                line,
            );
            contents(ui)
        })
        .inner
}

/// The button a dialog is about, in the accent colour so that it reads as
/// the one to press.
pub fn primary(
    ui: &mut egui::Ui,
    palette: &Palette,
    text: impl Into<String>,
    enabled: bool,
) -> egui::Response {
    let label = egui::RichText::new(text.into())
        .strong()
        .color(theme::on_accent(palette));
    ui.add_enabled(
        enabled,
        egui::Button::new(label)
            .fill(theme::color(palette.accent))
            .min_size(egui::vec2(128.0, 32.0)),
    )
}
