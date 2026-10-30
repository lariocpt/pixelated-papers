//! pixcore — pure logic behind [pixelated-papers](https://github.com/lariocpt/pixelated-papers):
//! presets, placement math, the pixelation pipeline, library manifest, config.
//!
//! No wayland, no daemon concerns — everything here is deterministic and
//! unit-testable, and `pixpapers export` renders bit-for-bit what the daemon
//! will put on screen.

pub mod color;
pub mod config;
pub mod error;
pub mod manifest;
pub mod optimize;
pub mod palette;
pub mod paths;
pub mod pipeline;
pub mod placement;
pub mod preset;
pub mod render;
pub mod variant;

pub use color::RgbCss;
pub use config::Config;
pub use error::PixError;
pub use manifest::{Entry, Library, Settings};
pub use palette::{Dither, PaletteMode};
pub use paths::Paths;
pub use placement::{Placement, Rect, View, view};
pub use preset::Pixel;
pub use render::ResolvedSettings;
pub use variant::{VariantKey, variant_path};

/// Library layout version written into `library.json`.
pub const MANIFEST_VERSION: u32 = 1;
/// Bump on **any** pipeline behavior change (filters, quantizer, rounding,
/// placement math) — stale variant caches invalidate automatically because
/// the version is part of the cache filename. v2: cache keys carry the
/// palette mode + dither, quantize/dither stages exist (PLAN-30 §A).
pub const PIPELINE_VERSION: u32 = 2;

/// XDG directory namespace (and `PIXPAPERS_HOME` override root name).
pub const APP_DIR: &str = "pixpapers";
