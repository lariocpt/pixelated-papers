//! Variant naming, lazy generation and caching (R7/R8). The variant is the
//! absolute-minimum-size palette PNG for one (preset, placement, output);
//! the cache key carries `PIPELINE_VERSION` so pipeline upgrades invalidate
//! automatically.

use std::fs;
use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use crate::PIPELINE_VERSION;
use crate::error::{PixError, Result};
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
    paths.variants_dir(id).join(format!(
        "v{PIPELINE_VERSION}-{pixel}-{placement}{g}-{W}x{H}-{Q}{D}.png",
        pixel = s.pixel,
        placement = s.placement,
        W = out.0,
        H = out.1,
        Q = s.palette,
        D = d,
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
        // in composite; no downscale, no palette.
        return pipeline::crop(orig, v.window);
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

    /// Acceptance (PLAN-30 A.4): Auto + None is **byte-identical to v0.1**.
    /// tests/golden/v01/ was captured from the untouched v0.1 tree (commit
    /// 29ef2b4) via the public API on the same sources (pixel 8, Fit,
    /// 320×180); sha256 recorded in the wiring commit message.
    #[test]
    fn auto_none_matches_v01_bytes() {
        for (name, src) in [
            ("photo128x80", testsrc::photo128x80()),
            ("gradient64", testsrc::gradient64()),
        ] {
            for (tag, palette) in [("auto", PaletteMode::Auto), ("off", PaletteMode::Off)] {
                let s = ResolvedSettings {
                    pixel: "8".parse().unwrap(),
                    placement: Placement::Fit,
                    palette,
                    ..ResolvedSettings::resolve(&Default::default(), &Default::default())
                };
                let canvas = generate_from_image(&src, &s, (320, 180));
                let bytes = encode_variant(&canvas, palette);
                let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/golden/v01/{name}-{tag}.png"));
                let want = std::fs::read(&golden).expect("v0.1 golden present");
                assert_eq!(
                    bytes, want,
                    "{name}/{tag} differs from the v0.1 capture (regression!)"
                );
            }
        }
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
