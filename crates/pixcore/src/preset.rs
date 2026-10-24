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
}

impl Pixel {
    pub const ALL: [Pixel; 6] = [
        Pixel::B8,
        Pixel::B16,
        Pixel::B32,
        Pixel::B64,
        Pixel::B128,
        Pixel::Normal,
    ];

    /// Block size in physical pixels (the one real parameter).
    pub fn block_px(self) -> u32 {
        match self {
            Pixel::B8 => 8,
            Pixel::B16 => 16,
            Pixel::B32 => 32,
            Pixel::B64 => 64,
            Pixel::B128 => 128,
            Pixel::Normal => 1,
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

impl fmt::Display for Pixel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Pixel::B8 => "8",
            Pixel::B16 => "16",
            Pixel::B32 => "32",
            Pixel::B64 => "64",
            Pixel::B128 => "128",
            Pixel::Normal => "normal",
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
            _ => Err(PixError::Invalid(format!(
                "pixel size must be 8|16|32|64|128|normal (got {s:?})"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_spellings() {
        for (s, want) in [
            ("8", Pixel::B8),
            ("16", Pixel::B16),
            ("32", Pixel::B32),
            ("64", Pixel::B64),
            ("128", Pixel::B128),
            ("normal", Pixel::Normal),
            ("16bit", Pixel::B16),
            ("16-bit", Pixel::B16),
            (" 8 ", Pixel::B8),
            ("16BIT", Pixel::B16),
        ] {
            assert_eq!(s.parse::<Pixel>().unwrap(), want, "parse {s}");
        }
        assert!("7".parse::<Pixel>().is_err());
        assert!("bit".parse::<Pixel>().is_err());
    }

    #[test]
    fn display_and_blocks() {
        assert_eq!(Pixel::B16.to_string(), "16");
        assert_eq!(Pixel::Normal.to_string(), "normal");
        let blocks: Vec<u32> = Pixel::ALL.iter().map(|p| p.block_px()).collect();
        assert_eq!(blocks, vec![8, 16, 32, 64, 128, 1]);
    }

    #[test]
    fn serde_round_trip_display_names() {
        let j = serde_json::to_string(&Pixel::B16).unwrap();
        assert_eq!(j, "\"16\"");
        let p: Pixel = serde_json::from_str(&j).unwrap();
        assert_eq!(p, Pixel::B16);
    }
}
