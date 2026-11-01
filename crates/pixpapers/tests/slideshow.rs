//! End-to-end coverage for live wallpapers (`pixpapers slideshow …`): the
//! CLI mutates the `[slideshow]` config section atomically, an interval of
//! `0` is a user error (exit 1), and `status` shows the slideshow state with
//! no daemon running. These run the **built** binary through a hermetic
//! `PIXPAPERS_HOME` with both display envs cleared, so `apply_now` records
//! the change and refuses to spawn a headless daemon (see `spawn.rs`) — the
//! whole test touches only a temp dir, never a live desktop, and never sleeps
//! on a real clock.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A fresh hermetic `PIXPAPERS_HOME` with the dirs the binary expects.
struct Home {
    root: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        // The binary creates dirs on demand, but a config written by
        // `slideshow` needs its parent to exist — the same guarantee
        // `Paths::ensure_dirs` gives, which the CLI hits on first save.
        Self { root, _tmp: tmp }
    }

    /// Run the built `pixpapers` binary in this home with the compositor env
    /// stripped (headless: no daemon spawn) and a pinned, minimal env.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_pixpapers"))
            .args(args)
            .env("PIXPAPERS_HOME", &self.root)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .env_remove("PIXPAPERS_LOG")
            .output()
            .expect("run pixpapers")
    }

    fn config_text(&self) -> String {
        std::fs::read_to_string(self.root.join("config.toml")).unwrap_or_default()
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// ── config persistence ───────────────────────────────────────────────────

#[test]
fn slideshow_on_writes_enabled_section() {
    let home = Home::new();
    let o = home.run(&["slideshow", "on"]);
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        out(&o),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        out(&o).contains("slideshow: on · every 300s"),
        "default interval is 300s: {}",
        out(&o)
    );
    let cfg = home.config_text();
    assert!(cfg.contains("[slideshow]"), "missing section:\n{cfg}");
    assert!(cfg.contains("enabled = true"), "not enabled:\n{cfg}");
    assert!(cfg.contains("interval_secs = 300"), "default kept:\n{cfg}");
}

#[test]
fn slideshow_on_with_interval_sets_both() {
    let home = Home::new();
    let o = home.run(&["slideshow", "on", "120"]);
    assert!(o.status.success());
    assert!(
        out(&o).contains("slideshow: on · every 120s"),
        "{}",
        out(&o)
    );
    let cfg = home.config_text();
    assert!(cfg.contains("enabled = true") && cfg.contains("interval_secs = 120"));
}

#[test]
fn slideshow_interval_keeps_power_state() {
    let home = Home::new();
    // On with the default, then change only the interval.
    assert!(home.run(&["slideshow", "on"]).status.success());
    let o = home.run(&["slideshow", "interval", "45"]);
    assert!(o.status.success());
    assert!(
        out(&o).contains("slideshow interval: 45s (on)"),
        "{}",
        out(&o)
    );
    let cfg = home.config_text();
    assert!(cfg.contains("enabled = true"), "stayed on:\n{cfg}");
    assert!(cfg.contains("interval_secs = 45"), "interval set:\n{cfg}");
}

#[test]
fn slideshow_off_disables_but_keeps_interval() {
    let home = Home::new();
    assert!(home.run(&["slideshow", "on", "90"]).status.success());
    let o = home.run(&["slideshow", "off"]);
    assert!(o.status.success());
    assert!(out(&o).contains("slideshow: off"), "{}", out(&o));
    let cfg = home.config_text();
    assert!(cfg.contains("enabled = false"), "disabled:\n{cfg}");
    // The interval the user chose survives the off toggle (not reset).
    assert!(cfg.contains("interval_secs = 90"), "interval kept:\n{cfg}");
}

// ── user errors (exit 1, not 2) ──────────────────────────────────────────

#[test]
fn interval_zero_is_a_user_error_exit_1() {
    let home = Home::new();
    assert!(home.run(&["slideshow", "on"]).status.success());
    let o = home.run(&["slideshow", "interval", "0"]);
    assert_eq!(o.status.code(), Some(1), "must be exit 1 (user error)");
    // No write on rejection: the previous interval is untouched.
    assert!(home.config_text().contains("interval_secs = 300"));
}

#[test]
fn on_with_zero_interval_is_a_user_error_exit_1() {
    let home = Home::new();
    let o = home.run(&["slideshow", "on", "0"]);
    assert_eq!(o.status.code(), Some(1), "must be exit 1 (user error)");
}

// ── status reporting without a daemon ────────────────────────────────────

#[test]
fn status_shows_slideshow_when_on_and_daemon_stopped() {
    let home = Home::new();
    assert!(home.run(&["slideshow", "on", "120"]).status.success());
    // No daemon here (display env stripped), so local_report fills enabled
    // from config and leaves next_change None → "daemon stopped".
    let o = home.run(&["status"]);
    assert!(o.status.success(), "{}", out(&o));
    let text = out(&o);
    assert!(
        text.contains("slideshow: on · daemon stopped"),
        "expected a slideshow line:\n{text}"
    );
}

#[test]
fn status_shows_no_slideshow_line_when_off() {
    let home = Home::new();
    // Default config: slideshow off. The line must not appear at all.
    let o = home.run(&["status"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(
        !out(&o).contains("slideshow:"),
        "off must stay silent:\n{}",
        out(&o)
    );
}

#[test]
fn status_json_reports_slideshow_fields() {
    let home = Home::new();
    assert!(home.run(&["slideshow", "on", "120"]).status.success());
    let o = home.run(&["status", "--json"]);
    assert!(o.status.success(), "{}", out(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).expect("status --json must parse");
    assert_eq!(v["slideshow_enabled"], serde_json::json!(true));
    // Daemon down → no next-change promise in the payload.
    assert!(
        v.get("next_change").is_none(),
        "next_change must be absent when off the live path: {v}"
    );
}

// ── backward-compatible default config (feature off) ─────────────────────

#[test]
fn fresh_home_config_defaults_to_off_and_loads() {
    let home = Home::new();
    // A benign command that forces the binary to materialize a config; then
    // assert the shipped default is disabled with the round 300s interval.
    assert!(home.run(&["status"]).status.success());
    let cfg = home.config_text();
    // Even if `[slideshow]` is written, it must default off at 300s.
    if cfg.contains("[slideshow]") {
        assert!(cfg.contains("enabled = false"), "default off:\n{cfg}");
        assert!(cfg.contains("interval_secs = 300"), "default 300:\n{cfg}");
    }
    // Sanity: a bare list must still work with no entries and no daemon.
    let l = home.run(&["list"]);
    assert!(l.status.success(), "{}", out(&l));
}
