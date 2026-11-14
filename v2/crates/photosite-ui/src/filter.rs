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
use photosite_core::filter::{Described, Expression, Filter, Shape};
use photosite_core::theme::Palette;
use photosite_core::{i18n, t, time};
use std::collections::BTreeSet;

/// Which end of the date range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    From,
    To,
}

/// The calendar under one of the two date boxes: which box it fills in,
/// and the month it is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Calendar {
    pub end: End,
    pub year: i64,
    pub month: u32,
}

impl Calendar {
    /// Opens on the month of the date already in the box. Failing that, on
    /// the end of the folder's span the box is for — the oldest photograph
    /// for "from", the newest for "to" — and failing that, on this month.
    fn opening(end: End, typed: &str, span: Option<(i64, i64)>) -> Self {
        let at = time::parse_date(typed)
            .or_else(|| {
                span.map(|(first, last)| match end {
                    End::From => first,
                    End::To => last,
                })
            })
            .unwrap_or_else(time::local_now);
        let (year, month, ..) = time::civil(at);
        Self { end, year, month }
    }

    /// So many months on, or back, across the ends of years.
    fn step(&mut self, months: i64) {
        let index = self.year * 12 + i64::from(self.month) - 1 + months;
        self.year = index.div_euclid(12);
        self.month = index.rem_euclid(12) as u32 + 1;
    }
}

/// The width of a day in the calendar, and of the weekday named above it.
const DAY: f32 = 26.0;

pub fn window(app: &mut App, ctx: &egui::Context, palette: &Palette) {
    if !app.show_filter {
        app.filter_calendar = None;
        return;
    }

    let mut open = true;
    let mut filter = app.filter.clone();
    let facets = app.facets.clone();
    let (mut from, mut to) = (app.filter_from.clone(), app.filter_to.clone());
    let mut calendar = app.filter_calendar;
    // Where the box the calendar belongs to was drawn this frame.
    let mut under: Option<egui::Rect> = None;
    let span = facets.taken;

    let shown = crate::dialog::window(ctx, &app.settings.window, "filter", t!("filter-title"))
        .open(&mut open)
        .default_width(380.0)
        .show(ctx, |ui| {
            crate::dialog::spacious(ui);
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

                    // Where some are described and some are not. Before a
                    // describe run the second button is the list of what is
                    // left to do; after it, the first is what came back.
                    if facets.divided_by_description() {
                        section(ui, palette, &t!("filter-section-described"), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (value, key) in [
                                    (Described::Yes, "filter-described"),
                                    (Described::No, "filter-not-described"),
                                ] {
                                    let on = filter.described == value;
                                    if ui.selectable_label(on, i18n::t(key)).clicked() {
                                        filter.described = if on { Described::Any } else { value };
                                    }
                                }
                            });
                        });
                    }

                    if facets.taken.is_some() {
                        section(ui, palette, &t!("filter-section-taken"), |ui| {
                            ui.horizontal(|ui| {
                                let ends = [
                                    (End::From, &mut from, t!("filter-from")),
                                    (End::To, &mut to, t!("filter-to")),
                                ];
                                for (end, text, hint) in ends {
                                    let response = date_box(ui, text, hint);
                                    // Stepping into a box opens the calendar
                                    // under it; the box can still be typed
                                    // into, and the calendar follows.
                                    if response.gained_focus() || response.clicked() {
                                        calendar = Some(Calendar::opening(end, text, span));
                                    }

                                    if calendar.is_some_and(|showing| showing.end == end) {
                                        under = Some(response.rect);
                                    }
                                }
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
    crate::dialog::remember(&mut app.settings.window, "filter", &shown);

    // The calendar floats under its box rather than sitting in the window:
    // a month of buttons that came and went would push everything beneath
    // the boxes down and up again with every click into them.
    calendar = match (calendar, under) {
        (Some(mut showing), Some(under)) if open => {
            let typed = match showing.end {
                End::From => &from,
                End::To => &to,
            };
            let chosen = time::parse_date(typed);
            let mut picked = None;
            let area = egui::Area::new(egui::Id::new("filter calendar"))
                .order(egui::Order::Foreground)
                .fixed_pos(under.left_bottom() + egui::vec2(0.0, 4.0))
                .constrain(true)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        picked = month(ui, palette, &mut showing, chosen);
                    });
                });

            // A press anywhere else puts it away, and so does Escape.
            let inside = [area.response.rect, under];
            let pressed_outside = ctx.input(|input| {
                input.pointer.any_pressed()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|at| !inside.iter().any(|rect| rect.contains(at)))
            });
            let escaped = ctx.input(|input| input.key_pressed(egui::Key::Escape));
            match picked {
                Some(day) => {
                    let text = time::format_date(day);
                    match showing.end {
                        End::From => from = text,
                        End::To => to = text,
                    }

                    None
                }
                None if pressed_outside || escaped => None,
                None => Some(showing),
            }
        }
        _ => None,
    };

    // The dates live as text so a half-typed one does not keep clearing
    // itself; only a whole date reaches the filter.
    filter.taken_from = time::parse_date(&from);
    filter.taken_to = time::parse_date_end(&to);

    app.filter_calendar = calendar;
    app.show_filter = open;
    app.filter_from = from;
    app.filter_to = to;
    app.set_filter(filter);
}

/// One end of the date range, as a box to type into. What is in it is red
/// while it is not a date: a filter that quietly ignores a date it could
/// not read looks exactly like a filter that does not work.
fn date_box(ui: &mut egui::Ui, text: &mut String, hint: String) -> egui::Response {
    let unread = !text.trim().is_empty() && time::parse_date(text).is_none();
    let mut edit = egui::TextEdit::singleline(text)
        .desired_width(96.0)
        .hint_text(hint);
    if unread {
        edit = edit.text_color(ui.visuals().error_fg_color);
    }

    ui.add(edit)
}

/// A month to pick a day from, with a step of a month and of a year either
/// way. Answers the day clicked, as its first second.
///
/// The month is named in figures, the way every date here is written, and
/// the weeks start on a Monday.
fn month(
    ui: &mut egui::Ui,
    palette: &Palette,
    showing: &mut Calendar,
    chosen: Option<i64>,
) -> Option<i64> {
    let mut picked = None;
    ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
    ui.horizontal(|ui| {
        for (text, months) in [("<<", -12), ("<", -1)] {
            if ui.small_button(text).clicked() {
                showing.step(months);
            }
        }

        let title = format!("{:04}-{:02}", showing.year, showing.month);
        ui.add_sized([DAY * 3.0, 18.0], egui::Label::new(title));
        for (text, months) in [(">", 1), (">>", 12)] {
            if ui.small_button(text).clicked() {
                showing.step(months);
            }
        }
    });

    let first = time::from_civil(showing.year, showing.month, 1, 0, 0, 0);
    let blank = time::weekday(first);
    let days = time::days_in_month(showing.year, showing.month);
    egui::Grid::new("filter calendar days")
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            for name in t!("calendar-weekdays").split_whitespace() {
                let name = egui::RichText::new(name)
                    .small()
                    .color(theme::color(palette.dim));
                ui.add_sized([DAY, 14.0], egui::Label::new(name));
            }
            ui.end_row();

            for _ in 0..blank {
                ui.label("");
            }

            for day in 1..=days {
                let at = first + i64::from(day - 1) * 86_400;
                let text = egui::WidgetText::from(day.to_string());
                let button = egui::Button::selectable(chosen == Some(at), text);
                if ui.add_sized([DAY, 20.0], button).clicked() {
                    picked = Some(at);
                }

                if (blank + day).is_multiple_of(7) {
                    ui.end_row();
                }
            }
        });
    picked
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

#[cfg(test)]
mod tests {
    use super::*;

    fn showing(year: i64, month: u32) -> Calendar {
        Calendar {
            end: End::From,
            year,
            month,
        }
    }

    #[test]
    fn stepping_a_month_crosses_the_end_of_the_year_both_ways() {
        let mut calendar = showing(2025, 12);
        calendar.step(1);
        assert_eq!((calendar.year, calendar.month), (2026, 1));
        calendar.step(-1);
        assert_eq!((calendar.year, calendar.month), (2025, 12));
        calendar.step(-12);
        assert_eq!((calendar.year, calendar.month), (2024, 12));
        calendar.step(14);
        assert_eq!((calendar.year, calendar.month), (2026, 2));
    }

    /// The calendar opens where somebody is most likely to want it: on the
    /// date already typed, and otherwise on the end of what the folder
    /// holds that the box is for.
    #[test]
    fn the_calendar_opens_on_what_is_typed_or_on_the_folders_own_end() {
        let span = Some((
            time::from_civil(2023, 8, 22, 9, 0, 0),
            time::from_civil(2026, 11, 14, 18, 0, 0),
        ));

        let typed = Calendar::opening(End::To, "2025-05-20", span);
        assert_eq!((typed.year, typed.month), (2025, 5));
        let day_first = Calendar::opening(End::To, "20.5.2025", span);
        assert_eq!((day_first.year, day_first.month), (2025, 5));

        let from = Calendar::opening(End::From, "", span);
        assert_eq!((from.end, from.year, from.month), (End::From, 2023, 8));
        let to = Calendar::opening(End::To, "not a date", span);
        assert_eq!((to.end, to.year, to.month), (End::To, 2026, 11));
    }
}
