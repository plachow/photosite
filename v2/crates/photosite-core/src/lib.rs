//! The PhotoSite core: domain, catalogue and runtime scaffolding.
//!
//! This crate **must not know that a UI exists.** No `egui`, no `eframe`, no
//! `wgpu` — not even indirectly. The test in `tests/no_ui.rs` guards it,
//! because this particular boundary erodes on its own, and losing it is the
//! single reason the v1 port was expensive: nineteen files outside the UI
//! folders reached for `BitmapSource`.

pub mod batch;
pub mod catalog;
pub mod commands;
pub mod compare;
pub mod diagnostics;
pub mod docks;
pub mod domain;
pub mod filter;
pub mod history;
pub mod i18n;
pub mod jobs;
pub mod paths;
pub mod people;
pub mod place;
pub mod settings;
pub mod theme;
pub mod time;
pub mod transfer;

pub use batch::{Plan, Preset};
pub use catalog::{Catalog, NewPhoto};
pub use compare::Compare;
pub use docks::Layout;
pub use domain::{FileIdentity, Photo, PhotoId, is_photo, sidecar_of};
pub use filter::{Facets, Filter, Shape};
pub use history::History;
pub use i18n::{t, t_args};
pub use jobs::{Cancel, Progress, TaskStatus, Tasks, Wishlist};
pub use paths::Paths;
pub use people::{Expressions, Face, Person, Region, Tag};
pub use place::{Place, Verdict};
pub use settings::Settings;
pub use theme::{Palette, Theme};

/// The version reported by `--version` and by the diagnostics window.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
