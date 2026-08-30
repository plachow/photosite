//! Kreslení doků podle stromu z [`photosite_core::docks`].
//!
//! Tenhle soubor o žádném konkrétním rozložení neví. Dostane strom, rozdělí
//! podle něj obdélník okna a každou plochu předá jejímu kreslíři. Přidat
//! informace pod náhled znamená změnit řetězec v nastavení, ne sáhnout sem.
//!
//! Dělítko je jediné místo, kde se rozložení mění, a drží tři pravidla:
//! táhne se, dvojklik vrátí půl na půl, a **pod nejmenší velikost doku
//! nepustí**. To poslední není kosmetika: náhled se dal přetáhnout na osm
//! pixelů, uložilo se to a zpátky ho nedostalo nic.

use crate::{App, grid, info, theme};
use eframe::egui;
use egui::Sense;
use photosite_core::docks::{Axis, Layout};
use photosite_core::t;
use photosite_core::theme::Palette;
use std::collections::HashSet;

/// Změna, kterou dělítko udělalo. Zapisuje se až po kreslení, aby se strom
/// neměnil zprostřed průchodu.
struct Moved {
    path: Vec<bool>,
    ratio: f64,
}

/// Co platí po celý průchod stromem. Pohromadě, ať se to netahá po jednom
/// argumentu do každého patra.
struct Board<'a> {
    palette: &'a Palette,
    hidden: HashSet<&'a str>,
    splitter: f64,
}

/// Dělítko i s tím, co o něm potřebuje vědět tažení.
struct Bar {
    rect: egui::Rect,
    axis: Axis,
    /// Kolik místa mají obě části dohromady, bez dělítka samotného.
    usable: f64,
    ratio: f64,
}

pub fn show(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    // Strom i seznam schovaných se vytáhnou stranou; kreslení si `app` půjčí
    // celý a půjčka na jeho pole by to zablokovala.
    let layout = app.layout.clone();
    let closed = app.hidden.clone();
    let board = Board {
        palette,
        hidden: closed.iter().map(String::as_str).collect(),
        splitter: app.settings.window.splitter.clamp(2.0, 16.0),
    };
    let rect = ui.available_rect_before_wrap();

    // Všechno schované by nechalo prázdné okno bez čehokoliv, čím ho vrátit.
    if !layout.visible(&board.hidden) {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(t!("docks-all-hidden")).color(theme::color(palette.dim)));
        });
        return;
    }

    let mut moved = None;
    let mut path = Vec::new();
    draw(app, ui, &board, &layout, rect, &mut path, &mut moved);

    if let Some(moved) = moved {
        app.layout.set_ratio(&moved.path, moved.ratio);
        app.settings.window.layout = app.layout.to_string();
    }
}

fn draw(
    app: &mut App,
    ui: &mut egui::Ui,
    board: &Board,
    node: &Layout,
    rect: egui::Rect,
    path: &mut Vec<bool>,
    moved: &mut Option<Moved>,
) {
    let Layout::Split {
        axis,
        ratio,
        first,
        second,
    } = node
    else {
        let Layout::Pane(id) = node else { return };
        return pane(app, ui, board.palette, id, rect);
    };

    // Schovaná polovina nebere místo ani si neúčtuje dělítko.
    if !first.visible(&board.hidden) {
        return draw(app, ui, board, second, rect, path, moved);
    }

    if !second.visible(&board.hidden) {
        return draw(app, ui, board, first, rect, path, moved);
    }

    let along = match axis {
        Axis::Across => rect.width() as f64,
        Axis::Down => rect.height() as f64,
    };
    let usable = (along - board.splitter).max(0.0);
    let ratio = Layout::clamp_ratio(
        first.min_along(*axis, &board.hidden, board.splitter),
        second.min_along(*axis, &board.hidden, board.splitter),
        usable,
        *ratio,
    );

    let cut = (usable * ratio) as f32;
    let thick = board.splitter as f32;
    let (first_rect, bar, second_rect) = match axis {
        Axis::Across => (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + cut, rect.max.y)),
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x + cut, rect.min.y),
                egui::pos2(rect.min.x + cut + thick, rect.max.y),
            ),
            egui::Rect::from_min_max(egui::pos2(rect.min.x + cut + thick, rect.min.y), rect.max),
        ),
        Axis::Down => (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + cut)),
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.min.y + cut),
                egui::pos2(rect.max.x, rect.min.y + cut + thick),
            ),
            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + cut + thick), rect.max),
        ),
    };

    path.push(false);
    draw(app, ui, board, first, first_rect, path, moved);
    path.pop();

    handle(
        ui,
        board,
        &Bar {
            rect: bar,
            axis: *axis,
            usable,
            ratio,
        },
        path,
        moved,
    );

    path.push(true);
    draw(app, ui, board, second, second_rect, path, moved);
    path.pop();
}

/// Dělítko: táhnout, nebo dvojklikem zpátky na půl.
fn handle(ui: &mut egui::Ui, board: &Board, bar: &Bar, path: &[bool], moved: &mut Option<Moved>) {
    let id = ui.id().with(("delitko", path));
    let response = ui.interact(bar.rect, id, Sense::click_and_drag());
    let cursor = match bar.axis {
        Axis::Across => egui::CursorIcon::ResizeHorizontal,
        Axis::Down => egui::CursorIcon::ResizeVertical,
    };
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(cursor);
    }

    // Zvýrazní se, až když je na něm myš — jinak by okno rozřezaly svítící
    // čáry, kterých si nikdo nechtěl všímat.
    let tint = if response.hovered() || response.dragged() {
        board.palette.accent
    } else {
        board.palette.bevel_dark
    };
    ui.painter().rect_filled(bar.rect, 0, theme::color(tint));

    if response.double_clicked() {
        *moved = Some(Moved {
            path: path.to_vec(),
            ratio: 0.5,
        });
        return;
    }

    if response.dragged() && bar.usable > 0.0 {
        let delta = match bar.axis {
            Axis::Across => response.drag_delta().x,
            Axis::Down => response.drag_delta().y,
        };
        if delta != 0.0 {
            *moved = Some(Moved {
                path: path.to_vec(),
                ratio: bar.ratio + delta as f64 / bar.usable,
            });
        }
    }
}

/// Jedna plocha. Tady je jediný seznam, který ví, co která znamená.
fn pane(app: &mut App, ui: &mut egui::Ui, palette: &Palette, id: &str, rect: egui::Rect) {
    let fill = match id {
        "tree" => palette.panel,
        _ => palette.window,
    };
    ui.painter().rect_filled(rect, 0, theme::color(fill));

    let mut child = child_ui(ui, id, rect.shrink(if id == "tree" { 6.0 } else { 0.0 }));
    child.set_clip_rect(rect);
    match id {
        "tree" => grid::tree(app, &mut child, palette),
        "gallery" => grid::gallery(app, &mut child, palette),
        "preview" => grid::preview(app, &mut child, palette),
        "info" => info::pane(app, &mut child, palette),
        // Neznámou plochu sem rozložení nepustí; kdyby přece, ať je vidět
        // prázdné místo a ne pád.
        other => tracing::warn!(plocha = other, "plocha bez kreslíře"),
    }
}

/// Prostor pro jednu plochu.
///
/// Klíč `id` tu není kosmetika. Bez něj dá egui všem dětem téhož rodiče
/// stejnou sůl — doslova `"child"` — a rozliší je jen pořadím, ve kterém
/// vznikly. Rolovací plochy uvnitř si pak sáhnou na společný stav a kolečko
/// nad stromem složek posouvá dlaždice v mřížce. Navíc by stačilo jednu
/// plochu schovat, aby se pořadí posunulo a stavy se prohodily.
fn child_ui(ui: &mut egui::Ui, id: &str, rect: egui::Rect) -> egui::Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Postaví dvě plochy vedle sebe, v každé rolovací seznam, pošle kolečko
    /// nad tu levou a vrátí posun obou.
    fn wheel_over_left(salt: bool) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut offsets = (0.0, 0.0);
        for _ in 0..2 {
            // Dvakrát: první průchod plochy teprve zakládá, měří se druhý.
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(100.0, 300.0)));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });

            let mut out = ctx.run_ui(input, |ui| {
                let left =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 600.0));
                let right =
                    egui::Rect::from_min_size(egui::pos2(400.0, 0.0), egui::vec2(400.0, 600.0));
                for (name, rect, first) in [("vlevo", left, true), ("vpravo", right, false)] {
                    let mut child = if salt {
                        child_ui(ui, name, rect)
                    } else {
                        ui.new_child(egui::UiBuilder::new().max_rect(rect))
                    };
                    let out = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(&mut child, |ui| {
                            for row in 0..200 {
                                ui.label(format!("{row}"));
                            }
                        });
                    if first {
                        offsets.0 = out.state.offset.y;
                    } else {
                        offsets.1 = out.state.offset.y;
                    }
                }
            });

            // Bez renderu se textury nikam nenahrají a epaint by na to při
            // zahození upozornil pádem.
            out.textures_delta.clear();
        }

        offsets
    }

    /// Roluje se tam, kde je myš. Nic jiného se hnout nesmí.
    ///
    /// Tohle bylo rozbité hned první den, co doky nahradily pevné panely:
    /// kolečko nad stromem složek posouvalo dlaždice.
    #[test]
    fn kolecko_hne_jen_plochou_pod_mysi() {
        let (vlevo, vpravo) = wheel_over_left(true);
        assert!(vlevo > 0.0, "plocha pod myší se neposunula ({vlevo})");
        assert_eq!(vpravo, 0.0, "posunula se i plocha, nad kterou myš nebyla");
    }

    /// Měřidlo samo: bez vlastního klíče se rolování opravdu rozteče, jinak
    /// by test výš hlídal něco, co nikdy nespadne.
    #[test]
    fn bez_vlastniho_klice_se_rolovani_rozteka() {
        let (vlevo, vpravo) = wheel_over_left(false);
        assert_eq!(
            vlevo, vpravo,
            "bez klíče se plochy chovaly správně — test výš pak nehlídá nic"
        );
        assert!(vpravo > 0.0, "nehnulo se vůbec nic, tak se nic neměří");
    }
}
