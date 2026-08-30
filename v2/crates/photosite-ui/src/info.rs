//! Plocha s informacemi o vybrané fotce.
//!
//! Existuje hlavně proto, že dokazuje, k čemu je rozložení daty: je to druhá
//! plocha ve svislém sloupci pod náhledem a nestálo to jediný zásah do
//! kreslení ostatních.
//!
//! Údaje se počítají **jednou na vybranou fotku**, ne každý snímek. Čte se
//! jen hlavička souboru, ne celý — u čtyřicetimegabajtového snímku na síťovém
//! disku by to jinak bylo znát na každém kliknutí.

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

    // Rozměr náhledu se mění, jak dekódování dobíhá, takže se nedá schovat
    // do vyrobených řádků.
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

/// Co se dá o souboru zjistit, aniž by se dekódoval.
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
        // Soubor, který zmizel mezi skenem a kliknutím, není důvod k pádu ani
        // k prázdné ploše — ostatní řádky platí dál.
        Err(error) => tracing::warn!(path = %path.display(), %error, "soubor nelze přečíst"),
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

/// Jen začátek souboru. EXIF je v prvních stovkách kilobajtů; načítat kvůli
/// němu celou fotku by znamenalo čekání na každé kliknutí.
fn header(path: &Path) -> Vec<u8> {
    use std::io::Read as _;

    let mut buffer = Vec::new();
    match std::fs::File::open(path) {
        Ok(file) => {
            let _ = file
                .take(photosite_image::exif::HEADER_BYTES as u64)
                .read_to_end(&mut buffer);
        }
        Err(error) => tracing::warn!(path = %path.display(), %error, "hlavičku nelze přečíst"),
    }

    buffer
}
