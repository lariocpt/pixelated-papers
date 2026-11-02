//! Headless view-model for the native GUI (feature `gui`).
//!
//! This module owns **no** windowing type and imports nothing from eframe —
//! every behaviour the control window has (selection, batched knob edits,
//! the dirty/save state, set-now, enable/disable, slideshow, defaults, add,
//! remove, the status view, thumbnail resolution) is plain state mutation
//! over `pixcore` + the **same** [`commands`] + [`cli::apply_now`] seam the
//! CLI and the TUI use. That is invariant #1 of the design: a front-end is
//! a *client*, never a second owner of state. The eframe layer in
//! [`super::app`] only paints this model and forwards widget events into
//! these methods, so all of it is unit-testable without a display.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Result;
use pixcore::{
    Config, Dither, Entry, GutterStyle, Library, PaletteMode, Paths, Pixel, Placement, Settings,
};

use crate::cli;
use crate::commands::{self, SlideshowAction};

/// Placement cycle order for the drop-down. `pixcore` exposes no
/// `Placement::ALL` (the TUI carries its own one-row table for the same
/// reason, PLAN-30 S7), so the GUI keeps a matching additive table here.
pub const PLACEMENTS: [Placement; 4] = [
    Placement::Fit,
    Placement::CoverGutter,
    Placement::CoverCrop,
    Placement::Stretch,
];

/// The two panes of the control window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Library,
    Status,
}

/// A batched, not-yet-saved edit of one entry's settings (+ its title).
/// Fields mirror [`Settings`] one-for-one so [`Draft::apply_to`] reproduces
/// exactly what a CLI knob command writes — `Some(v)` overrides, `None`
/// inherits the defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub name: String,
    pub pixel: Option<Pixel>,
    pub placement: Option<Placement>,
    pub palette: Option<PaletteMode>,
    pub dither: Option<Dither>,
    pub gutter_style: Option<GutterStyle>,
    /// Text field: `"#rrggbb"`, or empty to inherit. Parsed on save.
    pub bg: String,
    /// Text field: an integer, or empty to inherit. Parsed on save.
    pub gutter: String,
}

impl Draft {
    /// Seed a draft from an entry's committed state.
    fn from_entry(e: &Entry) -> Self {
        let s = &e.settings;
        Self {
            name: e.title.clone(),
            pixel: s.pixel,
            placement: s.placement,
            palette: s.palette,
            dither: s.dither,
            gutter_style: s.gutter_style,
            bg: s.bg_color.map(|c| c.to_string()).unwrap_or_default(),
            gutter: s.gutter_px.map(|g| g.to_string()).unwrap_or_default(),
        }
    }

    /// Write the draft's parsed fields into a [`Settings`], the exact shape
    /// a `commands::edit_entry` closure mutates. Parse errors are surfaced
    /// by the caller before this runs, so parsing here assumes valid input
    /// and simply ignores an unparseable optional (leaves it inherited).
    fn apply_to(&self, s: &mut Settings) {
        s.pixel = self.pixel;
        s.placement = self.placement;
        s.palette = self.palette;
        s.dither = self.dither;
        s.gutter_style = self.gutter_style;
        s.bg_color = RgbCssText::parse(&self.bg);
        s.gutter_px = self.gutter.trim().parse::<u32>().ok();
    }

    /// Has the draft diverged from the entry's committed settings/title?
    fn differs_from(&self, e: &Entry) -> bool {
        let mut probe = e.settings.clone();
        self.apply_to(&mut probe);
        self.name != e.title || probe != e.settings
    }
}

/// Thin wrapper so bg-colour parsing lives in one place (mirrors the CLI's
/// `RgbCss` value parser via `FromStr`).
struct RgbCssText;
impl RgbCssText {
    fn parse(s: &str) -> Option<pixcore::RgbCss> {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        pixcore::RgbCss::from_str(t).ok()
    }
}

/// Redirect fd 1 + 2 to `/dev/null` for the lifetime of a mutation so the
/// `println!`s inside `commands::*` (and any daemon note from `apply_now`)
/// never spam the terminal that launched the window. Unlike the TUI, the
/// GUI paints into its own window, so there is no alternate screen to
/// smear — but the launch terminal stays clean. Redirecting to `/dev/null`
/// (not a pipe) means a spawned child inheriting fd 1 is harmless: there is
/// nothing to drain and no EOF to await, so this is safe even while
/// `apply_now` auto-spawns a daemon.
///
/// A complete no-op under `cfg(test)`: libtest captures prints at the Rust
/// level, and touching process-global fds would race with other tests'
/// subprocess spawns (the deadlock the TUI's `Quiet` documents).
#[cfg(not(test))]
struct Quiet {
    saved: [libc::c_int; 2],
}

#[cfg(not(test))]
impl Quiet {
    fn arm() -> Self {
        let devnull = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY) };
        let mut saved = [-1 as libc::c_int; 2];
        if devnull >= 0 {
            for (slot, &fd) in [1, 2].iter().enumerate() {
                saved[slot] = unsafe { libc::dup(fd) };
                unsafe { libc::dup2(devnull, fd) };
            }
            unsafe { libc::close(devnull) };
        }
        Self { saved }
    }
}

#[cfg(not(test))]
impl Drop for Quiet {
    fn drop(&mut self) {
        for (slot, &fd) in [1, 2].iter().enumerate() {
            if self.saved[slot] >= 0 {
                unsafe {
                    libc::dup2(self.saved[slot], fd);
                    libc::close(self.saved[slot]);
                }
            }
        }
    }
}

/// Run `f` with the launch terminal silenced (real builds) or as-is (tests).
#[cfg(not(test))]
fn with_quiet<R>(f: impl FnOnce() -> R) -> R {
    let _q = Quiet::arm();
    f()
}

#[cfg(test)]
fn with_quiet<R>(f: impl FnOnce() -> R) -> R {
    f()
}

/// The whole control window's state, minus every pixel of chrome.
pub struct Gui {
    pub paths: Paths,
    pub cfg: Config,
    pub lib: Library,
    pub view: View,
    pub sel: usize,
    /// Transient status-bar message (mirrors the TUI's `last`).
    pub last: String,
    draft: Draft,
    /// `add` form path buffer.
    pub add_path: String,
    /// `[slideshow] interval` text buffer (seconds).
    pub interval: String,
    /// Rendered `status` report (JSON text), refreshed on demand.
    pub status_text: Option<String>,
}

impl Gui {
    pub fn new(paths: Paths, cfg: Config) -> Result<Self> {
        let lib = Library::load(&paths).unwrap_or_default();
        let draft = lib.entries.first().map(Draft::from_entry).unwrap_or(Draft {
            name: String::new(),
            pixel: None,
            placement: None,
            palette: None,
            dither: None,
            gutter_style: None,
            bg: String::new(),
            gutter: String::new(),
        });
        Ok(Self {
            paths,
            cfg,
            lib,
            view: View::Library,
            sel: 0,
            last: "ready".into(),
            draft,
            add_path: String::new(),
            interval: String::new(),
            status_text: None,
        })
    }

    // ---- selection ------------------------------------------------------

    pub fn selected(&self) -> Option<&Entry> {
        self.lib.entries.get(self.sel)
    }

    /// Re-seed the draft from the currently selected entry (drops edits).
    fn reload_draft(&mut self) {
        if let Some(e) = self.selected() {
            self.draft = Draft::from_entry(e);
        }
    }

    pub fn select(&mut self, i: usize) {
        if i < self.lib.entries.len() && i != self.sel {
            self.sel = i;
            self.reload_draft();
        }
    }

    pub fn move_sel(&mut self, delta: i64) {
        if self.lib.entries.is_empty() {
            self.sel = 0;
            return;
        }
        let n = self.lib.entries.len() as i64;
        let next = ((self.sel as i64 + delta).rem_euclid(n)) as usize;
        if next != self.sel {
            self.sel = next;
            self.reload_draft();
        }
    }

    // ---- draft editing --------------------------------------------------

    /// The current draft (read-only view for painting).
    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    /// Mutable access so the paint layer binds widgets straight onto the
    /// draft's fields (combo boxes → `Option<T>`, text edits → strings).
    /// Dirty state is recomputed with [`Gui::draft_dirty`]; nothing is
    /// written to disk until [`Gui::save`].
    pub fn draft_mut(&mut self) -> &mut Draft {
        &mut self.draft
    }

    /// Is the draft dirty vs the selected entry (drives Save/Revert enable)?
    pub fn draft_dirty(&self) -> bool {
        self.selected().is_some_and(|e| self.draft.differs_from(e))
    }

    /// Validate the draft's text fields; returns an error string on the
    /// first bad one, or `None` when the draft is safe to persist.
    pub fn validate(&self) -> Option<String> {
        let bg = self.draft.bg.trim();
        if !bg.is_empty() && pixcore::RgbCss::from_str(bg).is_err() {
            return Some(format!("bad colour {bg:?} (want #rrggbb)"));
        }
        let g = self.draft.gutter.trim();
        if !g.is_empty() && g.parse::<u32>().is_err() {
            return Some(format!("bad gutter {g:?} (want a whole number)"));
        }
        None
    }

    /// Persist the draft through `commands::edit_entry` — the exact seam the
    /// CLI knob commands use. Applies immediately when the edited entry is
    /// the current one (else notes it applies on `set`). Returns whether a
    /// write happened.
    pub fn save(&mut self) -> Result<bool> {
        let Some(entry) = self.selected() else {
            self.last = "nothing selected".into();
            return Ok(false);
        };
        if let Some(err) = self.validate() {
            self.last = err;
            return Ok(false);
        }
        if !self.draft_dirty() {
            self.last = "no changes".into();
            return Ok(false);
        }
        let id = entry.id.clone();
        let name = self.draft.name.clone();
        let draft = self.draft.clone();
        let res = with_quiet(|| {
            let (e, was_current) =
                commands::edit_entry(&self.paths, &id, move |s| draft.apply_to(s))?;
            let note = if was_current {
                crate::cli::apply_now(&self.paths);
                ""
            } else {
                " (not current — will apply on set)"
            };
            Ok::<_, anyhow::Error>((e, note))
        });
        match res {
            Ok((e, note)) => {
                // A title change isn't a `Settings` edit, so write it via a
                // dedicated path only if it actually diverged.
                let mut title_note = String::new();
                if name != e.title {
                    self.rename(&id, &name)?;
                    title_note = " · title".into();
                }
                self.last = format!("{} → updated{note}{title_note}", e.id);
                self.refresh()?;
                self.select_by_id(&id);
                Ok(true)
            }
            Err(e) => {
                self.last = format!("save failed: {e}");
                Ok(false)
            }
        }
    }

    /// Rename the selected entry (title only) through a manifest edit.
    fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        with_quiet(|| {
            let mut lib = Library::load(&self.paths)?;
            if let Some(e) = lib.entries.iter_mut().find(|e| e.id == id) {
                e.title = name.to_string();
                lib.save(&self.paths)?;
                crate::cli::apply_now(&self.paths);
            }
            Ok::<_, anyhow::Error>(())
        })
    }

    /// Discard the draft, re-seeding from the selected entry.
    pub fn revert(&mut self) {
        self.reload_draft();
        self.last = "reverted".into();
    }

    // ---- library actions (CLI-identical seams) --------------------------

    /// `pixpapers set <id>` — make the selected entry current.
    pub fn set_now(&mut self) -> Result<bool> {
        let Some(entry) = self.selected() else {
            self.last = "nothing selected".into();
            return Ok(false);
        };
        let id = entry.id.clone();
        let res = with_quiet(|| {
            let e = commands::set_current(&self.paths, &id)?;
            crate::cli::apply_now(&self.paths);
            Ok::<_, anyhow::Error>((e, cli::state_line(&self.paths)))
        });
        match res {
            Ok((e, state)) => {
                self.last = format!("set {} · {} ({})", e.id, e.title, state);
                self.refresh()?;
                self.select_by_id(&e.id);
                Ok(true)
            }
            Err(e) => {
                self.last = format!("set failed: {e}");
                Ok(false)
            }
        }
    }

    /// `pixpapers cycle [by]` — rotate current by `by`.
    pub fn cycle(&mut self, by: i64) -> Result<bool> {
        if self.lib.entries.is_empty() {
            self.last = "library empty — add one first".into();
            return Ok(false);
        }
        let res = with_quiet(|| {
            let e = commands::cycle_current(&self.paths, by)?;
            crate::cli::apply_now(&self.paths);
            Ok::<_, anyhow::Error>((e, cli::state_line(&self.paths)))
        });
        match res {
            Ok((e, state)) => {
                self.last = format!("cycle → {} · {} ({})", e.id, e.title, state);
                self.refresh()?;
                self.select_by_id(&e.id);
                Ok(true)
            }
            Err(e) => {
                self.last = format!("cycle failed: {e}");
                Ok(false)
            }
        }
    }

    /// `pixpapers enable` / `disable` — the global pixelation switch.
    pub fn set_enabled(&mut self, on: bool) -> Result<()> {
        let mut cfg = self.cfg.clone();
        with_quiet(|| {
            commands::set_enabled(&self.paths, &mut cfg, on)?;
            crate::cli::apply_now(&self.paths);
            Ok::<_, anyhow::Error>(())
        })?;
        self.last = format!("pixelation {}", if on { "on" } else { "off" });
        self.refresh()
    }

    /// `pixpapers remove <id>` — delete the selected entry.
    pub fn remove_selected(&mut self) -> Result<bool> {
        let Some(entry) = self.selected() else {
            self.last = "nothing selected".into();
            return Ok(false);
        };
        let id = entry.id.clone();
        with_quiet(|| commands::remove(&self.paths, &id))?;
        self.last = format!("removed {id}");
        self.refresh()?;
        // `sel` was clamped by refresh; land near where the entry was.
        if self.sel >= self.lib.entries.len() && !self.lib.entries.is_empty() {
            self.sel = self.lib.entries.len() - 1;
        }
        self.reload_draft();
        Ok(true)
    }

    /// `pixpapers add <path> [title]` — ingest a file into the library.
    pub fn add(&mut self) -> Result<bool> {
        let raw = self.add_path.trim().to_string();
        if raw.is_empty() {
            self.last = "enter a file path to add".into();
            return Ok(false);
        }
        let path = expand_tilde(&raw);
        if !path.is_file() {
            self.last = format!("no such file: {raw}");
            return Ok(false);
        }
        let cfg = self.cfg.clone();
        let res = with_quiet(|| commands::add(&self.paths, &cfg, &path, None));
        match res {
            Ok(e) => {
                self.last = format!("added {} · {}", e.id, e.title);
                self.add_path.clear();
                self.refresh()?;
                self.select_by_id(&e.id);
                Ok(true)
            }
            Err(err) => {
                self.last = format!("add failed: {err}");
                Ok(false)
            }
        }
    }

    // ---- slideshow ------------------------------------------------------

    /// `pixpapers slideshow on [sec]` / `off` / `interval <sec>`.
    fn slideshow(&mut self, action: SlideshowAction, msg: String) -> Result<()> {
        let mut cfg = self.cfg.clone();
        let res = with_quiet(|| {
            commands::set_slideshow(&self.paths, &mut cfg, action)?;
            crate::cli::apply_now(&self.paths);
            Ok::<_, anyhow::Error>(())
        });
        if let Err(e) = res {
            self.last = format!("slideshow failed: {e}");
            return Ok(());
        }
        self.last = msg;
        self.refresh()
    }

    pub fn slideshow_on(&mut self) -> Result<()> {
        self.slideshow(
            SlideshowAction::On { interval: None },
            "slideshow on".into(),
        )
    }
    pub fn slideshow_off(&mut self) -> Result<()> {
        self.slideshow(SlideshowAction::Off, "slideshow off".into())
    }
    pub fn slideshow_interval(&mut self) -> Result<()> {
        match self.interval.trim().parse::<u64>() {
            Ok(secs) => {
                let msg = format!("slideshow interval {secs}s");
                self.slideshow(SlideshowAction::Interval { secs }, msg)
            }
            Err(_) => {
                self.last = "interval needs a whole number of seconds".into();
                Ok(())
            }
        }
    }

    // ---- defaults (each mirrors `pixpapers defaults <what> <v>`) --------

    fn set_default<F>(&mut self, what: &str, f: F) -> Result<()>
    where
        F: FnOnce(&Paths, &mut Config) -> Result<()>,
    {
        let mut cfg = self.cfg.clone();
        let res = with_quiet(|| {
            f(&self.paths, &mut cfg)?;
            crate::cli::apply_now(&self.paths);
            Ok::<_, anyhow::Error>(())
        });
        if let Err(e) = res {
            self.last = format!("default failed: {e}");
            return Ok(());
        }
        self.last = format!("default {what}");
        self.refresh()
    }

    pub fn default_pixel(&mut self, v: Pixel) -> Result<()> {
        self.set_default("pixel", move |p, c| commands::set_default_pixel(p, c, v))
    }
    pub fn default_placement(&mut self, v: Placement) -> Result<()> {
        self.set_default("placement", move |p, c| {
            commands::set_default_placement(p, c, v)
        })
    }
    pub fn default_palette(&mut self, v: PaletteMode) -> Result<()> {
        self.set_default("palette", move |p, c| {
            commands::set_default_palette(p, c, v)
        })
    }
    pub fn default_dither(&mut self, v: Dither) -> Result<()> {
        self.set_default("dither", move |p, c| commands::set_default_dither(p, c, v))
    }
    pub fn default_gutter_style(&mut self, v: GutterStyle) -> Result<()> {
        self.set_default("gutter-style", move |p, c| {
            commands::set_default_gutter_style(p, c, v)
        })
    }

    // ---- status view ----------------------------------------------------

    /// Recompute the `status` report for the status pane.
    pub fn refresh_status(&mut self) {
        let r = crate::status::local_report(&self.paths, &self.cfg);
        self.status_text = Some(serde_json::to_string_pretty(&r).unwrap_or_default());
    }

    // ---- thumbnails -----------------------------------------------------

    /// The cached variant PNG nearest (but not above) `box_px` for the
    /// selected entry, or `None` when no variant is cached yet. Reuses what
    /// `set`/`export` already rendered — the GUI never launches a render.
    pub fn thumbnail_path(&self) -> Option<PathBuf> {
        self.thumbnail_for(self.selected()?, 512)
    }

    fn thumbnail_for(&self, entry: &Entry, box_px: u32) -> Option<PathBuf> {
        let dir = self.paths.variants_dir(&entry.id);
        let rd = std::fs::read_dir(&dir).ok()?;
        // Smallest cached variant at or above the box (sharp down-scale);
        // fall back to the largest available. Filenames embed `WxH` as
        // documented in pixcore::variant.
        let mut cands: Vec<(u32, PathBuf)> = Vec::new();
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|e| e.to_str()) != Some("png") {
                continue;
            }
            if let Some(w) = variant_width(&p) {
                cands.push((w, p));
            }
        }
        if cands.is_empty() {
            return None;
        }
        cands.sort_by_key(|(w, _)| *w);
        cands
            .iter()
            .find(|(w, _)| *w >= box_px)
            .or_else(|| cands.last())
            .map(|(_, p)| p.clone())
    }

    // ---- helpers --------------------------------------------------------

    /// Reload manifest + config from disk and clamp the selection. After any
    /// mutation the on-disk state is authoritative (the CLI owns it).
    pub fn refresh(&mut self) -> Result<()> {
        self.cfg = Config::load(&self.paths.config_file)?;
        self.lib = Library::load(&self.paths)?;
        if self.sel >= self.lib.entries.len() {
            self.sel = self.lib.entries.len().saturating_sub(1);
        }
        self.reload_draft();
        Ok(())
    }

    fn select_by_id(&mut self, id: &str) {
        if let Some(i) = self.lib.entries.iter().position(|e| e.id == id) {
            self.sel = i;
            self.reload_draft();
        }
    }
}

/// Parse the `W` from a variant filename's embedded `WxH` token.
fn variant_width(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    for tok in stem.split('-') {
        if let Some((w, h)) = tok.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                if w > 0 && h > 0 {
                    return Some(w);
                }
            }
        }
    }
    None
}

/// Tilde expansion — a local copy of pixcore's / the TUI's private one.
pub fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

// ---------------------------------------------------------------------------
// Headless view-model tests.
//
// These compile into the *binary's* test harness, so `cli::apply_now` is the
// real (headless no-op) seam and the model can mutate a hermetic home without
// a display. The two byte-identity tests drive the same home through the real
// CLI subprocess and through this view-model and demand identical manifest
// bytes — that is the machine-checked proof of invariant #1 (a GUI "set" is
// byte-for-byte the same disk write as a CLI `set`).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Home, fixture, skip_without_cli_binary};

    /// A view-model pointed at the same hermetic home (config re-read from
    /// disk, exactly as `run()` does at launch).
    fn gui_at(home: &Home) -> Gui {
        let cfg = Config::load(&home.paths.config_file).unwrap();
        Gui::new(home.paths.clone(), cfg).unwrap()
    }

    /// Seed one entry straight through `commands::add` (no subprocess), so
    /// the pure-model tests need no built binary.
    fn seed(home: &Home, title: &str, source: &Path) {
        let cfg = Config::load(&home.paths.config_file).unwrap();
        commands::add(&home.paths, &cfg, source, Some(title.to_string())).unwrap();
    }

    fn index_of(g: &Gui, id: &str) -> usize {
        g.lib.entries.iter().position(|e| e.id == id).unwrap()
    }

    /// Editing a knob marks the draft dirty; `save` persists it through
    /// `commands::edit_entry` and clears dirty; `revert` drops an unsaved
    /// edit back to the committed state.
    #[test]
    fn dirty_state_lifecycle() {
        let home = Home::new();
        seed(&home, "a", &fixture());
        let mut g = gui_at(&home);
        g.refresh().unwrap();
        assert_eq!(g.lib.entries.len(), 1, "seed added one entry");
        assert!(!g.draft_dirty(), "a freshly seeded draft is clean");

        g.draft_mut().pixel = Some(Pixel::B8);
        assert!(g.draft_dirty(), "a knob change marks the draft dirty");

        assert!(g.save().unwrap(), "save writes a dirty draft");
        assert!(!g.draft_dirty(), "save clears the dirty flag");
        let m = home.manifest_text();
        assert!(
            m.contains(&format!("\"pixel\": \"{}\"", Pixel::B8)),
            "manifest must store the saved pixel:\n{m}"
        );

        g.draft_mut().pixel = Some(Pixel::B64);
        assert!(g.draft_dirty(), "second change re-marks dirty");
        g.revert();
        assert!(!g.draft_dirty(), "revert drops the unsaved edit");
        assert_eq!(
            g.selected().unwrap().settings.pixel,
            Some(Pixel::B8),
            "revert kept the saved value"
        );
    }

    /// Bad text fields are rejected by `validate`, and `save` refuses to write
    /// an invalid draft (the manifest is left byte-untouched).
    #[test]
    fn validate_gates_save() {
        let home = Home::new();
        seed(&home, "a", &fixture());
        let mut g = gui_at(&home);
        g.refresh().unwrap();

        g.draft_mut().bg = "#123456".into();
        assert_eq!(g.validate(), None, "a valid hex colour passes");
        g.draft_mut().bg = "#nope".into();
        assert!(g.validate().is_some(), "a malformed colour is rejected");

        g.draft_mut().bg = String::new();
        g.draft_mut().gutter = "12".into();
        assert_eq!(g.validate(), None, "an integer gutter passes");
        g.draft_mut().gutter = "1.5".into();
        assert!(g.validate().is_some(), "a non-integer gutter is rejected");

        let before = home.manifest_text();
        assert!(!g.save().unwrap(), "save refuses an invalid draft");
        assert_eq!(
            home.manifest_text(),
            before,
            "an invalid save writes nothing"
        );
    }

    /// A GUI "set" is byte-identical to `pixpapers set <id>` on the same home:
    /// same manifest bytes, because both run the exact `commands::set_current`
    /// closure and `apply_now` writes no manifest byte.
    #[test]
    fn gui_set_matches_cli_set_bytes() {
        if skip_without_cli_binary() {
            return;
        }
        let home = Home::new();
        seed(&home, "a", &fixture());
        seed(&home, "b", &home.small_source());
        let (id_a, id_b) = (home.entry_id(0), home.entry_id(1));

        // The CLI makes b current; capture the manifest it wrote.
        let (code, _, err) = home.cli(&["set", &id_b]);
        assert_eq!(code, 0, "cli set b failed: {err}");
        let manifest_cli = home.manifest_text();

        // Reset to a through the CLI, then reproduce the same write in the GUI.
        let (code, _, err) = home.cli(&["set", &id_a]);
        assert_eq!(code, 0, "cli set a failed: {err}");
        let mut g = gui_at(&home);
        g.refresh().unwrap();
        g.select(index_of(&g, &id_b));
        assert!(g.set_now().unwrap(), "gui set wrote");

        assert_eq!(
            manifest_cli,
            home.manifest_text(),
            "the GUI's set must reproduce the CLI's manifest byte-for-byte"
        );
    }

    /// A GUI knob `save` is byte-identical to the CLI knob command that writes
    /// the same value on the same home: same `settings` block, same bytes.
    #[test]
    fn gui_save_matches_cli_knob_bytes() {
        if skip_without_cli_binary() {
            return;
        }
        let home = Home::new();
        seed(&home, "a", &fixture());
        let id = home.entry_id(0);

        // The GUI writes pixel = 8 (a, from `add`, is not current → no apply).
        let mut g = gui_at(&home);
        g.refresh().unwrap();
        g.select(index_of(&g, &id));
        g.draft_mut().pixel = Some(Pixel::B8);
        assert!(g.save().unwrap(), "gui save wrote");
        let manifest_gui = home.manifest_text();

        // Clear the knob back to inherit, then let the CLI write the same value.
        g.draft_mut().pixel = None;
        assert!(g.save().unwrap(), "gui clear wrote");
        assert!(
            !home.manifest_text().contains("\"pixel\""),
            "clearing the knob must remove it from the manifest"
        );
        let (code, _, err) = home.cli(&["pixel", &id, &Pixel::B8.to_string()]);
        assert_eq!(code, 0, "cli pixel 8 failed: {err}");

        assert_eq!(
            manifest_gui,
            home.manifest_text(),
            "the GUI's knob save must reproduce the CLI knob command's manifest byte-for-byte"
        );
    }

    /// The global switches reach the same persisted config the CLI's
    /// `enable`/`defaults`/`slideshow` commands write — read back through
    /// `Config::load`, so this holds regardless of serialization form.
    #[test]
    fn global_switches_update_config() {
        let home = Home::new();
        seed(&home, "a", &fixture());
        let mut g = gui_at(&home);
        g.refresh().unwrap();

        g.set_enabled(false).unwrap();
        assert!(
            !Config::load(&home.paths.config_file).unwrap().enabled,
            "disable must persist enabled = false"
        );

        g.default_palette(PaletteMode::Nes).unwrap();
        assert_eq!(
            Config::load(&home.paths.config_file).unwrap().palette,
            PaletteMode::Nes,
            "the defaults combo must persist the library default palette"
        );

        g.slideshow_on().unwrap();
        assert!(
            Config::load(&home.paths.config_file)
                .unwrap()
                .slideshow
                .enabled,
            "slideshow on must persist [slideshow] enabled"
        );
        g.slideshow_off().unwrap();
        assert!(
            !Config::load(&home.paths.config_file)
                .unwrap()
                .slideshow
                .enabled,
            "slideshow off must persist [slideshow] disabled"
        );
    }
}
