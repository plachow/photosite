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

    let gallery = app.gallery().clone();
    let tile_w = gallery.tile_size as f32;
    let tile_h = theme::tile_height(&gallery);
    let gap = gallery.gap as f32;
    let margin = gallery.prefetch_rows.clamp(0, 64) as usize;
    let count = app.count();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let cols = (((width - gap) / (tile_w + gap)).floor() as usize).max(1);
            let rows = count.div_ceil(cols);
            let pitch = tile_h + gap;
            let (area, _) =
                ui.allocate_exact_size(Vec2::new(width, rows as f32 * pitch + gap), Sense::hover());

            let first = ((viewport.min.y - gap) / pitch).floor().max(0.0) as usize;
            let last = ((viewport.max.y / pitch).ceil() as usize).min(rows);

            let mut wanted_quick: Vec<(usize, PathBuf)> = Vec::new();
            let mut wanted_sharp: Vec<(usize, PathBuf)> = Vec::new();
            let mut clicked: Option<(usize, egui::Modifiers)> = None;

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
                        theme::badges(ui.painter(), well, palette, &organisation);
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

            // Plain, Ctrl and Shift, the way every file list has worked for
            // thirty years. Getting this wrong is not a small thing: the
            // rating keys land on whatever is selected.
            if let Some((index, modifiers)) = clicked {
                if modifiers.command || modifiers.ctrl {
                    app.select_also(index);
                } else if modifiers.shift {
                    app.select_through(index);
                } else {
                    app.select_only(index);
                }
            }
        });
}

pub fn preview(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    app.wanted_preview = None;
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
        app.wanted_preview = Some(path.clone());
    }

    if let Some(key) = chosen {
        if let Some(texture) = app.texture(&key) {
            let size = texture.size();
            ui.painter().image(
                texture.id(),
                theme::fit(area.shrink(12.0), size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        app.touch(&key);
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

pub fn tree(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut pick = None;
            let current = app.folder.clone();
            // It has to be taken before drawing: if the scroll request were
            // cleared afterwards, it would clear the very one this tree just
            // created by being clicked.
            let scroll_to = app.scroll_tree_to.take();
            let mut roots = std::mem::take(&mut app.roots);
            for root in &mut roots {
                node(
                    ui,
                    root,
                    palette,
                    current.as_deref(),
                    scroll_to.as_deref(),
                    &mut pick,
                );
            }

            app.roots = roots;
            if let Some(folder) = pick {
                app.open(folder);
            }
        });
}

fn node(
    ui: &mut egui::Ui,
    node: &mut Node,
    palette: &Palette,
    current: Option<&Path>,
    scroll_to: Option<&Path>,
    pick: &mut Option<PathBuf>,
) {
    let is_current = current == Some(node.path.as_path());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;

        // The triangle is drawn, not written: egui's default font has no
        // ▸ or ▾ and they would come out as empty boxes.
        let (rect, response) = ui.allocate_exact_size(Vec2::new(14.0, 16.0), Sense::click());
        let center = rect.center();
        let tint = theme::color(if response.hovered() {
            palette.text
        } else {
            palette.dim
        });
        let points = if node.expanded {
            vec![
                egui::pos2(center.x - 4.0, center.y - 2.0),
                egui::pos2(center.x + 4.0, center.y - 2.0),
                egui::pos2(center.x, center.y + 3.0),
            ]
        } else {
            vec![
                egui::pos2(center.x - 2.0, center.y - 4.0),
                egui::pos2(center.x + 3.0, center.y),
                egui::pos2(center.x - 2.0, center.y + 4.0),
            ]
        };
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            tint,
            egui::Stroke::NONE,
        ));
        if response.clicked() {
            node.expanded = !node.expanded;
            if node.expanded {
                node.load_children();
            }
        }

        let label = egui::RichText::new(&node.name).color(theme::color(if is_current {
            palette.accent
        } else {
            palette.text
        }));
        let response = ui.add(egui::Button::new(label).frame(false));
        // An expanded tree is not enough on its own: the open folder may sit
        // far below the edge of the pane, and then it is no use.
        if scroll_to == Some(node.path.as_path()) {
            response.scroll_to_me(Some(egui::Align::Center));
        }

        if response.clicked() {
            node.load_children();
            node.expanded = true;
            *pick = Some(node.path.clone());
        }
    });

    if node.expanded
        && let Some(children) = node.children.as_mut()
    {
        ui.indent(node.path.as_path(), |ui| {
            for child in children {
                self::node(ui, child, palette, current, scroll_to, pick);
            }
        });
    }
}
