//! Composite: variant image → output-sized buffer. Paint `bg_color` first,
//! then nearest-neighbor stretch the variant into its dest rect.
//! `composite` (DynamicImage, used by `export`) and `composite_into` (u32
//! shm words, used by the daemon) share the core ⇒ they cannot diverge.

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};

use crate::color::RgbCss;
use crate::config::Config;
use crate::manifest::Settings;
use crate::palette::{Dither, PaletteMode};
use crate::placement::{Rect, View};
use crate::preset::Pixel;

/// The single resolved set of knobs for rendering one wallpaper right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedSettings {
    pub pixel: Pixel,
    pub placement: crate::placement::Placement,
    pub bg_color: RgbCss,
    pub gutter_px: u32,
    pub palette: PaletteMode,
    pub dither: Dither,
    /// How the `cover-gutter` ring is filled (mat vs blurred bleed).
    pub gutter_style: crate::gutter::GutterStyle,
}

impl ResolvedSettings {
    /// entry settings → config defaults (R5 → R2). The `enabled` switch (R6)
    /// is applied by the *caller* via [`ResolvedSettings::unpixelated`], so
    /// the policy is visible at the call site.
    pub fn resolve(entry: &Settings, cfg: &Config) -> Self {
        Self {
            pixel: entry.pixel.unwrap_or(cfg.default_pixel),
            placement: entry.placement.unwrap_or(cfg.default_placement),
            bg_color: entry.bg_color.unwrap_or(cfg.default_bg_color),
            gutter_px: entry.gutter_px.unwrap_or(cfg.default_gutter_px),
            palette: entry.palette.unwrap_or(cfg.palette),
            dither: entry.dither.unwrap_or(cfg.dither),
            gutter_style: entry.gutter_style.unwrap_or(cfg.default_gutter_style),
        }
    }

    /// R6 (disabled): pixelation off, placement/bg unchanged.
    pub fn unpixelated(self) -> Self {
        Self {
            pixel: Pixel::Normal,
            ..self
        }
    }
}

/// Stretch `variant` into `out` (output-sized RGBA) with bg fill.
pub fn composite(variant: &RgbaImage, view: &View, bg: RgbCss) -> RgbaImage {
    composite_with_bleed(variant, view, bg, None)
}

/// [`composite`] with an optional blurred **bleed layer**: when `Some`, the
/// whole output is first painted with `bleed` (nearest-stretched to fill),
/// then the sharp `variant` is stretched into `view.dest` on top — so the
/// gutter ring shows the bleed instead of a flat `bg_color` mat. `None` is
/// byte-identical to [`composite`] (flat bg fill).
pub fn composite_with_bleed(
    variant: &RgbaImage,
    view: &View,
    bg: RgbCss,
    bleed: Option<&RgbaImage>,
) -> RgbaImage {
    let (ow, oh) = view.output;
    let mut img = ImageBuffer::from_pixel(ow, oh, Rgba([bg.0[0], bg.0[1], bg.0[2], 255]));
    if let Some(b) = bleed {
        // Paint the bleed across the full output, then the sharp variant over
        // its dest — the bleed is just another stretched layer under the dest.
        stretch_into_rgba(b, Rect::new(0, 0, ow, oh), &mut img);
    }
    stretch_into_rgba(variant, view.dest, &mut img);
    img
}

/// Same as [`composite`] but writes u32 XRGB words straight into an shm
/// buffer slice (row-major, stride = `out_w * 4`).
pub fn composite_into(variant: &RgbaImage, view: &View, bg: RgbCss, buf: &mut [u32]) {
    composite_into_with_bleed(variant, view, bg, None, buf)
}

/// [`composite_into`] with an optional bleed layer, exactly mirroring
/// [`composite_with_bleed`] so the daemon buffer and the exported PNG cannot
/// diverge (the file-level invariant of this module).
pub fn composite_into_with_bleed(
    variant: &RgbaImage,
    view: &View,
    bg: RgbCss,
    bleed: Option<&RgbaImage>,
    buf: &mut [u32],
) {
    let (ow, oh) = view.output;
    assert!(buf.len() >= (ow as usize) * (oh as usize));
    let word = bg.to_buffer_word();
    buf[..ow as usize * oh as usize].fill(word);
    if let Some(b) = bleed {
        stretch_into_u32(b, Rect::new(0, 0, ow, oh), buf, ow);
    }
    stretch_into_u32(variant, view.dest, buf, ow);
}

pub(crate) fn stretch_into_u32(src: &RgbaImage, dest: Rect, buf: &mut [u32], out_w: u32) {
    let (sw, sh) = (src.width() as u64, src.height() as u64);
    let (dw, dh) = (dest.w as u64, dest.h as u64);
    if dw == 0 || dh == 0 {
        return;
    }
    for dy in 0..dh {
        let sy = (dy * sh / dh) as u32;
        let brow = (dest.y as u64 + dy) as usize * out_w as usize;
        for dx in 0..dw {
            let sx = (dx * sw / dw) as u32;
            let p = src.get_pixel(sx, sy).0;
            buf[brow + dest.x as usize + dx as usize] =
                0xFF00_0000 | ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32);
        }
    }
}

fn stretch_into_rgba(src: &RgbaImage, dest: Rect, out: &mut RgbaImage) {
    let (sw, sh) = (src.width() as u64, src.height() as u64);
    let (dw, dh) = (dest.w as u64, dest.h as u64);
    if dw == 0 || dh == 0 {
        return;
    }
    for dy in 0..dh {
        let sy = (dy * sh / dh) as u32;
        for dx in 0..dw {
            let sx = (dx * sw / dw) as u32;
            out.put_pixel(
                dest.x + dx as u32,
                dest.y + dy as u32,
                *src.get_pixel(sx, sy),
            );
        }
    }
}

/// Wrap as DynamicImage for the `export` path.
pub fn to_dynamic(img: RgbaImage) -> DynamicImage {
    DynamicImage::ImageRgba8(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::testsrc;
    use crate::placement::view;

    #[test]
    fn composite_and_composite_into_agree() {
        let var = testsrc::gradient64();
        let v = view((64, 64), (320, 180), crate::placement::Placement::Fit, 0);
        let bg: RgbCss = "#102030".parse().unwrap();
        let rgba = composite(&var, &v, bg);
        let mut words = vec![0u32; (v.output.0 * v.output.1) as usize];
        composite_into(&var, &v, bg, &mut words);
        for (x, y, p) in rgba.enumerate_pixels() {
            let w = words[y as usize * v.output.0 as usize + x as usize];
            // RGB must match; composite keeps source alpha for RGBA, buffer is XRGB
            assert_eq!(
                w & 0x00FF_FFFF,
                ((p.0[0] as u32) << 16 | (p.0[1] as u32) << 8 | p.0[2] as u32),
                "divergence at {x},{y}"
            );
        }
    }

    #[test]
    fn fit_borders_are_bg_color() {
        let var = testsrc::gradient64();
        let v = view(
            (1000, 2000),
            (1920, 1080),
            crate::placement::Placement::Fit,
            0,
        );
        let bg: RgbCss = "#ff00aa".parse().unwrap();
        let mut words = vec![0u32; (v.output.0 * v.output.1) as usize];
        composite_into(&var, &v, bg, &mut words);
        // corner is outside dest (portrait pillarbox) ⇒ bg
        assert_eq!(words[0], 0xFFFF_00AA);
        // dest center is the image
        let cx = (v.dest.x + v.dest.w / 2) as usize;
        let cy = (v.dest.y + v.dest.h / 2) as usize;
        assert_ne!(words[cy * v.output.0 as usize + cx], 0xFFFF_00AA);
    }

    #[test]
    fn bleed_composite_agrees_daemon_and_png() {
        // The daemon buffer (u32) and the exported PNG (RGBA) must match
        // everywhere on the bleed path — including the gutter ring where the
        // blurred bleed shows through — the module's file-level invariant.
        let var = testsrc::gradient64();
        let bleed = testsrc::photo128x80();
        let v = view(
            (64, 64),
            (320, 180),
            crate::placement::Placement::CoverGutter,
            20,
        );
        let bg: RgbCss = "#102030".parse().unwrap();
        let rgba = composite_with_bleed(&var, &v, bg, Some(&bleed));
        let mut words = vec![0u32; (v.output.0 * v.output.1) as usize];
        composite_into_with_bleed(&var, &v, bg, Some(&bleed), &mut words);
        for (x, y, p) in rgba.enumerate_pixels() {
            let w = words[y as usize * v.output.0 as usize + x as usize];
            assert_eq!(
                w & 0x00FF_FFFF,
                ((p.0[0] as u32) << 16 | (p.0[1] as u32) << 8 | p.0[2] as u32),
                "bleed divergence at {x},{y}"
            );
        }
    }

    #[test]
    fn bleed_fills_the_ring_solid_fills_bg() {
        // A flat bleed of a color distinct from variant+bg: every ring pixel
        // can only be the bleed color (bleed path) or the flat bg (solid path).
        let var = testsrc::gradient64();
        let bleed = ImageBuffer::from_pixel(4, 4, Rgba([7, 200, 9, 255]));
        let v = view(
            (64, 64),
            (320, 180),
            crate::placement::Placement::CoverGutter,
            20,
        );
        let bg: RgbCss = "#000000".parse().unwrap();
        let bled = composite_with_bleed(&var, &v, bg, Some(&bleed));
        let solid = composite_with_bleed(&var, &v, bg, None);
        let d = v.dest;
        let in_dest = |x: u32, y: u32| x >= d.x && x < d.x + d.w && y >= d.y && y < d.y + d.h;
        let mut ring = 0;
        for (x, y, p) in bled.enumerate_pixels() {
            if in_dest(x, y) {
                continue; // the dest is the sharp variant in both composites
            }
            ring += 1;
            assert_eq!(
                [p.0[0], p.0[1], p.0[2]],
                [7, 200, 9],
                "bleed ring at {x},{y}"
            );
            let sp = solid.get_pixel(x, y).0;
            assert_eq!(
                [sp[0], sp[1], sp[2]],
                [0, 0, 0],
                "solid ring at {x},{y} not bg"
            );
        }
        assert!(ring > 0, "cover-gutter must leave a ring to fill");
    }

    #[test]
    fn resolve_policy() {
        let cfg = Config::default();
        let s = Settings::default();
        let r = ResolvedSettings::resolve(&s, &cfg);
        assert_eq!(r.pixel, Pixel::B16); // R2 default
        let s2 = Settings {
            pixel: "8".parse().ok(),
            ..Default::default()
        };
        assert_eq!(ResolvedSettings::resolve(&s2, &cfg).pixel, Pixel::B8); // R5 entry wins
        assert_eq!(r.unpixelated().pixel, Pixel::Normal); // R6
        // palette/dither: entry overrides config default, None inherits (R5→R2).
        assert_eq!(r.palette, PaletteMode::Auto);
        assert_eq!(r.dither, Dither::None);
        let s3 = Settings {
            palette: Some(PaletteMode::Gb),
            dither: Some(Dither::Bayer4),
            ..Default::default()
        };
        let r3 = ResolvedSettings::resolve(&s3, &cfg);
        assert_eq!(r3.palette, PaletteMode::Gb);
        assert_eq!(r3.dither, Dither::Bayer4);
        let cfg2 = Config {
            palette: PaletteMode::Vga,
            dither: Dither::Atkinson,
            ..Default::default()
        };
        let r4 = ResolvedSettings::resolve(&s, &cfg2);
        assert_eq!(r4.palette, PaletteMode::Vga);
        assert_eq!(r4.dither, Dither::Atkinson);
        // R6 unpixelated must not touch the palette knobs.
        assert_eq!(r3.unpixelated().palette, PaletteMode::Gb);
        assert_eq!(r3.unpixelated().dither, Dither::Bayer4);
        // gutter_style resolve: Solid by default; the entry overrides the
        // config default; an unset entry inherits a bleed default.
        assert_eq!(r.gutter_style, crate::GutterStyle::Solid);
        let s5 = Settings {
            gutter_style: Some(crate::GutterStyle::Bleed),
            ..Default::default()
        };
        assert_eq!(
            ResolvedSettings::resolve(&s5, &cfg).gutter_style,
            crate::GutterStyle::Bleed
        );
        let cfg3 = Config {
            default_gutter_style: crate::GutterStyle::Bleed,
            ..Default::default()
        };
        assert_eq!(
            ResolvedSettings::resolve(&s, &cfg3).gutter_style,
            crate::GutterStyle::Bleed
        );
    }
}
