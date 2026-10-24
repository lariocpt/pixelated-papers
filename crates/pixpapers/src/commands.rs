//! Command implementations that run **without** the daemon (library ops,
//! offline export, local edits) plus the policy resolver shared with the
//! daemon. All manifest/config writes are atomic (pixcore guarantees).

use std::path::Path;

use anyhow::{Context, Result, bail};
use pixcore::{Config, Entry, Library, Paths, Pixel, Placement, ResolvedSettings, Settings};

/// Exit-code classifier: user errors (bad id, bad value, undecodable file)
/// exit 1; internal trouble exits 2.
pub fn is_user_error(e: &anyhow::Error) -> bool {
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
    let entry = Entry {
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
        "{:<10} {:<24} {:<7} {:<13} {:>10} {:>8}",
        "id", "title", "pixel", "placement", "size", "file"
    );
    for e in &lib.entries {
        let mark = if lib.current.as_deref() == Some(e.id.as_str()) {
            "*"
        } else {
            " "
        };
        let (px, pl) = display_settings(e);
        println!(
            "{mark}{:<9} {:<24.24} {:<7} {:<13} {:>6}×{:<3} {:>7}",
            e.id,
            e.title,
            px,
            pl,
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
        .ok_or_else(|| anyhow::anyhow!("no entry matching {id:?} (see `pixpapers list`)"))?;
    f(&mut lib.entries[idx].settings);
    let was_current = lib.current.as_deref() == Some(lib.entries[idx].id.as_str());
    let entry = lib.entries[idx].clone();
    lib.save(paths).context("save manifest")?;
    Ok((entry, was_current))
}

/// Record the current wallpaper (the daemon picks it up live or on start).
pub fn set_current(paths: &Paths, id: &str) -> Result<Entry> {
    let mut lib = Library::load(paths).context("load library")?;
    let entry = lib
        .find(id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no entry matching {id:?} (see `pixpapers list`)"))?;
    lib.set_current(entry.id.clone());
    lib.save(paths).context("save manifest")?;
    Ok(entry)
}

/// Cycle `by` steps from the current entry.
pub fn cycle_current(paths: &Paths, by: i64) -> Result<Entry> {
    let mut lib = Library::load(paths).context("load library")?;
    if lib.entries.is_empty() {
        bail!("library empty — add one with: pixpapers add <image>");
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
    println!("default look: {p}");
    Ok(())
}

pub fn set_default_placement(paths: &Paths, cfg: &mut Config, p: Placement) -> Result<()> {
    cfg.default_placement = p;
    cfg.save(&paths.config_file).context("save config")?;
    println!("default placement: {p}");
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
        .ok_or_else(|| anyhow::anyhow!("no entry matching {id:?} (see `pixpapers list`)"))?;
    // entry settings → overrides → config defaults
    let merged = merge_settings(&entry.settings, overrides);
    let base = resolve_effective(
        &Entry {
            settings: merged.clone(),
            ..entry.clone()
        },
        cfg,
    );
    let resolved = ResolvedSettings {
        palette: overrides.pixel.map(|_| cfg.palette).unwrap_or(cfg.palette),
        ..base
    };
    let (variant_path, _hit) = pixcore::variant::generate(paths, &entry.id, &resolved, out)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("entry {} render failed", entry.id))?;
    let canvas =
        pixcore::variant::decode_any(&std::fs::read(&variant_path).map_err(anyhow::Error::from)?)
            .map_err(anyhow::Error::from)?;
    // view computed from the ORIGINAL dims; canvas stretches into view.dest
    let view = pixcore::view(
        (entry.width, entry.height),
        out,
        resolved.placement,
        resolved.gutter_px,
    );
    let final_img = pixcore::render::composite(&canvas, &view, resolved.bg_color);
    let dest = dest.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        std::path::PathBuf::from(format!(
            "pixpapers-{}-{}-{}-{}x{}.png",
            entry.id, resolved.pixel, resolved.placement, out.0, out.1
        ))
    });
    let mut buf = Vec::new();
    {
        let enc = image::codecs::png::PngEncoder::new(&mut buf);
        use image::ImageEncoder as _;
        enc.write_image(
            final_img.as_raw(),
            final_img.width(),
            final_img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(anyhow::Error::from)?;
    }
    std::fs::write(&dest, &buf).map_err(|e| anyhow::anyhow!("write {}: {e}", dest.display()))?;
    println!(
        "wrote {} · {}×{} · {} {} · {:.0} KB",
        dest.display(),
        final_img.width(),
        final_img.height(),
        resolved.pixel,
        resolved.placement,
        buf.len() as f64 / 1024.0
    );
    Ok(())
}

fn merge_settings(base: &Settings, over: &Settings) -> Settings {
    Settings {
        pixel: over.pixel.or(base.pixel),
        placement: over.placement.or(base.placement),
        bg_color: over.bg_color.or(base.bg_color),
        gutter_px: over.gutter_px.or(base.gutter_px),
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

pub fn display_settings(e: &Entry) -> (String, String) {
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
    (px, pl)
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
