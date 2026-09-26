//! PhotoSite.
//!
//! This crate is the only one that knows about egui and about the GPU.
//! Everything else — the catalogue, paths, settings, themes, tasks, commands
//! — lives in the core and can be tested without a window.
//!
//! There are no constants here affecting looks or behaviour. It all comes
//! from [`Settings`], because what is hard-wired cannot be configured — and
//! what cannot be configured gets rewritten sooner or later.

// No console. An installed application that opens a black rectangle beside
// its own window looks broken; `startup::first` lends the process the
// terminal's console back when it was started from one.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod batch;
mod clipboard;
mod compare;
mod describe;
mod docks;
mod editor;
mod files;
mod filter;
mod grid;
mod info;
mod list;
mod people;
mod picker;
mod startup;
mod theme;
mod updates;

use anyhow::Result;
use eframe::egui;
use photosite_core::catalog::NewPhoto;
use photosite_core::commands::{Bindings, Group, Scope, Shortcut};
use photosite_core::compare::Compare;
use photosite_core::domain::{ColorLabel, Flag, Photo, PhotoId, Sort, SortField};
use photosite_core::filter::{Facets, Filter};
use photosite_core::history::History;
use photosite_core::settings::{Gallery, Kind, Settings, TUNABLES, Tunable};
use photosite_core::transfer::Mode;
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
    /// For the comparison, where somebody looks at a hundred per cent to
    /// decide which frame is the sharp one. Only ever asked for the two to
    /// four photographs being compared, which is what makes it affordable.
    Close,
    /// A photograph the People window is cutting face chips out of. One
    /// decode serves every face on it, however many that is.
    Face,
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
    startup::first();

    let mut args = std::env::args().skip(1);
    let mut data: Option<PathBuf> = None;
    let mut verbose = false;
    let mut selftest = false;
    let mut language: Option<String> = None;
    let mut shot: Option<PathBuf> = None;
    let mut reset = false;
    let mut open_settings = false;
    let mut open_filter = false;
    let mut open_people = false;
    let mut open_batch = false;
    let mut open_describe = false;
    let mut open_editor = false;
    let mut compare = 0usize;
    let mut search: Option<String> = None;
    let mut folder: Option<PathBuf> = None;
    let mut on: Option<std::ffi::OsString> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "--verbose" | "-v" => verbose = true,
            "--selftest" => selftest = true,
            "--lang" => language = args.next(),
            "--shot" => shot = args.next().map(PathBuf::from),
            "--reset-settings" => reset = true,
            "--open-settings" => open_settings = true,
            "--open-filter" => open_filter = true,
            // The same reason as `--open-filter`: a window that normally
            // needs a key can be opened from a command line, which is what
            // makes it possible to look at one without a hand on the
            // keyboard.
            "--open-people" => open_people = true,
            "--open-batch" => open_batch = true,
            "--open-describe" => open_describe = true,
            "--open-editor" => open_editor = true,
            // Opens straight into a comparison of the first few. Like
            // `--open-filter`, it exists so that a view which normally needs
            // a selection and a key can be seen from a command line.
            "--compare" => compare = args.next().and_then(|n| n.parse().ok()).unwrap_or(2),
            // Start already narrowed. The filter is not saved between
            // runs on purpose, so this is the only way to open on one.
            "--search" => search = args.next(),
            // A folder opens the gallery on it. A photograph opens the
            // folder it is in, standing on that photograph, and the
            // photograph itself in a tab — which is what "open with" from a
            // file manager means: look at this one.
            other => {
                let path = PathBuf::from(other);
                if path.is_file() {
                    on = path.file_name().map(|name| name.to_os_string());
                    folder = path.parent().map(Path::to_path_buf);
                } else {
                    folder = Some(path);
                }
            }
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
            app.show_filter = open_filter;
            app.people.open = open_people;
            app.people.stale = open_people;
            if open_batch {
                batch::open(&mut app);
            }

            if open_describe {
                describe::open(&mut app);
            }
            // The filter first: narrowing the gallery rebuilds the
            // selection, so choosing tiles before it means choosing tiles
            // that are about to be let go of.
            if let Some(search) = search {
                app.set_filter(photosite_core::filter::Filter {
                    search,
                    ..Default::default()
                });
            }

            if let Some(name) = on
                && let Some(at) = (0..app.count()).find(|at| {
                    app.photo(*at).and_then(|photo| photo.path.file_name()) == Some(&name)
                })
            {
                app.select_only(at);
                // Handed a photograph, not a folder: it goes straight into
                // the editor, with the manager standing on it behind.
                app.edit(at);
            }

            if compare > 0 {
                for at in 0..compare.min(app.count()) {
                    app.select_also(at);
                }

                app.run_for_shot("photo.compare");
            }

            if open_editor {
                if app.selected.is_none() && app.count() > 0 {
                    app.select_only(0);
                }

                app.run_for_shot("photo.edit");
            }
            app.dress(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("the window could not be opened: {error}"))
}

/// What the folder dialog is being asked for. The dialog is the same one;
/// only what becomes of the answer differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    ToOpen,
    ToCopyInto,
    ToMoveInto,
    /// Where a batch conversion puts its output.
    ToConvertInto,
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
    /// Whether a newer release is on its way. See [`updates`].
    updates: updates::Updates,
    images: jobs::Wishlist<Key, Pixels>,

    /// The catalogue.
    ///
    /// `None` when it could not be opened. The application still browses —
    /// it simply cannot remember anything, and the log says so rather than a
    /// rating vanishing without a word.
    catalog: Option<Catalog>,

    pub roots: Vec<Node>,
    pub folder: Option<PathBuf>,
    /// Where somebody has been, so back and forward mean something.
    history: History,
    /// A name being typed, when one is.
    pub asking: Option<files::Asking>,
    /// Kept alive so it keeps watching; dropping it stops the watch.
    watcher: Option<notify::RecommendedWatcher>,
    /// Set by the watcher when something in the folder changed. Read on the
    /// UI thread, which is the only place that may touch the rest.
    disturbed: Arc<std::sync::atomic::AtomicBool>,
    /// When the last disturbance was noticed. A folder being written to by
    /// something else produces a burst of these, and rereading on each one
    /// would be a rescan a millisecond.
    disturbed_at: Option<std::time::Instant>,
    /// The folder as the catalogue holds it, in the chosen order —
    /// everything, whether the filter lets it through or not.
    ///
    /// Kept apart from what is shown so that typing in the search box costs
    /// one pass over memory instead of one query. Over a hundred thousand
    /// photographs the difference is the box being usable or not.
    pub all: Vec<Photo>,
    /// Which of them get through, as positions into [`App::all`]. The grid,
    /// the preview and the selection all work in these, so a filtered folder
    /// behaves in every way like a folder.
    pub visible: Vec<usize>,
    pub filter: Filter,
    /// What this folder actually holds, so the panel offers nothing that
    /// would come back empty.
    pub facets: Facets,
    pub show_filter: bool,
    /// The two ends of the date range, as typed. Held as text so a
    /// half-written date does not keep clearing itself while somebody is
    /// still typing it.
    pub filter_from: String,
    pub filter_to: String,
    /// The tile the preview and the details follow, and the end a shift-click
    /// measures from. Always one of [`App::selection`] when there is one.
    pub selected: Option<usize>,
    /// Every tile the next rating lands on.
    ///
    /// Positions and not paths, because the grid works in positions and the
    /// set is rebuilt whenever the order changes — see [`App::relist`].
    pub selection: BTreeSet<usize>,
    /// Two to four photographs side by side, while somebody is looking at
    /// them. It is drawn in place of the docks, not over them: a comparison
    /// wants the whole window, which is the reason for opening one.
    pub compare: Option<Compare>,
    /// The background pass reading this folder's headers, while one runs.
    indexing: Option<u64>,
    /// The background pass writing what somebody said into the files.
    writing: Option<u64>,

    textures: HashMap<Key, egui::TextureHandle>,
    order: Vec<Key>,
    pub wanted_quick: Vec<PathBuf>,
    pub wanted_sharp: Vec<PathBuf>,
    pub wanted_preview: Vec<PathBuf>,
    pub wanted_close: Vec<PathBuf>,
    /// The photographs the People window wants to cut face chips out of.
    pub wanted_faces: Vec<PathBuf>,
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
    /// Whether the window is filling the screen. Held rather than asked for,
    /// because the toolkit reports it a frame late and a toggle read from a
    /// stale answer flickers.
    fullscreen: bool,
    show_settings: bool,
    /// The dock layout, read from the settings. Written back as soon as
    /// somebody moves a splitter.
    pub layout: layout::Layout,
    /// Which panes are hidden.
    pub hidden: Vec<String>,
    /// The People window: who is known, who is waiting to be named, and the
    /// sweep that finds them.
    pub people: people::People,
    /// The batch window: the settings, and what they would do.
    pub batch: batch::Batch,
    /// The describe window.
    pub describe: describe::Describe,
    /// The offline gazetteer, when there is one. Loaded once at start —
    /// it is tens of megabytes of text and reading it per photograph would
    /// be most of a describe run.
    pub places: Option<Arc<photosite_core::Gazetteer>>,
    /// The faces of the photograph in the preview, and whose they are.
    /// Read when the preview changes rather than when it is drawn.
    pub preview_faces_of: Option<PhotoId>,
    pub preview_faces: Vec<photosite_core::people::Face>,
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
    pub edit_place: String,
    /// A name being typed to say somebody is on the photograph.
    pub edit_person: String,
    /// The folder dialog, at most one — and what it is being asked for. The
    /// same dialog serves opening a folder and choosing where files go; only
    /// what happens to the answer differs.
    folder_dialog: Option<(picker::Picker, Wanted)>,
    /// What was last copied here, and whether it was a cut.
    clipboard: clipboard::Held,
    /// The folder the tree on the left should scroll to. Set on opening, and
    /// taken by the tree straight away.
    pub scroll_tree_to: Option<PathBuf>,
    /// The tile the grid should scroll to, as a position. Set on coming
    /// back from the editor, and taken by the grid straight away.
    pub scroll_grid_to: Option<usize>,
    /// The strip of tabs: the manager, then every photograph opened on its
    /// own. See [`editor::Tabs`].
    pub tabs: editor::Tabs,

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
            .field("photos", &self.all.len())
            .field("visible", &self.visible.len())
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
        let close = settings.loading.compare_size.clamp(64, 16384) as u32;
        let faces = settings.faces.crop_size.clamp(160, 4096) as u32;
        let embedded = settings.loading.use_embedded_thumbnails;
        // Only the tiles are kept. A preview is a megabyte and is wanted for
        // one photograph at a time; keeping those would be a library's worth
        // of disk for something that is decoded in the time it takes to
        // click.
        let tiles = settings
            .loading
            .cache_thumbnails
            .then(|| img::Cache::new(paths.thumbnails()));
        let waker: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
        let wake = waker.clone();

        let images = jobs::Wishlist::new(threads, move |key: &Key| {
            let (path, want) = key;
            let outcome = match want {
                Want::Quick if !embedded => Ok(None),
                Want::Quick => img::quick(path).map(|found| found.map(into_pixels)),
                Want::Thumb => match &tiles {
                    Some(cache) => cache.thumb(path, thumb).map(|rgb| Some(into_pixels(rgb))),
                    None => img::sized(path, thumb).map(|rgb| Some(into_pixels(rgb))),
                },
                Want::Preview => img::sized(path, preview).map(|rgb| Some(into_pixels(rgb))),
                Want::Close => img::sized(path, close).map(|rgb| Some(into_pixels(rgb))),
                Want::Face => img::sized(path, faces).map(|rgb| Some(into_pixels(rgb))),
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

        let paths_for_places = paths.places();
        let theme = palettes::resolve(&settings.appearance, None);
        let settings_layout = settings.window.layout.clone();
        let settings_hidden = settings.window.docks_hidden.clone();
        // Asked before the settings move into the application, and off
        // this thread. Not installed — every `cargo run` — is answered in a
        // line of the log and nothing else.
        let updates = updates::Updates::start(&settings, waker.clone());
        let mut app = Self {
            paths,
            settings,
            bindings: Bindings::defaults(),
            tasks: jobs::Tasks::new(),
            updates,
            images,
            catalog,
            roots: roots(),
            folder: None,
            history: History::new(),
            asking: None,
            watcher: None,
            disturbed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            disturbed_at: None,
            all: Vec::new(),
            visible: Vec::new(),
            filter: Filter::default(),
            facets: Facets::default(),
            show_filter: false,
            writing: None,
            filter_from: String::new(),
            filter_to: String::new(),
            selected: None,
            selection: BTreeSet::new(),
            indexing: None,
            textures: HashMap::new(),
            order: Vec::new(),
            wanted_quick: Vec::new(),
            wanted_sharp: Vec::new(),
            wanted_preview: Vec::new(),
            wanted_close: Vec::new(),
            wanted_faces: Vec::new(),
            people: people::People::default(),
            batch: batch::Batch::default(),
            describe: describe::Describe::default(),
            places: match photosite_core::Gazetteer::load(&paths_for_places) {
                Ok(places) => places.map(Arc::new),
                Err(error) => {
                    tracing::warn!(
                        error = %format!("{error:#}"),
                        "the gazetteer could not be read; describing will name no places"
                    );
                    None
                }
            },
            preview_faces_of: None,
            preview_faces: Vec::new(),
            compare: None,
            blank: 0,
            unsharp: 0,
            needed: 0,
            theme,
            waker,
            status: i18n::t("gallery-pick-folder"),
            show_diagnostics: false,
            fullscreen: false,
            show_settings: false,
            layout: layout::parse_or_default(&settings_layout),
            hidden: layout::hidden(&settings_hidden),
            info_of: None,
            info_rows: Vec::new(),
            edit_of: None,
            edit_title: String::new(),
            edit_description: String::new(),
            edit_keywords: String::new(),
            edit_place: String::new(),
            edit_person: String::new(),
            folder_dialog: None,
            clipboard: clipboard::Held::default(),
            scroll_tree_to: None,
            scroll_grid_to: None,
            tabs: editor::Tabs::default(),
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
        // A comparison is of photographs in front of us. Somewhere else is
        // somewhere else, and a comparison that outlived the folder it came
        // from would rate rows that are no longer on screen.
        self.compare = None;
        self.relist();
        self.start_indexing();
        // Anything left unwritten from last time goes now.
        self.start_writing();

        self.history.went(folder.clone());
        self.watch(&folder);
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

    /// Goes wherever the history says, without recording it again.
    fn go(&mut self, folder: Option<PathBuf>) {
        let Some(folder) = folder else {
            return;
        };

        if folder.is_dir() {
            self.open(folder);
        } else {
            // A folder that has been renamed or unplugged since. Saying so
            // beats opening nothing and leaving somebody wondering.
            self.status = t!("error-not-a-folder", path = folder.display().to_string());
        }
    }

    /// Watches the open folder, so that what another program does to it
    /// shows here.
    ///
    /// One folder at a time: the old watcher is dropped, which is what stops
    /// it. Watching every folder ever visited would hold handles on drives
    /// somebody has finished with.
    fn watch(&mut self, folder: &Path) {
        use notify::Watcher as _;

        self.watcher = None;
        let disturbed = self.disturbed.clone();
        let waker = self.waker.clone();
        let mut watcher = match notify::recommended_watcher(move |outcome| match outcome {
            Ok(_) => {
                disturbed.store(true, std::sync::atomic::Ordering::Relaxed);
                if let Some(ctx) = waker.get() {
                    ctx.request_repaint();
                }
            }
            Err(error) => tracing::warn!(%error, "the folder watch reported an error"),
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                // Not being able to watch is a smaller thing than not being
                // able to browse; F5 still works.
                tracing::warn!(%error, "the folder cannot be watched");
                return;
            }
        };

        let depth = if self.settings.gallery.recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };
        match watcher.watch(folder, depth) {
            Ok(()) => self.watcher = Some(watcher),
            Err(error) => tracing::warn!(folder = %folder.display(), %error, "cannot watch"),
        }
    }

    /// Rereads the folder once whatever was happening to it has stopped.
    fn collect_disturbance(&mut self) {
        if self
            .disturbed
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.disturbed_at = Some(std::time::Instant::now());
        }

        let Some(since) = self.disturbed_at else {
            return;
        };

        // Our own writing disturbs the folder, and rereading on the back of
        // it would mean a rescan for every star anybody presses.
        if self.writing.is_some() {
            self.disturbed_at = None;
            return;
        }

        if since.elapsed() < std::time::Duration::from_millis(600) {
            return;
        }

        self.disturbed_at = None;
        if let Some(folder) = self.folder.clone() {
            tracing::debug!(folder = %folder.display(), "the folder changed underneath us");
            self.open(folder);
        }
    }

    /// The photographs the file operations act on.
    fn chosen_paths(&self) -> Vec<PathBuf> {
        self.selection
            .iter()
            .filter_map(|at| self.photo(*at))
            .map(|photo| photo.path.clone())
            .collect()
    }

    /// Runs a file operation and says what came of it, whichever way it went.
    fn did<T>(&mut self, outcome: anyhow::Result<T>, said: impl FnOnce(T) -> String) {
        match outcome {
            Ok(value) => self.status = said(value),
            Err(error) => {
                let error = format!("{error:#}");
                tracing::error!(%error, "the file operation failed");
                self.status = error;
            }
        }
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
    pub(crate) fn relist(&mut self) {
        let Some(folder) = self.folder.clone() else {
            self.all.clear();
            self.visible.clear();
            self.facets = Facets::default();
            return;
        };

        let kept = self.hold();
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

        // What the panel may offer comes from the whole folder, not from
        // what is showing. Otherwise choosing a camera would take every
        // other camera out of the list and there would be no way back.
        self.facets = Facets::of(&photos);
        self.all = photos;
        self.narrow(kept);
    }

    /// Works out what gets through the filter, without touching the disk.
    ///
    /// This is what runs on every keystroke in the search box: one pass over
    /// memory rather than one query. Over a hundred thousand photographs
    /// that is the difference between a search box and a stutter.
    fn refilter(&mut self) {
        let kept = self.hold();
        self.narrow(kept);
    }

    /// The paths currently selected, so they can be found again afterwards.
    fn hold(&self) -> (HashSet<PathBuf>, Option<PathBuf>) {
        let held = self
            .selection
            .iter()
            .filter_map(|at| self.photo(*at))
            .map(|photo| photo.path.clone())
            .collect();
        let current = self.photo_at_cursor().map(|photo| photo.path.clone());
        (held, current)
    }

    /// Rebuilds what is shown and puts the selection back on the same
    /// photographs — by path, never by position. A filter that moved the
    /// selection onto whatever slid into the gap would be worse than no
    /// filter.
    fn narrow(&mut self, (held, current): (HashSet<PathBuf>, Option<PathBuf>)) {
        self.visible = self
            .all
            .iter()
            .enumerate()
            .filter(|(_, photo)| self.filter.keeps(photo))
            .map(|(at, _)| at)
            .collect();

        self.selection = self
            .visible
            .iter()
            .enumerate()
            .filter(|(_, at)| held.contains(&self.all[**at].path))
            .map(|(position, _)| position)
            .collect();
        self.selected = current
            .and_then(|path| {
                self.visible
                    .iter()
                    .position(|at| self.all[*at].path == path)
            })
            .or_else(|| self.selection.iter().next().copied());
    }

    /// How many photographs are showing.
    pub fn count(&self) -> usize {
        self.visible.len()
    }

    /// How many the folder holds, filter or no filter.
    pub fn total(&self) -> usize {
        self.all.len()
    }

    /// The photograph at a position in the gallery.
    pub fn photo(&self, at: usize) -> Option<&Photo> {
        self.all.get(*self.visible.get(at)?)
    }

    fn photo_mut(&mut self, at: usize) -> Option<&mut Photo> {
        let at = *self.visible.get(at)?;
        self.all.get_mut(at)
    }

    /// A photograph by its path, wherever the filter has left it.
    ///
    /// The whole folder and not the visible part of it: the comparison holds
    /// paths and has to go on knowing how large a photograph is even after
    /// the filter has hidden its tile.
    pub fn photo_named(&self, path: &Path) -> Option<&Photo> {
        self.all.iter().find(|photo| photo.path == path)
    }

    /// The one the preview and the details follow.
    pub fn photo_at_cursor(&self) -> Option<&Photo> {
        self.photo(self.selected?)
    }

    /// Replaces the filter and reworks what is shown, but only when it
    /// really changed — the search box hands one back every frame.
    fn set_filter(&mut self, filter: Filter) {
        if filter != self.filter {
            self.filter = filter;
            self.refilter();
        }
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

                let read: Vec<(NewPhoto, photosite_meta::xmp::Xmp)> = chunk
                    .iter()
                    .filter_map(|path| photosite_meta::scan(path))
                    .collect();
                let batch: Vec<NewPhoto> = read.iter().map(|(photo, _)| photo.clone()).collect();
                catalog.upsert_many(&batch)?;

                // A library that has been used before arrives with ratings
                // and titles already in the files. Taking them is what makes
                // an old library look like itself the first time it is
                // opened here — and `seed` never overwrites anything already
                // said in the catalogue.
                for (photo, said) in &read {
                    if said.is_empty() {
                        continue;
                    }

                    if let Some(id) = catalog.id_of(&photo.path)?
                        && catalog.seed(id, &said.as_organisation())?
                    {
                        tracing::debug!(path = %photo.path.display(), "took what the file said");
                    }
                }
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

    /// Reads the folder again after something happened to it on disk.
    /// Reads the open folder again, keeping whatever was just said about
    /// what happened.
    ///
    /// Opening a folder reports how many photographs it holds and how long
    /// it took, which is right when somebody opened it and wrong when they
    /// deleted something: "3 photographs in 2 ms" is not an answer to "did
    /// that delete work?". So the message survives the reload.
    fn reopen(&mut self) {
        let Some(folder) = self.folder.clone() else {
            return;
        };

        let said = std::mem::take(&mut self.status);
        self.open(folder);
        if !said.is_empty() {
            self.status = said;
        }
    }

    /// Notices that the background pass has finished and takes the rows
    /// again. Polled every frame, which costs one lock and nothing else.
    /// Notices that a face sweep has finished, and takes what it found.
    ///
    /// The summary is the task's own last message rather than something
    /// passed back another way: a task already has somewhere to say how it
    /// went, and a second channel for the same thing is a second thing to
    /// keep in step.
    fn collect_faces(&mut self) {
        let Some(id) = self.people.scanning else {
            return;
        };

        let Some(task) = self
            .tasks
            .snapshot()
            .into_iter()
            .find(|task| task.id == id && task.finished)
        else {
            return;
        };

        self.people.scanning = None;
        self.people.summary = match &task.error {
            Some(error) => error.clone(),
            None => task.message.clone(),
        };
        self.people.stale = true;
        // Who is on which photograph has changed, so the badges, the
        // details and the filter all need reading again.
        self.preview_faces_of = None;
        self.relist();
        self.start_writing();
        self.tasks.forget_finished();
    }

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
    fn ask_for_folder(&mut self, ctx: &egui::Context, wanted: Wanted) {
        if self.folder_dialog.is_some() {
            return;
        }

        // A destination dialog opens where the last one went, not where the
        // gallery is: sorting into piles means going back to the same place.
        let remembered = match wanted {
            Wanted::ToOpen => self.settings.gallery.last_folder.as_deref(),
            _ => self.settings.gallery.last_destination.as_deref().or(self
                .settings
                .gallery
                .last_folder
                .as_deref()),
        };
        let start = picker::start_dir(self.folder.as_deref(), remembered);
        let title = match wanted {
            Wanted::ToOpen => t!("dialog-pick-folder"),
            Wanted::ToCopyInto => t!("copy-into"),
            Wanted::ToMoveInto => t!("move-into"),
            Wanted::ToConvertInto => t!("batch-into"),
        };
        self.folder_dialog = Some((picker::ask(ctx, title, start), wanted));
    }

    /// Collects whatever the dialog returned. A cancelled dialog is reported
    /// nowhere — closing it is an answer like any other, not a failure.
    fn take_picked_folder(&mut self) {
        let Some((dialog, wanted)) = &self.folder_dialog else {
            return;
        };

        let wanted = *wanted;
        match dialog.answer() {
            picker::Answer::Waiting => {}
            picker::Answer::Cancelled => self.folder_dialog = None,
            picker::Answer::Picked(folder) => {
                self.folder_dialog = None;
                match wanted {
                    Wanted::ToOpen => self.open(folder),
                    Wanted::ToCopyInto => self.send_to(&folder, Mode::Copy),
                    Wanted::ToMoveInto => self.send_to(&folder, Mode::Move),
                    Wanted::ToConvertInto => {
                        self.batch.preset.into = Some(folder.to_string_lossy().into_owned());
                        self.batch.preset.beside_source = false;
                        self.batch.stale = true;
                    }
                }
            }
        }
    }

    /// Copies or moves what is chosen into a folder, and remembers it as the
    /// place the next `Ctrl+Shift+C` means.
    fn send_to(&mut self, folder: &Path, mode: Mode) {
        let chosen = self.chosen_paths();
        if chosen.is_empty() {
            self.status = t!("files-nothing-selected");
            return;
        }

        let outcome = files::transfer(self, &chosen, folder, mode);
        if outcome.is_ok() {
            self.settings.gallery.last_destination = Some(folder.display().to_string());
        }

        self.did(outcome, move |count| match mode {
            Mode::Copy => t!("files-copied", count = count as i64),
            Mode::Move => t!("files-moved", count = count as i64),
        });

        // A move takes photographs out of this folder and a copy can land in
        // it, so either way what is on screen may no longer be the truth.
        self.reopen();
    }

    /// Where the face models are: what the settings say, or the ordinary
    /// place beside the catalogue.
    pub fn model_folder(&self) -> PathBuf {
        let configured = self.settings.faces.models.trim();
        if configured.is_empty() {
            self.paths.models()
        } else {
            PathBuf::from(configured)
        }
    }

    /// How many threads background work may use. The same answer the
    /// decoding pool got, so a sweep and the grid are not each told a
    /// different number.
    pub fn worker_threads(&self) -> usize {
        match self.settings.loading.worker_threads {
            0 => jobs::worker_count(),
            count => count.clamp(1, 128) as usize,
        }
    }

    /// The texture a face chip is drawn from, and the part of it to draw.
    ///
    /// Nothing at all while the photograph is still being decoded — the
    /// window draws a plain frame then, rather than jumping about as things
    /// arrive.
    pub fn face_texture(
        &self,
        face: &photosite_core::people::Face,
    ) -> Option<(egui::TextureId, egui::Rect)> {
        let key = (face.path.clone(), Want::Face);
        let texture = self.textures.get(&key)?;
        let size = texture.size();
        let (x, y, width, height) = photosite_core::people::crop(
            face.rectangle(),
            self.settings.faces.crop_margin,
            (size[0] as u32, size[1] as u32),
        );
        Some((
            texture.id(),
            egui::Rect::from_min_size(
                egui::pos2(x as f32, y as f32),
                egui::vec2(width as f32, height as f32),
            ),
        ))
    }

    /// Asks where a batch conversion should put its output.
    pub fn ask_for_folder_for_batch(&mut self, ctx: &egui::Context) {
        self.ask_for_folder(ctx, Wanted::ToConvertInto);
    }

    /// Notices that a describe run has finished.
    fn collect_describing(&mut self) {
        let Some(id) = self.describe.running else {
            return;
        };

        let Some(task) = self
            .tasks
            .snapshot()
            .into_iter()
            .find(|task| task.id == id && task.finished)
        else {
            return;
        };

        self.describe.running = None;
        self.describe.summary = match &task.error {
            Some(error) => error.clone(),
            None => task.message.clone(),
        };
        // Titles, descriptions and keywords have changed, so the folder is
        // read again and the queue drained into the files.
        self.relist();
        self.start_writing();
        self.tasks.forget_finished();
    }

    /// Notices that a conversion has finished, and says how it went.
    fn collect_batch(&mut self) {
        let Some(id) = self.batch.running else {
            return;
        };

        let Some(task) = self
            .tasks
            .snapshot()
            .into_iter()
            .find(|task| task.id == id && task.finished)
        else {
            return;
        };

        self.batch.running = None;
        self.batch.summary = match &task.error {
            Some(error) => error.clone(),
            None => task.message.clone(),
        };
        // The outputs may have landed in the folder being looked at.
        self.batch.stale = true;
        self.reopen();
        self.tasks.forget_finished();
    }

    /// The faces of one photograph, read once and kept until another is
    /// looked at.
    pub fn load_faces(&mut self, photo: PhotoId) {
        if self.preview_faces_of == Some(photo) {
            return;
        }

        self.preview_faces = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.faces_of(photo).ok())
            .unwrap_or_default();
        self.preview_faces_of = Some(photo);
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
        self.run_for_shot(id);
    }

    /// The same, for the command line flags that open a view before the
    /// first frame — there is no context to hand them yet either.
    fn run_for_shot(&mut self, id: &str) {
        let ctx = egui::Context::default();
        self.run(id, &ctx);
    }

    pub fn run(&mut self, id: &str, ctx: &egui::Context) {
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
            "view.as_list" => self.settings.gallery.as_list = !self.settings.gallery.as_list,
            "view.fullscreen" => {
                self.fullscreen = !self.fullscreen;
                self.apply_fullscreen(ctx);
            }
            "photo.edit" => self.edit_chosen(),
            "editor.close" => {
                self.close_tab(self.tabs.active(), ctx);
            }
            "editor.back" => self.editor_back(ctx),
            "editor.fullscreen" => {
                self.tabs.fullscreen = !self.tabs.fullscreen;
                self.apply_fullscreen(ctx);
            }
            "editor.next" => self.page_editor(1),
            "editor.previous" => self.page_editor(-1),
            // One pixel per point, about the middle of what is on screen —
            // and the whole of it again. The editor remembers the size of
            // its cell from the last frame, which is what the zoom is
            // measured against.
            "editor.actual" => {
                if let Some(editor) = self.tabs.active_editor_mut() {
                    let zoom = photosite_core::compare::one_to_one(editor.cell, editor.image);
                    editor.view = photosite_core::compare::settled(
                        photosite_core::compare::View {
                            zoom,
                            ..editor.view
                        },
                        editor.cell,
                        editor.image,
                    );
                }
            }
            "editor.fit" => {
                if let Some(editor) = self.tabs.active_editor_mut() {
                    editor.view = photosite_core::compare::View::FITTED;
                }
            }
            "view.settings" => self.show_settings = !self.show_settings,
            "photo.batch" => batch::open(self),
            "photo.describe" => describe::open(self),
            "photo.people" => {
                self.people.open = !self.people.open;
                if self.people.open {
                    self.people.stale = true;
                }
            }
            "view.filter" => self.show_filter = !self.show_filter,
            "view.clear_filter" => {
                self.filter_from.clear();
                self.filter_to.clear();
                self.set_filter(Filter::default());
            }
            "view.toggle_tree" => self.toggle_dock("tree"),
            "view.toggle_preview" => self.toggle_dock("preview"),
            "view.toggle_info" => self.toggle_dock("info"),
            "view.reset_layout" => {
                self.settings.window.layout = layout::DEFAULT.to_owned();
                self.settings.window.docks_hidden = String::new();
                self.layout = layout::parse_or_default(layout::DEFAULT);
                self.hidden.clear();
            }
            "go.back" => {
                let to = self.history.back().map(Path::to_path_buf);
                self.go(to);
            }
            "go.forward" => {
                let to = self.history.forward().map(Path::to_path_buf);
                self.go(to);
            }
            "go.up" => {
                let to = self.folder.as_deref().and_then(History::up_from);
                self.go(to);
            }
            "file.rename" => {
                if let Some(photo) = self.photo_at_cursor() {
                    let path = photo.path.clone();
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.asking = Some(files::Asking::Rename { path, name });
                }
            }
            "file.duplicate" => {
                let chosen = self.chosen_paths();
                if chosen.is_empty() {
                    self.status = t!("files-nothing-selected");
                } else {
                    let made = files::duplicate(&chosen);
                    self.did(made, |made| {
                        t!("files-duplicated", count = made.len() as i64)
                    });
                    self.reopen();
                }
            }
            // Delete takes a photograph out of the comparison rather than
            // off the disk. The same key, and in both places it means "get
            // this out of what I am looking at" — it is only in the gallery
            // that what one is looking at is the folder itself.
            "file.delete" if self.compare.is_some() => self.drop_from_comparison(),
            "file.delete" => {
                let chosen = self.chosen_paths();
                if chosen.is_empty() {
                    self.status = t!("files-nothing-selected");
                } else {
                    let count = chosen.len() as i64;
                    let outcome = files::delete(self, &chosen);
                    self.did(outcome, move |()| t!("files-deleted", count = count));
                    self.reopen();
                }
            }
            "file.new_folder" => {
                if let Some(folder) = self.folder.clone() {
                    self.asking = Some(files::Asking::NewFolder {
                        inside: folder,
                        name: String::new(),
                    });
                }
            }
            "file.reveal" => {
                if let Some(photo) = self.photo_at_cursor() {
                    files::reveal(&photo.path.clone());
                }
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
            "photo.compare" => self.compare_chosen(),
            "photo.compare_next" => self.move_comparison_focus(1),
            "photo.compare_previous" => self.move_comparison_focus(-1),
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
            "file.open_folder" => self.ask_for_folder(ctx, Wanted::ToOpen),
            "file.copy" => self.onto_clipboard(ctx, false),
            "file.cut" => self.onto_clipboard(ctx, true),
            "file.paste" => self.paste(),
            "file.copy_to" => self.ask_for_folder(ctx, Wanted::ToCopyInto),
            "file.move_to" => self.ask_for_folder(ctx, Wanted::ToMoveInto),
            "file.copy_again" => match self.settings.gallery.last_destination.clone() {
                Some(folder) => self.send_to(Path::new(&folder), Mode::Copy),
                None => self.status = t!("files-nowhere-yet"),
            },
            other => tracing::warn!(command = other, "command with no handler"),
        }
    }

    /// Puts what is chosen on the clipboard, ours and the system's both.
    fn onto_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        let chosen = clipboard::existing(&self.chosen_paths());
        if chosen.is_empty() {
            self.status = t!("files-nothing-selected");
            return;
        }

        let count = chosen.len() as i64;
        self.clipboard = clipboard::put(ctx, &chosen, cut);
        self.status = if cut {
            t!("files-cut-to-the-clipboard", count = count)
        } else {
            t!("files-on-the-clipboard", count = count)
        };
    }

    /// Brings whatever is on the clipboard into the open folder.
    fn paste(&mut self) {
        let Some(folder) = self.folder.clone() else {
            self.status = t!("files-nowhere-to-paste");
            return;
        };

        let held = clipboard::take(&self.clipboard);
        let files = clipboard::pastable(&held);
        if files.is_empty() {
            self.status = t!("files-clipboard-empty");
            return;
        }

        let mode = held.mode();
        let outcome = files::transfer(self, &files, &folder, mode);
        // A cut is spent once it is pasted. Leaving it on would move the
        // same photographs again on the next Ctrl+V, from a folder they are
        // no longer in.
        if outcome.is_ok() && held.cut {
            self.clipboard = clipboard::Held::default();
        }

        match outcome {
            Ok(0) => self.status = t!("files-already-there"),
            outcome => self.did(outcome, move |count| match mode {
                Mode::Copy => t!("files-copied", count = count as i64),
                Mode::Move => t!("files-moved", count = count as i64),
            }),
        }

        self.reopen();
    }

    /// The photographs the next rating lands on.
    fn chosen(&self) -> Vec<PhotoId> {
        self.acting_on()
            .into_iter()
            .filter_map(|at| self.photo(at))
            .map(|photo| photo.id)
            .collect()
    }

    /// Where in the gallery the next rating lands.
    ///
    /// The selection — unless a comparison is open, and then it is the one
    /// photograph with the focus. The keys are the same keys; what they land
    /// on is whatever is being looked at, which is the only thing anybody
    /// pressing `3` ever means.
    fn acting_on(&self) -> Vec<usize> {
        match &self.compare {
            Some(compare) => self.position_of(compare.focused()).into_iter().collect(),
            None => self.selection.iter().copied().collect(),
        }
    }

    /// Where a photograph sits in the gallery right now.
    ///
    /// By path, because a comparison outlives a re-sort. It answers `None`
    /// for a photograph the filter has since hidden, and then the rating
    /// keys do nothing rather than landing on the wrong row.
    pub fn position_of(&self, path: &Path) -> Option<usize> {
        (0..self.count()).find(|at| self.photo(*at).map(|photo| photo.path.as_path()) == Some(path))
    }

    /// The photograph so many places on from this one, in the order the
    /// gallery shows. `None` at either end, and for a photograph that is no
    /// longer in the gallery at all.
    pub fn neighbour_of(&self, path: &Path, by: isize) -> Option<PathBuf> {
        let at = self.position_of(path)?;
        let to = at.checked_add_signed(by)?;
        self.photo(to).map(|photo| photo.path.clone())
    }

    /// Where the keys land: in the editor when a photograph is in front,
    /// otherwise in the manager.
    fn scope(&self) -> Scope {
        if self.tabs.manager_is_active() {
            Scope::Manager
        } else {
            Scope::Editor
        }
    }

    /// Opens the tile at this position in a tab of its own.
    pub fn edit(&mut self, at: usize) {
        let Some(path) = self.photo(at).map(|photo| photo.path.clone()) else {
            return;
        };

        self.tabs.open(path);
    }

    /// `Enter` in the manager: the tile the cursor is on, or a word about
    /// there being none.
    fn edit_chosen(&mut self) {
        match self.selected {
            Some(at) => self.edit(at),
            None => self.status = t!("editor-nothing-chosen"),
        }
    }

    /// Brings a tab to the front. The screen follows: the editor's
    /// fullscreen is the editor's, and the manager is never shown in it.
    pub fn activate_tab(&mut self, tab: usize, ctx: &egui::Context) {
        self.tabs.activate(tab);
        self.apply_fullscreen(ctx);
    }

    /// Closes a tab — from its cross or from the key. The one place the
    /// question about unsaved work will be asked, once there is any.
    pub fn close_tab(&mut self, tab: usize, ctx: &egui::Context) -> bool {
        let may = self
            .tabs
            .editors()
            .get(tab.wrapping_sub(1))
            .is_some_and(editor::Editor::can_close);
        if !may {
            return false;
        }

        self.tabs.close(tab);
        self.apply_fullscreen(ctx);
        true
    }

    /// `Enter` in the editor: the tab closes and the manager stands on its
    /// photograph — the folder opened if it has to be, the tree unfolded to
    /// it, the tile chosen and scrolled into view.
    fn editor_back(&mut self, ctx: &egui::Context) {
        let tab = self.tabs.active();
        let Some(path) = self.tabs.active_editor().map(|editor| editor.path.clone()) else {
            return;
        };

        if !self.close_tab(tab, ctx) {
            return;
        }

        self.tabs.activate(editor::Tabs::MANAGER);
        self.apply_fullscreen(ctx);
        self.show_in_manager(&path);
    }

    /// Puts the manager on a photograph, wherever it is. The folder is
    /// opened only if the photograph is not already in the gallery —
    /// which it is, however deep, when subfolders are included.
    pub fn show_in_manager(&mut self, path: &Path) {
        if self.position_of(path).is_none()
            && let Some(folder) = path.parent().map(Path::to_path_buf)
            && folder.is_dir()
        {
            self.open(folder);
        }

        match self.position_of(path) {
            Some(at) => {
                self.select_only(at);
                self.scroll_grid_to = Some(at);
            }
            None => self.status = t!("editor-gone"),
        }
    }

    /// Turns the page in the editor: the same tab, the next photograph
    /// along in the gallery's order. Nothing happens at either end, and
    /// nothing happens for a photograph the gallery has moved on without.
    pub fn page_editor(&mut self, by: isize) {
        let Some(path) = self.tabs.active_editor().map(|editor| editor.path.clone()) else {
            return;
        };

        // The tab is retargeted, not a new one opened: paging through a
        // folder must not leave a tab per photograph behind.
        if let Some(next) = self.neighbour_of(&path, by)
            && let Some(editor) = self.tabs.active_editor_mut()
            && editor.can_close()
        {
            editor.show(next);
        }
    }

    /// Tells the window whether to fill the screen: when the manager asked
    /// for it, or when the editor did and is in front.
    fn apply_fullscreen(&self, ctx: &egui::Context) {
        let wanted = self.fullscreen || (!self.tabs.manager_is_active() && self.tabs.fullscreen);
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(wanted));
    }

    /// Opens the comparison on what is chosen, or closes the one that is
    /// open. One key, both ways: nobody should have to hunt for the way out.
    fn compare_chosen(&mut self) {
        if self.compare.take().is_some() {
            return;
        }

        let chosen = self.chosen_paths();
        match Compare::open(&chosen) {
            Some(compare) => {
                if chosen.len() > photosite_core::compare::MOST {
                    self.status = t!(
                        "compare-only-four",
                        count = photosite_core::compare::MOST as i64
                    );
                }

                self.compare = Some(compare);
            }
            None => self.status = t!("compare-needs-two"),
        }
    }

    fn move_comparison_focus(&mut self, by: isize) {
        if let Some(compare) = &mut self.compare {
            compare.move_focus(by);
        }
    }

    /// Takes the focused photograph out of the comparison, and closes the
    /// comparison when what is left is no longer one. The file is untouched.
    fn drop_from_comparison(&mut self) {
        if let Some(compare) = &mut self.compare
            && !compare.drop_focused()
        {
            self.compare = None;
        }
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
        self.selection = (first..=last).filter(|at| *at < self.count()).collect();
        self.selected = Some(at);
    }

    fn select_all(&mut self) {
        self.selection = (0..self.count()).collect();
        if self.selected.is_none() {
            self.selected = self.selection.iter().next().copied();
        }
    }

    /// Queues the selection to be written into the files themselves.
    ///
    /// Always in the same breath as the catalogue change, so the two cannot
    /// come apart: the queue names the photograph and the write reads the
    /// catalogue, so whatever order things happen in, what lands in the file
    /// is what the catalogue says.
    fn queue_write(&mut self, photos: &[PhotoId]) {
        if photos.is_empty() {
            return;
        }

        let now = now();
        self.write_catalog(|catalog| catalog.enqueue(photos, now));
        self.start_writing();
    }

    /// Drains the queue on a thread of its own.
    ///
    /// Writing metadata means rewriting a six megabyte file. Doing that on
    /// the way to the next frame would stall the window for as long as the
    /// disk takes, and somebody culling a folder makes one of these every
    /// time they press a key.
    fn start_writing(&mut self) {
        if self.writing.is_some() {
            return;
        }

        // Nothing waiting means no thread. Opening a folder asks every time,
        // and spawning one to find out there is nothing to do is a thread
        // per folder for no reason.
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };

        match catalog.outbox() {
            Ok((0, _)) => return,
            Ok(_) => {}
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "cannot tell what is unwritten");
                return;
            }
        }

        let path = self.paths.catalog();
        let waker = self.waker.clone();
        let title = t!("task-writing-metadata");
        self.writing = Some(self.tasks.spawn(title, move |cancel, progress| {
            let mut catalog = Catalog::open(&path)?;
            let mut done = 0u64;

            loop {
                if cancel.cancelled() {
                    break;
                }

                let due = catalog.due(now(), 64)?;
                if due.is_empty() {
                    // Anything left is waiting out a backoff or has given up.
                    // Waiting here rather than finishing is what makes a file
                    // that was open elsewhere get written once it is closed,
                    // without somebody having to touch it again.
                    let (waiting, given_up) = catalog.outbox()?;
                    if waiting <= given_up {
                        break;
                    }

                    for _ in 0..30 {
                        if cancel.cancelled() {
                            return Ok(());
                        }

                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }

                    continue;
                }

                for entry in due {
                    if cancel.cancelled() {
                        break;
                    }

                    let path = entry.photo.path.clone();
                    match photosite_meta::write(
                        &path,
                        &entry.photo.organisation,
                        entry.photo.place,
                        regions_for(&entry),
                    ) {
                        Ok(_) => match photosite_core::FileIdentity::read(&path) {
                            Ok(identity) => catalog.written(entry.photo.id, &identity)?,
                            // Written but we cannot see it any more: the
                            // entry stays so the row is brought up to date
                            // on another go.
                            Err(error) => catalog.write_failed(
                                entry.photo.id,
                                now(),
                                &format!("written, but cannot be read back: {error}"),
                            )?,
                        },
                        Err(error) => {
                            let error = format!("{error:#}");
                            tracing::warn!(path = %path.display(), %error, "the metadata could not be written");
                            catalog.write_failed(entry.photo.id, now(), &error)?;
                        }
                    }

                    done += 1;
                    progress.report(done, None, path.display().to_string());
                }

                if let Some(ctx) = waker.get() {
                    ctx.request_repaint();
                }
            }

            Ok(())
        }));
    }

    /// Notices that the drain has finished.
    fn collect_writing(&mut self) {
        let Some(id) = self.writing else {
            return;
        };

        if self
            .tasks
            .snapshot()
            .into_iter()
            .any(|task| task.id == id && task.finished)
        {
            self.writing = None;
            self.tasks.forget_finished();
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
        self.queue_write(&chosen);
        let stars = stars.min(photosite_core::domain::Organisation::MAX_RATING);
        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
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
        self.queue_write(&chosen);
        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
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
            .acting_on()
            .into_iter()
            .filter_map(|at| self.photo(at))
            .all(|photo| photo.organisation.flag == flag);
        let wanted = if already { Flag::None } else { flag };

        // The verdict is not an XMP property and nothing is written into
        // the file for it — but the queue is still nudged, because the file
        // is rewritten from the catalogue as a whole and this keeps one code
        // path rather than two.
        self.write_catalog(|catalog| catalog.set_flag(&chosen, wanted));
        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
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
        self.edit_place = photo.place.map(|place| place.typed()).unwrap_or_default();
        self.edit_person.clear();
    }

    /// Says by hand that the person whose name was typed is on the chosen
    /// photographs — turned away, behind the camera, or simply not found by
    /// the detector. Ported from v1's "Assign person".
    ///
    /// The name goes into the keywords and the file exactly as a named face
    /// would, so nothing downstream can tell the two apart; the People
    /// window is where the faces themselves are named.
    pub fn tag_person_from_panel(&mut self) {
        let name = self.edit_person.trim().to_owned();
        let chosen = self.chosen();
        if name.is_empty() || chosen.is_empty() {
            return;
        }

        let now = now();
        self.write_catalog(move |catalog| {
            let person = catalog.person_named(&name)?;
            for photo in &chosen {
                catalog.tag_person(*photo, person, now)?;
            }

            Ok(())
        });
        self.edit_person.clear();
        // `tag_person` queues the file itself; what is left is to show it.
        self.relist();
        self.start_writing();
    }

    /// Takes a person off the chosen photographs: the hand-written tag, and
    /// any face of theirs on them, which returns to the unnamed pool.
    pub fn untag_person_from_panel(&mut self, person: i64) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        let now = now();
        self.write_catalog(move |catalog| {
            for photo in &chosen {
                catalog.untag_person(*photo, person, now)?;
            }

            Ok(())
        });
        self.relist();
        self.start_writing();
    }

    /// Writes the title as it now stands. Called when the field is left, not
    /// while it is being typed in.
    pub fn commit_title(&mut self) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        let text = self.edit_title.clone();
        let value = (!text.trim().is_empty()).then(|| text.trim().to_owned());
        let written = value.clone();
        self.write_catalog(move |catalog| catalog.set_title(&chosen, written.as_deref()));
        let chosen = self.chosen();
        self.queue_write(&chosen);
        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
                photo.organisation.title = value.clone();
            }
        }
    }

    pub fn commit_description(&mut self) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        let text = self.edit_description.clone();
        let value = (!text.trim().is_empty()).then(|| text.trim().to_owned());
        let written = value.clone();
        self.write_catalog(move |catalog| catalog.set_description(&chosen, written.as_deref()));
        let chosen = self.chosen();
        self.queue_write(&chosen);
        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
                photo.organisation.description = value.clone();
            }
        }
    }

    /// The keywords, as a line of them separated by commas.
    ///
    /// A comma and not a space, because a keyword can be two words —
    /// "Prague Castle" is one thing, not two.
    /// The keywords, from the box.
    ///
    /// **On one photograph they replace; on a selection they are added.** A
    /// person editing one photograph's keywords is editing a list they can
    /// see, and deleting a word out of the box must delete the word. Forty
    /// photographs have forty different lists and the box shows none of
    /// them, so setting would throw away everything already on thirty-nine
    /// of them — and nobody typing "holiday" into a box means that.
    pub fn commit_keywords(&mut self) {
        let chosen = self.chosen();
        let Some(first) = chosen.first().copied() else {
            return;
        };

        let words: Vec<String> = self
            .edit_keywords
            .split(',')
            .map(|word| word.trim().to_owned())
            .filter(|word| !word.is_empty())
            .collect();

        let many = chosen.len() > 1;
        let written = words.clone();
        let onto = chosen.clone();
        self.write_catalog(move |catalog| {
            if many {
                catalog.add_keywords(&onto, &written)
            } else {
                catalog.set_keywords(first, &written)
            }
        });
        self.queue_write(&chosen);

        // Read back what the catalogue made of it, so the field shows the
        // tidied list rather than what was typed. Only for one photograph —
        // a selection has no single list to show.
        let tidied = if many {
            words
        } else {
            self.catalog
                .as_ref()
                .and_then(|catalog| catalog.keywords_of(first).ok())
                .unwrap_or(words)
        };
        if !many {
            self.edit_keywords = tidied.join(", ");
        }

        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
                if many {
                    for word in &tidied {
                        if !photo.organisation.keywords.contains(word) {
                            photo.organisation.keywords.push(word.clone());
                        }
                    }

                    photo.organisation.keywords.sort();
                } else {
                    photo.organisation.keywords = tidied.clone();
                }
            }
        }
    }

    /// A position typed by a person, which is worth more than one read off a
    /// file: it goes into the catalogue as precise, and into the photograph
    /// itself so that a rescan reads back what was typed.
    ///
    /// An empty box is not a position of nothing — it is somebody having
    /// second thoughts, and nothing happens.
    pub fn commit_place(&mut self) {
        let Some(typed) = self.edit_place.trim().to_owned().into() else {
            return;
        };
        let typed: String = typed;
        if typed.is_empty() {
            return;
        }

        let Some(place) = photosite_core::place::parse(&typed) else {
            self.status = t!("info-place-unreadable");
            return;
        };

        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }

        let onto = chosen.clone();
        self.write_catalog(move |catalog| catalog.set_place(&onto, Some(place)));
        self.queue_write(&chosen);
        for at in self.acting_on() {
            if let Some(photo) = self.photo_mut(at) {
                photo.place = Some(place);
                photo.verdict = photosite_core::Verdict::Precise;
                photo.reason = None;
            }
        }

        self.edit_place = place.typed();
    }

    /// The stars, set from the details pane rather than the keyboard.
    /// Clicking the star a photograph already has takes the rating off,
    /// which is the only way to reach nought with the mouse.
    pub fn rate_from_panel(&mut self, stars: u8) {
        let already = self
            .selected
            .and_then(|at| self.photo(at))
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
    pub fn resize_tiles(&mut self, factor: f64) {
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
                    // A key the toolkit has no name for — `*` above all,
                    // which the numeric keypad sends and egui knows only as
                    // typed text. Letters and digits come through as keys
                    // already and are not taken twice.
                    egui::Event::Text(text)
                        if text.chars().count() == 1
                            && !text
                                .chars()
                                .all(|c| c.is_alphanumeric() || c.is_whitespace()) =>
                    {
                        Some(Shortcut {
                            ctrl: false,
                            shift: false,
                            alt: false,
                            key: text.clone(),
                        })
                    }
                    _ => None,
                })
                .collect()
        });

        for shortcut in pressed {
            if let Some(command) = self.bindings.command_for(&shortcut, self.scope()) {
                tracing::debug!(command = command.id, "shortcut");
                // Taken out of the input as well as acted on. Otherwise the
                // toolkit has its own use for the key afterwards — Tab moves
                // the focus to the search box while it is also moving the
                // focus in the comparison, and both happen at once.
                if let Some(key) = egui::Key::from_name(&shortcut.key) {
                    ctx.input_mut(|input| {
                        input.consume_key(
                            egui::Modifiers {
                                alt: shortcut.alt,
                                ctrl: shortcut.ctrl,
                                shift: shortcut.shift,
                                mac_cmd: false,
                                command: shortcut.ctrl,
                            },
                            key,
                        )
                    });
                }

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

/// What stands between the parts of the trail. Punctuation rather than a
/// word, so it needs no translation — and a single angle quote rather than
/// the greater-than sign, which reads as an operator.
const SEPARATOR_MARK: &str = "\u{203a}";

/// The face frames of one pending write, in the form the file wants.
///
/// Nothing at all comes back in two cases, and both are deliberate: when the
/// photograph has never been face-scanned, because another program's frames
/// are not ours to clear; and when we do not know the photograph's pixel
/// size, because an MWG region is a fraction of a frame and a frame of
/// unknown size is not something to guess at.
///
/// The size is the **shown** one. A camera held on its side writes the frame
/// as the sensor read it and records the turn separately; the faces were
/// found on the picture the right way up, so that is the frame they are
/// fractions of.
fn regions_for(entry: &photosite_core::catalog::Pending) -> Option<photosite_meta::xmp::Regions> {
    let faces = entry.regions.clone()?;
    let (width, height) = entry.photo.shown()?;
    Some(photosite_meta::xmp::Regions {
        width,
        height,
        faces,
    })
}

/// Seconds since the epoch.
///
/// The queue needs a clock for its backoff, and this is the only place the
/// application asks for one.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
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
        self.collect_faces();
        self.collect_batch();
        self.collect_describing();
        self.collect_writing();
        self.collect_disturbance();
        self.updates.poll();
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

        // The strip of tabs first, unless the editor is filling the screen:
        // then the photograph has the whole of it.
        if self.tabs.manager_is_active() || !self.tabs.fullscreen {
            egui::Panel::top("tabs")
                .frame(egui::Frame::NONE.fill(panel).inner_margin(4.0))
                .show(ui, |ui| editor::strip(self, ui, &palette, &ctx));
        }

        // Read after the strip: a click on it has just changed the answer.
        if !self.tabs.manager_is_active() {
            egui::CentralPanel::no_frame()
                .frame(egui::Frame::NONE.fill(window))
                .show(ui, |ui| editor::show(self, ui, &palette, &ctx));
        } else {
            egui::Panel::top("toolbar")
                .frame(egui::Frame::NONE.fill(panel).inner_margin(6.0))
                .show(ui, |ui| self.toolbar(ui, &palette, &ctx));

            egui::Panel::top("where")
                .frame(egui::Frame::NONE.fill(panel).inner_margin(4.0))
                .show(ui, |ui| self.where_we_are(ui, &palette, &ctx));

            egui::CentralPanel::no_frame()
                .frame(egui::Frame::NONE.fill(window))
                .show(ui, |ui| {
                    if self.compare.is_some() {
                        compare::show(self, ui, &palette);
                    } else {
                        docks::show(self, ui, &palette);
                    }
                });
        }

        self.diagnostics_window(&ctx);
        self.settings_window(&ctx);
        people::window(self, &ctx, &palette);
        batch::window(self, &ctx, &palette);
        describe::window(self, &ctx, &palette);
        filter::window(self, &ctx, &palette);
        self.ask_window(&ctx);

        // The wishlist is overwritten only here, once it is clear what is
        // visible and what is selected. Anything not on it stops being
        // decoded.
        self.images.wish(vec![
            // The People window's photographs go before anything else: it
            // is the window in front of somebody, and a face chip that
            // arrives after the tiles behind it is a window that fills in
            // backwards.
            std::mem::take(&mut self.wanted_faces)
                .into_iter()
                .map(|path| (path, Want::Face))
                .collect(),
            std::mem::take(&mut self.wanted_quick)
                .into_iter()
                .map(|path| (path, Want::Quick))
                .collect(),
            std::mem::take(&mut self.wanted_close)
                .into_iter()
                .map(|path| (path, Want::Close))
                .collect(),
            std::mem::take(&mut self.wanted_preview)
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

            // The direction is a triangle and not a sentence. "Oldest and
            // smallest first" is true of a date sort, true of a size sort
            // and wrong about a name sort, and it took a quarter of the
            // toolbar to be wrong in. The words are in the hover text for
            // whoever wants them.
            let descending = self.settings.gallery.sort_descending;
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::click());
            theme::caret(
                ui.painter(),
                rect.center(),
                descending,
                theme::color(if response.hovered() {
                    palette.accent
                } else {
                    palette.text
                }),
            );
            let way = if descending {
                t!("toolbar-sort-descending")
            } else {
                t!("toolbar-sort-ascending")
            };
            if response.on_hover_text(way).clicked() {
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

            let mut search = self.filter.search.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut search)
                        .desired_width(150.0)
                        .hint_text(t!("filter-search")),
                )
                .changed()
            {
                let mut filter = self.filter.clone();
                filter.search = search;
                self.set_filter(filter);
            }

            // The button says what is set rather than saying "Filter". Half a
            // folder missing with nothing on screen to say why is the worst
            // thing a filter can do.
            let described = self.filter.describe();
            let active = self.filter.is_active();
            let label = egui::RichText::new(described).color(theme::color(if active {
                palette.accent
            } else {
                palette.text
            }));
            if ui.button(label).clicked() {
                self.show_filter = !self.show_filter;
            }

            ui.separator();

            // The right-hand side is claimed first and the folder gets what
            // is left, truncated. The path is the one thing here that can be
            // any length at all: laid out first it pushed the count and the
            // task off the edge, and they drew on top of each other.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.count() != self.total() {
                    ui.label(
                        egui::RichText::new(t!(
                            "filter-showing",
                            shown = self.count() as i64,
                            all = self.total() as i64
                        ))
                        .color(theme::color(palette.accent)),
                    );
                }

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

                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let folder = self
                        .folder
                        .as_ref()
                        .map(|folder| folder.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(folder).color(theme::color(palette.dim)),
                        )
                        .truncate(),
                    );
                });
            });
        });
    }

    /// Asks for a name — for a rename, or for a new folder.
    ///
    /// One dialog for both, because they ask the same question and a second
    /// one would be a second place for Enter and Escape to behave slightly
    /// differently.
    fn ask_window(&mut self, ctx: &egui::Context) {
        let Some(asking) = self.asking.clone() else {
            return;
        };

        let (title, mut name) = match &asking {
            files::Asking::Rename { name, .. } => (t!("ask-rename"), name.clone()),
            files::Asking::NewFolder { name, .. } => (t!("ask-new-folder"), name.clone()),
        };

        let mut open = true;
        let mut go = false;
        // The window's own close button and the Cancel button both mean the
        // same thing, but egui already holds `open` while the contents are
        // drawn, so the button says so through a second flag.
        let mut cancelled = false;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("ask-name"));
                    let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
                    field.request_focus();
                    if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        go = true;
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button(t!("ask-confirm")).clicked() {
                        go = true;
                    }

                    if ui.button(t!("ask-cancel")).clicked() {
                        cancelled = true;
                    }
                });
            });

        if !open || cancelled {
            self.asking = None;
            return;
        }

        if !go {
            // Keep what has been typed so far.
            self.asking = Some(match asking {
                files::Asking::Rename { path, .. } => files::Asking::Rename { path, name },
                files::Asking::NewFolder { inside, .. } => {
                    files::Asking::NewFolder { inside, name }
                }
            });
            return;
        }

        self.asking = None;
        match asking {
            files::Asking::Rename { path, .. } => {
                let outcome = files::rename(self, &path, &name);
                self.did(outcome, |_| t!("files-renamed"));
                self.reopen();
            }
            files::Asking::NewFolder { inside, .. } => match files::new_folder(&inside, &name) {
                // Straight into it: making a folder and staying put means
                // hunting for it in the tree afterwards.
                Ok(made) => self.open(made),
                Err(error) => {
                    let error = format!("{error:#}");
                    tracing::error!(%error, "the folder could not be made");
                    self.status = error;
                }
            },
        }
    }

    /// Where we are, and how to get somewhere else.
    ///
    /// Its own row rather than more on the toolbar: the path is the one thing
    /// here of no fixed width, and a folder twelve deep would push everything
    /// else off the edge.
    fn where_we_are(
        &mut self,
        ui: &mut egui::Ui,
        palette: &palettes::Palette,
        ctx: &egui::Context,
    ) {
        ui.horizontal(|ui| {
            for (id, backwards) in [("go.back", true), ("go.forward", false)] {
                let allowed = if backwards {
                    self.history.can_go_back()
                } else {
                    self.history.can_go_forward()
                };
                let title = commands::command(id)
                    .map(|command| command.title())
                    .unwrap_or_default();
                if ui.add_enabled(allowed, egui::Button::new(title)).clicked() {
                    self.run(id, ctx);
                }
            }

            let can_go_up = self.folder.as_deref().and_then(History::up_from).is_some();
            let up = commands::command("go.up")
                .map(|command| command.title())
                .unwrap_or_default();
            if ui.add_enabled(can_go_up, egui::Button::new(up)).clicked() {
                self.run("go.up", ctx);
            }

            ui.separator();

            // What just happened goes on this row rather than the toolbar.
            // The toolbar is a dozen buttons and a search box wide already,
            // and a message of any length beside them draws straight over
            // the count: that row is claimed from the right, and whatever
            // does not fit overflows leftwards on top of everything else.
            // Here it sits beside the folder it is about.
            //
            // Its room is taken **before** the trail, and the trail gets
            // what is left. The other way round, a scroll area takes the
            // whole row and the message has nowhere to go.
            let said = std::mem::take(&mut self.status);
            let width = ui
                .painter()
                .layout_no_wrap(
                    said.clone(),
                    egui::FontId::proportional(14.0),
                    theme::color(palette.dim),
                )
                .size()
                .x;
            let room = ui.available_width();
            // Never more than half the row: a long message must not squeeze
            // the folder we are standing in down to nothing.
            let for_message = width.min(room * 0.5).max(0.0);
            let for_trail = (room - for_message - 12.0).max(60.0);

            // The trail, outermost first. Each part opens the folder it names.
            let mut pick = None;
            let trail = self
                .folder
                .as_deref()
                .map(History::trail)
                .unwrap_or_default();
            let last = trail.len().saturating_sub(1);
            ui.allocate_ui_with_layout(
                egui::Vec2::new(for_trail, ui.available_height()),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (at, part) in trail.iter().enumerate() {
                                    let name = part
                                        .file_name()
                                        .map(|name| name.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| part.to_string_lossy().into_owned());
                                    let text = egui::RichText::new(name).color(theme::color(
                                        if at == last {
                                            palette.text
                                        } else {
                                            palette.dim
                                        },
                                    ));
                                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                        pick = Some(part.clone());
                                    }

                                    if at != last {
                                        ui.label(
                                            egui::RichText::new(SEPARATOR_MARK)
                                                .color(theme::color(palette.dim)),
                                        );
                                    }
                                }
                            });
                        });
                },
            );

            let mut restart = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // A downloaded release is offered here and nowhere louder.
                // It waits: whoever does not click gets it on the next
                // start anyway.
                if let Some(ready) = self.updates.ready.as_ref() {
                    restart = ui.button(t!("updates-restart")).clicked();
                    ui.label(
                        egui::RichText::new(t!("updates-ready", version = ready.version.as_str()))
                            .color(theme::color(palette.accent)),
                    );
                    ui.separator();
                }

                ui.add(
                    egui::Label::new(egui::RichText::new(&said).color(theme::color(palette.dim)))
                        .truncate(),
                );
            });
            self.status = said;

            if restart {
                // The process ends inside `restart`, so `Drop` never runs:
                // what it would have saved is saved here.
                if let Err(error) = self.settings.save(&self.paths) {
                    tracing::error!(error = %format!("{error:#}"), "the settings could not be saved");
                }
                self.updates.restart();
            }

            if let Some(folder) = pick {
                self.open(folder);
            }
        });
    }

    fn diagnostics_window(&mut self, ctx: &egui::Context) {
        if !self.show_diagnostics {
            return;
        }

        let mut rows = diagnostics::about(&self.paths);
        rows.push((t!("diagnostics-photos"), self.total().to_string()));
        rows.push((t!("diagnostics-showing"), self.count().to_string()));
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
        rows.push((t!("diagnostics-updates"), self.updates.state.describe()));

        // What the two downloaded things add up to. This is the first
        // question a support conversation opens with, and "the models are
        // missing" is exactly what somebody needs told here rather than
        // discovered from a People window that will not start.
        let models = self.model_folder();
        let availability = photosite_faces::Availability::of(&models);
        rows.push((
            t!("diagnostics-models"),
            if availability.missing.is_empty() {
                models.display().to_string()
            } else {
                t!(
                    "diagnostics-missing",
                    missing = availability.missing.join(", ")
                )
            },
        ));
        rows.push((
            t!("diagnostics-places"),
            match &self.places {
                Some(places) => format!("{} ({})", places.source, places.places),
                None => self.paths.places().display().to_string(),
            },
        ));
        if let Some(catalog) = self.catalog.as_ref()
            && let Ok((faces, named, people)) = catalog.face_counts()
        {
            rows.push((
                t!("diagnostics-faces"),
                t!("diagnostics-faces-of", named = named, faces = faces),
            ));
            rows.push((t!("diagnostics-people"), people.to_string()));
        }
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
                    Kind::Text | Kind::Choice(_) | Kind::Secret => {
                        let mut value = self
                            .settings
                            .get(tunable.path)
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .unwrap_or_default();
                        let hidden = tunable.kind == Kind::Secret;
                        if ui
                            .add(egui::TextEdit::singleline(&mut value).password(hidden))
                            .changed()
                        {
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

        if self.count() == 0 {
            println!("{}", t!("selftest-no-photos"));
            std::process::exit(2);
        }

        if self.blank == 0 {
            println!(
                "{}",
                t!(
                    "selftest-ok",
                    count = self.count() as i64,
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
    pub fn app_over(files: &[(&str, usize)]) -> (App, tempfile::TempDir, tempfile::TempDir) {
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

    pub fn three() -> (App, tempfile::TempDir, tempfile::TempDir) {
        app_over(&[("a.jpg", 30), ("b.jpg", 20), ("c.jpg", 10)])
    }

    fn names(app: &App) -> Vec<String> {
        app.visible
            .iter()
            .filter_map(|at| app.all.get(*at))
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
            .filter_map(|at| app.photo(*at))
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
        assert!(app.visible.iter().all(|at| app.all[*at].id.0 > 0));
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

        assert_eq!(app.photo(1).unwrap().organisation.rating, 4);
        let path = app.photo(1).unwrap().path.clone();
        let written = app
            .catalog
            .as_ref()
            .expect("no catalogue")
            .by_path(&path)
            .expect("cannot read")
            .expect("no row");
        assert_eq!(written.organisation.rating, 4);
    }

    /// The catalogue and the queue move together, or the file and the
    /// catalogue come apart.
    #[test]
    fn saying_something_queues_the_file_to_be_written() {
        let (mut app, _data, _photos) = three();
        let catalog = app.catalog.as_ref().expect("no catalogue");
        assert_eq!(catalog.outbox().unwrap(), (0, 0));

        app.select_only(0);
        app.select_through(1);
        app.run_for_test("photo.rate_4");

        let catalog = app.catalog.as_ref().expect("no catalogue");
        assert_eq!(
            catalog.outbox().unwrap().0,
            2,
            "the stars went into the catalogue but nowhere near the files"
        );
    }

    /// The verdict is ours alone — there is no XMP property for it and
    /// inventing one would be a private dialect nothing else reads.
    #[test]
    fn a_verdict_is_kept_here_and_not_pushed_into_the_file() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("photo.pick");
        assert_eq!(app.photo(0).unwrap().organisation.flag, Flag::Picked);
        assert_eq!(
            app.catalog.as_ref().unwrap().outbox().unwrap().0,
            0,
            "a verdict has nothing to write"
        );
    }

    #[test]
    fn the_stars_go_on_everything_selected() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.select_through(2);
        app.run_for_test("photo.rate_5");
        assert!(
            app.visible
                .iter()
                .all(|at| app.all[*at].organisation.rating == 5)
        );
    }

    #[test]
    fn a_pick_pressed_twice_takes_itself_off() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("photo.pick");
        assert_eq!(app.photo(0).unwrap().organisation.flag, Flag::Picked);

        app.run_for_test("photo.pick");
        assert_eq!(app.photo(0).unwrap().organisation.flag, Flag::None);
    }

    #[test]
    fn rejecting_something_picked_rejects_it_rather_than_clearing_it() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("photo.pick");
        app.run_for_test("photo.reject");
        assert_eq!(app.photo(0).unwrap().organisation.flag, Flag::Rejected);
    }

    #[test]
    fn nothing_selected_means_nothing_written() {
        let (mut app, _data, _photos) = three();
        app.run_for_test("photo.rate_5");
        assert!(
            app.visible
                .iter()
                .all(|at| app.all[*at].organisation.rating == 0)
        );
    }

    #[test]
    fn back_and_forward_walk_the_folders_visited() {
        let (mut app, _data, photos) = three();
        let first = photos.path().to_path_buf();
        let deeper = first.join("deeper");
        std::fs::create_dir(&deeper).unwrap();

        app.open(deeper.clone());
        assert_eq!(app.folder.as_deref(), Some(deeper.as_path()));

        app.run_for_test("go.back");
        assert_eq!(app.folder.as_deref(), Some(first.as_path()));
        assert_eq!(app.count(), 3, "the folder came back without its contents");

        app.run_for_test("go.forward");
        assert_eq!(app.folder.as_deref(), Some(deeper.as_path()));
    }

    #[test]
    fn up_goes_up_and_stops_at_the_top() {
        let (mut app, _data, photos) = three();
        let deeper = photos.path().join("deeper");
        std::fs::create_dir(&deeper).unwrap();
        app.open(deeper);

        app.run_for_test("go.up");
        assert_eq!(app.folder.as_deref(), Some(photos.path()));
    }

    /// The one this slice is for. People rename files constantly and would
    /// never think to be careful about it.
    #[test]
    fn renaming_a_photograph_keeps_everything_said_about_it() {
        let (mut app, _data, photos) = three();
        app.select_only(0);
        app.run_for_test("photo.rate_5");
        let before = app.photo(0).unwrap().path.clone();

        files::rename(&mut app, &before, "renamed.jpg").expect("cannot rename");
        app.reopen();

        assert!(!before.exists(), "the old file is still there");
        let renamed = photos.path().join("renamed.jpg");
        assert!(renamed.exists());

        let photo = app
            .all
            .iter()
            .find(|photo| photo.path == renamed)
            .expect("the renamed photograph is not in the folder");
        assert_eq!(photo.organisation.rating, 5, "the stars did not follow");
        assert_eq!(app.total(), 3, "a second row was started");
    }

    #[test]
    fn a_rename_that_cannot_happen_leaves_the_file_alone() {
        let (mut app, _data, photos) = three();
        let path = photos.path().join("a.jpg");

        // Onto a name already taken, and onto something that is a path.
        assert!(files::rename(&mut app, &path, "b.jpg").is_err());
        assert!(files::rename(&mut app, &path, "../elsewhere.jpg").is_err());
        assert!(files::rename(&mut app, &path, "   ").is_err());
        assert!(path.exists(), "the file was moved anyway");
    }

    #[test]
    fn duplicating_leaves_the_original_where_it_was() {
        let (mut app, _data, photos) = three();
        app.select_only(0);
        app.run_for_test("file.duplicate");

        assert!(photos.path().join("a.jpg").exists());
        assert!(photos.path().join("a (2).jpg").exists());
        assert_eq!(app.total(), 4, "the copy is not in the folder");
    }

    #[test]
    fn a_file_operation_with_nothing_selected_says_so_rather_than_guessing() {
        let (mut app, _data, photos) = three();
        app.run_for_test("file.duplicate");
        assert_eq!(app.total(), 3);
        assert!(!photos.path().join("a (2).jpg").exists());
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

    /// The one that would be quiet and expensive to get wrong. With a
    /// filter on, position nought in the gallery is not row nought in the
    /// folder — and the rating keys work in positions.
    #[test]
    fn a_rating_lands_on_what_is_shown_and_not_on_what_is_hidden() {
        let (mut app, _data, _photos) = three();
        app.set_filter(Filter {
            search: "c.jpg".to_owned(),
            ..Default::default()
        });
        assert_eq!(app.count(), 1);
        assert_eq!(app.total(), 3);

        app.select_only(0);
        app.run_for_test("photo.rate_5");

        assert_eq!(app.photo(0).unwrap().organisation.rating, 5);
        let rated: Vec<&str> = app
            .all
            .iter()
            .filter(|photo| photo.organisation.rating == 5)
            .map(|photo| photo.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(rated, ["c.jpg"], "the stars went on the wrong photograph");
    }

    /// Copy here, paste there. The originals stay where they were — that is
    /// the whole difference between a copy and a move, and getting it the
    /// wrong way round loses photographs.
    #[test]
    fn copying_and_pasting_brings_the_files_and_leaves_the_originals() {
        let (mut app, _data, photos) = three();
        let elsewhere = tempfile::tempdir().unwrap();
        app.select_only(0);
        app.select_also(1);
        app.run_for_test("file.copy");

        app.open(elsewhere.path().to_owned());
        app.run_for_test("file.paste");

        assert!(elsewhere.path().join("a.jpg").exists());
        assert!(elsewhere.path().join("b.jpg").exists());
        assert!(
            photos.path().join("a.jpg").exists(),
            "the original was taken"
        );
        assert_eq!(app.total(), 2, "the folder does not show what arrived");
    }

    #[test]
    fn cutting_and_pasting_moves_them_and_spends_the_clipboard() {
        let (mut app, _data, photos) = three();
        let elsewhere = tempfile::tempdir().unwrap();
        app.select_only(0);
        app.run_for_test("file.cut");

        app.open(elsewhere.path().to_owned());
        app.run_for_test("file.paste");

        assert!(elsewhere.path().join("a.jpg").exists());
        assert!(!photos.path().join("a.jpg").exists(), "the original stayed");

        // A cut is spent. Pressing Ctrl+V again would otherwise try to move
        // the same photograph out of a folder it is no longer in.
        app.run_for_test("file.paste");
        assert!(!app.status.is_empty());
        assert_eq!(app.total(), 1, "it was pasted twice");
    }

    /// The catalogue follows a move, or the stars are left on a row pointing
    /// at a file that is not there any more.
    #[test]
    fn what_was_said_about_a_photograph_survives_a_move() {
        let (mut app, _data, _photos) = three();
        let elsewhere = tempfile::tempdir().unwrap();
        app.select_only(0);
        app.run_for_test("photo.rate_5");
        app.run_for_test("file.cut");

        app.open(elsewhere.path().to_owned());
        app.run_for_test("file.paste");

        assert_eq!(app.count(), 1);
        assert_eq!(
            app.photo(0).unwrap().organisation.rating,
            5,
            "the stars did not travel with the photograph"
        );
    }

    #[test]
    fn copying_to_the_last_folder_needs_a_last_folder() {
        let (mut app, _data, photos) = three();
        app.select_only(0);
        app.run_for_test("file.copy_again");
        assert!(
            !app.status.is_empty(),
            "nothing was said about having nowhere to go"
        );

        let elsewhere = tempfile::tempdir().unwrap();
        app.settings.gallery.last_destination = Some(elsewhere.path().display().to_string());
        app.run_for_test("file.copy_again");
        assert!(elsewhere.path().join("a.jpg").exists());
        assert!(photos.path().join("a.jpg").exists());
    }

    /// Reloading the folder used to say "3 photographs in 2 ms" over the top
    /// of whatever had just happened, which is not an answer to "did that
    /// work?".
    #[test]
    fn what_happened_survives_the_folder_being_read_again() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("file.duplicate");
        assert!(
            !app.status.contains("ms"),
            "the reload spoke over the operation: {}",
            app.status
        );
        assert_eq!(app.total(), 4);
    }

    #[test]
    fn pasting_with_nothing_held_says_so_rather_than_doing_nothing() {
        let (mut app, _data, _photos) = three();
        app.run_for_test("file.paste");
        assert!(!app.status.is_empty());
        assert_eq!(app.total(), 3);
    }

    #[test]
    fn a_comparison_needs_two_and_takes_at_most_four() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.run_for_test("photo.compare");
        assert!(app.compare.is_none(), "one photograph is not a comparison");
        assert!(!app.status.is_empty(), "and nothing was said about it");

        app.select_also(1);
        app.run_for_test("photo.compare");
        assert_eq!(app.compare.as_ref().map(Compare::len), Some(2));

        // The same key again closes it. Nobody should have to hunt for the
        // way out of a view that filled the window.
        app.run_for_test("photo.compare");
        assert!(app.compare.is_none());
    }

    #[test]
    fn the_focus_moves_round_the_comparison() {
        let (mut app, _data, _photos) = three();
        app.run_for_test("photo.select_all");
        app.run_for_test("photo.compare");

        app.run_for_test("photo.compare_next");
        assert_eq!(compared(&app), "b.jpg");
        app.run_for_test("photo.compare_previous");
        assert_eq!(compared(&app), "a.jpg");
        app.run_for_test("photo.compare_previous");
        assert_eq!(compared(&app), "c.jpg", "the focus fell off the front");
    }

    /// The one that would be expensive to get wrong. Delete in a comparison
    /// means "take this out of what I am looking at" — the file is on disk
    /// and nobody asked for it to go anywhere.
    #[test]
    fn delete_in_a_comparison_touches_the_comparison_and_not_the_disk() {
        let (mut app, _data, photos) = three();
        app.run_for_test("photo.select_all");
        app.run_for_test("photo.compare");
        assert_eq!(compared(&app), "a.jpg");

        app.run_for_test("file.delete");
        assert!(photos.path().join("a.jpg").exists(), "the file was deleted");
        assert_eq!(app.total(), 3, "the folder lost a photograph");
        assert_eq!(app.compare.as_ref().map(Compare::len), Some(2));
        assert_eq!(compared(&app), "b.jpg");

        // Down to one, and it is not a comparison any more.
        app.run_for_test("file.delete");
        assert!(app.compare.is_none());
        assert_eq!(app.total(), 3);
        assert!(photos.path().join("b.jpg").exists());
    }

    /// The keys are the gallery's keys. What they land on is whatever is
    /// being looked at — which in a comparison is one photograph, not the
    /// three that were selected when it opened.
    #[test]
    fn the_rating_keys_land_on_the_focused_photograph_alone() {
        let (mut app, _data, _photos) = three();
        app.run_for_test("photo.select_all");
        app.run_for_test("photo.compare");
        app.run_for_test("photo.compare_next");
        app.run_for_test("photo.rate_4");

        let rated: Vec<String> = app
            .all
            .iter()
            .filter(|photo| photo.organisation.rating == 4)
            .map(|photo| {
                photo
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(rated, ["b.jpg"], "the stars went on the whole selection");

        // And with the comparison closed they land on the selection again.
        app.run_for_test("photo.compare");
        app.run_for_test("photo.rate_1");
        assert!(
            app.all.iter().all(|photo| photo.organisation.rating == 1),
            "the selection stopped being what the keys act on"
        );
    }

    #[test]
    fn going_somewhere_else_closes_the_comparison() {
        let (mut app, _data, photos) = three();
        app.run_for_test("photo.select_all");
        app.run_for_test("photo.compare");
        assert!(app.compare.is_some());

        app.open(photos.path().to_owned());
        assert!(app.compare.is_none(), "it outlived the folder it came from");
    }

    /// Runs one frame of the comparison against a real context, with the
    /// pointer somewhere and the wheel turned.
    ///
    /// The arithmetic is tested where it lives; what this proves is the
    /// wiring — that the cell under the pointer is the one that answers, and
    /// that its answer reaches the shared view.
    fn a_frame_of_comparison(app: &mut App, ctx: &egui::Context, pointer: egui::Pos2, wheel: f32) {
        let palette = *app.palette();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        input.events.push(egui::Event::PointerMoved(pointer));
        if wheel != 0.0 {
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, wheel),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });
        }

        let mut out = ctx.run_ui(input, |ui| compare::show(app, ui, &palette));
        // With no renderer nothing is ever uploaded, and epaint says so with
        // a panic when the output is dropped.
        out.textures_delta.clear();
    }

    /// One view, shared. The wheel over either cell moves both, because
    /// there is only one thing to move.
    #[test]
    fn the_wheel_over_a_cell_reaches_the_shared_view() {
        let (mut app, _data, _photos) = three();
        for photo in &mut app.all {
            photo.width = Some(6000);
            photo.height = Some(4000);
            photo.orientation = 1;
        }

        app.select_only(0);
        app.select_also(1);
        app.run_for_test("photo.compare");

        let ctx = egui::Context::default();
        // Twice: the first frame is where the cells come into being, so
        // there is nothing to be hovering over until the second.
        for _ in 0..2 {
            a_frame_of_comparison(&mut app, &ctx, egui::pos2(300.0, 400.0), 300.0);
        }

        let view = app.compare.as_ref().expect("the comparison closed").view;
        assert!(
            view.zoom > 1.0,
            "the wheel never reached the view: {view:?}"
        );

        // And with the pointer outside the cells nothing moves, or the wheel
        // would be a global gesture rather than one aimed at a photograph.
        let before = view;
        for _ in 0..2 {
            a_frame_of_comparison(&mut app, &ctx, egui::pos2(1199.0, 1.0), 300.0);
        }

        assert_eq!(
            app.compare.as_ref().unwrap().view,
            before,
            "the wheel moved the view from outside every cell"
        );
    }

    /// Which photograph the focused path is, by name.
    fn compared(app: &App) -> String {
        app.compare
            .as_ref()
            .expect("no comparison is open")
            .focused()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_filter_narrows_what_is_shown_and_leaves_the_folder_alone() {
        let (mut app, _data, _photos) = three();
        app.set_filter(Filter {
            minimum_rating: 1,
            ..Default::default()
        });
        assert_eq!(app.count(), 0);
        assert_eq!(app.total(), 3, "the folder is still the folder");

        app.set_filter(Filter::default());
        assert_eq!(app.count(), 3);
    }

    #[test]
    fn what_the_filter_hides_leaves_the_selection() {
        let (mut app, _data, _photos) = three();
        app.select_only(0);
        app.select_through(2);
        assert_eq!(app.selection.len(), 3);

        // Deliberate: the selection is what is in front of you. Keeping
        // hidden photographs in it means the next rating lands somewhere
        // nobody can see.
        app.set_filter(Filter {
            search: "b.jpg".to_owned(),
            ..Default::default()
        });
        assert_eq!(app.selection.len(), 1);
        assert_eq!(
            app.photo(app.selected.unwrap())
                .unwrap()
                .path
                .file_name()
                .unwrap(),
            "b.jpg"
        );

        app.set_filter(Filter::default());
        assert_eq!(app.selection.len(), 1, "hidden ones do not come back");
    }

    #[test]
    fn the_facets_come_from_the_folder_and_not_from_what_is_showing() {
        let (mut app, _data, _photos) = three();
        let all = app.facets.clone();
        app.set_filter(Filter {
            search: "a.jpg".to_owned(),
            ..Default::default()
        });
        assert_eq!(app.count(), 1);
        assert_eq!(
            app.facets, all,
            "narrowing the gallery must not narrow what can be asked"
        );
    }

    #[test]
    fn clearing_the_filter_puts_the_folder_back() {
        let (mut app, _data, _photos) = three();
        app.filter_from = "2024-01-01".to_owned();
        app.set_filter(Filter {
            minimum_rating: 4,
            search: "nothing".to_owned(),
            ..Default::default()
        });
        assert_eq!(app.count(), 0);

        app.run_for_test("view.clear_filter");
        assert_eq!(app.count(), 3);
        assert!(!app.filter.is_active());
        assert!(app.filter_from.is_empty(), "the typed dates go too");
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

/// The editor tabs over a real gallery: opening, paging, and the way back.
///
/// Headless, like the culling tests above: nothing here decodes a pixel.
/// What is checked is that the tab follows the order of the gallery, and
/// that going back lands the manager on the right tile.
#[cfg(test)]
mod editing {
    use super::*;

    fn shown(app: &App) -> Option<String> {
        app.tabs.active_editor().and_then(|editor| {
            editor
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
    }

    #[test]
    fn a_tile_opens_in_a_tab_of_its_own_and_the_keys_follow() {
        let (mut app, _data, _photos) = culling::three();
        assert_eq!(app.scope(), Scope::Manager);
        app.edit(1);
        assert_eq!(shown(&app), Some("b.jpg".to_owned()));
        assert!(!app.tabs.manager_is_active());
        assert_eq!(app.scope(), Scope::Editor);
        assert_eq!(
            app.bindings
                .command_for(&"Ctrl+F".parse().unwrap(), app.scope())
                .map(|command| command.id),
            Some("editor.fullscreen")
        );
    }

    #[test]
    fn enter_opens_the_tile_the_cursor_is_on() {
        let (mut app, _data, _photos) = culling::three();
        let ctx = egui::Context::default();
        app.run("photo.edit", &ctx);
        assert!(app.tabs.manager_is_active(), "nothing was chosen");

        app.select_only(2);
        app.run("photo.edit", &ctx);
        assert_eq!(shown(&app), Some("c.jpg".to_owned()));
    }

    #[test]
    fn paging_follows_the_order_of_the_gallery_and_stops_at_the_ends() {
        let (mut app, _data, _photos) = culling::three();
        app.edit(0);
        app.page_editor(1);
        assert_eq!(shown(&app), Some("b.jpg".to_owned()));
        app.page_editor(1);
        assert_eq!(shown(&app), Some("c.jpg".to_owned()));
        app.page_editor(1);
        assert_eq!(shown(&app), Some("c.jpg".to_owned()), "the end is the end");
        app.page_editor(-1);
        assert_eq!(shown(&app), Some("b.jpg".to_owned()));
        // The same tab throughout, not one per photograph.
        assert_eq!(app.tabs.count(), 2);
    }

    #[test]
    fn going_back_lands_the_manager_on_the_photograph() {
        let (mut app, _data, _photos) = culling::three();
        let ctx = egui::Context::default();
        app.select_only(0);
        app.edit(0);
        app.page_editor(1);
        app.page_editor(1);
        app.run("editor.back", &ctx);
        assert!(app.tabs.manager_is_active());
        assert_eq!(app.tabs.count(), 1);
        assert_eq!(app.selected, Some(2));
        assert_eq!(app.selection.iter().copied().collect::<Vec<_>>(), [2]);
        assert_eq!(app.scroll_grid_to, Some(2));
    }

    #[test]
    fn closing_leaves_the_selection_where_it_was() {
        let (mut app, _data, _photos) = culling::three();
        let ctx = egui::Context::default();
        app.select_only(1);
        app.edit(1);
        app.page_editor(1);
        app.run("editor.close", &ctx);
        assert!(app.tabs.manager_is_active());
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.scroll_grid_to, None);
    }

    #[test]
    fn going_back_from_another_folder_opens_that_folder() {
        let (mut app, _data, photos) = culling::three();
        let ctx = egui::Context::default();
        let elsewhere = photos.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let there = elsewhere.join("d.jpg");
        std::fs::write(&there, b"xxxx").unwrap();

        app.tabs.open(there.clone());
        app.run("editor.back", &ctx);
        assert_eq!(app.folder.as_deref(), Some(elsewhere.as_path()));
        assert_eq!(
            app.photo_at_cursor().map(|photo| photo.path.clone()),
            Some(there)
        );
        assert_eq!(app.scroll_tree_to.as_deref(), Some(elsewhere.as_path()));
    }

    #[test]
    fn the_editor_fullscreen_is_left_with_the_editor() {
        let (mut app, _data, _photos) = culling::three();
        let ctx = egui::Context::default();
        app.edit(0);
        app.run("editor.fullscreen", &ctx);
        assert!(app.tabs.fullscreen);
        app.run("editor.close", &ctx);
        assert!(!app.tabs.fullscreen);
        assert!(
            !app.fullscreen,
            "the window's own fullscreen was never asked for"
        );
    }
}
