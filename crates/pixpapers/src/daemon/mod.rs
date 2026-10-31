//! The daemon: a single-threaded poll loop over [backend fd, IPC socket,
//! signal wake]. The CLI owns state; the daemon re-reads it on `Apply`.
//! The loop is backend-agnostic (PLAN-30 §B.1): it drives the active
//! display backend through `Backend` — pump, poll, pump, render, teardown,
//! exit 43. The apply-latency bench and the status plumbing stay with the
//! concrete backend for now (one backend exists; S5's status rework
//! generalizes them).
//!
//! `pump()` runs once *before* the poll as well as after it: the pre-poll
//! pump flushes the requests the previous tick queued (binds, commits),
//! so the compositor's answer arrives within this tick's poll — the exact
//! ordering of the pre-seam loop, which the cold-start handshake
//! (registry → globals → binds → configure) depends on. The pre-poll read
//! only drains data already waiting; whatever arrives during the poll is
//! read and dispatched by the post-poll pump. Same messages, same order,
//! same tick.

pub mod backend;
pub mod render;
pub mod spawn;

use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use pixcore::{Config, Paths};

use crate::ipc::{self, Request, Response, StatusReport};

use backend::Backend;
use backend::layer::LayerBackend;

/// SIGTERM/SIGINT flip this; the loop checks it every tick and tears down
/// cleanly. Raw handler: nothing else in the signal path can hang.
static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

unsafe extern "C" fn handle_shutdown(_sig: libc::c_int) {
    SHUTDOWN.store(false, std::sync::atomic::Ordering::SeqCst);
}

pub fn run_foreground(paths: Paths) -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().filter_or("PIXPAPERS_LOG", "info"))
        .format_timestamp_secs()
        .init();

    // ── backend ───────────────────────────────────────────────────────
    let mut backend = backend::layer::LayerBackend::connect_to_env(paths.clone())?;

    // ── socket (single instance) ──────────────────────────────────────
    let sock = paths.socket_file();
    prepare_socket(&sock)?;
    let listener = UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
    std::fs::write(paths.pid_file(), std::process::id().to_string()).context("write pidfile")?;

    // ── signals ───────────────────────────────────────────────────────
    // Raw sigaction(2): the handler is a single async-signal-safe atomic
    // store, and it *provably runs* on this machine — signal-hook's
    // flag::register installed handlers that silently never fired here
    // (SIGTERM-ignored daemon; see VERIFICATION.md V17).
    for sig in [libc::SIGTERM, libc::SIGINT] {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            let handler: unsafe extern "C" fn(libc::c_int) = handle_shutdown;
            sa.sa_sigaction = handler as usize;
            sa.sa_flags = 0; // empty mask; poll returns EINTR → loop re-checks
            if libc::sigaction(sig, &sa, std::ptr::null_mut()) != 0 {
                anyhow::bail!(
                    "sigaction({sig}) failed: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }

    log::info!(
        "pixpapers {} daemon up (pid {}), socket {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        sock.display()
    );

    // Apply the current wallpaper once surfaces have configured; the
    // configure event itself triggers the first render — nothing else to do.

    let sock_fd = listener.as_raw_fd();
    // The loop ends on Quit, display loss or signal; the backend only
    // reports the display side of that.
    let mut quit = false;

    while !quit && !backend.connection_lost() && SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst)
    {
        // Pre-poll pump: flush requests queued by the previous tick's
        // dispatch/handle_client so the compositor answers within this
        // tick's poll (pre-seam ordering; see the module note).
        backend.pump();
        // poll over [backend fd, socket fd]; a backend without a display
        // fd is polled on the socket alone. The backend fd in the poll set
        // wakes the loop when the connection has events.
        let sock_ready = match backend.poll_fd() {
            Some(fd) => poll2(fd, sock_fd, Duration::from_millis(300)).1,
            None => poll2(-1, sock_fd, Duration::from_millis(300)).1,
        };
        // Post-poll pump: drain the connection — flush, prepare_read →
        // read → dispatch_pending, the documented manual-loop pattern, now
        // owned by the backend. dispatch_pending alone never reads the
        // socket, which is why globals never arrived before.
        backend.pump();

        // IPC: accept + serve one client per tick (commands are tiny)
        if sock_ready && handle_client(&listener, &mut backend, &paths) {
            quit = true;
        }

        if !SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst) {
            log::info!("signal received; shutting down");
            quit = true;
        }
    }

    // teardown: backend first (destroys surfaces, drains the connection),
    // then socket/pid
    backend.close();
    let _ = std::fs::remove_file(&sock);
    let _ = std::fs::remove_file(paths.pid_file());
    log::info!("bye");
    // Session gone (niri restart / logout): distinct exit so a supervisor or
    // the TROUBLESHOOTING guide can tell "killed" from "session ended".
    if backend.connection_lost() {
        std::process::exit(43);
    }
    Ok(())
}

/// Stale-socket-safe bind prep: if a live daemon owns the socket, error;
/// else unlink the corpse.
fn prepare_socket(sock: &Path) -> Result<()> {
    if sock.exists() {
        if ipc::daemon_alive(sock) {
            anyhow::bail!("a pixpapers daemon is already running ({})", sock.display());
        }
        std::fs::remove_file(sock).ok();
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    Ok(())
}

/// Accept + serve one client (read a line, act, reply, close). Returns
/// true when the client asked the daemon to quit.
fn handle_client(listener: &UnixListener, backend: &mut LayerBackend, paths: &Paths) -> bool {
    let (stream, _) = match listener.accept() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => {
            // EOF or timeout with no bytes: answer rather than close silently,
            // so a client always gets a reply (never waits out its timeout).
            let _ = reply(&stream, &Response::err("empty request"));
            return false;
        }
        Ok(_) => {}
    }
    let req: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let _ = reply(&stream, &Response::err(format!("bad request: {e}")));
            return false;
        }
    };
    log::debug!("ipc: {req:?}");
    match req {
        Request::Apply => {
            let t0 = std::time::Instant::now();
            backend.render_all(paths);
            // The bench lives with the backend that owns the render path
            // (S4: one backend; S5's status rework moves this behind the
            // trait).
            backend
                .state
                .bench
                .record_apply(t0.elapsed().as_millis() as u64);
            let _ = reply(&stream, &Response::ok(None));
            false
        }
        Request::Status => {
            let _ = reply(&stream, &Response::ok(Some(status_report(backend, paths))));
            false
        }
        Request::Quit => {
            let _ = reply(&stream, &Response::ok(None));
            true
        }
    }
}

fn reply(stream: &std::os::unix::net::UnixStream, r: &Response) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(r).unwrap_or_default();
    bytes.push(b'\n');
    (&*stream).write_all(&bytes)
}

fn status_report(backend: &LayerBackend, paths: &Paths) -> StatusReport {
    let cfg = Config::load(&paths.config_file).unwrap_or_default();
    let lib = pixcore::Library::load(paths).ok().unwrap_or_default();
    let outputs = backend.snapshot();
    StatusReport {
        daemon_running: true,
        daemon_pid: Some(std::process::id()),
        enabled: cfg.enabled,
        default_pixel: cfg.default_pixel.to_string(),
        default_placement: cfg.default_placement.to_string(),
        current: lib.current,
        outputs,
        variants_on_disk: 0, // cheap scan done in status.rs against shared FS
        variant_bytes: 0,
        bench: Some(crate::ipc::BenchReport {
            applies: backend.state.bench.applies,
            last_switch_ms: backend.state.bench.last_switch_ms,
            switch_median_ms: backend.state.bench.switch_median_ms(),
            switch_max_ms: backend.state.bench.switch_max_ms(),
            render_median_ms: backend.state.bench.render_median_ms(),
            render_max_ms: backend.state.bench.render_max_ms(),
            variant_hits: backend.state.bench.hits,
            variant_misses: backend.state.bench.misses,
            rss_kb: self_rss_kb(),
            fds: self_fds(),
        }),
    }
}

/// VmRSS in kB from /proc/self/status (Linux; 0 elsewhere).
fn self_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

/// Open fd count from /proc/self/fd.
fn self_fds() -> u32 {
    std::fs::read_dir("/proc/self/fd")
        .map(|rd| rd.count() as u32)
        .unwrap_or(0)
}

/// poll(2) on exactly the two loop fds; returns per-fd POLLIN readiness.
fn poll2(fd_a: RawFd, fd_b: RawFd, timeout: Duration) -> (bool, bool) {
    let mut pfds = [
        libc::pollfd {
            fd: fd_a,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: fd_b,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let rc = unsafe { libc::poll(pfds.as_mut_ptr(), 2, ms) };
    if rc <= 0 {
        return (false, false);
    }
    (
        pfds[0].revents & libc::POLLIN != 0,
        pfds[1].revents & libc::POLLIN != 0,
    )
}
