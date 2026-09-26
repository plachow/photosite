//! The Describe window.
//!
//! A model on this machine looks at each photograph in turn and fills in a
//! title, a description and keywords. Everything it writes goes through the
//! ordinary catalogue-and-outbox path, so a description arrives in the file
//! exactly as one typed by hand would.
//!
//! The window shows a **pace and a finishing time**, and that is not
//! decoration: a vision model takes tens of seconds a photograph, so a run
//! over a folder is minutes to hours, and the only question anybody has
//! while it goes is whether to wait or go and do something else.

use crate::{App, theme};
use eframe::egui;
use photosite_ai::{Mode, Provider, Server, model_field, model_of};
use photosite_core::theme::Palette;
use photosite_core::{Catalog, Gazetteer, i18n, t};
use std::time::Duration;

/// The window's state.
#[derive(Debug, Default)]
pub struct Describe {
    pub open: bool,
    /// What the server offers, when it has been asked.
    pub models: Vec<String>,
    /// Why the models could not be listed.
    pub trouble: Option<String>,
    pub running: Option<u64>,
    pub summary: String,
    /// What the last photograph came back with, so somebody watching can see
    /// that the answers are worth having before leaving it running.
    pub last: String,
}

pub fn open(app: &mut App) {
    app.describe.open = true;
    if app.describe.models.is_empty() {
        ask_what_it_has(app);
    }
}

/// Asks the server which models it has.
fn ask_what_it_has(app: &mut App) {
    let server = Server::of(&app.settings.ai);
    if !server.admitted() {
        // No key, so nothing to ask with. Said here rather than as a
        // failed request, because the failure would be a 401 dressed up.
        app.describe.models.clear();
        app.describe.trouble = Some(t!("ai-no-key"));
        return;
    }

    let timeout = Duration::from_secs(10);
    match photosite_ai::models(&server, timeout) {
        Ok(models) => {
            app.describe.trouble = (models.is_empty()).then(|| t!("ai-no-models"));
            // Nothing chosen yet, and the server has something that can see:
            // choosing it is better than an empty box and a shrug. Only for
            // Ollama, whose list puts the vision models first; a cloud
            // provider's list is alphabetical and its first entry is as
            // likely to be an embedding model as anything.
            let model = model_field(&mut app.settings.ai);
            if !server.provider.remote()
                && model.trim().is_empty()
                && let Some(first) = models.first()
            {
                *model = first.clone();
            }

            app.describe.models = models;
        }
        Err(error) => {
            app.describe.models.clear();
            app.describe.trouble = Some(format!("{error:#}"));
        }
    }
}

pub fn window(app: &mut App, ctx: &egui::Context, palette: &Palette) {
    if !app.describe.open {
        return;
    }

    let mut open = true;
    let mut refresh = false;
    let mut go = false;
    let running = app
        .describe
        .running
        .and_then(|id| app.tasks.snapshot().into_iter().find(|task| task.id == id))
        .filter(|task| !task.finished);
    let waiting = chosen(app).len();

    egui::Window::new(t!("ai-title"))
        .open(&mut open)
        .default_width(520.0)
        .show(ctx, |ui| {
            // Who is asked. Switching is a change of promise — the local
            // model keeps the photographs at home, the others do not — so
            // it sits first and is said in words.
            ui.horizontal_wrapped(|ui| {
                ui.label(t!("ai-provider"));
                let chosen = Provider::from_id(&app.settings.ai.provider);
                for provider in Provider::ALL {
                    if ui
                        .selectable_label(provider == chosen, i18n::t(provider.title_key()))
                        .clicked()
                        && provider != chosen
                    {
                        app.settings.ai.provider = provider.id().to_owned();
                        app.describe.models.clear();
                        app.describe.trouble = None;
                        refresh = true;
                    }
                }
            });

            let provider = Provider::from_id(&app.settings.ai.provider);
            if provider.remote() {
                ui.horizontal(|ui| {
                    ui.label(t!("ai-api-key"));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut app.settings.ai.api_key)
                                .password(true)
                                .desired_width(220.0),
                        )
                        .lost_focus()
                    {
                        refresh = true;
                    }

                    ui.label(
                        egui::RichText::new(t!("ai-api-key-hint"))
                            .small()
                            .color(theme::color(palette.dim)),
                    );
                });
            }

            ui.horizontal(|ui| {
                ui.label(if provider.remote() {
                    t!("ai-cloud-endpoint")
                } else {
                    t!("ai-endpoint")
                });
                let endpoint = if provider.remote() {
                    &mut app.settings.ai.cloud_endpoint
                } else {
                    &mut app.settings.ai.endpoint
                };
                if ui
                    .add(
                        egui::TextEdit::singleline(endpoint)
                            .hint_text(provider.default_endpoint())
                            .desired_width(220.0),
                    )
                    .lost_focus()
                {
                    refresh = true;
                }

                if ui.button(t!("ai-refresh")).clicked() {
                    refresh = true;
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label(t!("ai-model"));
                let model = model_field(&mut app.settings.ai);
                // A cloud provider lists everything it has, embeddings and
                // all, by the dozen; there the box is where a name is
                // typed and the chips narrow to what has been typed so far.
                if app.describe.models.is_empty() || provider.remote() {
                    ui.add(egui::TextEdit::singleline(model).desired_width(220.0));
                }

                let typed = model.trim().to_ascii_lowercase();
                for name in &app.describe.models {
                    let on = model == name;
                    if provider.remote() && !on && !name.to_ascii_lowercase().contains(&typed) {
                        continue;
                    }

                    if ui.selectable_label(on, name).clicked() {
                        *model = name.clone();
                    }
                }
            });

            if let Some(trouble) = &app.describe.trouble {
                ui.label(egui::RichText::new(trouble).color(theme::color(palette.warn)));
            }

            ui.horizontal(|ui| {
                ui.label(t!("ai-language"));
                ui.add(
                    egui::TextEdit::singleline(&mut app.settings.ai.language).desired_width(140.0),
                );
                for mode in [Mode::FillEmpty, Mode::Overwrite] {
                    let on = app.settings.ai.overwrite == (mode == Mode::Overwrite);
                    if ui.selectable_label(on, i18n::t(mode.title_key())).clicked() {
                        app.settings.ai.overwrite = mode == Mode::Overwrite;
                    }
                }
            });

            // Whether there is a gazetteer, said plainly. A describer with
            // no places is a describer that will not name any, and finding
            // that out from a hundred vague descriptions is the wrong way to
            // learn it.
            ui.label(
                egui::RichText::new(match &app.places {
                    Some(places) => t!("ai-places-from", source = places.source.clone()),
                    None => t!(
                        "ai-no-places",
                        folder = app.paths.places().display().to_string()
                    ),
                })
                .small()
                .color(theme::color(palette.dim)),
            );

            ui.separator();
            match &running {
                Some(task) => {
                    ui.horizontal(|ui| {
                        if ui.button(t!("ai-stop")).clicked()
                            && let Some(id) = app.describe.running
                        {
                            app.tasks.cancel(id);
                        }

                        if let Some(fraction) = task.fraction() {
                            ui.add(egui::ProgressBar::new(fraction).desired_width(200.0));
                        }
                    });

                    ui.label(
                        egui::RichText::new(&task.message)
                            .small()
                            .color(theme::color(palette.dim)),
                    );
                }
                None => {
                    ui.horizontal(|ui| {
                        let ready = waiting > 0
                            && !model_of(&app.settings.ai).trim().is_empty()
                            && Server::of(&app.settings.ai).admitted();
                        if ui
                            .add_enabled(ready, egui::Button::new(t!("ai-run")))
                            .clicked()
                        {
                            go = true;
                        }

                        ui.label(
                            egui::RichText::new(t!("ai-waiting", count = waiting as i64))
                                .small()
                                .color(theme::color(palette.dim)),
                        );
                    });
                }
            }

            if !app.describe.summary.is_empty() {
                ui.label(
                    egui::RichText::new(&app.describe.summary).color(theme::color(palette.text)),
                );
            }

            if !app.describe.last.is_empty() {
                ui.label(
                    egui::RichText::new(&app.describe.last)
                        .small()
                        .color(theme::color(palette.dim)),
                );
            }
        });

    app.describe.open = open;
    if refresh {
        ask_what_it_has(app);
    }

    if go {
        start(app);
    }
}

/// Which photographs a run would work on: the selection, or the whole
/// folder when nothing is selected.
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

/// Sets a run going.
fn start(app: &mut App) {
    let photos = chosen(app);
    let path = app.paths.catalog();
    let places = app.places.clone();
    let settings = app.settings.ai.clone();
    let mode = if settings.overwrite {
        Mode::Overwrite
    } else {
        Mode::FillEmpty
    };
    // The model answers in the chosen language; when that is not English,
    // the same call also returns an English description for the catalogue,
    // so a library described in one language is searchable in both.
    let english_too = !settings.language.trim().eq_ignore_ascii_case("english");
    let request_size = settings.request_size.clamp(256, 4096) as u32;
    let timeout = Duration::from_secs(settings.timeout_seconds.clamp(10, 3600) as u64);
    let waker = app.waker.clone();

    app.describe.summary.clear();
    app.describe.last.clear();
    app.describe.running = Some(app.tasks.spawn(t!("task-describe"), move |cancel, progress| {
        let mut catalog = Catalog::open(&path)?;
        let total = photos.len() as u64;
        let started = std::time::Instant::now();
        let mut described = 0u64;
        let mut skipped = 0u64;
        let mut failed = 0u64;
        let mut attempted = 0u64;
        let mut stopped: Option<String> = None;

        for (at, photo) in photos.iter().enumerate() {
            if cancel.cancelled() {
                break;
            }

            if photosite_ai::should_skip(
                mode,
                photo.organisation.title.as_deref(),
                photo.organisation.description.as_deref(),
            ) {
                skipped += 1;
                progress.report(at as u64 + 1, Some(total), pace(started, attempted, total));
                continue;
            }

            let outcome = one(
                &mut catalog,
                photo,
                &settings,
                english_too,
                mode,
                places.as_deref(),
                request_size,
                timeout,
            );
            attempted += 1;
            match outcome {
                Ok(said) => {
                    described += 1;
                    progress.report(
                        at as u64 + 1,
                        Some(total),
                        format!("{} \u{b7} {said}", pace(started, attempted, total)),
                    );
                }
                Err(error) => {
                    failed += 1;
                    let error = format!("{error:#}");
                    tracing::warn!(path = %photo.path.display(), %error, "the photograph could not be described");
                    // Three failures before a single success point at the
                    // configuration, not at the photographs — and every
                    // remaining one would wait out the same timeout.
                    if described == 0 && failed == 3 {
                        stopped = Some(t!("ai-all-failing"));
                        break;
                    }

                    progress.report(at as u64 + 1, Some(total), error);
                }
            }

            if let Some(ctx) = waker.get() {
                ctx.request_repaint();
            }
        }

        let mut said = vec![
            t!("ai-described", count = described as i64),
            t!("ai-in", seconds = started.elapsed().as_secs_f64()),
        ];
        if skipped > 0 {
            said.push(t!("ai-skipped", count = skipped as i64));
        }

        if failed > 0 {
            said.push(t!("ai-failed", count = failed as i64));
        }

        if let Some(stopped) = stopped {
            said.push(stopped);
        }

        progress.report(total, Some(total), said.join(" \u{b7} "));
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }

        Ok(())
    }));
}

/// One photograph: ask, and write what came back.
#[allow(clippy::too_many_arguments)]
fn one(
    catalog: &mut Catalog,
    photo: &photosite_core::Photo,
    settings: &photosite_core::settings::Ai,
    english_too: bool,
    mode: Mode,
    places: Option<&Gazetteer>,
    request_size: u32,
    timeout: Duration,
) -> anyhow::Result<String> {
    // Never beyond its real size: a small photograph is sent as it is rather
    // than blown up to fill a number.
    let want = match photo.shown() {
        Some((width, height)) => request_size.min(width.max(height)),
        None => request_size,
    };
    let frame = photosite_image::sized(&photo.path, want)?;
    let jpeg = photosite_image::encode::encode(&frame, photosite_image::encode::Format::Jpeg, 85)?;

    // The place is resolved from the coordinates the *catalogue* holds,
    // which may be somebody's correction rather than what the file says.
    let place = photo
        .place
        .zip(places)
        .and_then(|(place, places)| places.nearest(place.latitude, place.longitude));
    let direction = place
        .as_ref()
        .map(|place| i18n::t(photosite_core::gazetteer::compass(place.bearing)))
        .unwrap_or_default();

    let insights = photosite_ai::describe(
        &Server::of(settings),
        model_of(settings),
        &jpeg,
        &settings.language,
        english_too,
        place.as_ref(),
        photo.verdict.is_doubted(),
        &direction,
        timeout,
    )?;

    photosite_ai::apply(catalog, photo, &insights, mode, crate::now())?;
    Ok(insights
        .title
        .clone()
        .unwrap_or_else(|| insights.description.clone().unwrap_or_default()))
}

/// How fast it is going and when it will be done.
///
/// The one number anybody watching actually wants: a run of four hundred
/// photographs at forty seconds each is four hours, and knowing that is the
/// difference between waiting and going away.
fn pace(started: std::time::Instant, attempted: u64, total: u64) -> String {
    if attempted == 0 {
        return String::new();
    }

    let each = started.elapsed().as_secs_f64() / attempted as f64;
    let left = total.saturating_sub(attempted) as f64 * each;
    t!("ai-pace", each = each, left = left / 60.0)
}
