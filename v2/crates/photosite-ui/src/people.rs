//! The People window, and the sweep that fills it.
//!
//! Three piles, in the order somebody works through them:
//!
//! 1. **Suggestions** — a face that is probably somebody already named. One
//!    question, two answers, and nothing is written until the yes.
//! 2. **Groups** — faces nobody has named, gathered by likeness. A whole
//!    group is named at once, which is the only thing that makes naming a
//!    library bearable.
//! 3. **People** — who is known, and every face said to be them, so a wrong
//!    match can be taken back.
//!
//! The sweep itself is the expensive part and runs as an ordinary background
//! task: it decodes on every core, and only the writing is serialised,
//! because there is one catalogue and it is the one thing that cannot be
//! done twice at once.

use crate::{App, dialog, theme};
use eframe::egui;
use photosite_core::people::{Face, Person};
use photosite_core::theme::Palette;
use photosite_core::{Catalog, TaskStatus, t};
use photosite_faces::fetch::{self, Model};
use photosite_faces::sweep::Scope;
use photosite_faces::{Availability, cluster};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// How many groups are offered at once.
///
/// A first sweep of a large library finds thousands of strangers, and a
/// window listing all of them is one nobody scrolls to the end of. The
/// biggest groups come first, which are the ones worth naming.
const GROUPS_SHOWN: usize = 40;

/// How many faces of one group are drawn. The rest are counted.
const FACES_PER_GROUP: usize = 12;

/// The side of a face chip, and of the one clicked to be looked at.
const CHIP: f32 = 96.0;
const PEEK: f32 = 288.0;

/// The side of the closer look that appears under the pointer.
const HOVER: f32 = 256.0;

/// How many of a person's faces the window shows, so a wrong match can be
/// found and taken back.
const FACES_PER_PERSON: usize = 60;

/// How many faces are read out of the catalogue to be grouped.
///
/// Clustering is quadratic in the number of faces, so this is a real
/// ceiling and not a tidy round number: it is what keeps opening the window
/// on a library of a hundred thousand photographs from taking a minute.
const FACES_CLUSTERED: usize = 3000;

/// What the window is showing, held so it is not rebuilt every frame.
///
/// Clustering a few thousand faces is a fraction of a second and would be
/// entirely wasted sixty times a second.
#[derive(Debug, Default)]
pub struct People {
    pub open: bool,
    /// Whether what is shown is still what the catalogue says.
    pub stale: bool,
    pub people: Vec<Person>,
    pub groups: Vec<Group>,
    pub suggestions: Vec<Suggestion>,
    /// How many unnamed faces there are in total, of which only the largest
    /// groups are shown.
    pub unnamed: usize,
    /// Whose faces are being looked at, and which they are.
    pub showing: Option<i64>,
    pub showing_faces: Vec<Face>,
    /// A name being typed, and for which group.
    pub naming: Option<(usize, String)>,
    /// A person being renamed.
    pub renaming: Option<(i64, String)>,
    /// The running sweep, if there is one.
    pub scanning: Option<u64>,
    /// What the last sweep found.
    pub summary: String,
    /// Why there is no sweep to be had.
    pub unavailable: Option<String>,
    /// The running download of the models, if there is one.
    pub fetching: Option<u64>,
    /// Which of the four are not on disk, so the window can offer them.
    pub missing: Vec<&'static Model>,
    /// Whether the folder has been looked at since the window was opened
    /// or the models last changed. Once, not every frame: four stats a
    /// frame is not nothing on a network drive.
    pub checked: bool,
    /// Why the last download did not finish.
    pub trouble: Option<String>,
    /// Look at every photograph again rather than only the ones no sweep
    /// has seen. Not a setting: it is asked for a run, not for good.
    pub rescan: bool,
    /// The face drawn large, to be looked at. One at a time.
    pub peek: Option<i64>,
}

/// Faces the arithmetic believes are one person.
#[derive(Debug, Clone)]
pub struct Group {
    pub faces: Vec<Face>,
}

/// A face that is probably somebody already named.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub person: i64,
    pub name: String,
    pub faces: Vec<Face>,
}

impl People {
    /// Every photograph a face chip will be cut from, so the decoding
    /// threads know what to fetch.
    fn wanted(&self) -> Vec<PathBuf> {
        let mut wanted: Vec<PathBuf> = Vec::new();
        let mut add = |faces: &[Face], limit: usize| {
            for face in faces.iter().take(limit) {
                if !wanted.contains(&face.path) {
                    wanted.push(face.path.clone());
                }
            }
        };

        for suggestion in &self.suggestions {
            add(&suggestion.faces, FACES_PER_GROUP);
        }

        for group in &self.groups {
            add(&group.faces, FACES_PER_GROUP);
        }

        add(&self.showing_faces, FACES_PER_PERSON);
        wanted
    }
}

/// Reads the piles out of the catalogue and groups the strangers.
pub fn refresh(app: &mut App) {
    let Some(catalog) = app.catalog.as_ref() else {
        return;
    };

    let people = catalog.people().unwrap_or_default();
    let names: HashMap<i64, String> = people
        .iter()
        .map(|person| (person.id, person.name.clone()))
        .collect();

    let unnamed = catalog.unnamed_faces(FACES_CLUSTERED).unwrap_or_default();
    let groups: Vec<Group> = cluster::cluster(&unnamed, cluster::GROUPING, |face| {
        (face.confidence, face.embedding.as_slice())
    })
    .into_iter()
    // A group of one is not a group: it says nothing more than the face
    // already did, and forty of them push the real groups off the
    // window.
    .filter(|found| found.members.len() > 1)
    .take(GROUPS_SHOWN)
    .map(|found| Group {
        faces: found.members,
    })
    .collect();

    // Suggestions are gathered per person, so the question is asked once
    // for somebody with nine borderline faces rather than nine times.
    let mut suggestions: Vec<Suggestion> = Vec::new();
    for face in catalog.suggested_faces(FACES_CLUSTERED).unwrap_or_default() {
        let Some(person) = face.suggested else {
            continue;
        };
        let Some(name) = names.get(&person) else {
            continue;
        };

        match suggestions
            .iter_mut()
            .find(|suggestion| suggestion.person == person)
        {
            Some(suggestion) => suggestion.faces.push(face),
            None => suggestions.push(Suggestion {
                person,
                name: name.clone(),
                faces: vec![face],
            }),
        }
    }

    let showing_faces = match app.people.showing {
        Some(person) => catalog
            .faces_of_person(person, FACES_PER_PERSON)
            .unwrap_or_default(),
        None => Vec::new(),
    };

    app.people.people = people;
    app.people.groups = groups;
    app.people.suggestions = suggestions;
    app.people.unnamed = unnamed.len();
    app.people.showing_faces = showing_faces;
    app.people.stale = false;
}

/// One thing somebody asked the window to do.
///
/// Collected while drawing and carried out afterwards, because the drawing
/// holds a borrow of everything and the catalogue needs one of its own.
enum Doing {
    Name(usize, String),
    NameFrom(usize, i64),
    Ignore(usize),
    Confirm(usize),
    Reject(usize),
    Show(Option<i64>),
    Rename(i64, String),
    Forget(i64),
    Unname(i64),
    Scan,
    Stop,
    Fetch,
    StopFetch,
}

pub fn window(app: &mut App, ctx: &egui::Context, palette: &Palette) {
    if !app.people.open {
        return;
    }

    if app.people.stale {
        refresh(app);
    }

    // The first thing the window does is look for the models, so that a
    // fresh install is told what is missing — and offered it — before
    // anybody presses anything.
    if !app.people.checked {
        check(app);
    }

    let mut open = true;
    let mut doing: Option<Doing> = None;
    let scanning = app
        .people
        .scanning
        .and_then(|id| app.tasks.snapshot().into_iter().find(|task| task.id == id))
        .filter(|task| !task.finished);
    let fetching = app
        .people
        .fetching
        .and_then(|id| app.tasks.snapshot().into_iter().find(|task| task.id == id))
        .filter(|task| !task.finished);

    let scope_said = if app.selection.is_empty() {
        t!("people-in-folder")
    } else {
        t!("people-in-selection", count = app.selection.len() as i64)
    };

    let shown = dialog::window(ctx, &app.settings.window, "people", t!("people-title"))
        .open(&mut open)
        .default_size([760.0, 640.0])
        .min_size([520.0, 360.0])
        .vscroll(false)
        .show(ctx, |ui| {
            dialog::spacious(ui);

            // The footer first: a panel inside a window is laid out before
            // whatever is left in the middle, and the piles scroll in what
            // is left.
            egui::Panel::bottom("people-footer")
                .frame(egui::Frame::new())
                .resizable(false)
                .show_separator_line(false)
                .show(ui, |ui| {
                    dialog::footer(ui, palette, |ui| match &scanning {
                        Some(task) => {
                            ui.horizontal(|ui| {
                                if ui.button(t!("people-stop")).clicked() {
                                    doing = Some(Doing::Stop);
                                }

                                if let Some(fraction) = task.fraction() {
                                    ui.add(
                                        egui::ProgressBar::new(fraction)
                                            .desired_width(ui.available_width().max(120.0)),
                                    );
                                }
                            });
                            dialog::hint(ui, palette, task.message.clone());
                        }
                        None => {
                            ui.horizontal_wrapped(|ui| {
                                let ready = app.people.unavailable.is_none() && fetching.is_none();
                                if dialog::primary(ui, palette, t!("people-scan"), ready).clicked()
                                {
                                    doing = Some(Doing::Scan);
                                }

                                // Which photographs, and whether the ones
                                // already seen are seen again — the same
                                // two choices the Describe window offers.
                                for again in [false, true] {
                                    let label = if again {
                                        t!("people-rescan")
                                    } else {
                                        t!("people-only-new")
                                    };
                                    if ui
                                        .selectable_label(app.people.rescan == again, label)
                                        .clicked()
                                    {
                                        app.people.rescan = again;
                                    }
                                }

                                dialog::hint(ui, palette, scope_said.clone());
                            });
                        }
                    });
                });

            if let Some(why) = &app.people.unavailable {
                ui.label(egui::RichText::new(why).color(theme::color(palette.warn)));
            } else if scanning.is_none() && !app.people.summary.is_empty() {
                dialog::hint(ui, palette, app.people.summary.clone());
            }

            if scanning.is_none() {
                offer(app, ui, palette, fetching.as_ref(), &mut doing);
            }

            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    suggestions(app, ui, palette, &mut doing);
                    groups(app, ui, palette, &mut doing);
                    known(app, ui, palette, &mut doing);
                });
        });
    dialog::remember(&mut app.settings.window, "people", &shown);

    app.people.open = open;

    if !open {
        // Looked at again when it is next opened: the models may have
        // been put there by hand in the meantime.
        app.people.checked = false;
    }

    if let Some(doing) = doing {
        act(app, doing);
    }

    // Whatever the window is showing needs its photographs decoded. This is
    // added to the wishlist by the same route the grid uses, so a face chip
    // and a tile compete for the same threads rather than for two sets.
    app.wanted_faces = app.people.wanted();
}

/// What is not on disk, and the button that fetches it — or, while it is
/// being fetched, how far that has got.
///
/// The offer says what each file is, who publishes it, under what licence
/// and how big it is, because seventy megabytes of somebody else's
/// binaries is a thing to agree to rather than have happen. The two
/// expression models are offered on their own when only they are missing:
/// a sweep runs without them, and says so, but a smile it cannot read is a
/// smile nobody gets to filter on.
fn offer(
    app: &App,
    ui: &mut egui::Ui,
    palette: &Palette,
    fetching: Option<&TaskStatus>,
    doing: &mut Option<Doing>,
) {
    if let Some(task) = fetching {
        ui.horizontal(|ui| {
            if ui.button(t!("people-stop")).clicked() {
                *doing = Some(Doing::StopFetch);
            }

            if let Some(fraction) = task.fraction() {
                ui.add(egui::ProgressBar::new(fraction).desired_width(180.0));
            }

            ui.label(
                egui::RichText::new(&task.message)
                    .small()
                    .color(theme::color(palette.dim)),
            );
        });
        return;
    }

    if app.people.missing.is_empty() {
        return;
    }

    if app.people.unavailable.is_none() {
        ui.label(
            egui::RichText::new(t!("people-fetch-optional"))
                .small()
                .color(theme::color(palette.dim)),
        );
    }

    ui.horizontal(|ui| {
        let size = fetch::megabytes(fetch::bytes_of(&app.people.missing));
        if ui.button(t!("people-fetch", size = size)).clicked() {
            *doing = Some(Doing::Fetch);
        }

        ui.label(
            egui::RichText::new(t!(
                "people-fetch-where",
                folder = app.model_folder().to_string_lossy().into_owned()
            ))
            .small()
            .color(theme::color(palette.dim)),
        );
    });

    for model in &app.people.missing {
        ui.label(
            egui::RichText::new(t!(
                "people-fetch-model",
                name = model.name,
                source = model.source,
                licence = model.licence,
                size = fetch::megabytes(model.bytes)
            ))
            .small()
            .color(theme::color(palette.dim)),
        );
    }

    if let Some(trouble) = &app.people.trouble {
        ui.label(egui::RichText::new(trouble).color(theme::color(palette.warn)));
    }
}

fn suggestions(app: &mut App, ui: &mut egui::Ui, palette: &Palette, doing: &mut Option<Doing>) {
    if app.people.suggestions.is_empty() {
        return;
    }

    heading(ui, palette, &t!("people-suggestions"));
    let mut peek = None;
    for (index, suggestion) in app.people.suggestions.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(t!("people-is-this", name = suggestion.name.clone()));
            if ui.button(t!("people-yes")).clicked() {
                *doing = Some(Doing::Confirm(index));
            }

            if ui.button(t!("people-no")).clicked() {
                *doing = Some(Doing::Reject(index));
            }
        });

        if let Some(face) = chips(app, ui, palette, &suggestion.faces, FACES_PER_GROUP) {
            peek = Some(face);
        }

        ui.add_space(8.0);
    }

    if let Some(face) = peek {
        toggle_peek(app, face);
    }

    ui.separator();
}

fn groups(app: &mut App, ui: &mut egui::Ui, palette: &Palette, doing: &mut Option<Doing>) {
    if app.people.groups.is_empty() {
        if app.people.suggestions.is_empty() {
            ui.label(
                egui::RichText::new(t!("people-nothing-to-name")).color(theme::color(palette.dim)),
            );
        }

        return;
    }

    heading(
        ui,
        palette,
        &t!("people-groups", count = app.people.unnamed as i64),
    );

    let names: Vec<(i64, String)> = app
        .people
        .people
        .iter()
        .map(|person| (person.id, person.name.clone()))
        .collect();

    for index in 0..app.people.groups.len() {
        let faces = app.people.groups[index].faces.clone();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t!("people-group-count", count = faces.len() as i64))
                    .small()
                    .color(theme::color(palette.dim)),
            );

            // Naming a group is one click when the person is already known,
            // which is what the chips are for. Typing is only for somebody
            // nobody has named yet.
            for (id, name) in &names {
                if ui.button(name).clicked() {
                    *doing = Some(Doing::NameFrom(index, *id));
                }
            }

            let typing = matches!(app.people.naming, Some((at, _)) if at == index);
            if typing {
                let mut name = match &app.people.naming {
                    Some((_, name)) => name.clone(),
                    None => String::new(),
                };
                match name_box(ui, &mut name) {
                    Typed::Entered => *doing = Some(Doing::Name(index, name)),
                    Typed::Cancelled => app.people.naming = None,
                    Typed::Typing => app.people.naming = Some((index, name)),
                }
            } else if ui.button(t!("people-new-person")).clicked() {
                app.people.naming = Some((index, String::new()));
            }

            if ui.button(t!("people-not-a-person")).clicked() {
                *doing = Some(Doing::Ignore(index));
            }
        });

        if let Some(face) = chips(app, ui, palette, &faces, FACES_PER_GROUP) {
            toggle_peek(app, face);
        }

        ui.add_space(10.0);
    }

    ui.separator();
}

fn known(app: &mut App, ui: &mut egui::Ui, palette: &Palette, doing: &mut Option<Doing>) {
    if app.people.people.is_empty() {
        return;
    }

    heading(ui, palette, &t!("people-known"));
    let people = app.people.people.clone();
    for person in &people {
        ui.horizontal(|ui| {
            let showing = app.people.showing == Some(person.id);
            let label = format!("{} ({})", person.name, person.faces);
            if ui.selectable_label(showing, label).clicked() {
                *doing = Some(Doing::Show(if showing { None } else { Some(person.id) }));
            }

            match &app.people.renaming {
                Some((id, name)) if *id == person.id => {
                    let mut name = name.clone();
                    match name_box(ui, &mut name) {
                        Typed::Entered => *doing = Some(Doing::Rename(person.id, name)),
                        Typed::Cancelled => app.people.renaming = None,
                        Typed::Typing => app.people.renaming = Some((person.id, name)),
                    }
                }
                _ => {
                    if ui.button(t!("people-rename")).clicked() {
                        app.people.renaming = Some((person.id, person.name.clone()));
                    }
                }
            }

            if ui.button(t!("people-forget")).clicked() {
                *doing = Some(Doing::Forget(person.id));
            }
        });

        if app.people.showing == Some(person.id) {
            let faces = app.people.showing_faces.clone();
            if faces.is_empty() {
                ui.label(
                    egui::RichText::new(t!("people-no-faces"))
                        .small()
                        .color(theme::color(palette.dim)),
                );
            } else {
                ui.label(
                    egui::RichText::new(t!("people-click-to-remove"))
                        .small()
                        .color(theme::color(palette.dim)),
                );
                if let Some(face) = clickable(app, ui, palette, &faces, FACES_PER_PERSON) {
                    *doing = Some(Doing::Unname(face));
                }
            }

            ui.add_space(8.0);
        }
    }
}

fn heading(ui: &mut egui::Ui, palette: &Palette, title: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(theme::color(palette.text)),
    );
}

/// What came of a name being typed.
enum Typed {
    Typing,
    Entered,
    Cancelled,
}

/// The box a name is typed into, with the two ways out of it: Enter or OK
/// to name, Escape or Cancel to think better of it.
fn name_box(ui: &mut egui::Ui, name: &mut String) -> Typed {
    let field = ui.add(
        egui::TextEdit::singleline(name)
            .desired_width(160.0)
            .hint_text(t!("people-new-name")),
    );
    field.request_focus();
    // Whether it has the keys now, not whether it lost them: it is handed
    // them back every frame, so it never does — which is exactly why Enter
    // did nothing here for a while.
    let entered = (field.has_focus() || field.lost_focus())
        && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let confirmed = ui.button(t!("people-name-ok")).clicked();
    let cancelled = ui.button(t!("people-cancel")).clicked()
        || ui.input(|input| input.key_pressed(egui::Key::Escape));
    if cancelled {
        Typed::Cancelled
    } else if (entered || confirmed) && !name.trim().is_empty() {
        Typed::Entered
    } else {
        Typed::Typing
    }
}

/// Draws one face large, or stops doing so.
fn toggle_peek(app: &mut App, face: i64) {
    app.people.peek = if app.people.peek == Some(face) {
        None
    } else {
        Some(face)
    };
}

/// A row of face thumbnails, cut out of the photographs they came from.
/// Returns the one clicked, which the caller draws large.
fn chips(
    app: &App,
    ui: &mut egui::Ui,
    palette: &Palette,
    faces: &[Face],
    limit: usize,
) -> Option<i64> {
    clickable_inner(app, ui, palette, faces, limit, false)
}

/// The same, where a click means something: taking the face off the
/// person. Returns which was clicked.
fn clickable(
    app: &App,
    ui: &mut egui::Ui,
    palette: &Palette,
    faces: &[Face],
    limit: usize,
) -> Option<i64> {
    clickable_inner(app, ui, palette, faces, limit, true)
}

fn clickable_inner(
    app: &App,
    ui: &mut egui::Ui,
    palette: &Palette,
    faces: &[Face],
    limit: usize,
    removes: bool,
) -> Option<i64> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for face in faces.iter().take(limit) {
            let side = if !removes && app.people.peek == Some(face.id) {
                PEEK
            } else {
                CHIP
            };
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
            if response.clicked() {
                clicked = Some(face.id);
            }

            match app.face_texture(face) {
                Some((texture, uv)) => {
                    ui.painter().image(texture, rect, uv, egui::Color32::WHITE);
                    // A closer look under the pointer, without clicking
                    // anything: the face at a size a face can be seen at,
                    // and which photograph it came from.
                    response.on_hover_ui(|ui| {
                        let (big, _) =
                            ui.allocate_exact_size(egui::vec2(HOVER, HOVER), egui::Sense::hover());
                        ui.painter().image(texture, big, uv, egui::Color32::WHITE);
                        ui.label(
                            face.path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        );
                        if !removes {
                            ui.label(
                                egui::RichText::new(t!("people-peek"))
                                    .small()
                                    .color(theme::color(palette.dim)),
                            );
                        }
                    });
                }
                None => {
                    // Nothing to draw yet. A frame rather than a hole, so
                    // the row does not jump about as decoding finishes.
                    ui.painter()
                        .rect_filled(rect, 2.0, theme::color(palette.panel));
                }
            }
        }

        if faces.len() > limit {
            ui.label(
                egui::RichText::new(t!("people-and-more", count = (faces.len() - limit) as i64))
                    .small()
                    .color(theme::color(palette.dim)),
            );
        }
    });

    clicked
}

/// Carries out what the window was asked for, and asks it to read itself
/// again.
fn act(app: &mut App, doing: Doing) {
    let now = crate::now();
    let outcome = match doing {
        Doing::Scan => {
            start(app);
            return;
        }
        Doing::Stop => {
            if let Some(id) = app.people.scanning {
                app.tasks.cancel(id);
            }

            return;
        }
        Doing::Fetch => {
            start_fetch(app);
            return;
        }
        Doing::StopFetch => {
            if let Some(id) = app.people.fetching {
                app.tasks.cancel(id);
            }

            return;
        }
        Doing::Show(person) => {
            app.people.showing = person;
            app.people.stale = true;
            return;
        }
        Doing::Name(index, name) => {
            app.people.naming = None;
            let faces = face_ids(app, index);
            with_catalog(app, |catalog| {
                let person = catalog.person_named(&name)?;
                catalog.name_faces(&faces, person, now)
            })
        }
        Doing::NameFrom(index, person) => {
            app.people.naming = None;
            let faces = face_ids(app, index);
            with_catalog(app, |catalog| catalog.name_faces(&faces, person, now))
        }
        Doing::Ignore(index) => {
            let faces = face_ids(app, index);
            with_catalog(app, |catalog| {
                catalog.ignore_faces(&faces)?;
                Ok(Vec::new())
            })
        }
        Doing::Confirm(index) => {
            let Some(suggestion) = app.people.suggestions.get(index).cloned() else {
                return;
            };
            let faces: Vec<i64> = suggestion.faces.iter().map(|face| face.id).collect();
            with_catalog(app, |catalog| {
                catalog.name_faces(&faces, suggestion.person, now)
            })
        }
        Doing::Reject(index) => {
            let Some(suggestion) = app.people.suggestions.get(index).cloned() else {
                return;
            };
            let faces: Vec<i64> = suggestion.faces.iter().map(|face| face.id).collect();
            with_catalog(app, |catalog| {
                catalog.clear_suggestions(&faces)?;
                Ok(Vec::new())
            })
        }
        Doing::Rename(person, name) => {
            app.people.renaming = None;
            // Renaming a person has to reach their photographs: the old name
            // is a keyword on every one of them and the new one is not.
            with_catalog(app, |catalog| {
                let was = catalog
                    .people()?
                    .into_iter()
                    .find(|had| had.id == person)
                    .map(|had| had.name);
                catalog.rename_person(person, &name)?;
                let photos = photos_of(catalog, person)?;
                if let Some(was) = was {
                    catalog.remove_keywords(&photos, &[was])?;
                }

                catalog.add_keywords(&photos, &[name.trim()])?;
                catalog.enqueue(&photos, now)?;
                Ok(photos)
            })
        }
        Doing::Forget(person) => with_catalog(app, |catalog| {
            let photos = photos_of(catalog, person)?;
            let name = catalog
                .people()?
                .into_iter()
                .find(|had| had.id == person)
                .map(|had| had.name);
            catalog.delete_person(person)?;
            if let Some(name) = name {
                catalog.remove_keywords(&photos, &[name])?;
            }

            catalog.enqueue(&photos, now)?;
            Ok(photos)
        }),
        Doing::Unname(face) => with_catalog(app, |catalog| catalog.unname_faces(&[face], now)),
    };

    match outcome {
        Ok(touched) => {
            if !touched.is_empty() {
                app.start_writing();
            }

            app.people.stale = true;
            // Who is on which photograph has changed, so the folder is read
            // again: the badges, the details and the filter all come off it.
            app.preview_faces_of = None;
            app.relist();
        }
        Err(error) => {
            let error = format!("{error:#}");
            tracing::error!(%error, "the change to the people could not be made");
            app.status = error;
        }
    }
}

fn face_ids(app: &App, index: usize) -> Vec<i64> {
    app.people
        .groups
        .get(index)
        .map(|group| group.faces.iter().map(|face| face.id).collect())
        .unwrap_or_default()
}

/// Every photograph a person is on — what a rename or a removal has to
/// reach.
fn photos_of(
    catalog: &Catalog,
    person: i64,
) -> anyhow::Result<Vec<photosite_core::domain::PhotoId>> {
    let faces = catalog.faces_of_person(person, usize::MAX)?;
    let mut photos: Vec<photosite_core::domain::PhotoId> = Vec::new();
    for face in faces {
        if !photos.contains(&face.photo) {
            photos.push(face.photo);
        }
    }

    Ok(photos)
}

fn with_catalog<T>(
    app: &mut App,
    work: impl FnOnce(&mut Catalog) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    match app.catalog.as_mut() {
        Some(catalog) => work(catalog),
        None => anyhow::bail!("there is no catalogue to write to"),
    }
}

/// Looks for the models, so the window can say what is missing and offer
/// it. What is on disk is the truth, whatever any task said: somebody may
/// have put them there by hand, or a download may have stopped halfway
/// with two of the four in place.
fn check(app: &mut App) {
    let folder = app.model_folder();
    let availability = Availability::of(&folder);
    app.people.missing = fetch::missing(&folder);
    app.people.unavailable = (!availability.recognition).then(|| {
        t!(
            "people-no-models",
            folder = folder.to_string_lossy().into_owned(),
            missing = availability.missing.join(", ")
        )
    });
    app.people.checked = true;
}

/// Starts fetching whichever models are not on disk.
fn start_fetch(app: &mut App) {
    let folder = app.model_folder();
    let models = fetch::missing(&folder);
    if models.is_empty() {
        app.people.checked = false;
        return;
    }

    app.people.trouble = None;
    let waker = app.waker.clone();
    app.people.fetching = Some(app.tasks.spawn(t!("task-models"), move |cancel, progress| {
        fetch::fetch(&folder, &models, cancel, progress, |model| {
            t!("people-fetching", name = model.name)
        })?;
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }

        Ok(())
    }));
}

/// Notices that a download has finished, and looks at the folder again
/// rather than at the result: a download stopped halfway left the finished
/// ones in place, and the offer shrinks to what is still missing.
pub fn collect_fetch(app: &mut App) {
    let Some(id) = app.people.fetching else {
        return;
    };

    let Some(task) = app
        .tasks
        .snapshot()
        .into_iter()
        .find(|task| task.id == id && task.finished)
    else {
        return;
    };

    app.people.fetching = None;
    app.people.checked = false;
    app.people.trouble = task.error.clone();
    if task.error.is_none() && !task.cancelled {
        app.people.summary = t!("people-fetched");
    }

    app.tasks.forget_finished();
}

/// Starts the sweep over the folder in front of us.
fn start(app: &mut App) {
    let Some(folder) = app.folder.clone() else {
        app.status = t!("people-no-folder");
        return;
    };

    check(app);
    if app.people.unavailable.is_some() {
        return;
    }

    let models = app.model_folder();
    let path = app.paths.catalog();
    let recursive = app.settings.gallery.recursive;
    let detect = app.settings.faces.detect_size.clamp(320, 8192) as u32;
    let threads = app.worker_threads();
    let waker = app.waker.clone();
    // The selection when there is one, the folder when there is not — the
    // rule the Describe window follows, so the two windows mean the same
    // thing by "these photographs".
    let scope = if app.selection.is_empty() {
        Scope::Folder {
            folder: folder.clone(),
            recursive,
        }
    } else {
        Scope::Photos(
            app.selection
                .iter()
                .filter_map(|at| app.photo(*at).cloned())
                .collect(),
        )
    };
    let again = app.people.rescan;

    app.people.scanning = Some(app.tasks.spawn(t!("task-faces"), move |cancel, progress| {
        let mut catalog = Catalog::open(&path)?;
        let engine = Arc::new(photosite_faces::Engine::load(&models)?);
        let now = crate::now();
        let mut report = photosite_faces::sweep::sweep(
            &mut catalog,
            &engine,
            &scope,
            again,
            detect,
            threads,
            now,
            cancel,
            progress,
        )?;

        // Faces found before the expression models were on disk are scored
        // here, in place, without their names being disturbed.
        if engine.scores_expressions() {
            report.scored = photosite_faces::sweep::score(
                &mut catalog,
                &engine,
                &folder,
                recursive,
                detect,
                threads,
                &t!("people-scoring"),
                cancel,
                progress,
            )?;
        }

        // The last thing it says is what it found, which is what the window
        // shows once the task is done.
        progress.report(
            report.photographs,
            Some(report.photographs),
            summarise(&report),
        );
        if let Some(ctx) = waker.get() {
            ctx.request_repaint();
        }

        Ok(())
    }));
}

fn summarise(report: &photosite_faces::sweep::Report) -> String {
    let mut parts = vec![t!(
        "people-swept",
        photos = report.photographs as i64,
        faces = report.faces as i64
    )];
    if report.assigned > 0 {
        parts.push(t!("people-recognised", count = report.assigned as i64));
    }

    if report.suggested > 0 {
        parts.push(t!("people-to-confirm", count = report.suggested as i64));
    }

    if report.scored > 0 {
        parts.push(t!("people-scored", count = report.scored as i64));
    }

    if report.failed > 0 {
        parts.push(t!("people-unreadable", count = report.failed as i64));
    }

    parts.join(" \u{b7} ")
}

#[cfg(test)]
mod tests {
    use photosite_core::people::crop;

    /// The chip is drawn by handing the painter a fraction of a texture, so
    /// the crop has to come back as fractions that stay inside it.
    #[test]
    fn a_face_chip_stays_inside_its_texture() {
        for face in [
            (0.0, 0.0, 0.05, 0.08),
            (0.95, 0.9, 0.05, 0.08),
            (0.4, 0.4, 0.3, 0.4),
        ] {
            let (x, y, width, height) = crop(face, 0.35, (640, 427));
            assert!((0.0..=1.0).contains(&x), "{x}");
            assert!((0.0..=1.0).contains(&y), "{y}");
            assert!(x + width <= 1.0 + 1e-9);
            assert!(y + height <= 1.0 + 1e-9);
        }
    }
}
