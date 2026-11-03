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
    /// When the current wallpaper uses an `auto` / `auto-content` pixel, the
    /// concrete preset the daemon resolves it to at its primary output — e.g.
    /// `auto→16 @1920x1080` or `auto-content→64 @1920x1080`. This names a
    /// concrete preset (never a bare `auto`), matching the cache filenames.
    /// Additive like `backend`: a pre-feature daemon omits it and an old
    /// client ignores it; a new client reading an old report sees `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_auto_detail: Option<String>,
    /// Position of the current wallpaper within the ordered library
    /// (0-based), and the library length — the read-only "track N of M"
    /// metadata a media widget shows alongside the title. Pure observation
    /// (the daemon re-reads the manifest to render anyway), so it keeps the
    /// daemon a pure renderer. Additive like `slideshow_enabled`: a pre-feature
    /// report omits both keys (byte-identical), an old client ignores them, and
    /// a new client reading an old report sees `None`. `current_index` is
    /// `None` when no wallpaper is set; `library_len` is the entry count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_len: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputStatus {
    pub name: String,
    /// Buffer (physical) dimensions the daemon renders at.
    pub width: u32,
    pub height: u32,
    /// Legacy integer `wl_output.scale` as the compositor reported it.
    pub scale: u32,
    pub variant: Option<String>,
    /// How the buffer size was negotiated: `"exact"` = wp_viewporter +
    /// wp_fractional_scale_v1 engaged, buffer is the physical pixel grid
    /// (width×height above is the panel's own grid); `"legacy"` = integer
    /// oversampling (buffer = logical × `scale`). Additive, skip-if-absent
    /// (the media-keys pattern): the x11 backend omits it, a pre-feature
    /// daemon omits it, an old client ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_path: Option<String>,
    /// The compositor-negotiated fractional scale as a fixed two-decimal
    /// string (`"1.50"`), present only once a `preferred_scale` arrived.
    /// Same additive contract as `scale_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frac_scale: Option<String>,
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
    //! The slideshow + content-auto status fields are additive JSON-lines
    //! changes: a report without them (a pre-feature daemon) must serialize to
    //! the exact old shape and a new client must read an old report as `None`.
    //! This is the wire-contract guarantee — no version bump, old peers
    //! unaffected.
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
            current_auto_detail: None,
            current_index: None,
            library_len: None,
        }
    }

    #[test]
    fn slideshow_fields_are_skipped_when_absent() {
        // Feature off / pre-feature daemon: neither key is emitted, so the
        // JSON bytes are identical to the shape before the feature existed.
        let json = serde_json::to_string(&base_report()).unwrap();
        assert!(!json.contains("slideshow_enabled"), "{json}");
        assert!(!json.contains("next_change"), "{json}");
        assert!(!json.contains("current_auto_detail"), "{json}");
        assert!(!json.contains("current_index"), "{json}");
        assert!(!json.contains("library_len"), "{json}");
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
        assert_eq!(r.current_auto_detail, None);
        assert_eq!(r.current_index, None);
        assert_eq!(r.library_len, None);
    }

    #[test]
    fn new_report_shape_round_trips() {
        let mut r = base_report();
        r.slideshow_enabled = Some(true);
        r.next_change = Some(1_700_000_000);
        r.current_auto_detail = Some("auto-content→64 @1920x1080".into());
        r.current_index = Some(2);
        r.library_len = Some(7);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"slideshow_enabled\":true"), "{json}");
        assert!(json.contains("\"next_change\":1700000000"), "{json}");
        assert!(json.contains("auto-content"), "{json}");
        assert!(json.contains("\"current_index\":2"), "{json}");
        assert!(json.contains("\"library_len\":7"), "{json}");
        let back: StatusReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slideshow_enabled, Some(true));
        assert_eq!(back.next_change, Some(1_700_000_000));
        assert_eq!(
            back.current_auto_detail.as_deref(),
            Some("auto-content→64 @1920x1080")
        );
        assert_eq!(back.current_index, Some(2));
        assert_eq!(back.library_len, Some(7));
    }

    // ── exact fractional buffers: the per-output additive fields ──

    fn base_output() -> OutputStatus {
        OutputStatus {
            name: "eDP-1".into(),
            width: 3840,
            height: 2160,
            scale: 2,
            variant: Some("16/fit-auto".into()),
            scale_path: None,
            frac_scale: None,
        }
    }

    #[test]
    fn output_scale_fields_are_skipped_when_absent() {
        // x11 backend / pre-feature daemon: neither key is emitted, so the
        // output object serializes to the exact pre-feature shape.
        let json = serde_json::to_string(&base_output()).unwrap();
        assert!(!json.contains("scale_path"), "{json}");
        assert!(!json.contains("frac_scale"), "{json}");
    }

    #[test]
    fn old_output_shape_deserializes_to_none() {
        // A pre-feature status line must load with both fields None —
        // the skip-if-absent contract a new client relies on.
        let old = r#"{"name":"eDP-1","width":3840,"height":2160,"scale":2,"variant":null}"#;
        let o: OutputStatus = serde_json::from_str(old).unwrap();
        assert_eq!(o.scale_path, None);
        assert_eq!(o.frac_scale, None);
    }

    #[test]
    fn exact_path_output_round_trips() {
        // The container-proof shape: exact path at 1.5 names the path and
        // the negotiated scale next to the physical buffer dims.
        let mut o = base_output();
        o.width = 2880;
        o.height = 1620;
        o.scale_path = Some("exact".into());
        o.frac_scale = Some("1.50".into());
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains("\"scale_path\":\"exact\""), "{json}");
        assert!(json.contains("\"frac_scale\":\"1.50\""), "{json}");
        let back: OutputStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scale_path.as_deref(), Some("exact"));
        assert_eq!(back.frac_scale.as_deref(), Some("1.50"));
        assert_eq!((back.width, back.height), (2880, 1620));
    }
}
