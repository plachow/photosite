//! The batch window.
//!
//! Everything in it is a setting, and the **summary underneath is the whole
//! point**: it is recomputed from the plan on every change, so the line that
//! says *forty written · three skipped · one overwritten*, and the real path
//! the first one would take, are true of the settings as they stand rather
//! than of the ones somebody had a moment ago. A batch is the one thing here
//! that can destroy work, and the plan exists so that it is visible before
//! it does.
//!
//! Recomputing it means asking the disk whether a name is taken, which is
//! why it is **not** done every frame. It is done when something changes.

use crate::{App, theme};
use eframe::egui;
use photosite_core::batch::{
    self, BATCH, Carry, Format, Naming, OnCollision, Plan, Preset, Resize,
};
use photosite_core::theme::Palette;
use photosite_core::{i18n, t};

/// The window's state.
#[derive(Debug, Default)]
pub struct Batch {
    pub open: bool,
    /// The presets in the catalogue, read when the window opens.
    pub presets: Vec<Preset>,
    /// What the settings on screen say. It starts as a copy of a preset and
    /// stops being one the moment anything is touched — which is why saving
    /// asks for a name rather than quietly changing the preset underneath.
    pub preset: Preset,
    /// What would happen, as of the last change.
    pub plan: Plan,
    /// Whether the plan is still true.
    pub stale: bool,
    /// A name being typed for a preset about to be saved.
    pub saving: Option<String>,
    pub running: Option<u64>,
    /// How the last run went, failures named.
    pub summary: String,
}

/// Opens the window on the current selection.
pub fn open(app: &mut App) {
    if let Some(catalog) = app.catalog.as_mut() {
        if let Err(error) = catalog.seed_presets() {
            tracing::warn!(error = %format!("{error:#}"), "the starter presets could not be written");
        }

        app.batch.presets = catalog.presets(BATCH).unwrap_or_default();
    }

    if app.batch.preset.name.is_empty()
        && let Some(first) = app.batch.presets.first().cloned()
    {
        app.batch.preset = first;
    }

    // Somewhere to put them, if nowhere has been chosen yet: wherever the
    // last copy or move went. Better than nothing and better than the folder
    // being converted, which would fill with outputs.
    if app.batch.preset.into.is_none() {
        app.batch.preset.into = app.settings.gallery.last_destination.clone();
    }

    app.batch.open = true;
    app.batch.stale = true;
}

/// Recomputes what would happen.
pub fn replan(app: &mut App) {
    let chosen = chosen(app);
    app.batch.plan = batch::plan(&chosen, &app.batch.preset, &|path| path.exists());
    app.batch.stale = false;
}

/// The photographs a run would work on: the selection, or the whole folder
/// when nothing is selected.
///
/// The same rule the rest of the application follows for anything that acts
/// on photographs, and the reason it is worth stating: "convert" with
/// nothing selected meaning "convert nothing" would be a menu entry that
/// does nothing and says nothing.
fn chosen(app: &App) -> Vec<photosite_core::Photo> {
    let positions: Vec<usize> = if app.selection.is_empty() {
        app.visible.clone()
    } else {
        app.selection.iter().copied().collect()
    };
    positions
        .into_iter()
        .filter_map(|at| app.photo(at).cloned())
        .collect()
}

pub fn window(app: &mut App, ctx: &egui::Context, palette: &Palette) {
    if !app.batch.open {
        return;
    }

    if app.batch.stale {
        replan(app);
    }

    let mut open = true;
    let mut changed = false;
    let mut go = false;
    let mut pick_folder = false;
    let mut save = false;
    let mut delete = None;
    let mut load: Option<Preset> = None;

    let running = app
        .batch
        .running
        .and_then(|id| app.tasks.snapshot().into_iter().find(|task| task.id == id))
        .filter(|task| !task.finished);

    egui::Window::new(t!("batch-title"))
        .open(&mut open)
        .default_width(520.0)
        .show(ctx, |ui| {
            let preset = &mut app.batch.preset;

            // The presets, as a row of chips. Choosing one replaces the
            // settings; nothing is saved until somebody says so.
            ui.horizontal_wrapped(|ui| {
                for stored in &app.batch.presets {
                    let on = stored.name == preset.name;
                    if ui.selectable_label(on, &stored.name).clicked() {
                        load = Some(stored.clone());
                    }
                }
            });

            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    changed |= where_to(ui, preset, palette, &mut pick_folder);
                    changed |= what(ui, preset, palette);
                    changed |= how_big(ui, preset, palette);
                    changed |= called_what(ui, preset, palette);
                    changed |= carrying(ui, preset, palette);
                });

            ui.separator();
            summary(app, ui, palette);
            ui.separator();

            ui.horizontal(|ui| match &running {
                Some(task) => {
                    if ui.button(t!("batch-stop")).clicked()
                        && let Some(id) = app.batch.running
                    {
                        app.tasks.cancel(id);
                    }

                    if let Some(fraction) = task.fraction() {
                        ui.add(egui::ProgressBar::new(fraction).desired_width(160.0));
                    }

                    ui.label(
                        egui::RichText::new(&task.message)
                            .small()
                            .color(theme::color(palette.dim)),
                    );
                }
                None => {
                    let ready = app.batch.plan.writes() > 0;
                    if ui
                        .add_enabled(ready, egui::Button::new(t!("batch-run")))
                        .clicked()
                    {
                        go = true;
                    }

                    match &app.batch.saving {
                        Some(name) => {
                            let mut name = name.clone();
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut name)
                                    .desired_width(160.0)
                                    .hint_text(t!("batch-preset-name")),
                            );
                            field.request_focus();
                            let entered = field.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            if entered && !name.trim().is_empty() {
                                save = true;
                            }

                            app.batch.saving = Some(name);
                        }
                        None => {
                            if ui.button(t!("batch-save-preset")).clicked() {
                                app.batch.saving = Some(app.batch.preset.name.clone());
                            }
                        }
                    }

                    if !app.batch.preset.name.trim().is_empty()
                        && ui.button(t!("batch-delete-preset")).clicked()
                    {
                        delete = Some(app.batch.preset.name.clone());
                    }
                }
            });

            if !app.batch.summary.is_empty() {
                ui.label(egui::RichText::new(&app.batch.summary).color(theme::color(palette.text)));
            }
        });

    app.batch.open = open;
    if let Some(preset) = load {
        app.batch.preset = preset;
        changed = true;
    }

    if changed {
        app.batch.stale = true;
    }

    if pick_folder {
        app.ask_for_folder_for_batch(ctx);
    }

    if save {
        let name = app.batch.saving.take().unwrap_or_default();
        app.batch.preset.name = name.trim().to_owned();
        let preset = app.batch.preset.clone();
        if let Some(catalog) = app.catalog.as_ref() {
            match catalog.save_preset(BATCH, &preset) {
                Ok(()) => app.batch.presets = catalog.presets(BATCH).unwrap_or_default(),
                Err(error) => app.status = format!("{error:#}"),
            }
        }
    }

    if let Some(name) = delete
        && let Some(catalog) = app.catalog.as_ref()
    {
        let _ = catalog.delete_preset(BATCH, &name);
        app.batch.presets = catalog.presets(BATCH).unwrap_or_default();
    }

    if go {
        start(app);
    }
}

fn where_to(ui: &mut egui::Ui, preset: &mut Preset, palette: &Palette, pick: &mut bool) -> bool {
    let mut changed = false;
    section(ui, palette, &t!("batch-where"), |ui| {
        changed |= ui
            .checkbox(&mut preset.beside_source, t!("batch-beside-source"))
            .changed();
        if !preset.beside_source {
            ui.horizontal(|ui| {
                let mut into = preset.into.clone().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut into)
                            .desired_width(300.0)
                            .hint_text(t!("batch-into")),
                    )
                    .changed()
                {
                    preset.into = Some(into);
                    changed = true;
                }

                if ui.button(t!("batch-choose-folder")).clicked() {
                    *pick = true;
                }
            });
        }

        changed |= ui
            .checkbox(&mut preset.folder_per_day, t!("batch-folder-per-day"))
            .changed();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(t!("batch-on-collision"))
                    .small()
                    .color(theme::color(palette.dim)),
            );
            for value in OnCollision::ALL {
                changed |= ui
                    .selectable_value(&mut preset.on_collision, value, i18n::t(value.title_key()))
                    .changed();
            }
        });
    });
    changed
}

fn what(ui: &mut egui::Ui, preset: &mut Preset, palette: &Palette) -> bool {
    let mut changed = false;
    section(ui, palette, &t!("batch-format"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for value in Format::ALL {
                changed |= ui
                    .selectable_value(&mut preset.format, value, i18n::t(value.title_key()))
                    .changed();
            }
        });

        // Where the quality slider would be, either the slider or the reason
        // there is not one.
        if preset.format.has_quality() {
            changed |= ui
                .add(egui::Slider::new(&mut preset.quality, 1..=100).text(t!("batch-quality")))
                .changed();
        } else if let Some(note) = preset.format.note_key() {
            ui.label(
                egui::RichText::new(i18n::t(note))
                    .small()
                    .color(theme::color(palette.dim)),
            );
        }
    });
    changed
}

fn how_big(ui: &mut egui::Ui, preset: &mut Preset, palette: &Palette) -> bool {
    let mut changed = false;
    section(ui, palette, &t!("batch-size"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for value in Resize::ALL {
                changed |= ui
                    .selectable_value(&mut preset.resize, value, i18n::t(value.title_key()))
                    .changed();
            }
        });

        if preset.resize != Resize::None {
            ui.horizontal(|ui| {
                changed |= ui
                    .add(egui::DragValue::new(&mut preset.resize_to).range(1..=30_000))
                    .changed();
                changed |= ui
                    .checkbox(&mut preset.allow_enlarging, t!("batch-allow-enlarging"))
                    .changed();
            });
        }

        changed |= ui
            .add(egui::Slider::new(&mut preset.sharpen, 0..=100).text(t!("batch-sharpen")))
            .changed();
    });
    changed
}

fn called_what(ui: &mut egui::Ui, preset: &mut Preset, palette: &Palette) -> bool {
    let mut changed = false;
    section(ui, palette, &t!("batch-naming"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for value in Naming::ALL {
                changed |= ui
                    .selectable_value(&mut preset.naming, value, i18n::t(value.title_key()))
                    .changed();
            }
        });

        match preset.naming {
            Naming::Custom => {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut preset.custom_name)
                            .desired_width(220.0)
                            .hint_text(t!("batch-custom-name")),
                    )
                    .changed();
            }
            Naming::DateTaken => {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut preset.date_format)
                            .desired_width(300.0)
                            .hint_text(t!("batch-date-tokens")),
                    )
                    .changed();
                ui.label(
                    egui::RichText::new(t!("batch-date-tokens"))
                        .small()
                        .color(theme::color(palette.dim)),
                );
            }
            Naming::Original => {}
        }

        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut preset.prefix)
                        .desired_width(100.0)
                        .hint_text(t!("batch-prefix")),
                )
                .changed();
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut preset.suffix)
                        .desired_width(100.0)
                        .hint_text(t!("batch-suffix")),
                )
                .changed();
        });

        ui.horizontal(|ui| {
            changed |= ui
                .checkbox(&mut preset.numbering, t!("batch-numbering"))
                .changed();
            if preset.numbering {
                changed |= ui
                    .add(egui::DragValue::new(&mut preset.number_from).range(0..=999_999))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut preset.number_digits).range(1..=9))
                    .changed();
            }
        });
    });
    changed
}

fn carrying(ui: &mut egui::Ui, preset: &mut Preset, palette: &Palette) -> bool {
    let mut changed = false;
    section(ui, palette, &t!("batch-carry"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for value in Carry::ALL {
                changed |= ui
                    .selectable_value(&mut preset.carry, value, i18n::t(value.title_key()))
                    .changed();
            }
        });
    });
    changed
}

/// What would happen, said before it does.
fn summary(app: &App, ui: &mut egui::Ui, palette: &Palette) {
    let plan = &app.batch.plan;
    let mut parts = vec![t!("batch-will-write", count = plan.writes() as i64)];
    if plan.skips() > 0 {
        parts.push(t!("batch-will-skip", count = plan.skips() as i64));
    }

    if plan.overwrites() > 0 {
        parts.push(t!("batch-will-overwrite", count = plan.overwrites() as i64));
    }

    // Overwriting is the one that can lose work, so it is the one that is
    // not the same colour as the rest.
    let colour = if plan.overwrites() > 0 {
        palette.warn
    } else {
        palette.text
    };
    ui.label(egui::RichText::new(parts.join(" \u{b7} ")).color(theme::color(colour)));

    // The real path the first one would take, so nobody has to guess what
    // the settings add up to.
    if let Some(example) = plan.example() {
        ui.label(
            egui::RichText::new(example.to_string_lossy())
                .small()
                .color(theme::color(palette.dim)),
        );
    } else {
        ui.label(
            egui::RichText::new(t!("batch-nowhere"))
                .small()
                .color(theme::color(palette.warn)),
        );
    }
}

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

/// Sets the conversion going.
fn start(app: &mut App) {
    let plan = app.batch.plan.clone();
    let preset = app.batch.preset.clone();
    let threads = app.worker_threads();
    let waker = app.waker.clone();
    app.batch.summary.clear();

    app.batch.running = Some(app.tasks.spawn(t!("task-batch"), move |cancel, progress| {
        let outcome = photosite_batch::run(&plan, &preset, threads, cancel, progress);
        progress.report(
            outcome.written as u64,
            Some(outcome.written as u64),
            describe(&outcome),
        );
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }

        Ok(())
    }));
}

/// How a finished run went, in one line.
///
/// The failures are **named**, not counted. "Three failed" is a sentence
/// nobody can act on; "three failed: DSC_1.jpg, DSC_2.jpg" is one somebody
/// can go and look at. Only the first few — the rest are in the log, which
/// is where a hundred of anything belongs. The whole line travels back as
/// the task's own message, because a task already has somewhere to say how
/// it went.
pub fn describe(outcome: &photosite_batch::Outcome) -> String {
    let mut parts = vec![t!("batch-written", count = outcome.written as i64)];
    if outcome.skipped > 0 {
        parts.push(t!("batch-skipped", count = outcome.skipped as i64));
    }

    if outcome.failed > 0 {
        let mut failed = t!("batch-failed", count = outcome.failed as i64);
        let named: Vec<&str> = outcome.errors.iter().take(3).map(String::as_str).collect();
        if !named.is_empty() {
            failed.push_str(": ");
            failed.push_str(&named.join("; "));
        }

        parts.push(failed);
    }

    if outcome.cancelled {
        parts.push(t!("batch-cancelled"));
    }

    parts.join(" \u{b7} ")
}
