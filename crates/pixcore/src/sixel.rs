//! Deterministic sixel encoder (ECMA-48: `ESC P q … ESC \\`) — pure, so
//! the TUI can emit sixel without a C dependency (PLAN-30 §C.3).
//!
//! Wire model (verified against the libsixel 1.8.6 encoder/decoder):
//! - introducer `ESC P q`, terminator ST `ESC \\`;
//! - color table `#i;2;R;G;B` with R,G,B as **percent** (0..=100);
//! - sixel chars `'?'`(0x3F, empty) .. `'~'`(0x7E): each paints one
//!   column of 6 vertical pixels for the active color;
//! - `$` (DECGCR) resets the graphics column to 0; `-` (DECGNL) starts
//!   the next 6-row band; `!N` repeats the next char N times (chunked
//!   at 255 for VT240 compatibility).
//!
//! Color table: unique opaque colors in row-major first-occurrence
//! order. More than 256 distinct colors ⇒ the existing deterministic
//! `median_cut(255)` + nearest map runs first (never silent drops).
//! Pixels with alpha ≤ 8 paint as table entry 0 — the TUI pre-maps them
//! to the entry's bg color (the terminal has no alpha; the daemon's XRGB
//! buffer discards it the same way). A final band shorter than 6 rows
//! repeats the image's last row (documented; bottom edge extends ≤ 5 px).

use std::borrow::Cow;

use image::{Rgba, RgbaImage};

use crate::pipeline;

/// Encode `img` as a complete sixel stream (`ESC P q … ESC \\`).
pub fn encode(img: &RgbaImage) -> Vec<u8> {
    let img = quantize_if_needed(img);
    let table = color_table(&img);
    let (w, h) = (img.width(), img.height());

    let mut out = Vec::new();
    out.extend_from_slice(b"\x1bPq");
    for (i, c) in table.iter().enumerate() {
        // percent form, same rounding as libsixel: (v*100 + 127) / 255
        let pct = |v: u8| (v as u32 * 100 + 127) / 255;
        out.extend_from_slice(
            format!("#{i};2;{};{};{}", pct(c.0[0]), pct(c.0[1]), pct(c.0[2])).as_bytes(),
        );
    }

    // column index per color for the current band
    let mut masks = vec![0u8; w as usize];
    let mut rle = Rle::default();
    let mut band_top = 0u32;
    while band_top < h {
        if band_top > 0 {
            out.push(b'-'); // DECGNL: next 6-row band
        }
        for (c, col) in table.iter().enumerate() {
            let mut any = false;
            for (x, m) in masks.iter_mut().enumerate() {
                let mut bits = 0u8;
                for row in 0..6u32 {
                    // short final band repeats the last row
                    let y = (band_top + row).min(h - 1);
                    let p = img.get_pixel(x as u32, y);
                    let hit = if p.0[3] <= 8 {
                        c == 0 // transparent paints as entry 0 (defensive)
                    } else {
                        p.0[0] == col.0[0] && p.0[1] == col.0[1] && p.0[2] == col.0[2]
                    };
                    if hit {
                        bits |= 1 << row;
                    }
                }
                *m = bits;
                any |= bits != 0;
            }
            if !any {
                continue; // color absent from this band
            }
            out.extend_from_slice(format!("#{c}").as_bytes());
            out.push(b'$'); // DECGCR: this color's pass starts at column 0
            rle.reset();
            // emit through the last painted column; the trailing gap is
            // dead weight (nothing follows this color in the band)
            let last = masks.iter().rposition(|m| *m != 0).unwrap();
            for m in masks.iter().take(last + 1) {
                rle.push(&mut out, b'?' + m);
            }
            rle.flush(&mut out);
        }
        band_top += 6;
    }
    out.extend_from_slice(b"\x1b\\");
    out
}

/// RLE coalescer: runs of 1..3 identical chars stay literal (shorter or
/// equal), runs ≥ 4 become `!N char`, chunked at 255 (VT240 gri limit).
#[derive(Default)]
struct Rle {
    ch: u8,
    n: u32,
}

impl Rle {
    fn reset(&mut self) {
        self.ch = 0;
        self.n = 0;
    }

    fn push(&mut self, out: &mut Vec<u8>, ch: u8) {
        if self.n > 0 && ch == self.ch {
            self.n += 1;
            return;
        }
        self.flush(out);
        self.ch = ch;
        self.n = 1;
    }

    fn flush(&mut self, out: &mut Vec<u8>) {
        let mut left = self.n;
        while left > 0 {
            if left <= 3 {
                for _ in 0..left {
                    out.push(self.ch);
                }
                break;
            }
            let chunk = left.min(255);
            out.push(b'!');
            out.extend_from_slice(chunk.to_string().as_bytes());
            out.push(self.ch);
            left -= chunk;
        }
        self.n = 0;
    }
}

/// More than 256 distinct colors ⇒ deterministic median-cut first.
fn quantize_if_needed(img: &RgbaImage) -> Cow<'_, RgbaImage> {
    if distinct_colors(img) <= 256 {
        return Cow::Borrowed(img);
    }
    let pal = pipeline::median_cut(img, 255);
    Cow::Owned(pipeline::apply_palette(img, &pal))
}

fn distinct_colors(img: &RgbaImage) -> usize {
    let mut seen: Vec<[u8; 4]> = Vec::with_capacity(257);
    for p in img.pixels() {
        let c = [p.0[0], p.0[1], p.0[2], p.0[3]];
        if !seen.contains(&c) {
            seen.push(c);
            if seen.len() > 256 {
                return seen.len(); // caller only needs the > 256 signal
            }
        }
    }
    seen.len()
}

/// Unique opaque colors in first-occurrence order; never empty (black
/// is the fallback entry 0 for an all-transparent image).
fn color_table(img: &RgbaImage) -> Vec<Rgba<u8>> {
    let mut out: Vec<Rgba<u8>> = Vec::with_capacity(64);
    for p in img.pixels() {
        if p.0[3] <= 8 {
            continue;
        }
        let c = Rgba([p.0[0], p.0[1], p.0[2], 255]);
        if !out.contains(&c) {
            out.push(c);
        }
    }
    if out.is_empty() {
        out.push(Rgba([0, 0, 0, 255]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn two_color_band_is_byte_exact() {
        // 12×6: left half black, right half red. One band; color 0 paints
        // columns 0..6 fully (mask 63 ⇒ '~'), color 1 resets with '$',
        // gap-fills 6 empties ('?') then paints 6 full columns.
        let img = RgbaImage::from_fn(12, 6, |x, _| {
            if x < 6 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 0, 0, 255])
            }
        });
        let want = "\x1bPq#0;2;0;0;0#1;2;100;0;0#0$!6~#1$!6?!6~\x1b\\";
        assert_eq!(String::from_utf8(encode(&img)).unwrap(), want);
    }

    #[test]
    fn envelope_and_color_cap() {
        // noisy 64×64 image: > 256 distinct colors ⇒ median-cut first;
        // the table never exceeds 256 entries and the stream is framed.
        let img = RgbaImage::from_fn(64, 64, |x, y| {
            Rgba([(x * 7 + y * 13) as u8, (x * y) as u8, (x + y) as u8, 255])
        });
        let s = encode(&img);
        assert!(s.starts_with(b"\x1bPq"));
        assert!(s.ends_with(b"\x1b\\"));
        let body = String::from_utf8(s.clone()).unwrap();
        // color definitions are `#i;2;…`; band passes are `#i$`. The
        // median-cut table caps definitions at 255 (+ the fallback slot).
        let defs = body
            .split('#')
            .skip(1)
            .filter(|t| t.starts_with(|c: char| c.is_ascii_digit()) && t.contains(";2;"))
            .count();
        assert!(defs <= 256, "{defs} color definitions");
        // determinism
        assert_eq!(encode(&img), s);
    }

    #[test]
    fn rle_chunking_and_padding() {
        // 300-wide single color: one run of 300 ⇒ !255 + !45
        let img = RgbaImage::from_pixel(300, 6, Rgba([10, 20, 30, 255]));
        let s = String::from_utf8(encode(&img)).unwrap();
        assert!(s.contains("!255~!45~"), "{s}");
        // 7 rows ⇒ second band repeats the last row (one '-' separator)
        let img7 = RgbaImage::from_pixel(4, 7, Rgba([10, 20, 30, 255]));
        let s7 = String::from_utf8(encode(&img7)).unwrap();
        assert_eq!(s7.matches('-').count(), 1);
    }

    #[test]
    fn transparent_maps_to_entry_zero() {
        // all-transparent image: entry 0 = black fallback, nothing painted
        let img = RgbaImage::from_pixel(6, 6, Rgba([0, 0, 0, 0]));
        let s = String::from_utf8(encode(&img)).unwrap();
        assert!(s.starts_with("\x1bPq#0;2;0;0;0"));
        assert!(s.ends_with("\x1b\\"));
    }
}
