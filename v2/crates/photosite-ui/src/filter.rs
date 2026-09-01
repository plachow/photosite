//! The filter panel.
//!
//! Everything it offers comes from [`photosite_core::Facets`] — the folder
//! in front of you — so it never offers a camera that would leave the gallery
//! empty. A list of every camera ever owned, most of them matching nothing
//! here, is a list nobody reads.
//!
//! Nothing chosen in a section means "any", which is the same rule the core
//! applies. The sections narrow each other; the values inside one section
//! widen it.

use crate::{App, theme};
use eframe::egui;
use photosite_core::domain::Organisation;
use photosite_core::filter::{Expression, Filter, Shape};
use photosite_core::theme::Palette;
use photosite_core::{i18n, t, time};
use std::collections::BTreeSet;

pub fn window(app: &mut App, ctx: &egui::Context, palette: &Palette) {
    if !app.show_filter {
        return;
    }

    let mut open = true;
    let mut filter = app.filter.clone();
    let facets = app.facets.clone();
    let (mut from, mut to) = (app.filter_from.clone(), app.filter_to.clone());

    egui::Window::new(t!("filter-title"))
        .open(&mut open)
        .default_width(380.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(520.0)
                .show(ui, |ui| {
                    // Only what this folder can answer gets a section. A
                    // "Lens" heading over an empty row in a folder of phone
                    // photographs is a question with no answers.
                    if facets.highest_rating > 0 {
                        section(ui, palette, &t!("filter-section-rating"), |ui| {
                            rating(ui, &mut filter, facets.highest_rating);
                        });
                    }

                    if facets.labels.len() > 1 {
                        section(ui, palette, &t!("filter-section-label"), |ui| {
                            chips(ui, &mut filter.labels, &facets.labels, |label| {
                                i18n::t(label.title_key())
                            });
                        });
                    }

                    if facets.flags.len() > 1 {
                        section(ui, palette, &t!("filter-section-flag"), |ui| {
                            chips(ui, &mut filter.flags, &facets.flags, |flag| {
                                i18n::t(flag.title_key())
                            });
                        });
                    }

                    if facets.formats.len() > 1 {
                        section(ui, palette, &t!("filter-section-format"), |ui| {
                            chips(ui, &mut filter.formats, &facets.formats, |format| {
                                format.to_uppercase()
                            });
                        });
                    }

                    if !facets.cameras.is_empty() {
                        section(ui, palette, &t!("filter-section-camera"), |ui| {
                            chips(ui, &mut filter.cameras, &facets.cameras, Clone::clone);
                        });
                    }

                    if !facets.lenses.is_empty() {
                        section(ui, palette, &t!("filter-section-lens"), |ui| {
                            chips(ui, &mut filter.lenses, &facets.lenses, Clone::clone);
                        });
                    }

                    if facets.shapes.len() > 1 {
                        section(ui, palette, &t!("filter-section-shape"), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for shape in Shape::ALL {
                                    if shape != Shape::Any && !facets.shapes.contains(&shape) {
                                        continue;
                                    }

                                    ui.selectable_value(
                                        &mut filter.shape,
                                        shape,
                                        i18n::t(shape.title_key()),
                                    );
                                }
                            });
                        });
                    }

                    // Only when there is something to review. A folder
                    // where every position is precise — or where nothing
                    // carries one at all — has nothing to ask about.
                    if facets.places.iter().any(|verdict| verdict.is_doubted()) {
                        section(ui, palette, &t!("filter-places"), |ui| {
                            chips(ui, &mut filter.places, &facets.places, |verdict| {
                                i18n::t(verdict.title_key())
                            });
                        });
                    }

                    // Who is on the photographs. The chips narrow rather
                    // than widen — two names means the shot they are both
                    // in — so the section says so on its own.
                    if !facets.people.is_empty() {
                        section(ui, palette, &t!("filter-section-people"), |ui| {
                            chips(ui, &mut filter.people, &facets.people, Clone::clone);
                        });
                    }

                    // Only where something has actually been scored. Two
                    // buttons that can only ever empty the gallery are worse
                    // than no heading at all.
                    if facets.expressions {
                        section(ui, palette, &t!("filter-section-expression"), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                sides(
                                    ui,
                                    &mut filter.smile,
                                    "filter-all-smiling",
                                    "filter-someone-not-smiling",
                                );
                                sides(
                                    ui,
                                    &mut filter.eyes,
                                    "filter-all-eyes-open",
                                    "filter-someone-blinking",
                                );
                            });
                        });
                    }

                    if facets.taken.is_some() {
                        section(ui, palette, &t!("filter-section-taken"), |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut from)
                                        .desired_width(96.0)
                                        .hint_text(t!("filter-from")),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut to)
                                        .desired_width(96.0)
                                        .hint_text(t!("filter-to")),
                                );
                            });

                            // The range the folder actually spans, so
                            // somebody can see what there is to ask for
                            // rather than guessing at the format.
                            if let Some((first, last)) = facets.taken {
                                ui.label(
                                    egui::RichText::new(t!(
                                        "filter-taken-range",
                                        from = time::format_date(first),
                                        to = time::format_date(last)
                                    ))
                                    .small()
                                    .color(theme::color(palette.dim)),
                                );
                            }
                        });
                    }

                    ui.add_space(4.0);
                    ui.checkbox(&mut filter.hide_rejected, t!("filter-hide-rejected"));
                });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(t!("filter-clear")).clicked() {
                    filter = Filter::default();
                    from.clear();
                    to.clear();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(t!(
                            "filter-showing",
                            shown = app.count() as i64,
                            all = app.total() as i64
                        ))
                        .color(theme::color(palette.dim)),
                    );
                });
            });
        });

    // The dates live as text so a half-typed one does not keep clearing
    // itself; only a whole date reaches the filter.
    filter.taken_from = time::parse_date(&from);
    filter.taken_to = time::parse_date_end(&to);

    app.show_filter = open;
    app.filter_from = from;
    app.filter_to = to;
    app.set_filter(filter);
}

/// A heading and whatever belongs under it.
fn section(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(title)
            .small()
            .color(theme::color(palette.dim)),
    );
    contents(ui);
}

/// Stars as a floor: any, one and up, two and up. Only as far as the folder
/// goes — offering "four and up" where nothing has more than two is offering
/// an empty gallery.
fn rating(ui: &mut egui::Ui, filter: &mut Filter, highest: u8) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut filter.minimum_rating, 0, i18n::t("filter-any"));
        for stars in 1..=highest.min(Organisation::MAX_RATING) {
            ui.selectable_value(
                &mut filter.minimum_rating,
                stars,
                i18n::t_args("filter-rating", &[("count", (stars as i64).into())]),
            );
        }
    });
}

/// One expression facet, as two buttons that toggle.
///
/// Two and not a dropdown, because a portrait cull is done from either end
/// and both ends have to be one click away: keep the ones where everybody
/// smiled, or find the one where somebody blinked. Pressing a chosen side
/// again clears it, so there is a way back to "any" without a third button
/// that says nothing.
fn sides(ui: &mut egui::Ui, facet: &mut Expression, all: &str, anyone: &str) {
    for (value, key) in [(Expression::All, all), (Expression::Anyone, anyone)] {
        let on = *facet == value;
        if ui.selectable_label(on, i18n::t(key)).clicked() {
            *facet = if on { Expression::Any } else { value };
        }
    }
}

/// A row of values, any number of which can be picked. None picked means
/// any, which is what an empty set means to the core.
fn chips<T: Clone + Ord>(
    ui: &mut egui::Ui,
    chosen: &mut BTreeSet<T>,
    options: &[T],
    name: impl Fn(&T) -> String,
) {
    ui.horizontal_wrapped(|ui| {
        for option in options {
            let on = chosen.contains(option);
            if ui.selectable_label(on, name(option)).clicked() {
                if on {
                    chosen.remove(option);
                } else {
                    chosen.insert(option.clone());
                }
            }
        }
    });
}
