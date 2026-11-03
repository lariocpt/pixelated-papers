//! Content-based auto-preset: measure how much **detail** a picture holds
//! and let that refine the block size. This is the analysis half of
//! `Pixel::AutoContent` (the resolve half lives in [`crate::preset`]).
//!
//! [**Backward compat**] This module changes nothing about the shipped
//! `auto` preset, which stays a pure function of the *output size* and is
//! never routed through here. Only the explicit `auto-content` value
//! consults these numbers.
//!
//! [**Determinism**] Everything here is integer arithmetic over the decoded
//! pixels — no floats, no RNG, no clocks. The same source bytes therefore
//! yield the same [`ContentProbe`] on every machine and every run, so a
//! persisted probe is reproducible and a rule upgrade is detectable (via
//! [`CONTENT_RULE_VERSION`]).
//!
//! [**Architecture**] The probe is computed on the **CLI/state side** — at
//! `add`/`pixel`/`default pixel` time — and persisted in the manifest. The
//! daemon never calls this module: it renders from the persisted integer
//! ([`crate::render::ResolvedSettings::content_density`]), so it stays a pure
//! renderer that never decodes an image *to analyse it*.

use serde::{Deserialize, Serialize};

use crate::error::{PixError, Result};
use crate::pipeline;

/// Bump this whenever the statistic or the preset mapping changes. A persisted
/// probe whose `rule` does not equal this version is treated as absent and the
/// pixel falls back to the size-only floor — so an old probe can never be
/// silently reinterpreted by a new rule.
pub const CONTENT_RULE_VERSION: u32 = 1;

/// The largest analysis edge (px). A source is area-averaged down to at most
/// this wide/tall **before** the statistic is measured, so the cost is bounded
/// by O(edge²) regardless of source size. This also makes the reading stable
/// across resolutions of a *real* picture (its features keep their size as the
/// file gets bigger, so a 4K and a 1080p copy read alike) — though it is not
/// invariant for a full-frame pattern whose features rescale with the frame
/// (see the aliasing note on [`content_density`]). 512 is a balance: wide enough
/// that fine synthetic test patterns (≤ this edge) are measured at full detail
/// with no averaging, and a calm photograph and a busy one keep a ~10× gap
/// without the aggressive smoothing of a smaller edge (which would alias real
/// texture away).
pub const ANALYSIS_EDGE: u32 = 512;

/// What we learned about one wallpaper's own content, measured once at
/// add/set/pixel time and stored in the manifest as provenance.
///
/// [`Self::density`] is the measured statistic (mean absolute neighbour
/// gradient of the downscaled grayscale, scaled ×16 for sub-integer
/// resolution); [`Self::rule`] is the
/// [`CONTENT_RULE_VERSION`] it was measured under. Together they let render
/// re-derive the preset deterministically and let a rule change invalidate old
/// probes rather than misread them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentProbe {
    /// [`CONTENT_RULE_VERSION`] in force when `density` was measured.
    pub rule: u32,
    /// Mean absolute neighbour gradient of the downscaled grayscale, scaled
    /// ×16 (so `density / 16` is the raw mean, a smooth photo ~20, a textured
    /// one ~150). Higher ⇒ more detail (more per-pixel change); 0 ⇒ flat.
    pub density: u32,
}

impl ContentProbe {
    /// A fresh probe under the current rule.
    pub fn current(density: u32) -> Self {
        Self {
            rule: CONTENT_RULE_VERSION,
            density,
        }
    }
}

/// Decode an image file and measure its content detail. Returns a [`Result`]
/// so a caller can decide what to do with an undecodable file (the CLI treats
/// a probe failure as "no probe" and falls back to the size floor).
pub fn analyze_file(path: &std::path::Path) -> Result<ContentProbe> {
    let img = image::open(path)
        .map_err(|e| PixError::Invalid(format!("analyse {}: {e}", path.display())))?;
    Ok(ContentProbe::current(content_density(&img.to_rgba8())))
}

/// The statistic: **mean absolute neighbour gradient** (horizontal + vertical)
/// of the image's integer luma, measured after an area-average downscale to
/// [`ANALYSIS_EDGE`]. Pure integer — determinism by construction.
///
/// Higher ⇒ the picture changes a lot per pixel (photo texture, hard edges);
/// `0` ⇒ perfectly flat (solid, a smooth gradient). The content rule maps
/// **higher detail ⇒ a finer preset** (detail survives small blocks) and
/// **lower detail ⇒ a coarser preset** (a calm image loses nothing at big
/// blocks), then clamps to be no finer than the size-only floor (see
/// [`crate::preset::Pixel::resolve_content_for`]).
///
/// [**Failure mode — aliasing**] An area-average downscale washes very fine
/// high-frequency texture (a per-pixel checkerboard, dense sensor noise) toward
/// flat grey, so such an image reads as *low* detail and lands coarser. This is
/// conservative and safe (a bigger block on an already-uniform-after-average
/// image loses nothing visible) and is documented as the known limitation of a
/// coarse integer statistic. Test patterns smaller than [`ANALYSIS_EDGE`] are
/// never downscaled, so their true detail is measured exactly.
pub fn content_density(img: &image::RgbaImage) -> u32 {
    // Bound the cost and stabilise the reading across resolutions: area-average
    // the *colour* image to at most `ANALYSIS_EDGE` on its long edge **first**
    // (`downscale` never upscales, so an image already within the edge is
    // measured untouched), then take the luma of that. Measuring gradients in
    // the downscaled space keeps a big real picture's reading close to a small
    // copy of the same picture (features keep their size); it does *not* rescue
    // near-Nyquist texture, which averages toward flat — the documented
    // aliasing limitation.
    let (w, h) = (img.width(), img.height());
    let long = w.max(h);
    let small = if long > ANALYSIS_EDGE {
        let scale = ANALYSIS_EDGE as u64 * 1000 / long as u64; // integer, <1000
        let tw = ((w as u64 * scale) / 1000).max(1) as u32;
        let th = ((h as u64 * scale) / 1000).max(1) as u32;
        pipeline::downscale(img, tw, th)
    } else {
        img.clone()
    };
    let gray = luma_gray(&small);

    let (w, h) = (gray.width() as usize, gray.height() as usize);
    // Sum |Δ| over every horizontal and vertical neighbour pair. One pass,
    // integer only. Degenerate axes contribute zero pairs and are guarded.
    let mut sum: u64 = 0;
    let mut pairs: u64 = 0;
    for y in 0..h {
        for x in 0..w {
            let g = gray.get_pixel(x as u32, y as u32)[0] as i32;
            if x + 1 < w {
                sum += (gray.get_pixel(x as u32 + 1, y as u32)[0] as i32 - g).unsigned_abs() as u64;
                pairs += 1;
            }
            if y + 1 < h {
                sum += (gray.get_pixel(x as u32, y as u32 + 1)[0] as i32 - g).unsigned_abs() as u64;
                pairs += 1;
            }
        }
    }
    // Rounded mean over the neighbour pairs, **scaled ×16** so the low range
    // (where most real photographs live) keeps sub-integer resolution: a smooth
    // photo reads ~20, a textured one ~150, a flat graphic ~20, a solid ~0. The
    // statistic is a fixed-point integer — determinism by construction, no float.
    // A 1×1 (or any single-pixel) image has zero neighbour pairs ⇒ flat ⇒ 0.
    if pairs == 0 {
        return 0;
    }
    ((sum * 16 + pairs / 2) / pairs) as u32
}

/// Integer luma (Rec.601 fixed-point, `>>8`), alpha ignored. Fully deterministic
/// integer arithmetic — the statistic never depends on float rounding.
fn luma_gray(img: &image::RgbaImage) -> image::GrayImage {
    let (w, h) = img.dimensions();
    let mut out = image::GrayImage::new(w, h);
    for (x, y, p) in out.enumerate_pixels_mut() {
        let [r, g, b, _] = img.get_pixel(x, y).0;
        let luma = (19595 * r as u32 + 38470 * g as u32 + 7471 * b as u32) >> 16;
        *p = image::Luma([luma as u8]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    fn checkerboard(w: u32, h: u32) -> RgbaImage {
        checker_cells(w, h, 1)
    }

    /// A checkerboard with `cell`-pixel squares (cell 1 = the worst-case
    /// high-frequency aliasing target; larger cells look like pixel-art blocks).
    fn checker_cells(w: u32, h: u32, cell: u32) -> RgbaImage {
        let cell = cell.max(1);
        let mut img = RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let v = if ((x / cell) + (y / cell)).is_multiple_of(2) {
                255
            } else {
                0
            };
            *p = Rgba([v, v, v, 255]);
        }
        img
    }

    fn gradient(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            // a smooth diagonal ramp — low detail, but not flat
            let v = ((x + y) * 255 / (w + h).max(2)) as u8;
            *p = Rgba([v, v, v, 255]);
        }
        img
    }

    #[test]
    fn statistic_orders_busy_flat_and_solid() {
        let busy = content_density(&checkerboard(64, 64));
        let calm = content_density(&gradient(64, 64));
        let flat = content_density(&solid(64, 64, 128));
        // ordering is what the rule relies on: busy > calm > flat
        assert!(
            busy > calm,
            "checkerboard ({busy}) must beat gradient ({calm})"
        );
        assert!(calm >= flat, "gradient ({calm}) must be ≥ solid ({flat})");
        assert_eq!(flat, 0, "a solid image is perfectly flat");
        // busy fills a good chunk of the scaled range (×16; a checkerboard is
        // the busiest possible image: every neighbour pair differs by 255)
        assert!(
            busy > 2048,
            "checkerboard density {busy} should be very high"
        );
    }

    #[test]
    fn statistic_is_deterministic_and_bounded() {
        let a = content_density(&gradient(200, 120));
        let b = content_density(&gradient(200, 120));
        assert_eq!(a, b, "same bytes ⇒ same statistic");
        for img in [
            checkerboard(300, 300),
            gradient(300, 300),
            solid(300, 300, 10),
        ] {
            // ×16 scale: the ceiling is 255 * 16 (every neighbour differing by 255)
            assert!(content_density(&img) <= 255 * 16);
        }
    }

    #[test]
    fn degenerate_images_are_flat_and_never_panic() {
        // the boundary/degenerate shapes the CLI can actually meet
        assert_eq!(content_density(&solid(1, 1, 42)), 0);
        assert_eq!(
            content_density(&solid(1, 50, 42)),
            0,
            "single column: no Δx pairs"
        );
        assert_eq!(
            content_density(&solid(50, 1, 42)),
            0,
            "single row: no Δy pairs"
        );
        // extreme aspect ratio still produces a bounded, finite number
        let d = content_density(&gradient(2000, 3));
        assert!((0..=255 * 16).contains(&d));
    }

    #[test]
    fn print_measured_densities_for_threshold_calibration() {
        // Diagnostic only (printed under `cargo test -- --nocapture`): the raw
        // statistic the threshold map in `preset` is calibrated against.
        let rows = [
            ("solid64", content_density(&solid(64, 64, 128))),
            ("gradient64", content_density(&gradient(64, 64))),
            ("checker64", content_density(&checkerboard(64, 64))),
            ("gradient128", content_density(&gradient(128, 128))),
            ("checker512", content_density(&checkerboard(512, 512))),
            ("photoish64", {
                // sine source like pipeline::testsrc::photo128x80, at 64
                let mut img = RgbaImage::new(64, 40);
                for (x, y, p) in img.enumerate_pixels_mut() {
                    let r = ((x as f64 * 0.03).sin() * 60.0 + 160.0) as u8;
                    let g = ((y as f64 * 0.07).cos() * 40.0 + 120.0) as u8;
                    let b = (((x + y) as f64 * 0.02).sin() * 50.0 + 120.0) as u8;
                    *p = Rgba([r, g, b, 255]);
                }
                content_density(&img)
            }),
        ];
        for (name, d) in rows {
            println!("density[{name}] = {d}");
        }
    }

    #[test]
    fn large_images_are_downscaled_and_still_discriminate() {
        // Above ANALYSIS_EDGE the statistic takes the downscale path (bounded
        // cost). The property the rule relies on survives it: a big image with
        // pixel-art-scale structure reads far higher than a big flat one, and
        // both stay in the ×16 range.
        let busy = content_density(&checker_cells(600, 600, 8));
        let flat = content_density(&solid(600, 600, 128));
        assert!(
            busy > flat * 4 + 16,
            "busy {busy} must clearly beat flat {flat}"
        );
        assert!(busy <= 255 * 16 && flat <= 255 * 16);
    }

    #[test]
    fn aliasing_is_the_documented_limitation() {
        // A per-pixel checkerboard is the textbook aliasing victim: downscaled
        // toward ANALYSIS_EDGE it averages toward flat grey, so it reads LOW —
        // conservative (a big block on an already-flat-after-average image
        // loses nothing), and documented as the limit of a coarse integer
        // statistic. Below the edge the same pattern is measured untouched and
        // reads maximal, which is why real wallpapers (features ≥ a few px)
        // are read honestly.
        let under_edge = content_density(&checkerboard(256, 256)); // ≤ edge, exact
        let over_edge = content_density(&checkerboard(2048, 2048)); // 4× downscale
        assert!(
            under_edge > 2048,
            "a small fine checker is measured at full detail: {under_edge}"
        );
        assert!(
            over_edge < under_edge,
            "the same pattern, downscaled, aliases toward flat: {over_edge} < {under_edge}"
        );
    }
}
