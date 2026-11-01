//! How the `cover-gutter` ring is filled. The shipped default (`solid`) is a
//! flat `bg_color` mat. `bleed` instead fills the ring with the wallpaper's
//! own edges, blurred and quantized to the wallpaper's palette, so the margin
//! reads as a soft extension of the image instead of a plain mat — while the
//! sharp pixelated variant still sits in the content box. The pixel look is
//! preserved: the bleed is built by the same area-average downscale kernels
//! the pipeline uses, then quantized to the wallpaper palette.

use serde::{Deserialize, Serialize};

/// Fill style for the `cover-gutter` ring (the area outside the content box).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GutterStyle {
    /// Uniform `bg_color` mat (the shipped default; unchanged behavior).
    #[default]
    Solid,
    /// Blur the wallpaper's own edges and bleed them into the ring, quantized
    /// to the wallpaper palette (`off` ⇒ plain RGBA bleed).
    Bleed,
}

impl GutterStyle {
    /// Every style, in cycle order (TUI `G` cycles this list).
    pub const ALL: [GutterStyle; 2] = [GutterStyle::Solid, GutterStyle::Bleed];

    /// Compact cache-key/label token. `solid` is the default and is OMITTED
    /// from the cache key (mirrors `Dither::None` omitting its segment), so
    /// every default-named variant keeps its exact pre-bleed filename.
    /// `bleed` returns `Some("bleed")`.
    pub fn token(self) -> Option<&'static str> {
        match self {
            GutterStyle::Solid => None,
            GutterStyle::Bleed => Some("bleed"),
        }
    }
}

impl std::fmt::Display for GutterStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            GutterStyle::Solid => "solid",
            GutterStyle::Bleed => "bleed",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for GutterStyle {
    type Err = crate::PixError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "solid" | "mat" | "flat" => Ok(GutterStyle::Solid),
            "bleed" | "blurred" | "blur" => Ok(GutterStyle::Bleed),
            other => Err(crate::PixError::Invalid(format!(
                "gutter style must be solid|bleed (got {other:?})"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_solid() {
        assert_eq!(GutterStyle::default(), GutterStyle::Solid);
    }

    #[test]
    fn token_omits_solid() {
        assert_eq!(GutterStyle::Solid.token(), None);
        assert_eq!(GutterStyle::Bleed.token(), Some("bleed"));
    }

    #[test]
    fn serde_kebab_round_trip() {
        assert_eq!(
            serde_json::to_string(&GutterStyle::Bleed).unwrap(),
            "\"bleed\""
        );
        assert_eq!(
            serde_json::to_string(&GutterStyle::Solid).unwrap(),
            "\"solid\""
        );
        assert_eq!(
            serde_json::from_str::<GutterStyle>("\"bleed\"").unwrap(),
            GutterStyle::Bleed
        );
    }

    #[test]
    fn parses_aliases_and_rejects() {
        assert_eq!("solid".parse::<GutterStyle>().unwrap(), GutterStyle::Solid);
        assert_eq!("bleed".parse::<GutterStyle>().unwrap(), GutterStyle::Bleed);
        assert_eq!("BLEED".parse::<GutterStyle>().unwrap(), GutterStyle::Bleed);
        assert_eq!(" mat ".parse::<GutterStyle>().unwrap(), GutterStyle::Solid);
        assert!("glow".parse::<GutterStyle>().is_err());
    }

    #[test]
    fn display_matches_serde() {
        for g in GutterStyle::ALL {
            let s = g.to_string();
            assert_eq!(s.parse::<GutterStyle>().unwrap(), g, "{g}");
        }
    }
}
