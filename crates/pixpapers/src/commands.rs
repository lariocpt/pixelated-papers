//! Command implementations that run **without** the daemon (library ops,
//! offline export, local edits) plus the policy resolver shared with the
//! daemon. All manifest/config writes are atomic (pixcore guarantees).

use std::path::Path;

use anyhow::{Context, Result};
use pixcore::{
    Config, ContentProbe, Dither, Entry, GutterStyle, Library, PaletteMode, Paths, Pixel,
    Placement, ResolvedSettings, Settings,
};

/// A user-condition error: a bad id, an empty library, an unparseable
/// value — the user said something wrong, no internal work failed.
/// User error, exit 1 (S9 task 7), the same classification rule
/// [`crate::daemon::DaemonStartupError`] follows.
#[derive(Debug)]
pub struct UserError(pub String);

impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UserError {}

/// Exit-code classifier: user errors (bad id, bad value, undecodable file)
/// exit 1; internal trouble exits 2.
pub fn is_user_error(e: &anyhow::Error) -> bool {
    // Daemon start failures are environment problems (no session, bad
    // depth, refused connection) — user error, exit 1.
    if e.downcast_ref::<crate::daemon::DaemonStartupError>()
        .is_some()
    {
        return true;
    }
    // User conditions (bad id, empty library, bad size) — exit 1.
    if e.downcast_ref::<UserError>().is_some() {
        return true;
    }
    if let Some(pe) = e.downcast_ref::<pixcore::PixError>() {
        !matches!(pe, pixcore::PixError::Other(_))
    } else {
        false
    }
}

/// R2/R5/R6: entry settings → config defaults; if the app is disabled the
/// pixel size becomes `Normal` (placement/bg untouched).
pub fn resolve_effective(entry: &Entry, cfg: &Config) -> ResolvedSettings {
    let r = ResolvedSettings::resolve(&entry.settings, cfg);
    if cfg.enabled { r } else { r.unpixelated() }
}

pub fn add(paths: &Paths, cfg: &Config, source: &Path, title: Option<String>) -> Result<Entry> {
    let mut lib = Library::load(paths).context("load library")?;
    let (id, hash) = pixcore::manifest::hash_file(source)
        .with_context(|| format!("hash {}", source.display()))?;
    if let Some(existing) = lib.entries.iter().find(|e| e.hash == hash) {
        println!("already in library: {} ({})", existing.id, existing.title);
        return Ok(existing.clone());
    }
    let opt = pixcore::optimize::optimize(source, &cfg.optimize)
        .with_context(|| format!("optimize {}", source.display()))?;
    let title = title.unwrap_or_else(|| {
        source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "wallpaper".into())
    });
    let mut entry = Entry {
        id: id.clone(),
        title,
        hash,
        ext: opt.ext.to_string(),
        added_unix: now_unix(),
        width: opt.width,
        height: opt.height,
        original_bytes: opt.bytes.len() as u64,
        optimize_note: opt.note.clone(),
        settings: Settings::default(),
    };
    paths.ensure_dirs().context("create library dirs")?;
    let entry_dir = paths.entry_dir(&id);
    std::fs::create_dir_all(&entry_dir).map_err(|e| pixcore::PixError::io(&entry_dir, e))?;
    let orig = entry_dir.join(format!("original.{}", opt.ext));
    std::fs::write(&orig, &opt.bytes).map_err(|e| pixcore::PixError::io(&orig, e))?;
    // If the entry will *inherit* `auto-content`, measure the stored original
    // once here so the render side has a density to read (the analysis is of
    // the optimized bytes — exactly what the daemon renders from).
    if cfg.default_pixel == Pixel::AutoContent {
        entry.settings.content = pixcore::content::analyze_file(&orig).ok();
    }
    lib.entries.push(entry.clone());
    lib.save(paths).context("save manifest")?;
    println!(
        "added {} · {} · {}×{} · {} · {:.0} KB",
        entry.id,
        entry.title,
        entry.width,
        entry.height,
        entry.optimize_note,
        entry.original_bytes as f64 / 1024.0
    );
    Ok(entry)
}

pub fn list(paths: &Paths) -> Result<()> {
    let lib = Library::load(paths).context("load library")?;
    if lib.entries.is_empty() {
        println!("library empty — add one with: pixpapers add <image>");
        return Ok(());
    }
    println!(
        "{:<10} {:<24} {:<7} {:<13} {:<8} {:<8} {:>10} {:>8}",
        "id", "title", "pixel", "placement", "palette", "dither", "size", "file"
    );
    for e in &lib.entries {
        let mark = if lib.current.as_deref() == Some(e.id.as_str()) {
            "*"
        } else {
            " "
        };
        let (px, pl, pal, di) = display_settings(e);
        println!(
            "{mark}{:<9} {:<24.24} {:<7} {:<13} {:<8} {:<8} {:>6}×{:<3} {:>7}",
            e.id,
            e.title,
            px,
            pl,
            pal,
            di,
            e.width,
            format!("{}", e.height),
            human_bytes(e.original_bytes)
        );
    }
    Ok(())
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let mut lib = Library::load(paths).context("load library")?;
    let entry = lib.remove(paths, id).map_err(anyhow::Error::from)?;
    lib.save(paths).context("save manifest")?;
    println!("removed {} ({})", entry.id, entry.title);
    Ok(())
}

/// Mutate one entry knob and persist. Returns (entry, was_current).
pub fn edit_entry(paths: &Paths, id: &str, f: impl FnOnce(&mut Settings)) -> Result<(Entry, bool)> {
    let mut lib = Library::load(paths).context("load library")?;
    let idx = lib
        .find_index(id)
        .ok_or_else(|| UserError(format!("no entry matching {id:?} (see `pixpapers list`)")))?;
    f(&mut lib.entries[idx].settings);
    let was_current = lib.current.as_deref() == Some(lib.entries[idx].id.as_str());
    let entry = lib.entries[idx].clone();
    lib.save(paths).context("save manifest")?;
    Ok((entry, was_current))
}

/// `pixpapers pixel <id> <preset>`: set one entry's pixel look and keep the
/// persisted content probe consistent with it. Choosing `auto-content`
/// analyses the stored original **once** here on the CLI/state side (the
/// daemon never re-analyses) and stores the measurement next to the setting;
/// switching to any other look clears a stale probe so nothing dangles. An
/// undecodable/missing original leaves `content: None`, the documented
/// size-floor fallback (== plain `auto`).
pub fn set_pixel(paths: &Paths, id: &str, pixel: Pixel) -> Result<(Entry, bool)> {
    let mut lib = Library::load(paths).context("load library")?;
    let idx = lib
        .find_index(id)
        .ok_or_else(|| UserError(format!("no entry matching {id:?} (see `pixpapers list`)")))?;
    let entry = &mut lib.entries[idx];
    entry.settings.pixel = Some(pixel);
    entry.settings.content = if pixel == Pixel::AutoContent {
        content_probe_for(paths, &entry.id)
    } else {
        None
    };
    let was_current = lib.current.as_deref() == Some(entry.id.as_str());
    let out = entry.clone();
    lib.save(paths).context("save manifest")?;
    Ok((out, was_current))
}

/// Analyse an entry's stored original into a content probe (see
/// [`pixcore::content`]). Returns `None` (never an error) when the original is
/// missing or undecodable, so callers treat "no probe" as the documented
/// size-floor fallback rather than a failed command. This is the *only* place
/// the picture is analysed — always on the state side, never in the daemon.
pub fn content_probe_for(paths: &Paths, id: &str) -> Option<ContentProbe> {
    let path = pixcore::variant::original_path(paths, id).ok()?;
    pixcore::content::analyze_file(&path).ok()
}

/// Record the current wallpaper (the daemon picks it up live or on start).
pub fn set_current(paths: &Paths, id: &str) -> Result<Entry> {
    let mut lib = Library::load(paths).context("load library")?;
    let entry = lib
        .find(id)
        .cloned()
        .ok_or_else(|| UserError(format!("no entry matching {id:?} (see `pixpapers list`)")))?;
    lib.set_current(entry.id.clone());
    lib.save(paths).context("save manifest")?;
    Ok(entry)
}

/// Cycle `by` steps from the current entry.
pub fn cycle_current(paths: &Paths, by: i64) -> Result<Entry> {
    let mut lib = Library::load(paths).context("load library")?;
    if lib.entries.is_empty() {
        return Err(anyhow::Error::from(pixcore::PixError::empty()));
    }
    let cur_idx = lib
        .current
        .as_deref()
        .and_then(|c| lib.entries.iter().position(|e| e.id == c))
        .unwrap_or(0);
    let n = lib.entries.len() as i64;
    let next = (((cur_idx as i64 + by) % n) + n) % n;
    let entry = lib.entries[next as usize].clone();
    lib.set_current(entry.id.clone());
    lib.save(paths).context("save manifest")?;
    Ok(entry)
}

/// `pixpapers bar` — exactly one line for the waybar module (S8/D.1):
/// `▞ <title> · <pixel>/<placement-short>`, or `▞ pixpapers — set one` /
/// `▞ pixpapers · daemon down`. The label is offline config+manifest
/// truth through the daemon's own resolution chain
/// (`resolve_effective`), so it can never show a look the daemon would
/// not render; the daemon probe only disambiguates the no-entry case.
/// Never writes, never spawns.
pub fn bar(paths: &Paths, cfg: &Config) -> Result<()> {
    let lib = Library::load(paths).ok().unwrap_or_default();
    let current = lib
        .current
        .as_deref()
        .and_then(|id| lib.entries.iter().find(|e| e.id == id))
        .map(|e| (e.title.clone(), resolve_effective(e, cfg)));
    let line = pixcore::bar_label(
        current.as_ref().map(|(t, r)| (t.as_str(), r)),
        crate::ipc::daemon_alive(&paths.socket_file()),
    );
    println!("{line}");
    Ok(())
}

pub fn set_enabled(paths: &Paths, cfg: &mut Config, on: bool) -> Result<()> {
    cfg.enabled = on;
    cfg.save(&paths.config_file).context("save config")?;
    println!(
        "pixelation {}",
        if on {
            "enabled"
        } else {
            "disabled — originals only (still stretched/placed)"
        }
    );
    Ok(())
}

pub fn set_default_pixel(paths: &Paths, cfg: &mut Config, p: Pixel) -> Result<()> {
    cfg.default_pixel = p;
    cfg.save(&paths.config_file).context("save config")?;
    // When the default becomes `auto-content`, the entries that *inherit* it
    // (no per-entry pixel) need a content probe now — the daemon reads a
    // persisted density, it never analyses. Entries with their own explicit
    // look are left untouched (their probe is unaffected by the default).
    if p == Pixel::AutoContent {
        backfill_inherited_probes(paths)?;
    }
    println!("default look: {p}");
    Ok(())
}

/// Compute content probes for every entry that inherits the default pixel
/// (its own `pixel` is unset). Called when the default switches to
/// `auto-content` so those entries carry a fresh density. A missing/undecodable
/// original simply leaves that entry without a probe (size-floor fallback).
/// One manifest write only if anything actually gained a probe.
fn backfill_inherited_probes(paths: &Paths) -> Result<()> {
    let mut lib = Library::load(paths).context("load library")?;
    let mut changed = false;
    for e in &mut lib.entries {
        if e.settings.pixel.is_none() && e.settings.content.is_none() {
            if let Some(probe) = content_probe_for(paths, &e.id) {
                e.settings.content = Some(probe);
                changed = true;
            }
        }
    }
    if changed {
        lib.save(paths).context("save manifest")?;
    }
    Ok(())
}

pub fn set_default_placement(paths: &Paths, cfg: &mut Config, p: Placement) -> Result<()> {
    cfg.default_placement = p;
    cfg.save(&paths.config_file).context("save config")?;
    println!("default placement: {p}");
    Ok(())
}

pub fn set_default_palette(paths: &Paths, cfg: &mut Config, p: PaletteMode) -> Result<()> {
    cfg.palette = p;
    cfg.save(&paths.config_file).context("save config")?;
    println!("default palette: {p}");
    Ok(())
}

pub fn set_default_dither(paths: &Paths, cfg: &mut Config, p: Dither) -> Result<()> {
    cfg.dither = p;
    cfg.save(&paths.config_file).context("save config")?;
    println!("default dither: {p}");
    Ok(())
}

/// `pixpapers default gutter-style <solid|bleed>`: the ring style new /
/// unset entries inherit.
pub fn set_default_gutter_style(paths: &Paths, cfg: &mut Config, style: GutterStyle) -> Result<()> {
    cfg.default_gutter_style = style;
    cfg.save(&paths.config_file).context("save config")?;
    println!("default gutter-style: {style}");
    Ok(())
}

/// The three `pixpapers slideshow …` verbs, resolved to a config mutation.
pub enum SlideshowAction {
    /// Turn cycling on; optionally set the interval in the same call.
    On { interval: Option<u64> },
    /// Turn cycling off (back to the manual `set`/`cycle` wallpaper).
    Off,
    /// Change the seconds-per-wallpaper, leaving on/off untouched.
    Interval { secs: u64 },
}

/// `pixpapers slideshow on|off|interval <sec>`: mutate the `[slideshow]`
/// section and persist (atomic). An interval of `0` is rejected as a user
/// error (exit 1) rather than stored — the pure model treats it as "off", but
/// a `0`-second cycle is never what someone meant.
pub fn set_slideshow(paths: &Paths, cfg: &mut Config, action: SlideshowAction) -> Result<()> {
    match action {
        SlideshowAction::On { interval } => {
            if let Some(secs) = interval {
                if secs == 0 {
                    return Err(anyhow::Error::from(UserError(
                        "interval must be at least 1 second".into(),
                    )));
                }
                cfg.slideshow.interval_secs = secs;
            }
            cfg.slideshow.enabled = true;
            cfg.save(&paths.config_file).context("save config")?;
            println!("slideshow: on · every {}s", cfg.slideshow.interval_secs);
        }
        SlideshowAction::Off => {
            cfg.slideshow.enabled = false;
            cfg.save(&paths.config_file).context("save config")?;
            println!("slideshow: off · showing the manually set wallpaper");
        }
        SlideshowAction::Interval { secs } => {
            if secs == 0 {
                return Err(anyhow::Error::from(UserError(
                    "interval must be at least 1 second".into(),
                )));
            }
            cfg.slideshow.interval_secs = secs;
            cfg.save(&paths.config_file).context("save config")?;
            let state = if cfg.slideshow.enabled { "on" } else { "off" };
            println!("slideshow interval: {secs}s ({state})");
        }
    }
    Ok(())
}

/// Offline render (no daemon) — `pixpapers export`.
pub fn export(
    paths: &Paths,
    cfg: &Config,
    id: &str,
    overrides: &Settings,
    out: (u32, u32),
    dest: Option<&Path>,
) -> Result<()> {
    let lib = Library::load(paths).context("load library")?;
    let entry = lib
        .find(id)
        .cloned()
        .ok_or_else(|| UserError(format!("no entry matching {id:?} (see `pixpapers list`)")))?;
    // entry settings → overrides → config defaults (the one resolution
    // chain the daemon uses, so export == screen)
    let merged = merge_settings(&entry.settings, overrides);
    let resolved = resolve_effective(
        &Entry {
            settings: merged,
            ..entry.clone()
        },
        cfg,
    );
    // An `auto` / `auto-content` pixel resolves against the requested output
    // size — `auto` by the pure size rule, `auto-content` by the persisted
    // density clamped to that size floor (no re-analysis of the picture).
    // Either way the exported file (name, bytes and the line below) names the
    // concrete preset used.
    let content_requested = resolved.pixel == Pixel::AutoContent;
    let auto_requested = resolved.pixel == Pixel::Auto;
    let resolved = resolved.resolved_for_output(out);
    let pixel_token = if content_requested {
        format!("auto-content→{}", resolved.pixel)
    } else if auto_requested {
        format!("auto→{}", resolved.pixel)
    } else {
        resolved.pixel.to_string()
    };
    // The one offline render path, shared with the TUI preview (S6):
    // variant generate-or-cache → composite (view from ORIGINAL dims —
    // invariant #2) → the exact export encoder. Byte-identical to the
    // pre-S6 export bytes; the moved code is verbatim.
    let (final_img, buf) = pixcore::variant::render_offline(
        paths,
        &entry.id,
        (entry.width, entry.height),
        &resolved,
        out,
    )
    .map_err(anyhow::Error::from)
    .with_context(|| format!("entry {} render failed", entry.id))?;
    let dest = dest.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        std::path::PathBuf::from(format!(
            "pixpapers-{}-{}-{}-{}x{}.png",
            entry.id, resolved.pixel, resolved.placement, out.0, out.1
        ))
    });
    std::fs::write(&dest, &buf).map_err(|e| anyhow::anyhow!("write {}: {e}", dest.display()))?;
    println!(
        "wrote {} · {}×{} · {} {} · {:.0} KB",
        dest.display(),
        final_img.width(),
        final_img.height(),
        pixel_token,
        resolved.placement,
        buf.len() as f64 / 1024.0
    );
    Ok(())
}

/// `over` (the CLI's `--` flags) wins; unset fields fall through to `base`
/// (the entry's stored knobs).
fn merge_settings(base: &Settings, over: &Settings) -> Settings {
    Settings {
        pixel: over.pixel.or(base.pixel),
        placement: over.placement.or(base.placement),
        bg_color: over.bg_color.or(base.bg_color),
        gutter_px: over.gutter_px.or(base.gutter_px),
        palette: over.palette.or(base.palette),
        dither: over.dither.or(base.dither),
        gutter_style: over.gutter_style.or(base.gutter_style),
        // The content probe travels with the entry (the CLI's `--pixel` flag
        // never carries one): export must resolve `auto-content` from the
        // measurement the entry already stores, never re-analyse the picture.
        content: over.content.or(base.content),
    }
}

pub fn optimize_all(paths: &Paths, cfg: &Config) -> Result<()> {
    let notes = pixcore::variant::optimize_all(paths, &cfg.optimize);
    for (id, note) in &notes {
        println!("{id}: {note}");
    }
    println!("{} entries scanned", notes.len());
    Ok(())
}

/// Display strings for `list`: the entry's own knob when set, otherwise the
/// resolved value marked `*` (inherit the config default) — the marker
/// convention of pixel/placement, extended to palette/dither.
pub fn display_settings(e: &Entry) -> (String, String, String, String) {
    let cfg = Config::default();
    let r = ResolvedSettings::resolve(&e.settings, &cfg);
    let px = e
        .settings
        .pixel
        .map(|p| p.to_string())
        .unwrap_or_else(|| format!("{}*", r.pixel));
    let pl = e
        .settings
        .placement
        .map(|p| p.to_string())
        .unwrap_or_else(|| format!("{}*", r.placement));
    let pal = e
        .settings
        .palette
        .map(|p| p.to_string())
        .unwrap_or_else(|| format!("{}*", r.palette));
    let di = e
        .settings
        .dither
        .map(|d| d.to_string())
        .unwrap_or_else(|| format!("{}*", r.dither));
    (px, pl, pal, di)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn human_bytes(b: u64) -> String {
    const K: u64 = 1024;
    if b < K {
        format!("{}B", b)
    } else if b < K * K {
        format!("{:.0}K", b as f64 / K as f64)
    } else {
        format!("{:.1}M", b as f64 / (K * K) as f64)
    }
}

#[cfg(test)]
mod tests {
    //! S9 task 7: user-condition errors classify as exit 1 (the messages
    //! stayed byte-identical; only the classification changed).

    use super::{cycle_current, edit_entry, export, is_user_error, set_current};
    use crate::harness::Home;
    use pixcore::{Config, Settings};

    /// `cycle` on an empty library is a user error (exit 1, was 2) and
    /// keeps the exact wording `list` prints for the same condition.
    #[test]
    fn cycle_empty_library_is_user_error() {
        let home = Home::new();
        let err = cycle_current(&home.paths, 1).expect_err("cycle on an empty library must fail");
        assert!(is_user_error(&err), "exit must be 1, not 2");
        assert_eq!(
            err.to_string(),
            "library empty — add one with: pixpapers add <image>"
        );
    }

    /// Unknown ids on the id-taking paths are user errors, byte-identical
    /// to the pre-audit messages.
    #[test]
    fn unknown_id_is_user_error() {
        let home = Home::new();
        let id = "0123456789ab";
        let want = "no entry matching \"0123456789ab\" (see `pixpapers list`)";
        let err = set_current(&home.paths, id).expect_err("unknown id must fail");
        assert!(is_user_error(&err));
        assert_eq!(err.to_string(), want);
        let err = edit_entry(&home.paths, id, |_| {}).expect_err("unknown id must fail");
        assert!(is_user_error(&err));
        assert_eq!(err.to_string(), want);
        let err = export(
            &home.paths,
            &Config::default(),
            id,
            &Settings::default(),
            (16, 16),
            None,
        )
        .expect_err("unknown id must fail");
        assert!(is_user_error(&err));
        assert_eq!(err.to_string(), want);
    }
}
