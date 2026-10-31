//! Command implementations that run **without** the daemon (library ops,
//! offline export, local edits) plus the policy resolver shared with the
//! daemon. All manifest/config writes are atomic (pixcore guarantees).

use std::path::Path;

use anyhow::{Context, Result, bail};
use pixcore::{
    Config, Dither, Entry, Library, PaletteMode, Paths, Pixel, Placement, ResolvedSettings, RgbCss,
    Settings,
};

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
    let buf = encode_final(&final_img, &canvas, resolved.palette, resolved.bg_color);
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
        resolved.pixel,
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
    }
}

/// The palette `encode_final` should publish for a mode: the mode's exact
/// fixed table (deduped, first occurrence — the variant encoder's rule), or
/// for `gen16` the table derived on the canvas the same way the variant
/// encoder does (`median_cut(16)` snapped to RGB555). `off`/`auto` have no
/// fixed table (`auto` would need 255 + bg, which does not fit the ≤255
/// v0.1 scheme).
fn fixed_palette(mode: PaletteMode, canvas: &image::RgbaImage) -> Option<Vec<image::Rgba<u8>>> {
    match mode {
        PaletteMode::Off | PaletteMode::Auto => None,
        pixcore::PaletteMode::Gen16 => Some(dedup_first(
            pixcore::pipeline::median_cut(canvas, 16)
                .into_iter()
                .map(pixcore::palette::snap_rgb555),
        )),
        named => Some(dedup_first(
            named
                .fixed_table()
                .expect("named mode has a table")
                .iter()
                .copied(),
        )),
    }
}

/// Encode the export's final composite as a PNG. Every quantizing mode
/// except `auto` writes a true palette PNG — the mode's exact table (for
/// `gen16`, the per-image table derived on the canvas the same way the
/// variant encoder does) plus the bg color when it is not already a table
/// entry — with the v0.1 index-0-transparent convention. `Off` stays plain
/// RGBA, and `Auto` (255 + bg would not fit the ≤255 v0.1 scheme) does too,
/// so the v0.1 export shape is preserved byte-for-byte. Encoder trouble
/// falls back to RGBA, mirroring `pixcore::variant::encode_variant`.
fn encode_final(
    img: &image::RgbaImage,
    canvas: &image::RgbaImage,
    mode: PaletteMode,
    bg: RgbCss,
) -> Vec<u8> {
    use image::ImageEncoder as _;
    let encode_rgba = || {
        let mut out = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        let _ = enc.write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        );
        out
    };
    let Some(mut pal) = fixed_palette(mode, canvas) else {
        return encode_rgba();
    };
    let bgc = image::Rgba([bg.0[0], bg.0[1], bg.0[2], 255]);
    if !pal.contains(&bgc) {
        pal.push(bgc);
    }
    if pal.len() <= 255 {
        if let Some(bytes) = encode_indexed(img, &pal) {
            return bytes;
        }
    }
    encode_rgba()
}

/// First-occurrence dedup, same semantics as the variant encoder's.
fn dedup_first(iter: impl Iterator<Item = image::Rgba<u8>>) -> Vec<image::Rgba<u8>> {
    let mut out: Vec<image::Rgba<u8>> = Vec::with_capacity(64);
    for c in iter {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Indexed PNG over `pal` with the v0.1 scheme: index 0 is the
/// transparent placeholder black, palette entries shifted +1. Pixels
/// already in the palette (quantized canvas colors, bg) map exactly; the
/// nearest fallback mirrors the variant encoder. `None` on encoder trouble.
fn encode_indexed(img: &image::RgbaImage, pal: &[image::Rgba<u8>]) -> Option<Vec<u8>> {
    let mut plte: Vec<u8> = vec![0, 0, 0];
    for c in pal {
        plte.extend_from_slice(&[c.0[0], c.0[1], c.0[2]]);
    }
    let indices: Vec<u8> = img
        .pixels()
        .map(|p| {
            if p.0[3] <= 8 {
                return 0u8;
            }
            let mut best = 1usize;
            let mut bestd = u64::MAX;
            for (i, c) in pal.iter().enumerate() {
                let dr = p.0[0] as i64 - c.0[0] as i64;
                let dg = p.0[1] as i64 - c.0[1] as i64;
                let db = p.0[2] as i64 - c.0[2] as i64;
                let d = (dr * dr + dg * dg + db * db) as u64;
                if d < bestd {
                    bestd = d;
                    best = i + 1;
                }
            }
            best as u8
        })
        .collect();
    let mut buf = Vec::new();
    let ok = {
        let mut enc = png::Encoder::new(&mut buf, img.width(), img.height());
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(plte);
        enc.set_trns(vec![0u8]); // only index 0 is transparent (v0.1 verbatim)
        match enc.write_header() {
            Ok(mut w) => w.write_image_data(&indices).is_ok(),
            Err(_) => false,
        }
    };
    if ok { Some(buf) } else { None }
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
