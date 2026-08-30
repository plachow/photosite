//! Jádro PhotoSite: doména, katalog a běhové zázemí.
//!
//! Tahle crate **nesmí vědět, že existuje UI.** Žádné `egui`, žádné `eframe`,
//! žádné `wgpu` — ani nepřímo. Hlídá to test v `tests/bez_ui.rs`, protože
//! přesně tahle hranice se rozpadá sama od sebe a její ztráta je jediný důvod,
//! proč byl port v1 drahý: devatenáct souborů mimo UI složky tam sahalo na
//! `BitmapSource`.

pub mod catalog;
pub mod commands;
pub mod diagnostics;
pub mod domain;
pub mod i18n;
pub mod jobs;
pub mod paths;
pub mod settings;
pub mod theme;

pub use catalog::{Catalog, NewPhoto};
pub use domain::{FileIdentity, Photo, PhotoId, is_photo};
pub use i18n::{t, t_args};
pub use jobs::{Cancel, Progress, TaskStatus, Tasks, Wishlist};
pub use paths::Paths;
pub use settings::Settings;
pub use theme::{Palette, Theme};

/// Verze, kterou hlásí `--version` i diagnostika.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
