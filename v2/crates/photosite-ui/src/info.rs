//! The pane with details of the selected photograph.
//!
//! It exists mainly because it proves what layout-as-data is for: it is a
//! second pane in the vertical column below the preview, and it cost not one
//! change to how anything else is drawn.
//!
//! The values are worked out **once per selected photograph**, not once per
//! frame. Only the file header is read, not the whole file — on a forty
//! megabyte frame on a network drive, anything else would be felt on every
//! click.

use crate::{App, Want, theme};
use eframe::egui;
use photosite_core::t;
use photosite_core::theme::Palette;
use std::path::Path;

pub fn pane(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let Some(index) = app.selected else {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(t!("info-pick-tile")).color(theme::color(palette.dim)));
        });
        return;
    };

    let path = app.photos[index].clone();
    if app.info_of.as_deref() != Some(path.as_path()) {
        app.info_rows = read(&path);
        app.info_of = Some(path.clone());
    }

    // The preview's size changes as decoding catches up, so it cannot be
    // baked into the rows built above.
    let decoded = app
        .texture(&(path.clone(), Want::Preview))
        .map(|texture| {
            let size = texture.size();
            t!(
                "info-preview-px",
                width = size[0] as i64,
                height = size[1] as i64
            )
        })
        .unwrap_or_else(|| t!("info-preview-waiting"));

    let rows: Vec<(String, String)> = app
        .info_rows
        .iter()
        .cloned()
        .chain(std::iter::once((t!("info-preview"), decoded)))
        .collect();
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            for (label, value) in &rows {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("{label:width$}"))
                            .monospace()
                            .color(theme::color(palette.dim)),
                    );
                    ui.label(
                        egui::RichText::new(value)
                            .monospace()
                            .color(theme::color(palette.text)),
                    );
                });
            }
        });
}

/// What can be learned about a file without decoding it.
fn read(path: &Path) -> Vec<(String, String)> {
    let mut rows = vec![(
        t!("info-name"),
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )];

    if let Some(folder) = path.parent() {
        rows.push((t!("info-folder"), folder.to_string_lossy().into_owned()));
    }

    match photosite_core::FileIdentity::read(path) {
        Ok(identity) => rows.push((
            t!("info-size"),
            t!(
                "info-size-mb",
                mb = identity.file_size as f64 / (1024.0 * 1024.0)
            ),
        )),
        // A file that vanished between the scan and the click is no reason to
        // panic, nor to leave the pane empty — the other rows still hold.
        Err(error) => tracing::warn!(path = %path.display(), %error, "the file cannot be read"),
    }

    let header = header(path);
    let meta = photosite_image::exif::read(&header);
    rows.push((t!("info-orientation"), meta.orientation.to_string()));
    rows.push((
        t!("info-embedded"),
        match meta.thumbnail {
            Some(thumbnail) => t!("info-embedded-at", bytes = thumbnail.len as i64),
            None => t!("info-embedded-none"),
        },
    ));

    rows
}

/// Only the start of the file. EXIF lives in the first few hundred kilobytes;
/// reading the whole photograph for it would mean a wait on every click.
fn header(path: &Path) -> Vec<u8> {
    use std::io::Read as _;

    let mut buffer = Vec::new();
    match std::fs::File::open(path) {
        Ok(file) => {
            let _ = file
                .take(photosite_image::exif::HEADER_BYTES as u64)
                .read_to_end(&mut buffer);
        }
        Err(error) => tracing::warn!(path = %path.display(), %error, "the header cannot be read"),
    }

    buffer
}
