//! PhotoSite.
//!
//! This crate is the only one that knows about egui and about the GPU.
//! Everything else — the catalogue, paths, settings, themes, tasks, commands
//! — lives in the core and can be tested without a window.
//!
//! There are no constants here affecting looks or behaviour. It all comes
//! from [`Settings`], because what is hard-wired cannot be configured — and
//! what cannot be configured gets rewritten sooner or later.

mod docks;
mod grid;
mod info;
mod picker;
mod theme;

use anyhow::Result;
use eframe::egui;
use photosite_core::catalog::NewPhoto;
use photosite_core::commands::{Bindings, Group, Shortcut};
use photosite_core::domain::{ColorLabel, Flag, Photo, PhotoId, Sort, SortField};
use photosite_core::settings::{Gallery, Kind, Settings, TUNABLES, Tunable};
use photosite_core::{
    Catalog, FileIdentity, Paths, commands, diagnostics, docks as layout, i18n, jobs, t,
    theme as palettes,
};
use photosite_image as img;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// What is wanted from an image. The order is also the priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Want {
    /// The EXIF thumbnail: blurry, but immediate.
    Quick,
    Thumb,
    Preview,
}

pub type Key = (PathBuf, Want);

/// A finished image waiting to be uploaded to the GPU.
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
        tracing::info!("settings returned to their defaults");
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
    .map_err(|error| anyhow::anyhow!("the window could not be opened: {error}"))
}

/// A node of the folder tree. Children are loaded only on expansion.
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

    /// The catalogue.
    ///
    /// `None` when it could not be opened. The application still browses —
    /// it simply cannot remember anything, and the log says so rather than a
    /// rating vanishing without a word.
    catalog: Option<Catalog>,

    pub roots: Vec<Node>,
    pub folder: Option<PathBuf>,
    pub photos: Vec<Photo>,
    /// The tile the preview and the details follow, and the end a shift-click
    /// measures from. Always one of [`App::selection`] when there is one.
    pub selected: Option<usize>,
    /// Every tile the next rating lands on.
    ///
    /// Positions and not paths, because the grid works in positions and the
    /// set is rebuilt whenever the order changes — see [`App::relist`].
    pub selection: BTreeSet<usize>,
    /// The background pass reading this folder's headers, while one runs.
    indexing: Option<u64>,

    textures: HashMap<Key, egui::TextureHandle>,
    order: Vec<Key>,
    pub wanted_quick: Vec<PathBuf>,
    pub wanted_sharp: Vec<PathBuf>,
    pub wanted_preview: Option<PathBuf>,
    pub blank: usize,
    pub unsharp: usize,
    /// How many textures the grid currently wishes to keep. The cache
    /// ceiling must not fall below this number, or something is evicted every
    /// frame and decoded again at once.
    pub needed: usize,

    /// The theme currently being drawn. Recomputed when the settings change
    /// or when the system switches between light and dark mode.
    theme: &'static palettes::Theme,
    status: String,
    show_diagnostics: bool,
    show_settings: bool,
    /// The dock layout, read from the settings. Written back as soon as
    /// somebody moves a splitter.
    pub layout: layout::Layout,
    /// Which panes are hidden.
    pub hidden: Vec<String>,
    /// Which photograph the prepared detail rows belong to.
    pub info_of: Option<PathBuf>,
    pub info_rows: Vec<(String, String)>,
    /// The text fields of the details pane, and whose they are.
    ///
    /// They are held rather than read from the row every frame, because a
    /// field has to keep what is being typed into it — and because writing
    /// to the catalogue on every keystroke is a transaction per character.
    /// The write happens when the field is left.
    pub edit_of: Option<PathBuf>,
    pub edit_title: String,
    pub edit_description: String,
    pub edit_keywords: String,
    /// The open folder dialog, at most one.
    folder_dialog: Option<picker::Picker>,
    /// The folder the tree on the left should scroll to. Set on opening, and
    /// taken by the tree straight away.
    pub scroll_tree_to: Option<PathBuf>,

    /// How the decoding threads can ask for a repaint. Without it the UI
    /// would have to check regularly whether anything had arrived, which
    /// means waking over and over even when nothing is happening.
    waker: Arc<OnceLock<egui::Context>>,

    pub selftest: bool,
    pub shot: Option<PathBuf>,
    frames: u32,
    started: std::time::Instant,
}

// TextureHandle does not implement Debug, so we write our own — and write it
// to print what is actually interesting while debugging.
impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("folder", &self.folder)
            .field("photos", &self.photos.len())
            .field("textures", &self.textures.len())
            .field("blank", &self.blank)
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
        let waker: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
        let wake = waker.clone();

        let images = jobs::Wishlist::new(threads, move |key: &Key| {
            let (path, want) = key;
            let outcome = match want {
                Want::Quick if !embedded => Ok(None),
                Want::Quick => img::quick(path).map(|found| found.map(into_pixels)),
                Want::Thumb => img::sized(path, thumb).map(|rgb| Some(into_pixels(rgb))),
                Want::Preview => img::sized(path, preview).map(|rgb| Some(into_pixels(rgb))),
            };

            let outcome = match outcome {
                Ok(pixels) => pixels,
                Err(error) => {
                    // In a library of tens of thousands, an unreadable file
                    // is ordinary. A silent one is not.
                    tracing::warn!(path = %path.display(), ?want, error = %format!("{error:#}"), "the image cannot be loaded");
                    None
                }
            };

            // Wake the UI so it collects the result. Otherwise it would have
            // to keep looking, and that costs processor even at rest.
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }

            outcome
        });

        // A catalogue that will not open must not take the application with
        // it: browsing still works, only nothing is remembered.
        let catalog = match Catalog::open(&paths.catalog()) {
            Ok(catalog) => Some(catalog),
            Err(error) => {
                tracing::error!(
                    error = %format!("{error:#}"),
                    "the catalogue could not be opened; nothing will be remembered"
                );
                None
            }
        };

        let theme = palettes::resolve(&settings.appearance, None);
        let settings_layout = settings.window.layout.clone();
        let settings_hidden = settings.window.docks_hidden.clone();
        let mut app = Self {
            paths,
            settings,
            bindings: Bindings::defaults(),
            tasks: jobs::Tasks::new(),
            images,
            catalog,
            roots: roots(),
            folder: None,
            photos: Vec::new(),
            selected: None,
            selection: BTreeSet::new(),
            indexing: None,
            textures: HashMap::new(),
            order: Vec::new(),
            wanted_quick: Vec::new(),
            wanted_sharp: Vec::new(),
            wanted_preview: None,
            blank: 0,
            unsharp: 0,
            needed: 0,
            theme,
            waker,
            status: i18n::t("gallery-pick-folder"),
            show_diagnostics: false,
            show_settings: false,
            layout: layout::parse_or_default(&settings_layout),
            hidden: layout::hidden(&settings_hidden),
            info_of: None,
            info_rows: Vec::new(),
            edit_of: None,
            edit_title: String::new(),
            edit_description: String::new(),
            edit_keywords: String::new(),
            folder_dialog: None,
            scroll_tree_to: None,
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

    /// Recomputes the theme and runs it through egui. Called at startup and
    /// after every change that concerns the look.
    fn dress(&mut self, ctx: &egui::Context) {
        // The decoding threads need the context so they can ask for a
        // repaint.
        let _ = self.waker.set(ctx.clone());
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
        let files = walk(&folder, self.settings.gallery.recursive);

        // A row for every file before a single one is opened. It costs one
        // transaction, and it is what makes the folder ratable from the first
        // frame — a rating needs somewhere to go, and waiting for seven
        // thousand headers to be read first is not somewhere.
        if let Some(catalog) = self.catalog.as_mut()
            && let Err(error) = catalog.upsert_identities(&files)
        {
            tracing::error!(
                error = %format!("{error:#}"),
                "the folder could not be written to the catalogue"
            );
        }

        let count = files.len();
        self.folder = Some(folder.clone());
        self.selected = None;
        self.selection.clear();
        self.relist();
        self.start_indexing();

        self.status = t!(
            "gallery-count",
            count = count as i64,
            ms = started.elapsed().as_secs_f64() * 1000.0
        );
        tracing::info!(folder = %folder.display(), count, "folder opened");
        self.settings.gallery.last_folder = Some(folder.to_string_lossy().into_owned());

        // The tree on the left follows the gallery, however somebody got to
        // the folder. Without this, seven thousand photographs glow in the
        // grid while the tree still shows collapsed roots — the highlighted
        // folder is somewhere inside and cannot be seen.
        let mut roots = std::mem::take(&mut self.roots);
        reveal(&mut roots, &folder);
        self.roots = roots;
        self.scroll_tree_to = Some(folder);
    }

    /// How the gallery is ordered right now.
    fn sort(&self) -> Sort {
        Sort::new(
            SortField::from_id(&self.settings.gallery.sort_field),
            self.settings.gallery.sort_descending,
        )
    }

    /// Takes the folder from the catalogue and puts it in the chosen order.
    ///
    /// Called on opening, when the order changes, and when the background
    /// pass has learned something. **The selection follows the photographs
    /// and not the positions** — reordering a folder with something selected
    /// would otherwise leave the next rating landing on whatever slid into
    /// that slot, which is the kind of mistake nobody notices until later.
    fn relist(&mut self) {
        let Some(folder) = self.folder.clone() else {
            self.photos.clear();
            return;
        };

        let held: HashSet<PathBuf> = self
            .selection
            .iter()
            .filter_map(|at| self.photos.get(*at))
            .map(|photo| photo.path.clone())
            .collect();
        let current = self
            .selected
            .and_then(|at| self.photos.get(at))
            .map(|photo| photo.path.clone());

        let mut photos = match self.catalog.as_ref() {
            Some(catalog) => catalog
                .in_folder(&folder, self.settings.gallery.recursive)
                .unwrap_or_else(|error| {
                    tracing::error!(
                        error = %format!("{error:#}"),
                        "the folder could not be read from the catalogue"
                    );
                    Vec::new()
                }),
            None => Vec::new(),
        };
        self.sort().apply(&mut photos);
        self.photos = photos;

        self.selection = self
            .photos
            .iter()
            .enumerate()
            .filter(|(_, photo)| held.contains(&photo.path))
            .map(|(at, _)| at)
            .collect();
        self.selected = current
            .and_then(|path| self.photos.iter().position(|photo| photo.path == path))
            .or_else(|| self.selection.iter().next().copied());
    }

    /// Reads the headers of whatever this folder has not given up yet, on a
    /// thread of its own.
    ///
    /// It must not happen on the way to the first frame. Seven thousand file
    /// opens is a third of a second on a warm disk and many seconds on a
    /// network share, and Windows calls a window that quiet unresponsive.
    fn start_indexing(&mut self) {
        if self.indexing.is_some() {
            return;
        }

        let (Some(folder), Some(catalog)) = (self.folder.clone(), self.catalog.as_ref()) else {
            return;
        };

        let waiting = match catalog.unindexed(&folder, self.settings.gallery.recursive) {
            Ok(waiting) => waiting,
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "cannot tell what is left to read");
                return;
            }
        };
        if waiting.is_empty() {
            return;
        }

        // The task opens its own connection. A catalogue in WAL mode takes
        // more than one, and handing this one across would lock the UI out
        // of it for the whole pass.
        let path = self.paths.catalog();
        let total = waiting.len() as u64;
        let waker = self.waker.clone();
        let title = t!("task-reading-folder");
        let message = t!("task-reading-headers");
        self.indexing = Some(self.tasks.spawn(title, move |cancel, progress| {
            let mut catalog = Catalog::open(&path)?;
            let mut done = 0u64;
            for chunk in waiting.chunks(500) {
                if cancel.cancelled() {
                    break;
                }

                let batch: Vec<NewPhoto> =
                    chunk.iter().filter_map(|path| read_header(path)).collect();
                catalog.upsert_many(&batch)?;
                done += chunk.len() as u64;
                progress.report(done, Some(total), message.clone());

                // Show the dates as they arrive rather than at the end. Over
                // a large folder that is the difference between a gallery
                // filling in and one that sits still and then jumps.
                if let Some(ctx) = waker.get() {
                    ctx.request_repaint();
                }
            }

            Ok(())
        }));
    }

    /// Notices that the background pass has finished and takes the rows
    /// again. Polled every frame, which costs one lock and nothing else.
    fn collect_indexing(&mut self) {
        let Some(id) = self.indexing else {
            return;
        };

        let finished = self
            .tasks
            .snapshot()
            .into_iter()
            .any(|task| task.id == id && task.finished);
        if finished {
            self.indexing = None;
            self.relist();
            self.tasks.forget_finished();
        }
    }

    /// Asks for a folder with the native dialog.
    fn ask_for_folder(&mut self, ctx: &egui::Context) {
        if self.folder_dialog.is_some() {
            return;
        }

        let start = picker::start_dir(
            self.folder.as_deref(),
            self.settings.gallery.last_folder.as_deref(),
        );
        self.folder_dialog = Some(picker::ask(ctx, t!("dialog-pick-folder"), start));
    }

    /// Collects whatever the dialog returned. A cancelled dialog is reported
    /// nowhere — closing it is an answer like any other, not a failure.
    fn take_picked_folder(&mut self) {
        let Some(dialog) = &self.folder_dialog else {
            return;
        };

        match dialog.answer() {
            picker::Answer::Waiting => {}
            picker::Answer::Cancelled => self.folder_dialog = None,
            picker::Answer::Picked(folder) => {
                self.folder_dialog = None;
                self.open(folder);
            }
        }
    }

    pub fn texture(&self, key: &Key) -> Option<&egui::TextureHandle> {
        self.textures.get(key)
    }

    pub fn has(&self, path: &Path, want: Want) -> bool {
        self.textures.contains_key(&(path.to_path_buf(), want))
    }

    /// Marks a texture as just used, so the LRU does not evict it from under
    /// our hands.
    pub fn touch(&mut self, key: &Key) {
        if let Some(at) = self.order.iter().position(|existing| existing == key) {
            let key = self.order.remove(at);
            self.order.push(key);
        }
    }

    /// Returns how many images arrived — that is how we know whether
    /// anything is still happening or repainting can stop.
    fn collect(&mut self, ctx: &egui::Context) -> usize {
        let uploads = self.settings.loading.uploads_per_frame.clamp(1, 4096) as usize;
        let delivered = self.images.drain(uploads);
        let count = delivered.len();
        for (key, pixels) in delivered {
            let image = egui::ColorImage::from_rgb(pixels.size, &pixels.rgb);
            let handle =
                ctx.load_texture(key.0.to_string_lossy(), image, egui::TextureOptions::LINEAR);
            self.order.retain(|existing| existing != &key);
            self.order.push(key.clone());

            // Once the sharp version is in place, the quick one from EXIF is
            // no use. Keeping both doubles the pressure on the cache for
            // nothing.
            if key.1 == Want::Thumb {
                let quick = (key.0.clone(), Want::Quick);
                self.textures.remove(&quick);
                self.order.retain(|existing| existing != &quick);
            }

            self.textures.insert(key, handle);
        }

        let budget = effective_budget(self.settings.loading.texture_budget, self.needed);
        while self.order.len() > budget {
            let oldest = self.order.remove(0);
            self.textures.remove(&oldest);
            // An evicted texture will have to be made again.
            self.images.forget(&oldest);
        }

        count
    }

    /// Runs a command that has no business with the window.
    ///
    /// The tests drive the commands the keyboard drives, and every one of
    /// them they reach — the ratings, the labels, the verdict, the selection
    /// — needs no context. Quitting and the folder dialog do, and are not
    /// among them.
    #[cfg(test)]
    fn run_for_test(&mut self, id: &str) {
        let ctx = egui::Context::default();
        self.run(id, &ctx);
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
            "view.toggle_tree" => self.toggle_dock("tree"),
            "view.toggle_preview" => self.toggle_dock("preview"),
            "view.toggle_info" => self.toggle_dock("info"),
            "view.reset_layout" => {
                self.settings.window.layout = layout::DEFAULT.to_owned();
                self.settings.window.docks_hidden = String::new();
                self.layout = layout::parse_or_default(layout::DEFAULT);
                self.hidden.clear();
            }
            "photo.rate_0" => self.rate(0),
            "photo.rate_1" => self.rate(1),
            "photo.rate_2" => self.rate(2),
            "photo.rate_3" => self.rate(3),
            "photo.rate_4" => self.rate(4),
            "photo.rate_5" => self.rate(5),
            "photo.label_none" => self.label(ColorLabel::None),
            "photo.label_red" => self.label(ColorLabel::Red),
            "photo.label_yellow" => self.label(ColorLabel::Yellow),
            "photo.label_green" => self.label(ColorLabel::Green),
            "photo.label_blue" => self.label(ColorLabel::Blue),
            "photo.label_purple" => self.label(ColorLabel::Purple),
            "photo.pick" => self.flag(Flag::Picked),
            "photo.reject" => self.flag(Flag::Rejected),
            "photo.select_all" => self.select_all(),
            "sort.taken" => self.sort_by(SortField::TakenAt),
            "sort.name" => self.sort_by(SortField::Name),
            "sort.rating" => self.sort_by(SortField::Rating),
            "sort.modified" => self.sort_by(SortField::ModifiedAt),
            "sort.size" => self.sort_by(SortField::FileSize),
            "sort.dimensions" => self.sort_by(SortField::Dimensions),
            "sort.reverse" => {
                self.settings.gallery.sort_descending = !self.settings.gallery.sort_descending;
                self.relist();
            }
            "help.diagnostics" => self.show_diagnostics = !self.show_diagnostics,
            "file.open_folder" => self.ask_for_folder(ctx),
            other => tracing::warn!(command = other, "command with no handler"),
        }
    }

    /// The photographs the next rating lands on.
    fn chosen(&self) -> Vec<PhotoId> {
        self.selection
            .iter()
            .filter_map(|at| self.photos.get(*at))
            .map(|photo| photo.id)
            .collect()
    }

    pub fn is_selected(&self, at: usize) -> bool {
        self.selection.contains(&at)
    }

    /// A plain click: this one, and nothing else.
    pub fn select_only(&mut self, at: usize) {
        self.selection.clear();
        self.selection.insert(at);
        self.selected = Some(at);
    }

    /// Ctrl-click: add it or take it away, and leave the rest alone.
    pub fn select_also(&mut self, at: usize) {
        if !self.selection.remove(&at) {
            self.selection.insert(at);
            self.selected = Some(at);
        } else if self.selected == Some(at) {
            // The tile the preview follows was just taken out of the
            // selection, so the preview follows something still in it rather
            // than going blank.
            self.selected = self.selection.iter().next().copied();
        }
    }

    /// Shift-click: everything from the current tile to this one.
    pub fn select_through(&mut self, at: usize) {
        let from = self.selected.unwrap_or(at);
        let (first, last) = if from <= at { (from, at) } else { (at, from) };
        self.selection = (first..=last)
            .filter(|at| *at < self.photos.len())
            .collect();
        self.selected = Some(at);
    }

    fn select_all(&mut self) {
        self.selection = (0..self.photos.len()).collect();
        if self.selected.is_none() {
            self.selected = self.selection.iter().next().copied();
        }
    }

    /// Writes through to the catalogue, and says so when it cannot.
    ///
    /// A change that reached the screen and not the disk is the worst of the
    /// three possible outcomes, so it is never allowed to pass in silence.
    fn write_catalog<F>(&mut self, what: F)
    where
        F: FnOnce(&mut Catalog) -> anyhow::Result<()>,
    {
        let Some(catalog) = self.catalog.as_mut() else {
            tracing::error!("there is no catalogue; the change is on screen only");
            return;
        };

        if let Err(error) = what(catalog) {
            tracing::error!(error = %format!("{error:#}"), "the change could not be written");
        }
    }

    /// The stars, on everything selected.
    ///
    /// The rows on screen are changed too rather than being read back. The
    /// order is deliberately **not** redone: rating while sorted by rating
    /// would slide the tile out from under the hand that just rated it, and
    /// culling a folder is done by holding the keys down.
    fn rate(&mut self, stars: u8) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        self.write_catalog(|catalog| catalog.set_rating(&chosen, stars));
        let stars = stars.min(photosite_core::domain::Organisation::MAX_RATING);
        for at in &self.selection {
            if let Some(photo) = self.photos.get_mut(*at) {
                photo.organisation.rating = stars;
            }
        }
    }

    fn label(&mut self, label: ColorLabel) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        self.write_catalog(|catalog| catalog.set_label(&chosen, label));
        for at in &self.selection {
            if let Some(photo) = self.photos.get_mut(*at) {
                photo.organisation.label = label;
            }
        }
    }

    /// The culling verdict, which toggles.
    ///
    /// Pressing P on something already picked takes the pick off. Without
    /// that there is no way back to undecided from the keyboard, and culling
    /// is done with one hand on the keys.
    fn flag(&mut self, flag: Flag) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        let already = self
            .selection
            .iter()
            .filter_map(|at| self.photos.get(*at))
            .all(|photo| photo.organisation.flag == flag);
        let wanted = if already { Flag::None } else { flag };

        self.write_catalog(|catalog| catalog.set_flag(&chosen, wanted));
        for at in &self.selection {
            if let Some(photo) = self.photos.get_mut(*at) {
                photo.organisation.flag = wanted;
            }
        }
    }

    /// Fills the text fields from the photograph, when it is a different one.
    ///
    /// Comparing by path and not by position: the position changes whenever
    /// the order does, and reloading a field somebody is typing into would
    /// take the words back out from under them.
    pub fn load_edits(&mut self, photo: &Photo) {
        if self.edit_of.as_deref() == Some(photo.path.as_path()) {
            return;
        }

        self.edit_of = Some(photo.path.clone());
        self.edit_title = photo.organisation.title.clone().unwrap_or_default();
        self.edit_description = photo.organisation.description.clone().unwrap_or_default();
        self.edit_keywords = photo.organisation.keywords.join(", ");
    }

    /// Writes the title as it now stands. Called when the field is left, not
    /// while it is being typed in.
    pub fn commit_title(&mut self) {
        let Some(photo) = self.current() else { return };
        let text = self.edit_title.clone();
        let value = (!text.trim().is_empty()).then(|| text.trim().to_owned());
        self.write_catalog(|catalog| catalog.set_title(photo, value.as_deref()));
        if let Some(at) = self.selected
            && let Some(photo) = self.photos.get_mut(at)
        {
            photo.organisation.title = value;
        }
    }

    pub fn commit_description(&mut self) {
        let Some(photo) = self.current() else { return };
        let text = self.edit_description.clone();
        let value = (!text.trim().is_empty()).then(|| text.trim().to_owned());
        self.write_catalog(|catalog| catalog.set_description(photo, value.as_deref()));
        if let Some(at) = self.selected
            && let Some(photo) = self.photos.get_mut(at)
        {
            photo.organisation.description = value;
        }
    }

    /// The keywords, as a line of them separated by commas.
    ///
    /// A comma and not a space, because a keyword can be two words —
    /// "Prague Castle" is one thing, not two.
    pub fn commit_keywords(&mut self) {
        let Some(photo) = self.current() else { return };
        let words: Vec<String> = self
            .edit_keywords
            .split(',')
            .map(|word| word.trim().to_owned())
            .filter(|word| !word.is_empty())
            .collect();
        self.write_catalog(|catalog| catalog.set_keywords(photo, &words));

        // Read back what the catalogue made of it, so the field shows the
        // tidied list rather than what was typed.
        let tidied = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.keywords_of(photo).ok())
            .unwrap_or(words);
        self.edit_keywords = tidied.join(", ");
        if let Some(at) = self.selected
            && let Some(photo) = self.photos.get_mut(at)
        {
            photo.organisation.keywords = tidied;
        }
    }

    /// The photograph the details pane is showing.
    fn current(&self) -> Option<PhotoId> {
        self.selected
            .and_then(|at| self.photos.get(at))
            .map(|photo| photo.id)
    }

    /// The stars, set from the details pane rather than the keyboard.
    /// Clicking the star a photograph already has takes the rating off,
    /// which is the only way to reach nought with the mouse.
    pub fn rate_from_panel(&mut self, stars: u8) {
        let already = self
            .selected
            .and_then(|at| self.photos.get(at))
            .map(|photo| photo.organisation.rating == stars)
            .unwrap_or(false);
        self.rate(if already { 0 } else { stars });
    }

    pub fn label_from_panel(&mut self, label: ColorLabel) {
        self.label(label);
    }

    pub fn flag_from_panel(&mut self, flag: Flag) {
        self.flag(flag);
    }

    /// Reorders the gallery. Unlike a rating, this is meant to move things.
    fn sort_by(&mut self, field: SortField) {
        self.settings.gallery.sort_field = field.id().to_owned();
        self.relist();
    }

    /// Hides a pane or brings it back. Hidden panes go into the settings, so
    /// the application remembers what somebody has closed.
    fn toggle_dock(&mut self, id: &str) {
        match self.hidden.iter().position(|hidden| hidden == id) {
            Some(at) => {
                self.hidden.remove(at);
            }
            None => self.hidden.push(id.to_owned()),
        }

        self.settings.window.docks_hidden = layout::hidden_to_text(&self.hidden);
    }

    /// The limits come from the field descriptions, not from numbers written
    /// here.
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

    /// Shortcuts are read from the registry, not from hard-written
    /// conditions.
    ///
    /// **Nothing fires while a text field has the keys.** A bare `1` is a
    /// rating and a bare `P` is a pick — until somebody is typing a title,
    /// where they are a 1 and a P. Without this the keywords field cannot be
    /// filled in at all, which is the sort of thing that is noticed only
    /// once the feature is finished.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }

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
                tracing::debug!(command = command.id, "shortcut");
                self.run(command.id, ctx);
            }
        }
    }
}

/// The texture cache ceiling.
///
/// The configured value is a wish, not a law: it must not go below what is
/// on screen right now. A smaller ceiling does not mean "less memory" but an
/// endless round — every frame something is evicted, ordered again at once
/// and decoded again. This is exactly what burned three quarters of a core
/// with 80px tiles and a ceiling of 300.
fn effective_budget(configured: i64, needed: usize) -> usize {
    let configured = configured.clamp(16, 65_536) as usize;
    // A quarter on top, so the cache does not touch the ceiling on every
    // scroll.
    let floor = needed + needed / 4 + 16;
    if configured < floor {
        tracing::debug!(
            configured,
            floor,
            "cache ceiling raised to the size of the screen"
        );
    }

    configured.max(floor)
}

/// Every photograph in a folder, with what the directory listing already
/// says about it.
///
/// Nothing is opened here. `metadata` on a walked entry is answered out of
/// what reading the directory already returned, so this stays a walk rather
/// than seven thousand file opens on the way to the first frame.
fn walk(folder: &Path, recursive: bool) -> Vec<FileIdentity> {
    let mut files: Vec<FileIdentity> = walkdir::WalkDir::new(folder)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| photosite_core::is_photo(entry.path()))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let modified_at = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_secs() as i64)
                .unwrap_or(0);
            Some(FileIdentity {
                file_size: meta.len(),
                modified_at,
                path: entry.into_path(),
            })
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// What one file's header says, as a row for the catalogue.
fn read_header(path: &Path) -> Option<NewPhoto> {
    let identity = FileIdentity::read(path).ok()?;
    let meta = img::exif::read_file(path);
    Some(NewPhoto {
        path: identity.path,
        file_size: identity.file_size,
        modified_at: identity.modified_at,
        taken_at: meta.taken_at,
        width: meta.width,
        height: meta.height,
        orientation: meta.orientation,
    })
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
        let delivered = self.collect(&ctx);
        self.collect_indexing();
        self.take_picked_folder();
        self.shortcuts(&ctx);

        // The system may have switched to dark mode in the meantime.
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

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(window))
            .show(ui, |ui| docks::show(self, ui, &palette));

        self.diagnostics_window(&ctx);
        self.settings_window(&ctx);

        // The wishlist is overwritten only here, once it is clear what is
        // visible and what is selected. Anything not on it stops being
        // decoded.
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

        // Another frame straight away only when something really is
        // happening. The condition "something is still missing" was here
        // before and it was a trap: when a missing tile could not be filled,
        // the application span at full speed for nothing. The threads
        // announce finished work themselves, so the interval below is only a
        // safety net in case a wake-up were lost.
        if delivered > 0 {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                self.settings.loading.idle_repaint_ms.clamp(1, 60_000) as u64,
            ));
        }

        // The window state is gathered every frame and saved only on close.
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

            // One control for six commands. The registry still owns the
            // names and the keys — this only draws the choice, the same way
            // the checkbox above draws `view.recursive`.
            let mut field = SortField::from_id(&self.settings.gallery.sort_field);
            let chosen = field;
            egui::ComboBox::from_id_salt("sort")
                .selected_text(i18n::t(field.title_key()))
                .show_ui(ui, |ui| {
                    for option in SortField::ALL {
                        ui.selectable_value(&mut field, option, i18n::t(option.title_key()));
                    }
                });
            if field != chosen {
                self.sort_by(field);
            }

            let descending = self.settings.gallery.sort_descending;
            let way = if descending {
                t!("toolbar-sort-descending")
            } else {
                t!("toolbar-sort-ascending")
            };
            if ui.button(way).clicked() {
                self.run("sort.reverse", ctx);
            }

            // The buttons and their order come from the command registry.
            // Which command belongs on the toolbar it says itself — the
            // drawing layer must not ask by name, or it gets rewritten with
            // every command added after it.
            let mut previous: Option<Group> = None;
            for command in commands::COMMANDS.iter().filter(|command| command.toolbar) {
                if previous != Some(command.group) {
                    ui.separator();
                }

                previous = Some(command.group);
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
                if self.selection.len() > 1 {
                    ui.label(
                        egui::RichText::new(t!(
                            "gallery-selected",
                            count = self.selection.len() as i64
                        ))
                        .color(theme::color(palette.accent)),
                    );
                }

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
        rows.push((t!("diagnostics-selected"), self.selection.len().to_string()));
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

    /// The settings screen is assembled from the field descriptions, not
    /// from hand-written controls. Adding an option means adding a line to
    /// `TUNABLES`.
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
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(440.0)
                    .show(ui, |ui| {
                        for tunable in TUNABLES {
                            // Window state is not a preference and does not
                            // belong in the settings.
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
            tracing::error!(error = %format!("{error:#}"), "the settings could not be saved");
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
                tracing::warn!(path, error = %format!("{error:#}"), "the value cannot be set");
                false
            }
        }
    }

    /// Photographs the window once the tiles are in place, then quits.
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
                Ok(()) => tracing::info!(path = %path.display(), "screenshot saved"),
                Err(error) => {
                    tracing::error!(error = %format!("{error:#}"), "the screenshot could not be saved")
                }
            }

            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// The self-check: open a folder, run for a while and verify that no
    /// visible tile was left blank. This is exactly the fault the WPF
    /// benchmark had, where one swallowed exception meant not a single
    /// thumbnail was produced while the application looked like it was
    /// running.
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
            tracing::error!(error = %format!("{error:#}"), "the settings could not be saved");
        }
    }
}

/// Saves RGBA as a PNG. Written by hand so the UI does not drag in a whole
/// codec package for one debugging switch.
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

    // Uncompressed deflate blocks: no codec needed here.
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

/// Expands the tree down to the given folder and leaves it that way.
///
/// More than one root may contain the folder — on Windows the home folder
/// sits under `C:\` and is a root in its own right. So every match is
/// expanded, not only the first.
fn reveal(nodes: &mut [Node], folder: &Path) {
    for node in nodes {
        if !folder.starts_with(&node.path) {
            continue;
        }

        node.load_children();
        node.expanded = true;
        if node.path != folder
            && let Some(children) = node.children.as_mut()
        {
            reveal(children, folder);
        }
    }
}

/// The roots of the tree. The one place where the platform matters.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_ceiling_never_drops_below_the_screen() {
        // A setting of 300 against 400 needed textures meant endless
        // eviction and reloading, that is three quarters of a core for
        // nothing.
        assert!(effective_budget(300, 400) > 400);
        assert!(effective_budget(16, 1000) > 1000);
    }

    #[test]
    fn a_larger_setting_is_respected() {
        assert_eq!(effective_budget(5000, 100), 5000);
    }

    #[test]
    fn nonsense_values_do_not_get_through() {
        assert!(effective_budget(-1, 0) >= 16);
        assert!(effective_budget(i64::MAX, 0) <= 65_536);
    }

    fn child<'a>(node: &'a Node, name: &str) -> &'a Node {
        node.children
            .as_ref()
            .expect("children not loaded")
            .iter()
            .find(|child| child.name == name)
            .unwrap_or_else(|| panic!("{name} is missing from the tree"))
    }

    #[test]
    fn the_tree_expands_all_the_way_to_the_open_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2019").join("summer");
        std::fs::create_dir_all(path.join("sea")).unwrap();
        std::fs::create_dir_all(dir.path().join("2019").join("winter")).unwrap();
        std::fs::create_dir_all(dir.path().join("2020")).unwrap();

        let mut roots = vec![Node::new(dir.path().to_owned())];
        reveal(&mut roots, &path);

        assert!(roots[0].expanded);
        let year = child(&roots[0], "2019");
        assert!(
            year.expanded,
            "the path to the target has to be expanded all the way"
        );
        assert!(child(year, "summer").expanded);

        // Siblings are not expanded. Expanding everything along the way
        // means reading half the disk for one folder.
        assert!(!child(year, "winter").expanded);
        assert!(!child(&roots[0], "2020").expanded);

        // And below the target we do not descend: what is inside it we learn
        // when somebody asks by clicking.
        assert!(!child(child(year, "summer"), "sea").expanded);
    }

    #[test]
    fn a_root_off_the_path_stays_closed() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("photos")).unwrap();

        let mut roots = vec![
            Node::new(elsewhere.path().to_owned()),
            Node::new(dir.path().to_owned()),
        ];
        reveal(&mut roots, &dir.path().join("photos"));

        assert!(!roots[0].expanded, "a foreign root is not opened");
        assert!(roots[0].children.is_none(), "nor read from the disk");
        assert!(roots[1].expanded);
    }

    #[test]
    fn a_folder_that_is_gone_does_not_upset_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut roots = vec![Node::new(dir.path().to_owned())];
        reveal(&mut roots, &dir.path().join("deleted").join("deeper"));

        // The root opens, because the path really does point under it;
        // deeper there is nothing to find and that is where it ends — not in
        // a panic.
        assert!(roots[0].expanded);
        assert!(has_no_child(&roots[0], "deleted"));
    }

    /// The log filter matches targets by crate name, and for a binary that
    /// is not the package name (`photosite_ui`) but the target name
    /// (`photosite`). Until that was fixed, not one line from the application
    /// itself reached the log — and there was no way to tell, because
    /// warnings and errors fell through the general level at the end.
    #[test]
    fn the_applications_own_log_gets_through_the_filter() {
        let name = module_path!().split("::").next().expect("empty path");
        for verbose in [false, true] {
            let filter = diagnostics::default_filter(verbose);
            assert!(
                filter.contains(&format!("{name}=")),
                "the default filter does not know {name}: {filter}"
            );
        }
    }

    fn has_no_child(node: &Node, name: &str) -> bool {
        node.children
            .as_ref()
            .is_some_and(|children| !children.iter().any(|child| child.name == name))
    }
}

/// Selecting tiles and saying things about them.
///
/// Kept apart from the module above, which is about the cache, the tree
/// and the log. Different subjects, and a hundred-line test module that
/// covers everything is one nobody reads.
#[cfg(test)]
mod culling {
    use super::*;

    /// An application over a folder of files that are not really
    /// photographs.
    ///
    /// Nothing here decodes anything: the selection and the catalogue do not
    /// care what is inside a file, and building real JPEGs would be testing
    /// the decoder instead. The length is what the sort has to tell them
    /// apart by.
    fn app_over(files: &[(&str, usize)]) -> (App, tempfile::TempDir, tempfile::TempDir) {
        let data = tempfile::tempdir().expect("no temp folder");
        let photos = tempfile::tempdir().expect("no temp folder");
        for (name, size) in files {
            std::fs::write(photos.path().join(name), vec![b'x'; *size]).expect("cannot write");
        }

        let paths = Paths::resolve(Some(data.path())).expect("no paths");
        paths.ensure().expect("cannot make the folders");
        let mut settings = Settings::default();
        // By name, so the order in the test is the order in the list rather
        // than whatever the files happen to say.
        settings.gallery.sort_field = SortField::Name.id().to_owned();
        let app = App::new(paths, settings, Some(photos.path().to_path_buf()));
        (app, data, photos)
    }

    fn three() -> (App, tempfile::TempDir, tempfile::TempDir) {
        app_over(&[("a.jpg", 30), ("b.jpg", 20), ("c.jpg", 10)])
    }

    fn names(app: &App) -> Vec<String> {
        app.photos
            .iter()
            .map(|photo| {
                photo
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn chosen_names(app: &App) -> Vec<String> {
        app.selection
            .iter()
            .filter_map(|at| app.photos.get(*at))
            .map(|photo| {
                photo
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn a_folder_arrives_with_a_row_for_every_file() {
        let (app, _data, _photos) = three();
        assert_eq!(names(&app), ["a.jpg", "b.jpg", "c.jpg"]);
        // Every one of them has a catalogue row already, or there would be
        // nowhere for a rating to go.
        assert!(app.photos.iter().all(|photo| photo.id.0 > 0));
    }

    #[test]
    fn a_plain_click_selects_one_and_only_one() {
        let (mut app, _data, _photos) = three();
        app.select_only(2);
        app.select_only(0);
        assert_eq!(chosen_names(&app), ["a.jpg"]);
        assert_eq!(app.selected, Some(0));
    }

    #[test]
    fn ctrl_click_adds_and_takes_away() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.select_also(2);
        assert_eq!(chosen_names(&app), ["a.jpg", "c.jpg"]);

        app.select_also(2);
        assert_eq!(chosen_names(&app), ["a.jpg"]);
    }

    #[test]
    fn taking_the_current_tile_out_moves_the_preview_to_another() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.select_also(1);
        assert_eq!(app.selected, Some(1));

        // The preview was following the tile just removed. It must follow
        // something still selected rather than going blank.
        app.select_also(1);
        assert_eq!(app.selected, Some(0));
    }

    #[test]
    fn shift_click_takes_everything_between() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.select_through(2);
        assert_eq!(chosen_names(&app), ["a.jpg", "b.jpg", "c.jpg"]);

        // And the other way round, which is the same range.
        app.select_only(2);
        app.select_through(0);
        assert_eq!(chosen_names(&app), ["a.jpg", "b.jpg", "c.jpg"]);
    }

    #[test]
    fn select_all_takes_the_folder() {
        let (mut app, _data, _photos) = three();
        app.run_for_test("photo.select_all");
        assert_eq!(app.selection.len(), 3);
        assert!(app.selected.is_some());
    }

    #[test]
    fn the_stars_reach_both_the_rows_and_the_catalogue() {
        let (mut app, _data, _photos) = three();
        app.select_only(1);
        app.run_for_test("photo.rate_4");

        assert_eq!(app.photos[1].organisation.rating, 4);
        let path = app.photos[1].path.clone();
        let written = app
            .catalog
            .as_ref()
            .expect("no catalogue")
            .by_path(&path)
            .expect("cannot read")
            .expect("no row");
        assert_eq!(written.organisation.rating, 4);
    }

    #[test]
    fn the_stars_go_on_everything_selected() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.select_through(2);
        app.run_for_test("photo.rate_5");
        assert!(
            app.photos
                .iter()
                .all(|photo| photo.organisation.rating == 5)
        );
    }

    #[test]
    fn a_pick_pressed_twice_takes_itself_off() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("photo.pick");
        assert_eq!(app.photos[0].organisation.flag, Flag::Picked);

        app.run_for_test("photo.pick");
        assert_eq!(app.photos[0].organisation.flag, Flag::None);
    }

    #[test]
    fn rejecting_something_picked_rejects_it_rather_than_clearing_it() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("photo.pick");
        app.run_for_test("photo.reject");
        assert_eq!(app.photos[0].organisation.flag, Flag::Rejected);
    }

    #[test]
    fn nothing_selected_means_nothing_written() {
        let (mut app, _data, _photos) = three();
        app.run_for_test("photo.rate_5");
        assert!(
            app.photos
                .iter()
                .all(|photo| photo.organisation.rating == 0)
        );
    }

    /// The one worth having. Rate something, change the order, and the stars
    /// must still be on the same photograph — not on whichever tile slid
    /// into that position.
    #[test]
    fn the_selection_follows_the_photographs_and_not_the_positions() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        assert_eq!(chosen_names(&app), ["a.jpg"]);

        // a.jpg is the largest, so by size it goes from first to last.
        app.settings.gallery.sort_descending = true;
        app.sort_by(SortField::FileSize);
        assert_eq!(names(&app), ["a.jpg", "b.jpg", "c.jpg"]);

        app.settings.gallery.sort_descending = false;
        app.sort_by(SortField::FileSize);
        assert_eq!(names(&app), ["c.jpg", "b.jpg", "a.jpg"]);
        assert_eq!(chosen_names(&app), ["a.jpg"], "the selection changed hands");
        assert_eq!(app.selected, Some(2));
    }

    #[test]
    fn a_rating_does_not_reorder_the_folder_under_the_hand_that_made_it() {
        let (mut app, _data, _photos) = three();
        app.sort_by(SortField::Rating);
        let before = names(&app);
        app.select_only(0);
        app.run_for_test("photo.rate_5");
        assert_eq!(
            names(&app),
            before,
            "the tile moved out from under the key that rated it"
        );
    }
}
