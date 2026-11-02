use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{PixError, Result};

/// The vibe knob: pixel block size in **physical** pixels.
///
/// The era names are pure vibe (8 ≈ NES, 16 ≈ Sega, 64 ≈ N64 …) — nothing
/// is console-accurate. `16` means a 64×64 screen area shows 16 blocks
/// (a 4×4 grid of 16 px blocks): the classic "game" look. `Normal` is 1 px
/// (no pixelation).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Pixel {
    /// 8 px blocks — chunkiest, "8-bit".
    #[serde(rename = "8")]
    B8,
    /// 16 px blocks — the default look.
    #[serde(rename = "16")]
    B16,
    /// 32 px blocks.
    #[serde(rename = "32")]
    B32,
    /// 64 px blocks.
    #[serde(rename = "64")]
    B64,
    /// 128 px blocks — enormous tiles, pure abstraction.
    #[serde(rename = "128")]
    B128,
    /// 1 px — no pixelation (render the original, still nearest-scaled).
    #[serde(rename = "normal")]
    Normal,
    /// Pick the preset from the **output size** at render time
    /// ([`Pixel::resolve_for`]) — bigger output ⇒ coarser blocks, so the
    /// blocky look stays consistent across screen sizes. Never rendered
    /// as itself: every render path resolves it to a concrete preset
    /// before the variant cache key is built.
    #[serde(rename = "auto")]
    Auto,
    /// Pick the preset from the output size **and** the picture's own
    /// content: the size rule is a **floor** and a persisted content
    /// measurement ([`crate::content`]) may only make blocks **coarser**
    /// (a calm image loses nothing at big blocks; a busy one stays on the
    /// floor exactly like `auto`). Like `auto`, it is **never rendered as
    /// itself** — [`Pixel::resolve_content_for`] turns it into a concrete
    /// preset (falling back to the size-only floor when no usable
    /// measurement is stored), so `auto-content` never reaches a cache name.
    #[serde(rename = "auto-content")]
    AutoContent,
}

/// The auto-preset rule's block budget: the chosen preset keeps the
/// output's long edge at **at most this many blocks** (see
/// [`Pixel::resolve_for`]). 120 is not arbitrary: 1920/16 = 120, so the
/// rule reproduces the shipped default look exactly at 1080p.
pub const AUTO_MAX_BLOCKS: u32 = 120;

impl Pixel {
    /// Cycle order for the TUI `p` key: presets ascending, then `normal`,
    /// then `auto` (picks a preset for you from the output size), then
    /// `auto-content` (also reads the picture's own detail).
    pub const ALL: [Pixel; 8] = [
        Pixel::B8,
        Pixel::B16,
        Pixel::B32,
        Pixel::B64,
        Pixel::B128,
        Pixel::Normal,
        Pixel::Auto,
        Pixel::AutoContent,
    ];

    /// The concrete presets `auto` may resolve to (block size ascending).
    pub const CONCRETE: [Pixel; 5] = [Pixel::B8, Pixel::B16, Pixel::B32, Pixel::B64, Pixel::B128];

    /// Block size in physical pixels (the one real parameter).
    ///
    /// `Auto`/`AutoContent` are never rendered as themselves — render paths
    /// call [`Pixel::resolve_for`] / [`Pixel::resolve_content_for`] first. The
    /// fallback value here is the shipped default (16), so a missed resolve
    /// degrades to the default look instead of panicking.
    pub fn block_px(self) -> u32 {
        match self {
            Pixel::B8 => 8,
            Pixel::B16 => 16,
            Pixel::B32 => 32,
            Pixel::B64 => 64,
            Pixel::B128 => 128,
            Pixel::Normal => 1,
            Pixel::Auto => 16,
            Pixel::AutoContent => 16,
        }
    }

    /// Resolve `auto` (and the size-only **floor** of `auto-content`) to a
    /// concrete preset for one output size; every other preset (and `normal`)
    /// passes through unchanged, so this is idempotent and safe to call on
    /// already-resolved settings. **It never returns `Auto` or `AutoContent`**
    /// — that is the cache-name concreteness guarantee (a cache filename
    /// always names a concrete preset).
    ///
    /// The rule (the plan's "consistent blocky look" intent, step-6 §6.4:
    /// bigger output ⇒ coarser pixel): the **smallest** preset from
    /// 8/16/32/64/128 whose grid keeps the output's long edge at ≤
    /// [`AUTO_MAX_BLOCKS`] blocks. Equivalent ladder on `L = max(W, H)`:
    /// `L ≤ 960 → 8`, `≤ 1920 → 16`, `≤ 3840 → 32`, `≤ 7680 → 64`,
    /// else `128`. `auto` never picks `normal` — asking for auto means
    /// asking to be pixelated. Pure function of the output size only
    /// (not the source image), so the same wallpaper on two screens of
    /// different sizes can pick different presets.
    pub fn resolve_for(self, out: (u32, u32)) -> Pixel {
        if self != Pixel::Auto && self != Pixel::AutoContent {
            return self;
        }
        let long_edge = out.0.max(out.1).max(1);
        for p in Pixel::CONCRETE {
            let block = p.block_px();
            if long_edge.div_ceil(block) <= AUTO_MAX_BLOCKS {
                return p;
            }
        }
        Pixel::B128
    }

    /// Resolve `auto-content` for one output size **and** the picture's
    /// persisted content `density` (see [`crate::content`]); every other
    /// preset (including plain `auto`) passes through [`Pixel::resolve_for`].
    /// Like `resolve_for`, it **never returns `Auto` or `AutoContent`**.
    ///
    /// The combine rule: the size-only `auto` result is a **floor**. Content
    /// may only push blocks **coarser** (bigger), never finer than the size
    /// rule. So `final = max(content_preset(density), size_floor)` on block
    /// size. A **missing or stale probe** (`None`) resolves to the floor —
    /// byte-identical to plain `auto` — so a picture with no measurement
    /// behaves exactly as before the feature existed. Busy pictures land on
    /// the floor too (content wants finer than the floor ⇒ clamped up), which
    /// is why `auto-content` reproduces plain `auto` on photographs and only
    /// diverges on calm pictures (gradients, large solid regions), which get
    /// coarser blocks with no visible loss.
    pub fn resolve_content_for(self, out: (u32, u32), density: Option<u32>) -> Pixel {
        if self != Pixel::AutoContent {
            return self.resolve_for(out);
        }
        let floor = Pixel::Auto.resolve_for(out);
        match density {
            Some(d) => {
                let want = content_preset(d);
                if want.block_px() > floor.block_px() {
                    want
                } else {
                    floor
                }
            }
            None => floor,
        }
    }

    /// Identity pixelation: no downscale, no quantize.
    pub fn is_identity(self) -> bool {
        matches!(self, Pixel::Normal)
    }

    /// Preset order by block size ascending (`Normal` first at 1 px).
    pub fn ord_key(self) -> u32 {
        self.block_px()
    }
}

/// Map a persisted content `density` ([`crate::content`]: mean absolute
/// neighbour gradient, scaled ×16, measured at `ANALYSIS_EDGE = 512`) to a
/// concrete preset for `auto-content`.
///
/// **More detail ⇒ finer blocks** (fine blocks preserve texture); **less
/// detail ⇒ coarser blocks** (a calm image — gradients, large solid regions —
/// loses nothing at big blocks). The thresholds are calibrated on the
/// production statistic over real fixtures (busy photographs / pixel-art read
/// ~80–190 → B8/B16 and land on the size floor; a UI strip reads ~24 → B32;
/// calm photographs and flat vector graphics read ~10 → B64; smooth gradients
/// read <8 → B128). [`Pixel::resolve_content_for`] clamps this against the
/// size floor, so a busy image that maps finer than the floor stays exactly on
/// the floor like plain `auto`.
pub fn content_preset(density: u32) -> Pixel {
    if density >= 64 {
        Pixel::B8 // dense texture / pixel-art detail
    } else if density >= 32 {
        Pixel::B16
    } else if density >= 16 {
        Pixel::B32
    } else if density >= 8 {
        Pixel::B64 // calm photographs, flat graphics
    } else {
        Pixel::B128 // smooth gradients, large solids
    }
}

impl fmt::Display for Pixel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Pixel::B8 => "8",
            Pixel::B16 => "16",
            Pixel::B32 => "32",
            Pixel::B64 => "64",
            Pixel::B128 => "128",
            Pixel::Normal => "normal",
            Pixel::Auto => "auto",
            Pixel::AutoContent => "auto-content",
        };
        f.write_str(s)
    }
}

impl FromStr for Pixel {
    type Err = PixError;
    fn from_str(s: &str) -> Result<Self> {
        let t = s.trim().to_ascii_lowercase();
        // The content-auto family has its own spellings (a multi-word token,
        // so match it before the "8bit"/"8b" era stripping below).
        if matches!(t.as_str(), "auto-content" | "auto content" | "content") {
            return Ok(Pixel::AutoContent);
        }
        // Accept the era flavor: "8", "8bit", "8-bit", "8b", "normal".
        let core = t
            .strip_suffix("bit")
            .or_else(|| t.strip_suffix("b"))
            .unwrap_or(&t);
        let core = core.strip_suffix('-').unwrap_or(core);
        match core.trim() {
            "8" => Ok(Pixel::B8),
            "16" => Ok(Pixel::B16),
            "32" => Ok(Pixel::B32),
            "64" => Ok(Pixel::B64),
            "128" => Ok(Pixel::B128),
            "normal" => Ok(Pixel::Normal),
            "auto" | "automatic" | "a" => Ok(Pixel::Auto),
            _ => Err(PixError::Invalid(format!(
                "pixel size must be 8|16|32|64|128|normal|auto|auto-content (got {s:?})"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_all_spellings() {
        for (s, want) in [
            ("8", Pixel::B8),
            ("16", Pixel::B16),
            ("32", Pixel::B32),
            ("64", Pixel::B64),
            ("128", Pixel::B128),
            ("normal", Pixel::Normal),
            ("auto", Pixel::Auto),
            ("AUTO", Pixel::Auto),
            ("automatic", Pixel::Auto),
            ("a", Pixel::Auto),
            ("16bit", Pixel::B16),
            ("16-bit", Pixel::B16),
            (" 8 ", Pixel::B8),
            ("16BIT", Pixel::B16),
            ("auto-content", Pixel::AutoContent),
            ("auto content", Pixel::AutoContent),
            ("AUTO-CONTENT", Pixel::AutoContent),
            ("content", Pixel::AutoContent),
        ] {
            assert_eq!(s.parse::<Pixel>().unwrap(), want, "parse {s}");
        }
        assert!("7".parse::<Pixel>().is_err());
        assert!("bit".parse::<Pixel>().is_err());
        assert!("automagic".parse::<Pixel>().is_err());
    }

    #[test]
    fn display_and_blocks() {
        assert_eq!(Pixel::B16.to_string(), "16");
        assert_eq!(Pixel::Normal.to_string(), "normal");
        assert_eq!(Pixel::Auto.to_string(), "auto");
        assert_eq!(Pixel::AutoContent.to_string(), "auto-content");
        let blocks: Vec<u32> = Pixel::ALL.iter().map(|p| p.block_px()).collect();
        // `auto`/`auto-content`'s block_px is the defensive fallback (16),
        // never used on a render path — resolve_for/resolve_content_for run
        // first.
        assert_eq!(blocks, vec![8, 16, 32, 64, 128, 1, 16, 16]);
    }

    #[test]
    fn serde_round_trip_display_names() {
        let j = serde_json::to_string(&Pixel::B16).unwrap();
        assert_eq!(j, "\"16\"");
        let p: Pixel = serde_json::from_str(&j).unwrap();
        assert_eq!(p, Pixel::B16);
        // `auto` is the new token; old tokens keep their exact spelling.
        let j = serde_json::to_string(&Pixel::Auto).unwrap();
        assert_eq!(j, "\"auto\"");
        assert_eq!(
            serde_json::from_str::<Pixel>("\"auto\"").unwrap(),
            Pixel::Auto
        );
        // `auto-content` is a distinct token (manifest + config spelling).
        let j = serde_json::to_string(&Pixel::AutoContent).unwrap();
        assert_eq!(j, "\"auto-content\"");
        assert_eq!(
            serde_json::from_str::<Pixel>("\"auto-content\"").unwrap(),
            Pixel::AutoContent
        );
    }

    #[test]
    fn auto_resolves_by_output_ladder() {
        // The documented ladder on L = max(W, H):
        // ≤960→8, ≤1920→16, ≤3840→32, ≤7680→64, else 128.
        for (out, want) in [
            ((116, 28), Pixel::B8), // TUI preview canvas
            ((800, 600), Pixel::B8),
            ((960, 540), Pixel::B8),  // boundary
            ((961, 540), Pixel::B16), // just past it
            ((1920, 1080), Pixel::B16),
            ((1280, 720), Pixel::B16),
            ((1920, 1200), Pixel::B16), // boundary
            ((1921, 1080), Pixel::B32),
            ((2560, 1440), Pixel::B32),
            ((3840, 2160), Pixel::B32), // 4K → 32: 120 blocks across
            ((3841, 2160), Pixel::B64),
            ((7680, 4320), Pixel::B64), // 8K boundary
            ((7681, 4320), Pixel::B128),
            ((16384, 1000), Pixel::B128),
        ] {
            assert_eq!(Pixel::Auto.resolve_for(out), want, "auto at {out:?}");
        }
        // portrait outputs key off the long edge too
        assert_eq!(Pixel::Auto.resolve_for((1080, 1920)), Pixel::B16);
    }

    #[test]
    fn concrete_presets_pass_through_resolve() {
        for p in Pixel::ALL {
            for out in [(640, 360), (1920, 1080), (7680, 4320)] {
                let r = p.resolve_for(out);
                if p == Pixel::Auto || p == Pixel::AutoContent {
                    // Both auto variants resolve to a concrete preset here;
                    // `auto-content`'s size floor must never leak either
                    // `Auto` nor `AutoContent` (the cache-name guarantee).
                    assert_ne!(r, Pixel::Auto, "auto must never stay auto");
                    assert_ne!(
                        r,
                        Pixel::AutoContent,
                        "auto-content must never stay auto-content"
                    );
                    assert_ne!(r, Pixel::Normal, "auto never picks normal");
                    // idempotent: resolving the resolved value is a no-op
                    assert_eq!(r.resolve_for(out), r);
                } else {
                    assert_eq!(r, p, "{p} must pass through");
                }
            }
        }
    }

    #[test]
    fn auto_content_never_leaks_and_matches_auto_without_a_probe() {
        // With no usable probe (None) every output resolves `auto-content`
        // to exactly the size-only floor ⇒ byte-identical to plain `auto`.
        for out in [(640, 360), (1920, 1080), (3840, 2160), (100, 100)] {
            let via_content = Pixel::AutoContent.resolve_content_for(out, None);
            assert_eq!(
                via_content,
                Pixel::Auto.resolve_for(out),
                "no-probe auto-content must equal plain auto at {out:?}"
            );
            assert_ne!(via_content, Pixel::AutoContent);
            assert_ne!(via_content, Pixel::Auto);
            // Concrete presets ignore density entirely.
            assert_eq!(Pixel::B64.resolve_content_for(out, Some(189)), Pixel::B64);
            assert_eq!(
                Pixel::Auto.resolve_content_for(out, Some(189)),
                Pixel::Auto.resolve_for(out)
            );
        }
    }

    #[test]
    fn auto_content_floor_is_coarser_only() {
        // 1080p size floor is B16 (block 16).
        let out = (1920, 1080);
        assert_eq!(Pixel::Auto.resolve_for(out), Pixel::B16);
        // A busy picture (high density) maps finer than the floor ⇒ clamped
        // back to the floor (16), reproducing plain `auto`.
        for busy in [32, 64, 83, 123, 4080] {
            assert_eq!(
                Pixel::AutoContent.resolve_content_for(out, Some(busy)),
                Pixel::B16,
                "busy density {busy} stays on the 1080p floor"
            );
        }
        // A calm picture maps coarser and that coarse value wins (content may
        // only push coarser, never finer).
        assert_eq!(
            Pixel::AutoContent.resolve_content_for(out, Some(10)),
            Pixel::B64
        );
        assert_eq!(
            Pixel::AutoContent.resolve_content_for(out, Some(24)),
            Pixel::B32
        );
        assert_eq!(
            Pixel::AutoContent.resolve_content_for(out, Some(4)),
            Pixel::B128
        );
        // content_preset ordering: more density never yields a bigger block.
        let mut prev = content_preset(0).block_px();
        for d in [8, 16, 32, 64, 120, 4080] {
            let b = content_preset(d).block_px();
            assert!(b <= prev, "non-monotone content rule at density {d}");
            prev = b;
        }
        // The result is concrete for every density: auto-content never leaks.
        for d in 0..300 {
            let r = Pixel::AutoContent.resolve_content_for(out, Some(d));
            assert!(!matches!(
                r,
                Pixel::Auto | Pixel::AutoContent | Pixel::Normal
            ));
        }
    }

    proptest! {
        /// The auto rule is a pure, monotone function of the long edge:
        /// bigger output never gets a *finer* preset, the result is always
        /// one of the five concrete presets, and resolving twice is a no-op.
        #[test]
        fn auto_rule_is_monotone_concrete_and_idempotent(
            w in 1u32..=16384,
            h in 1u32..=16384,
            w2 in 1u32..=16384,
            h2 in 1u32..=16384,
        ) {
            let a = Pixel::Auto.resolve_for((w, h));
            let b = Pixel::Auto.resolve_for((w2, h2));
            prop_assert!(Pixel::CONCRETE.contains(&a));
            prop_assert!(Pixel::CONCRETE.contains(&b));
            prop_assert_eq!(a.resolve_for((w, h)), a);
            let long_a = w.max(h);
            let long_b = w2.max(h2);
            if long_a <= long_b {
                prop_assert!(a.block_px() <= b.block_px(), "non-monotone: {long_a}→{a} vs {long_b}→{b}");
            }
        }
    }
}
