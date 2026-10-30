use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::color::RgbCss;
use crate::error::{PixError, Result};
use crate::palette::palette_mode_compat;
use crate::palette::{Dither, PaletteMode};
use crate::placement::Placement;
use crate::preset::Pixel;

/// Everything configurable lives here. Written atomically; missing file
/// means defaults (the file is only created on first explicit change).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Master pixelation switch (R6). `false` ⇒ render originals, placement unchanged.
    pub enabled: bool,
    /// Default look for wallpapers without an override (R2).
    pub default_pixel: Pixel,
    pub default_placement: Placement,
    pub default_bg_color: RgbCss,
    /// Only used by `cover-gutter`.
    pub default_gutter_px: u32,
    /// Reserved visibility floor (0 = off; presets are exact by design).
    pub min_visible_px: u32,
    /// Default palette quantization look (R2). `auto` is the v0.1 look
    /// (median-cut ≤256); named modes map onto fixed console tables.
    /// Legacy `palette = true/false` TOML bools still load (true→auto,
    /// false→off); new saves write the string.
    #[serde(with = "palette_mode_compat")]
    pub palette: PaletteMode,
    /// Default dither applied when mapping onto the palette (`none` = the
    /// v0.1 look; deterministic for fixed input).
    pub dither: Dither,
    pub optimize: Optimize,
    pub daemon: DaemonConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Optimize {
    /// Longest edge for stored originals (clamped 1080..=7680).
    pub max_side: u32,
    pub jpeg_quality: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    /// Log level: error | warn | info | debug | trace.
    pub log: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            default_pixel: Pixel::B16,
            default_placement: Placement::Fit,
            default_bg_color: RgbCss::BLACK,
            default_gutter_px: 24,
            min_visible_px: 0,
            palette: PaletteMode::Auto,
            dither: Dither::None,
            optimize: Optimize::default(),
            daemon: DaemonConfig::default(),
        }
    }
}

impl Default for Optimize {
    fn default() -> Self {
        Self {
            max_side: 3840,
            jpeg_quality: 87,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self { log: "info".into() }
    }
}

impl Config {
    /// Load from `path`. Missing file ⇒ defaults (no error, no write).
    /// Malformed ⇒ error naming the path (never silently overwrite).
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| PixError::Config {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(PixError::io(path, e)),
        }
    }

    /// Atomic save: serialize → `<path>.tmp` → rename.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| PixError::io(parent, e))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| PixError::other(e.to_string()))?;
        write_atomic(path, text.as_bytes()).map_err(|e| PixError::io(path, e))?;
        Ok(())
    }
}

/// Write `data` to `path` atomically (unique tmp in the same dir + rename).
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let mut tmp = tempfile::NamedTempFile::new_in(dir.unwrap_or(Path::new(".")))?;
    tmp.write_all(data)?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn missing_file_is_defaults() {
        let p = Path::new("/nonexistent-dir-pixpapers/config.toml");
        assert_eq!(Config::load(p).unwrap(), Config::default());
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn round_trip_and_malformed_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.default_pixel = "32".parse().unwrap();
        cfg.default_bg_color = "#123456".parse().unwrap();
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, cfg);

        fs::write(&path, "this is = not [valid").unwrap();
        let err = Config::load(&path).unwrap_err().to_string();
        assert!(
            err.contains(path.to_str().unwrap()),
            "error must name the path: {err}"
        );
    }

    #[test]
    fn defaults_match_plan() {
        let c = Config::default();
        assert!(c.enabled);
        assert_eq!(c.default_pixel, Pixel::B16);
        assert_eq!(c.default_placement, Placement::Fit);
        assert_eq!(c.default_bg_color, RgbCss::BLACK);
        assert_eq!(c.optimize.max_side, 3840);
    }

    proptest! {
        #[test]
        fn save_load_round_trips(
            q in 50u8..=100,
            g in 0u32..4096,
            enabled: bool,
            qi in 0..8u8,
            di in 0..6u8,
        ) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            let cfg = Config {
                enabled,
                palette: PaletteMode::ALL[qi as usize],
                dither: Dither::ALL[di as usize],
                default_gutter_px: g,
                optimize: Optimize { max_side: 3840, jpeg_quality: q },
                ..Default::default()
            };
            cfg.save(&path).unwrap();
            prop_assert_eq!(Config::load(&path).unwrap(), cfg);
        }
    }

    /// Acceptance (PLAN-30 A.4): v0.1 config files keep loading — the bool
    /// knob maps to auto/off; new saves are string-only.
    #[test]
    fn legacy_palette_bool_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "palette = true\n").unwrap();
        assert_eq!(Config::load(&path).unwrap().palette, PaletteMode::Auto);
        fs::write(&path, "palette = false\n").unwrap();
        assert_eq!(Config::load(&path).unwrap().palette, PaletteMode::Off);
        fs::write(&path, "palette = \"nes\"\ndither = \"fs\"\n").unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.palette, PaletteMode::Nes);
        assert_eq!(c.dither, Dither::FloydSteinberg);
        assert!(
            Config::load(&{
                let p = dir.path().join("bad.toml");
                fs::write(&p, "palette = \"pvm\"\n").unwrap();
                p
            })
            .is_err()
        );
        let c2 = Config {
            palette: PaletteMode::Vga,
            dither: Dither::Atkinson,
            ..Default::default()
        };
        c2.save(&path).unwrap();
        let txt = fs::read_to_string(&path).unwrap();
        assert!(txt.contains("palette = \"vga\""), "{txt}");
        assert!(txt.contains("dither = \"atkinson\""), "{txt}");
    }
}
