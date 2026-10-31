//! The backend seam (PLAN-30 §B.1). The supervisor (`super`) owns the loop
//! shape — poll over [backend fd, IPC socket] + SIGTERM/INT, IPC, teardown,
//! exit 43 — and drives the active display backend through this trait.
//! Deliberately small: a backend knows how pixels reach its display server;
//! the supervisor already knows the loop shape.

use std::os::fd::RawFd;

use pixcore::Paths;

use crate::ipc::OutputStatus;

pub mod layer;
#[cfg(feature = "x11")]
pub mod x11;

/// Rolling render/apply counters, owned by whichever backend renders
/// (S4 left them behind `LayerBackend`; §B.3's status rework moves them
/// behind the trait — both backends report through this shape).
#[derive(Clone, Copy, Debug, Default)]
pub struct BenchCounters {
    pub applies: u64,
    pub last_switch_ms: u64,
    pub switch_median_ms: u64,
    pub switch_max_ms: u64,
    pub render_median_ms: u64,
    pub render_max_ms: u64,
    pub variant_hits: u64,
    pub variant_misses: u64,
}

/// One display backend. The daemon renders on exactly one at a time.
pub trait Backend {
    /// Stable backend name; reported in `status` (§B.3).
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

    /// Apply/render counters for `status --bench` (see [`BenchCounters`]).
    fn bench_counters(&self) -> BenchCounters;

    /// The display server went away (compositor restart / session end).
    /// The supervisor tears down and exits 43 — never restart-on-43.
    fn connection_lost(&self) -> bool;

    /// Teardown: destroy surfaces before the supervisor removes the socket
    /// and pidfile.
    fn close(&mut self);
}
