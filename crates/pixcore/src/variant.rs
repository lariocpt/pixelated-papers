//! Variant naming, lazy generation and caching (R7/R8). The variant is the
//! absolute-minimum-size palette PNG for one (preset, placement, output);
//! the cache key carries `PIPELINE_VERSION` so pipeline upgrades invalidate
//! automatically.

use std::fs;
use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use crate::PIPELINE_VERSION;
use crate::error::{PixError, Result};
use crate::gutter::GutterStyle;
use crate::optimize;
use crate::palette::{self, Dither, PaletteMode};
use crate::paths::Paths;
use crate::pipeline;
use crate::placement::Placement;
use crate::preset::Pixel;
use crate::render::ResolvedSettings;

/// Cache key for one rendered variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VariantKey<'a> {
    pub pixel: Pixel,
    pub placement: Placement,
    pub gutter_px: u32,
    pub out: (u32, u32),
    pub _id: &'a str, // phantom-ish clarity; path building scopes per entry
}

/// `library/<id>/variants/v<V>-<pixel>-<placement>[-g<g>]-<W>x<H>-<Q>[-<D>].png`
/// — v2 carries the palette mode token (`-auto`, `-gen16`, …) and, when not
/// `none`, the dither token: every (mode, dither) cell is its own cache
/// entry (`Dither::None` omits the segment, keeping the v0.1 look's shape).
pub fn variant_path(paths: &Paths, id: &str, s: &ResolvedSettings, out: (u32, u32)) -> PathBuf {
    let g = if s.placement == Placement::CoverGutter {
        format!("-g{}", s.gutter_px)
    } else {
        String::new()
    };
    let d = if s.dither == Dither::None {
        String::new()
    } else {
        format!("-{}", s.dither)
    };
    // `bleed` is the only non-default gutter style; it gets its own cache
    // entry so a bleed render never collides with the solid-mat variant.
    // `solid` omits the segment (mirrors the dither rule), so every default
    // variant keeps its exact pre-bleed filename and cached bytes.
    let bl = match s.gutter_style.token() {
        Some(t) => format!("-{t}"),
        None => String::new(),
    };
    paths.variants_dir(id).join(format!(
        "v{PIPELINE_VERSION}-{pixel}-{placement}{g}-{W}x{H}-{Q}{D}{BL}.png",
        pixel = s.pixel,
        placement = s.placement,
        W = out.0,
        H = out.1,
        Q = s.palette,
        D = d,
        BL = bl,
    ))
}

/// Generate-or-cache: stat the cache, on miss run the pipeline and write
/// atomically. Returns the path + whether it was a cache hit.
pub fn generate(
    paths: &Paths,
    id: &str,
    s: &ResolvedSettings,
    out: (u32, u32),
) -> Result<(PathBuf, bool)> {
    let path = variant_path(paths, id, s, out);
    if path.exists() && fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok((path, true));
    }
    let orig = original_path(paths, id)?;
    let generated = generate_from_file(&orig, s, out)?;
    let bytes = encode_variant(&generated, s.palette);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| PixError::io(parent, e))?;
    }
    crate::config::write_atomic(&path, &bytes).map_err(|e| PixError::io(&path, e))?;
    Ok((path, false))
}

/// Run the pipeline for one (settings, output size) from the stored original.
pub fn generate_from_file(
    orig: &std::path::Path,
    s: &ResolvedSettings,
    out: (u32, u32),
) -> Result<image::RgbaImage> {
    let bytes = fs::read(orig).map_err(|e| PixError::io(orig, e))?;
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| PixError::Decode {
            path: orig.to_path_buf(),
            reason: e.to_string(),
        })?;
    let img = reader.decode().map_err(|e| PixError::Decode {
        path: orig.to_path_buf(),
        reason: e.to_string(),
    })?;
    Ok(generate_from_image(&img.into_rgba8(), s, out))
}

pub fn generate_from_image(
    orig: &image::RgbaImage,
    s: &ResolvedSettings,
    out: (u32, u32),
) -> image::RgbaImage {
    let v = crate::placement::view((orig.width(), orig.height()), out, s.placement, s.gutter_px);
    if s.pixel.is_identity() {
        // `normal`: the canvas is the dest itself — nearest stretch happens
        // in composite. S3.5 settled rule: the plain-crop early return
        // applies ONLY when the resolved look is exactly (Auto, None) —
        // the v0.1 default, whose bytes must stay identical (encode still
        // does its median_cut(255), exactly as v0.1 did). Every other
        // (palette, dither) combination runs the crop through the existing
        // pipeline::quantize stage — the same stage the pixelated presets
        // use — before the unchanged encode_variant:
        //   (Off, *)       → quantize is identity (a dither has no palette
        //                    to diffuse into) → the same crop bytes as
        //                    v0.1's palette=false normal
        //   (named, none) → full-canvas nearest to the named table (encode's
        //                    re-map is idempotent on colors already on the
        //                    table)
        //   (*, dither ≠ none) → full-canvas quantize + error diffusion /
        //                    ordered dither — the "photo → console" look
        //                    A.1 describes
        let crop = pipeline::crop(orig, v.window);
        if s.palette == PaletteMode::Auto && s.dither == Dither::None {
            return crop;
        }
        return pipeline::quantize(&crop, s.palette, s.dither);
    }
    let mut var = pipeline::downscale_to_variant(orig, &v, s.pixel.block_px());
    if s.palette != PaletteMode::Off {
        // Auto + None is the exact v0.1 call sequence (median_cut +
        // apply_palette), so those variants stay byte-identical.
        var = pipeline::quantize(&var, s.palette, s.dither);
    }
    var
}

/// Encode a variant as PNG. Every quantizing mode writes a true palette PNG
/// via the `png` crate (absolute-minimum bytes; named tables use exactly
/// their entries); `Off` stays plain RGBA.
pub fn encode_variant(img: &image::RgbaImage, mode: PaletteMode) -> Vec<u8> {
    if palette::encode_as_palette_png(mode) {
        if let Some(bytes) = encode_palette_png(img, mode) {
            return bytes;
        }
    }
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    let _ = image::ImageEncoder::write_image(
        enc,
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    );
    out
}

/// The exact PLTE content for `img` under `mode`, index 0 *not* reserved
/// (the encoder adds the transparent slot). `Auto` re-runs the v0.1
/// `median_cut(255)` without deduplication so already-cached variants stay
/// byte-identical; named tables are deduplicated (first occurrence wins —
/// the NES table repeats black 12×), Gen16 re-runs `median_cut(16)` + snap.
/// Deterministic for a given `img`.
fn encode_palette(mode: PaletteMode, img: &RgbaImage) -> Vec<Rgba<u8>> {
    match mode {
        PaletteMode::Auto => pipeline::median_cut(img, 255),
        PaletteMode::Gen16 => dedup(
            pipeline::median_cut(img, 16)
                .into_iter()
                .map(palette::snap_rgb555),
        ),
        named => dedup(
            named
                .fixed_table()
                .expect("named mode has a table")
                .iter()
                .copied(),
        ),
    }
}

fn dedup(iter: impl Iterator<Item = Rgba<u8>>) -> Vec<Rgba<u8>> {
    let mut out: Vec<Rgba<u8>> = Vec::with_capacity(64);
    for c in iter {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Palette PNG. ≤ 255 entries use the v0.1 scheme verbatim: index 0 is a
/// placeholder-black transparent slot (tRNS), real entries shifted +1. A
/// full 256-entry table (VGA) has no room for that slot: with no
/// transparent pixels the table is written as-is; otherwise the first
/// entry no opaque pixel uses becomes the transparent index (documented
/// merge), and if every entry is used the caller falls back to RGBA.
/// `None` on any encoder trouble.
fn encode_palette_png(img: &image::RgbaImage, mode: PaletteMode) -> Option<Vec<u8>> {
    let pal = encode_palette(mode, img);
    if pal.is_empty() || pal.len() > 256 {
        return None;
    }
    let has_transparent = img.pixels().any(|p| p.0[3] <= 8);
    let mut buf = Vec::new();
    if pal.len() == 256 && has_transparent {
        // Free one index: the first table entry no opaque pixel uses.
        let mut used = [false; 256];
        for p in img.pixels().filter(|p| p.0[3] > 8) {
            used[pipeline::nearest_index(&pal, p.0[0] as i64, p.0[1] as i64, p.0[2] as i64)] = true;
        }
        let tidx = used.iter().position(|u| !u)? as u8;
        let indices: Vec<u8> = img
            .pixels()
            .map(|p| {
                if p.0[3] <= 8 {
                    tidx
                } else {
                    pipeline::nearest_index(&pal, p.0[0] as i64, p.0[1] as i64, p.0[2] as i64) as u8
                }
            })
            .collect();
        let mut trns = vec![255u8; 256];
        trns[tidx as usize] = 0;
        let ok = {
            let mut enc = png::Encoder::new(&mut buf, img.width(), img.height());
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_palette(
                pal.iter()
                    .flat_map(|c| [c.0[0], c.0[1], c.0[2]])
                    .collect::<Vec<_>>(),
            );
            enc.set_trns(trns);
            match enc.write_header() {
                Ok(mut w) => w.write_image_data(&indices).is_ok(),
                Err(_) => false,
            }
        };
        return if ok { Some(buf) } else { None };
    }
    // v0.1 scheme: index 0 = transparent placeholder black (only used when
    // the image has transparent pixels; harmless otherwise), entries +1.
    let mut plte: Vec<u8> = vec![0, 0, 0];
    for c in &pal {
        plte.extend_from_slice(&[c.0[0], c.0[1], c.0[2]]);
    }
    let indices: Vec<u8> = img
        .pixels()
        .map(|p| {
            if p.0[3] <= 8 {
                return 0u8;
            }
            let mut best = 1usize;
            let mut bestd = u64::MAX;
            for (i, c) in pal.iter().enumerate() {
                let dr = p.0[0] as i64 - c.0[0] as i64;
                let dg = p.0[1] as i64 - c.0[1] as i64;
                let db = p.0[2] as i64 - c.0[2] as i64;
                let d = (dr * dr + dg * dg + db * db) as u64;
                if d < bestd {
                    bestd = d;
                    best = i + 1;
                }
            }
            best as u8
        })
        .collect();
    let ok = {
        let mut enc = png::Encoder::new(&mut buf, img.width(), img.height());
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(plte);
        enc.set_trns(vec![0u8]); // only index 0 is transparent (v0.1 verbatim)
        match enc.write_header() {
            Ok(mut w) => w.write_image_data(&indices).is_ok(),
            Err(_) => false,
        }
    };
    if ok { Some(buf) } else { None }
}

/// The full offline render path, shared by `export` and the TUI preview
/// (PLAN-30 §C.3: the preview is literally a small export, not a
/// reimplementation): generate-or-cache the variant at `out`, composite
/// into `out` with the resolved placement/bg (the `view` comes from the
/// ORIGINAL dims — invariant #2), and encode exactly like export does.
/// Returns the final image and its PNG bytes.
pub fn render_offline(
    paths: &Paths,
    id: &str,
    src: (u32, u32),
    s: &ResolvedSettings,
    out: (u32, u32),
) -> Result<(image::RgbaImage, Vec<u8>)> {
    let (variant_path, _hit) = generate(paths, id, s, out)?;
    let bytes = fs::read(&variant_path).map_err(|e| PixError::io(&variant_path, e))?;
    let canvas = decode_any(&bytes)?;
    let view = crate::placement::view(src, out, s.placement, s.gutter_px);
    let bleed = match s.gutter_style {
        GutterStyle::Solid => None,
        GutterStyle::Bleed => Some(bleed_layer(paths, id, s, out)?),
    };
    let img = crate::render::composite_with_bleed(&canvas, &view, s.bg_color, bleed.as_ref());
    let png = encode_final(&img, &canvas, s.palette, s.bg_color);
    Ok((img, png))
}

/// The palette `encode_final` should publish for a mode: the mode's exact
/// fixed table (deduped, first occurrence — the variant encoder's rule),
/// or for `gen16` the table derived on the canvas the same way the variant
/// encoder does (`median_cut(16)` snapped to RGB555). `off`/`auto` have no
/// fixed table (`auto` would need 255 + bg, which does not fit the ≤255
/// v0.1 scheme).
fn fixed_palette(mode: PaletteMode, canvas: &image::RgbaImage) -> Option<Vec<image::Rgba<u8>>> {
    match mode {
        PaletteMode::Off | PaletteMode::Auto => None,
        PaletteMode::Gen16 => Some(dedup_first(
            crate::pipeline::median_cut(canvas, 16)
                .into_iter()
                .map(crate::palette::snap_rgb555),
        )),
        named => Some(dedup_first(
            named
                .fixed_table()
                .expect("named mode has a table")
                .iter()
                .copied(),
        )),
    }
}

/// Encode a final composite as a PNG (moved verbatim from
/// `pixpapers::commands` so export and the TUI preview share one
/// encoder — byte-identical to the pre-S6 export bytes). Every quantizing
/// mode except `auto` writes a true palette PNG (exact table + bg merged
/// in, v0.1 index-0-transparent convention); `off`/`auto` stay plain
/// RGBA; encoder trouble falls back to RGBA.
pub fn encode_final(
    img: &image::RgbaImage,
    canvas: &image::RgbaImage,
    mode: PaletteMode,
    bg: crate::color::RgbCss,
) -> Vec<u8> {
    use image::ImageEncoder as _;
    let encode_rgba = || {
        let mut out = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        let _ = enc.write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        );
        out
    };
    let Some(mut pal) = fixed_palette(mode, canvas) else {
        return encode_rgba();
    };
    let bgc = image::Rgba([bg.0[0], bg.0[1], bg.0[2], 255]);
    if !pal.contains(&bgc) {
        pal.push(bgc);
    }
    if pal.len() <= 255 {
        if let Some(bytes) = encode_indexed(img, &pal) {
            return bytes;
        }
    }
    encode_rgba()
}

/// First-occurrence dedup, same semantics as the variant encoder's.
fn dedup_first(iter: impl Iterator<Item = image::Rgba<u8>>) -> Vec<image::Rgba<u8>> {
    let mut out: Vec<image::Rgba<u8>> = Vec::with_capacity(64);
    for c in iter {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Indexed PNG over `pal` with the v0.1 scheme: index 0 is the
/// transparent placeholder black, palette entries shifted +1. Pixels
/// already in the palette (quantized canvas colors, bg) map exactly; the
/// nearest fallback mirrors the variant encoder. `None` on encoder trouble.
fn encode_indexed(img: &image::RgbaImage, pal: &[image::Rgba<u8>]) -> Option<Vec<u8>> {
    let mut plte: Vec<u8> = vec![0, 0, 0];
    for c in pal {
        plte.extend_from_slice(&[c.0[0], c.0[1], c.0[2]]);
    }
    let indices: Vec<u8> = img
        .pixels()
        .map(|p| {
            if p.0[3] <= 8 {
                return 0u8;
            }
            let mut best = 1usize;
            let mut bestd = u64::MAX;
            for (i, c) in pal.iter().enumerate() {
                let dr = p.0[0] as i64 - c.0[0] as i64;
                let dg = p.0[1] as i64 - c.0[1] as i64;
                let db = p.0[2] as i64 - c.0[2] as i64;
                let d = (dr * dr + dg * dg + db * db) as u64;
                if d < bestd {
                    bestd = d;
                    best = i + 1;
                }
            }
            best as u8
        })
        .collect();
    let mut buf = Vec::new();
    let ok = {
        let mut enc = png::Encoder::new(&mut buf, img.width(), img.height());
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(plte);
        enc.set_trns(vec![0u8]); // only index 0 is transparent (v0.1 verbatim)
        match enc.write_header() {
            Ok(mut w) => w.write_image_data(&indices).is_ok(),
            Err(_) => false,
        }
    };
    if ok { Some(buf) } else { None }
}

/// Decode any bytes we accept (used by export/tests).
pub fn decode_any(bytes: &[u8]) -> Result<image::RgbaImage> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| PixError::other(e.to_string()))?;
    let img = reader
        .decode()
        .map_err(|e| PixError::other(e.to_string()))?;
    Ok(img.into_rgba8())
}

/// Locate the stored original for an entry (extension recorded in manifest;
/// glob the two candidates as fallback).
pub fn original_path(paths: &Paths, id: &str) -> Result<std::path::PathBuf> {
    for ext in ["png", "jpg"] {
        let p = paths.entry_dir(id).join(format!("original.{ext}"));
        if p.exists() {
            return Ok(p);
        }
    }
    Err(PixError::NotFound(format!(
        "no stored original for entry {id}"
    )))
}

/// Cache path for the blurred bleed background of a `bleed` gutter render.
/// Keyed by only the tokens that change the bleed (pixel block size, output
/// size, palette, dither) — NOT `gutter_px` (the bleed covers the whole output
/// regardless of ring width) nor `placement`. Lives beside the variant PNG.
pub fn bleed_path(paths: &Paths, id: &str, s: &ResolvedSettings, out: (u32, u32)) -> PathBuf {
    let d = if s.dither == Dither::None {
        String::new()
    } else {
        format!("-{}", s.dither)
    };
    paths.variants_dir(id).join(format!(
        "bleed-v{PIPELINE_VERSION}-{pixel}-{W}x{H}-{Q}{D}.png",
        pixel = s.pixel,
        W = out.0,
        H = out.1,
        Q = s.palette,
        D = d,
    ))
}

/// The palette the bleed quantizes onto, derived from the *sharp variant
/// canvas* so the ring shares the exact colors on screen. `off` has no
/// palette (empty ⇒ RGBA bleed). For named modes this is the fixed table
/// (identical to what `encode_final` publishes); `gen16` re-runs
/// `median_cut(16)`+snap on the canvas, matching `fixed_palette`.
fn bleed_palette(mode: PaletteMode, variant: &RgbaImage) -> Vec<Rgba<u8>> {
    match mode {
        PaletteMode::Off => Vec::new(),
        PaletteMode::Auto => pipeline::median_cut(variant, 255),
        PaletteMode::Gen16 => pipeline::median_cut(variant, 16)
            .into_iter()
            .map(crate::palette::snap_rgb555)
            .collect(),
        named => named
            .fixed_table()
            .map(|t| t.to_vec())
            .unwrap_or_else(|| pipeline::median_cut(variant, 255)),
    }
}

/// Materialize the blurred bleed background for a `bleed` gutter render:
/// decode from cache if present, else build it from the stored original,
/// quantize it onto the wallpaper palette and cache it as RGBA.
pub fn bleed_layer(
    paths: &Paths,
    id: &str,
    s: &ResolvedSettings,
    out: (u32, u32),
) -> Result<RgbaImage> {
    let p = bleed_path(paths, id, s, out);
    if p.exists() {
        if let Ok(bytes) = fs::read(&p) {
            if let Ok(img) = decode_any(&bytes) {
                return Ok(img);
            }
        }
    }
    let orig = original_path(paths, id)?;
    let bytes = fs::read(&orig).map_err(|e| PixError::io(&orig, e))?;
    let oimg = decode_any(&bytes)?;
    // The bleed's palette comes from the (cached) sharp variant canvas.
    let pal = match s.palette {
        PaletteMode::Off => Vec::new(),
        mode => {
            let (vp, _) = generate(paths, id, s, out)?;
            let vb = fs::read(&vp).map_err(|e| PixError::io(&vp, e))?;
            let canvas = decode_any(&vb)?;
            bleed_palette(mode, &canvas)
        }
    };
    let bleed = build_bleed(&oimg, s, out, &pal);
    let enc = encode_variant(&bleed, PaletteMode::Off);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| PixError::io(parent, e))?;
    }
    crate::config::write_atomic(&p, &enc).map_err(|e| PixError::io(&p, e))?;
    Ok(bleed)
}

/// Build the bleed background canvas: cover-crop `orig` to the full output
/// aspect, area-average downscale to the pixel grid (the pipeline's own box
/// kernels), one extra box-blur pass over the grid, then quantize onto `pal`
/// (empty ⇒ left as RGBA). Blur happens **before** quantize so the pixelated
/// look survives; the result is nearest-stretched over the whole output by the
/// compositor.
fn build_bleed(
    orig: &RgbaImage,
    s: &ResolvedSettings,
    out: (u32, u32),
    pal: &[Rgba<u8>],
) -> RgbaImage {
    let cv = crate::placement::view((orig.width(), orig.height()), out, Placement::CoverCrop, 0);
    let cover = pipeline::crop(orig, cv.window);
    let block = s.pixel.block_px().max(1);
    let gw = (out.0 / block).max(1);
    let gh = (out.1 / block).max(1);
    let mut g = pipeline::downscale(&cover, gw, gh);
    // One extra 2×2 box-average pass (the pipeline's own kernel) softens the
    // bleed past the crisp pixel grid — a soft on-palette halo, not sharp tiles.
    if g.width() > 1 && g.height() > 1 {
        g = pipeline::box_halve(&g);
    }
    if !pal.is_empty() {
        g = pipeline::apply_palette(&g, pal);
    }
    g
}

/// Re-run add-time optimization over the whole library.
pub fn optimize_all(paths: &Paths, cfg: &crate::config::Optimize) -> Vec<(String, String)> {
    let mut notes = Vec::new();
    let Ok(lib) = crate::manifest::Library::load(paths) else {
        return notes;
    };
    for e in &lib.entries {
        let Ok(orig) = original_path(paths, &e.id) else {
            continue;
        };
        if let Ok(opt) = optimize::optimize(&orig.clone(), cfg) {
            notes.push((
                e.id.clone(),
                format!("{}×{} {}", opt.width, opt.height, opt.note),
            ));
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::testsrc;
    use proptest::prelude::*;

    fn settings(pixel: &str) -> ResolvedSettings {
        ResolvedSettings {
            pixel: pixel.parse().unwrap(),
            ..ResolvedSettings::resolve(&Default::default(), &Default::default())
        }
    }

    #[test]
    fn path_scheme_includes_version_and_mode() {
        let p = variant_path(
            &Paths::with_root("/tmp/x".as_ref()),
            "id1",
            &settings("16"),
            (1920, 1080),
        );
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            name,
            format!("v{PIPELINE_VERSION}-16-fit-1920x1080-auto.png")
        );
        // dither token appears only for non-none; gutter stays conditional.
        let s = ResolvedSettings {
            dither: Dither::FloydSteinberg,
            placement: Placement::CoverGutter,
            gutter_px: 12,
            ..settings("8")
        };
        let name = variant_path(
            &Paths::with_root("/tmp/x".as_ref()),
            "id1",
            &s,
            (3840, 2160),
        )
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
        assert_eq!(
            name,
            format!("v{PIPELINE_VERSION}-8-cover-gutter-g12-3840x2160-auto-fs.png")
        );
    }

    // ---- blurred-bleed gutter ----

    #[test]
    fn bleed_variant_token_is_conditional_and_bleed_cache_ignores_ring() {
        let mut solid = settings("16");
        solid.placement = Placement::CoverGutter;
        solid.gutter_px = 24;
        let mut bleed = solid;
        bleed.gutter_style = GutterStyle::Bleed;

        let paths = Paths::with_root(std::path::Path::new("/tmp/x"));
        let out = (1920, 1080);
        let sname = variant_path(&paths, "id1", &solid, out)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let bname = variant_path(&paths, "id1", &bleed, out)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        // solid keeps the exact pre-bleed filename (byte-compatible defaults).
        assert!(!sname.contains("bleed"), "solid leaked a token: {sname}");
        assert_eq!(
            sname,
            format!("v{PIPELINE_VERSION}-16-cover-gutter-g24-1920x1080-auto.png")
        );
        // bleed appends `-bleed` immediately before `.png`.
        assert_eq!(bname, sname.replace(".png", "-bleed.png"));

        // The bleed sibling cache is keyed by pixel+out+palette+dither only:
        // no ring/placement segment, and identical for solid vs bleed and for
        // any gutter width (the bleed covers the whole output regardless).
        let bkey = bleed_path(&paths, "id1", &bleed, out)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            bkey,
            format!("bleed-v{PIPELINE_VERSION}-16-1920x1080-auto.png")
        );
        assert_eq!(
            bleed_path(&paths, "id1", &solid, out),
            bleed_path(&paths, "id1", &bleed, out)
        );
        let mut wide = bleed;
        wide.gutter_px = 60;
        assert_eq!(
            bleed_path(&paths, "id1", &bleed, out),
            bleed_path(&paths, "id1", &wide, out)
        );
    }

    #[test]
    fn render_offline_bleed_paints_ring_solid_paints_bg() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_root(dir.path());
        let id = "wall";
        let out = (240u32, 150u32);
        // Seed a colorful stored original (original_path only resolves once present).
        let entry = paths.entry_dir(id);
        std::fs::create_dir_all(&entry).unwrap();
        testsrc::gradient64()
            .save(entry.join("original.png"))
            .unwrap();

        let mut solid = settings("8");
        solid.placement = Placement::CoverGutter;
        solid.gutter_px = 12;
        solid.bg_color = "#000000".parse().unwrap();
        let (solid_img, _) = render_offline(&paths, id, (64, 64), &solid, out).unwrap();
        assert!(
            !bleed_path(&paths, id, &solid, out).exists(),
            "a solid render must never build a bleed layer"
        );

        let mut bleed = solid;
        bleed.gutter_style = GutterStyle::Bleed;
        let (bleed_img, _) = render_offline(&paths, id, (64, 64), &bleed, out).unwrap();
        assert!(
            bleed_path(&paths, id, &bleed, out).exists(),
            "a bleed render must cache the blur layer"
        );

        // Ring geometry comes from the same view the compositor uses.
        let v = crate::placement::view((64, 64), out, Placement::CoverGutter, 12);
        let d = v.dest;
        let in_dest = |x: u32, y: u32| x >= d.x && x < d.x + d.w && y >= d.y && y < d.y + d.h;
        let mut ring = 0;
        let mut bled_color = false;
        for (x, y, p) in bleed_img.enumerate_pixels() {
            if in_dest(x, y) {
                // the sharp variant is painted into dest identically for both
                assert_eq!(p.0, solid_img.get_pixel(x, y).0, "dest diverged at {x},{y}");
                continue;
            }
            ring += 1;
            // solid gutter is the flat bg mat…
            let sc = solid_img.get_pixel(x, y).0;
            assert_eq!(
                [sc[0], sc[1], sc[2]],
                [0, 0, 0],
                "solid ring {x},{y} not bg"
            );
            // …bleed gutter carries the wallpaper's own blurred colour.
            if [p.0[0], p.0[1], p.0[2]] != [0, 0, 0] {
                bled_color = true;
            }
        }
        assert!(ring > 0, "cover-gutter must leave a ring");
        assert!(
            bled_color,
            "bleed ring must carry wallpaper color, not flat bg"
        );
    }

    // Acceptance (PLAN-30 A.4): every (palette, dither) cell is its own
    // cache file — 48 distinct names.
    proptest! {
        #[test]
        fn distinct_mode_dither_cells_get_distinct_paths(qi in 0..8u8, di in 0..6u8) {
            let mut seen: std::collections::HashSet<String> = Default::default();
            let _ = (qi, di); // the loop below is the exhaustive space; proptest drives the sample count
            for q in PaletteMode::ALL {
                for d in Dither::ALL {
                    let s = ResolvedSettings {
                        palette: q,
                        dither: d,
                        ..settings("16")
                    };
                    let name = variant_path(&Paths::with_root("/tmp/x".as_ref()), "id1", &s, (1920, 1080))
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    prop_assert!(seen.insert(name.clone()), "collision at {q}/{d}: {name}");
                }
            }
        }
    }

    /// Acceptance (PLAN-30 A.4 / S3.5): the v0.1 default look is
    /// **byte-identical to v0.1**. tests/golden/v01/ was captured from the
    /// untouched v0.1 tree (commit 29ef2b4) via the public API on the same
    /// sources (Fit, 320×180); sha256 recorded in the wiring commit message.
    /// S1/S3 proved pixel 8; S3.5 extends the fixture to `normal` — the
    /// preset that matters now. (auto, none) early-returns the crop (encode
    /// still does its median_cut(255), exactly as v0.1); (off, none) is the
    /// plain-crop RGBA path.
    #[test]
    fn auto_none_matches_v01_bytes() {
        for (name, src) in [
            ("photo128x80", testsrc::photo128x80()),
            ("gradient64", testsrc::gradient64()),
        ] {
            for (pixel, mid) in [("8", ""), ("normal", "-normal")] {
                for (tag, palette) in [("auto", PaletteMode::Auto), ("off", PaletteMode::Off)] {
                    let s = ResolvedSettings {
                        pixel: pixel.parse().unwrap(),
                        placement: Placement::Fit,
                        palette,
                        ..ResolvedSettings::resolve(&Default::default(), &Default::default())
                    };
                    let canvas = generate_from_image(&src, &s, (320, 180));
                    let bytes = encode_variant(&canvas, palette);
                    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join(format!("tests/golden/v01/{name}{mid}-{tag}.png"));
                    let want = std::fs::read(&golden).expect("v0.1 golden present");
                    assert_eq!(
                        bytes, want,
                        "{name}{mid}/{tag} differs from the v0.1 capture (regression!)"
                    );
                }
            }
        }
    }

    // S3.5: `normal` + a *named* palette runs the crop through quantize, so
    // every output color is inside the chosen table (proptest over
    // arbitrary pixel content, all named modes × all dithers). The
    // fully-transparent entry is allowed for pixels that were transparent.
    proptest! {
        #[test]
        fn normal_named_stays_in_table(
            w in 1u32..12,
            h in 1u32..12,
            seed in 0u64..u64::MAX,
        ) {
            let mut st = seed | 1;
            let img = image::ImageBuffer::from_fn(w, h, |_, _| {
                let mut px = [0u8; 4];
                for b in px.iter_mut() {
                    st ^= st << 13;
                    st ^= st >> 7;
                    st ^= st << 17;
                    *b = (st % 256) as u8;
                }
                Rgba(px)
            });
            for m in PaletteMode::ALL {
                let Some(table) = m.fixed_table() else {
                    continue; // Off/Auto/Gen16 have no fixed table
                };
                for d in Dither::ALL {
                    let s = ResolvedSettings {
                        pixel: "normal".parse().unwrap(),
                        palette: m,
                        dither: d,
                        ..ResolvedSettings::resolve(&Default::default(), &Default::default())
                    };
                    let out = generate_from_image(&img, &s, (320, 180));
                    for (x, y, px) in out.enumerate_pixels() {
                        let src_a = img.get_pixel(x, y).0[3];
                        if src_a <= 8 {
                            prop_assert_eq!(
                                px.0,
                                [0, 0, 0, 0],
                                "{m}/{d} @({x},{y})",
                                m = m,
                                d = d,
                                x = x,
                                y = y
                            );
                            continue;
                        }
                        prop_assert!(
                            table.contains(px),
                            "output {:?} @({}, {}) outside the {} table ({})",
                            px.0, x, y, m, d
                        );
                    }
                }
            }
        }
    }

    /// S3.5: dither is *honored* on `normal` (the wiring gap S3 exposed).
    /// On photo128x80, (vga, fs) must differ from (vga, none) in bytes while
    /// the shipped PNG colors stay in the VGA table (encode's re-map is
    /// idempotent on a full 256-entry table); same byte assertion for
    /// (auto, fs) — where the dither's *canvas* colors are inside the
    /// auto palette and the shipped PNG colors are the encode's own
    /// median_cut(255) re-map entries (the v0.1 Auto encode contract).
    #[test]
    fn normal_dither_changes_bytes_stays_in_palette() {
        use image::Rgba;
        let src = testsrc::photo128x80();
        let run = |mode: PaletteMode, dither: Dither| -> (RgbaImage, Vec<u8>) {
            let s = ResolvedSettings {
                pixel: "normal".parse().unwrap(),
                palette: mode,
                dither,
                ..ResolvedSettings::resolve(&Default::default(), &Default::default())
            };
            let canvas = generate_from_image(&src, &s, (320, 180));
            let bytes = encode_variant(&canvas, mode);
            (canvas, bytes)
        };

        // (vga, fs) vs (vga, none)
        let (_, vga_none) = run(PaletteMode::Vga, Dither::None);
        let (vga_canvas, vga_fs) = run(PaletteMode::Vga, Dither::FloydSteinberg);
        assert_ne!(vga_none, vga_fs, "vga: dither must change the bytes");
        let vga = PaletteMode::Vga.fixed_table().unwrap();
        let back = decode_any(&vga_fs).unwrap();
        for px in back.pixels() {
            if px.0[3] <= 8 {
                continue;
            }
            assert!(
                vga.contains(&Rgba(px.0)),
                "vga-fs: {px:?} outside the VGA table"
            );
        }
        // (the canvas itself is on the table too)
        for px in vga_canvas.pixels() {
            if px.0[3] <= 8 {
                continue;
            }
            assert!(vga.contains(&Rgba(px.0)), "vga-fs canvas: {px:?} off-table");
        }

        // (auto, fs) vs (auto, none)
        let (_, auto_none) = run(PaletteMode::Auto, Dither::None);
        let (auto_canvas, auto_fs) = run(PaletteMode::Auto, Dither::FloydSteinberg);
        assert_ne!(auto_none, auto_fs, "auto: dither must change the bytes");
        let allow = PaletteMode::Auto.accepts(&src).unwrap();
        for px in auto_canvas.pixels() {
            if px.0[3] <= 8 {
                continue;
            }
            assert!(
                allow.contains(&Rgba(px.0)),
                "auto-fs canvas: {px:?} off-palette"
            );
        }
        let cut = pipeline::median_cut(&auto_canvas, 255);
        let back = decode_any(&auto_fs).unwrap();
        for px in back.pixels() {
            if px.0[3] <= 8 {
                continue;
            }
            assert!(
                cut.contains(&Rgba(px.0)),
                "auto-fs PNG: {px:?} not a re-map entry"
            );
        }
    }

    /// S3.5 determinism: two runs of each new `normal` path produce equal
    /// bytes — every (palette, dither) cell except the (auto, none) early
    /// return, which the v0.1 golden already pins.
    #[test]
    fn normal_paths_are_deterministic() {
        let src = testsrc::photo128x80();
        for m in PaletteMode::ALL {
            for d in Dither::ALL {
                if m == PaletteMode::Auto && d == Dither::None {
                    continue; // v0.1-identical early return (golden-pinned)
                }
                let s = ResolvedSettings {
                    pixel: "normal".parse().unwrap(),
                    palette: m,
                    dither: d,
                    ..ResolvedSettings::resolve(&Default::default(), &Default::default())
                };
                let b1 = encode_variant(&generate_from_image(&src, &s, (320, 180)), m);
                let b2 = encode_variant(&generate_from_image(&src, &s, (320, 180)), m);
                assert_eq!(b1, b2, "{m}/{d} not deterministic");
            }
        }
    }

    /// S3.5 goldens: `normal` preset end-to-end on photo128x80 — one per
    /// representative cell: dithered auto, dithered vga, and the two named
    /// nearest cases (nes, gen16). Checked in under tests/golden/normal/;
    /// regenerate deliberately with PIX_UPDATE_GOLDENS=1.
    #[test]
    fn normal_golden_cells() {
        use PaletteMode::*;
        use std::io::Write as _;
        let cells: [(&str, PaletteMode, Dither); 4] = [
            ("auto-fs", Auto, Dither::FloydSteinberg),
            ("vga-fs", Vga, Dither::FloydSteinberg),
            ("nes-none", Nes, Dither::None),
            ("gen16-none", Gen16, Dither::None),
        ];
        let src = testsrc::photo128x80();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/normal");
        let regenerate = std::env::var_os("PIX_UPDATE_GOLDENS").is_some();
        if regenerate {
            std::fs::create_dir_all(&dir).unwrap();
        }
        for (cell, m, d) in cells {
            let s = ResolvedSettings {
                pixel: "normal".parse().unwrap(),
                palette: m,
                dither: d,
                ..ResolvedSettings::resolve(&Default::default(), &Default::default())
            };
            let bytes = encode_variant(&generate_from_image(&src, &s, (320, 180)), m);
            let path = dir.join(format!("{cell}.png"));
            if regenerate {
                let mut f = std::fs::File::create(&path).unwrap();
                f.write_all(&bytes).unwrap();
                continue;
            }
            let want = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "missing golden {}: {e} (write it with PIX_UPDATE_GOLDENS=1)",
                    path.display()
                )
            });
            assert_eq!(
                bytes, want,
                "golden {cell} differs — intentional? re-run with PIX_UPDATE_GOLDENS=1"
            );
        }
    }

    /// S3.5 cache: a `normal` variant at (Q, D) gets the same v2 key shape as
    /// any other variant — the key scheme already covers it, so this just
    /// pins the normal names (the (auto, none) default keeps the v0.1 shape).
    #[test]
    fn normal_variant_path_shape() {
        let p = |palette: PaletteMode, dither: Dither| {
            let s = ResolvedSettings {
                pixel: "normal".parse().unwrap(),
                palette,
                dither,
                ..ResolvedSettings::resolve(&Default::default(), &Default::default())
            };
            variant_path(
                &Paths::with_root("/tmp/x".as_ref()),
                "id1",
                &s,
                (1920, 1080),
            )
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
        };
        assert_eq!(
            p(PaletteMode::Auto, Dither::None),
            format!("v{PIPELINE_VERSION}-normal-fit-1920x1080-auto.png")
        );
        assert_eq!(
            p(PaletteMode::Vga, Dither::FloydSteinberg),
            format!("v{PIPELINE_VERSION}-normal-fit-1920x1080-vga-fs.png")
        );
        assert_eq!(
            p(PaletteMode::Off, Dither::Atkinson),
            format!("v{PIPELINE_VERSION}-normal-fit-1920x1080-off-atkinson.png")
        );
    }

    /// Named modes encode with exactly their table (deduped), index 0 =
    /// transparent slot. GB and NES (black-heavy) pin the contract.
    #[test]
    fn named_encode_uses_exact_table() {
        for (mode, table) in [
            (PaletteMode::Gb, PaletteMode::Gb.fixed_table().unwrap()),
            (PaletteMode::Nes, PaletteMode::Nes.fixed_table().unwrap()),
        ] {
            let canvas = generate_from_image(
                &testsrc::photo128x80(),
                &ResolvedSettings {
                    palette: mode,
                    ..settings("16")
                },
                (320, 180),
            );
            let bytes = encode_variant(&canvas, mode);
            let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
            let reader = decoder.read_info().unwrap();
            let mut want: Vec<u8> = vec![0, 0, 0]; // index 0 placeholder
            let mut seen: Vec<[u8; 3]> = Vec::new();
            for c in table {
                let rgb = [c.0[0], c.0[1], c.0[2]];
                if !seen.contains(&rgb) {
                    seen.push(rgb);
                    want.extend_from_slice(&rgb);
                }
            }
            assert_eq!(
                reader.info().palette.as_deref(),
                Some(want.as_slice()),
                "{mode} PLTE"
            );
            assert!(bytes.len() < 20 * 1024, "{mode} variant size");
        }
    }

    #[test]
    fn variant_is_tiny() {
        // the R8 smoke budget: a 1080p @ 16 variant is far below 20 KB
        let photo = image::ImageBuffer::from_fn(1920, 1080, |x, y| {
            image::Rgba([
                (x as f64 * 0.01).sin() as u8,
                (y % 251) as u8,
                ((x * y) % 97) as u8,
                255,
            ])
        });
        let v = generate_from_image(&photo, &settings("16"), (1920, 1080));
        assert_eq!((v.width(), v.height()), (120, 68)); // ceil(1920/16)×ceil(1080/16)
        let bytes = encode_variant(&v, PaletteMode::Auto);
        assert!(
            bytes.len() < 20 * 1024,
            "variant {} KB > 20 KB budget",
            bytes.len() / 1024
        );
        // decodes back to the same dims
        let back = image::load_from_memory(&bytes).unwrap();
        assert_eq!((back.width(), back.height()), (120, 68));
    }

    #[test]
    fn lazy_generate_caches() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::with_root(dir.path());
        paths.ensure_dirs().unwrap();
        let id = "test0001";
        std::fs::create_dir_all(paths.entry_dir(id)).unwrap();
        let img = testsrc::photo128x80();
        let mut buf = Vec::new();
        {
            let enc = image::codecs::png::PngEncoder::new(&mut buf);
            image::ImageEncoder::write_image(
                enc,
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        }
        std::fs::write(paths.entry_dir(id).join("original.png"), &buf).unwrap();
        let s = settings("16");
        let (p1, hit1) = generate(&paths, id, &s, (320, 180)).unwrap();
        let (p2, hit2) = generate(&paths, id, &s, (320, 180)).unwrap();
        assert!(!hit1 && hit2 && p1 == p2);
    }
}
