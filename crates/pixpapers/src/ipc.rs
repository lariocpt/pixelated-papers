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
    /// Active display backend ("layer" | "x11"); None when no daemon
    /// answered. Additive (§B.3): old clients ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    pub variants_on_disk: usize,
    pub variant_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bench: Option<BenchReport>,
    /// Whether the timed slideshow is on (live wallpapers). Additive, like
    /// `backend`: `skip_serializing_if` keeps a pre-feature report byte-identical
    /// and lets an old client ignore it; a new client reading an old report
    /// sees `None` (nothing to show).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slideshow_enabled: Option<bool>,
    /// Unix second of the next clock-driven change, present only when the
    /// slideshow is on. `None` when off or when the interval is degenerate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_change: Option<u64>,
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

#[cfg(test)]
mod tests {
    //! The slideshow status fields are an additive JSON-lines change: a
    //! report without them (a pre-feature daemon, or one with the feature
    //! off) must serialize to the exact old shape and a new client must read
    //! an old report as `None`. This is the wire-contract guarantee for live
    //! wallpapers — no version bump, old peers unaffected.
    use super::*;

    fn base_report() -> StatusReport {
        StatusReport {
            daemon_running: true,
            daemon_pid: Some(1),
            enabled: true,
            default_pixel: "16".into(),
            default_placement: "fit".into(),
            current: Some("abc".into()),
            outputs: vec![],
            backend: Some("layer".into()),
            variants_on_disk: 0,
            variant_bytes: 0,
            bench: None,
            slideshow_enabled: None,
            next_change: None,
        }
    }

    #[test]
    fn slideshow_fields_are_skipped_when_absent() {
        // Feature off / pre-feature daemon: neither key is emitted, so the
        // JSON bytes are identical to the shape before the feature existed.
        let json = serde_json::to_string(&base_report()).unwrap();
        assert!(!json.contains("slideshow_enabled"), "{json}");
        assert!(!json.contains("next_change"), "{json}");
    }

    #[test]
    fn old_report_shape_deserializes_to_none() {
        // Hand-written pre-feature JSON (no slideshow keys, `backend` present
        // as it was already additive) must load with both new fields None.
        let old = r#"{"daemon_running":true,"daemon_pid":null,"enabled":true,
            "default_pixel":"16","default_placement":"fit","current":null,
            "outputs":[],"variants_on_disk":0,"variant_bytes":0}"#;
        let r: StatusReport = serde_json::from_str(old).unwrap();
        assert_eq!(r.slideshow_enabled, None);
        assert_eq!(r.next_change, None);
    }

    #[test]
    fn new_report_shape_round_trips() {
        let mut r = base_report();
        r.slideshow_enabled = Some(true);
        r.next_change = Some(1_700_000_000);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"slideshow_enabled\":true"), "{json}");
        assert!(json.contains("\"next_change\":1700000000"), "{json}");
        let back: StatusReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slideshow_enabled, Some(true));
        assert_eq!(back.next_change, Some(1_700_000_000));
    }
}
