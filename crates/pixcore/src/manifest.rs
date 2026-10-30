use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::MANIFEST_VERSION;
use crate::RgbCss;
use crate::config::write_atomic;
use crate::error::{PixError, Result};
use crate::palette::{Dither, PaletteMode};
use crate::paths::Paths;
use crate::placement::Placement;
use crate::preset::Pixel;

/// Per-wallpaper knobs. `None` means "inherit the config default".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel: Option<Pixel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg_color: Option<RgbCss>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gutter_px: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<PaletteMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dither: Option<Dither>,
}

/// One library entry. The **original** lives on disk at
/// `library/<id>/original.<ext>` (R7); variants are generated lazily beside it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// blake3 of the source bytes at add-time (dedupe key).
    pub hash: String,
    /// Stored original extension (post-optimization): "png" | "jpg".
    pub ext: String,
    pub added_unix: i64,
    pub width: u32,
    pub height: u32,
    pub original_bytes: u64,
    /// How the original was optimized (for honesty/`optimize --all`).
    pub optimize_note: String,
    pub settings: Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    /// Currently displayed entry (daemon replays this on start; `cycle` order).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub entries: Vec<Entry>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            current: None,
            entries: Vec::new(),
        }
    }
}

impl Library {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.manifest_file();
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(PixError::io(path, e)),
        };
        let lib: Library = serde_json::from_str(&text).map_err(|e| PixError::Manifest {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        if lib.version != MANIFEST_VERSION {
            return Err(PixError::Manifest {
                path,
                reason: format!(
                    "unsupported version {} (expected {})",
                    lib.version, MANIFEST_VERSION
                ),
            });
        }
        Ok(lib)
    }

    /// Atomic save (temp + rename) — a crash never corrupts the manifest.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let path = paths.manifest_file();
        let mut with_trailing =
            serde_json::to_string_pretty(self).map_err(|e| PixError::other(e.to_string()))?;
        with_trailing.push('\n');
        write_atomic(&path, with_trailing.as_bytes()).map_err(|e| PixError::io(&path, e))
    }

    pub fn find(&self, id_or_title: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.id == id_or_title || e.title == id_or_title)
            // unique-title prefix match for ergonomics
            .or_else(|| {
                let mut hits = self
                    .entries
                    .iter()
                    .filter(|e| e.id.starts_with(id_or_title) || e.title.starts_with(id_or_title));
                let first = hits.next()?;
                hits.next().is_none().then_some(first)
            })
    }

    pub fn find_index(&self, id_or_title: &str) -> Option<usize> {
        self.entries.iter().position(|e| {
            let id_match = e.id == id_or_title || e.id.starts_with(id_or_title);
            let title_match = e.title == id_or_title || e.title.starts_with(id_or_title);
            id_match || title_match
        })
    }

    pub fn remove(&mut self, paths: &Paths, id_or_title: &str) -> Result<Entry> {
        let idx = self.find_index(id_or_title).ok_or_else(|| {
            PixError::NotFound(format!(
                "no entry matching {id_or_title:?} (see `pixpapers list`)"
            ))
        })?;
        let entry = self.entries.remove(idx);
        let dir = paths.entry_dir(&entry.id);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| PixError::io(dir, e))?;
        }
        if self.current.as_deref() == Some(entry.id.as_str()) {
            self.current = None;
        }
        Ok(entry)
    }

    pub fn set_current(&mut self, id: impl Into<String>) {
        self.current = Some(id.into());
    }

    /// Delete variant files of this entry not referenced by `keep` paths.
    pub fn prune_variants(&self, paths: &Paths, id: &str, keep: &[PathBuf]) -> io::Result<()> {
        let dir = paths.variants_dir(id);
        if !dir.is_dir() {
            return Ok(());
        }
        for file in fs::read_dir(&dir)? {
            let file = file?;
            let p = file.path();
            if p.is_file() && !keep.iter().any(|k| k == &p) {
                let _ = fs::remove_file(p);
            }
        }
        Ok(())
    }
}

/// Compute the library id + content hash for a file.
pub fn hash_file(path: &Path) -> Result<(String, String)> {
    let bytes = fs::read(path).map_err(|e| PixError::io(path, e))?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    Ok((hash[..8].to_string(), hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_in(dir: &Path) -> Paths {
        Paths::with_root(dir)
    }

    #[test]
    fn save_load_round_trip_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        paths.ensure_dirs().unwrap();
        let mut lib = Library::default();
        lib.entries.push(Entry {
            id: "a1b2c3d4".into(),
            title: "nebula".into(),
            hash: "x".repeat(64),
            ext: "png".into(),
            added_unix: 0,
            width: 1920,
            height: 1080,
            original_bytes: 1234,
            optimize_note: "png".into(),
            settings: Settings::default(),
        });
        lib.set_current("a1b2c3d4");
        lib.save(&paths).unwrap();
        let loaded = Library::load(&paths).unwrap();
        assert_eq!(loaded, lib);
        // no tmp leftovers
        let leftovers: Vec<_> = fs::read_dir(paths.data_dir)
            .unwrap()
            .flatten()
            .filter(|f| f.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    /// Acceptance (PLAN-30 A.4): old manifests (no palette/dither fields)
    /// parse unchanged — missing means "inherit the config default".
    #[test]
    fn settings_knobs_parse_old_and_new_forms() {
        let old: Settings = serde_json::from_str(r#"{"pixel":"16"}"#).unwrap();
        assert_eq!(old.palette, None);
        assert_eq!(old.dither, None);
        // New fields serialize as lowercase tokens, skipped when None.
        let s = Settings {
            palette: Some(PaletteMode::Nes),
            dither: Some(Dither::Atkinson),
            ..Default::default()
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"palette":"nes","dither":"atkinson"}"#);
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        assert_eq!(serde_json::to_string(&Settings::default()).unwrap(), "{}");
    }

    #[test]
    fn find_by_prefix_id_title() {
        let dir = tempfile::tempdir().unwrap();
        let _paths = paths_in(dir.path());
        let mut lib = Library::default();
        lib.entries.push(Entry {
            id: "abcd1234".into(),
            title: "mountain view".into(),
            ..Entry {
                id: "x".into(),
                title: "y".into(),
                hash: String::new(),
                ext: "png".into(),
                added_unix: 0,
                width: 0,
                height: 0,
                original_bytes: 0,
                optimize_note: String::new(),
                settings: Settings::default(),
            }
        });
        assert!(lib.find("abcd").is_some()); // id prefix
        assert!(lib.find("mountain").is_some()); // title prefix
        assert!(lib.find("nope").is_none());
    }

    #[test]
    fn remove_deletes_entry_dir_and_current() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        paths.ensure_dirs().unwrap();
        let mut lib = Library::default();
        let e = Entry {
            id: "deadbeef".into(),
            title: "t".into(),
            hash: "h".into(),
            ext: "png".into(),
            added_unix: 0,
            width: 1,
            height: 1,
            original_bytes: 1,
            optimize_note: String::new(),
            settings: Settings::default(),
        };
        fs::create_dir_all(paths.entry_dir("deadbeef")).unwrap();
        fs::create_dir_all(paths.variants_dir("deadbeef")).unwrap();
        fs::write(
            paths
                .variants_dir("deadbeef")
                .join("v1-16-fit-1920x1080.png"),
            b"x",
        )
        .unwrap();
        lib.entries.push(e.clone());
        lib.set_current("deadbeef");
        lib.remove(&paths, "dead").unwrap();
        assert!(!paths.entry_dir("deadbeef").exists());
        assert_eq!(lib.current, None);
        assert!(lib.entries.is_empty());
    }

    #[test]
    fn wrong_version_is_an_error_not_a_wipe() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        fs::create_dir_all(&paths.data_dir).unwrap();
        fs::write(paths.manifest_file(), r#"{"version":99,"entries":[]}"#).unwrap();
        let err = Library::load(&paths).unwrap_err().to_string();
        assert!(err.contains("version"), "{err}");
    }
}
