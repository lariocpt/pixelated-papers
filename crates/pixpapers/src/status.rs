//! `pixpapers status` — asks the daemon for truth, falls back to the
//! filesystem when it's unreachable.

use pixcore::{Config, Library, Paths};

use crate::ipc::{self, Request, StatusReport};

pub fn local_report(paths: &Paths, cfg: &Config) -> StatusReport {
    let lib = Library::load(paths).ok().unwrap_or_default();
    let (variant_bytes, variants_on_disk) = scan_variants(paths, &lib);
    // Read-only "track N of M" position (media-widget metadata). The index is
    // the current entry's slot in the ordered library; `None` when nothing is
    // set. Computed before `current` is moved into the report below.
    let current_index = lib
        .current
        .as_deref()
        .and_then(|c| lib.entries.iter().position(|e| e.id == c));
    let library_len = lib.entries.len();
    StatusReport {
        daemon_running: false,
        daemon_pid: read_pid(paths),
        enabled: cfg.enabled,
        default_pixel: cfg.default_pixel.to_string(),
        default_placement: cfg.default_placement.to_string(),
        current: lib.current,
        outputs: vec![],
        backend: None,
        variants_on_disk,
        variant_bytes,
        bench: None,
        // A stopped daemon schedules nothing, so `next_change` stays `None`;
        // the enabled flag still reflects the config the user set.
        slideshow_enabled: Some(cfg.slideshow.enabled),
        next_change: None,
        // Without a daemon there is no output size to resolve `auto` /
        // `auto-content` against, so the concrete-preset detail stays absent.
        current_auto_detail: None,
        current_index,
        library_len: Some(library_len),
    }
}

/// The report as JSON, with the config's palette/dither defaults merged in
/// CLI-side — the daemon's report shape stays untouched (it predates the
/// v0.2 knobs), same enrichment pattern as the local variant scan in
/// `status_bench`.
fn report_value(r: &StatusReport, cfg: &Config) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("default_palette".into(), serde_json::json!(cfg.palette));
        obj.insert("default_dither".into(), serde_json::json!(cfg.dither));
    }
    v
}

pub fn print_report(r: &StatusReport, cfg: &Config, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report_value(r, cfg)).unwrap()
        );
        return;
    }
    let pid = r
        .daemon_pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".into());
    let state = if r.daemon_running {
        "running"
    } else {
        "not running"
    };
    match &r.backend {
        Some(b) => println!("daemon:   {state} (pid {pid}, backend {b})"),
        None => println!("daemon:   {state} (pid {pid})"),
    }
    println!(
        "policy:   pixelation {} · default {} / {} · palette {} · dither {}",
        if r.enabled {
            "on"
        } else {
            "OFF (originals only)"
        },
        r.default_pixel,
        r.default_placement,
        cfg.palette,
        cfg.dither
    );
    println!("current:  {}", r.current.as_deref().unwrap_or("(none)"));
    // "track N of M" position — only when a current entry sits in the library
    // (media-widget metadata). 1-based for humans; the JSON stays 0-based.
    if let (Some(idx), Some(len)) = (r.current_index, r.library_len)
        && len > 0
    {
        println!("position: {} / {}", idx + 1, len);
    }
    // When the current wallpaper resolves an `auto`/`auto-content` pixel, name
    // the concrete preset the daemon renders (only present with a live daemon).
    if let Some(d) = &r.current_auto_detail {
        println!("  look:     {d}");
    }
    if r.slideshow_enabled == Some(true) {
        // Only shown when the user turned the slideshow on, so the default
        // (off) status output is unchanged.
        match r.next_change {
            Some(next) => {
                let secs = next.saturating_sub(pixcore::slideshow::now_unix());
                println!("slideshow: on · next change in {secs}s");
            }
            None => println!("slideshow: on · daemon stopped"),
        }
    }
    if r.outputs.is_empty() {
        println!(
            "outputs:  {}",
            if r.daemon_running {
                "none connected"
            } else {
                "start the daemon to see outputs"
            }
        );
    } else {
        for o in &r.outputs {
            // The scale column names the negotiation only when the exact
            // fractional path is live ("scale 1.50 exact"); every other
            // case (legacy integer, x11, pre-feature daemon) prints the
            // plain integer exactly as before.
            let scale = match (&o.scale_path, &o.frac_scale) {
                (Some(p), Some(f)) if p == "exact" => format!("{f} exact"),
                _ => format!("{}", o.scale),
            };
            println!(
                "  {:<14} {}x{} scale {scale}   {}",
                o.name,
                o.width,
                o.height,
                o.variant.as_deref().unwrap_or("(empty)")
            );
        }
    }
    let kb = r.variant_bytes as f64 / 1024.0;
    println!(
        "cache:    {} variant file(s), {:.0} KB",
        r.variants_on_disk, kb
    );
}

/// `status --bench` — the R8 numbers, straight from the daemon's own renders.
pub fn print_bench(r: &StatusReport, json: bool) {
    let Some(b) = &r.bench else {
        println!("bench:    unavailable (start the daemon; these are live-render numbers)");
        return;
    };
    if json {
        println!("{}", serde_json::to_string_pretty(b).unwrap());
        return;
    }
    let total = b.variant_hits + b.variant_misses;
    let hit_pct = if total > 0 {
        b.variant_hits as f64 * 100.0 / total as f64
    } else {
        0.0
    };
    println!(
        "bench (daemon self-timed, last {} applies / {} renders):",
        b.applies, total
    );
    println!(
        "  switch:   last {} ms · median {} ms · max {} ms",
        b.last_switch_ms, b.switch_median_ms, b.switch_max_ms
    );
    println!(
        "  render:   median {} ms · max {} ms (per output, composite→buffer)",
        b.render_median_ms, b.render_max_ms
    );
    println!(
        "  variants: {} hits / {} misses ({:.0}% cache), {:.0} KB on disk",
        b.variant_hits,
        b.variant_misses,
        hit_pct,
        r.variant_bytes as f64 / 1024.0
    );
    println!("  footprint: {} KB RSS · {} fds", b.rss_kb, b.fds);
}

/// Try the daemon, fall back to local.
pub fn status(paths: &Paths, cfg: &Config, json: bool) {
    match ipc::roundtrip(
        &paths.socket_file(),
        &Request::Status,
        std::time::Duration::from_millis(800),
    ) {
        Ok(resp) if resp.ok && resp.data.is_some() => {
            print_report(resp.data.as_ref().unwrap(), cfg, json)
        }
        _ => print_report(&local_report(paths, cfg), cfg, json),
    }
}

/// `status --bench`: ask the daemon for its self-timed numbers, enrich with a
/// cheap local variant scan (filesystem is shared truth), and print.
pub fn status_bench(paths: &Paths, cfg: &Config, json: bool) {
    let r = match ipc::roundtrip(
        &paths.socket_file(),
        &Request::Status,
        std::time::Duration::from_millis(800),
    ) {
        Ok(resp) if resp.ok && resp.data.is_some() => {
            let mut r = resp.data.unwrap();
            // daemon leaves the cache scan to us (cheap, and it is the truth)
            let lib = Library::load(paths).ok().unwrap_or_default();
            let (bytes, count) = scan_variants(paths, &lib);
            r.variant_bytes = bytes;
            r.variants_on_disk = count;
            if !json {
                print_report(&r, cfg, false);
            }
            r
        }
        _ => {
            let r = local_report(paths, cfg);
            if !json {
                print_report(&r, cfg, false);
            }
            r
        }
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report_value(&r, cfg)).unwrap()
        );
        return;
    }
    if r.bench.is_none() {
        println!(
            "bench:    unavailable — not running under `pixpapers daemon` (no live renders to time)"
        );
    }
    print_bench(&r, false);
}

/// `pixpapers stop` — ask the daemon to quit.
pub fn stop(paths: &Paths) -> anyhow::Result<()> {
    match ipc::roundtrip(
        &paths.socket_file(),
        &Request::Quit,
        std::time::Duration::from_millis(800),
    ) {
        Ok(_) => {
            println!("daemon stopped");
            Ok(())
        }
        Err(_) => {
            println!("no running daemon");
            Ok(())
        }
    }
}

fn read_pid(paths: &Paths) -> Option<u32> {
    std::fs::read_to_string(paths.pid_file())
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub(crate) fn scan_variants(paths: &Paths, lib: &Library) -> (u64, usize) {
    let (mut bytes, mut count) = (0u64, 0usize);
    for e in &lib.entries {
        let dir = paths.variants_dir(&e.id);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for f in rd.flatten() {
            if let Ok(m) = f.metadata() {
                count += 1;
                bytes += m.len();
            }
        }
    }
    (bytes, count)
}
