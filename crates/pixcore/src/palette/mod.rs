//! The palette/dither model (PLAN-30 §A.1): what a `PaletteMode` means,
//! which fixed table backs each name, and how the knobs serialise.
//!
//! Names stay *vibe*, not console-accurate (README promise): quantization
//! onto canonical tables + deterministic dithering, no PPU emulation.

use std::fmt;
use std::str::FromStr;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::error::{PixError, Result};
use crate::pipeline;

pub mod tables;

pub use tables::snap_rgb555;

/// Palette quantization mode for the variant canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaletteMode {
    /// No quantization — variants stay RGBA (today's `palette = false`).
    #[serde(rename = "off")]
    Off,
    /// Median-cut ≤ 256 + nearest map (today's `palette = true` default).
    #[serde(rename = "auto")]
    Auto,
    /// NES 2C02 master table ([`tables::NES`]).
    #[serde(rename = "nes")]
    Nes,
    /// Game Boy DMG four greens ([`tables::GB`]).
    #[serde(rename = "gb")]
    Gb,
    /// CGA 16 ([`tables::CGA`]).
    #[serde(rename = "cga")]
    Cga,
    /// EGA 64 — classic 4×4×4 ([`tables::EGA`]).
    #[serde(rename = "ega")]
    Ega,
    /// Standard VGA 256 ([`tables::VGA`]).
    #[serde(rename = "vga")]
    Vga,
    /// Per-image: median-cut to 16, snapped onto the 15-bit RGB555 grid
    /// (Genesis/Mega Drive flavour; an approximation, documented in tables).
    #[serde(rename = "gen16")]
    Gen16,
}

/// Dithering applied after palette mapping, in canvas space. All variants
/// are deterministic at fixed input (row-major scan, integer math only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dither {
    /// Nearest mapping only (the v0.1 look).
    #[default]
    #[serde(rename = "none")]
    None,
    /// Floyd–Steinberg error diffusion (7/16, 3/16, 5/16, 1/16).
    #[serde(rename = "fs")]
    FloydSteinberg,
    /// Atkinson (MacPaint) diffusion: six neighbours at 1/8, 1/4 discarded.
    #[serde(rename = "atkinson")]
    Atkinson,
    /// Ordered 2×2 Bayer threshold.
    #[serde(rename = "bayer2")]
    Bayer2,
    /// Ordered 4×4 Bayer threshold.
    #[serde(rename = "bayer4")]
    Bayer4,
    /// Ordered 8×8 Bayer threshold.
    #[serde(rename = "bayer8")]
    Bayer8,
}

impl PaletteMode {
    pub const ALL: [PaletteMode; 8] = [
        PaletteMode::Off,
        PaletteMode::Auto,
        PaletteMode::Nes,
        PaletteMode::Gb,
        PaletteMode::Cga,
        PaletteMode::Ega,
        PaletteMode::Vga,
        PaletteMode::Gen16,
    ];

    /// Serde/cache-key token (`-auto-`, `-gen16`, …).
    pub fn token(self) -> &'static str {
        match self {
            PaletteMode::Off => "off",
            PaletteMode::Auto => "auto",
            PaletteMode::Nes => "nes",
            PaletteMode::Gb => "gb",
            PaletteMode::Cga => "cga",
            PaletteMode::Ega => "ega",
            PaletteMode::Vga => "vga",
            PaletteMode::Gen16 => "gen16",
        }
    }

    /// The fixed table for named modes; `None` for `Off`/`Auto`/`Gen16`
    /// (those derive their palette per image or skip it).
    pub fn fixed_table(self) -> Option<&'static [Rgba<u8>]> {
        match self {
            PaletteMode::Nes => Some(&tables::NES),
            PaletteMode::Gb => Some(&tables::GB),
            PaletteMode::Cga => Some(&tables::CGA),
            PaletteMode::Ega => Some(&tables::EGA),
            PaletteMode::Vga => Some(&tables::VGA),
            _ => None,
        }
    }

    /// The exact set of opaque colors a [`pipeline::quantize`] output may
    /// contain for `img` (the acceptance contract; the fully-transparent
    /// entry is additionally allowed for pixels that were transparent in
    /// the input). `None` for `Off` — no constraint. Deterministic for a
    /// given `img`.
    pub fn accepts(self, img: &RgbaImage) -> Option<Vec<Rgba<u8>>> {
        match self {
            PaletteMode::Off => None,
            PaletteMode::Auto => Some(pipeline::median_cut(img, 256)),
            PaletteMode::Gen16 => Some(
                pipeline::median_cut(img, 16)
                    .into_iter()
                    .map(snap_rgb555)
                    .collect(),
            ),
            named => Some(
                named
                    .fixed_table()
                    .expect("named mode has a table")
                    .to_vec(),
            ),
        }
    }
}

/// Should the variant PNG be written as a true palette PNG? True for every
/// quantizing mode (named tables encode with exactly their entries); `Off`
/// keeps the RGBA path. (`Auto`/`Gen16` derive a per-image table.)
pub fn encode_as_palette_png(mode: PaletteMode) -> bool {
    !matches!(mode, PaletteMode::Off)
}

impl Dither {
    pub const ALL: [Dither; 6] = [
        Dither::None,
        Dither::FloydSteinberg,
        Dither::Atkinson,
        Dither::Bayer2,
        Dither::Bayer4,
        Dither::Bayer8,
    ];

    /// Serde/cache-key token; `none` is omitted from cache filenames.
    pub fn token(self) -> &'static str {
        match self {
            Dither::None => "none",
            Dither::FloydSteinberg => "fs",
            Dither::Atkinson => "atkinson",
            Dither::Bayer2 => "bayer2",
            Dither::Bayer4 => "bayer4",
            Dither::Bayer8 => "bayer8",
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy Config.palette bool (<- v0.1 `palette = true/false` in config.toml)
// ---------------------------------------------------------------------------

/// Deserialize a [`PaletteMode`] from its string **or** the legacy TOML
/// bool (`true` → `Auto`, `false` → `Off`), so pre-v0.2 config files keep
/// loading. Serialize as the plain string (new saves are string-only).
pub(crate) mod palette_mode_compat {
    use super::PaletteMode;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(mode: &PaletteMode, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(mode.token())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PaletteMode, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Legacy {
            Bool(bool),
            Name(String),
        }
        match Deserialize::deserialize(d)? {
            Legacy::Bool(on) => Ok(if on {
                PaletteMode::Auto
            } else {
                PaletteMode::Off
            }),
            Legacy::Name(name) => name.parse().map_err(serde::de::Error::custom),
        }
    }
}

impl fmt::Display for PaletteMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

impl fmt::Display for Dither {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

impl FromStr for PaletteMode {
    type Err = PixError;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(PaletteMode::Off),
            "auto" => Ok(PaletteMode::Auto),
            "nes" => Ok(PaletteMode::Nes),
            "gb" => Ok(PaletteMode::Gb),
            "cga" => Ok(PaletteMode::Cga),
            "ega" => Ok(PaletteMode::Ega),
            "vga" => Ok(PaletteMode::Vga),
            "gen16" => Ok(PaletteMode::Gen16),
            _ => Err(PixError::Invalid(format!(
                "palette mode must be off|auto|nes|gb|cga|ega|vga|gen16 (got {s:?})"
            ))),
        }
    }
}

impl FromStr for Dither {
    type Err = PixError;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Dither::None),
            "fs" => Ok(Dither::FloydSteinberg),
            "atkinson" => Ok(Dither::Atkinson),
            "bayer2" => Ok(Dither::Bayer2),
            "bayer4" => Ok(Dither::Bayer4),
            "bayer8" => Ok(Dither::Bayer8),
            _ => Err(PixError::Invalid(format!(
                "dither must be none|fs|atkinson|bayer2|bayer4|bayer8 (got {s:?})"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_tokens_are_lowercase_and_stable() {
        assert_eq!(Dither::default(), Dither::None); // v0.1 look stays default
        for m in PaletteMode::ALL {
            let j = serde_json::to_string(&m).unwrap();
            assert_eq!(j, format!("\"{}\"", m.token()));
            let back: PaletteMode = serde_json::from_str(&j).unwrap();
            assert_eq!(back, m);
        }
        for d in Dither::ALL {
            let j = serde_json::to_string(&d).unwrap();
            assert_eq!(j, format!("\"{}\"", d.token()));
            let back: Dither = serde_json::from_str(&j).unwrap();
            assert_eq!(back, d);
        }
    }

    #[test]
    fn display_and_parse_round_trip() {
        for m in PaletteMode::ALL {
            assert_eq!(m.to_string().parse::<PaletteMode>().unwrap(), m);
        }
        for d in Dither::ALL {
            assert_eq!(d.to_string().parse::<Dither>().unwrap(), d);
        }
        assert!("Nes".parse::<PaletteMode>().unwrap() == PaletteMode::Nes);
        assert!(" FS ".parse::<Dither>().unwrap() == Dither::FloydSteinberg);
        assert!("pvm".parse::<PaletteMode>().is_err());
        assert!("sierra".parse::<Dither>().is_err());
    }

    #[test]
    fn fixed_tables_shape() {
        assert!(PaletteMode::Off.fixed_table().is_none());
        assert!(PaletteMode::Auto.fixed_table().is_none());
        assert!(PaletteMode::Gen16.fixed_table().is_none());
        assert_eq!(PaletteMode::Nes.fixed_table().unwrap().len(), 64);
        assert_eq!(PaletteMode::Gb.fixed_table().unwrap().len(), 4);
        assert_eq!(PaletteMode::Cga.fixed_table().unwrap().len(), 16);
        assert_eq!(PaletteMode::Ega.fixed_table().unwrap().len(), 64);
        assert_eq!(PaletteMode::Vga.fixed_table().unwrap().len(), 256);
    }

    #[test]
    fn legacy_bool_and_string_forms_deserialize() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct S {
            #[serde(deserialize_with = "palette_mode_compat::deserialize")]
            palette: PaletteMode,
        }
        let on: S = serde_json::from_str(r#"{"palette":true}"#).unwrap();
        assert_eq!(on.palette, PaletteMode::Auto);
        let off: S = serde_json::from_str(r#"{"palette":false}"#).unwrap();
        assert_eq!(off.palette, PaletteMode::Off);
        let named: S = serde_json::from_str(r#"{"palette":"vga"}"#).unwrap();
        assert_eq!(named.palette, PaletteMode::Vga);
        assert!(serde_json::from_str::<S>(r#"{"palette":"bogus"}"#).is_err());
        // Serialization is always the string form (new saves never write bool).
        assert_eq!(
            serde_json::to_string(&PaletteMode::Auto).unwrap(),
            "\"auto\""
        );
    }

    #[test]
    fn encode_predicate_follows_mode() {
        assert!(!encode_as_palette_png(PaletteMode::Off));
        for m in PaletteMode::ALL {
            assert_eq!(encode_as_palette_png(m), m != PaletteMode::Off, "{m}");
        }
    }
}
