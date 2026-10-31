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

use anyhow::Result;
use pixcore::{Config, Paths};

/// Run the TUI. Feature-gated (`tui`): a slim build has no `tui` subcommand.
pub fn run(_paths: &Paths, _cfg: &Config) -> Result<()> {
    // S6 lands this over the layout/preview commits; the subcommand and
    // feature wiring exist now (PLAN-30 §7: shared with S7).
    anyhow::bail!("tui: not implemented yet (S6)");
}
