//! PhotoSite.
//!
//! Tahle crate je jediná, která ví o egui a o GPU. Všechno ostatní —
//! katalog, cesty, nastavení, motivy, úlohy, příkazy — bydlí v jádře a dá se
//! otestovat bez okna.
//!
//! Nejsou tu žádné konstanty ovlivňující vzhled ani chování. Všechno jde
//! z [`Settings`], protože co je zadrátované, to nejde nastavit — a co nejde
//! nastavit, to se jednou přepisuje.

mod grid;
mod theme;

use anyhow::Result;
use eframe::egui;
use photosite_core::commands::{Bindings, Group, Shortcut};
use photosite_core::settings::{Gallery, Kind, PRESETS, Settings, TUNABLES, Tunable};
use photosite_core::{Paths, commands, diagnostics, i18n, jobs, t, theme as palettes};
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
    let mut language: Option<String> = None;
    let mut shot: Option<PathBuf> = None;
    let mut reset = false;
    let mut open_settings = false;
    let mut folder: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "--verbose" | "-v" => verbose = true,
            "--selftest" => selftest = true,
            "--lang" => language = args.next(),
            "--shot" => shot = args.next().map(PathBuf::from),
            "--reset-settings" => reset = true,
            "--open-settings" => open_settings = true,
            other => folder = Some(PathBuf::from(other)),
        }
    }

    let paths = Paths::resolve(data.as_deref())?;
    paths.ensure()?;
    let _logging = diagnostics::start(&paths, verbose);
    diagnostics::install_panic_hook(&paths);
    tracing::info!(verze = photosite_core::VERSION, "start");

    let mut settings = Settings::load(&paths);
    if reset {
        settings.reset_all();
        settings.save(&paths)?;
        tracing::info!("nastavení vráceno na výchozí");
    }

    i18n::set_language(&i18n::negotiate(
        language.as_deref().or(Some(&settings.appearance.language)),
    ));

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([settings.window.width as f32, settings.window.height as f32])
        .with_min_inner_size([900.0, 600.0])
        .with_maximized(settings.window.maximized)
        .with_title("PhotoSite");
    if let (Some(x), Some(y)) = (settings.window.x, settings.window.y) {
        viewport = viewport.with_position([x as f32, y as f32]);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "PhotoSite",
        options,
        Box::new(move |cc| {
            let mut app = App::new(paths, settings, folder);
            app.selftest = selftest;
            app.shot = shot;
            app.show_settings = open_settings;
            app.dress(&cc.egui_ctx);
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

pub struct App {
    paths: Paths,
    pub settings: Settings,
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

    /// Motiv, který se právě kreslí. Přepočítá se, když se změní nastavení
    /// nebo když systém přepne mezi světlým a tmavým režimem.
    theme: &'static palettes::Theme,
    status: String,
    show_diagnostics: bool,
    show_settings: bool,

    pub selftest: bool,
    pub shot: Option<PathBuf>,
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
    fn new(paths: Paths, settings: Settings, folder: Option<PathBuf>) -> Self {
        let threads = match settings.loading.worker_threads {
            0 => jobs::worker_count(),
            count => count.clamp(1, 128) as usize,
        };
        let thumb = settings.loading.thumb_size.clamp(32, 4096) as u32;
        let preview = settings.loading.preview_size.clamp(64, 16384) as u32;
        let embedded = settings.loading.use_embedded_thumbnails;

        let images = jobs::Wishlist::new(threads, move |key: &Key| {
            let (path, want) = key;
            let outcome = match want {
                Want::Quick if !embedded => Ok(None),
                Want::Quick => img::quick(path).map(|found| found.map(into_pixels)),
                Want::Thumb => img::sized(path, thumb).map(|rgb| Some(into_pixels(rgb))),
                Want::Preview => img::sized(path, preview).map(|rgb| Some(into_pixels(rgb))),
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

        let theme = palettes::resolve(&settings.appearance, None);
        let mut app = Self {
            paths,
            settings,
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
            theme,
            status: i18n::t("gallery-pick-folder"),
            show_diagnostics: false,
            show_settings: false,
            selftest: false,
            shot: None,
            frames: 0,
            started: std::time::Instant::now(),
        };

        let start = folder.or_else(|| app.settings.gallery.last_folder.as_ref().map(PathBuf::from));
        if let Some(folder) = start.filter(|folder| folder.is_dir()) {
            app.open(folder);
        }

        app
    }

    /// Přepočítá motiv a prožene ho skrz egui. Volá se při startu a po každé
    /// změně, která se vzhledu týká.
    fn dress(&mut self, ctx: &egui::Context) {
        let system_dark = ctx.system_theme().map(|theme| theme == egui::Theme::Dark);
        self.theme = palettes::resolve(&self.settings.appearance, system_dark);
        theme::apply(
            ctx,
            &self.theme.palette,
            self.theme.dark,
            self.settings.appearance.ui_scale as f32,
        );
    }

    pub fn palette(&self) -> &'static palettes::Palette {
        &self.theme.palette
    }

    pub fn gallery(&self) -> &Gallery {
        &self.settings.gallery
    }

    pub fn open(&mut self, folder: PathBuf) {
        let started = std::time::Instant::now();
        let depth = if self.settings.gallery.recursive {
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
        self.status = t!(
            "gallery-count",
            count = photos.len() as i64,
            ms = started.elapsed().as_secs_f64() * 1000.0
        );
        tracing::info!(folder = %folder.display(), pocet = photos.len(), "složka otevřena");
        self.photos = photos;
        self.selected = None;
        self.settings.gallery.last_folder = Some(folder.to_string_lossy().into_owned());
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
        if let Some(at) = self.order.iter().position(|existing| existing == key) {
            let key = self.order.remove(at);
            self.order.push(key);
        }
    }

    fn collect(&mut self, ctx: &egui::Context) {
        let uploads = self.settings.loading.uploads_per_frame.clamp(1, 4096) as usize;
        for (key, pixels) in self.images.drain(uploads) {
            let image = egui::ColorImage::from_rgb(pixels.size, &pixels.rgb);
            let handle =
                ctx.load_texture(key.0.to_string_lossy(), image, egui::TextureOptions::LINEAR);
            self.order.retain(|existing| existing != &key);
            self.order.push(key.clone());
            self.textures.insert(key, handle);
        }

        let budget = self.settings.loading.texture_budget.clamp(16, 65_536) as usize;
        while self.order.len() > budget {
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
                self.settings.gallery.recursive = !self.settings.gallery.recursive;
                if let Some(folder) = self.folder.clone() {
                    self.open(folder);
                }
            }
            "view.bigger_tiles" => self.resize_tiles(1.25),
            "view.smaller_tiles" => self.resize_tiles(1.0 / 1.25),
            "view.next_theme" => {
                let at = palettes::THEMES
                    .iter()
                    .position(|theme| theme.id == self.theme.id)
                    .unwrap_or(0);
                let next = &palettes::THEMES[(at + 1) % palettes::THEMES.len()];
                self.settings.appearance.theme = next.id.to_owned();
                self.dress(ctx);
            }
            "view.settings" => self.show_settings = !self.show_settings,
            "help.diagnostics" => self.show_diagnostics = !self.show_diagnostics,
            // Otevírací dialog přijde s `rfd`; do té doby se složka vybírá
            // ve stromu vlevo.
            "file.open_folder" => self.status = t!("gallery-pick-folder"),
            other => tracing::warn!(prikaz = other, "příkaz bez obsluhy"),
        }
    }

    /// Meze bere z popisu polí, ne z čísel napsaných tady.
    fn resize_tiles(&mut self, factor: f64) {
        let (min, max) = match TUNABLES
            .iter()
            .find(|tunable| tunable.path == "gallery.tile_size")
            .map(|tunable| tunable.kind)
        {
            Some(Kind::Float { min, max }) => (min, max),
            _ => (80.0, 640.0),
        };
        self.settings.gallery.tile_size =
            (self.settings.gallery.tile_size * factor).clamp(min, max);
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

fn into_pixels(rgb: img::Rgb) -> Pixels {
    Pixels {
        size: [rgb.width as usize, rgb.height as usize],
        rgb: rgb.pixels,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.collect(&ctx);
        self.shortcuts(&ctx);

        // Systém mohl mezitím přepnout na tmavý režim.
        if self.settings.appearance.theme == palettes::AUTOMATIC {
            let system_dark = ctx.system_theme().map(|theme| theme == egui::Theme::Dark);
            if palettes::resolve(&self.settings.appearance, system_dark).id != self.theme.id {
                self.dress(&ctx);
            }
        }

        let palette = *self.palette();
        let panel = theme::color(palette.panel);
        let window = theme::color(palette.window);

        egui::Panel::top("toolbar")
            .frame(egui::Frame::NONE.fill(panel).inner_margin(6.0))
            .show(ui, |ui| self.toolbar(ui, &palette, &ctx));

        egui::Panel::left("tree")
            .resizable(true)
            .default_size(self.settings.window.tree_width as f32)
            .frame(egui::Frame::NONE.fill(panel).inner_margin(6.0))
            .show(ui, |ui| {
                self.settings.window.tree_width = ui.available_width() as f64;
                grid::tree(self, ui, &palette);
            });

        egui::Panel::right("preview")
            .resizable(true)
            .default_size(self.settings.window.preview_width as f32)
            .frame(egui::Frame::NONE.fill(window))
            .show(ui, |ui| {
                self.settings.window.preview_width = ui.available_width() as f64;
                grid::preview(self, ui, &palette);
            });

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(window))
            .show(ui, |ui| grid::gallery(self, ui, &palette));

        self.diagnostics_window(&ctx);
        self.settings_window(&ctx);

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
            ctx.request_repaint_after(std::time::Duration::from_millis(
                self.settings.loading.idle_repaint_ms.clamp(1, 60_000) as u64,
            ));
        }

        // Stav okna se sbírá každý snímek, ukládá se až při zavření.
        ctx.input(|input| {
            if let Some(rect) = input.viewport().inner_rect {
                self.settings.window.width = rect.width() as f64;
                self.settings.window.height = rect.height() as f64;
            }

            if let Some(outer) = input.viewport().outer_rect {
                self.settings.window.x = Some(outer.min.x as f64);
                self.settings.window.y = Some(outer.min.y as f64);
            }

            self.settings.window.maximized = input.viewport().maximized.unwrap_or(false);
        });

        self.frames += 1;
        if self.selftest {
            self.run_selftest(&ctx);
        }

        self.grab(&ctx);
    }
}

impl App {
    fn toolbar(&mut self, ui: &mut egui::Ui, palette: &palettes::Palette, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let mut recursive = self.settings.gallery.recursive;
            if ui
                .checkbox(&mut recursive, t!("toolbar-recursive"))
                .changed()
            {
                self.run("view.recursive", ctx);
            }

            ui.separator();
            // Tlačítka se berou z registru příkazů, ne z ručně psaného seznamu.
            for command in commands::COMMANDS
                .iter()
                .filter(|command| command.group == Group::View && command.id != "view.recursive")
            {
                let title = command.title();
                let hint = match self.bindings.shortcut(command.id) {
                    Some(shortcut) => format!("{title}  ({shortcut})"),
                    None => title.clone(),
                };
                if ui.button(title).on_hover_text(hint).clicked() {
                    self.run(command.id, ctx);
                }
            }

            ui.separator();
            ui.label(
                egui::RichText::new(
                    self.folder
                        .as_ref()
                        .map(|folder| folder.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
                .color(theme::color(palette.dim)),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.status).color(theme::color(palette.dim)));
                for task in self.tasks.running() {
                    ui.label(egui::RichText::new(&task.title).color(theme::color(palette.accent)));
                }
            });
        });
    }

    fn diagnostics_window(&mut self, ctx: &egui::Context) {
        if !self.show_diagnostics {
            return;
        }

        let mut rows = diagnostics::about(&self.paths);
        rows.push((t!("diagnostics-photos"), self.photos.len().to_string()));
        rows.push((t!("diagnostics-textures"), self.textures.len().to_string()));
        rows.push((
            t!("diagnostics-decoding"),
            self.images.running().to_string(),
        ));
        rows.push((
            t!("diagnostics-tasks"),
            self.tasks.running().len().to_string(),
        ));
        rows.push((t!("diagnostics-blank"), self.blank.to_string()));
        rows.push((t!("diagnostics-unsharp"), self.unsharp.to_string()));
        let width = rows
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or(0);

        let mut open = true;
        egui::Window::new(t!("diagnostics-title"))
            .open(&mut open)
            .show(ctx, |ui| {
                for (label, value) in &rows {
                    ui.monospace(format!("{label:width$}  {value}"));
                }
            });
        self.show_diagnostics = open;
    }

    /// Obrazovka nastavení se skládá z popisu polí, ne z ručně psaných
    /// ovládacích prvků. Přidat volbu znamená přidat řádek do `TUNABLES`.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let mut open = true;
        let mut changed = false;
        let mut redress = false;
        egui::Window::new(t!("settings-title"))
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(t!("settings-presets"));
                    for preset in PRESETS {
                        if ui.button(i18n::t(preset.label_key)).clicked() {
                            if let Err(error) = self.settings.apply_preset(preset.id) {
                                tracing::error!(error = %format!("{error:#}"), "sada selhala");
                            }

                            changed = true;
                        }
                    }
                });

                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(440.0)
                    .show(ui, |ui| {
                        for tunable in TUNABLES {
                            // Stav okna není předvolba, do nastavení nepatří.
                            if tunable.kind == Kind::State {
                                continue;
                            }

                            if self.tunable_row(ui, tunable) {
                                changed = true;
                                redress |= tunable.path.starts_with("appearance.");
                            }
                        }
                    });

                ui.separator();
                if ui.button(t!("settings-reset")).clicked() {
                    self.settings.reset_all();
                    changed = true;
                    redress = true;
                }
            });

        self.show_settings = open;
        if redress {
            self.dress(ctx);
        }

        if changed && let Err(error) = self.settings.save(&self.paths) {
            tracing::error!(error = %format!("{error:#}"), "nastavení se nepodařilo uložit");
        }
    }

    fn tunable_row(&mut self, ui: &mut egui::Ui, tunable: &Tunable) -> bool {
        let label = i18n::t(tunable.label_key);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match tunable.kind {
                    Kind::Bool => {
                        let mut value = self
                            .settings
                            .get(tunable.path)
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false);
                        if ui.checkbox(&mut value, "").changed() {
                            changed = self.write(tunable.path, toml::Value::Boolean(value));
                        }
                    }
                    Kind::Float { min, max } => {
                        let mut value = self
                            .settings
                            .get(tunable.path)
                            .and_then(|value| value.as_float())
                            .unwrap_or(0.0);
                        if ui.add(egui::Slider::new(&mut value, min..=max)).changed() {
                            changed = self.write(tunable.path, toml::Value::Float(value));
                        }
                    }
                    Kind::Int { min, max } => {
                        let mut value = self
                            .settings
                            .get(tunable.path)
                            .and_then(|value| value.as_integer())
                            .unwrap_or(0);
                        if ui.add(egui::Slider::new(&mut value, min..=max)).changed() {
                            changed = self.write(tunable.path, toml::Value::Integer(value));
                        }
                    }
                    Kind::Text | Kind::Choice(_) => {
                        let mut value = self
                            .settings
                            .get(tunable.path)
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .unwrap_or_default();
                        if ui.text_edit_singleline(&mut value).changed() {
                            changed = self.write(tunable.path, toml::Value::String(value));
                        }
                    }
                    Kind::State => {}
                },
            );
        });

        changed
    }

    fn write(&mut self, path: &str, value: toml::Value) -> bool {
        match self.settings.set(path, value) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(path, error = %format!("{error:#}"), "hodnotu nelze nastavit");
                false
            }
        }
    }

    /// Vyfotí okno, jakmile jsou dlaždice na místě, a skončí.
    fn grab(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else {
            return;
        };

        if self.frames == 60 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }

        let captured = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = captured {
            let rgba: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|color| color.to_array())
                .collect();
            match write_png(&path, &rgba, image.size[0] as u32, image.size[1] as u32) {
                Ok(()) => tracing::info!(path = %path.display(), "snímek uložen"),
                Err(error) => {
                    tracing::error!(error = %format!("{error:#}"), "snímek se nepodařilo uložit")
                }
            }

            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Samokontrola: otevři složku, chvíli běž a ověř, že žádná viditelná
    /// dlaždice nezůstala prázdná. Přesně tuhle vadu měl WPF benchmark, kde
    /// jedna spolknutá výjimka způsobila, že se nevyrobil jediný náhled a
    /// aplikace se přitom tvářila, že běží.
    fn run_selftest(&mut self, ctx: &egui::Context) {
        let elapsed = self.started.elapsed().as_secs_f64();
        if self.frames < 10 {
            return;
        }

        if self.photos.is_empty() {
            println!("{}", t!("selftest-no-photos"));
            std::process::exit(2);
        }

        if self.blank == 0 {
            println!(
                "{}",
                t!(
                    "selftest-ok",
                    count = self.photos.len() as i64,
                    ms = elapsed * 1000.0
                )
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if elapsed > 20.0 {
            println!(
                "{}",
                t!(
                    "selftest-blank",
                    count = self.blank as i64,
                    seconds = elapsed
                )
            );
            std::process::exit(3);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(error) = self.settings.save(&self.paths) {
            tracing::error!(error = %format!("{error:#}"), "nastavení se nepodařilo uložit");
        }
    }
}

/// Uloží RGBA jako PNG. Vlastní zápis, aby si UI kvůli jednomu ladicímu
/// přepínači netáhlo celý kodekový balík.
fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
    use std::io::Write as _;

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut crc = 0xFFFF_FFFFu32;
        for byte in kind.iter().chain(body) {
            crc ^= *byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }

        out.extend_from_slice(&(!crc).to_be_bytes());
    }

    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);

    // Nekomprimované deflate bloky: kodek tu nepotřebujeme.
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in 0..height as usize {
        raw.push(0);
        let from = row * width as usize * 4;
        raw.extend_from_slice(&rgba[from..from + width as usize * 4]);
    }

    let mut zlib = vec![0x78, 0x01];
    for (index, block) in raw.chunks(65_535).enumerate() {
        let last = (index + 1) * 65_535 >= raw.len();
        zlib.push(if last { 1 } else { 0 });
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }

    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + *byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }

    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    std::fs::File::create(path)?.write_all(&png)?;
    Ok(())
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
