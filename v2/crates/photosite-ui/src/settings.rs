//! The settings window.
//!
//! The screen is assembled from the field descriptions in `TUNABLES`, not
//! from hand-written controls: adding an option means adding a line there
//! and a label to the bundle. What this module adds is the shape — the
//! sections down the left, a search box above them, a label beside every
//! control — and the controls for the values `Kind` cannot say enough
//! about: a theme is a choice from the themes there are, not a word to be
//! typed exactly.
//!
//! Nothing is staged. A change takes effect as it is made and is saved at
//! once, the way the rest of the application treats its settings, so there
//! is no OK to press and no Cancel to go back to. The window is closed, not
//! confirmed; what a reset returns to is the defaults, never "before".

use crate::{App, theme};
use eframe::egui;
use photosite_core::settings::{Kind, TUNABLES, Tunable};
use photosite_core::theme::{AUTOMATIC, Palette, THEMES};
use photosite_core::{i18n, t};

/// Where somebody is in the window: the section open, and the words typed
/// into the search box.
#[derive(Debug, Default)]
pub struct View {
    section: usize,
    search: String,
}

/// One group of settings, named for what it is about rather than for the
/// table it is kept in: the splitter is a `window` key, and nobody would
/// look for it there.
struct Section {
    label_key: &'static str,
    /// The setting paths it holds, as prefixes. Also what a reset of the
    /// section returns to its defaults.
    paths: &'static [&'static str],
}

impl Section {
    fn holds(&self, path: &str) -> bool {
        self.paths.iter().any(|&prefix| {
            path == prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
    }
}

const SECTIONS: &[Section] = &[
    Section {
        label_key: "settings-section-gallery",
        paths: &["gallery"],
    },
    Section {
        label_key: "settings-section-loading",
        paths: &["loading"],
    },
    Section {
        label_key: "settings-section-appearance",
        paths: &["appearance", "window.splitter"],
    },
    Section {
        label_key: "settings-section-faces",
        paths: &["faces"],
    },
    Section {
        label_key: "settings-section-describing",
        paths: &["ai"],
    },
    Section {
        label_key: "settings-section-updates",
        paths: &["updates"],
    },
];

/// The width of the list of sections, and of a text field.
const SECTIONS_WIDTH: f32 = 170.0;
const FIELD_WIDTH: f32 = 260.0;

pub fn window(app: &mut App, ctx: &egui::Context, palette: &Palette) {
    if !app.show_settings {
        return;
    }

    let mut open = true;
    let mut changed = false;
    let mut redress = false;
    let mut reset: Option<&'static [&'static str]> = None;
    let mut reset_all = false;
    let mut close = false;
    let query = app.settings_view.search.trim().to_lowercase();
    let searching = !query.is_empty();

    egui::Window::new(t!("settings-title"))
        .open(&mut open)
        .default_size([780.0, 540.0])
        .min_size([560.0, 360.0])
        .vscroll(false)
        .show(ctx, |ui| {
            // Panels inside the window: the search above, the buttons
            // below, the sections down the left, and the settings
            // themselves in what is left.
            egui::Panel::top("settings-search").show(ui, |ui| {
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut app.settings_view.search)
                        .hint_text(t!("settings-search"))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
            });

            egui::Panel::bottom("settings-buttons").show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // A search shows pieces of several sections, and which
                    // of them "this section" would be is anybody's guess.
                    let section = &SECTIONS[app.settings_view.section];
                    if ui
                        .add_enabled(!searching, egui::Button::new(t!("settings-reset-section")))
                        .clicked()
                    {
                        reset = Some(section.paths);
                    }

                    if ui.button(t!("settings-reset")).clicked() {
                        reset_all = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t!("settings-close")).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);
            });

            egui::Panel::left("settings-sections")
                .resizable(false)
                .exact_size(SECTIONS_WIDTH)
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        for (at, section) in SECTIONS.iter().enumerate() {
                            // While searching no section is "the" one;
                            // choosing one ends the search.
                            let on = !searching && at == app.settings_view.section;
                            if ui
                                .selectable_label(on, i18n::t(section.label_key))
                                .clicked()
                            {
                                app.settings_view.section = at;
                                app.settings_view.search.clear();
                            }
                        }
                    });
                });

            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        if searching {
                            let mut any = false;
                            for (at, section) in SECTIONS.iter().enumerate() {
                                let found: Vec<&Tunable> = TUNABLES
                                    .iter()
                                    .filter(|tunable| {
                                        shown(tunable, section) && matches(tunable, section, &query)
                                    })
                                    .collect();
                                if found.is_empty() {
                                    continue;
                                }

                                any = true;
                                ui.heading(i18n::t(section.label_key));
                                ui.add_space(8.0);
                                let (touched, dressed) = rows(app, ui, &found, at);
                                changed |= touched;
                                redress |= dressed;
                                ui.add_space(16.0);
                            }

                            if !any {
                                ui.label(
                                    egui::RichText::new(t!("settings-no-match"))
                                        .color(theme::color(palette.dim)),
                                );
                            }
                        } else {
                            let at = app.settings_view.section;
                            let section = &SECTIONS[at];
                            let found: Vec<&Tunable> = TUNABLES
                                .iter()
                                .filter(|tunable| shown(tunable, section))
                                .collect();
                            ui.heading(i18n::t(section.label_key));
                            ui.add_space(8.0);
                            let (touched, dressed) = rows(app, ui, &found, at);
                            changed |= touched;
                            redress |= dressed;
                        }
                    });
            });
        });

    if close {
        open = false;
    }

    app.show_settings = open;

    if let Some(paths) = reset {
        for &path in paths {
            if let Err(error) = app.settings.reset(path) {
                tracing::warn!(path, error = %format!("{error:#}"), "the section cannot be reset");
            }
        }

        changed = true;
        redress = true;
    }

    if reset_all {
        app.settings.reset_all();
        changed = true;
        redress = true;
    }

    if redress {
        app.dress(ctx);
    }

    if changed {
        app.save_settings();
    }
}

/// Whether a setting belongs on the screen at all, and in this section.
/// Window state is not a preference and does not belong in the settings.
fn shown(tunable: &Tunable, section: &Section) -> bool {
    tunable.kind != Kind::State && section.holds(tunable.path)
}

/// Whether a search finds this setting: by its label, by the name of its
/// section, or by the key it is kept under, for whoever read the file.
fn matches(tunable: &Tunable, section: &Section, query: &str) -> bool {
    i18n::t(tunable.label_key).to_lowercase().contains(query)
        || i18n::t(section.label_key).to_lowercase().contains(query)
        || tunable.path.contains(query)
}

/// The rows of one section, a label and its control each, lined up in two
/// columns. Answers whether anything changed, and whether what changed is
/// the look of the window.
fn rows(app: &mut App, ui: &mut egui::Ui, tunables: &[&Tunable], salt: usize) -> (bool, bool) {
    let mut changed = false;
    let mut redress = false;
    egui::Grid::new(("settings", salt))
        .num_columns(2)
        .spacing([24.0, 10.0])
        .min_col_width(180.0)
        .show(ui, |ui| {
            for tunable in tunables {
                ui.label(i18n::t(tunable.label_key));
                if control(app, ui, tunable) {
                    changed = true;
                    redress |= tunable.path.starts_with("appearance.");
                }

                ui.end_row();
            }
        });

    (changed, redress)
}

/// One control, drawn from the kind of value, and written back the moment
/// it is touched. Answers whether it was.
fn control(app: &mut App, ui: &mut egui::Ui, tunable: &Tunable) -> bool {
    match tunable.kind {
        Kind::Bool => {
            let mut value = app
                .settings
                .get(tunable.path)
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            ui.checkbox(&mut value, "").changed()
                && app.write(tunable.path, toml::Value::Boolean(value))
        }
        Kind::Float { min, max } => {
            let mut value = app
                .settings
                .get(tunable.path)
                .and_then(|value| value.as_float())
                .unwrap_or(0.0);
            ui.add(egui::Slider::new(&mut value, min..=max)).changed()
                && app.write(tunable.path, toml::Value::Float(value))
        }
        Kind::Int { min, max } => {
            let mut value = app
                .settings
                .get(tunable.path)
                .and_then(|value| value.as_integer())
                .unwrap_or(0);
            ui.add(egui::Slider::new(&mut value, min..=max)).changed()
                && app.write(tunable.path, toml::Value::Integer(value))
        }
        Kind::Choice(ids) => {
            let options: Vec<(String, String)> = ids
                .iter()
                .map(|id| ((*id).to_owned(), named(tunable, id)))
                .collect();
            chosen(app, ui, tunable, &options)
        }
        Kind::Text | Kind::Secret => match tunable.path {
            // The themes there are, by name; "automatic" only where it
            // means something, which is the one that follows the system.
            "appearance.theme" | "appearance.theme_dark" | "appearance.theme_light" => {
                let mut options: Vec<(String, String)> = THEMES
                    .iter()
                    .map(|theme| (theme.id.to_owned(), i18n::t(theme.label_key)))
                    .collect();
                if tunable.path == "appearance.theme" {
                    options.insert(0, (AUTOMATIC.to_owned(), t!("theme-automatic")));
                }

                chosen(app, ui, tunable, &options)
            }
            "appearance.language" => {
                let options: Vec<(String, String)> = i18n::available()
                    .iter()
                    .map(|(language, _)| (language.to_string(), language.to_string()))
                    .collect();
                chosen(app, ui, tunable, &options)
            }
            _ => {
                let mut value = text(app, tunable);
                let hidden = tunable.kind == Kind::Secret;
                ui.add(
                    egui::TextEdit::singleline(&mut value)
                        .password(hidden)
                        .desired_width(FIELD_WIDTH),
                )
                .changed()
                    && app.write(tunable.path, toml::Value::String(value))
            }
        },
        Kind::State => false,
    }
}

/// The value as text, which is what every choice and every field holds.
fn text(app: &App, tunable: &Tunable) -> String {
    app.settings
        .get(tunable.path)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A choice of several, as a drop-down. The ids are what is kept; the
/// names are only for showing. A value that is none of the options — one
/// typed into the file by hand — is shown as it is rather than hidden.
fn chosen(
    app: &mut App,
    ui: &mut egui::Ui,
    tunable: &Tunable,
    options: &[(String, String)],
) -> bool {
    let current = text(app, tunable);
    let shown = options
        .iter()
        .find(|(id, _)| *id == current)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| current.clone());
    let mut picked: Option<String> = None;
    egui::ComboBox::from_id_salt(tunable.path)
        .selected_text(shown)
        .width(FIELD_WIDTH)
        .show_ui(ui, |ui| {
            for (id, name) in options {
                if ui.selectable_label(*id == current, name).clicked() {
                    picked = Some(id.clone());
                }
            }
        });

    match picked {
        Some(id) if id != current => app.write(tunable.path, toml::Value::String(id)),
        _ => false,
    }
}

/// What an option of a choice is called. The bundle keeps the names under
/// the keys the rest of the application already uses for the same things;
/// an option nobody named is shown by its id.
fn named(tunable: &Tunable, id: &str) -> String {
    let key = match tunable.path {
        "gallery.sort_field" => format!("sort-{id}"),
        "ai.provider" => format!("ai-provider-{id}"),
        _ => return id.to_owned(),
    };
    i18n::t(&key)
}
