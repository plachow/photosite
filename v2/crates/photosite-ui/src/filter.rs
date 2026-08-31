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
use photosite_core::filter::{Filter, Shape};
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
