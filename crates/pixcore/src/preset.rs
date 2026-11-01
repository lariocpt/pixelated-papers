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
}

/// The auto-preset rule's block budget: the chosen preset keeps the
/// output's long edge at **at most this many blocks** (see
/// [`Pixel::resolve_for`]). 120 is not arbitrary: 1920/16 = 120, so the
/// rule reproduces the shipped default look exactly at 1080p.
pub const AUTO_MAX_BLOCKS: u32 = 120;

impl Pixel {
    /// Cycle order for the TUI `p` key: presets ascending, then `normal`,
    /// then `auto` (the setting that picks a preset for you).
    pub const ALL: [Pixel; 7] = [
        Pixel::B8,
        Pixel::B16,
        Pixel::B32,
        Pixel::B64,
        Pixel::B128,
        Pixel::Normal,
        Pixel::Auto,
    ];

    /// The concrete presets `auto` may resolve to (block size ascending).
    pub const CONCRETE: [Pixel; 5] = [Pixel::B8, Pixel::B16, Pixel::B32, Pixel::B64, Pixel::B128];

    /// Block size in physical pixels (the one real parameter).
    ///
    /// `Auto` is never rendered as itself — render paths call
    /// [`Pixel::resolve_for`] first. The fallback value here is the
    /// shipped default (16), so a missed resolve degrades to the default
    /// look instead of panicking.
    pub fn block_px(self) -> u32 {
        match self {
            Pixel::B8 => 8,
            Pixel::B16 => 16,
            Pixel::B32 => 32,
            Pixel::B64 => 64,
            Pixel::B128 => 128,
            Pixel::Normal => 1,
            Pixel::Auto => 16,
        }
    }

    /// Resolve `auto` to a concrete preset for one output size; every
    /// other preset (and `normal`) passes through unchanged, so this is
    /// idempotent and safe to call on already-resolved settings.
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
        if self != Pixel::Auto {
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

    /// Identity pixelation: no downscale, no quantize.
    pub fn is_identity(self) -> bool {
        matches!(self, Pixel::Normal)
    }

    /// Preset order by block size ascending (`Normal` first at 1 px).
    pub fn ord_key(self) -> u32 {
        self.block_px()
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
        };
        f.write_str(s)
    }
}

impl FromStr for Pixel {
    type Err = PixError;
    fn from_str(s: &str) -> Result<Self> {
        let t = s.trim().to_ascii_lowercase();
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
                "pixel size must be 8|16|32|64|128|normal|auto (got {s:?})"
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
        let blocks: Vec<u32> = Pixel::ALL.iter().map(|p| p.block_px()).collect();
        // `auto`'s block_px is the defensive fallback (16), never used on
        // a render path — resolve_for runs first.
        assert_eq!(blocks, vec![8, 16, 32, 64, 128, 1, 16]);
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
                if p == Pixel::Auto {
                    assert_ne!(r, Pixel::Auto, "auto must never stay auto");
                    assert_ne!(r, Pixel::Normal, "auto never picks normal");
                    // idempotent: resolving the resolved value is a no-op
                    assert_eq!(r.resolve_for(out), r);
                } else {
                    assert_eq!(r, p, "{p} must pass through");
                }
            }
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
