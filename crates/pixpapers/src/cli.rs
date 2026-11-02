//! clap surface + command routing. All state mutations happen locally
//! (atomic writes); a live daemon is then told to re-apply.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_complete::Shell;

use pixcore::{
    Config, Dither, GutterStyle, PaletteMode, Paths, Pixel, Placement, RgbCss, Settings,
};

use crate::ipc::{self, Request};
#[cfg(feature = "tui")]
use crate::tui;
use crate::{commands, daemon, status};

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
    /// Set the pixel look for one wallpaper: 8|16|32|64|128|normal|auto
    Pixel { id: String, pixel: Pixel },
    /// Set placement for one wallpaper: fit|cover-gutter|cover-crop|stretch
    Placement { id: String, placement: Placement },
    /// Set border/gutter color (#rrggbb) for one wallpaper
    Bg { id: String, color: RgbCss },
    /// Set gutter width in px (cover-gutter) for one wallpaper
    Gutter { id: String, px: u32 },
    /// Set the palette look for one wallpaper: off|auto|nes|gb|cga|ega|vga|gen16
    Palette { id: String, palette: PaletteMode },
    /// Set the dither for one wallpaper: none|fs|atkinson|bayer2|bayer4|bayer8
    Dither { id: String, dither: Dither },
    /// Set the cover-gutter ring style for one wallpaper: solid|bleed
    GutterStyle { id: String, style: GutterStyle },
    /// Change defaults for new wallpapers / unset entries
    Default {
        #[command(subcommand)]
        what: DefaultWhat,
    },
    /// Timed slideshow through the library (live wallpapers). The daemon
    /// picks the wallpaper from the wall clock, so it survives a restart.
    Slideshow {
        #[command(subcommand)]
        what: SlideshowWhat,
    },
    /// Force pixelation on for everything (default)
    Enable,
    /// Turn pixelation off (originals only, placement unchanged)
    Disable,
    /// Rotate to the next wallpaper (by N, default 1; negative = back)
    Cycle {
        #[arg(allow_negative_numbers = true)]
        by: Option<i64>,
    },
    /// Daemon status
    Status {
        #[arg(long)]
        json: bool,
        /// Live render timings, variant cache stats, RSS/fd (R8 proof)
        #[arg(long)]
        bench: bool,
    },
    /// One-line status for toolbar modules (contrib/waybar)
    Bar,
    /// Offline render to a PNG (no daemon needed): e.g. 1920x1080
    Export {
        id: String,
        size: String,
        /// Pixel preset for this render only (not persisted); `auto`
        /// picks from the output size
        #[arg(long)]
        pixel: Option<Pixel>,
        #[arg(long)]
        placement: Option<Placement>,
        #[arg(long)]
        bg: Option<RgbCss>,
        #[arg(long)]
        gutter: Option<u32>,
        /// Quantization look for this render only (not persisted)
        #[arg(long)]
        palette: Option<PaletteMode>,
        /// Dither for this render only (not persisted)
        #[arg(long)]
        dither: Option<Dither>,
        /// Gutter ring style for this render only (not persisted)
        #[arg(long)]
        gutter_style: Option<GutterStyle>,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Re-run original optimization over the library
    Optimize {
        #[arg(long)]
        all: bool,
    },
    /// Run the wallpaper daemon (foreground)
    Daemon {
        /// Display backend: layer (wayland) or x11
        #[arg(long, value_name = "BACKEND")]
        backend: Option<String>,
    },
    /// Ask a running daemon to stop
    Stop,
    /// Interactive terminal front-end (library browser + offline preview)
    #[cfg(feature = "tui")]
    Tui,
    /// Native control window (library browser + set-now + per-image knobs)
    #[cfg(feature = "gui")]
    Gui,
    /// Emit a shell completion script
    Completions { shell: Shell },
}

#[derive(Subcommand)]
enum DefaultWhat {
    Pixel { pixel: Pixel },
    Placement { placement: Placement },
    Palette { palette: PaletteMode },
    Dither { dither: Dither },
    GutterStyle { style: GutterStyle },
}

#[derive(Subcommand)]
enum SlideshowWhat {
    /// Start cycling the library (optionally set the seconds per wallpaper
    /// in the same call, e.g. `slideshow on 120`)
    On {
        /// Seconds each wallpaper shows (omit to keep the current interval)
        #[arg(value_name = "SECONDS")]
        interval: Option<u64>,
    },
    /// Stop cycling — show the manually set/cycled wallpaper again
    Off,
    /// Change how long each wallpaper shows (seconds), keeping on/off as-is
    Interval { seconds: u64 },
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
        Cmd::Palette { id, palette } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.palette = Some(palette))?;
            println!("{} → palette {palette}{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::Dither { id, dither } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.dither = Some(dither))?;
            println!("{} → dither {dither}{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::GutterStyle { id, style } => {
            let (e, cur) = commands::edit_entry(paths, &id, |s| s.gutter_style = Some(style))?;
            println!("{} → gutter-style {style}{}", e.id, changed_note(cur));
            if cur {
                apply_now(paths);
            }
        }
        Cmd::Default { what } => {
            let mut cfg = cfg.clone();
            match what {
                DefaultWhat::Pixel { pixel } => {
                    commands::set_default_pixel(paths, &mut cfg, pixel)?
                }
                DefaultWhat::Placement { placement } => {
                    commands::set_default_placement(paths, &mut cfg, placement)?
                }
                DefaultWhat::Palette { palette } => {
                    commands::set_default_palette(paths, &mut cfg, palette)?
                }
                DefaultWhat::Dither { dither } => {
                    commands::set_default_dither(paths, &mut cfg, dither)?
                }
                DefaultWhat::GutterStyle { style } => {
                    commands::set_default_gutter_style(paths, &mut cfg, style)?
                }
            }
            apply_now(paths);
        }
        Cmd::Slideshow { what } => {
            let mut cfg = cfg.clone();
            let action = match what {
                SlideshowWhat::On { interval } => commands::SlideshowAction::On { interval },
                SlideshowWhat::Off => commands::SlideshowAction::Off,
                SlideshowWhat::Interval { seconds } => {
                    commands::SlideshowAction::Interval { secs: seconds }
                }
            };
            commands::set_slideshow(paths, &mut cfg, action)?;
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
            println!(
                "cycle → {} · {} ({})",
                entry.id,
                entry.title,
                state_line(paths)
            );
        }
        Cmd::Status { json, bench } => {
            if bench {
                status::status_bench(paths, cfg, json)
            } else {
                status::status(paths, cfg, json)
            }
        }
        Cmd::Bar => commands::bar(paths, cfg)?,
        Cmd::Export {
            id,
            size,
            pixel,
            placement,
            bg,
            gutter,
            palette,
            dither,
            gutter_style,
            out,
        } => {
            let (w, h) = parse_size(&size)?;
            let overrides = Settings {
                pixel,
                placement,
                bg_color: bg,
                gutter_px: gutter,
                palette,
                dither,
                gutter_style,
            };
            commands::export(paths, cfg, &id, &overrides, (w, h), out.as_deref())?;
        }
        Cmd::Optimize { all } => {
            if all {
                commands::optimize_all(paths, cfg)?;
            } else {
                println!("use --all (whole-library re-optimize is the only mode)");
            }
        }
        Cmd::Daemon { backend } => daemon::run_foreground(paths.clone(), cfg, backend.as_deref())?,
        Cmd::Stop => status::stop(paths)?,
        #[cfg(feature = "tui")]
        Cmd::Tui => tui::run(paths, cfg)?,
        #[cfg(feature = "gui")]
        Cmd::Gui => crate::gui::run(paths, cfg)?,
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
        .ok_or_else(|| commands::UserError("size must be <W>x<H>, e.g. 1920x1080".into()))?;
    Ok((
        w.trim()
            .parse()
            .map_err(|_| commands::UserError(format!("bad width in {s:?}")))?,
        h.trim()
            .parse()
            .map_err(|_| commands::UserError(format!("bad height in {s:?}")))?,
    ))
}

/// Push an Apply to a live daemon (no spawn — knob edits shouldn't wake it).
fn notify_daemon(paths: &Paths) {
    let _ = ipc::roundtrip(
        &paths.socket_file(),
        &Request::Apply,
        std::time::Duration::from_millis(900),
    );
}

/// Apply now, auto-spawning the daemon when needed. `pub(crate)`: the
/// TUI (S7) calls the *same* seam — the client never reimplements
/// spawn/notify (PLAN-30 invariant #1).
pub(crate) fn apply_now(paths: &Paths) {
    if daemon::spawn::ensure_running(paths) {
        let _ = ipc::roundtrip(
            &paths.socket_file(),
            &Request::Apply,
            std::time::Duration::from_millis(2500),
        );
    }
}

/// `pub(crate)` for the TUI's set/cycle status lines (same wording as the
/// CLI output).
pub(crate) fn state_line(paths: &Paths) -> &'static str {
    if ipc::daemon_alive(&paths.socket_file()) {
        "applied"
    } else {
        "recorded — will show when the daemon runs"
    }
}

fn changed_note(was_current: bool) -> String {
    if was_current {
        ""
    } else {
        " (not current — will apply on set)"
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    //! S9 task 7: unparseable sizes are user errors (exit 1, was 2).

    use super::parse_size;
    use crate::commands::is_user_error;

    #[test]
    fn bad_size_is_user_error() {
        for bad in ["1920", "x1080", "1920x", "axb", "12x7x9"] {
            let err = parse_size(bad).expect_err(&format!("{bad:?} must not parse"));
            assert!(
                is_user_error(&err),
                "{bad:?} must classify as user error (exit 1)"
            );
        }
        assert_eq!(parse_size("1920x1080").unwrap(), (1920, 1080));
        assert_eq!(parse_size(" 1920 x 1080 ").unwrap(), (1920, 1080));
    }
}
