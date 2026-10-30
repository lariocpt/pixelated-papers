//! The pixelation pipeline: window crop → iterative-box downscale to the
//! absolute-minimum canvas → (optional) palette quantize + dither →
//! nearest-neighbor stretch. Deterministic: same input bytes ⇒ same output
//! bytes, for every stage including quantize/dither.

use image::{ImageBuffer, Rgba, RgbaImage};

use crate::palette::{self, Dither, PaletteMode};
use crate::placement::{Rect, View};

/// Iterate halve-and-box-average until ≤ 2× the target, then box to exact.
/// `window` selects the source sub-rect first (cover modes crop here).
/// Canvas size = `⌈dest.w/B⌉ × ⌈dest.h/B⌉` — the absolute minimum image that
/// still covers the content box at block size B (R1/R8).
pub fn downscale_to_variant(src: &RgbaImage, view: &View, block: u32) -> RgbaImage {
    let b = block.max(1);
    let cw = view.dest.w.div_ceil(b).max(1);
    let ch = view.dest.h.div_ceil(b).max(1);
    let window = crop(src, view.window);
    if window.width() == cw && window.height() == ch {
        return window;
    }
    downscale(&window, cw, ch)
}

/// Crop a rect (clamped to the image).
pub fn crop(src: &RgbaImage, r: Rect) -> RgbaImage {
    let w = r.w.min(src.width()).max(1);
    let h = r.h.min(src.height()).max(1);
    let x = r.x.min(src.width() - 1);
    let y = r.y.min(src.height() - 1);
    let w = w.min(src.width() - x);
    let h = h.min(src.height() - y);
    let mut out = ImageBuffer::new(w, h);
    for (ox, oy, px) in out.enumerate_pixels_mut() {
        *px = *src.get_pixel(x + ox, y + oy);
    }
    out
}

/// Area-average downscale: iterative halving (box2) then a final exact box.
/// Never upscales; equal size ⇒ clone.
pub fn downscale(src: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let (sw, sh) = (src.width(), src.height());
    let tw = tw.clamp(1, sw);
    let th = th.clamp(1, sh);
    if tw == sw && th == sh {
        return src.clone();
    }
    let mut cur = src.clone();
    // Halve while both dims stay ≥ 2× the target (each pass is a cheap
    // 2×2 box average). One dim may still be larger — the final box handles it.
    while cur.width() / 2 >= tw
        && cur.height() / 2 >= th
        && cur.width() % 2 == 0
        && cur.height() % 2 == 0
    {
        cur = box_halve(&cur);
    }
    box_to(&cur, tw, th)
}

fn box_halve(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width(), src.height());
    let (nw, nh) = (w / 2, h / 2);
    let mut out = ImageBuffer::new(nw, nh);
    for (x, y, px) in out.enumerate_pixels_mut() {
        let mut acc = [0u32; 4];
        let [a, b, c, d] = [
            src.get_pixel(x * 2, y * 2).0,
            src.get_pixel(x * 2 + 1, y * 2).0,
            src.get_pixel(x * 2, y * 2 + 1).0,
            src.get_pixel(x * 2 + 1, y * 2 + 1).0,
        ];
        for i in 0..4 {
            acc[i] = (a[i] as u32 + b[i] as u32 + c[i] as u32 + d[i] as u32 + 2) / 4;
        }
        *px = Rgba([acc[0] as u8, acc[1] as u8, acc[2] as u8, acc[3] as u8]);
    }
    out
}

/// Box-average `src → (tw, th)` with a single pass (src ≥ target in both dims).
fn box_to(src: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let (sw, sh) = (src.width() as u64, src.height() as u64);
    let (tw, th) = (tw as u64, th as u64);
    let mut out = ImageBuffer::new(tw as u32, th as u32);
    for (tx, ty, px) in out.enumerate_pixels_mut() {
        let x0 = tx as u64 * sw / tw;
        let x1 = ((tx as u64 + 1) * sw / tw).max(x0 + 1).min(sw);
        let y0 = ty as u64 * sh / th;
        let y1 = ((ty as u64 + 1) * sh / th).max(y0 + 1).min(sh);
        let mut acc = [0u64; 4];
        let mut n = 0u64;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let p = src.get_pixel(xx as u32, yy as u32).0;
                for i in 0..4 {
                    acc[i] += p[i] as u64;
                }
                n += 1;
            }
        }
        let s = n.max(1);
        *px = Rgba([
            ((acc[0] + s / 2) / s) as u8,
            ((acc[1] + s / 2) / s) as u8,
            ((acc[2] + s / 2) / s) as u8,
            ((acc[3] + s / 2) / s) as u8,
        ]);
    }
    out
}

/// Deterministic median-cut over opaque pixels, ≤ `max_colors` entries.
/// Determinism: sort tie-breaks by value (input order never matters).
pub fn median_cut(img: &RgbaImage, max_colors: usize) -> Vec<Rgba<u8>> {
    let max_colors = max_colors.clamp(1, 256);
    let mut pts: Vec<[u16; 3]> = img
        .pixels()
        .filter(|p| p.0[3] > 8) // skip near-transparent
        .map(|p| [p.0[0] as u16, p.0[1] as u16, p.0[2] as u16])
        .collect();
    if pts.is_empty() {
        return vec![Rgba([0, 0, 0, 255])];
    }
    pts.sort_unstable();
    let mut buckets = vec![pts];
    while buckets.len() < max_colors {
        // split the bucket with the longest channel range; ties: most pixels,
        // then smallest first element (total order ⇒ deterministic)
        let best = buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .max_by_key(|(_, b)| (longest_range(b), b.len(), std::cmp::Reverse(b[0])))
            .map(|(i, _)| i);
        let Some(i) = best else { break };
        let bucket = buckets.remove(i);
        let axis = longest_axis(&bucket);
        let mut bucket = bucket;
        bucket.sort_by_key(|p| (p[axis], p[0], p[1], p[2]));
        let mid = bucket.len() / 2;
        buckets.insert(i, bucket[..mid].to_vec());
        buckets.insert(i + 1, bucket[mid..].to_vec());
    }
    let mut pal: Vec<Rgba<u8>> = buckets
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let mut acc = [0u64; 3];
            for p in &b {
                for i in 0..3 {
                    acc[i] += p[i] as u64;
                }
            }
            let n = b.len() as u64;
            Rgba([
                (acc[0] / n) as u8,
                (acc[1] / n) as u8,
                (acc[2] / n) as u8,
                255,
            ])
        })
        .collect();
    pal.sort_unstable_by_key(|c| (c.0[0], c.0[1], c.0[2], c.0[3]));
    pal
}

fn longest_axis(pts: &[[u16; 3]]) -> usize {
    let mut best = 0usize;
    let mut best_r = 0u16;
    for axis in 0..3 {
        let lo = pts.iter().map(|p| p[axis]).min().unwrap_or(0);
        let hi = pts.iter().map(|p| p[axis]).max().unwrap_or(0);
        if hi - lo > best_r {
            best_r = hi - lo;
            best = axis;
        }
    }
    best
}

fn longest_range(pts: &[[u16; 3]]) -> u16 {
    let mut best = 0u16;
    for axis in 0..3 {
        let lo = pts.iter().map(|p| p[axis]).min().unwrap_or(0);
        let hi = pts.iter().map(|p| p[axis]).max().unwrap_or(0);
        best = best.max(hi - lo);
    }
    best
}

/// Nearest-neighbor map to a fixed palette. Dithering is a separate concern
/// — see [`quantize`] for the mode/dither stage. Deterministic: ties resolve
/// to the lowest palette index, palette order is fixed by the table/median_cut.
pub fn apply_palette(img: &RgbaImage, pal: &[Rgba<u8>]) -> RgbaImage {
    let mut out = ImageBuffer::new(img.width(), img.height());
    for (x, y, px) in out.enumerate_pixels_mut() {
        let p = img.get_pixel(x, y).0;
        if p[3] <= 8 {
            *px = Rgba([0, 0, 0, 0]);
            continue;
        }
        let best = nearest_index(pal, p[0] as i64, p[1] as i64, p[2] as i64);
        *px = pal[best];
    }
    out
}

pub(crate) fn nearest_index(pal: &[Rgba<u8>], r: i64, g: i64, b: i64) -> usize {
    let mut best = 0usize;
    let mut bestd = u64::MAX;
    for (i, c) in pal.iter().enumerate() {
        let dr = r - c.0[0] as i64;
        let dg = g - c.0[1] as i64;
        let db = b - c.0[2] as i64;
        let d = (dr * dr + dg * dg + db * db) as u64;
        if d < bestd {
            bestd = d;
            best = i;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Quantize stage (PLAN-30 §A.2): palette mapping + deterministic dithering.
// Integer math only — no floats anywhere in these paths.
// ---------------------------------------------------------------------------

/// Map the canvas onto its palette and dither it, in canvas space (block
/// space for presets). Pure and deterministic at fixed input:
///
/// - `Off` → identity (no palette, no dither — today's `palette = false`).
/// - `Auto` → `median_cut(256)` + nearest map: byte-identical to v0.1 when
///   `dither` is `None`. `Gen16` → `median_cut(16)` snapped to RGB555.
///   Named modes use their fixed table ([`PaletteMode::fixed_table`]).
/// - `dither`: FS/Atkinson error diffusion (u16 accumulators over R,G,B,
///   row-major, truncated integer weights); Bayer2/4/8 ordered threshold
///   at `(x & (n-1), y & (n-1))`.
///
/// Alpha: `α ≤ 8` stays fully transparent and is never dithered (same rule
/// as [`apply_palette`]); every mapped pixel gets the palette entry's α=255.
/// Output colors ⊆ [`PaletteMode::accepts`] (plus the transparent entry).
pub fn quantize(img: &RgbaImage, mode: PaletteMode, dither: Dither) -> RgbaImage {
    if mode == PaletteMode::Off {
        return img.clone();
    }
    let pal: Vec<Rgba<u8>> = match mode {
        PaletteMode::Auto => median_cut(img, 256),
        PaletteMode::Gen16 => median_cut(img, 16)
            .into_iter()
            .map(palette::snap_rgb555)
            .collect(),
        named => named
            .fixed_table()
            .expect("named mode has a table")
            .to_vec(),
    };
    match dither {
        Dither::None => apply_palette(img, &pal),
        Dither::FloydSteinberg => diffuse(img, &pal, false),
        Dither::Atkinson => diffuse(img, &pal, true),
        Dither::Bayer2 | Dither::Bayer4 | Dither::Bayer8 => ordered_dither(img, &pal, dither),
    }
}

/// Floyd–Steinberg (7/16, 3/16, 5/16, 1/16) or Atkinson (six neighbours at
/// 1/8 — 1/4 of the error deliberately discarded, the MacPaint look).
/// Row-major with u16 accumulators: input ≤ 255 and bounded diffusion keep
/// values inside u16 by a wide margin; negatives clamp to 0, over-range to
/// 65535 (integer truncation toward zero — the documented determinism
/// contract, mirroring the kernel weights exactly).
fn diffuse(img: &RgbaImage, pal: &[Rgba<u8>], atkinson: bool) -> RgbaImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut acc: Vec<u16> = img
        .pixels()
        .flat_map(|p| [p.0[0] as u16, p.0[1] as u16, p.0[2] as u16])
        .collect();
    let mut out = ImageBuffer::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let src = img.get_pixel(x as u32, y as u32).0;
            if src[3] <= 8 {
                out.put_pixel(x as u32, y as u32, Rgba([0, 0, 0, 0]));
                continue;
            }
            let cur = [
                acc[3 * i] as i64,
                acc[3 * i + 1] as i64,
                acc[3 * i + 2] as i64,
            ];
            let chosen = pal[nearest_index(pal, cur[0], cur[1], cur[2])];
            out.put_pixel(x as u32, y as u32, chosen);
            let taps: &[(i64, i64, i64)] = if atkinson {
                &[
                    (1, 0, 1),
                    (2, 0, 1),
                    (-1, 1, 1),
                    (0, 1, 1),
                    (1, 1, 1),
                    (0, 2, 1),
                ]
            } else {
                &[(1, 0, 7), (-1, 1, 3), (0, 1, 5), (1, 1, 1)]
            };
            let denom = if atkinson { 8 } else { 16 };
            for (ch, &curv) in cur.iter().enumerate() {
                let err = curv - chosen.0[ch] as i64;
                for &(dx, dy, wgt) in taps {
                    let (tx, ty) = (x as i64 + dx, y as i64 + dy);
                    if tx < 0 || ty < 0 || tx >= w as i64 || ty >= h as i64 {
                        continue;
                    }
                    let t = (ty as usize * w + tx as usize) * 3 + ch;
                    let v = acc[t] as i64 + (err * wgt) / denom;
                    acc[t] = v.clamp(0, u16::MAX as i64) as u16;
                }
            }
        }
    }
    out
}

const BAYER2: [u8; 4] = [0, 2, 3, 1];
const BAYER4: [u8; 16] = [
    0, 8, 2, 10, //
    12, 4, 14, 6, //
    3, 11, 1, 9, //
    15, 7, 13, 5,
];
const BAYER8: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, //
    48, 16, 56, 24, 50, 18, 58, 26, //
    12, 44, 4, 36, 14, 46, 6, 38, //
    60, 28, 52, 20, 62, 30, 54, 22, //
    3, 35, 11, 43, 1, 33, 9, 41, //
    51, 19, 59, 27, 49, 17, 57, 25, //
    15, 47, 7, 39, 13, 45, 5, 37, //
    63, 31, 55, 23, 61, 29, 53, 21,
];

fn bayer_matrix(d: Dither) -> (usize, &'static [u8]) {
    match d {
        Dither::Bayer2 => (2, &BAYER2),
        Dither::Bayer4 => (4, &BAYER4),
        Dither::Bayer8 => (8, &BAYER8),
        _ => (1, &BAYER2), // unreachable; callers match Bayer* first
    }
}

/// Bayer threshold offset for pixel (x, y): maps matrix cell t ∈ [0, n²) to
/// a 0..=n²-level brightness bias `(t·256 + n²/2) / n²`, added to the
/// pre-map color and clamped per channel.
fn bayer_offset(d: Dither, x: u32, y: u32) -> i64 {
    let (n, m) = bayer_matrix(d);
    let t = m[((y as usize & (n - 1)) * n) + (x as usize & (n - 1))] as i64;
    let n2 = (n * n) as i64;
    (t * 256 + n2 / 2) / n2
}

/// Ordered dither: threshold-shift each pixel by its Bayer cell, then
/// nearest-map. No state carried across pixels — trivially row-major and
/// deterministic.
fn ordered_dither(img: &RgbaImage, pal: &[Rgba<u8>], d: Dither) -> RgbaImage {
    let mut out = ImageBuffer::new(img.width(), img.height());
    for (x, y, px) in out.enumerate_pixels_mut() {
        let p = img.get_pixel(x, y).0;
        if p[3] <= 8 {
            *px = Rgba([0, 0, 0, 0]);
            continue;
        }
        let off = bayer_offset(d, x, y);
        let r = (p[0] as i64 + off).clamp(0, 255);
        let g = (p[1] as i64 + off).clamp(0, 255);
        let b = (p[2] as i64 + off).clamp(0, 255);
        *px = pal[nearest_index(pal, r, g, b)];
    }
    out
}

#[cfg(test)]
pub mod testsrc {
    use super::*;

    /// Deterministic 64×64 test source: diagonal gradient + solid quadrants
    /// + hard edges + alpha band. Shared by golden tests.
    pub fn gradient64() -> RgbaImage {
        let mut img = ImageBuffer::new(64, 64);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let base = [
                ((x * 4) % 256) as u8,
                ((y * 6) % 256) as u8,
                (((x + y) * 3) % 256) as u8,
            ];
            let a = if x < 8 { 128 } else { 255 };
            *px = Rgba([base[0], base[1], base[2], a]);
        }
        img
    }

    /// Deterministic 128×80 photo-ish source: sine gradient (no hard edges).
    pub fn photo128x80() -> RgbaImage {
        let mut img = ImageBuffer::new(128, 80);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let r = ((x as f64 * 0.03).sin() * 60.0 + 160.0) as u8;
            let g = ((y as f64 * 0.07).cos() * 40.0 + 120.0) as u8;
            let b = (((x + y) as f64 * 0.02).sin() * 50.0 + 120.0) as u8;
            *px = Rgba([r, g, b, 255]);
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::{Placement, view};
    use proptest::prelude::*;

    #[test]
    fn variant_canvas_is_absolute_minimum() {
        // big source (real case): canvas is exactly ⌈dest/B⌉ per axis
        let big = testsrc::photo128x80();
        let big = downscale(&big, 128, 80); // already that size; identity
        let _ = big;
        let src = image::ImageBuffer::from_fn(512, 512, |x, y| {
            image::Rgba([(x % 250) as u8, (y % 250) as u8, 100, 255])
        });
        for p in [
            Placement::Fit,
            Placement::CoverGutter,
            Placement::CoverCrop,
            Placement::Stretch,
        ] {
            let v = view((512, 512), (1920, 1080), p, 24);
            let var = downscale_to_variant(&src, &v, 16);
            assert_eq!(var.width(), v.dest.w.div_ceil(16), "{p}");
            assert_eq!(var.height(), v.dest.h.div_ceil(16), "{p}");
        }
        // small source (upscaling case): never upscale — canvas clamps to the
        // window; composite stretches it to dest
        let small = testsrc::gradient64();
        let v = view((64, 64), (1920, 1080), Placement::Fit, 0);
        let var = downscale_to_variant(&small, &v, 16);
        assert_eq!((var.width(), var.height()), (64, 64));
    }

    #[test]
    fn block16_averages_flat_colors_exactly() {
        let mut img = ImageBuffer::from_pixel(64, 64, Rgba([10, 200, 40, 255]));
        for (x, y, p) in img.enumerate_pixels_mut() {
            if x % 2 == 0 && y % 2 == 0 {
                *p = Rgba([12, 196, 44, 255]);
            }
        }
        let down = downscale(&img, 16, 16);
        // 1/4 of pixels are [12,196,44], 3/4 are [10,200,40] ⇒ weighted mean
        assert_eq!(down.get_pixel(3, 3).0, [11, 199, 41, 255]);
    }

    #[test]
    fn pipeline_is_deterministic_and_idempotent_downscaling() {
        let img = testsrc::photo128x80();
        let v = view((128, 80), (320, 180), Placement::Fit, 0);
        let a = downscale_to_variant(&img, &v, 8);
        let b = downscale_to_variant(&img, &v, 8);
        assert_eq!(a, b);
    }

    #[test]
    fn median_cut_respects_max_colors_and_alpha() {
        let img = testsrc::photo128x80();
        let pal = median_cut(&img, 16);
        assert!((1..=16).contains(&pal.len()));
        let img2 = testsrc::gradient64(); // has alpha<255 band
        let pal2 = median_cut(&img2, 8);
        assert!((1..=8).contains(&pal2.len()));
        let mapped = apply_palette(&img, &pal);
        assert_eq!(mapped.width(), img.width());
    }

    proptest! {
        #[test]
        fn downscale_dims_and_bytes_bounded(
            w in 4u32..300, h in 4u32..300, tw in 1u32..200, th in 1u32..200
        ) {
            let img = ImageBuffer::from_fn(w, h, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, 77, 255]));
            let out = downscale(&img, tw, th);
            prop_assert_eq!(out.width(), tw.clamp(1, w));
            prop_assert_eq!(out.height(), th.clamp(1, h));
        }

        #[test]
        fn median_cut_deterministic(seed in 0u8..4u8) {
            let img = match seed {
                0 => testsrc::gradient64(),
                1 => testsrc::photo128x80(),
                _ => testsrc::gradient64(),
            };
            let a = median_cut(&img, 32);
            let b = median_cut(&img, 32);
            prop_assert_eq!(a, b);
        }
    }

    // -----------------------------------------------------------------------
    // quantize stage (PLAN-30 §A.2/§A.4)
    // -----------------------------------------------------------------------

    #[test]
    fn quantize_off_is_identity() {
        let img = testsrc::gradient64();
        for d in Dither::ALL {
            assert_eq!(quantize(&img, PaletteMode::Off, d), img, "Off/{d}");
        }
    }

    #[test]
    fn quantize_output_is_inside_accepts_set() {
        for src in [testsrc::photo128x80(), testsrc::gradient64()] {
            for m in PaletteMode::ALL {
                let allow = m.accepts(&src);
                for d in Dither::ALL {
                    let out = quantize(&src, m, d);
                    assert_eq!(out.dimensions(), src.dimensions(), "{m}/{d}");
                    for (x, y, px) in out.enumerate_pixels() {
                        let s = src.get_pixel(x, y).0;
                        if s[3] <= 8 {
                            assert_eq!(px.0, [0, 0, 0, 0], "{m}/{d} @({x},{y})");
                            continue;
                        }
                        match &allow {
                            None => assert_eq!(px.0, s, "{m}/{d} Off must not change"),
                            Some(set) => assert!(
                                set.contains(px),
                                "{m}/{d}: output {:?} @({x},{y}) outside palette",
                                px.0
                            ),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn quantize_keeps_transparent_pixels_and_emits_no_error() {
        let mut src = ImageBuffer::from_pixel(4, 2, Rgba([255, 0, 0, 255]));
        src.put_pixel(1, 0, Rgba([7, 200, 30, 0])); // hole
        src.put_pixel(2, 0, Rgba([7, 200, 30, 8])); // hole (α ≤ 8 boundary)
        for m in PaletteMode::ALL {
            if m == PaletteMode::Off {
                continue; // Off is identity — pixels pass through untouched
            }
            for d in Dither::ALL {
                let out = quantize(&src, m, d);
                assert_eq!(out.get_pixel(1, 0).0, [0, 0, 0, 0], "{m}/{d}");
                assert_eq!(out.get_pixel(2, 0).0, [0, 0, 0, 0], "{m}/{d}");
            }
        }
    }

    #[test]
    fn quantize_is_byte_deterministic() {
        // Determinism is a contract (PLAN-30 A.2): two runs on the same
        // input produce identical pixels for every mode × dither cell.
        for src in [testsrc::photo128x80(), testsrc::gradient64()] {
            for m in PaletteMode::ALL {
                for d in Dither::ALL {
                    assert_eq!(quantize(&src, m, d), quantize(&src, m, d), "{m}/{d}");
                }
            }
        }
    }

    #[test]
    fn gen16_output_lives_on_the_rgb555_grid() {
        let src = testsrc::photo128x80();
        for d in Dither::ALL {
            let out = quantize(&src, PaletteMode::Gen16, d);
            let mut distinct: Vec<[u8; 4]> = out.pixels().map(|p| p.0).collect();
            distinct.sort_unstable();
            distinct.dedup();
            assert!(distinct.len() <= 16, "gen16/{d}: {} colors", distinct.len());
            for c in distinct {
                for v in c.iter().take(3) {
                    let hi = *v >> 3;
                    assert_eq!(*v, (hi << 3) | (hi >> 2), "gen16/{d} {c:?} off-grid");
                }
            }
        }
    }

    /// Mean-colour drift on a full 0..255 grey ramp (256×8, VGA palette —
    /// it contains the grey ramp itself, so drift isolates the dither).
    /// Contract, measured at write time: both measured 0/channel on this
    /// ramp; the enforced bounds below (FS ≤ 2, Atkinson ≤ 16) leave room
    /// for Atkinson's deliberate behaviour: it discards 1/4 of the error
    /// each step and may drift brighter on other sources. Integer
    /// truncation toward zero is part of the determinism contract, not a
    /// bug to fix with floats.
    #[test]
    fn diffusion_mean_color_drift_is_bounded() {
        let mut ramp = ImageBuffer::new(256, 8);
        for (x, _y, px) in ramp.enumerate_pixels_mut() {
            let v = x as u8;
            *px = Rgba([v, v, v, 255]);
        }
        let mean = |img: &RgbaImage| -> [u64; 3] {
            let n = img.pixels().count() as u64;
            let s = img.pixels().fold([0u64; 3], |mut a, p| {
                a[0] += p.0[0] as u64;
                a[1] += p.0[1] as u64;
                a[2] += p.0[2] as u64;
                a
            });
            [s[0] / n, s[1] / n, s[2] / n]
        };
        let want = mean(&ramp);
        for (d, bound) in [(Dither::FloydSteinberg, 2i64), (Dither::Atkinson, 16)] {
            let out = quantize(&ramp, PaletteMode::Vga, d);
            let got = mean(&out);
            for ch in 0..3 {
                let drift = got[ch] as i64 - want[ch] as i64;
                assert!(
                    drift.abs() <= bound,
                    "{d} channel {ch} drift {drift} > {bound}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Golden cells: 12 palette × dither pairs on the photo source (PLAN-30
    // §A.4). Exact PNG bytes, checked in under goldens/quantize/. Covers
    // every PaletteMode and every Dither at least once:
    //   auto × {none,fs,atkinson,bayer2,bayer4,bayer8} +
    //   {nes,gb,cga,ega,vga,gen16} × none
    // Regenerate deliberately with PIX_UPDATE_GOLDENS=1 cargo test.
    // -----------------------------------------------------------------------

    fn png_bytes(img: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        image::ImageEncoder::write_image(
            enc,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        out
    }

    #[test]
    fn quantize_golden_cells() {
        use PaletteMode::*;
        use std::io::Write as _;
        let cells: [(&str, PaletteMode, Dither); 12] = [
            ("auto__none", Auto, Dither::None),
            ("auto__fs", Auto, Dither::FloydSteinberg),
            ("auto__atkinson", Auto, Dither::Atkinson),
            ("auto__bayer2", Auto, Dither::Bayer2),
            ("auto__bayer4", Auto, Dither::Bayer4),
            ("auto__bayer8", Auto, Dither::Bayer8),
            ("nes__none", Nes, Dither::None),
            ("gb__none", Gb, Dither::None),
            ("cga__none", Cga, Dither::None),
            ("ega__none", Ega, Dither::None),
            ("vga__none", Vga, Dither::None),
            ("gen16__none", Gen16, Dither::None),
        ];
        let src = testsrc::photo128x80();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/quantize");
        let regenerate = std::env::var_os("PIX_UPDATE_GOLDENS").is_some();
        if regenerate {
            std::fs::create_dir_all(&dir).unwrap();
        }
        for (cell, m, d) in cells {
            let bytes = png_bytes(&quantize(&src, m, d));
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
}
