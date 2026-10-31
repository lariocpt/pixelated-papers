//! Test-only bench helper (PLAN-30 S3): the measured numbers behind the
//! palette/dither section of docs/PERF.md. Driven by test/bench-dither.sh
//! and test/bench-sizes.sh — run those (they build `--release`; debug
//! timings are meaningless).
//!
//!   quantize-cost   stage timings for `pipeline::quantize` (dither cost)
//!   sizes           variant byte sizes + blake3 hashes (palette cost)
//!
//! The `photo()` fixture is byte-for-byte the generator from
//! crates/pixcore/examples/variant_sizes.rs, so these numbers line up with
//! the existing PERF.md size table (same deterministic source, same
//! CoverCrop/gutter-24 settings). `sizes` also emits machine-readable
//! `#row|…` lines that test/bench-sizes.sh joins against the v0.1 baseline.

use image::{ImageBuffer, Rgba, RgbaImage};
use pixcore::pipeline::{self, quantize};
use pixcore::variant;
use pixcore::{Dither, PaletteMode, Placement, ResolvedSettings, RgbCss};
use std::hint::black_box;
use std::time::Instant;

/// Mirrors examples/variant_sizes.rs `photo()` exactly.
fn photo(w: u32, h: u32) -> RgbaImage {
    let mut img = ImageBuffer::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let xf = x as f64;
        let yf = y as f64;
        let r = ((xf * 0.013).sin() * 70.0 + ((yf * 0.004).cos() * 40.0) + 130.0) as u8;
        let g = ((yf * 0.017).cos() * 60.0 + ((xf * 0.009).sin() * 30.0) + 120.0) as u8;
        let b = (((xf + yf) * 0.011).sin() * 65.0 + 125.0) as u8;
        *px = Rgba([r, g, b, 255]);
    }
    img
}

fn kb(n: usize) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

// ---------------------------------------------------------------------------
// quantize-cost
// ---------------------------------------------------------------------------

/// min + median ms over `iters` timed runs (2 warmup runs first).
fn time_quantize(canvas: &RgbaImage, mode: PaletteMode, d: Dither, iters: usize) -> (f64, f64) {
    for _ in 0..2 {
        black_box(quantize(black_box(canvas), mode, d));
    }
    let mut ts: Vec<f64> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t = Instant::now();
        let out = quantize(canvas, mode, d);
        black_box(out);
        ts.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (ts[0], ts[ts.len() / 2])
}

fn quantize_cost() {
    println!("# quantize stage cost — evidence for docs/PERF.md\n");
    println!("| canvas | shape | palette | dither | min ms | median ms | Δ vs none |");
    println!("|---|---|---|---|---|---|---|");

    // The preset canvas shapes (⌈out/block⌉), built the way the pipeline
    // builds them: downscale the full source to the canvas size.
    let groups = [
        (
            "120×68",
            "1080p @ 16 (the default)",
            pipeline::downscale(&photo(1920, 1080), 120, 68),
            300,
        ),
        (
            "240×135",
            "1080p @ 8 · 4K @ 16",
            pipeline::downscale(&photo(3840, 2160), 240, 135),
            150,
        ),
        (
            "480×270",
            "4K @ 8",
            pipeline::downscale(&photo(3840, 2160), 480, 270),
            50,
        ),
    ];
    for (canvas, shape, img, iters) in groups {
        let mut none_med = 0.0;
        for d in [Dither::None, Dither::FloydSteinberg, Dither::Bayer8] {
            let (min, med) = time_quantize(&img, PaletteMode::Auto, d, iters);
            if d == Dither::None {
                none_med = med;
            }
            println!(
                "| {canvas} | {shape} | auto | {d} | {min:.2} | {med:.2} | {} |",
                if d == Dither::None {
                    "—".to_string()
                } else {
                    format!("{:+.2} ms", med - none_med)
                }
            );
        }
    }

    // The one-time full-res case: a `normal`-sized canvas (1920×1080) run
    // through quantize directly. NOTE: today's `normal` path bypasses
    // quantize entirely (see PERF.md) — these rows are the stage cost if a
    // full-res canvas were quantized, which is what the plan's risk row
    // budgets against.
    let full = photo(1920, 1080);
    let mut none_med = 0.0;
    for (mode, d, iters) in [
        (PaletteMode::Vga, Dither::None, 10),
        (PaletteMode::Vga, Dither::FloydSteinberg, 10),
        (PaletteMode::Auto, Dither::None, 4),
        (PaletteMode::Auto, Dither::FloydSteinberg, 4),
    ] {
        let (min, med) = time_quantize(&full, mode, d, iters);
        if d == Dither::None {
            none_med = med;
        }
        println!(
            "| 1920×1080 | full-res (one-time) | {mode} | {d} | {min:.2} | {med:.2} | {} |",
            if d == Dither::None {
                "—".to_string()
            } else {
                format!("{:+.2} ms", med - none_med)
            }
        );
    }
}

// ---------------------------------------------------------------------------
// sizes
// ---------------------------------------------------------------------------

fn settings(preset: &str, palette: PaletteMode) -> ResolvedSettings {
    ResolvedSettings {
        pixel: preset.parse().unwrap(),
        placement: Placement::CoverCrop,
        bg_color: RgbCss::BLACK,
        gutter_px: 24,
        palette,
        dither: Dither::None,
    }
}

fn sizes() {
    println!("# variant sizes (v2 default path + named palettes) — evidence for docs/PERF.md\n");
    println!("| fixture | preset | canvas | palette | variant | bytes | blake3/12 | Δ vs auto |");
    println!("|---|---|---|---|---|---|---|---|");
    for (fx, w, h) in [("photo 1080p", 1920u32, 1080u32), ("photo 4K", 3840, 2160)] {
        let orig = photo(w, h);
        for preset in ["16", "8"] {
            let mut auto_bytes = 0usize;
            for mode in [
                PaletteMode::Auto,
                PaletteMode::Nes,
                PaletteMode::Gb,
                PaletteMode::Cga,
                PaletteMode::Ega,
                PaletteMode::Vga,
                PaletteMode::Gen16,
            ] {
                let canvas = variant::generate_from_image(&orig, &settings(preset, mode), (w, h));
                let bytes = variant::encode_variant(&canvas, mode);
                let hash = blake3::hash(&bytes).to_hex()[..12].to_string();
                if mode == PaletteMode::Auto {
                    auto_bytes = bytes.len();
                }
                let delta = if mode == PaletteMode::Auto {
                    "—".to_string()
                } else {
                    let pct = 100.0 * (bytes.len() as f64 / auto_bytes as f64 - 1.0);
                    format!("{pct:+.0}%")
                };
                println!("#row|{fx}|{preset}|{mode}|{}|{hash}", bytes.len());
                println!(
                    "| {fx} | {preset} | {}×{} | {mode} | {} | {} | {hash} | {delta} |",
                    canvas.width(),
                    canvas.height(),
                    kb(bytes.len()),
                    bytes.len(),
                );
            }
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("quantize-cost") => quantize_cost(),
        Some("sizes") => sizes(),
        _ => {
            eprintln!("usage: bench-quantize quantize-cost | sizes");
            std::process::exit(2);
        }
    }
}
