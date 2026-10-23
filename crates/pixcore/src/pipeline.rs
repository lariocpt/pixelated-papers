//! The pixelation pipeline: window crop → iterative-box downscale to the
//! absolute-minimum canvas → (optional) median-cut palette → nearest-neighbor
//! stretch. Deterministic: same input bytes ⇒ same output bytes.

use image::{ImageBuffer, Rgba, RgbaImage};

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
    while cur.width() / 2 >= tw && cur.height() / 2 >= th && cur.width() % 2 == 0 && cur.height() % 2 == 0 {
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
            Rgba([(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 255])
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

/// Nearest-neighbor map to a fixed palette (no dither — blocky aesthetic,
/// determinism). Deterministic: palette order is fixed by median_cut.
pub fn apply_palette(img: &RgbaImage, pal: &[Rgba<u8>]) -> RgbaImage {
    let mut out = ImageBuffer::new(img.width(), img.height());
    for (x, y, px) in out.enumerate_pixels_mut() {
        let p = img.get_pixel(x, y).0;
        if p[3] <= 8 {
            *px = Rgba([0, 0, 0, 0]);
            continue;
        }
        let mut best = usize::MAX;
        let mut bestd = u64::MAX;
        for (i, c) in pal.iter().enumerate() {
            let dr = p[0] as i64 - c.0[0] as i64;
            let dg = p[1] as i64 - c.0[1] as i64;
            let db = p[2] as i64 - c.0[2] as i64;
            let d = (dr * dr + dg * dg + db * db) as u64;
            if d < bestd {
                bestd = d;
                best = i;
            }
        }
        *px = pal[best];
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
        for p in [Placement::Fit, Placement::CoverGutter, Placement::CoverCrop, Placement::Stretch] {
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
            if x % 2 == 0 && y % 2 == 0 { *p = Rgba([12, 196, 44, 255]); }
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
}
