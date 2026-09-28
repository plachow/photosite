//! The status bar along the bottom of the window, the way v1 had it.
//!
//! Three things, always in the same places, so that the eye learns where to
//! look: how much of the folder is showing, how much of it is selected, and
//! what the application is doing or has just done. A task in progress takes
//! the last place over — "Moving 114 / 117 (97 %)" — and gives it back when
//! it is finished, with what came of it.
//!
//! It replaces a message squeezed in beside the folder's path, and the
//! counts and task names that crowded the right-hand end of the toolbar.

use crate::{App, theme};
use eframe::egui;
use photosite_core::t;
use photosite_core::theme::Palette;

pub fn bar(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let dim = theme::color(palette.dim);
    let accent = theme::color(palette.accent);
    ui.horizontal(|ui| {
        ui.add_space(4.0);

        // Showing. A filter hiding part of the folder says so in the accent
        // colour: half a folder missing with nothing to say why is the
        // worst thing a filter can do.
        let shown_bytes: u64 = (0..app.count())
            .filter_map(|at| app.photo(at))
            .map(|photo| photo.file_size)
            .sum();
        let shown = if app.count() == app.total() {
            t!(
                "status-shown",
                count = app.count() as i64,
                size = size(shown_bytes)
            )
        } else {
            t!(
                "status-shown-of",
                shown = app.count() as i64,
                all = app.total() as i64,
                size = size(shown_bytes)
            )
        };
        let filtered = app.count() != app.total();
        ui.label(egui::RichText::new(shown).color(if filtered { accent } else { dim }));
        ui.separator();

        let selected: Vec<u64> = app
            .selection
            .iter()
            .filter_map(|at| app.photo(*at))
            .map(|photo| photo.file_size)
            .collect();
        let chosen = if selected.is_empty() {
            t!("status-nothing-selected")
        } else {
            t!(
                "status-selected",
                count = selected.len() as i64,
                size = size(selected.iter().sum())
            )
        };
        ui.label(egui::RichText::new(chosen).color(dim));
        ui.separator();

        // What is happening, or what happened last. The first task still
        // running has the place; with none, the last word said; with no
        // word either, that all is well.
        match app.tasks.running().into_iter().next() {
            Some(task) => {
                if let Some(fraction) = task.fraction() {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(90.0)
                            .desired_height(10.0),
                    );
                }
                let said = match task.total {
                    Some(total) if total > 0 => t!(
                        "status-task",
                        title = task.title.as_str(),
                        done = task.done as i64,
                        total = total as i64,
                        percent = (task.fraction().unwrap_or(0.0) * 100.0).floor() as i64
                    ),
                    _ => t!("status-task-running", title = task.title.as_str()),
                };
                ui.add(egui::Label::new(egui::RichText::new(said).color(accent)).truncate());
            }
            None => {
                let said = if app.status.is_empty() {
                    t!("status-ready")
                } else {
                    app.status.clone()
                };
                ui.add(egui::Label::new(egui::RichText::new(said).color(dim)).truncate());
            }
        }
    });
}

/// A number of bytes the way somebody reads one.
fn size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= 1024.0 * MB {
        t!("status-size-gb", gb = bytes / (1024.0 * MB))
    } else if bytes >= MB {
        t!("status-size-mb", mb = bytes / MB)
    } else {
        t!("status-size-kb", kb = (bytes / 1024.0).ceil())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_in_the_unit_that_reads_best() {
        assert_eq!(size(42 * 1024), t!("status-size-kb", kb = 42.0));
        assert_eq!(size(5 * 1024 * 1024), t!("status-size-mb", mb = 5.0));
        assert_eq!(size(3 * 1024 * 1024 * 1024), t!("status-size-gb", gb = 3.0));
    }
}
