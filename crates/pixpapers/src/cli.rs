//! clap surface + command routing. All state mutations happen locally
//! (atomic writes); a live daemon is then told to re-apply.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_complete::Shell;

use pixcore::{Config, Paths, Placement, Pixel, RgbCss, Settings};

use crate::{commands, daemon, status};
use crate::ipc::{self, Request};

#[derive(Parser)]
#[command(name = "pixpapers", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add wallpaper(s) to the library (original stored optimized)
    Add {
        path: PathBuf,
        #[arg(long)]
        title: Option<String>,
    },
    /// List the library
    List,
    /// Remove a wallpaper (entry + variants)
    Remove { id: String },
    /// Apply a wallpaper now (starts the daemon if needed)
    Set { id: String },
    /// Set the pixel look for one wallpaper: 8|16|32|64|128|normal
    Pixel { id: String, pixel: Pixel },
    /// Set placement for one wallpaper: fit|cover-gutter|cover-crop|stretch
    Placement { id: String, placement: Placement },
    /// Set border/gutter color (#rrggbb) for one wallpaper
    Bg { id: String, color: RgbCss },
    /// Set gutter width in px (cover-gutter) for one wallpaper
    Gutter { id: String, px: u32 },
    /// Change defaults for new wallpapers / unset entries
    Default {
        #[command(subcommand)]
        what: DefaultWhat,
    },
    /// Force pixelation on for everything (default)
    Enable,
    /// Turn pixelation off (originals only, placement unchanged)
    Disable,
    /// Rotate to the next wallpaper (by N, default 1)
    Cycle { by: Option<i64> },
    /// Daemon status
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Offline render to a PNG (no daemon needed): e.g. 1920x1080
    Export {
        id: String,
        size: String,
        #[arg(long)]
        pixel: Option<Pixel>,
        #[arg(long)]
        placement: Option<Placement>,
        #[arg(long)]
        bg: Option<RgbCss>,
        #[arg(long)]
        gutter: Option<u32>,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Re-run original optimization over the library
    Optimize {
        #[arg(long)]
        all: bool,
    },
    /// Run the wallpaper daemon (foreground)
    Daemon,
    /// Ask a running daemon to stop
    Stop,
    /// Emit a shell completion script
    Completions { shell: Shell },
}

#[derive(Subcommand)]
enum DefaultWhat {
    Pixel { pixel: Pixel },
    Placement { placement: Placement },
}

pub fn run(cmd: Cli, paths: &Paths, cfg: &Config) -> Result<()> {
    match cmd.cmd {
        Cmd::Add { path, title } => {
            commands::add(paths, cfg, &path, title)?;
        }
        Cmd::List => commands::list(paths)?,
        Cmd::Remove { id } => {
            commands::remove(paths, &id)?;
            notify_daemon(paths);
        }
        Cmd::Set { id } => {
            let entry = commands::set_current(paths, &id)?;
            apply_now(paths);
            println!("set {} · {} ({})", entry.id, entry.title, state_line(paths));
        }
        Cmd::Pixel { id, pixel } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.pixel = Some(pixel))?;
            println!("{} → pixel {pixel}{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::Placement { id, placement } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.placement = Some(placement))?;
            println!("{} → placement {placement}{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::Bg { id, color } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.bg_color = Some(color))?;
            println!("{} → bg {color}{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::Gutter { id, px } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.gutter_px = Some(px))?;
            println!("{} → gutter {px}px{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::Default { what } => {
            let mut cfg = cfg.clone();
            match what {
                DefaultWhat::Pixel { pixel } => commands::set_default_pixel(paths, &mut cfg, pixel)?,
                DefaultWhat::Placement { placement } => {
                    commands::set_default_placement(paths, &mut cfg, placement)?
                }
            }
            apply_now(paths);
        }
        Cmd::Enable | Cmd::Disable => {
            let mut cfg = cfg.clone();
            commands::set_enabled(paths, &mut cfg, matches!(cmd.cmd, Cmd::Enable))?;
            apply_now(paths);
        }
        Cmd::Cycle { by } => {
            let entry = commands::cycle_current(paths, by.unwrap_or(1))?;
            apply_now(paths);
            println!("cycle → {} · {} ({})", entry.id, entry.title, state_line(paths));
        }
        Cmd::Status { json } => status::status(paths, cfg, json),
        Cmd::Export { id, size, pixel, placement, bg, gutter, out } => {
            let (w, h) = parse_size(&size)?;
            let overrides = Settings { pixel, placement, bg_color: bg, gutter_px: gutter };
            commands::export(paths, cfg, &id, &overrides, (w, h), out.as_deref())?;
        }
        Cmd::Optimize { all } => {
            if all {
                commands::optimize_all(paths, cfg)?;
            } else {
                println!("use --all (whole-library re-optimize is the only mode)");
            }
        }
        Cmd::Daemon => daemon::run_foreground(paths.clone())?,
        Cmd::Stop => status::stop(paths)?,
        Cmd::Completions { shell } => {
            let mut app = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut app, "pixpapers", &mut std::io::stdout());
        }
    }
    Ok(())
}

fn parse_size(s: &str) -> Result<(u32, u32)> {
    let lower = s.to_ascii_lowercase();
    let (w, h) = lower
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("size must be <W>x<H>, e.g. 1920x1080"))?;
    Ok((
        w.trim().parse().map_err(|_| anyhow::anyhow!("bad width in {s:?}"))?,
        h.trim().parse().map_err(|_| anyhow::anyhow!("bad height in {s:?}"))?,
    ))
}

/// Push an Apply to a live daemon (no spawn — knob edits shouldn't wake it).
fn notify_daemon(paths: &Paths) {
    let _ = ipc::roundtrip(&paths.socket_file(), &Request::Apply, std::time::Duration::from_millis(900));
}

/// Apply now, auto-spawning the daemon when needed.
fn apply_now(paths: &Paths) {
    if daemon::spawn::ensure_running(paths) {
        let _ = ipc::roundtrip(&paths.socket_file(), &Request::Apply, std::time::Duration::from_millis(2500));
    }
}

fn state_line(paths: &Paths) -> &'static str {
    if ipc::daemon_alive(&paths.socket_file()) {
        "applied"
    } else {
        "recorded — will show when the daemon runs"
    }
}

fn changed_note(was_current: bool) -> String {
    if was_current { "" } else { " (not current — will apply on set)" }.to_string()
}
