//! Pixely: dekódování, zmenšování, EXIF.
//!
//! Stejně jako jádro tahle crate **neví, že existuje UI.** Vrací holé RGB
//! a je na volajícím, co s ním udělá — textura, soubor, nebo obojí.

pub mod decode;
pub mod exif;

pub use decode::{Rgb, fit, quick, rotate, sized};

/// Delší hrana dlaždice v mřížce.
pub const THUMB: u32 = 320;
/// Delší hrana plného náhledu. Víc než tohle žádný panel nezobrazí a
/// dekódovat kvůli tomu 45 Mpx by bylo plýtvání.
pub const PREVIEW: u32 = 2560;
