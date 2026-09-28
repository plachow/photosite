//! Three panes: the folder tree, the grid of slides, the full preview.
//!
//! The grid is virtualised by hand — only visible rows are drawn, so the
//! number of photographs does not matter. Nothing in this loop grows with the
//! size of the library.
//!
//! It invents no dimensions: the gap, the aspect ratio, the caption height
//! and how many rows are loaded ahead all live in the settings.

use crate::{App, Node, Want, theme};
use eframe::egui;
use egui::{Sense, Vec2};
use photosite_core::settings::same_folder;
use photosite_core::t;
use photosite_core::theme::Palette;
use std::path::{Path, PathBuf};

pub fn gallery(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    if app.count() == 0 {
        // "There is nothing here" and "the filter is hiding it all" are
        // different problems with different answers, and telling somebody
        // the wrong one sends them looking in the wrong place.
        let message = if app.total() > 0 {
            t!("filter-nothing-matches")
        } else {
            t!("gallery-empty")
        };
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(message).color(theme::color(palette.dim)));
        });
        return;
    }

    // Two views of one folder, and a switch rather than two panels: side by
    // side they would each be too narrow to be worth having.
    if app.gallery().as_list {
        return crate::list::show(app, ui, palette);
    }

    let gallery = app.gallery().clone();
    let tile_w = gallery.tile_size as f32;
    let tile_h = theme::tile_height(&gallery);
    let gap = gallery.gap as f32;
    let margin = gallery.prefetch_rows.clamp(0, 64) as usize;
    let count = app.count();

    // Ctrl and the wheel resizes the tiles, the way it does in every file
    // manager and every browser. The toolkit never shows that wheel as a
    // scroll: with Ctrl held it becomes a zoom factor, so the scroll area
    // cannot see it and nothing has to be taken away from it. Reading the
    // scroll delta here instead is why the tiles once did not resize at all.
    let zooming = ui.input(|input| input.zoom_delta());
    if zooming != 1.0 && ui.rect_contains_pointer(ui.max_rect()) {
        app.resize_tiles(f64::from(zooming));
    }

    speed_wheel(ui, gallery.wheel_speed);
    theme::solid_scrollbar(ui);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let cols = (((width - gap) / (tile_w + gap)).floor() as usize).max(1);
            let rows = count.div_ceil(cols);
            let pitch = tile_h + gap;
            let (area, _) =
                ui.allocate_exact_size(Vec2::new(width, rows as f32 * pitch + gap), Sense::hover());

            let first = ((viewport.min.y - gap) / pitch).floor().max(0.0) as usize;
            let last = ((viewport.max.y / pitch).ceil() as usize).min(rows);

            // What the keys need to know about the layout: how many tiles
            // make a row, and how many rows make a page.
            app.grid_cols = cols;
            app.grid_page_rows = ((viewport.height() / pitch).floor() as usize).max(1);

            // Coming back from the editor: the tile it was opened from is
            // brought into view, wherever the grid was left. Worked out
            // from the position and not from a drawn tile, because the one
            // wanted is as likely as not outside the rows being drawn.
            if let Some(index) = app.scroll_grid_to.take() {
                let rect = egui::Rect::from_min_size(
                    area.min
                        + Vec2::new(
                            gap + (index % cols) as f32 * (tile_w + gap),
                            gap + (index / cols) as f32 * pitch,
                        ),
                    Vec2::new(tile_w, tile_h),
                );
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
            }

            let mut wanted_quick: Vec<(usize, PathBuf)> = Vec::new();
            let mut wanted_sharp: Vec<(usize, PathBuf)> = Vec::new();
            let mut clicked: Option<(usize, egui::Modifiers)> = None;
            let mut opened: Option<usize> = None;
            let mut menu_on: Option<(usize, egui::Response)> = None;

            for row in first..last {
                for col in 0..cols {
                    let index = row * cols + col;
                    if index >= count {
                        break;
                    }

                    let rect = egui::Rect::from_min_size(
                        area.min
                            + Vec2::new(
                                gap + col as f32 * (tile_w + gap),
                                gap + row as f32 * pitch,
                            ),
                        Vec2::new(tile_w, tile_h),
                    );
                    let Some(path) = app.photo(index).map(|photo| photo.path.clone()) else {
                        continue;
                    };
                    let response = ui.interact(rect, ui.id().with(index), Sense::click());
                    if response.clicked() {
                        clicked = Some((index, ui.input(|input| input.modifiers)));
                    }

                    if response.double_clicked() {
                        opened = Some(index);
                    }

                    if crate::menu::wanted(&response) {
                        menu_on = Some((index, response.clone()));
                    }

                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let well = theme::slide(
                        ui.painter(),
                        rect,
                        palette,
                        &gallery,
                        &name,
                        app.is_selected(index),
                        response.hovered(),
                    );

                    // The sharp version wins; until there is one, the EXIF
                    // thumbnail is drawn. A blank tile is only the third
                    // choice.
                    let sharp = app.has(&path, Want::Thumb);
                    if !sharp {
                        wanted_sharp.push((index, path.clone()));
                    }

                    let key = (path.clone(), if sharp { Want::Thumb } else { Want::Quick });
                    match app.texture(&key) {
                        Some(texture) => {
                            let size = texture.size();
                            ui.painter().image(
                                texture.id(),
                                theme::fit(well, size),
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        None => wanted_quick.push((index, path.clone())),
                    }

                    if let Some(photo) = app.photo(index) {
                        let organisation = photo.organisation.clone();
                        // The pin is a setting, and off by default: a phone
                        // that geotags everything grades most of its
                        // library as approximate, and a pin on most tiles
                        // says nothing.
                        let verdict = if gallery.show_position_badge {
                            photo.verdict
                        } else {
                            photosite_core::Verdict::Nowhere
                        };
                        let people = photo.people.clone();
                        let expressions = photo.expressions;
                        theme::badges(
                            ui.painter(),
                            well,
                            palette,
                            &organisation,
                            verdict,
                            &people,
                            expressions,
                        );
                    }
                }
            }

            // From the middle of the viewport outwards: the middle is where
            // somebody is looking.
            let middle = (first + last) as f32 * 0.5 * cols as f32;
            let order = |mut list: Vec<(usize, PathBuf)>| {
                list.sort_by_key(|(index, _)| (*index as f32 - middle).abs() as i64);
                list.into_iter().map(|(_, path)| path).collect::<Vec<_>>()
            };
            app.blank = wanted_quick.len();
            app.unsharp = wanted_sharp.len();
            app.wanted_quick = order(wanted_quick);
            app.wanted_sharp = order(wanted_sharp);

            // Rows above and below the viewport: prepared ahead, but not
            // counted as blank — nobody is looking at those.
            let ahead_from = first.saturating_sub(margin) * cols;
            let ahead_to = ((last + margin) * cols).min(count);
            for index in ahead_from..ahead_to {
                let Some(path) = app.photo(index).map(|photo| photo.path.clone()) else {
                    continue;
                };
                if !app.has(&path, Want::Thumb) && !app.wanted_sharp.contains(&path) {
                    app.wanted_sharp.push(path);
                }
            }

            // How many textures the grid wishes to keep. The cache ceiling
            // is raised by this, so that what will be wanted again in a
            // moment is not thrown away.
            app.needed = (ahead_to - ahead_from) * 2;

            // Touch what was used only after drawing, so the LRU does not
            // evict exactly what is needed. Prefetched rows count too —
            // otherwise they are the first to go and are ordered again at
            // once.
            for index in ahead_from..ahead_to {
                let Some(path) = app.photo(index).map(|photo| photo.path.clone()) else {
                    continue;
                };
                app.touch(&(path.clone(), Want::Thumb));
                app.touch(&(path, Want::Quick));
            }

            if let Some((index, modifiers)) = clicked {
                app.click_tile(index, modifiers.command || modifiers.ctrl, modifiers.shift);
            }

            // A double-click opens the tile in a tab of its own. It was
            // chosen by the first click of the two, so the tab and the
            // selection agree.
            if let Some(index) = opened {
                app.edit(index);
            }

            // The menu last, once the tiles are drawn: what it does can
            // reorder the folder under the loop that drew them.
            if let Some((index, response)) = menu_on {
                crate::menu::attach(app, index, &response);
            }
        });
}

pub fn preview(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    app.wanted_preview.clear();
    let Some(index) = app.selected else {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(t!("preview-pick-tile")).color(theme::color(palette.dim)));
        });
        return;
    };

    let Some(path) = app.photo(index).map(|photo| photo.path.clone()) else {
        return;
    };
    let area = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(area, 0, theme::color(palette.well));

    // Until the full resolution is decoded, whatever is already there is
    // shown — so the pane never flashes empty.
    let chosen = [Want::Preview, Want::Thumb, Want::Quick]
        .into_iter()
        .map(|want| (path.clone(), want))
        .find(|key| app.texture(key).is_some());
    if !app.has(&path, Want::Preview) {
        app.wanted_preview.push(path.clone());
    }

    if let Some(key) = chosen {
        let mut drawn = None;
        if let Some(texture) = app.texture(&key) {
            let size = texture.size();
            let into = theme::fit(area.shrink(12.0), size);
            ui.painter().image(
                texture.id(),
                into,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            drawn = Some(into);
        }

        app.touch(&key);

        // The faces, over the photograph. Only where somebody is named:
        // a frame round every face in a group shot is a photograph nobody
        // can see any more, and the frames exist to say who, not to say
        // that a detector ran.
        if let Some(into) = drawn
            && app.settings.faces.show_frames
        {
            frames(app, ui, index, into);
        }
    }

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.painter().text(
        egui::pos2(area.center().x, area.max.y - 14.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(12.0),
        theme::color(palette.dim),
    );
}

/// Frames the named faces over the preview, each in that person's colour.
///
/// The rectangles are fractions of the frame, so they land in the right
/// place whatever size the preview happens to be drawn at — which is the
/// whole reason they are stored that way.
fn frames(app: &mut App, ui: &mut egui::Ui, index: usize, into: egui::Rect) {
    let Some(photo) = app.photo(index) else {
        return;
    };
    if photo.people.is_empty() {
        return;
    }

    // Read once per photograph, not once per frame. A query to draw a
    // rectangle, sixty times a second, is the shape of mistake this
    // application has a rule about.
    app.load_faces(photo.id);
    for face in app.preview_faces.clone() {
        let Some(person) = face.person else {
            continue;
        };
        let colour = photosite_core::theme::person_color(person);
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                into.min.x + (face.x as f32) * into.width(),
                into.min.y + (face.y as f32) * into.height(),
            ),
            egui::vec2(
                (face.width as f32) * into.width(),
                (face.height as f32) * into.height(),
            ),
        );
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(colour.r, colour.g, colour.b)),
            egui::StrokeKind::Middle,
        );
    }
}

/// The height of a row of the tree, and of a favourite above it.
const TREE_ROW: f32 = 18.0;

pub fn tree(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    // Explorer's spacing: a step of a box per level, and no guide lines.
    ui.spacing_mut().indent = 16.0;
    ui.visuals_mut().indent_has_left_vline = false;
    theme::solid_scrollbar(ui);
    favourites(app, ui, palette);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut pick = None;
            let mut favourite = None;
            let current = app.folder.clone();
            // It has to be taken before drawing: if the scroll request were
            // cleared afterwards, it would clear the very one this tree just
            // created by being clicked.
            let scroll_to = app.scroll_tree_to.take();
            for (folder, has) in app.probing.drain(256) {
                app.subfolders.insert(folder, has);
            }

            let mut roots = std::mem::take(&mut app.roots);
            let mut unknown: Vec<PathBuf> = Vec::new();
            for root in &mut roots {
                node(
                    ui,
                    root,
                    palette,
                    Place {
                        current: current.as_deref(),
                        scroll_to: scroll_to.as_deref(),
                        known: &app.subfolders,
                        gallery: &app.settings.gallery,
                    },
                    &mut pick,
                    &mut favourite,
                    &mut unknown,
                );
            }

            app.roots = roots;
            app.probing.wish(vec![unknown]);
            if let Some(folder) = favourite {
                app.toggle_favourite(&folder);
            }
            if let Some(folder) = pick {
                app.open(folder);
            }
        });
}

/// The favourites, above the tree and apart from it: a row each, the name
/// in bold, and a ✕ to take it off the list. Nothing at all when there are
/// none, not even the line — a heading over nothing is clutter.
///
/// Outside the tree's scroll area on purpose. The tree scrolls itself to
/// whatever folder is opened, and a favourite would slide away from under
/// the pointer that had just clicked it.
fn favourites(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    if app.settings.gallery.favourites.is_empty() {
        return;
    }

    let favourites = app.settings.gallery.favourites.clone();
    let current = app.folder.clone();
    let mut open = None;
    let mut forget = None;
    // A long list scrolls in its own share of the pane rather than pushing
    // the tree out of it.
    egui::ScrollArea::vertical()
        .id_salt("favourites")
        .max_height(ui.available_height() * 0.4)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for folder in &favourites {
                let path = Path::new(folder);
                let lit = current
                    .as_deref()
                    .is_some_and(|current| same_folder(folder, &current.display().to_string()));
                if let Some(asked) = favourite(ui, path, folder, lit, palette) {
                    match asked {
                        Asked::Open => open = Some(path.to_path_buf()),
                        Asked::Forget => forget = Some(folder.clone()),
                    }
                }
            }
        });

    // Decently apart: a hairline with air on both sides, the tree's own
    // colours and nothing louder.
    ui.add_space(2.0);
    ui.separator();
    ui.add_space(2.0);

    if let Some(folder) = forget {
        app.forget_favourite(&folder);
    }
    // Through the same door as the history: a favourite on a disk that is
    // not plugged in says so, and stays on the list for when it is back.
    if let Some(folder) = open {
        app.go(Some(folder));
    }
}

/// What a favourite's row was asked to do.
enum Asked {
    Open,
    Forget,
}

/// One favourite: the folder, its name in bold, the whole row lit when it
/// is the folder open, and the ✕ at the far end, where the crosses stand in
/// a column however long the names.
fn favourite(
    ui: &mut egui::Ui,
    path: &Path,
    folder: &str,
    lit: bool,
    palette: &Palette,
) -> Option<Asked> {
    const CROSS: f32 = 14.0;
    let mut asked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let whole =
            egui::Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), TREE_ROW));
        let (row, response) = ui.allocate_exact_size(
            Vec2::new((whole.width() - CROSS - 2.0).max(TREE_ROW), TREE_ROW),
            Sense::click(),
        );
        // Lit from the folder on, as the tree lights its rows, and across
        // the ✕ too: it is one row. Hovered by the pointer anywhere in it,
        // or the light would go out on the way to the cross.
        let band = egui::Rect::from_min_max(egui::pos2(row.min.x + TREE_ROW, row.min.y), whole.max);
        if lit {
            ui.painter()
                .rect_filled(band, 2, theme::color(palette.accent).gamma_multiply(0.35));
        } else if ui.rect_contains_pointer(whole) {
            ui.painter().rect_filled(
                band,
                2,
                theme::color(palette.bevel_light).gamma_multiply(0.6),
            );
        }

        // Where the tree has its plus, a favourite has its star: the same
        // column, so the folders line up with the tree's below.
        theme::star(
            ui.painter(),
            egui::pos2(row.min.x + TREE_ROW * 0.5, row.center().y),
            5.0,
            theme::color(palette.dim),
        );
        theme::folder(
            ui.painter(),
            egui::Rect::from_min_size(
                egui::pos2(row.min.x + TREE_ROW + 1.0, row.center().y - 6.0),
                Vec2::new(16.0, 12.0),
            ),
            lit,
        );

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.to_owned());
        let left = row.min.x + TREE_ROW * 2.0 + 4.0;
        let mut job = egui::text::LayoutJob::simple_singleline(
            name,
            egui::FontId::proportional(13.0),
            theme::color(palette.text),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((row.max.x - left - 2.0).max(0.0));
        let galley = ui.painter().layout_job(job);
        theme::bold(
            ui.painter(),
            egui::pos2(left, row.center().y - galley.size().y * 0.5),
            galley,
            theme::color(palette.text),
        );

        let response = response.on_hover_text(folder);
        if response.clicked() {
            asked = Some(Asked::Open);
        }
        response.context_menu(|ui| {
            if ui.button(t!("tree-remove-favourite")).clicked() {
                asked = Some(Asked::Forget);
                ui.close();
            }
        });

        if crate::editor::cross(ui, palette)
            .on_hover_text(t!("tree-remove-favourite"))
            .clicked()
        {
            asked = Some(Asked::Forget);
        }
    });
    asked
}

/// What every folder of the tree is drawn against.
#[derive(Clone, Copy)]
struct Place<'a> {
    /// The folder open in the gallery.
    current: Option<&'a Path>,
    /// The folder to bring into view.
    scroll_to: Option<&'a Path>,
    /// Which folders have folders in them, as far as anybody has looked.
    known: &'a std::collections::HashMap<PathBuf, bool>,
    /// Which of them are favourites, for the menu on a folder.
    gallery: &'a photosite_core::settings::Gallery,
}

/// One folder of the tree, drawn the way Explorer draws one: a boxed plus
/// or minus, a yellow folder, the name, and the open folder's whole row
/// lit. Everything is painted rather than written — the default font has
/// no glyph for any of it, and a folder that comes out as an empty box is
/// not a folder.
///
/// A folder whose own folders are not known yet goes on `unknown`, to be
/// looked into alongside. One made a favourite, or taken off them, from its
/// menu goes on `favourite`.
fn node(
    ui: &mut egui::Ui,
    node: &mut Node,
    palette: &Palette,
    place: Place<'_>,
    pick: &mut Option<PathBuf>,
    favourite: &mut Option<PathBuf>,
    unknown: &mut Vec<PathBuf>,
) {
    const ROW: f32 = TREE_ROW;
    let is_current = place.current == Some(node.path.as_path());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;

        // The expander. Whether a folder has folders in it is found out on
        // a thread alongside, for the folders on screen, and until the
        // answer comes the box is shown: most folders do have folders in
        // them, and asking the disk while drawing is what makes a tree over
        // a network drive take a minute to open. A folder known to have none
        // gets the space and no box.
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ROW, ROW), Sense::click());
        let childless = !node.expandable(place.known);
        // Only what is on screen: a folder scrolled out of sight is not
        // worth a trip to the disk yet.
        if node.children.is_none()
            && !place.known.contains_key(&node.path)
            && ui.is_rect_visible(rect)
        {
            unknown.push(node.path.clone());
        }
        if !childless {
            let tint = theme::color(if response.hovered() {
                palette.text
            } else {
                palette.dim
            });
            let square = egui::Rect::from_center_size(rect.center(), Vec2::splat(9.0));
            ui.painter().rect(
                square,
                0,
                theme::color(palette.window),
                egui::Stroke::new(1.0, tint),
                egui::StrokeKind::Inside,
            );
            let centre = square.center();
            ui.painter().line_segment(
                [
                    egui::pos2(centre.x - 2.5, centre.y),
                    egui::pos2(centre.x + 2.5, centre.y),
                ],
                egui::Stroke::new(1.0, tint),
            );
            if !node.expanded {
                ui.painter().line_segment(
                    [
                        egui::pos2(centre.x, centre.y - 2.5),
                        egui::pos2(centre.x, centre.y + 2.5),
                    ],
                    egui::Stroke::new(1.0, tint),
                );
            }
        }

        if response.clicked() && !childless {
            node.expanded = !node.expanded;
            if node.expanded {
                node.load_children();
            }
        }

        // The folder and its name are one thing to click, and the open one
        // is lit across the row rather than merely coloured: a coloured
        // word among words is easy to lose, a lit row is not.
        let galley = ui.painter().layout_no_wrap(
            node.name.clone(),
            egui::FontId::proportional(13.0),
            theme::color(palette.text),
        );
        let width = ROW + 4.0 + galley.size().x + 6.0;
        let (row, response) = ui.allocate_exact_size(Vec2::new(width, ROW), Sense::click());
        if is_current {
            ui.painter()
                .rect_filled(row, 2, theme::color(palette.accent).gamma_multiply(0.35));
        } else if response.hovered() {
            ui.painter().rect_filled(
                row,
                2,
                theme::color(palette.bevel_light).gamma_multiply(0.6),
            );
        }

        theme::folder(
            ui.painter(),
            egui::Rect::from_min_size(
                egui::pos2(row.min.x + 1.0, row.center().y - 6.0),
                Vec2::new(16.0, 12.0),
            ),
            node.expanded,
        );
        ui.painter().galley(
            egui::pos2(
                row.min.x + ROW + 4.0,
                row.center().y - galley.size().y * 0.5,
            ),
            galley,
            theme::color(palette.text),
        );

        // An expanded tree is not enough on its own: the open folder may sit
        // far below the edge of the pane, and then it is no use.
        if place.scroll_to == Some(node.path.as_path()) {
            response.scroll_to_me(Some(egui::Align::Center));
        }

        if response.clicked() {
            node.load_children();
            node.expanded = true;
            *pick = Some(node.path.clone());
        }

        response.context_menu(|ui| {
            let title = if place.gallery.is_favourite(&node.path) {
                t!("tree-remove-favourite")
            } else {
                t!("tree-add-favourite")
            };
            if ui.button(title).clicked() {
                *favourite = Some(node.path.clone());
                ui.close();
            }
        });
    });

    if node.expanded
        && let Some(children) = node.children.as_mut()
    {
        ui.indent(node.path.as_path(), |ui| {
            for child in children {
                self::node(ui, child, palette, place, pick, favourite, unknown);
            }
        });
    }
}

/// Makes a notch of the wheel move the gallery by more than a line.
///
/// The toolkit scrolls a notch by what a text box wants, and a wall of
/// tiles is not a text box: at that pace a folder of two thousand
/// photographs takes a full minute of wheel to cross. The delta is
/// multiplied before the scroll area reads it, and only while the pointer
/// is over the gallery, so the tree and the details pane keep their own
/// pace. Ctrl and the wheel is taken by the tile zoom before this runs.
pub fn speed_wheel(ui: &mut egui::Ui, speed: f64) {
    let speed = speed.clamp(0.5, 10.0) as f32;
    if (speed - 1.0).abs() < f32::EPSILON || !ui.rect_contains_pointer(ui.max_rect()) {
        return;
    }

    ui.input_mut(|input| {
        input.smooth_scroll_delta.y *= speed;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Paths, Settings, Startup};

    /// What a frame of the tree pane drew: every piece of text with where it
    /// stands, and every filled rectangle.
    struct Drawn {
        texts: Vec<(String, egui::Rect)>,
        fills: Vec<(egui::Rect, egui::Color32)>,
    }

    impl Drawn {
        /// Where a piece of text was drawn, the first time it was.
        fn at(&self, text: &str) -> egui::Rect {
            self.texts
                .iter()
                .find(|(drawn, _)| drawn == text)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("{text} was not drawn: {:?}", self.names()))
        }

        /// Where it was drawn the last time: the tree is drawn after the
        /// favourites, so this is the tree's own row.
        fn last(&self, text: &str) -> egui::Rect {
            self.texts
                .iter()
                .rev()
                .find(|(drawn, _)| drawn == text)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("{text} was not drawn: {:?}", self.names()))
        }

        fn count(&self, text: &str) -> usize {
            self.texts.iter().filter(|(drawn, _)| drawn == text).count()
        }

        fn names(&self) -> Vec<&str> {
            self.texts.iter().map(|(text, _)| text.as_str()).collect()
        }

        /// Whether the row holding this text is lit as the folder open.
        fn lit(&self, text: &str, palette: &Palette) -> bool {
            let middle = self.at(text).center();
            let light = theme::color(palette.accent).gamma_multiply(0.35);
            self.fills
                .iter()
                .any(|(rect, fill)| *fill == light && rect.contains(middle))
        }
    }

    fn tree_frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> Drawn {
        fn walk(shape: &egui::Shape, into: &mut Drawn) {
            match shape {
                egui::Shape::Text(text) => into
                    .texts
                    .push((text.galley.text().to_owned(), text.visual_bounding_rect())),
                egui::Shape::Rect(rect) => into.fills.push((rect.rect, rect.fill)),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, into)),
                _ => {}
            }
        }

        let palette = *app.palette();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(300.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| tree(app, ui, &palette));
        // With no renderer nothing is ever uploaded, and epaint says so with
        // a panic when the output is dropped.
        out.textures_delta.clear();
        let mut drawn = Drawn {
            texts: Vec::new(),
            fills: Vec::new(),
        };
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut drawn);
        }
        drawn
    }

    fn button(at: egui::Pos2, which: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: which,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// A click the way a hand makes one: over it, down, up.
    fn click(app: &mut App, ctx: &egui::Context, at: egui::Pos2, which: egui::PointerButton) {
        tree_frame(app, ctx, vec![egui::Event::PointerMoved(at)]);
        tree_frame(app, ctx, vec![button(at, which, true)]);
        tree_frame(app, ctx, vec![button(at, which, false)]);
    }

    /// An application whose tree is one folder holding `keep` and `sort`,
    /// with `keep` a favourite.
    fn with_a_favourite() -> (App, tempfile::TempDir, tempfile::TempDir, tempfile::TempDir) {
        let (mut app, data, photos) = crate::culling::three();
        let disk = tempfile::tempdir().unwrap();
        for name in ["keep", "sort"] {
            std::fs::create_dir(disk.path().join(name)).unwrap();
        }
        let mut root = Node::new(disk.path().to_path_buf());
        root.load_children();
        root.expanded = true;
        app.roots = vec![root];
        app.settings.gallery.favourites = vec![disk.path().join("keep").display().to_string()];
        (app, data, photos, disk)
    }

    fn name_of(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn the_favourites_stand_above_the_tree_in_bold() {
        let (mut app, _data, _photos, disk) = with_a_favourite();
        let ctx = egui::Context::default();
        tree_frame(&mut app, &ctx, Vec::new());
        let drawn = tree_frame(&mut app, &ctx, Vec::new());

        assert!(
            drawn.at("keep").max.y <= drawn.at(&name_of(disk.path())).min.y,
            "the favourite is not above the tree: {:?}",
            drawn.names()
        );
        // Twice or more as a favourite, which is how bold is drawn, and once
        // in the tree, where it is an ordinary folder.
        assert!(drawn.count("keep") >= 3, "{:?}", drawn.names());
        assert_eq!(drawn.count("sort"), 1, "only the favourite is bold");
    }

    #[test]
    fn with_no_favourites_the_tree_starts_at_the_top() {
        let (mut app, _data, _photos, disk) = with_a_favourite();
        app.settings.gallery.favourites.clear();
        let ctx = egui::Context::default();
        tree_frame(&mut app, &ctx, Vec::new());
        let drawn = tree_frame(&mut app, &ctx, Vec::new());

        let root = drawn.at(&name_of(disk.path()));
        assert!(root.min.y < TREE_ROW, "{root:?}");
        assert_eq!(drawn.count("keep"), 1);
    }

    #[test]
    fn a_click_on_a_favourite_opens_it_and_its_cross_takes_it_off() {
        let (mut app, _data, _photos, disk) = with_a_favourite();
        let keep = disk.path().join("keep");
        let ctx = egui::Context::default();
        tree_frame(&mut app, &ctx, Vec::new());
        let at = tree_frame(&mut app, &ctx, Vec::new()).at("keep").center();

        click(&mut app, &ctx, at, egui::PointerButton::Primary);
        assert_eq!(app.folder.as_deref(), Some(keep.as_path()));
        let palette = *app.palette();
        let drawn = tree_frame(&mut app, &ctx, Vec::new());
        assert!(drawn.lit("keep", &palette), "the favourite open is not lit");
        // Still where it was clicked: the tree scrolled to the folder, the
        // favourites did not go with it.
        assert!((drawn.at("keep").center().y - at.y).abs() < 1.0);

        // The cross stands at the far end of the row.
        click(
            &mut app,
            &ctx,
            egui::pos2(300.0 - 8.0, at.y),
            egui::PointerButton::Primary,
        );
        assert!(app.settings.gallery.favourites.is_empty());
        assert!(keep.is_dir(), "the folder itself went with it");
        assert!(
            Settings::load(&app.paths).gallery.favourites.is_empty(),
            "taken off the list, but not in the settings on the disk"
        );
        assert_eq!(tree_frame(&mut app, &ctx, Vec::new()).count("keep"), 1);
    }

    #[test]
    fn a_folder_in_the_tree_is_made_a_favourite_from_its_menu() {
        let (mut app, _data, _photos, disk) = with_a_favourite();
        let ctx = egui::Context::default();
        tree_frame(&mut app, &ctx, Vec::new());
        let sort = tree_frame(&mut app, &ctx, Vec::new()).last("sort").center();

        click(&mut app, &ctx, sort, egui::PointerButton::Secondary);
        let add = tree_frame(&mut app, &ctx, Vec::new())
            .at("Add to favourites")
            .center();
        click(&mut app, &ctx, add, egui::PointerButton::Primary);

        let sort = disk.path().join("sort");
        assert!(app.settings.gallery.is_favourite(&sort));
        assert_eq!(
            app.settings.gallery.favourites.last(),
            Some(&sort.display().to_string()),
            "a new favourite goes to the end"
        );
        assert!(Settings::load(&app.paths).gallery.is_favourite(&sort));

        // And the same menu on a favourite says the opposite.
        let keep = tree_frame(&mut app, &ctx, Vec::new()).last("keep").center();
        click(&mut app, &ctx, keep, egui::PointerButton::Secondary);
        let menu = tree_frame(&mut app, &ctx, Vec::new());
        assert_eq!(
            menu.count("Remove from favourites"),
            1,
            "{:?}",
            menu.names()
        );
    }

    #[test]
    fn the_go_menu_switch_makes_the_open_folder_a_favourite_and_back() {
        let (mut app, _data, photos) = crate::culling::three();
        assert_eq!(app.checked("go.favourite"), Some(false));

        app.run_for_test("go.favourite");
        assert!(app.settings.gallery.is_favourite(photos.path()));
        assert_eq!(app.checked("go.favourite"), Some(true));

        app.run_for_test("go.favourite");
        assert!(app.settings.gallery.favourites.is_empty());
    }

    /// A favourite on a disk that is not plugged in says so, and stays.
    #[test]
    fn a_favourite_that_is_gone_says_so_and_stays_on_the_list() {
        let (mut app, _data, photos, disk) = with_a_favourite();
        std::fs::remove_dir(disk.path().join("keep")).unwrap();
        let ctx = egui::Context::default();
        tree_frame(&mut app, &ctx, Vec::new());
        let at = tree_frame(&mut app, &ctx, Vec::new()).at("keep").center();

        click(&mut app, &ctx, at, egui::PointerButton::Primary);
        assert_eq!(app.folder.as_deref(), Some(photos.path()));
        assert!(app.status.contains("keep"), "{}", app.status);
        assert_eq!(app.settings.gallery.favourites.len(), 1);
    }

    /// The whole of what was asked for: work ends in a favourite, and the
    /// next start opens it and lights it where it was left.
    #[test]
    fn the_favourite_left_last_time_is_lit_again_after_a_restart() {
        let (mut app, data, _photos, disk) = with_a_favourite();
        let keep = disk.path().join("keep");
        app.go(Some(keep.clone()));
        drop(app);

        let paths = Paths::resolve(Some(data.path())).unwrap();
        let settings = Settings::load(&paths);
        let folder = settings.gallery.last_folder.as_ref().map(PathBuf::from);
        assert_eq!(folder.as_deref(), Some(keep.as_path()));
        assert!(settings.gallery.is_favourite(&keep));

        let mut app = App::new(paths, settings);
        let mut root = Node::new(disk.path().to_path_buf());
        root.load_children();
        app.roots = vec![root];
        app.start(Startup {
            folder,
            ..Default::default()
        });
        let palette = *app.palette();
        let ctx = egui::Context::default();
        tree_frame(&mut app, &ctx, Vec::new());
        let drawn = tree_frame(&mut app, &ctx, Vec::new());
        assert!(drawn.lit("keep", &palette), "{:?}", drawn.names());
    }
}
