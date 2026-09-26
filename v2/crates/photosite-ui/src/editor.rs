//! One photograph on its own, in a tab of its own.
//!
//! The window carries a strip of tabs: the manager first, which cannot be
//! closed, and after it one tab per photograph that was opened. A tab holds
//! the photograph and how it is being looked at; nothing in it is written
//! anywhere yet, which is why closing one asks nothing. When the editor can
//! change a photograph, [`Editor::can_close`] is the one place that has to
//! learn to say no.
//!
//! The keys are commands from the registry, not keys read here — so that
//! `Escape`, `Enter` and the rest can be rebound with everything else. What
//! is read here is what has no name to bind: the wheel, a drag, a middle
//! click.
//!
//! The arithmetic of the view is [`photosite_core::compare`]'s: a fitted
//! photograph, a zoom about the pointer, a drag that stays under the hand.
//! One set of sums for both places a photograph is looked at closely.

use crate::{App, Want, compare as looking, theme};
use eframe::egui;
use egui::{Sense, Vec2};
use photosite_core::compare;
use photosite_core::t;
use photosite_core::theme::Palette;
use std::path::{Path, PathBuf};

/// How far the wheel has to turn before the page turns. A notch of a mouse
/// wheel is more than this; a touchpad gets there in a few movements rather
/// than turning a page per pixel.
const NOTCH: f32 = 30.0;

/// A photograph open in a tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    pub path: PathBuf,
    /// How closely it is being looked at, and at which part.
    pub view: compare::View,
    /// Wheel movement not yet turned into a page. See [`NOTCH`].
    wheel: f32,
    /// The space the photograph was last drawn in, and the photograph's own
    /// size, in points. Kept from the last frame so that a key can ask for
    /// "one pixel per point" without a hand on the canvas: the zoom is a
    /// factor over the fitted size, and the fitted size depends on both.
    pub cell: (f32, f32),
    pub image: (f32, f32),
}

impl Editor {
    pub fn open(path: PathBuf) -> Self {
        Self {
            path,
            view: compare::View::FITTED,
            wheel: 0.0,
            cell: (1.0, 1.0),
            image: (1.0, 1.0),
        }
    }

    /// Whether the tab may close without asking. Always, for now: the editor
    /// holds nothing that is not in the file. When it does, this is where
    /// the question goes — and nowhere else, so that the cross on the tab,
    /// the key and paging away all ask it the same way.
    pub fn can_close(&self) -> bool {
        true
    }

    /// Moves the tab on to another photograph. The view starts over: a
    /// corner of one frame is nowhere in particular in the next.
    pub fn show(&mut self, path: PathBuf) {
        self.path = path;
        self.view = compare::View::FITTED;
    }
}

/// The strip of tabs: the manager, then the open photographs.
///
/// Tabs are numbered with the manager as `0`, so that "which tab" is one
/// number wherever it is asked; the editors are the numbers after it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tabs {
    editors: Vec<Editor>,
    active: usize,
    /// Whether the editor is filling the screen. The editor's own, apart
    /// from the window's: entered for a photograph and left with it.
    pub fullscreen: bool,
}

impl Tabs {
    pub const MANAGER: usize = 0;

    pub fn editors(&self) -> &[Editor] {
        &self.editors
    }

    /// Which tab is in front, the manager being [`Tabs::MANAGER`].
    pub fn active(&self) -> usize {
        self.active
    }

    pub fn manager_is_active(&self) -> bool {
        self.active == Self::MANAGER
    }

    pub fn active_editor(&self) -> Option<&Editor> {
        self.editors.get(self.active.checked_sub(1)?)
    }

    pub fn active_editor_mut(&mut self) -> Option<&mut Editor> {
        self.editors.get_mut(self.active.checked_sub(1)?)
    }

    /// Brings a tab to the front. A number past the end is the last tab,
    /// not a panic: the strip may have shrunk since the number was read.
    pub fn activate(&mut self, tab: usize) {
        self.active = tab.min(self.editors.len());
    }

    /// Opens a photograph, or brings its tab to the front if it already has
    /// one. Two tabs of one file would be two views of one truth, and once
    /// the editor can change a photograph, two places to change it.
    pub fn open(&mut self, path: PathBuf) -> usize {
        let at = match self.editors.iter().position(|editor| editor.path == path) {
            Some(at) => at,
            None => {
                self.editors.push(Editor::open(path));
                self.editors.len() - 1
            }
        };
        self.active = at + 1;
        self.active
    }

    /// Closes a tab and says which photograph it held. The manager cannot be
    /// closed, and asking is answered with `None`.
    ///
    /// The tab in front afterwards is the one that was in front, if it is
    /// still there; otherwise the one to the left, which is the manager when
    /// the last editor goes. What was being looked at is not lost to a cross
    /// clicked on a tab beside it.
    pub fn close(&mut self, tab: usize) -> Option<Editor> {
        let at = tab.checked_sub(1)?;
        if at >= self.editors.len() {
            return None;
        }

        let closed = self.editors.remove(at);
        if self.active > tab || self.active > self.editors.len() {
            self.active -= 1;
        }

        if self.manager_is_active() {
            self.fullscreen = false;
        }

        Some(closed)
    }

    /// How many tabs there are, the manager counted.
    pub fn count(&self) -> usize {
        self.editors.len() + 1
    }
}

/// The strip along the top. Every tab is a button; every editor's tab also
/// has its cross.
pub fn strip(app: &mut App, ui: &mut egui::Ui, palette: &Palette, ctx: &egui::Context) {
    let mut chosen: Option<usize> = None;
    let mut closed: Option<usize> = None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let active = app.tabs.active();
        let names: Vec<(usize, String)> = std::iter::once((Tabs::MANAGER, t!("tab-manager")))
            .chain(app.tabs.editors().iter().enumerate().map(|(at, editor)| {
                let name = editor
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| editor.path.to_string_lossy().into_owned());
                (at + 1, name)
            }))
            .collect();

        for (tab, name) in names {
            let is_active = tab == active;
            let text = egui::RichText::new(name).color(theme::color(if is_active {
                palette.accent
            } else {
                palette.text
            }));
            let response = ui.add(egui::Button::new(text).frame(false));
            if is_active {
                let rect = response.rect;
                ui.painter().hline(
                    rect.x_range(),
                    rect.max.y - 1.0,
                    egui::Stroke::new(2.0, theme::color(palette.accent)),
                );
            }

            if response.clicked() {
                chosen = Some(tab);
            }

            // The manager has no cross: it is where the rest is opened
            // from, and a window with no way back in is a trap.
            if tab != Tabs::MANAGER && cross(ui, palette).clicked() {
                closed = Some(tab);
            }

            ui.add_space(6.0);
        }
    });

    if let Some(tab) = closed {
        app.close_tab(tab, ctx);
    } else if let Some(tab) = chosen {
        app.activate_tab(tab, ctx);
    }
}

/// A small cross to close a tab, drawn rather than written: egui's default
/// font has no ✕ and it would come out as an empty box.
fn cross(ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    let tint = theme::color(if response.hovered() {
        palette.text
    } else {
        palette.dim
    });
    let centre = rect.center();
    let arm = 3.5;
    let stroke = egui::Stroke::new(1.5, tint);
    ui.painter().line_segment(
        [
            egui::pos2(centre.x - arm, centre.y - arm),
            egui::pos2(centre.x + arm, centre.y + arm),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(centre.x - arm, centre.y + arm),
            egui::pos2(centre.x + arm, centre.y - arm),
        ],
        stroke,
    );
    response
}

/// The photograph, as large as the window allows, and the row beneath it.
pub fn show(app: &mut App, ui: &mut egui::Ui, palette: &Palette, ctx: &egui::Context) {
    let Some(editor) = app.tabs.active_editor().cloned() else {
        return;
    };

    let full = ui.available_rect_before_wrap();
    let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0;
    let area = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.max.y - row));
    ui.painter()
        .rect_filled(area, 0, theme::color(palette.well));

    let image = looking::shown(app, &editor.path);
    let cell = (area.width(), area.height());
    let frame = compare::frame(cell, image, editor.view);
    let response = ui.interact(area, ui.id().with("editor"), Sense::click_and_drag());

    // Whatever is already decoded is drawn at once and replaced as
    // something better arrives, so the tab is never empty while the disk
    // is being read.
    let chosen = [Want::Close, Want::Preview, Want::Thumb, Want::Quick]
        .into_iter()
        .map(|want| (editor.path.clone(), want))
        .find(|key| app.texture(key).is_some());
    match chosen {
        Some(key) => {
            if let Some(texture) = app.texture(&key) {
                ui.painter().with_clip_rect(area).image(
                    texture.id(),
                    egui::Rect::from_min_size(
                        area.min + Vec2::new(frame.target[0], frame.target[1]),
                        Vec2::new(frame.target[2], frame.target[3]),
                    ),
                    egui::Rect::from_min_size(
                        egui::pos2(frame.source[0], frame.source[1]),
                        Vec2::new(frame.source[2], frame.source[3]),
                    ),
                    egui::Color32::WHITE,
                );
            }

            app.touch(&key);
        }
        None => {
            ui.painter().text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                t!("compare-unreadable"),
                egui::FontId::proportional(12.0),
                theme::color(palette.dim),
            );
        }
    }

    // The closest decode for this one, and the ordinary preview for the
    // two beside it — so that turning the page shows something at once and
    // the sharp version follows.
    let mut wanted_close = Vec::new();
    if !app.has(&editor.path, Want::Close) {
        wanted_close.push(editor.path.clone());
    }

    let mut wanted_preview = Vec::new();
    for by in [1, -1] {
        if let Some(neighbour) = app.neighbour_of(&editor.path, by)
            && !app.has(&neighbour, Want::Preview)
        {
            wanted_preview.push(neighbour);
        }
    }

    app.wanted_close = wanted_close;
    app.wanted_preview = wanted_preview;

    // The wheel with Ctrl looks closer, the plain wheel turns the page, and
    // the drag moves what is looked at. All three are read from the same
    // response, and the page only turns when the pointer is over the
    // photograph — a wheel over the row beneath it means nothing.
    let (control, wheel, pointer) = ui.input(|input| {
        (
            input.modifiers.command || input.modifiers.ctrl,
            input.smooth_scroll_delta.y,
            input.pointer.latest_pos(),
        )
    });
    let mut view = editor.view;
    let mut turned: Option<isize> = None;
    let mut accumulated = editor.wheel;
    let dragged = response.drag_delta();
    if dragged != Vec2::ZERO {
        view = looking::pulled(view, &frame, dragged);
    } else if response.hovered() && wheel != 0.0 {
        if control {
            if let Some(pointer) = pointer {
                view = looking::wheeled(view, &frame, pointer - area.min, wheel);
            }
        } else {
            // Turning back the other way starts over: half a notch down and
            // half a notch up is nothing, not a page.
            if accumulated.signum() != wheel.signum() {
                accumulated = 0.0;
            }

            accumulated += wheel;
            if accumulated.abs() >= NOTCH {
                // A wheel turned towards oneself is the next page, the way
                // it scrolls a list down.
                turned = Some(if accumulated < 0.0 { 1 } else { -1 });
                accumulated = 0.0;
            }
        }
    }

    if let Some(current) = app.tabs.active_editor_mut() {
        current.view = compare::settled(view, cell, image);
        current.wheel = accumulated;
        current.cell = cell;
        current.image = image;
    }

    // The middle button fills the screen and gives it back.
    if response.clicked_by(egui::PointerButton::Middle) {
        app.run("editor.fullscreen", ctx);
    }

    if let Some(by) = turned {
        app.page_editor(by);
    }

    beneath(
        app,
        ui,
        egui::Rect::from_min_max(egui::pos2(full.min.x, area.max.y), full.max),
        palette,
        &editor.path,
        cell,
        image,
    );
}

/// The row beneath the photograph: its name, where it stands in the folder,
/// how closely it is being looked at, and what the keys do.
fn beneath(
    app: &mut App,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &Palette,
    path: &Path,
    cell: (f32, f32),
    image: (f32, f32),
) {
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("editor-strip")
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    bar.add_space(6.0);
    bar.label(egui::RichText::new(name).color(theme::color(palette.text)));

    // Where it stands, in the order the manager shows the folder. Nothing
    // when the folder has moved on without it.
    let standing = app.position_of(path).map(|at| (at + 1, app.count()));
    let said = match standing {
        Some((at, count)) => t!("editor-position", at = at as i64, count = count as i64),
        None => t!("editor-gone"),
    };
    bar.label(egui::RichText::new(said).color(theme::color(palette.dim)));

    let zoom = app
        .tabs
        .active_editor()
        .map(|editor| editor.view.zoom)
        .unwrap_or(1.0);
    let percent = compare::magnification(cell, image, zoom) * 100.0;
    bar.label(
        egui::RichText::new(t!(
            "compare-magnification",
            percent = percent.round() as i64
        ))
        .color(theme::color(palette.dim)),
    );

    bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(t!("editor-hint")).color(theme::color(palette.dim)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(name: &str) -> PathBuf {
        PathBuf::from(format!("C:/photos/{name}.jpg"))
    }

    #[test]
    fn the_manager_is_there_from_the_start_and_cannot_be_closed() {
        let mut tabs = Tabs::default();
        assert!(tabs.manager_is_active());
        assert_eq!(tabs.count(), 1);
        assert!(tabs.close(Tabs::MANAGER).is_none());
        assert_eq!(tabs.count(), 1);
    }

    #[test]
    fn opening_a_photograph_adds_a_tab_and_brings_it_to_the_front() {
        let mut tabs = Tabs::default();
        let tab = tabs.open(photo("a"));
        assert_eq!(tab, 1);
        assert_eq!(tabs.active(), 1);
        assert_eq!(
            tabs.active_editor().map(|e| e.path.clone()),
            Some(photo("a"))
        );
        assert!(!tabs.manager_is_active());
    }

    #[test]
    fn a_photograph_opened_twice_has_one_tab() {
        let mut tabs = Tabs::default();
        tabs.open(photo("a"));
        tabs.open(photo("b"));
        tabs.activate(Tabs::MANAGER);
        assert_eq!(tabs.open(photo("a")), 1);
        assert_eq!(tabs.count(), 3);
        assert_eq!(tabs.active(), 1);
    }

    #[test]
    fn closing_the_last_editor_lands_on_the_manager() {
        let mut tabs = Tabs::default();
        tabs.open(photo("a"));
        let closed = tabs.close(1).unwrap();
        assert_eq!(closed.path, photo("a"));
        assert!(tabs.manager_is_active());
    }

    #[test]
    fn closing_the_front_tab_moves_to_the_one_on_its_left() {
        let mut tabs = Tabs::default();
        tabs.open(photo("a"));
        tabs.open(photo("b"));
        tabs.open(photo("c"));
        tabs.close(3);
        assert_eq!(
            tabs.active_editor().map(|e| e.path.clone()),
            Some(photo("b"))
        );

        // In the middle: the one that slid into its place.
        tabs.open(photo("c"));
        tabs.activate(2);
        tabs.close(2);
        assert_eq!(
            tabs.active_editor().map(|e| e.path.clone()),
            Some(photo("c"))
        );
    }

    #[test]
    fn closing_a_tab_beside_the_front_one_keeps_what_was_being_looked_at() {
        let mut tabs = Tabs::default();
        tabs.open(photo("a"));
        tabs.open(photo("b"));
        tabs.open(photo("c"));
        tabs.close(1);
        assert_eq!(
            tabs.active_editor().map(|e| e.path.clone()),
            Some(photo("c"))
        );
        assert_eq!(tabs.active(), 2);

        tabs.activate(1);
        tabs.close(2);
        assert_eq!(
            tabs.active_editor().map(|e| e.path.clone()),
            Some(photo("b"))
        );
    }

    #[test]
    fn a_tab_number_past_the_end_is_the_last_tab() {
        let mut tabs = Tabs::default();
        tabs.open(photo("a"));
        tabs.activate(7);
        assert_eq!(tabs.active(), 1);
    }

    #[test]
    fn leaving_the_editor_leaves_its_fullscreen() {
        let mut tabs = Tabs::default();
        tabs.open(photo("a"));
        tabs.fullscreen = true;
        tabs.close(1);
        assert!(!tabs.fullscreen);
    }

    #[test]
    fn paging_starts_the_view_over() {
        let mut editor = Editor::open(photo("a"));
        editor.view = compare::View {
            centre: (0.2, 0.2),
            zoom: 4.0,
        };
        editor.show(photo("b"));
        assert_eq!(editor.path, photo("b"));
        assert_eq!(editor.view, compare::View::FITTED);
    }
}
