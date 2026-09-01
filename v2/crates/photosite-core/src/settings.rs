//! Settings.
//!
//! Four principles worth keeping from the start, because every one of them is
//! expensive to introduce afterwards:
//!
//! 1. **Defaults live in code, once.** `Default` is the single source of
//!    truth. Whoever set nothing gets what we consider right — and when we
//!    change our minds, they get that too without having to delete anything.
//! 2. **Only what differs goes into the file.** Saving the whole tree means
//!    freezing today's defaults for everyone who ever started the
//!    application. A sparse file is also readable, and shows at a glance what
//!    somebody has changed.
//! 3. **Everything has a path.** `gallery.tile_size` can be read and written
//!    as text, so a settings screen can one day be generated over it without
//!    code being written per entry.
//! 4. **Reset is first class.** One entry, a whole group, or everything. When
//!    something can be safely undone, people dare to experiment.
//!
//! Ready-made sets of settings are deliberately **not** here. There were
//! some, and they were premature: one of them literally copied the defaults,
//! so improving those would have quietly left it on the old ones, and for the
//! rest there was no telling whether anybody would want them. Resetting to
//! the defaults covers "give me back something sensible" entirely. Once there
//! are enough settings for combinations to make sense, they will be data in a
//! file, not constants in code.

use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub window: Window,
    pub gallery: Gallery,
    pub loading: Loading,
    pub appearance: Appearance,
    pub faces: Faces,
    pub ai: Ai,
}

/// The window state. That the application opens where somebody left it is
/// one of the first things they notice — even when they do not notice it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Window {
    pub width: f64,
    pub height: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub maximized: bool,
    /// The dock layout. See [`crate::docks`] — one line, not nested tables,
    /// so it can be read and corrected by hand.
    pub layout: String,
    /// The hidden panes, comma separated.
    pub docks_hidden: String,
    /// The thickness of the splitter between docks. This too is a setting,
    /// not a constant in the drawing layer — on a touch screen six points is
    /// not enough.
    pub splitter: f64,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1600.0,
            height: 1000.0,
            x: None,
            y: None,
            maximized: false,
            layout: crate::docks::DEFAULT.to_owned(),
            docks_hidden: String::new(),
            splitter: 6.0,
        }
    }
}

/// What the grid looks like. None of it is a constant in code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gallery {
    pub tile_size: f64,
    /// The gap between tiles.
    pub gap: f64,
    /// The height of the image area against the width of the tile.
    pub tile_aspect: f64,
    /// The height of the caption strip under the photograph.
    pub caption_height: f64,
    /// How much room the frame leaves around the photograph.
    pub tile_padding: f64,
    /// How many rows above and below the viewport are loaded ahead.
    pub prefetch_rows: i64,
    pub show_captions: bool,
    /// The folder as a list of rows rather than a wall of tiles.
    pub as_list: bool,
    pub recursive: bool,
    /// What the gallery is ordered by, under the stable name from
    /// [`crate::domain::SortField::id`]. A name rather than a number, so the
    /// file stays readable and reordering the enum cannot silently change
    /// what somebody chose.
    pub sort_field: String,
    pub sort_descending: bool,
    pub last_folder: Option<String>,
    /// Where the Map button goes, as an address holding `{lat}` and `{lon}`.
    ///
    /// A template rather than a fixed map, because which one somebody wants
    /// is not ours to decide: it differs by country, by habit and by whether
    /// they have an account anywhere. OpenStreetMap is the one that needs
    /// none of those.
    pub map_url: String,
    /// Where the last copy or move went. Remembered so that sorting a folder
    /// into piles is one dialog and then a key, rather than one dialog for
    /// every photograph.
    pub last_destination: Option<String>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            tile_size: 220.0,
            gap: 10.0,
            tile_aspect: 0.72,
            caption_height: 22.0,
            tile_padding: 7.0,
            prefetch_rows: 3,
            show_captions: true,
            as_list: false,
            recursive: false,
            // Date taken, oldest first: the order the photographs happened
            // in, which is the one nobody has to think about.
            sort_field: crate::domain::SortField::TakenAt.id().to_owned(),
            sort_descending: false,
            last_folder: None,
            last_destination: None,
            map_url: crate::place::OPENSTREETMAP.to_owned(),
        }
    }
}

/// Image loading. These values decide how smooth it feels, so they have to
/// be adjustable without a recompile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Loading {
    /// The longer edge of a tile in pixels.
    pub thumb_size: i64,
    /// The longer edge of the full preview.
    pub preview_size: i64,
    /// The longer edge of a photograph in the comparison.
    ///
    /// Larger than the preview on purpose: a comparison is where somebody
    /// looks at a hundred per cent to decide which frame is the sharp one,
    /// and a preview blown up past its own pixels answers that question
    /// wrongly. At most four of these exist at a time, which is what makes
    /// the size affordable.
    pub compare_size: i64,
    /// How many finished images are uploaded to the GPU per frame. Without a
    /// ceiling, one batch would stall the thread that draws.
    pub uploads_per_frame: i64,
    /// How many textures are kept before the least recently used start to
    /// go.
    pub texture_budget: i64,
    /// Decoding threads; `0` means by the number of cores.
    pub worker_threads: i64,
    /// Use the thumbnail embedded in EXIF until the sharp one is decoded. It
    /// can be switched off mainly so that its benefit can be measured.
    pub use_embedded_thumbnails: bool,
    /// Keep finished tiles on disk so a folder opened before opens at once.
    /// Only the tiles: previews and comparisons are too large to be worth
    /// keeping and are wanted too rarely to be worth finding.
    pub cache_thumbnails: bool,
    /// A safety net: the longest pause between frames when nothing is
    /// happening. The decoding threads announce finished work themselves, so
    /// this only covers the case of a lost wake-up. A short interval means
    /// waking for nothing.
    pub idle_repaint_ms: i64,
}

impl Default for Loading {
    fn default() -> Self {
        Self {
            thumb_size: 320,
            preview_size: 2560,
            compare_size: 4096,
            uploads_per_frame: 32,
            texture_budget: 900,
            worker_threads: 0,
            use_embedded_thumbnails: true,
            cache_thumbnails: true,
            idle_repaint_ms: 1000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    /// The theme key, or `automatic` to follow the system.
    pub theme: String,
    /// Which theme to use when the system reports dark mode.
    pub theme_dark: String,
    /// And which when it reports light.
    pub theme_light: String,
    /// The interface language. An unknown one falls back to `en-US`.
    pub language: String,
    /// The scale of the whole interface.
    pub ui_scale: f64,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "automatic".to_owned(),
            theme_dark: "dark".to_owned(),
            theme_light: "light".to_owned(),
            language: crate::i18n::FALLBACK.to_string(),
            ui_scale: 1.0,
        }
    }
}

/// Finding faces. Everything here is local, and nothing in it reaches a
/// network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Faces {
    /// Where the four ONNX models are. Empty means the ordinary place —
    /// `models` beside the catalogue — which moves with `--data` like
    /// everything else. A path here is for somebody who keeps a hundred
    /// megabytes of models on another disk and does not want a second copy.
    pub models: String,
    /// The longer edge a photograph is decoded at for a scan.
    ///
    /// It is not the detector's own canvas, which is fixed by the model.
    /// This is the frame the faces are cut out of and described from, so a
    /// larger number means better recognition of small faces and a slower
    /// sweep — it is the one number worth turning up on a library of group
    /// photographs.
    pub detect_size: i64,
    /// The longer edge a photograph is decoded at to cut face thumbnails
    /// out of. One decode serves every face on it.
    pub crop_size: i64,
    /// How much of the frame around a face a thumbnail keeps, so that hair
    /// and chin survive the crop.
    pub crop_margin: f64,
    /// Draw the face frames over the preview.
    pub show_frames: bool,
}

impl Default for Faces {
    fn default() -> Self {
        Self {
            models: String::new(),
            detect_size: 1024,
            crop_size: 640,
            crop_margin: 0.35,
            show_frames: true,
        }
    }
}

/// Asking a vision model on this machine to describe a photograph.
///
/// The address defaults to localhost and the whole feature is built around
/// that: a photograph goes to the model as pixels, and a model on somebody
/// else's computer is a different promise entirely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ai {
    pub endpoint: String,
    pub model: String,
    /// What language to write in, as the model is asked for it. A name and
    /// not a code, because that is what a model understands.
    pub language: String,
    /// Overwrite a title and a description that are already there, rather
    /// than only filling in what is empty.
    ///
    /// Filling in the empty ones is the default because it is what makes an
    /// interrupted run over a thousand photographs restartable: everything
    /// already described is skipped.
    pub overwrite: bool,
    /// The longer edge sent to the model. A vision model reads a photograph
    /// comfortably at this size and it keeps the request far smaller than
    /// the file.
    pub request_size: i64,
    /// How long to wait for one answer. A vision model on a modest machine
    /// takes tens of seconds a photograph, so this is minutes rather than
    /// the seconds an HTTP client would default to.
    pub timeout_seconds: i64,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:11434".to_owned(),
            model: String::new(),
            language: "English".to_owned(),
            overwrite: false,
            request_size: 1024,
            timeout_seconds: 300,
        }
    }
}

// -------------------------------------------------------------------- file

impl Settings {
    /// Loads the settings. Never fails because of the file's contents — at
    /// worst the defaults come back and the damaged file is set aside.
    pub fn load(paths: &Paths) -> Self {
        let file = paths.config_file();
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %file.display(), "no settings yet, carrying on with the defaults");
                return Self::default();
            }
            Err(error) => {
                tracing::warn!(path = %file.display(), %error, "the settings cannot be read");
                return Self::default();
            }
        };

        match toml::from_str(&text) {
            Ok(settings) => settings,
            // A setting we removed must not cost the rest of the file.
            Err(error) => match rescue(&text) {
                Some((settings, dropped)) => {
                    tracing::warn!(
                        keys = dropped.join(", "),
                        "the settings hold keys we no longer use; the rest stays"
                    );
                    settings
                }
                None => {
                    let broken = file.with_extension("toml.broken");
                    tracing::error!(
                        path = %file.display(),
                        odlozeno = %broken.display(),
                        %error,
                        "the settings are damaged"
                    );
                    let _ = std::fs::rename(&file, &broken);
                    Self::default()
                }
            },
        }
    }

    /// Saves only what differs from the defaults.
    ///
    /// It writes through a temporary file, so a crash mid-write does not
    /// leave half of one on disk.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let file = paths.config_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let sparse = self.overrides()?;
        let text = if sparse.is_empty() {
            String::from("# Empty: everything is at its default.\n")
        } else {
            toml::to_string_pretty(&sparse).context("the settings cannot be serialised")?
        };

        let temporary = file.with_extension("toml.tmp");
        std::fs::write(&temporary, text)
            .with_context(|| format!("cannot write {}", temporary.display()))?;
        std::fs::rename(&temporary, &file)
            .with_context(|| format!("cannot rename to {}", file.display()))?;
        Ok(())
    }

    /// What differs from the defaults. What gets saved.
    pub fn overrides(&self) -> Result<toml::Table> {
        let mine = toml::Table::try_from(self)?;
        let default = toml::Table::try_from(Settings::default())?;
        Ok(diff(&mine, &default))
    }

    /// The value at a path such as `gallery.tile_size`. For a generic
    /// settings screen.
    pub fn get(&self, path: &str) -> Option<toml::Value> {
        let mut value = toml::Value::try_from(self).ok()?;
        for part in path.split('.') {
            value = value.as_table()?.get(part)?.clone();
        }

        Some(value)
    }

    /// Writes a value at a path. An unknown path or the wrong type is an
    /// error, not a silent nothing — otherwise the settings screen would
    /// quietly not work.
    pub fn set(&mut self, path: &str, value: toml::Value) -> Result<()> {
        let mut root = toml::Value::try_from(&*self)?;
        {
            let mut cursor = &mut root;
            let parts: Vec<&str> = path.split('.').collect();
            let (last, prefix) = parts.split_last().context("empty path")?;
            for part in prefix {
                cursor = cursor
                    .as_table_mut()
                    .and_then(|table| table.get_mut(*part))
                    .with_context(|| format!("the path {path} leads nowhere"))?;
            }

            let table = cursor
                .as_table_mut()
                .with_context(|| format!("the path {path} leads nowhere"))?;
            anyhow::ensure!(table.contains_key(*last), "the path {path} does not exist");
            table.insert((*last).to_owned(), value);
        }

        *self = root
            .try_into()
            .with_context(|| format!("the value at {path} is of the wrong type"))?;
        Ok(())
    }

    /// Returns one entry, or a whole group, to its default.
    pub fn reset(&mut self, path: &str) -> Result<()> {
        let default = Settings::default();
        let value = default
            .get(path)
            .with_context(|| format!("the path {path} does not exist"))?;
        if self
            .get(path)
            .map(|current| current == value)
            .unwrap_or(false)
        {
            return Ok(());
        }

        // A group cannot be set in one write, because `set` expects a leaf.
        match value.as_table() {
            Some(table) => {
                for key in table.keys() {
                    self.reset(&format!("{path}.{key}"))?;
                }
            }
            None => self.set(path, value)?,
        }

        Ok(())
    }

    /// Everything back to the defaults, except what is not a preference —
    /// the last opened folder and the window position are not what a reset
    /// means.
    pub fn reset_all(&mut self) {
        let keep_folder = self.gallery.last_folder.clone();
        let window = self.window.clone();
        *self = Settings::default();
        self.gallery.last_folder = keep_folder;
        self.window = window;
    }
}

/// Settings whose only fault is a key we stopped using.
///
/// Options come and go. If one removed key sent the whole settings file
/// aside, somebody would lose everything else they ever set — and the only
/// thing they did wrong was to use the application earlier. The dropped keys
/// go to the log; not even they may be lost in silence.
fn rescue(text: &str) -> Option<(Settings, Vec<String>)> {
    let mut table: toml::Table = toml::from_str(text).ok()?;
    let known = paths_in_settings();
    let mut dropped = Vec::new();
    prune("", &mut table, &known, &mut dropped);
    if dropped.is_empty() {
        // The keys were not the problem, something else was — text where a
        // number belongs, say. That is not for rescuing.
        return None;
    }

    let settings = toml::Value::Table(table).try_into().ok()?;
    Some((settings, dropped))
}

fn prune(prefix: &str, table: &mut toml::Table, known: &[String], dropped: &mut Vec<String>) {
    table.retain(|key, value| {
        let path = if prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{prefix}.{key}")
        };

        if let toml::Value::Table(nested) = value {
            // A group stays only while something known remains beneath it.
            if !known.iter().any(|it| it.starts_with(&format!("{path}."))) {
                dropped.push(path);
                return false;
            }

            prune(&path, nested, known, dropped);
            return true;
        }

        if known.iter().any(|it| it == &path) {
            return true;
        }

        dropped.push(path);
        false
    });
}

/// A recursive difference of two tables; only what differs is left.
fn diff(mine: &toml::Table, default: &toml::Table) -> toml::Table {
    let mut out = toml::Table::new();
    for (key, value) in mine {
        match (value, default.get(key)) {
            (toml::Value::Table(mine), Some(toml::Value::Table(default))) => {
                let nested = diff(mine, default);
                if !nested.is_empty() {
                    out.insert(key.clone(), toml::Value::Table(nested));
                }
            }
            (value, Some(default)) if value == default => {}
            (value, _) => {
                out.insert(key.clone(), value.clone());
            }
        }
    }

    out
}

// --------------------------------------------------------- field descriptions

/// What kind of value this is. The control will one day be drawn from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Float {
        min: f64,
        max: f64,
    },
    Int {
        min: i64,
        max: i64,
    },
    Text,
    /// A choice of several; the values are keys, not names.
    Choice(&'static [&'static str]),
    /// Not a preference — state the application remembers on its own.
    State,
}

/// One settings entry as a person will one day see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tunable {
    pub path: &'static str,
    /// A translation key. There is no text here either.
    pub label_key: &'static str,
    pub kind: Kind,
}

pub const TUNABLES: &[Tunable] = &[
    Tunable {
        path: "window.width",
        label_key: "setting-window-width",
        kind: Kind::State,
    },
    Tunable {
        path: "window.height",
        label_key: "setting-window-height",
        kind: Kind::State,
    },
    Tunable {
        path: "window.x",
        label_key: "setting-window-x",
        kind: Kind::State,
    },
    Tunable {
        path: "window.y",
        label_key: "setting-window-y",
        kind: Kind::State,
    },
    Tunable {
        path: "window.maximized",
        label_key: "setting-window-maximized",
        kind: Kind::State,
    },
    Tunable {
        path: "window.layout",
        label_key: "setting-window-layout",
        kind: Kind::State,
    },
    Tunable {
        path: "window.docks_hidden",
        label_key: "setting-window-docks-hidden",
        kind: Kind::State,
    },
    Tunable {
        path: "window.splitter",
        label_key: "setting-window-splitter",
        kind: Kind::Float {
            min: 2.0,
            max: 16.0,
        },
    },
    Tunable {
        path: "gallery.tile_size",
        label_key: "setting-tile-size",
        kind: Kind::Float {
            min: 80.0,
            max: 640.0,
        },
    },
    Tunable {
        path: "gallery.gap",
        label_key: "setting-gap",
        kind: Kind::Float {
            min: 0.0,
            max: 48.0,
        },
    },
    Tunable {
        path: "gallery.tile_aspect",
        label_key: "setting-tile-aspect",
        kind: Kind::Float { min: 0.3, max: 2.0 },
    },
    Tunable {
        path: "gallery.caption_height",
        label_key: "setting-caption-height",
        kind: Kind::Float {
            min: 0.0,
            max: 64.0,
        },
    },
    Tunable {
        path: "gallery.tile_padding",
        label_key: "setting-tile-padding",
        kind: Kind::Float {
            min: 0.0,
            max: 32.0,
        },
    },
    Tunable {
        path: "gallery.prefetch_rows",
        label_key: "setting-prefetch-rows",
        kind: Kind::Int { min: 0, max: 32 },
    },
    Tunable {
        path: "gallery.show_captions",
        label_key: "setting-show-captions",
        kind: Kind::Bool,
    },
    Tunable {
        path: "gallery.recursive",
        label_key: "setting-recursive",
        kind: Kind::Bool,
    },
    Tunable {
        path: "gallery.sort_field",
        label_key: "setting-sort-field",
        kind: Kind::Choice(crate::domain::SORT_FIELD_IDS),
    },
    Tunable {
        path: "gallery.sort_descending",
        label_key: "setting-sort-descending",
        kind: Kind::Bool,
    },
    Tunable {
        path: "gallery.as_list",
        label_key: "setting-as-list",
        kind: Kind::Bool,
    },
    Tunable {
        path: "gallery.map_url",
        label_key: "setting-map-url",
        kind: Kind::Text,
    },
    Tunable {
        path: "gallery.last_folder",
        label_key: "setting-last-folder",
        kind: Kind::State,
    },
    Tunable {
        path: "gallery.last_destination",
        label_key: "setting-last-destination",
        kind: Kind::State,
    },
    Tunable {
        path: "loading.thumb_size",
        label_key: "setting-thumb-size",
        kind: Kind::Int { min: 96, max: 1024 },
    },
    Tunable {
        path: "loading.preview_size",
        label_key: "setting-preview-size",
        kind: Kind::Int {
            min: 512,
            max: 8192,
        },
    },
    Tunable {
        path: "loading.compare_size",
        label_key: "setting-compare-size",
        kind: Kind::Int {
            min: 1024,
            max: 16384,
        },
    },
    Tunable {
        path: "loading.uploads_per_frame",
        label_key: "setting-uploads-per-frame",
        kind: Kind::Int { min: 1, max: 256 },
    },
    Tunable {
        path: "loading.texture_budget",
        label_key: "setting-texture-budget",
        kind: Kind::Int { min: 64, max: 8192 },
    },
    Tunable {
        path: "loading.worker_threads",
        label_key: "setting-worker-threads",
        kind: Kind::Int { min: 0, max: 128 },
    },
    Tunable {
        path: "loading.cache_thumbnails",
        label_key: "setting-cache-thumbnails",
        kind: Kind::Bool,
    },
    Tunable {
        path: "loading.use_embedded_thumbnails",
        label_key: "setting-use-embedded",
        kind: Kind::Bool,
    },
    Tunable {
        path: "loading.idle_repaint_ms",
        label_key: "setting-idle-repaint",
        kind: Kind::Int {
            min: 50,
            max: 10_000,
        },
    },
    Tunable {
        path: "appearance.theme",
        label_key: "setting-theme",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.theme_dark",
        label_key: "setting-theme-dark",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.theme_light",
        label_key: "setting-theme-light",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.language",
        label_key: "setting-language",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.ui_scale",
        label_key: "setting-ui-scale",
        kind: Kind::Float { min: 0.5, max: 3.0 },
    },
    Tunable {
        path: "faces.models",
        label_key: "setting-face-models",
        kind: Kind::Text,
    },
    Tunable {
        path: "faces.detect_size",
        label_key: "setting-face-detect-size",
        kind: Kind::Int {
            min: 320,
            max: 8192,
        },
    },
    Tunable {
        path: "faces.crop_size",
        label_key: "setting-face-crop-size",
        kind: Kind::Int {
            min: 160,
            max: 4096,
        },
    },
    Tunable {
        path: "faces.crop_margin",
        label_key: "setting-face-crop-margin",
        kind: Kind::Float { min: 0.0, max: 1.5 },
    },
    Tunable {
        path: "faces.show_frames",
        label_key: "setting-face-frames",
        kind: Kind::Bool,
    },
    Tunable {
        path: "ai.endpoint",
        label_key: "setting-ai-endpoint",
        kind: Kind::Text,
    },
    Tunable {
        path: "ai.model",
        label_key: "setting-ai-model",
        kind: Kind::Text,
    },
    Tunable {
        path: "ai.language",
        label_key: "setting-ai-language",
        kind: Kind::Text,
    },
    Tunable {
        path: "ai.overwrite",
        label_key: "setting-ai-overwrite",
        kind: Kind::Bool,
    },
    Tunable {
        path: "ai.request_size",
        label_key: "setting-ai-request-size",
        kind: Kind::Int {
            min: 256,
            max: 4096,
        },
    },
    Tunable {
        path: "ai.timeout_seconds",
        label_key: "setting-ai-timeout",
        kind: Kind::Int { min: 10, max: 3600 },
    },
];

/// Every path that really exists in the settings.
pub fn paths_in_settings() -> Vec<String> {
    fn walk(prefix: &str, table: &toml::Table, into: &mut Vec<String>) {
        for (key, value) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            match value {
                toml::Value::Table(nested) => walk(&path, nested, into),
                _ => into.push(path),
            }
        }
    }

    let mut found = Vec::new();
    // `Option::None` is not serialised into TOML, so it is added separately.
    let mut probe = Settings::default();
    probe.window.x = Some(0.0);
    probe.window.y = Some(0.0);
    probe.gallery.last_folder = Some(String::new());
    probe.gallery.last_destination = Some(String::new());
    if let Ok(table) = toml::Table::try_from(&probe) {
        walk("", &table, &mut found);
    }

    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::portable(dir.path());
        paths.ensure().unwrap();
        (dir, paths)
    }

    /// A setting we removed must not take everything else with it. This
    /// happened at once: `window.preview_width` went with the docks, and
    /// everybody who had ever started the application had it in their file.
    #[test]
    fn a_removed_key_does_not_cost_the_rest() {
        let (_dir, paths) = scratch();
        std::fs::write(
            paths.config_file(),
            "[gallery]
tile_size = 96.0

[window]
preview_width = 8.0
tree_width = 237.0
",
        )
        .unwrap();

        let settings = Settings::load(&paths);
        assert_eq!(
            settings.gallery.tile_size, 96.0,
            "the rest should have survived"
        );
        assert!(
            paths.config_file().exists(),
            "a removed key does not send the file aside"
        );
    }

    #[test]
    fn genuinely_damaged_settings_go_aside() {
        let (_dir, paths) = scratch();
        std::fs::write(
            paths.config_file(),
            "[gallery]
tile_size = 'sto'
",
        )
        .unwrap();

        assert_eq!(Settings::load(&paths), Settings::default());
        assert!(
            !paths.config_file().exists(),
            "a damaged file is set aside, not overwritten"
        );
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let (_dir, paths) = scratch();
        assert_eq!(Settings::load(&paths), Settings::default());
    }

    #[test]
    fn only_what_differs_is_saved() {
        let (_dir, paths) = scratch();
        let mut settings = Settings::default();
        settings.save(&paths).unwrap();
        let text = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(
            !text.contains("tile_size"),
            "the default state should not be saved:\n{text}"
        );

        settings.gallery.tile_size = 333.0;
        settings.save(&paths).unwrap();
        let text = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(text.contains("tile_size"), "{text}");
        assert!(
            !text.contains("preview_size"),
            "what was never touched should not be saved:\n{text}"
        );
    }

    #[test]
    fn a_new_default_reaches_somebody_who_has_already_run_it() {
        // When only the difference is saved, a new default reaches somebody
        // who has already run the application. That is the whole point.
        let (_dir, paths) = scratch();
        let mut settings = Settings::default();
        settings.gallery.tile_size = 333.0;
        settings.save(&paths).unwrap();

        let loaded = Settings::load(&paths);
        assert_eq!(loaded.gallery.tile_size, 333.0);
        assert_eq!(
            loaded.loading.texture_budget,
            Loading::default().texture_budget
        );
    }

    #[test]
    fn what_was_saved_comes_back_the_same() {
        let (_dir, paths) = scratch();
        let mut settings = Settings::default();
        settings.gallery.tile_size = 333.0;
        settings.window.maximized = true;
        settings.appearance.theme = "sepia".to_owned();
        settings.loading.worker_threads = 4;
        settings.save(&paths).unwrap();
        assert_eq!(Settings::load(&paths), settings);
    }

    #[test]
    fn a_damaged_file_is_set_aside_and_not_lost() {
        let (_dir, paths) = scratch();
        std::fs::write(paths.config_file(), "this = is not { toml").unwrap();
        assert_eq!(Settings::load(&paths), Settings::default());
        assert!(paths.config_file().with_extension("toml.broken").exists());
    }

    #[test]
    fn a_path_can_be_read_and_written() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.get("gallery.tile_size"),
            Some(toml::Value::Float(220.0))
        );
        settings
            .set("gallery.tile_size", toml::Value::Float(180.0))
            .unwrap();
        assert_eq!(settings.gallery.tile_size, 180.0);

        settings
            .set("gallery.show_captions", toml::Value::Boolean(false))
            .unwrap();
        assert!(!settings.gallery.show_captions);
    }

    #[test]
    fn an_unknown_path_or_the_wrong_type_is_an_error() {
        let mut settings = Settings::default();
        assert!(
            settings
                .set("gallery.no_such_thing", toml::Value::Integer(1))
                .is_err()
        );
        assert!(
            settings
                .set("nothing.is.there", toml::Value::Integer(1))
                .is_err()
        );
        assert!(
            settings
                .set("gallery.tile_size", toml::Value::String("large".into()))
                .is_err(),
            "the wrong type has to fail, not pass"
        );
    }

    #[test]
    fn a_reset_returns_one_entry_or_a_whole_group() {
        let mut settings = Settings::default();
        settings.gallery.tile_size = 999.0;
        settings.reset("gallery.tile_size").unwrap();
        assert_eq!(settings.gallery.tile_size, Gallery::default().tile_size);

        settings.loading.texture_budget = 1;
        settings.loading.thumb_size = 111;
        settings.reset("loading").unwrap();
        assert_eq!(settings.loading, Loading::default());
    }

    #[test]
    fn resetting_everything_leaves_the_window_and_the_folder() {
        let mut settings = Settings::default();
        settings.gallery.tile_size = 999.0;
        settings.gallery.last_folder = Some("E:/photos".to_owned());
        settings.window.width = 1234.0;
        settings.reset_all();

        assert_eq!(settings.gallery.tile_size, Gallery::default().tile_size);
        assert_eq!(settings.gallery.last_folder.as_deref(), Some("E:/photos"));
        assert_eq!(
            settings.window.width, 1234.0,
            "a reset does not clear the window position"
        );
    }

    #[test]
    fn the_field_descriptions_cover_exactly_what_the_settings_hold() {
        let actual = paths_in_settings();
        let described: Vec<String> = TUNABLES
            .iter()
            .map(|t| t.path.to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        let missing: Vec<&String> = actual
            .iter()
            .filter(|path| !described.contains(path))
            .collect();
        assert!(
            missing.is_empty(),
            "the settings hold fields with no description: {missing:?}"
        );

        let extra: Vec<&String> = described
            .iter()
            .filter(|path| !actual.contains(path))
            .collect();
        assert!(
            extra.is_empty(),
            "a description points at a field that does not exist: {extra:?}"
        );
    }

    #[test]
    fn every_field_has_a_translated_label() {
        for tunable in TUNABLES {
            assert!(
                crate::i18n::has(tunable.label_key),
                "{} points at the missing key {}",
                tunable.path,
                tunable.label_key
            );
        }
    }
}
