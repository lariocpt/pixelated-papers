//! End-to-end coverage for the media-key aliases (`next`/`prev`/`play`/
//! `pause`) and the read-only "track N of M" status fields. The aliases are
//! the transport-agnostic control surface a media-key daemon (sxhkd, a niri
//! `bind`, keyboardctl) drives in place of a D-Bus MPRIS service.
//!
//! The load-bearing guarantee is byte-identity of the resulting **state**:
//! `next` is `cycle 1`, `prev` is `cycle -1`, `play` is `slideshow on`,
//! `pause` is `slideshow off` — the same `commands::` + `apply_now` seam, so
//! the manifest/config the CLI owns must come out *byte-for-byte identical*
//! run from the same starting state. These run the **built** binary through a
//! hermetic `PIXPAPERS_HOME` with both display envs cleared, so `apply_now`
//! records the change and refuses to spawn a headless daemon — the whole test
//! touches only a temp dir, never a live desktop, and never sleeps on a clock.

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
        // `set`/`cycle`/`next` save through `Paths::ensure_dirs`, but a
        // hand-written seed manifest needs its parents to exist first.
        std::fs::create_dir_all(root.join("data").join("library")).expect("data dir");
        std::fs::create_dir_all(root.join("state")).expect("state dir");
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

    fn manifest_bytes(&self) -> Vec<u8> {
        std::fs::read(self.root.join("data").join("library.json")).unwrap_or_default()
    }

    fn current_id(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.root.join("data").join("library.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.get("current")?.as_str().map(|s| s.to_string())
    }

    /// Seed a 3-entry library (ids `aaaa`,`bbbb`,`cccc`) with `current` set to
    /// the given id. Written as compact JSON; the CLI rewrites it canonically
    /// on the first mutating command, so both homes converge on identical
    /// bytes regardless of the seed's own formatting.
    fn seed_library(&self, current: &str) {
        let entry = |id: &str| {
            "{\"id\":\"@\",\"title\":\"wall @\",\"hash\":\"h@\",\"ext\":\"png\",\"added_unix\":1,\"width\":8,\"height\":8,\"original_bytes\":70,\"optimize_note\":\"none\",\"settings\":{}}"
                .replace('@', id)
        };
        let entries = [entry("aaaa"), entry("bbbb"), entry("cccc")].join(",");
        let json = format!(r#"{{"version":1,"current":"{current}","entries":[{entries}]}}"#);
        std::fs::write(self.root.join("data").join("library.json"), json).expect("seed manifest");
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

// ── next / prev are byte-identical to cycle ±1 ────────────────────────────

#[test]
fn next_is_byte_identical_to_cycle_one() {
    let a = Home::new();
    let b = Home::new();
    a.seed_library("aaaa");
    b.seed_library("aaaa");

    assert!(a.run(&["cycle"]).status.success()); // default +1
    assert!(b.run(&["next"]).status.success());

    assert_eq!(a.current_id().as_deref(), Some("bbbb"));
    assert_eq!(b.current_id().as_deref(), Some("bbbb"));
    assert_eq!(
        a.manifest_bytes(),
        b.manifest_bytes(),
        "`next` must leave a manifest byte-identical to `cycle 1`"
    );
}

#[test]
fn prev_is_byte_identical_to_cycle_minus_one() {
    let a = Home::new();
    let b = Home::new();
    a.seed_library("bbbb");
    b.seed_library("bbbb");

    assert!(a.run(&["cycle", "-1"]).status.success());
    assert!(b.run(&["prev"]).status.success());

    assert_eq!(a.current_id().as_deref(), Some("aaaa"));
    assert_eq!(b.current_id().as_deref(), Some("aaaa"));
    assert_eq!(a.manifest_bytes(), b.manifest_bytes());
}

#[test]
fn prev_wraps_like_cycle_from_the_first_entry() {
    // `prev` off the first entry wraps to the last, exactly as `cycle -1`
    // (the documented modulo), never to a stuck state.
    let home = Home::new();
    home.seed_library("aaaa");
    assert!(home.run(&["prev"]).status.success());
    assert_eq!(home.current_id().as_deref(), Some("cccc"));
}

#[test]
fn next_wraps_like_cycle_from_the_last_entry() {
    let home = Home::new();
    home.seed_library("cccc");
    assert!(home.run(&["next"]).status.success());
    assert_eq!(home.current_id().as_deref(), Some("aaaa"));
}

// ── play / pause are byte-identical to slideshow on/off ───────────────────

#[test]
fn play_is_byte_identical_to_slideshow_on() {
    let a = Home::new();
    let b = Home::new();
    assert!(a.run(&["slideshow", "on"]).status.success());
    assert!(b.run(&["play"]).status.success());

    let cfg_a = a.config_text();
    assert!(cfg_a.contains("enabled = true"), "on:\n{cfg_a}");
    assert!(
        cfg_a.contains("interval_secs = 300"),
        "default interval kept:\n{cfg_a}"
    );
    assert_eq!(
        a.config_text(),
        b.config_text(),
        "`play` must leave config byte-identical to `slideshow on`"
    );
}

#[test]
fn pause_is_byte_identical_to_slideshow_off() {
    let a = Home::new();
    let b = Home::new();
    // Start both from the same "on" state, then turn off via different verbs.
    assert!(a.run(&["slideshow", "on", "120"]).status.success());
    assert!(b.run(&["slideshow", "on", "120"]).status.success());
    assert!(a.run(&["slideshow", "off"]).status.success());
    assert!(b.run(&["pause"]).status.success());

    let cfg_a = a.config_text();
    assert!(cfg_a.contains("enabled = false"), "off:\n{cfg_a}");
    assert_eq!(
        a.config_text(),
        b.config_text(),
        "`pause` must leave config byte-identical to `slideshow off`"
    );
}

#[test]
fn play_pause_only_flips_the_enabled_flag() {
    // The slideshow time model is a pure function of the wall clock — there is
    // no "resume epoch" anywhere. A full play→pause→play cycle must only ever
    // change the `enabled` boolean in `[slideshow]`, never add a timestamp,
    // and Play-after-Pause must resume by the clock (nothing to reset).
    let home = Home::new();
    home.seed_library("aaaa");
    for _ in 0..2 {
        assert!(home.run(&["play"]).status.success());
        assert!(home.run(&["pause"]).status.success());
    }
    assert!(home.run(&["play"]).status.success());

    let cfg = home.config_text();
    assert!(cfg.contains("enabled = true"), "ends on:\n{cfg}");
    // No epoch/resume/start field leaked into the config by toggling.
    for forbidden in ["epoch", "resumed", "started", "started_at", "last_change"] {
        assert!(
            !cfg.contains(forbidden),
            "slideshow config must not carry a {forbidden:?} field (pure-clock model):\n{cfg}"
        );
    }
    // Only the known `[slideshow]` keys survive — enabled + interval_secs.
    let section = cfg
        .split("[slideshow]")
        .nth(1)
        .expect("[slideshow] section present");
    for line in section.lines().map(str::trim).filter(|l| l.contains('=')) {
        let key = line.split('=').next().unwrap().trim();
        assert!(
            key == "enabled" || key == "interval_secs",
            "unexpected [slideshow] key {key:?}:\n{cfg}"
        );
    }
}

// ── read-only status fields (track N of M) ────────────────────────────────

#[test]
fn status_json_reports_position_and_count() {
    let home = Home::new();
    home.seed_library("bbbb"); // 1-based position 2 of 3 → 0-based index 1
    let o = home.run(&["status", "--json"]);
    assert!(o.status.success(), "{}", stdout(&o));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("status --json parses");
    assert_eq!(v["current_index"], serde_json::json!(1), "{v}");
    assert_eq!(v["library_len"], serde_json::json!(3), "{v}");
}

#[test]
fn status_json_position_absent_when_nothing_set() {
    // Degraded/no-selection state: `library_len` still reports the count, but
    // `current_index` is omitted entirely (not 0) — a new client reads `None`.
    let home = Home::new();
    home.seed_library("aaaa");
    // `set` a nonexistent id would error; instead clear current by cycling on a
    // single-entry view is overkill — simplest: write a current-less manifest.
    std::fs::write(
        home.root.join("data").join("library.json"),
        r#"{"version":1,"entries":[{"id":"aaaa","title":"wall aaaa","hash":"haaaa","ext":"png","added_unix":1,"width":8,"height":8,"original_bytes":70,"optimize_note":"none","settings":{}}]}"#,
    )
    .expect("no-current manifest");
    let o = home.run(&["status", "--json"]);
    assert!(o.status.success(), "{}", stdout(&o));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("status --json parses");
    assert!(
        v.get("current_index").is_none(),
        "current_index must be absent with no current entry: {v}"
    );
    assert_eq!(v["library_len"], serde_json::json!(1), "{v}");
}

// ── degraded/empty state behaves exactly like the underlying commands ──────

#[test]
fn next_on_empty_library_matches_cycle() {
    // Empty library: `next` must fail the same way `cycle` does — same exit
    // code, same error text — because it shares `commands::cycle_current`.
    let a = Home::new();
    let b = Home::new();
    let ca = a.run(&["cycle"]);
    let cb = b.run(&["next"]);
    assert!(
        !ca.status.success(),
        "cycle on empty library must fail: {}",
        stdout(&ca)
    );
    assert_eq!(
        ca.status.code(),
        cb.status.code(),
        "next and cycle must share the empty-library exit code"
    );
    assert_eq!(
        stderr(&ca).trim(),
        stderr(&cb).trim(),
        "next and cycle must report the empty-library error identically"
    );
}
