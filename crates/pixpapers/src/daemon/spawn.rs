//! Client-side auto-spawn: `set`/`cycle`/knobs bring the daemon up when it
//! isn't running (best effort; commands still succeed with a clear note).

use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::Duration;

use pixcore::Paths;

use crate::ipc;

/// Ensure a daemon is listening; spawn one if not. Returns whether one is
/// alive now.
pub fn ensure_running(paths: &Paths) -> bool {
    let sock = paths.socket_file();
    if ipc::daemon_alive(&sock) {
        return true;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        log_note("WAYLAND_DISPLAY is not set — cannot start the daemon headlessly");
        return false;
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return false,
    };
    let log = match OpenOptions::new().create(true).append(true).open(paths.log_file()) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap_or_else(|_| {
            OpenOptions::new().create(true).append(true).open(paths.log_file()).expect("log")
        })))
        .stderr(Stdio::from(log))
        .process_group(0); // detach into its own session leader group
    match cmd.spawn() {
        Ok(_child) => {}
        Err(_) => return false,
    }
    // give it a moment to bind
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(50));
        if ipc::daemon_alive(&sock) {
            return true;
        }
    }
    false
}

fn log_note(msg: &str) {
    eprintln!("note: {msg}");
}
