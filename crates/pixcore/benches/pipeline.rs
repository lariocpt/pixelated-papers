//! CPU-side pipeline benchmarks — the whole interesting surface (R8). Run:
//!
//!     cargo bench -p pixcore
//!
//! Sizes mirror real use: a photo-ish source downscaled to the variant
//! canvases the daemon caches, the median-cut quantize, the nearest stretch
//! into an output buffer, and the cold-switch core (orig → canvas → buffer).
//! Synthetic sources are deterministic; generation is setup, never timed.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use image::{ImageBuffer, Rgba, RgbaImage};
use std::hint::black_box;
use pixcore::{
    pipeline, view, Placement, Pixel, ResolvedSettings, RgbCss,
};

/// Deterministic photo-ish source: sine gradients, no hard edges — the case
/// where median-cut and box-downscale do real work.
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

fn settings(pixel: Pixel, placement: Placement, palette: bool) -> ResolvedSettings {
    ResolvedSettings {
        pixel,
        placement,
        bg_color: RgbCss::BLACK,
        gutter_px: 24,
        palette,
    }
}

fn parse(s: &str) -> Pixel {
    s.parse().unwrap()
}

fn downscale_box(c: &mut Criterion) {
    let mut g = c.benchmark_group("downscale_box");
    let src = photo(1280, 800);
    let out = (1920u32, 1080u32);
    for p in ["8", "16", "32", "64", "128"] {
        let pixel = parse(p);
        let v = view((src.width(), src.height()), out, Placement::CoverCrop, 0);
        let block = pixel.block_px();
        g.bench_with_input(BenchmarkId::from_parameter(p), &src, |b, src| {
            b.iter(|| {
                let canvas = pipeline::downscale_to_variant(black_box(src), &v, block);
                black_box(canvas.width());
            });
        });
    }
    g.finish();
}

fn median_cut_bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("median_cut_to_256");
    // A 1080p-ish canvas is what median-cut actually runs on.
    let src = photo(640, 360);
    let canvas = pipeline::downscale_to_variant(&src, &view((640, 360), (1920, 1080), Placement::CoverCrop, 0), 16);
    let small = photo(1280, 800);
    let big_canvas = pipeline::downscale_to_variant(&small, &view((1280, 800), (3840, 2160), Placement::CoverCrop, 0), 8);
    g.bench_function("canvas_1080p", |b| {
        b.iter(|| {
            let pal = pipeline::median_cut(black_box(&canvas), 256);
            black_box(pal.len());
        });
    });
    g.bench_function("canvas_4k@8", |b| {
        b.iter(|| {
            let pal = pipeline::median_cut(black_box(&big_canvas), 256);
            black_box(pal.len());
        });
    });
    g.finish();
}

fn composite_stretch(c: &mut Criterion) {
    let mut g = c.benchmark_group("composite_stretch");
    // Small canvases stretching to big outputs = the nearest-neighbour hot path.
    for (label, (ow, oh)) in [("1920x1080", (1920u32, 1080u32)), ("3840x2160", (3840u32, 2160u32))] {
        // 16px blocks of a 1280x800 source
        let block = 16u32;
        let canvas = ImageBuffer::from_fn(ow / block, oh / block, |x, y| {
            let t = ((x + y) % 32) as u8 * 8;
            Rgba([t, 255 - t, (x as u8) * 3, 255])
        });
        let v = view((canvas.width(), canvas.height()), (ow, oh), Placement::Fit, 0);
        let bg = RgbCss::BLACK;
        let mut buf = vec![0u32; (ow * oh) as usize];
        g.bench_function(label, |b| {
            b.iter(|| {
                pixcore::render::composite_into(black_box(&canvas), &v, bg, black_box(&mut buf));
            });
        });
    }
    g.finish();
}

fn cold_switch(c: &mut Criterion) {
    let mut g = c.benchmark_group("cold_switch");
    // The full cold path: decode already done upstream, so orig → canvas →
    // palette → stretch into buffer. Representative photo → 1080p @ 16.
    let src = photo(1280, 800);
    let out = (1920u32, 1080u32);
    let s = settings(parse("16"), Placement::CoverCrop, true);
    let mut buf = vec![0u32; (out.0 * out.1) as usize];
    g.bench_function("photo1280_to_1080p@16_pal", |b| {
        b.iter(|| {
            let canvas = pixcore::variant::generate_from_image(black_box(&src), &s, out);
            let v = view((src.width(), src.height()), out, s.placement, s.gutter_px);
            pixcore::render::composite_into(&canvas, &v, s.bg_color, black_box(&mut buf));
        });
    });
    let s4 = settings(parse("8"), Placement::Stretch, true);
    let out4 = (3840u32, 2160u32);
    let mut buf4 = vec![0u32; (out4.0 * out4.1) as usize];
    g.bench_function("photo1280_to_4k@8_pal", |b| {
        b.iter(|| {
            let canvas = pixcore::variant::generate_from_image(black_box(&src), &s4, out4);
            let v = view((src.width(), src.height()), out4, s4.placement, s4.gutter_px);
            pixcore::render::composite_into(&canvas, &v, s4.bg_color, black_box(&mut buf4));
        });
    });
    g.finish();
}

criterion_group!(benches, downscale_box, median_cut_bench, composite_stretch, cold_switch);
criterion_main!(benches);
