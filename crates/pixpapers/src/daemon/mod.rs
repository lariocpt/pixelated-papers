//! The daemon: a single-threaded poll loop over [wayland fd, IPC socket,
//! signal wake]. The CLI owns state; the daemon re-reads it on `Apply`.

pub mod render;
pub mod spawn;
mod wayland;

use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use wayland_client::{Connection, EventQueue};

use pixcore::{Config, Paths};

use crate::ipc::{self, OutputStatus, Request, Response, StatusReport};
use wayland::Daemon;

pub fn run_foreground(paths: Paths) -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or("PIXPAPERS_LOG", "info"),
    )
    .format_timestamp_secs()
    .init();

    // ── wayland ───────────────────────────────────────────────────────
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            anyhow::bail!("no wayland session (WAYLAND_DISPLAY?): {e}");
        }
    };
    let mut queue: EventQueue<Daemon> = conn.new_event_queue();
    let qh = queue.handle();

    let mut daemon = Daemon::new(paths.clone());
    daemon.set_qh(qh.clone());

    let registry = conn.display().get_registry(&qh, ());
    let _ = registry; // events arrive via Dispatch<WlRegistry, ()>

    // ── socket (single instance) ──────────────────────────────────────
    let sock = paths.socket_file();
    prepare_socket(&sock)?;
    let listener = UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
    std::fs::write(paths.pid_file(), std::process::id().to_string())
        .context("write pidfile")?;

    // ── signals ───────────────────────────────────────────────────────
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(sig, running.clone())
            .context("register signal handler")?;
    }
    let running_fd_check = running.clone();

    log::info!("pixpapers {} daemon up (pid {}), socket {}", env!("CARGO_PKG_VERSION"), std::process::id(), sock.display());

    // Apply the current wallpaper once surfaces have configured; the
    // configure event itself triggers the first render — nothing else to do.

    let wayland_fd = conn.backend().poll_fd().as_raw_fd();
    let sock_fd = listener.as_raw_fd();

    while daemon.running && running.load(std::sync::atomic::Ordering::SeqCst) {
        if let Err(e) = conn.flush() {
            // EPIPE here means the compositor is gone (session end / niri
            // restart): clean teardown, then exit 43 (see TROUBLESHOOTING).
            log::error!("conn.flush: {e}");
            daemon.compositor_lost = true;
            daemon.running = false;
        }
        // Drain wayland events: prepare_read → poll → read → dispatch_pending.
        // This is the documented manual-loop pattern; dispatch_pending alone
        // never reads the socket, which is why globals never arrived before.
        let guard = queue.prepare_read();
        let (wl_ready, sock_ready) = poll2(wayland_fd, sock_fd, Duration::from_millis(300));
        if wl_ready {
            if let Some(g) = guard {
                if let Err(e) = g.read() {
                    use wayland_client::backend::WaylandError;
                    match e {
                        WaylandError::Io(ref io)
                            if io.kind() == std::io::ErrorKind::WouldBlock => {}
                        _ => {
                            log::error!("read_events: {e}");
                            daemon.compositor_lost = true;
                            daemon.running = false;
                        }
                    }
                }
            }
        }
        match queue.dispatch_pending(&mut daemon) {
            Ok(n) if n > 0 => log::debug!("dispatched {n} wayland events"),
            Err(wayland_client::DispatchError::Backend(e)) => {
                log::error!("dispatch backend error: {e}");
                daemon.compositor_lost = true;
                daemon.running = false;
            }
            _ => {}
        }

        // IPC: accept + serve one client per tick (commands are tiny)
        if sock_ready {
            handle_client(&listener, &mut daemon, &conn);
        }

        if !running_fd_check.load(std::sync::atomic::Ordering::SeqCst) {
            log::info!("signal received; shutting down");
            daemon.running = false;
        }
    }

    // teardown: drop outputs (Out::Drop destroys surfaces), then socket/pid
    daemon.outputs.clear();
    let _ = conn.flush();
    let _ = queue.dispatch_pending(&mut daemon);
    let _ = std::fs::remove_file(&sock);
    let _ = std::fs::remove_file(paths.pid_file());
    log::info!("bye");
    // Session gone (niri restart / logout): distinct exit so a supervisor or
    // the TROUBLESHOOTING guide can tell "killed" from "session ended".
    if daemon.compositor_lost {
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

/// Accept + serve one client (read a line, act, reply, close).
fn handle_client(listener: &UnixListener, daemon: &mut Daemon, conn: &Connection) {
    let (stream, _) = match listener.accept() {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let req: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let _ = reply(&stream, &Response::err(format!("bad request: {e}")));
            return;
        }
    };
    log::debug!("ipc: {req:?}");
    match req {
        Request::Apply => {
            let t0 = std::time::Instant::now();
            daemon.render_all(conn);
            daemon.bench.record_apply(t0.elapsed().as_millis() as u64);
            let _ = reply(&stream, &Response::ok(None));
        }
        Request::Status => {
            let _ = reply(&stream, &Response::ok(Some(status_report(daemon))));
        }
        Request::Quit => {
            let _ = reply(&stream, &Response::ok(None));
            daemon.running = false;
        }
    }
}

fn reply(stream: &std::os::unix::net::UnixStream, r: &Response) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(r).unwrap_or_default();
    bytes.push(b'\n');
    (&*stream).write_all(&bytes)
}

fn status_report(daemon: &Daemon) -> StatusReport {
    let cfg = Config::load(&daemon.paths.config_file).unwrap_or_default();
    let lib = pixcore::Library::load(&daemon.paths).ok().unwrap_or_default();
    let outputs: Vec<OutputStatus> = daemon
        .snapshot()
        .into_iter()
        .map(|(name, width, height, scale, variant)| OutputStatus {
            name,
            width,
            height,
            scale,
            variant,
        })
        .collect();
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
            applies: daemon.bench.applies,
            last_switch_ms: daemon.bench.last_switch_ms,
            switch_median_ms: daemon.bench.switch_median_ms(),
            switch_max_ms: daemon.bench.switch_max_ms(),
            render_median_ms: daemon.bench.render_median_ms(),
            render_max_ms: daemon.bench.render_max_ms(),
            variant_hits: daemon.bench.hits,
            variant_misses: daemon.bench.misses,
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
    std::fs::read_dir("/proc/self/fd").map(|rd| rd.count() as u32).unwrap_or(0)
}

/// poll(2) on exactly the two loop fds; returns per-fd POLLIN readiness.
fn poll2(fd_a: RawFd, fd_b: RawFd, timeout: Duration) -> (bool, bool) {
    let mut pfds = [
        libc::pollfd { fd: fd_a, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: fd_b, events: libc::POLLIN, revents: 0 },
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
