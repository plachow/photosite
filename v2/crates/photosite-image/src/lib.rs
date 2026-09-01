//! Pixels: decoding, downscaling, EXIF.
//!
//! Like the core, this crate **does not know a UI exists.** It hands back
//! plain RGB and leaves it to the caller what to make of it — a texture, a
//! file, or both.

pub mod cache;
pub mod decode;
pub mod encode;
pub mod exif;
pub mod raw;

pub use cache::Cache;
pub use decode::{Rgb, fit, quick, rotate, sized};
pub use encode::{resize, sharpen};

/// Longer edge of a tile in the grid.
pub const THUMB: u32 = 320;
/// Longer edge of the full preview. No pane shows more than this, and
/// decoding 45 Mpx to get there would be waste.
pub const PREVIEW: u32 = 2560;
