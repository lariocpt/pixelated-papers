//! `pixpapers status` — asks the daemon for truth, falls back to the
//! filesystem when it's unreachable.

use pixcore::{Config, Library, Paths};

use crate::ipc::{self, Request, StatusReport};

pub fn local_report(paths: &Paths, cfg: &Config) -> StatusReport {
    let lib = Library::load(paths).ok().unwrap_or_default();
    let (variant_bytes, variants_on_disk) = scan_variants(paths, &lib);
    StatusReport {
        daemon_running: false,
        daemon_pid: read_pid(paths),
        enabled: cfg.enabled,
        default_pixel: cfg.default_pixel.to_string(),
        default_placement: cfg.default_placement.to_string(),
        current: lib.current,
        outputs: vec![],
        variants_on_disk,
        variant_bytes,
        bench: None,
    }
}

pub fn print_report(r: &StatusReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(r).unwrap());
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
    println!("daemon:   {state} (pid {pid})");
    println!(
        "policy:   pixelation {} · default {} / {}",
        if r.enabled {
            "on"
        } else {
            "OFF (originals only)"
        },
        r.default_pixel,
        r.default_placement
    );
    println!("current:  {}", r.current.as_deref().unwrap_or("(none)"));
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
            println!(
                "  {:<14} {}x{} scale {}   {}",
                o.name,
                o.width,
                o.height,
                o.scale,
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
            print_report(resp.data.as_ref().unwrap(), json)
        }
        _ => print_report(&local_report(paths, cfg), json),
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
                print_report(&r, false);
            }
            r
        }
        _ => {
            let r = local_report(paths, cfg);
            if !json {
                print_report(&r, false);
            }
            r
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&r).unwrap());
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
