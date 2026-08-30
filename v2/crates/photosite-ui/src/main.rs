//! PhotoSite.
//!
//! Tahle crate je jediná, která ví o egui a o GPU. Všechno ostatní —
//! katalog, cesty, nastavení, úlohy, příkazy — bydlí v jádře a dá se
//! otestovat bez okna.

mod grid;
mod theme;

use anyhow::Result;
use eframe::egui;
use photosite_core::commands::{Bindings, Group, Shortcut};
use photosite_core::{Config, Paths, commands, diagnostics, jobs};
use photosite_image as img;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Co se z obrázku chce. Pořadí je zároveň priorita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Want {
    /// Náhled z EXIFu: rozmazaný, ale hned.
    Quick,
    Thumb,
    Preview,
}

pub type Key = (PathBuf, Want);

/// Hotový obrázek čekající na nahrání do GPU.
pub struct Pixels {
    pub size: [usize; 2],
    pub rgb: Vec<u8>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pixels").field("size", &self.size).finish()
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut data: Option<PathBuf> = None;
    let mut verbose = false;
    let mut selftest = false;
    let mut folder: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "--verbose" | "-v" => verbose = true,
            "--selftest" => selftest = true,
            other => folder = Some(PathBuf::from(other)),
        }
    }

    let paths = Paths::resolve(data.as_deref())?;
    paths.ensure()?;
    let _logging = diagnostics::start(&paths, verbose);
    diagnostics::install_panic_hook(&paths);
    tracing::info!(verze = photosite_core::VERSION, "start");

    let config = Config::load(&paths);
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([config.window.width, config.window.height])
        .with_min_inner_size([900.0, 600.0])
        .with_maximized(config.window.maximized)
        .with_title("PhotoSite");
    if let (Some(x), Some(y)) = (config.window.x, config.window.y) {
        viewport = viewport.with_position([x, y]);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "PhotoSite",
        options,
        Box::new(move |cc| {
            let palette = theme::by_id(&config.appearance.theme).1;
            theme::apply(&cc.egui_ctx, palette);
            let mut app = App::new(paths, config, folder);
            app.selftest = selftest;
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("okno se nepodařilo otevřít: {error}"))
}

/// Uzel stromu složek. Děti se načtou až při rozbalení.
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub expanded: bool,
    pub children: Option<Vec<Node>>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("path", &self.path).finish()
    }
}

impl Node {
    fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self {
            path,
            name,
            expanded: false,
            children: None,
        }
    }

    pub fn load_children(&mut self) {
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
        found.sort_by_key(|a| a.name.to_lowercase());
        self.children = Some(found);
    }
}

/// Kolik obrázků se za snímek nahraje do GPU. Bez stropu by jedna dávka
/// dokončených dekódů zasekla vlákno, které kreslí.
const UPLOADS_PER_FRAME: usize = 32;
/// Kolik textur se drží, než začnou vypadávat nejdéle nepoužité.
const TEXTURE_BUDGET: usize = 900;

pub struct App {
    paths: Paths,
    config: Config,
    bindings: Bindings,
    tasks: jobs::Tasks,
    images: jobs::Wishlist<Key, Pixels>,

    pub roots: Vec<Node>,
    pub folder: Option<PathBuf>,
    pub photos: Vec<PathBuf>,
    pub selected: Option<usize>,

    textures: HashMap<Key, egui::TextureHandle>,
    order: Vec<Key>,
    pub wanted_quick: Vec<PathBuf>,
    pub wanted_sharp: Vec<PathBuf>,
    pub wanted_preview: Option<PathBuf>,
    pub blank: usize,
    pub unsharp: usize,

    palette: usize,
    status: String,
    show_diagnostics: bool,

    /// Samokontrola: otevři složku, chvíli běž a ověř, že žádná viditelná
    /// dlaždice nezůstala prázdná. Přesně tuhle vadu měl WPF benchmark, kde
    /// jedna spolknutá výjimka způsobila, že se nevyrobil jediný náhled a
    /// aplikace se přitom tvářila, že běží.
    pub selftest: bool,
    frames: u32,
    started: std::time::Instant,
}

// TextureHandle Debug neimplementuje, takže si ho napíšeme sami — a rovnou
// tak, aby vypisoval to, co je při ladění zajímavé.
impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("folder", &self.folder)
            .field("fotek", &self.photos.len())
            .field("textur", &self.textures.len())
            .field("prázdných", &self.blank)
            .finish()
    }
}

impl App {
    fn new(paths: Paths, config: Config, folder: Option<PathBuf>) -> Self {
        let images = jobs::Wishlist::new(jobs::worker_count(), |key: &Key| {
            let (path, want) = key;
            let outcome = match want {
                Want::Quick => img::quick(path).map(|found| {
                    found.map(|rgb| Pixels {
                        size: [rgb.width as usize, rgb.height as usize],
                        rgb: rgb.pixels,
                    })
                }),
                Want::Thumb => img::sized(path, img::THUMB).map(|rgb| {
                    Some(Pixels {
                        size: [rgb.width as usize, rgb.height as usize],
                        rgb: rgb.pixels,
                    })
                }),
                Want::Preview => img::sized(path, img::PREVIEW).map(|rgb| {
                    Some(Pixels {
                        size: [rgb.width as usize, rgb.height as usize],
                        rgb: rgb.pixels,
                    })
                }),
            };

            match outcome {
                Ok(pixels) => pixels,
                Err(error) => {
                    // Nečitelný soubor je v knihovně o desítkách tisíc fotek
                    // normální jev. Tichý není.
                    tracing::warn!(path = %path.display(), ?want, error = %format!("{error:#}"), "obrázek nelze načíst");
                    None
                }
            }
        });

        let (palette, _) = theme::by_id(&config.appearance.theme);
        let mut app = Self {
            paths,
            config,
            bindings: Bindings::defaults(),
            tasks: jobs::Tasks::new(),
            images,
            roots: roots(),
            folder: None,
            photos: Vec::new(),
            selected: None,
            textures: HashMap::new(),
            order: Vec::new(),
            wanted_quick: Vec::new(),
            wanted_sharp: Vec::new(),
            wanted_preview: None,
            blank: 0,
            unsharp: 0,
            palette,
            status: "Vyber složku vlevo".to_owned(),
            show_diagnostics: false,
            selftest: false,
            frames: 0,
            started: std::time::Instant::now(),
        };

        let start = folder.or_else(|| app.config.gallery.last_folder.as_ref().map(PathBuf::from));
        if let Some(folder) = start.filter(|f| f.is_dir()) {
            app.open(folder);
        }

        app
    }

    pub fn config_tile(&self) -> f32 {
        self.config.gallery.tile_size
    }

    pub fn palette(&self) -> &'static theme::Palette {
        &theme::PALETTES[self.palette]
    }

    pub fn open(&mut self, folder: PathBuf) {
        let started = std::time::Instant::now();
        let depth = if self.config.gallery.recursive {
            usize::MAX
        } else {
            1
        };
        let mut photos: Vec<PathBuf> = walkdir::WalkDir::new(&folder)
            .max_depth(depth)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .filter(|path| photosite_core::is_photo(path))
            .collect();
        photos.sort();
        self.status = format!(
            "{} fotek za {:.0} ms",
            photos.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        tracing::info!(folder = %folder.display(), pocet = photos.len(), "složka otevřena");
        self.photos = photos;
        self.selected = None;
        self.config.gallery.last_folder = Some(folder.to_string_lossy().into_owned());
        self.folder = Some(folder);
    }

    pub fn texture(&self, key: &Key) -> Option<&egui::TextureHandle> {
        self.textures.get(key)
    }

    pub fn has(&self, path: &Path, want: Want) -> bool {
        self.textures.contains_key(&(path.to_path_buf(), want))
    }

    /// Označí texturu jako právě použitou, aby ji LRU nevyhodila zpod ruky.
    pub fn touch(&mut self, key: &Key) {
        if let Some(at) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(at);
            self.order.push(key);
        }
    }

    fn collect(&mut self, ctx: &egui::Context) {
        for (key, pixels) in self.images.drain(UPLOADS_PER_FRAME) {
            let image = egui::ColorImage::from_rgb(pixels.size, &pixels.rgb);
            let handle =
                ctx.load_texture(key.0.to_string_lossy(), image, egui::TextureOptions::LINEAR);
            self.order.retain(|existing| existing != &key);
            self.order.push(key.clone());
            self.textures.insert(key, handle);
        }

        while self.order.len() > TEXTURE_BUDGET {
            let oldest = self.order.remove(0);
            self.textures.remove(&oldest);
            // Vyhozenou texturu bude potřeba vyrobit znovu.
            self.images.forget(&oldest);
        }
    }

    fn run(&mut self, id: &str, ctx: &egui::Context) {
        match id {
            "file.rescan" => {
                if let Some(folder) = self.folder.clone() {
                    self.open(folder);
                }
            }
            "file.quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "view.recursive" => {
                self.config.gallery.recursive = !self.config.gallery.recursive;
                if let Some(folder) = self.folder.clone() {
                    self.open(folder);
                }
            }
            "view.bigger_tiles" => {
                self.config.gallery.tile_size = (self.config.gallery.tile_size * 1.25).min(460.0);
            }
            "view.smaller_tiles" => {
                self.config.gallery.tile_size = (self.config.gallery.tile_size / 1.25).max(110.0);
            }
            "view.next_theme" => {
                self.palette = (self.palette + 1) % theme::PALETTES.len();
                self.config.appearance.theme = self.palette().id.to_owned();
                theme::apply(ctx, self.palette());
            }
            "help.diagnostics" => self.show_diagnostics = !self.show_diagnostics,
            // Otevírací dialog přijde s `rfd`; do té doby se složka vybírá
            // ve stromu vlevo.
            "file.open_folder" => self.status = "Složku vyber ve stromu vlevo".to_owned(),
            other => tracing::warn!(prikaz = other, "příkaz bez obsluhy"),
        }
    }

    /// Zkratky se čtou z registru, ne z natvrdo napsaných podmínek.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed: Vec<Shortcut> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some(Shortcut {
                        ctrl: modifiers.command || modifiers.ctrl,
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        key: key.name().to_owned(),
                    }),
                    _ => None,
                })
                .collect()
        });

        for shortcut in pressed {
            if let Some(command) = self.bindings.command_for(&shortcut) {
                tracing::debug!(prikaz = command.id, "zkratka");
                self.run(command.id, ctx);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.collect(&ctx);
        self.shortcuts(&ctx);
        let palette = *self.palette();

        egui::Panel::top("toolbar")
            .frame(egui::Frame::NONE.fill(palette.panel).inner_margin(6.0))
            .show(ui, |ui| self.toolbar(ui, &palette, &ctx));

        egui::Panel::left("tree")
            .resizable(true)
            .default_size(self.config.window.tree_width)
            .frame(egui::Frame::NONE.fill(palette.panel).inner_margin(6.0))
            .show(ui, |ui| {
                self.config.window.tree_width = ui.available_width();
                grid::tree(self, ui, &palette);
            });

        egui::Panel::right("preview")
            .resizable(true)
            .default_size(self.config.window.preview_width)
            .frame(egui::Frame::NONE.fill(palette.window))
            .show(ui, |ui| {
                self.config.window.preview_width = ui.available_width();
                grid::preview(self, ui, &palette);
            });

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(palette.window))
            .show(ui, |ui| grid::gallery(self, ui, &palette));

        if self.show_diagnostics {
            let text = diagnostics::about(&self.paths);
            let running = self.tasks.running().len();
            let mut open = true;
            egui::Window::new("Diagnostika")
                .open(&mut open)
                .show(&ctx, |ui| {
                    ui.monospace(&text);
                    ui.monospace(format!("fotek ve složce  {}", self.photos.len()));
                    ui.monospace(format!("textur v paměti  {}", self.textures.len()));
                    ui.monospace(format!("dekóduje se      {}", self.images.running()));
                    ui.monospace(format!("běžících úloh    {running}"));
                    ui.monospace(format!("prázdných        {}", self.blank));
                    ui.monospace(format!("neostrých        {}", self.unsharp));
                });
            self.show_diagnostics = open;
        }

        // Seznam přání se přepíše až tady, když je jasné, co je vidět a co je
        // vybrané. Všechno, co v něm není, se přestane dekódovat.
        self.images.wish(vec![
            std::mem::take(&mut self.wanted_quick)
                .into_iter()
                .map(|path| (path, Want::Quick))
                .collect(),
            self.wanted_preview
                .clone()
                .into_iter()
                .map(|path| (path, Want::Preview))
                .collect(),
            std::mem::take(&mut self.wanted_sharp)
                .into_iter()
                .map(|path| (path, Want::Thumb))
                .collect(),
        ]);

        // Dokud něco chybí, chceme další snímek hned. Čekat znamená, že hotové
        // dlaždice leží a mezitím se stihnou objednat podruhé.
        if self.blank > 0 || self.unsharp > 0 || self.wanted_preview.is_some() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        self.frames += 1;
        if self.selftest {
            self.run_selftest(&ctx);
        }

        // Stav okna se sbírá každý snímek, ukládá se až při zavření.
        ctx.input(|input| {
            if let Some(rect) = input.viewport().inner_rect {
                self.config.window.width = rect.width();
                self.config.window.height = rect.height();
            }

            if let Some(outer) = input.viewport().outer_rect {
                self.config.window.x = Some(outer.min.x);
                self.config.window.y = Some(outer.min.y);
            }

            self.config.window.maximized = input.viewport().maximized.unwrap_or(false);
        });
    }
}

impl App {
    fn toolbar(&mut self, ui: &mut egui::Ui, palette: &theme::Palette, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let mut recursive = self.config.gallery.recursive;
            if ui.checkbox(&mut recursive, "Rekurzivně").changed() {
                self.run("view.recursive", ctx);
            }

            ui.separator();
            // Tlačítka se berou z registru příkazů, ne z ručně psaného seznamu.
            for command in commands::COMMANDS.iter().filter(|c| c.group == Group::View) {
                if command.id == "view.recursive" {
                    continue;
                }

                let label = match self.bindings.shortcut(command.id) {
                    Some(shortcut) => format!("{}  ({shortcut})", command.title),
                    None => command.title.to_owned(),
                };
                if ui.button(command.title).on_hover_text(label).clicked() {
                    self.run(command.id, ctx);
                }
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
                for task in self.tasks.running() {
                    ui.label(egui::RichText::new(&task.title).color(palette.accent));
                }
            });
        });
    }
}

impl App {
    fn run_selftest(&mut self, ctx: &egui::Context) {
        let elapsed = self.started.elapsed().as_secs_f64();
        // Pár snímků na rozjezd, pak se čeká, až se dlaždice doplní.
        if self.frames < 10 {
            return;
        }

        if self.photos.is_empty() {
            println!("SAMOKONTROLA SELHALA: ve složce nejsou žádné fotky");
            std::process::exit(2);
        }

        if self.blank == 0 {
            println!(
                "samokontrola v pořádku: {} fotek, žádná prázdná dlaždice, {:.0} ms",
                self.photos.len(),
                elapsed * 1000.0
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if elapsed > 20.0 {
            println!(
                "SAMOKONTROLA SELHALA: po {elapsed:.0} s je {} dlaždic prázdných",
                self.blank
            );
            std::process::exit(3);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(error) = self.config.save(&self.paths) {
            tracing::error!(error = %format!("{error:#}"), "nastavení se nepodařilo uložit");
        }
    }
}

/// Kořeny stromu. Jediné místo, kde na platformě záleží.
fn roots() -> Vec<Node> {
    let mut found = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let path = PathBuf::from(home);
        if path.exists() {
            found.push(Node::new(path));
        }
    }

    #[cfg(windows)]
    for letter in 'A'..='Z' {
        let path = PathBuf::from(format!("{letter}:\\"));
        if path.exists() {
            let mut node = Node::new(path);
            node.name = format!("{letter}:");
            found.push(node);
        }
    }

    #[cfg(not(windows))]
    {
        found.push(Node::new(PathBuf::from("/")));
        let user = std::env::var("USER").unwrap_or_default();
        for mount in [
            PathBuf::from("/Volumes"),
            PathBuf::from(format!("/media/{user}")),
            PathBuf::from(format!("/run/media/{user}")),
            PathBuf::from("/mnt"),
        ] {
            let Ok(entries) = std::fs::read_dir(&mount) else {
                continue;
            };

            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    found.push(Node::new(entry.path()));
                }
            }
        }
    }

    found
}
