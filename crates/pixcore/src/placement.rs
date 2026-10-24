use serde::{Deserialize, Serialize};

/// Integer rectangle (x, y, w, h), all ≥ 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// How the image is placed on the output. The four options requested:
/// fit-with-borders, cover-with-gutter, cover-cropped (centered), and
/// stretch (no fixed aspect ratio).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    /// Whole image visible; leftover output is filled with the wallpaper's
    /// `bg_color`. The default.
    Fit,
    /// Cover an inset content box (uniform `gutter_px` ring, the *mat*
    /// interpretation of "cover with gutter"); the ring gets `bg_color`.
    CoverGutter,
    /// Cover the full output; the overflow is cropped, centered.
    CoverCrop,
    /// Fill the full output exactly, x and y scaled independently —
    /// aspect ratio is NOT preserved.
    Stretch,
}

impl std::fmt::Display for Placement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Placement::Fit => "fit",
            Placement::CoverGutter => "cover-gutter",
            Placement::CoverCrop => "cover-crop",
            Placement::Stretch => "stretch",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for Placement {
    type Err = crate::PixError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fit" => Ok(Placement::Fit),
            "cover-gutter" | "gutter" => Ok(Placement::CoverGutter),
            "cover-crop" | "cover" | "cover-cropped" => Ok(Placement::CoverCrop),
            "stretch" | "fill" => Ok(Placement::Stretch),
            other => Err(crate::PixError::Invalid(format!(
                "placement must be fit|cover-gutter|cover-crop|stretch (got {other:?})"
            ))),
        }
    }
}

/// Result of placement math: which part of the *image* (`window`) goes into
/// which part of the *output* (`dest`). Everything outside `dest` is filled
/// with `bg_color` by the renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct View {
    /// Sub-rect of the source image (whole source for Fit/Stretch).
    pub window: Rect,
    /// Rect of the output buffer that the image occupies (the content box).
    pub dest: Rect,
    /// Full output size.
    pub output: (u32, u32),
}

/// Placement math (PLAN §2.2). `src` = source image size, `out` = output
/// buffer (physical px) size, `gutter_px` only matters for CoverGutter.
pub fn view(src: (u32, u32), out: (u32, u32), p: Placement, gutter_px: u32) -> View {
    let (sw, sh) = src;
    let (ow, oh) = out;
    let whole_src = Rect::new(0, 0, sw, sh);

    let (window, dest) = match p {
        Placement::Stretch => (whole_src, Rect::new(0, 0, ow, oh)),
        Placement::Fit => {
            // uniform scale s = min(ow/sw, oh/sh); integer dest size so the
            // image never exceeds the output.
            let (dw, dh) = fit_size(sw, sh, ow, oh);
            let dx = (ow - dw) / 2;
            let dy = (oh - dh) / 2;
            (whole_src, Rect::new(dx, dy, dw, dh))
        }
        Placement::CoverCrop => (
            centered_cover_window(sw, sh, ow, oh),
            Rect::new(0, 0, ow, oh),
        ),
        Placement::CoverGutter => {
            // Content box centered, clamped to ≥1 px so absurd gutters
            // degrade to a stripe instead of vanishing.
            let gw = ow.saturating_sub(2 * gutter_px).max(1);
            let gh = oh.saturating_sub(2 * gutter_px).max(1);
            let dx = (ow - gw) / 2;
            let dy = (oh - gh) / 2;
            let win = centered_cover_window(sw, sh, gw, gh);
            (win, Rect::new(dx, dy, gw, gh))
        }
    };

    View {
        window,
        dest,
        output: out,
    }
}

/// Largest integer size fitting `sw×sh` into `box_w×box_h` preserving aspect.
fn fit_size(sw: u32, sh: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    if sw == 0 || sh == 0 {
        return (box_w.min(1), box_h.min(1));
    }
    // compare sw/box_w vs sh/box_h with integers: src is "wider" if sw*box_h > sh*box_w
    if (sw as u64) * (box_h as u64) >= (sh as u64) * (box_w as u64) {
        // width-limited
        let dw = box_w;
        let dh = (((sh as u64 * box_w as u64) / sw as u64).min(box_h as u64) as u32).max(1);
        (dw, dh)
    } else {
        let dh = box_h;
        let dw = ((sw as u64 * box_h as u64) / sh as u64).min(box_w as u64) as u32;
        (dw.max(1), dh)
    }
}

/// Centered sub-rect of a `sw×sh` source with aspect ratio `bw:bh` (cover:
/// the window always covers, cropping the excess axis).
fn centered_cover_window(sw: u32, sh: u32, bw: u32, bh: u32) -> Rect {
    if sw == 0 || sh == 0 || bw == 0 || bh == 0 {
        return Rect::new(0, 0, sw.max(1), sh.max(1));
    }
    // src aspect wider than target?  sw/sh > bw/bh  ⇔  sw*bh > sh*bw
    if (sw as u64) * (bh as u64) > (sh as u64) * (bw as u64) {
        // crop width
        let ww = ((sh as u64 * bw as u64) / bh as u64).min(sw as u64).max(1) as u32;
        let wx = (sw - ww) / 2;
        Rect::new(wx, 0, ww, sh)
    } else {
        // crop height (also exact-aspect → whole source)
        let wh = ((sw as u64 * bh as u64) / bw as u64).min(sh as u64).max(1) as u32;
        let wy = (sh - wh) / 2;
        Rect::new(0, wy, sw, wh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(src: (u32, u32), out: (u32, u32), p: Placement, g: u32) -> View {
        view(src, out, p, g)
    }

    #[test]
    fn stretch_is_whole_no_aspect() {
        let vi = v((1000, 500), (1920, 1080), Placement::Stretch, 0);
        assert_eq!(vi.window, Rect::new(0, 0, 1000, 500));
        assert_eq!(vi.dest, Rect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn fit_pillarboxes_and_never_crops() {
        // wide src into 16:9 output ⇒ exact fill
        let vi = v((3840, 2160), (1920, 1080), Placement::Fit, 0);
        assert_eq!(vi.dest, Rect::new(0, 0, 1920, 1080));
        // portrait src ⇒ pillarbox (borders left/right)
        let vi = v((1000, 2000), (1920, 1080), Placement::Fit, 0);
        assert_eq!(vi.dest.h, 1080);
        assert_eq!(vi.dest.w, ((1000.0f64 * 1080.0 / 2000.0).floor()) as u32);
        assert_eq!(vi.dest.x, (1920 - vi.dest.w) / 2);
        assert_eq!(vi.window, Rect::new(0, 0, 1000, 2000));
    }

    #[test]
    fn cover_crop_is_centered() {
        let vi = v((1000, 2000), (1920, 1080), Placement::CoverCrop, 0);
        assert_eq!(vi.dest, Rect::new(0, 0, 1920, 1080));
        // portrait source cropped vertically, centered (integer floor)
        let expected_h = ((1000.0f64 * 1080.0 / 1920.0).floor() as u32).clamp(1, 2000);
        assert_eq!(vi.window.h, expected_h);
        assert_eq!(vi.window.y, (2000 - expected_h) / 2);
        assert_eq!(vi.window.w, 1000);
    }

    #[test]
    fn gutter_insets_content_box() {
        let vi = v((1000, 1000), (1920, 1080), Placement::CoverGutter, 24);
        assert_eq!(vi.dest, Rect::new(24, 24, 1872, 1032));
        assert_eq!(vi.window.w, 1000); // square src → width stays
        assert_eq!(
            vi.window.h,
            ((1000.0f64 * 1032.0 / 1872.0).floor() as u32).clamp(1, 1000)
        );
    }

    #[test]
    fn absurd_gutter_clamps_to_stripe() {
        let vi = v((1000, 1000), (100, 100), Placement::CoverGutter, 10_000);
        assert_eq!(vi.dest.w, 1);
        assert_eq!(vi.dest.h, 1);
    }

    #[test]
    fn zero_gutter_cover_gutter_equals_cover_crop() {
        let a = v((1234, 4321), (1920, 1200), Placement::CoverGutter, 0);
        let b = v((1234, 4321), (1920, 1200), Placement::CoverCrop, 0);
        assert_eq!(a.window, b.window);
        assert_eq!(a.dest, b.dest);
    }

    #[test]
    fn proptest_containment_and_coverage() {
        use proptest::prelude::*;
        let dims = 1..6000u32;
        proptest!(|(sw in &dims, sh in &dims, ow in &dims, oh in &dims, g in 0..=500u32)| {
            for p in [Placement::Fit, Placement::CoverGutter, Placement::CoverCrop, Placement::Stretch] {
                let vi = view((sw, sh), (ow, oh), p, g);
                // dest always inside output
                prop_assert!(vi.dest.x + vi.dest.w <= ow);
                prop_assert!(vi.dest.y + vi.dest.h <= oh);
                prop_assert!(vi.dest.w >= 1 && vi.dest.h >= 1);
                // window inside source
                prop_assert!(vi.window.x + vi.window.w <= sw);
                prop_assert!(vi.window.y + vi.window.h <= sh);
                // fit never crops
                if p == Placement::Fit {
                    prop_assert_eq!(vi.window, Rect::new(0, 0, sw, sh));
                }
                // covers fill their content box exactly
                if p == Placement::CoverCrop {
                    prop_assert_eq!(vi.dest, Rect::new(0, 0, ow, oh));
                }
            }
        });
    }

    #[test]
    fn placement_round_trips_serde_kebab() {
        let s = serde_json::to_string(&Placement::CoverGutter).unwrap();
        assert_eq!(s, "\"cover-gutter\"");
        assert_eq!(
            serde_json::from_str::<Placement>(&s).unwrap(),
            Placement::CoverGutter
        );
    }
}
