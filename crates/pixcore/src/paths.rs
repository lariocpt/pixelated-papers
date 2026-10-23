use std::path::{Path, PathBuf};

use crate::APP_DIR;

/// Filesystem layout. Everything honors `PIXPAPERS_HOME`: when set, that one
/// directory replaces data+config+state roots (hermetic tests, portable
/// setups). Otherwise standard XDG locations.
#[derive(Clone, Debug)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    /// Hermetic root (used by tests; `PIXPAPERS_HOME` resolves to this).
    pub fn with_root(root: &Path) -> Self {
        Self {
            data_dir: root.join("data"),
            config_file: root.join("config.toml"),
            state_dir: root.join("state"),
        }
    }

    pub fn from_env() -> Self {
        if let Ok(home) = std::env::var("PIXPAPERS_HOME") {
            return Self::with_root(&expand_tilde(&home));
        }
        let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            data_dir: dirs::data_dir()
                .map(|d| d.join(APP_DIR))
                .unwrap_or_else(|| base.join(".local/share").join(APP_DIR)),
            config_file: dirs::config_dir()
                .map(|d| d.join(APP_DIR).join("config.toml"))
                .unwrap_or_else(|| base.join(".config").join(APP_DIR).join("config.toml")),
            state_dir: dirs::state_dir()
                .map(|d| d.join(APP_DIR))
                .unwrap_or_else(|| base.join(".local/state").join(APP_DIR)),
        }
    }

    pub fn library_dir(&self) -> PathBuf {
        self.data_dir.join("library")
    }
    pub fn manifest_file(&self) -> PathBuf {
        self.data_dir.join("library.json")
    }
    pub fn entry_dir(&self, id: &str) -> PathBuf {
        self.library_dir().join(id)
    }
    pub fn variants_dir(&self, id: &str) -> PathBuf {
        self.entry_dir(id).join("variants")
    }
    pub fn socket_file(&self) -> PathBuf {
        self.state_dir.join("sock")
    }
    pub fn pid_file(&self) -> PathBuf {
        self.state_dir.join("daemon.pid")
    }
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("daemon.log")
    }

    /// Create the directories that must exist before writes.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.library_dir())?;
        std::fs::create_dir_all(&self.state_dir)?;
        if let Some(parent) = self.config_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    Path::new(s).to_path_buf()
}
