//! The backend seam (PLAN-30 §B.1). The supervisor (`super`) owns the loop
//! shape — poll over [backend fd, IPC socket] + SIGTERM/INT, IPC, teardown,
//! exit 43 — and drives the active display backend through this trait.
//! Deliberately small: a backend knows how pixels reach its display server;
//! the supervisor already knows the loop shape.

use std::os::fd::RawFd;

use pixcore::Paths;

use crate::ipc::OutputStatus;

pub mod layer;

/// One display backend. The daemon renders on exactly one at a time.
pub trait Backend {
    /// Stable backend name. Unused until §B.3 (S5) reports it in
    /// `status`; the trait keeps its full shape so the seam doesn't move
    /// again.
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// The fd the supervisor polls for connection events. `None` ⇒ the
    /// supervisor polls only the IPC socket.
    fn poll_fd(&self) -> Option<RawFd>;

    /// Drain the connection: flush outgoing, read events, dispatch them to
    /// the backend's state. The supervisor calls this every tick, ready or
    /// not — the read must be non-blocking, so a would-block (fd not ready)
    /// is a no-op, exactly like the old wayland-only loop.
    fn pump(&mut self);

    /// (Re)render the current wallpaper on every output.
    fn render_all(&mut self, paths: &Paths);

    /// Per-output state; feeds `StatusReport.outputs`.
    fn snapshot(&self) -> Vec<OutputStatus>;

    /// The display server went away (compositor restart / session end).
    /// The supervisor tears down and exits 43 — never restart-on-43.
    fn connection_lost(&self) -> bool;

    /// Teardown: destroy surfaces before the supervisor removes the socket
    /// and pidfile.
    fn close(&mut self);
}
