//! Proklikatelný prototyp PhotoSite v Rustu.
//!
//! Tři doky: strom složek vlevo, mřížka diapozitivů uprostřed, plný náhled
//! vpravo. Nic se nepředpočítává — náhledy se dekódují za běhu na pozadí,
//! v DCT doméně na osminu, takže složka se otevře okamžitě a dlaždice se
//! doplňují během pár desetin sekundy.

mod loader;
mod theme;

use eframe::egui;
use egui::{Sense, Vec2};
use loader::{Kind, Loader};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use theme::Palette;

const GAP: f32 = 10.0;
/// Kolik dekódovaných obrázků se drží v paměti, než začnou vypadávat ty
/// nejdéle nepoužité.
const TEXTURE_BUDGET: usize = 900;
/// Kolik hotových obrázků se za jeden snímek nahraje do GPU. Bez stropu by
/// jedna dávka dokončených dekódů zasekla hlavní vlákno.
const UPLOADS_PER_FRAME: usize = 32;

struct Args {
    folder: Option<PathBuf>,
    shot: Option<PathBuf>,
    recursive: bool,
    theme: usize,
    drag: bool,
}

fn args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        raw.iter()
            .position(|a| a == name)
            .and_then(|at| raw.get(at + 1))
            .map(PathBuf::from)
    };
    Args {
        folder: value("--folder"),
        shot: value("--shot"),
        recursive: raw.iter().any(|a| a == "--recursive"),
        drag: raw.iter().any(|a| a == "--drag"),
        theme: value("--theme")
            .and_then(|t| t.to_string_lossy().parse::<usize>().ok())
            .unwrap_or(0)
            .min(theme::PALETTES.len() - 1),
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("PhotoSite — prototyp"),
        ..Default::default()
    };
    eframe::run_native(
        "PhotoSite prototyp",
        options,
        Box::new(|cc| {
            let args = args();
            theme::apply(&cc.egui_ctx, &theme::PALETTES[args.theme]);
            let mut app = App::new();
            app.palette = args.theme;
            app.recursive = args.recursive;
            if let Some(folder) = args.folder {
                app.open(folder);
            }

            app.shot = args.shot;
            app.drag = args.drag;
            Ok(Box::new(app))
        }),
    )
}

/// Uzel stromu složek. Děti se načtou až při rozbalení — jinak by otevření
/// disku znamenalo projít celý strom.
struct Node {
    path: PathBuf,
    name: String,
    expanded: bool,
    children: Option<Vec<Node>>,
}

impl Node {
    fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self { path, name, expanded: false, children: None }
    }

    fn load_children(&mut self) {
        if self.children.is_some() {
            return;
        }

        let mut found: Vec<Node> = std::fs::read_dir(&self.path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|entry| Node::new(entry.path()))
            .filter(|node| !node.name.starts_with('$'))
            .collect();
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.children = Some(found);
    }
}

struct App {
    roots: Vec<Node>,
    folder: Option<PathBuf>,
    recursive: bool,
    photos: Vec<PathBuf>,
    selected: Option<usize>,
    tile: f32,
    palette: usize,
    loader: Loader,
    textures: HashMap<(PathBuf, Kind), egui::TextureHandle>,
    order: Vec<(PathBuf, Kind)>,
    /// Co je právě vidět. Skládá se při kreslení mřížky a na konci snímku se
    /// tím přepíše seznam přání loaderu.
    wanted_quick: Vec<PathBuf>,
    wanted_sharp: Vec<PathBuf>,
    wanted_full: Option<PathBuf>,
    status: String,
    /// Kam uložit snímek okna. Slouží jen k ověření, že to opravdu kreslí to,
    /// co si myslím — jinak by prototyp nešel zkontrolovat jinak než očima.
    shot: Option<PathBuf>,
    frames: u32,
    /// Nasimuluje tažení scrollbarem do dvou třetin a změří, za jak dlouho po
    /// zastavení jsou všechny viditelné dlaždice načtené. Bez tohohle bych
    /// opravu fronty jen tvrdil.
    drag: bool,
    forced_scroll: Option<f32>,
    settled_at: Option<std::time::Instant>,
    blank_at: Option<f64>,
    /// Viditelné dlaždice, které nemají vůbec nic, a ty, které nemají ostrou
    /// verzi. Rozlišit se to musí, protože prázdná dlaždice je problém, kdežto
    /// rozmazaná ne.
    blank: usize,
    unsharp: usize,
}

impl App {
    fn new() -> Self {
        Self {
            roots: roots(),
            folder: None,
            recursive: false,
            photos: Vec::new(),
            selected: None,
            tile: 220.0,
            palette: 0,
            loader: Loader::new(),
            textures: HashMap::new(),
            order: Vec::new(),
            wanted_quick: Vec::new(),
            wanted_sharp: Vec::new(),
            wanted_full: None,
            status: "Vyber složku vlevo".to_owned(),
            shot: None,
            frames: 0,
            drag: false,
            forced_scroll: None,
            settled_at: None,
            blank_at: None,
            blank: 0,
            unsharp: 0,
        }
    }

    fn palette(&self) -> &'static Palette {
        &theme::PALETTES[self.palette]
    }

    fn open(&mut self, folder: PathBuf) {
        let started = std::time::Instant::now();
        let walk = walkdir::WalkDir::new(&folder).max_depth(if self.recursive { 100 } else { 1 });
        let mut photos: Vec<PathBuf> = walk
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| is_photo(p))
            .collect();
        photos.sort();
        self.status = format!(
            "{} fotek za {:.0} ms",
            photos.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        self.photos = photos;
        self.selected = None;
        self.folder = Some(folder);
    }

    /// Přijme, co dorazilo z dekódovacích vláken, a udělá z toho textury.
    fn collect(&mut self, ctx: &egui::Context) {
        for done in self.loader.drain(UPLOADS_PER_FRAME) {
            let image = egui::ColorImage::from_rgb(done.size, &done.rgb);
            let handle = ctx.load_texture(
                done.path.to_string_lossy(),
                image,
                egui::TextureOptions::LINEAR,
            );
            let key = (done.path, done.kind);
            self.order.retain(|k| k != &key);
            self.order.push(key.clone());
            self.textures.insert(key, handle);
        }

        while self.order.len() > TEXTURE_BUDGET {
            let oldest = self.order.remove(0);
            self.textures.remove(&oldest);
            // Vyhozenou texturu bude potřeba vyrobit znovu, takže loader musí
            // zapomenout, že ji už kdysi dekódoval.
            self.loader.forget(&oldest);
        }
    }

    /// Označí texturu jako právě použitou, aby ji LRU nevyhodila zpod ruky.
    fn touch(&mut self, key: &(PathBuf, Kind)) {
        if let Some(at) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(at);
            self.order.push(key);
        }
    }
}

impl eframe::App for App {
    // eframe 0.36 podává rovnou kořenový Ui, panely se do něj vnořují.
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.collect(&ctx);
        let palette = *self.palette();

        egui::Panel::top("toolbar")
            .frame(egui::Frame::NONE.fill(palette.panel).inner_margin(8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let before = self.recursive;
                    ui.checkbox(&mut self.recursive, "Rekurzivně");
                    if before != self.recursive {
                        if let Some(folder) = self.folder.clone() {
                            self.open(folder);
                        }
                    }

                    ui.separator();
                    ui.label("Velikost");
                    ui.add(egui::Slider::new(&mut self.tile, 120.0..=400.0).show_value(false));

                    ui.separator();
                    let mut chosen = self.palette;
                    egui::ComboBox::from_id_salt("motiv")
                        .selected_text(theme::PALETTES[self.palette].name)
                        .show_ui(ui, |ui| {
                            for (at, p) in theme::PALETTES.iter().enumerate() {
                                ui.selectable_value(&mut chosen, at, p.name);
                            }
                        });
                    if chosen != self.palette {
                        self.palette = chosen;
                        theme::apply(&ctx, self.palette());
                    }

                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            self.folder
                                .as_ref()
                                .map(|f| f.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        )
                        .color(palette.dim),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&self.status).color(palette.dim));
                    });
                });
            });

        egui::Panel::left("tree")
            .resizable(true)
            .default_size(250.0)
            .frame(egui::Frame::NONE.fill(palette.panel).inner_margin(6.0))
            .show(ui, |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    let mut pick = None;
                    let current = self.folder.clone();
                    for root in &mut self.roots {
                        tree_node(ui, root, &palette, current.as_deref(), &mut pick);
                    }

                    if let Some(folder) = pick {
                        self.open(folder);
                    }
                });
            });

        egui::Panel::right("preview")
            .resizable(true)
            .default_size(620.0)
            .frame(egui::Frame::NONE.fill(palette.window))
            .show(ui, |ui| self.preview(ui, &palette));

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(palette.window))
            .show(ui, |ui| self.grid(ui, &palette));

        // Seznam přání se přepíše až tady, když je jasné, co je vidět a co je
        // vybrané. Všechno, co v něm není, se přestane dekódovat.
        self.loader.want(
            std::mem::take(&mut self.wanted_quick),
            std::mem::take(&mut self.wanted_sharp),
            self.wanted_full.clone(),
        );

        // Dokud něco chybí, chceme další snímek hned. Čekat 60 ms znamená, že
        // hotové dlaždice čekají na nahrání a mezitím se stihnou objednat
        // podruhé.
        if self.blank > 0 || self.unsharp > 0 || self.wanted_full.is_some() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        self.frames += 1;

        if self.drag {
            if self.frames == 90 {
                self.settled_at = Some(std::time::Instant::now());
                println!("tažení skončilo, čekám na dlaždice…");
            }

            if let Some(started) = self.settled_at {
                if self.blank == 0 && self.blank_at.is_none() && self.frames > 92 {
                    self.blank_at = Some(started.elapsed().as_secs_f64() * 1000.0);
                }

                if self.blank == 0 && self.unsharp == 0 && self.frames > 92 {
                    let (decodes, ms) = self.loader.stats();
                    println!(
                        "žádná prázdná dlaždice za {:.0} ms",
                        self.blank_at.unwrap_or(0.0)
                    );
                    println!(
                        "všechny ostré za {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                    println!(
                        "dekódů celkem {decodes}, průměr {:.1} ms na obrázek",
                        ms / decodes.max(1) as f64
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else if started.elapsed().as_secs_f64() > 30.0 {
                    println!(
                        "po 30 s pořád {} prázdných a {} neostrých",
                        self.blank, self.unsharp
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        if let Some(path) = self.shot.clone() {
            // Chvíli počkat, ať se náhledy stihnou dekódovat, pak vyfotit okno.
            // Při tažení chceme snímek z jeho průběhu, ne až po zastavení —
            // právě tam si stěžuješ na prázdné boxy.
            let at = if self.drag { 60 } else { 90 };
            if self.frames == at {
                if !self.photos.is_empty() && !self.drag {
                    self.selected = Some(0);
                }

                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::UserData::default(),
                ));
            }

            let captured = ctx.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = captured {
                let rgba: Vec<u8> =
                    image.pixels.iter().flat_map(|c| c.to_array()).collect();
                let _ = image::save_buffer(
                    &path,
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                );
                println!("snímek uložen: {}", path.display());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl App {
    fn grid(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        if self.photos.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Žádné fotky").color(palette.dim));
            });
            return;
        }

        let tile_w = self.tile;
        let tile_h = self.tile * 0.72 + theme::CAPTION + theme::PADDING;
        let count = self.photos.len();

        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(offset) = self.forced_scroll {
            scroll = scroll.vertical_scroll_offset(offset);
        }

        scroll.show_viewport(
            ui,
            |ui, viewport| {
                let width = ui.available_width();
                let cols = (((width - GAP) / (tile_w + GAP)).floor() as usize).max(1);
                let rows = count.div_ceil(cols);
                let pitch = tile_h + GAP;
                let (area, _) = ui.allocate_exact_size(
                    Vec2::new(width, rows as f32 * pitch + GAP),
                    Sense::hover(),
                );

                // Vidět je jen pár řádků; zbytek se nekreslí ani nepočítá.
                let first = ((viewport.min.y - GAP) / pitch).floor().max(0.0) as usize;
                let last = (((viewport.max.y) / pitch).ceil() as usize).min(rows);
                let mut wanted_quick: Vec<(usize, PathBuf)> = Vec::new();
                let mut wanted_sharp: Vec<(usize, PathBuf)> = Vec::new();
                let mut clicked = None;

                for row in first..last {
                    for col in 0..cols {
                        let index = row * cols + col;
                        if index >= count {
                            break;
                        }

                        let rect = egui::Rect::from_min_size(
                            area.min
                                + Vec2::new(
                                    GAP + col as f32 * (tile_w + GAP),
                                    GAP + row as f32 * pitch,
                                ),
                            Vec2::new(tile_w, tile_h),
                        );
                        let path = &self.photos[index];
                        let response =
                            ui.interact(rect, ui.id().with(index), Sense::click());
                        if response.clicked() {
                            clicked = Some(index);
                        }

                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let well = theme::slide(
                            ui.painter(),
                            rect,
                            palette,
                            &name,
                            self.selected == Some(index),
                            response.hovered(),
                        );

                        // Ostrá verze má přednost, ale dokud není, kreslí se
                        // ta z EXIFu. Prázdná dlaždice je až třetí možnost.
                        let sharp = (path.clone(), Kind::Thumb);
                        let quick = (path.clone(), Kind::Quick);
                        let has_sharp = self.textures.contains_key(&sharp);
                        if !has_sharp {
                            wanted_sharp.push((index, path.clone()));
                        }

                        let shown = if has_sharp {
                            self.textures.get(&sharp)
                        } else {
                            self.textures.get(&quick)
                        };
                        match shown {
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
                    }
                }

                // Od středu viewportu ven: doprostřed se člověk dívá.
                let middle = (first + last) as f32 * 0.5 * cols as f32;
                let order = |list: Vec<(usize, PathBuf)>| {
                    let mut list = list;
                    list.sort_by_key(|(index, _)| (*index as f32 - middle).abs() as i64);
                    list.into_iter().map(|(_, path)| path).collect::<Vec<_>>()
                };
                self.wanted_quick = order(wanted_quick);
                self.wanted_sharp = order(wanted_sharp);

                // Dotknout se použitých až po kreslení, aby LRU nevyhodila
                // zrovna to, co je na obrazovce.
                let visible: Vec<(PathBuf, Kind)> = (first * cols
                    ..(last * cols).min(count))
                    .map(|i| (self.photos[i].clone(), Kind::Thumb))
                    .collect();
                for key in &visible {
                    self.touch(key);
                }

                if let Some(index) = clicked {
                    self.selected = Some(index);
                }

                self.blank = self.wanted_quick.len();
                self.unsharp = self.wanted_sharp.len();
                if self.drag {
                    // 90 snímků tažení do dvou třetin obsahu, pak stát.
                    let total = rows as f32 * pitch;
                    self.forced_scroll = Some(if self.frames < 90 {
                        total * 0.66 * (self.frames as f32 / 90.0)
                    } else {
                        total * 0.66
                    });
                }
            },
        );
    }

    fn preview(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        self.wanted_full = None;
        let Some(index) = self.selected else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Klikni na dlaždici").color(palette.dim),
                );
            });
            return;
        };

        let path = self.photos[index].clone();
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, palette.well);

        let full = (path.clone(), Kind::Full);
        let chosen = if self.textures.contains_key(&full) {
            Some(full.clone())
        } else {
            self.wanted_full = Some(path.clone());
            // Než se dekóduje plné rozlišení, ukáže se zvětšená dlaždice —
            // panel tak nikdy neproblikne prázdnotou.
            let fallback = (path.clone(), Kind::Thumb);
            self.textures.contains_key(&fallback).then_some(fallback)
        };

        if let Some(key) = chosen {
            if let Some(texture) = self.textures.get(&key) {
                let size = texture.size();
                ui.painter().image(
                    texture.id(),
                    theme::fit(area.shrink(12.0), size),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            self.touch(&key);
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ui.painter().text(
            egui::pos2(area.center().x, area.max.y - 14.0),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(12.0),
            palette.dim,
        );
    }
}

fn tree_node(
    ui: &mut egui::Ui,
    node: &mut Node,
    palette: &Palette,
    current: Option<&Path>,
    pick: &mut Option<PathBuf>,
) {
    let is_current = current == Some(node.path.as_path());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // Trojúhelník se kreslí, nepíše: ▸ a ▾ v základním fontu egui nejsou
        // a vyjdou jako prázdné čtverečky.
        let (arrow, arrow_response) =
            ui.allocate_exact_size(Vec2::new(14.0, 16.0), Sense::click());
        let center = arrow.center();
        let tint = if arrow_response.hovered() { palette.text } else { palette.dim };
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
        if arrow_response.clicked() {
            node.expanded = !node.expanded;
            if node.expanded {
                node.load_children();
            }
        }

        let label = egui::RichText::new(&node.name)
            .color(if is_current { palette.accent } else { palette.text });
        if ui.add(egui::Button::new(label).frame(false)).clicked() {
            node.load_children();
            node.expanded = true;
            *pick = Some(node.path.clone());
        }
    });

    if node.expanded {
        if let Some(children) = node.children.as_mut() {
            ui.indent(node.path.as_path(), |ui| {
                for child in children {
                    tree_node(ui, child, palette, current, pick);
                }
            });
        }
    }
}

fn is_photo(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp" | "bmp" | "tif" | "tiff" | "gif")
    )
}

/// Kořeny stromu. Na Windows disky, jinde kořen a domovská složka.
fn roots() -> Vec<Node> {
    let mut found = Vec::new();
    if cfg!(windows) {
        for letter in 'A'..='Z' {
            let path = PathBuf::from(format!("{letter}:\\"));
            if path.exists() {
                let mut node = Node::new(path.clone());
                node.name = format!("{letter}:");
                found.push(node);
            }
        }
    } else {
        found.push(Node::new(PathBuf::from("/")));
    }

    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let path = PathBuf::from(home);
        if path.exists() {
            found.insert(0, Node::new(path));
        }
    }

    found
}
