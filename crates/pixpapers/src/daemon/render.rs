//! Daemon-side rendering: produce a wallpaper frame for one output directly
//! into an XRGB8888 buffer. Uses the same pixcore composite as `export`, so
//! `pixpapers export …` previews exactly what lands on screen.
//!
//! Contract: `view` is always computed from the **original** dimensions
//! (window semantics in source space); the variant canvas is only ever
//! stretched into `view.dest`.

use anyhow::Result;
use pixcore::{Config, Entry, Paths};

use crate::commands;

/// Render the entry to `buf` (out_w*out_h u32 words, row-major). Returns
/// (label like "16/fit-auto" or "16/fit-vga-fs", variant cache hit) for
/// status/bench — the same `{Q}[-{D}]` token convention as the cache key
/// (`Dither::None` omits its segment), so the live label always names the
/// exact look on screen.
pub fn render_into(
    paths: &Paths,
    cfg: &Config,
    entry: &Entry,
    out: (u32, u32),
    buf: &mut [u32],
) -> Result<(String, bool)> {
    let s = commands::resolve_effective(entry, cfg);
    let (variant_path, hit) =
        pixcore::variant::generate(paths, &entry.id, &s, out).map_err(anyhow::Error::from)?;
    let bytes = std::fs::read(&variant_path).map_err(anyhow::Error::from)?;
    let canvas = pixcore::variant::decode_any(&bytes).map_err(anyhow::Error::from)?;
    let view = pixcore::view((entry.width, entry.height), out, s.placement, s.gutter_px);
    // A `bleed` gutter composites the wallpaper's blurred edges as the ring
    // background — the same `bleed_layer` + `composite_into_with_bleed` the
    // offline `export` path uses, so `export` previews exactly this frame.
    let bleed = match s.gutter_style {
        pixcore::GutterStyle::Solid => None,
        pixcore::GutterStyle::Bleed => {
            Some(pixcore::variant::bleed_layer(paths, &entry.id, &s, out)?)
        }
    };
    pixcore::render::composite_into_with_bleed(&canvas, &view, s.bg_color, bleed.as_ref(), buf);
    let d = if s.dither == pixcore::Dither::None {
        String::new()
    } else {
        format!("-{}", s.dither)
    };
    Ok((
        format!("{}/{}-{}{}", s.pixel, s.placement, s.palette, d),
        hit,
    ))
}

/// The current entry to display, or None if nothing is set / gone.
pub fn current_entry(paths: &Paths) -> Option<Entry> {
    let lib = pixcore::Library::load(paths).ok()?;
    let cur = lib.current?;
    lib.entries.iter().find(|e| e.id == cur).cloned()
}
