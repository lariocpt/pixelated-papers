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

mod draw;

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
        self.sel = (((cur + delta).rem_euclid(n)) as usize).min(n as usize - 1);
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
        Ok(())
    }
}

/// The C.4 action keys, unwired: S7 replaces these with the real
/// `commands::*` calls + Apply. Until then they are honest no-ops that
/// say so — the status bar prints `(S7)` (task contract).
fn s7_note(key: char) -> String {
    format!("(S7) '{key}' — actions land in S7")
}

pub fn run(paths: &Paths, cfg: &Config) -> Result<()> {
    let mut app = App::new(paths.clone(), cfg.clone())?;
    let _guard = TerminalGuard::arm()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::new(backend).context("init terminal")?;

    loop {
        // ratatui 0.30 starts every draw with an absolute MoveTo, so
        // emitting image sequences between frames (C3) cannot desync it.
        term.draw(|f| draw::frame(f, &app)).context("draw frame")?;

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
                        // may have smeared; a full redraw fixes it).
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
            // preview (C.1). The next draw recomputes everything from
            // `f.area()`, which ratatui refreshes from the backend size.
            Event::Resize(_, _) => {
                let size = term.size()?;
                term.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height))?;
            }
            _ => {}
        }
    }
    Ok(())
}
