//! CLI ↔ daemon protocol: JSON lines over a `0600` unix socket.
//! Each client connection sends one request line, reads one reply, closes.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Client → daemon. The CLI owns ALL state (manifest/config); the daemon is
/// a pure renderer that re-reads state on `Apply` — no cross-process writes,
/// no protocol surface for partial updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    /// Re-read manifest + config and re-apply the current wallpaper.
    Apply,
    Status,
    Quit,
}

/// Daemon → client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<StatusReport>,
}

impl Response {
    pub fn ok(data: Option<StatusReport>) -> Self {
        Response {
            ok: true,
            error: None,
            data,
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Response {
            ok: false,
            error: Some(msg.into()),
            data: None,
        }
    }
}

/// Everything `status` reports; the daemon fills it, the CLI also uses it
/// for the no-daemon view.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusReport {
    pub daemon_running: bool,
    pub daemon_pid: Option<u32>,
    pub enabled: bool,
    pub default_pixel: String,
    pub default_placement: String,
    pub current: Option<String>,
    pub outputs: Vec<OutputStatus>,
    pub variants_on_disk: usize,
    pub variant_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bench: Option<BenchReport>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputStatus {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    pub variant: Option<String>,
}

/// `status --bench` payload: rolling numbers from the daemon's own render
/// path (zero sampling cost when idle) + process footprint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchReport {
    pub applies: u64,
    pub last_switch_ms: u64,
    pub switch_median_ms: u64,
    pub switch_max_ms: u64,
    pub render_median_ms: u64,
    pub render_max_ms: u64,
    pub variant_hits: u64,
    pub variant_misses: u64,
    pub rss_kb: u64,
    pub fds: u32,
}

/// Send one request and read one reply. Caller maps `io::Error` kinds
/// (NotFound/ConnectionRefused) to "daemon not running".
pub fn roundtrip(
    socket: &std::path::Path,
    req: &Request,
    timeout: Duration,
) -> std::io::Result<Response> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut line = serde_json::to_string(req).map_err(io_err)?.into_bytes();
    line.push(b'\n');
    (&stream).write_all(&line)?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut reader = BufReader::new(&stream);
    let mut resp_line = String::new();
    let _ = reader.read_line(&mut resp_line);
    if resp_line.trim().is_empty() {
        return Err(io_err("daemon closed the connection"));
    }
    serde_json::from_str(&resp_line).map_err(io_err)
}

fn io_err(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// Does a daemon appear to be listening?
pub fn daemon_alive(socket: &std::path::Path) -> bool {
    roundtrip(socket, &Request::Status, Duration::from_millis(700)).is_ok()
}
