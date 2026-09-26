//! The pane with the details of the selected photograph.
//!
//! Two halves, and the order between them is deliberate. **What somebody
//! said comes first** — the stars, the label, the verdict, the words — with
//! what was read out of the file underneath. The pane is used far more often
//! to change something than to look something up.
//!
//! The read-only values are worked out **once per selected photograph**, not
//! once per frame, and only the file header is read. On a forty megabyte
//! frame on a network drive, anything else would be felt on every click.

use crate::{App, Want, files, theme};
use eframe::egui;
use photosite_core::domain::{ColorLabel, Flag, Organisation, Photo};
use photosite_core::theme::Palette;
use photosite_core::{i18n, t};

pub fn pane(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let Some(index) = app.selected else {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(t!("info-pick-tile")).color(theme::color(palette.dim)));
        });
        return;
    };

    let Some(photo) = app.photo(index).cloned() else {
        return;
    };

    app.load_edits(&photo);
    let many = app.selection.len();

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);

            // With more than one tile selected everything still works and
            // goes on all of them — but the pane says how many, and says it
            // in the accent colour, because writing a title over forty
            // photographs by accident is not a small mistake.
            if many > 1 {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(t!("info-many-selected", count = many as i64))
                            .color(theme::color(palette.accent)),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(t!("info-many-hint"))
                            .small()
                            .color(theme::color(palette.dim)),
                    );
                });
                ui.add_space(4.0);
            }

            said(app, ui, palette, &photo, many > 1);
            ui.add_space(8.0);
            ui.separator();
            facts(app, ui, palette, &photo);
        });
}

/// What somebody said about the photograph, and where they say it.
fn said(app: &mut App, ui: &mut egui::Ui, palette: &Palette, photo: &Photo, many: bool) {
    let organisation = &photo.organisation;

    row(ui, palette, &t!("info-rating"), |ui| {
        if let Some(stars) = rating(ui, palette, organisation.rating) {
            app.rate_from_panel(stars);
        }
    });

    row(ui, palette, &t!("info-label"), |ui| {
        for option in ColorLabel::ALL {
            if swatch(ui, palette, option, organisation.label == option) {
                app.label_from_panel(option);
            }
        }
    });

    row(ui, palette, &t!("info-flag"), |ui| {
        for option in [Flag::Picked, Flag::None, Flag::Rejected] {
            let chosen = organisation.flag == option;
            let text = egui::RichText::new(photosite_core::i18n::t(option.title_key())).color(
                theme::color(if chosen { palette.accent } else { palette.dim }),
            );
            if ui.selectable_label(chosen, text).clicked() {
                app.flag_from_panel(option);
            }
        }
    });

    ui.add_space(4.0);

    // A position belongs to one photograph. Forty of them were not all taken
    // in the same spot, and offering one box for the lot would say they were.
    if !many {
        place(app, ui, palette, photo);
    }

    if field(ui, palette, &t!("info-title"), &mut app.edit_title, false) {
        app.commit_title();
    }

    if field(
        ui,
        palette,
        &t!("info-description"),
        &mut app.edit_description,
        true,
    ) {
        app.commit_description();
    }

    if field(
        ui,
        palette,
        &t!("info-keywords"),
        &mut app.edit_keywords,
        false,
    ) {
        app.commit_keywords();
    }

    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(if many {
                // Added, not replaced. Setting the keywords of forty
                // photographs to one word would throw away everything
                // already said about each of them, and nobody typing a word
                // into a box means that.
                t!("info-keywords-hint-many")
            } else {
                t!("info-keywords-hint")
            })
            .small()
            .color(theme::color(palette.dim)),
        );
    });

    people(app, ui, palette, photo, many);
}

/// Who is on it — the named faces and the people said to be there by hand —
/// with a way to take one off and a box to add one.
///
/// The names are chips rather than a comma-separated line because each one
/// is a thing to click: taking a person off a photograph is one ✕, not a
/// trip to the People window. Adding one is typing a name and pressing
/// Enter; a name nobody has used yet becomes a new person, the same as in
/// the People window. On a selection only the box is offered — the chips of
/// one photograph would say nothing true about forty.
fn people(app: &mut App, ui: &mut egui::Ui, palette: &Palette, photo: &Photo, many: bool) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("info-people")).color(theme::color(palette.dim)));
    });

    let mut take_off: Option<i64> = None;
    let mut add = false;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        if !many {
            for tag in &photo.people {
                ui.label(egui::RichText::new(&tag.name).color(theme::color(palette.text)));
                if ui
                    .small_button("\u{2715}")
                    .on_hover_text(t!("info-people-remove", name = tag.name.clone()))
                    .clicked()
                {
                    take_off = Some(tag.id);
                }

                ui.add_space(4.0);
            }
        }

        let box_ = ui.add(
            egui::TextEdit::singleline(&mut app.edit_person)
                .hint_text(if many {
                    t!("info-people-add-many")
                } else {
                    t!("info-people-add")
                })
                .desired_width(140.0),
        );
        add = box_.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    });
    ui.add_space(2.0);

    if let Some(person) = take_off {
        app.untag_person_from_panel(person);
    }

    if add {
        app.tag_person_from_panel();
    }
}

/// Where it was taken, how much of that to believe, and a way to go and look.
///
/// The verdict is a coloured dot on the button rather than a sentence: it has
/// to be readable at a glance and it must not push the coordinates off the
/// row. The sentence is there for whoever hovers, which is whoever wondered.
fn place(app: &mut App, ui: &mut egui::Ui, palette: &Palette, photo: &Photo) {
    let mut written = false;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t!("info-place"))
                .small()
                .color(theme::color(palette.dim)),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Disabled without coordinates rather than hidden: a button that
            // comes and goes is a button nobody learns the place of.
            let button = ui.add_enabled(photo.place.is_some(), egui::Button::new(t!("info-map")));
            let button = match photo.reason {
                Some(reason) => button.on_hover_text(i18n::t(reason.title_key())),
                None => button,
            };
            if button.clicked()
                && let Some(place) = photo.place
            {
                let url = place.in_map(&app.settings.gallery.map_url);
                tracing::info!(%url, "opening the map");
                files::open_link(&url);
            }

            if let Some(colour) = theme::verdict_color(photo.verdict) {
                let (rect, response) =
                    ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.5, colour);
                response.on_hover_text(i18n::t(photo.verdict.title_key()));
            }

            // Typed rather than shown. A position read off a phone is a
            // guess often enough that correcting one has to be as easy as
            // reading one — and typing it is what clears the mark.
            let box_ = ui.add(
                egui::TextEdit::singleline(&mut app.edit_place)
                    .desired_width(ui.available_width().max(120.0))
                    .hint_text(t!("info-place-hint")),
            );
            written = box_.lost_focus()
                && (ui.input(|input| input.key_pressed(egui::Key::Enter)) || !box_.has_focus());
        });
    });

    if written {
        app.commit_place();
    }

    ui.add_space(4.0);
}

/// Five stars. Returns the one clicked, counting from one.
fn rating(ui: &mut egui::Ui, palette: &Palette, rating: u8) -> Option<u8> {
    let mut clicked = None;
    let size = 18.0;
    for star in 1..=Organisation::MAX_RATING {
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
        let lit = star <= rating;
        let fill = if lit {
            egui::Color32::from_rgb(0xF2, 0xC5, 0x4E)
        } else if response.hovered() {
            theme::color(palette.text)
        } else {
            theme::color(palette.dim)
        };
        theme::star(ui.painter(), rect.center(), size * 0.45, fill);
        if response.clicked() {
            clicked = Some(star);
        }
    }

    clicked
}

/// One colour label to click. `None` is drawn as an outline, because "no
/// label" has no colour to show and an empty gap would look like a bug.
fn swatch(ui: &mut egui::Ui, palette: &Palette, label: ColorLabel, chosen: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::click());
    let radius = egui::CornerRadius::same(3);
    match label.color() {
        Some(colour) => {
            ui.painter().rect_filled(
                rect.shrink(2.0),
                radius,
                egui::Color32::from_rgb(colour.r, colour.g, colour.b),
            );
        }
        None => {
            ui.painter().rect_stroke(
                rect.shrink(2.0),
                radius,
                egui::Stroke::new(1.0, theme::color(palette.dim)),
                egui::StrokeKind::Inside,
            );
        }
    }

    if chosen || response.hovered() {
        let _ = ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(
                if chosen { 2.0 } else { 1.0 },
                theme::color(if chosen { palette.accent } else { palette.text }),
            ),
            egui::StrokeKind::Inside,
        );
    }

    let clicked = response.clicked();
    response.on_hover_text(photosite_core::i18n::t(label.title_key()));
    clicked
}

/// A labelled text field. Returns true when it has just been left, which is
/// when the value is written — not on every keystroke, which would be a
/// transaction per character.
fn field(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    value: &mut String,
    tall: bool,
) -> bool {
    let mut done = false;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(label).color(theme::color(palette.dim)));
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let widget = if tall {
            ui.add(
                egui::TextEdit::multiline(value)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            )
        } else {
            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY))
        };

        // Leaving the field writes it; so does Enter on a single line.
        done = widget.lost_focus();
    });
    ui.add_space(2.0);
    done
}

fn row(ui: &mut egui::Ui, palette: &Palette, label: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(label)
                .monospace()
                .color(theme::color(palette.dim)),
        );
        contents(ui);
    });
}

/// What was read out of the file. Never editable — it is what the camera
/// said, and correcting it belongs to writing metadata, not to a details
/// pane.
fn facts(app: &mut App, ui: &mut egui::Ui, palette: &Palette, photo: &Photo) {
    if app.info_of.as_deref() != Some(photo.path.as_path()) {
        app.info_rows = read(photo);
        app.info_of = Some(photo.path.clone());
    }

    // The preview's size changes as decoding catches up, so it cannot be
    // baked into the rows built above.
    let decoded = app
        .texture(&(photo.path.clone(), Want::Preview))
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
}

/// What the catalogue holds about a file, plus what only the header knows.
fn read(photo: &Photo) -> Vec<(String, String)> {
    let path = photo.path.as_path();
    let mut rows = vec![(
        t!("info-name"),
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )];

    if let Some(folder) = path.parent() {
        rows.push((t!("info-folder"), folder.to_string_lossy().into_owned()));
    }

    rows.push((
        t!("info-size"),
        t!(
            "info-size-mb",
            mb = photo.file_size as f64 / (1024.0 * 1024.0)
        ),
    ));

    // Straight from the catalogue: the background pass has read this
    // already, and reading it again on every click would be work for
    // nothing. A folder still being read simply has nothing here yet.
    rows.push((
        t!("info-taken"),
        match photo.taken_at {
            Some(seconds) => photosite_core::time::format(seconds),
            None => t!("info-taken-none"),
        },
    ));
    rows.push((
        t!("info-dimensions"),
        match (photo.width, photo.height) {
            (Some(width), Some(height)) => t!(
                "info-dimensions-px",
                width = width as i64,
                height = height as i64
            ),
            _ => t!("info-taken-none"),
        },
    ));
    rows.push((t!("info-orientation"), photo.orientation.to_string()));

    let meta = photosite_image::exif::read_file(path);

    // The exposure triangle, and the focal length beside it. One row rather
    // than four: `1/250 . f/2.8 . ISO 400 . 24 mm` is how a photographer
    // reads it, and four labelled lines are four times the panel for the
    // same sentence.
    let exposure = &meta.exposure;
    if !exposure.is_empty() {
        let mut parts: Vec<String> = Vec::with_capacity(4);
        parts.extend(exposure.shutter());
        parts.extend(exposure.f_number());
        if let Some(iso) = exposure.sensitivity {
            parts.push(t!("info-iso", iso = iso as i64));
        }

        if let Some(focal) = exposure.focal_mm {
            parts.push(match exposure.focal_equivalent_mm {
                // The equivalent only when it says something the real focal
                // length does not. On a full-frame camera they are the same
                // number twice.
                Some(equivalent) if (equivalent as f64 - focal).abs() > 1.0 => t!(
                    "info-focal-equivalent",
                    mm = focal,
                    equivalent = equivalent as i64
                ),
                _ => t!("info-focal", mm = focal),
            });
        }

        rows.push((t!("info-exposure"), parts.join("  \u{b7}  ")));
    }

    // How its faces scored — a catalogue row rather than anything read out
    // of the file, so it costs nothing here. Who is on it is drawn above,
    // among the things that can be changed.
    let expressions = photo.expressions;
    if expressions.scored > 0 {
        // Counts and not a verdict: "1/2 smiling" says which frame of a
        // burst to keep, where "somebody is not smiling" says only that
        // there is something to look at. The denominator is what was
        // actually looked at, so a face the models never saw is not counted
        // as a frown.
        let said = [
            t!(
                "info-expression-smiling",
                count = expressions.smiling as i64,
                of = expressions.scored as i64
            ),
            t!(
                "info-expression-eyes",
                count = expressions.eyes_open as i64,
                of = expressions.scored as i64
            ),
        ];
        rows.push((t!("info-expression"), said.join("  \u{b7}  ")));
    }

    rows.push((
        t!("info-embedded"),
        match meta.thumbnail {
            Some(thumbnail) => t!("info-embedded-at", bytes = thumbnail.len as i64),
            None => t!("info-embedded-none"),
        },
    ));

    rows
}
