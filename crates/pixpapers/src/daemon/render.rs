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
    // auto resolves to the concrete preset for THIS output before the
    // cache key is built, and the label below then names the preset
    // actually on screen (status shows the resolved look, per output).
    let s = commands::resolve_effective(entry, cfg).resolved_for_output(out);
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

#[cfg(test)]
mod tests {
    use super::*;
    use pixcore::{Config, Settings};

    /// The status label is where `auto` becomes observable: it names the
    /// preset resolved for THIS output, and the variant cache on disk
    /// stays concrete (never an `auto` filename).
    #[test]
    fn render_label_names_the_resolved_preset_for_auto() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_root(dir.path());
        paths.ensure_dirs().unwrap();
        let entry_dir = paths.entry_dir("auto1");
        std::fs::create_dir_all(&entry_dir).unwrap();
        let src: image::RgbaImage =
            image::ImageBuffer::from_pixel(64, 64, image::Rgba([10, 200, 30, 255]));
        src.save(entry_dir.join("original.png")).unwrap();
        let entry = pixcore::Entry {
            id: "auto1".into(),
            title: "t".into(),
            hash: "h".into(),
            ext: "png".into(),
            added_unix: 0,
            width: 64,
            height: 64,
            original_bytes: 1,
            optimize_note: "png".into(),
            settings: Settings {
                pixel: Some("auto".parse().unwrap()),
                ..Default::default()
            },
        };
        let out = (320u32, 180u32); // long edge 320 → auto picks 8
        let mut buf = vec![0u32; (out.0 * out.1) as usize];
        let (label, _hit) =
            render_into(&paths, &Config::default(), &entry, out, &mut buf).expect("render");
        assert_eq!(label, "8/fit-auto", "label must name the resolved preset");
        // the cache file on disk is the concrete-preset name
        let names: Vec<String> = std::fs::read_dir(paths.variants_dir("auto1"))
            .unwrap()
            .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.contains("-8-fit-320x180-")),
            "variant not cached under the concrete preset: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("v2-auto-")),
            "auto leaked into a cache filename: {names:?}"
        );
    }
}
