//! The R8 "absolute minimum size" table: for each preset and source class,
//! how many bytes the cached variant is vs the stored original. Run:
//!
//!     cargo run --release -p pixcore --example variant_sizes
//!
//! Numbers here are the evidence behind docs/PERF.md. Deterministic sources;
//! no wall-clock timing (this is a byte-size fact table, not a benchmark).

use image::{ImageBuffer, Rgba, RgbaImage};
use pixcore::{Dither, PaletteMode, Pixel, Placement, ResolvedSettings, RgbCss};

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

/// Flat-art: large solid regions + a few hard edges (poster/UI art) — PNG
/// compresses the original well, the hardest case for our ratio claim.
fn flat(w: u32, h: u32) -> RgbaImage {
    let mut img = ImageBuffer::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let band = y / (h / 6 + 1);
        let base = match band % 6 {
            0 => [20, 30, 60],
            1 => [200, 60, 40],
            2 => [40, 160, 120],
            3 => [240, 200, 40],
            4 => [120, 80, 200],
            _ => [250, 250, 250],
        };
        let edge = if x % (w / 8) < 2 { 12 } else { base[0] };
        *px = Rgba([edge, base[1], base[2], 255]);
    }
    img
}

fn png_bytes(img: &RgbaImage) -> usize {
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    let _ = image::ImageEncoder::write_image(
        enc,
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    );
    out.len()
}

fn parse(s: &str) -> Pixel {
    s.parse().unwrap()
}

fn kb(n: usize) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

fn row(orig: &RgbaImage, orig_png: usize, out: (u32, u32), preset: &str, label: &str) {
    let s = ResolvedSettings {
        pixel: parse(preset),
        placement: Placement::CoverCrop,
        bg_color: RgbCss::BLACK,
        gutter_px: 24,
        palette: PaletteMode::Auto,
        dither: Dither::None,
    };
    let canvas = pixcore::variant::generate_from_image(orig, &s, out);
    let bytes = pixcore::variant::encode_variant(&canvas, PaletteMode::Auto).len();
    let ratio = orig_png as f64 / bytes as f64;
    println!(
        "| {label} | {preset} | {}×{} | {} | {} | {:.0}× |",
        canvas.width(),
        canvas.height(),
        kb(bytes),
        kb(orig_png),
        ratio
    );
}

fn main() {
    println!("# variant sizes (palette PNG) — evidence for docs/PERF.md\n");
    for (name, w, h) in [
        ("photo", 1920u32, 1080u32),
        ("photo", 3840u32, 2160u32),
        ("flat-art", 1920u32, 1080u32),
        ("flat-art", 3840u32, 2160u32),
    ] {
        let orig = if name == "photo" {
            photo(w, h)
        } else {
            flat(w, h)
        };
        let orig_png = png_bytes(&orig);
        let out = (w, h);
        println!("**{name} {w}×{h}** (original PNG: {})\n", kb(orig_png));
        println!("| source | preset | canvas | variant | original | ratio |");
        println!("|--------|--------|--------|---------|----------|-------|");
        for p in ["8", "16", "32", "64", "128", "normal"] {
            row(&orig, orig_png, out, p, name);
        }
        println!();
    }
}
