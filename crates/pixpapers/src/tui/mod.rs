//! `pixpapers tui` — an interactive terminal front-end to the *same* state
//! the CLI owns (PLAN-30 §C.1–C.3): library list on the left, an offline
//! rendered preview on the right, a status bar for daemon aliveness and the
//! terminal image-protocol detection result.
//!
//! The TUI is a *client* exactly like the CLI: in this step (S6) it only
//! previews offline from the library — no state mutation happens here (the
//! action keys are S7, and they no-op with a `(S7)` status note until then).
//! A dead daemon is fine: the preview is offline and the status bar says so.
//!
//! Terminal model (C.1): the TUI owns its own single-threaded loop —
//! crossterm events in, one ratatui frame out. No event-loop crate, no
//! tokio (the daemon's posture, kept). Alternate screen + raw mode are
//! armed on entry and restored on exit by a `Drop` guard (panic-safe).

mod detect;
mod draw;
mod emit;
#[cfg(test)]
mod goldens;

use std::io;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use pixcore::{Config, Library, Paths};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub use detect::ImageProto;

/// The preview pane's image state (C.3). `Idle` = nothing rendered yet
/// (or the selection changed); the pane then shows "rendering preview …"
/// while the blocking render runs, or the fallback text in a
/// non-image terminal.
pub enum PreviewState {
    Idle,
    Ready {
        png: Vec<u8>,
    },
    /// Encoded preview exceeded the 256 KB inline budget (C.3 guard).
    Refused {
        kb: u32,
    },
    /// Render failed (missing original, decode trouble, pane too small).
    Failed(String),
}

/// Restore the terminal no matter how we leave the loop — normal exit,
/// `?`-propagated error, or panic on the way out (house rule: panic-safe).
struct TerminalGuard;

impl TerminalGuard {
    fn arm() -> Result<Self> {
        enable_raw_mode().context("enable raw mode")?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen).context("enter alternate screen")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        // Best-effort: if the terminal is gone there is nothing to fix,
        // and Drop must never panic on top of the original error.
        let _ = execute!(out, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

/// Everything the frame renders from. Pure state: no threads, no timers.
pub struct App {
    pub paths: Paths,
    pub cfg: Config,
    pub lib: Library,
    /// Index into `lib.entries` (the selection cursor).
    pub sel: usize,
    /// Last action result — one line, most recent wins (C.2).
    pub last: String,
    /// Socket probe result from startup / `r` refresh.
    pub daemon_up: bool,
    /// Image-protocol detection (C.3) — startup only, shown as `img:`.
    pub proto: ImageProto,
    pub preview: PreviewState,
    /// Set on selection change / refresh / resize: the next loop pass
    /// re-renders the preview (never on a timer).
    preview_dirty: bool,
    /// The Ready preview still needs its bytes emitted once.
    emit_pending: bool,
    /// A kitty image is currently displayed (needs `a=d` before replace).
    image_on_screen: bool,
    /// Monotonic kitty image id; `prev` is deleted before each new one.
    kitty_id: u32,
    prev_kitty_id: u32,
}

impl App {
    fn new(paths: Paths, cfg: Config) -> Result<Self> {
        let lib = Library::load(&paths).context("load library")?;
        let mut app = Self {
            paths,
            cfg,
            lib,
            sel: 0,
            last: "ready".into(),
            daemon_up: false,
            proto: detect::detect(),
            preview: PreviewState::Idle,
            preview_dirty: true,
            emit_pending: false,
            image_on_screen: false,
            kitty_id: 0,
            prev_kitty_id: 0,
        };
        app.refresh_state()?;
        // Open on the current entry when there is one.
        if let Some(cur) = app.lib.current.as_deref()
            && let Some(i) = app.lib.entries.iter().position(|e| e.id == cur)
        {
            app.sel = i;
        }
        Ok(app)
    }

    pub fn selected(&self) -> Option<&pixcore::Entry> {
        self.lib.entries.get(self.sel)
    }

    fn move_sel(&mut self, delta: i64) {
        let n = self.lib.entries.len() as i64;
        if n == 0 {
            return;
        }
        let cur = self.sel as i64;
        let next = (((cur + delta).rem_euclid(n)) as usize).min(n as usize - 1);
        if next != self.sel {
            self.sel = next;
            // The old preview belongs to the old entry: drop it and mark
            // dirty so the loop clears the screen image and re-renders.
            self.preview = PreviewState::Idle;
            self.preview_dirty = true;
            self.emit_pending = false;
        }
    }

    /// Re-read manifest + config and re-probe the daemon (`r` refresh —
    /// the one C.4 row that is not an action: no mutation, no IPC).
    fn refresh_state(&mut self) -> Result<()> {
        self.lib = Library::load(&self.paths).context("load library")?;
        self.cfg = Config::load(&self.paths.config_file).unwrap_or_else(|_| self.cfg.clone());
        self.daemon_up = crate::ipc::daemon_alive(&self.paths.socket_file());
        if self.sel >= self.lib.entries.len() {
            self.sel = self.lib.entries.len().saturating_sub(1);
        }
        self.preview = PreviewState::Idle;
        self.preview_dirty = true;
        self.emit_pending = false;
        Ok(())
    }
}

/// The C.4 action keys, unwired: S7 replaces these with the real
/// `commands::*` calls + Apply. Until then they are honest no-ops that
/// say so — the status bar prints `(S7)` (task contract).
fn s7_note(key: char) -> String {
    format!("(S7) '{key}' — actions land in S7")
}

/// The blocking preview render (C.3): literally a small export — the
/// same `render_offline` the CLI export runs, at the pane canvas. Only
/// called when the terminal can display an image (proto ≠ None).
fn render_preview(app: &mut App, canvas: (u32, u32)) {
    let Some(entry) = app.selected() else {
        app.preview = PreviewState::Idle;
        return;
    };
    let r = crate::commands::resolve_effective(entry, &app.cfg);
    match pixcore::variant::render_offline(
        &app.paths,
        &entry.id,
        (entry.width, entry.height),
        &r,
        canvas,
    ) {
        Ok((_, png)) => {
            let kb = png.len().div_ceil(1024) as u32;
            if pixcore::preview::png_over_budget(&png) {
                app.preview = PreviewState::Refused { kb };
                app.last = format!("preview too large — {kb} KB, see pane");
            } else {
                app.last = format!(
                    "preview {}×{} · {kb} KB · {}",
                    canvas.0,
                    canvas.1,
                    app.proto.token()
                );
                app.preview = PreviewState::Ready { png };
            }
        }
        Err(e) => {
            app.preview = PreviewState::Failed(e.to_string());
            app.last = format!("preview failed: {e}");
        }
    }
}

pub fn run(paths: &Paths, cfg: &Config) -> Result<()> {
    let mut app = App::new(paths.clone(), cfg.clone())?;
    let _guard = TerminalGuard::arm()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::new(backend).context("init terminal")?;
    let mut sink = emit::Sink::Live;

    loop {
        // ratatui 0.30 starts every draw with an absolute MoveTo, so
        // emitting image sequences between frames cannot desync it.
        let mut geom = draw::Geom::default();
        term.draw(|f| geom = draw::frame(f, &app))
            .context("draw frame")?;

        // (Re)generate the preview when dirty — selection, refresh,
        // resize. Never on a timer. The render can take seconds (the
        // `normal` preset quantizes a full-size photo), so the status
        // bar is redrawn with "rendering preview …" *before* blocking.
        if app.preview_dirty && app.proto != ImageProto::None && app.selected().is_some() {
            app.preview_dirty = false;
            if geom.cols == 0 || geom.rows == 0 {
                app.preview = PreviewState::Failed("pane too small for a preview".into());
            } else {
                if app.image_on_screen {
                    emit::clear_prev(&mut sink, app.proto, app.prev_kitty_id);
                    app.image_on_screen = false;
                    app.prev_kitty_id = 0;
                }
                app.last = "rendering preview …".into();
                term.draw(|f| {
                    draw::frame(f, &app);
                })?;
                render_preview(&mut app, geom.canvas);
                app.emit_pending = matches!(app.preview, PreviewState::Ready { .. });
                term.draw(|f| {
                    draw::frame(f, &app);
                })?;
            }
        }

        // Emit the fresh image once, over the flushed frame, anchored to
        // the pane area the renderer just reported.
        if app.emit_pending {
            app.emit_pending = false;
            if let PreviewState::Ready { png, .. } = &app.preview {
                emit::clear_prev(&mut sink, app.proto, app.prev_kitty_id);
                emit::move_to(&mut sink, geom.anchor.0, geom.anchor.1);
                app.kitty_id += 1;
                emit::emit(
                    &mut sink,
                    app.proto,
                    png,
                    app.kitty_id,
                    geom.cols,
                    geom.rows,
                );
                app.prev_kitty_id = app.kitty_id;
                app.image_on_screen = true;
            }
        }

        let ev = event::read().context("read event")?;
        match ev {
            // crossterm fires Press/Release/Repeat on some terminals; act once.
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if ctrl => break,
                    KeyCode::Down | KeyCode::Char('j') => app.move_sel(1),
                    KeyCode::Up | KeyCode::Char('k') => app.move_sel(-1),
                    KeyCode::Char('r') => {
                        app.refresh_state()?;
                        app.last = "refreshed".into();
                    }
                    KeyCode::Char('l') if ctrl => {
                        // Ctrl-L: repaint (no state change — the terminal
                        // may have smeared; a full redraw fixes it). The
                        // displayed image is untouched by a text repaint.
                        term.clear()?;
                    }
                    // C.4 action keys, unwired (plain `l` is the placement
                    // cycle — Ctrl-L repaints above and never reaches here).
                    KeyCode::Char(
                        c @ ('a' | 's' | 'c' | 'p' | 'l' | 'b' | 'g' | 't' | 'd' | 'e' | 'x'),
                    ) => {
                        app.last = s7_note(c);
                    }
                    // `rm` is two keys; S7 decides the binding (r alone is
                    // refresh). Until then `m` is the remove no-op marker.
                    KeyCode::Char('m') => app.last = s7_note('m'),
                    _ => {}
                }
            }
            // Resize → relayout from terminal.size() and re-render the
            // preview at the new canvas (C.1/C.3).
            Event::Resize(_, _) => {
                let size = term.size()?;
                term.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height))?;
                app.preview = PreviewState::Idle;
                app.preview_dirty = true;
                app.emit_pending = false;
            }
            _ => {}
        }
    }
    Ok(())
}
