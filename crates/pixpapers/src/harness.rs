//! In-process test harness for the CLI: a throwaway unix listener standing
//! in for the daemon, a hermetic `PIXPAPERS_HOME`, and subprocess
//! round-trips. S7's TUI tests reuse [`DaemonHarness`] the same way.
//!
//! Every subprocess runs with `PIXPAPERS_HOME` pointed at a fresh tempdir
//! and both `WAYLAND_DISPLAY` and `DISPLAY` removed, so the CLI can never
//! auto-spawn a real daemon (S5 made `DISPLAY` sufficient for the x11
//! backend); the only socket traffic it can produce goes to our listener.

use std::collections::HashSet;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use pixcore::{Dither, PaletteMode, Paths};

use crate::ipc::Request;

/// The repo's shared test fixture (`crates/pixpapers` → `test/fixture.png`).
///
/// That path is outside the crate, so a published crate tarball does not
/// carry it — it ships its own copy at `tests/fixtures/fixture.png`
/// (kept byte-identical by `bundled_fixture_matches_repo_fixture`).
/// Falling back to it is what lets the CLI goldens run inside an unpacked
/// `.crate`; in a workspace checkout the shared file always wins, so the
/// fixture every script and golden uses stays the single canonical one.
pub(crate) fn fixture() -> PathBuf {
    let shared = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixture.png");
    if shared.exists() {
        return shared;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixture.png")
}

/// True outside a full workspace checkout — i.e. inside a published crate
/// tarball, where the workspace `pixcore` sources are gone (it comes from
/// the registry instead) and the repo-level `test/` tree is not shipped.
/// Detection is the sibling source dir, not a `cfg`: the same test binary
/// has to behave differently in the two trees.
pub(crate) fn is_packaged() -> bool {
    !Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pixcore/src")
        .is_dir()
}

/// Where the binary [`Home::cli`] drives lives. `CARGO_BIN_EXE_pixpapers` is
/// set for integration test targets; unit tests inside the bin never get it,
/// so the fallback reconstructs `target/<profile>/pixpapers` from the test
/// binary's location.
fn cli_binary_path() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_pixpapers")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let exe = std::env::current_exe().unwrap();
            let deps = exe
                .parent()
                .expect("test binary lives in a deps/ directory");
            deps.parent()
                .expect("deps/ lives in a target directory")
                .join("pixpapers")
        })
}

/// Early-return guard for the CLI goldens: `true` means *this environment
/// cannot run them at all* — a crate tarball with no built binary — so the
/// test must not fail. `#[test]` fns use it as
/// `if skip_without_cli_binary() { return; }` as their first statement.
///
/// Deliberately narrow: it skips only when the binary **cannot exist**
/// here. A *stale* binary is never skipped, packaged or not — that is an
/// accident the mtime check in [`fresh_bin`] must keep shouting about, and
/// quietly passing would hide it.
pub(crate) fn skip_without_cli_binary() -> bool {
    if !is_packaged() || cli_binary_path().exists() {
        return false;
    }
    eprintln!(
        "note: skipping CLI golden — a crate tarball has no built pixpapers \
         binary (`cargo build --release` before `cargo test` runs these)"
    );
    true
}

/// The workspace `pixcore` source tree, when this checkout has one.
/// `fresh_bin` watches it because pixcore changes rebuild the binary; a
/// published crate tarball has no such directory, so the walk is skipped.
fn sibling_pixcore_src() -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pixcore/src");
    path.is_dir().then_some(path)
}

/// Refuse to run the CLI goldens against a missing or stale binary.
/// `cargo test` alone does not rebuild the plain bin artifact for in-bin
/// unit tests, so `target/<profile>/pixpapers` can silently lag behind the
/// sources; the mtime check turns that silent false-positive into a loud
/// "run `cargo build` first" failure.
///
/// Test-only modules are excluded from the comparison: a file behind a
/// file-level `#[cfg(test)] mod x;` declaration (the harness itself is the
/// only one today) never compiles into the plain binary, so editing it
/// cannot make the binary stale — and `cargo build` correctly leaves the
/// binary's mtime alone, which an unfiltered walk would flag as a
/// staleness no build can cure.
fn fresh_bin(bin: &Path) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    let pp_src = manifest.join("src");
    walk_mtime(&pp_src, &pp_src, &mut newest, &test_only_paths(&pp_src));
    // The workspace pixcore tree only exists in a full checkout; a crate
    // tarball resolves pixcore from the registry, so there is nothing to
    // watch and walking a missing dir would panic before any real check.
    if let Some(pc_src) = sibling_pixcore_src() {
        walk_mtime(&pc_src, &pc_src, &mut newest, &test_only_paths(&pc_src));
    }
    for toml in ["Cargo.toml", "../pixcore/Cargo.toml"] {
        let path = manifest.join(toml);
        let t = match std::fs::metadata(&path) {
            Ok(m) => m.modified().unwrap(),
            // Missing only means "crate tarball, no workspace pixcore
            // manifest"; any other error is a broken checkout — stay loud.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("{path:?} unreadable: {e}"),
        };
        if newest.is_none() || t > newest.as_ref().unwrap().0 {
            newest = Some((t, path));
        }
    }
    let Some((newest_t, newest_p)) = newest else {
        panic!("no sources found under {}", manifest.display());
    };
    let bin_meta = match std::fs::metadata(bin) {
        Ok(m) => m,
        Err(e) => panic!(
            "pixpapers binary not found at {} ({e}) — the CLI goldens need a \
             build; run `cargo build` first",
            bin.display()
        ),
    };
    if bin_meta.modified().unwrap() < newest_t {
        panic!(
            "pixpapers binary at {} is older than the sources ({} is newer) \
             — the CLI goldens would verify a stale build; run `cargo build` first",
            bin.display(),
            newest_p.display()
        );
    }
}

/// `root`-relative paths of entries that compile only under
/// `#[cfg(test)]`: for every `mod name;` declaration (file-level — an
/// inline `mod name {` block stays in the file it is written in, which
/// the walk keeps tracking) whose attribute run includes `cfg(test)`,
/// both `name.rs` and a `name/` module tree are skipped, wherever the
/// declaring file lives (nesting composes: a test-only file's own
/// declarations are scanned too).
fn test_only_paths(root: &Path) -> HashSet<PathBuf> {
    let mut skip: HashSet<PathBuf> = HashSet::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if ft.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_some_and(|e| e != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(file_rel) = path.strip_prefix(root) else {
                continue;
            };
            let parent = file_rel.parent().map(Path::to_path_buf).unwrap_or_default();
            for name in cfg_test_file_mods(&text) {
                skip.insert(parent.join(format!("{name}.rs")));
                skip.insert(parent.join(&name));
            }
        }
    }
    skip
}

/// Names of file-level `mod name;` declarations whose attribute run
/// contains `cfg(test)`. The run is the attribute lines immediately above
/// the declaration (doc/other comments do not break it); any other code
/// line does. A `cfg(test)` anywhere in the run gates the module out of
/// the plain build, so the run — not just the last attribute — decides.
fn cfg_test_file_mods(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut test_attr = false;
    for raw in text.lines() {
        let t = raw.trim();
        let rest = t
            .strip_prefix("pub(crate) mod ")
            .or_else(|| t.strip_prefix("pub mod "))
            .or_else(|| t.strip_prefix("mod "));
        if let Some(rest) = rest {
            let rest = rest.trim_start();
            // The name runs to the first `;`, `{` or whitespace; the
            // terminator is the first non-blank character after it
            // (`;` = file-level module, `{` = inline block).
            let cut = rest.find([';', '{', ' ', '\t']).unwrap_or(rest.len());
            let name = rest[..cut].trim();
            let term = rest[cut..].trim_start().chars().next();
            let is_ident = !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if term == Some(';') && is_ident && test_attr {
                out.push(name.to_string());
            }
            test_attr = false;
        } else if let Some(attr) = t.strip_prefix("#[") {
            if attr.contains("cfg(test)") {
                test_attr = true;
            }
        } else if !t.is_empty()
            && !t.starts_with("//")
            && !t.starts_with("/*")
            && !t.starts_with("*")
        {
            test_attr = false;
        }
    }
    out
}

fn walk_mtime(
    root: &Path,
    dir: &Path,
    newest: &mut Option<(SystemTime, PathBuf)>,
    skip: &HashSet<PathBuf>,
) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let ft = entry.file_type().unwrap();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if skip.contains(rel) {
            continue;
        }
        if ft.is_dir() {
            walk_mtime(root, &path, newest, skip);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let t = entry.metadata().unwrap().modified().unwrap();
            if newest.is_none() || t > newest.as_ref().unwrap().0 {
                *newest = Some((t, path));
            }
        }
    }
}

/// A hermetic `PIXPAPERS_HOME` (tempdir, required dirs created).
/// `pub(crate)`: the S7 TUI unit tests drive `App` against the same
/// hermetic homes.
pub(crate) struct Home {
    _dir: tempfile::TempDir,
    pub(crate) root: PathBuf,
    pub(crate) paths: Paths,
}

impl Home {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let paths = Paths::with_root(&root);
        paths.ensure_dirs().unwrap();
        Self {
            _dir: dir,
            root,
            paths,
        }
    }

    /// The real `pixpapers` binary; see [`cli_binary_path`] for where it
    /// lives and [`fresh_bin`] for why it must be fresh. Tests that call
    /// this must be guarded with [`skip_without_cli_binary`] (no built
    /// binary exists in a crate tarball to fail against).
    pub(crate) fn bin() -> PathBuf {
        let path = cli_binary_path();
        fresh_bin(&path);
        path
    }

    /// Run the CLI as a subprocess against this home (headless-safe).
    pub(crate) fn cli(&self, args: &[&str]) -> (i32, String, String) {
        let out = std::process::Command::new(Self::bin())
            .args(args)
            .env("PIXPAPERS_HOME", &self.root)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    pub(crate) fn entry_id(&self, idx: usize) -> String {
        let text = std::fs::read_to_string(self.paths.manifest_file()).unwrap();
        let lib: pixcore::Library = serde_json::from_str(&text).unwrap();
        lib.entries[idx].id.clone()
    }

    pub(crate) fn manifest_text(&self) -> String {
        std::fs::read_to_string(self.paths.manifest_file()).unwrap()
    }

    pub(crate) fn config_text(&self) -> String {
        std::fs::read_to_string(&self.paths.config_file).unwrap()
    }

    /// A second, distinct source (16×16 solid) for multi-entry homes — the
    /// manifest dedupes by content hash, so it must differ from the fixture;
    /// 16 is the exact minimum short side `optimize` accepts.
    pub(crate) fn small_source(&self) -> PathBuf {
        let p = self.root.join("solid-green.png");
        let img = image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 208, 0, 255]));
        image::save_buffer(
            &p,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ColorType::Rgba8,
        )
        .unwrap();
        p
    }

    /// Assert one stdout line (snapbox keeps the declared dev-dep honest and
    /// gives a readable diff on mismatch). stdout always ends the line with
    /// `\n`, so the expected text is normalized to carry it.
    pub(crate) fn assert_line(&self, actual: &str, expected: &str) {
        snapbox::Assert::new().eq(actual, snapbox::Data::text(format!("{expected}\n")));
    }
}

/// A throwaway listener standing in for the daemon, bound to the home's
/// socket file. It records every request it receives and replies
/// `{"ok":true}` — enough for `ipc::daemon_alive` to report a daemon and
/// for `apply_now` to deliver its Apply.
///
/// The accept thread lives until the test process exits; each harness binds
/// a unique socket under its own tempdir, so parallel tests cannot collide.
pub struct DaemonHarness {
    requests: Arc<Mutex<Vec<Request>>>,
}

impl DaemonHarness {
    pub fn start(paths: &Paths) -> Self {
        let sock = paths.socket_file();
        let _ = std::fs::remove_file(&sock); // stale socket from a crashed run
        let listener = UnixListener::bind(&sock).expect("bind harness socket");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let rec = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    break;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let mut line = String::new();
                let mut reader = BufReader::new(&stream);
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let req: Request = match serde_json::from_str(line.trim()) {
                    Ok(req) => req,
                    Err(_) => {
                        let _ = stream.write_all(b"{\"ok\":false,\"error\":\"bad request\"}\n");
                        continue;
                    }
                };
                rec.lock().unwrap().push(req.clone());
                let _ = stream.write_all(b"{\"ok\":true}\n");
            }
        });
        Self { requests }
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// CLI round-trips (filesystem truth)
// ---------------------------------------------------------------------------

/// Every palette value round-trips: stored on the entry as its token,
/// printed on the edit line, visible in the manifest bytes.
#[test]
fn cli_palette_round_trips() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "fixture"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    for mode in PaletteMode::ALL {
        let tok = mode.token();
        let (code, out, err) = home.cli(&["palette", id.as_str(), tok]);
        assert_eq!(code, 0, "palette {tok} failed:\n{out}\n{err}");
        home.assert_line(
            &out,
            &format!("{id} → palette {tok} (not current — will apply on set)"),
        );
        let m = home.manifest_text();
        assert!(
            m.contains(&format!("\"palette\": \"{tok}\"")),
            "manifest does not store palette {tok}:\n{m}"
        );
    }
}

/// Every dither value round-trips the same way.
#[test]
fn cli_dither_round_trips() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "fixture"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    for d in Dither::ALL {
        let tok = d.token();
        let (code, out, err) = home.cli(&["dither", id.as_str(), tok]);
        assert_eq!(code, 0, "dither {tok} failed:\n{out}\n{err}");
        home.assert_line(
            &out,
            &format!("{id} → dither {tok} (not current — will apply on set)"),
        );
        let m = home.manifest_text();
        assert!(
            m.contains(&format!("\"dither\": \"{tok}\"")),
            "manifest does not store dither {tok}:\n{m}"
        );
    }
}

/// `default palette|dither` edits the config file (string forms) and leaves
/// the entries' own knobs at None (inherit).
#[test]
fn cli_default_palette_dither() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "fixture"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    for mode in PaletteMode::ALL {
        let tok = mode.token();
        let (code, out, err) = home.cli(&["default", "palette", tok]);
        assert_eq!(code, 0, "default palette {tok} failed:\n{out}\n{err}");
        home.assert_line(&out, &format!("default palette: {tok}"));
        let c = home.config_text();
        assert!(
            c.contains(&format!("palette = \"{tok}\"")),
            "config does not store palette {tok}:\n{c}"
        );
    }
    for d in Dither::ALL {
        let tok = d.token();
        let (code, out, err) = home.cli(&["default", "dither", tok]);
        assert_eq!(code, 0, "default dither {tok} failed:\n{out}\n{err}");
        home.assert_line(&out, &format!("default dither: {tok}"));
        let c = home.config_text();
        assert!(
            c.contains(&format!("dither = \"{tok}\"")),
            "config does not store dither {tok}:\n{c}"
        );
    }
    // defaults are config-wide: no entry knob was written
    let m = home.manifest_text();
    assert!(
        m.contains("\"settings\": {}"),
        "an entry gained explicit settings — defaults must not touch entries:\n{m}"
    );
}

// ---------------------------------------------------------------------------
// Daemon harness (IPC truth)
// ---------------------------------------------------------------------------

/// Editing the CURRENT entry pushes exactly one Apply to the (harness)
/// daemon — the same contract the pixel/placement/bg/gutter edits have.
#[test]
fn cli_edit_current_entry_applies() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "current"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let cur = home.entry_id(0);
    let small = home.small_source();
    let (code, out, err) = home.cli(&["add", small.to_str().unwrap(), "--title", "other"]);
    assert_eq!(code, 0, "add (second) failed:\n{out}\n{err}");
    crate::commands::set_current(&home.paths, &cur).unwrap();

    let harness = DaemonHarness::start(&home.paths);
    let (code, out, err) = home.cli(&["palette", cur.as_str(), "nes"]);
    assert_eq!(code, 0, "palette on current entry failed:\n{out}\n{err}");
    home.assert_line(&out, &format!("{cur} → palette nes"));

    let reqs = harness.requests();
    let applies = reqs.iter().filter(|r| matches!(r, Request::Apply)).count();
    assert_eq!(applies, 1, "expected exactly one Apply, got {reqs:?}");
    assert!(
        matches!(reqs.last(), Some(Request::Apply)),
        "the Apply must be the final request, got {reqs:?}"
    );
}

/// Editing a non-current entry only records the change — no daemon traffic.
#[test]
fn cli_edit_non_current_entry_records_only() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "current"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let cur = home.entry_id(0);
    let small = home.small_source();
    let (code, out, err) = home.cli(&["add", small.to_str().unwrap(), "--title", "other"]);
    assert_eq!(code, 0, "add (second) failed:\n{out}\n{err}");
    let other = home.entry_id(1);
    crate::commands::set_current(&home.paths, &cur).unwrap();

    let harness = DaemonHarness::start(&home.paths);
    let (code, out, err) = home.cli(&["dither", other.as_str(), "fs"]);
    assert_eq!(code, 0, "dither on non-current entry failed:\n{out}\n{err}");
    home.assert_line(
        &out,
        &format!("{other} → dither fs (not current — will apply on set)"),
    );

    let reqs = harness.requests();
    assert!(
        reqs.is_empty(),
        "non-current edit must not touch the daemon: {reqs:?}"
    );
}

/// `cycle -1` through the real clap surface: without
/// `allow_negative_numbers`, clap rejects the leading `-` as an unknown
/// flag, which would break the waybar snippet's on-scroll-down binding
/// (S8: wheel down = `cycle -1`, per 8c2f171). Two entries is enough to
/// see the direction: from the second, -1 lands on the first.
#[test]
fn cli_cycle_negative_parses_and_steps_back() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "a"]);
    assert_eq!(code, 0, "add a failed:\n{out}\n{err}");
    let small = home.small_source();
    let (code, out, err) = home.cli(&["add", small.to_str().unwrap(), "--title", "b"]);
    assert_eq!(code, 0, "add b failed:\n{out}\n{err}");
    let (id_a, id_b) = (home.entry_id(0), home.entry_id(1));
    crate::commands::set_current(&home.paths, &id_b).unwrap();

    let (code, out, err) = home.cli(&["cycle", "-1"]);
    assert_eq!(code, 0, "cycle -1 failed:\n{out}\n{err}");
    home.assert_line(
        &out,
        &format!("cycle → {id_a} · a (recorded — will show when the daemon runs)"),
    );
    assert_eq!(
        pixcore::Library::load(&home.paths)
            .unwrap()
            .current
            .as_deref(),
        Some(id_a.as_str())
    );
}

// ---------------------------------------------------------------------------
// Bar label (S8: the waybar line, exactly one line each state)
// ---------------------------------------------------------------------------

/// A current entry wins even with a dead daemon socket: the label is
/// config+manifest truth (what the daemon *would* render), so a recorded
/// wallpaper keeps its name offline. Fresh home ⇒ defaults 16/fit.
#[test]
fn bar_current_entry_dead_daemon() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "nebula"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    crate::commands::set_current(&home.paths, &id).unwrap();

    let (code, out, err) = home.cli(&["bar"]);
    assert_eq!(code, 0, "bar failed:\n{out}\n{err}");
    home.assert_line(&out, "▞ nebula · 16/fit");
}

/// The entry line is identical with a live daemon: the liveness probe
/// never changes the entry case (it only disambiguates a missing entry).
#[test]
fn bar_current_entry_alive_daemon() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "nebula"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    crate::commands::set_current(&home.paths, &id).unwrap();
    DaemonHarness::start(&home.paths);

    let (code, out, err) = home.cli(&["bar"]);
    assert_eq!(code, 0, "bar failed:\n{out}\n{err}");
    home.assert_line(&out, "▞ nebula · 16/fit");
}

/// No current entry (no manifest yet) + a daemon that answers: ask the
/// user to set one. The harness's `{"ok":true}` reply is all
/// `ipc::daemon_alive` needs.
#[test]
fn bar_no_entry_alive_daemon() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    DaemonHarness::start(&home.paths);

    let (code, out, err) = home.cli(&["bar"]);
    assert_eq!(code, 0, "bar failed:\n{out}\n{err}");
    home.assert_line(&out, "▞ pixpapers — set one");
}

/// No current entry + a dead socket: the daemon-down line.
#[test]
fn bar_no_entry_dead_daemon() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();

    let (code, out, err) = home.cli(&["bar"]);
    assert_eq!(code, 0, "bar failed:\n{out}\n{err}");
    home.assert_line(&out, "▞ pixpapers · daemon down");
}

// ---------------------------------------------------------------------------
// Export encoding
// ---------------------------------------------------------------------------

/// `export --palette vga --dither fs` writes a true palette PNG whose PLTE
/// is exactly the VGA table (first-occurrence dedup, placeholder black, bg
/// merged only when absent) and whose pixels all live in that table.
#[test]
fn export_palette_vga_dither_fs() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "fixture"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    let dest = home.root.join("vga-fs.png");
    let (code, out, err) = home.cli(&[
        "export",
        id.as_str(),
        "320x180",
        "--palette",
        "vga",
        "--dither",
        "fs",
        "--out",
        dest.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "export failed:\n{out}\n{err}");
    let bytes = std::fs::read(&dest).unwrap();

    let dec = png::Decoder::new(std::io::Cursor::new(&bytes));
    let mut reader = dec.read_info().unwrap();
    let info = reader.info().clone();
    assert_eq!(info.color_type, png::ColorType::Indexed);
    assert_eq!((info.width, info.height), (320, 180));
    let plte: Vec<[u8; 3]> = info
        .palette
        .unwrap()
        .to_vec()
        .chunks(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();

    // Expected table, computed the encoder's way (commands::encode_final):
    // placeholder black + first-occurrence dedup of the VGA table + the bg
    // color when it is not already an entry (config default: black, which
    // the table already has).
    let mut seen: Vec<[u8; 3]> = Vec::new();
    for c in pixcore::palette::tables::VGA.iter() {
        let rgb = [c.0[0], c.0[1], c.0[2]];
        if !seen.contains(&rgb) {
            seen.push(rgb);
        }
    }
    let bg = [0u8, 0, 0];
    if !seen.contains(&bg) {
        seen.push(bg);
    }
    let mut expected = vec![[0u8, 0, 0]];
    expected.extend(seen);
    assert_eq!(plte, expected);

    // Index 0 is the transparent placeholder; the fixture has no
    // transparent pixels, so every index is a real entry.
    for &ix in &buf {
        assert!(
            (1..=plte.len() as u8).contains(&ix),
            "index {ix} out of range 1..={}",
            plte.len()
        );
    }

    // Sampled rows: every pixel decodes to a color that is in the VGA table.
    let vga: std::collections::HashSet<[u8; 3]> = pixcore::palette::tables::VGA
        .iter()
        .map(|c| [c.0[0], c.0[1], c.0[2]])
        .collect();
    let w = info.width as usize;
    for y in [0usize, 30, 59] {
        for x in 0..w {
            let rgb = plte[buf[y * w + x] as usize];
            assert!(
                vga.contains(&rgb),
                "row {y} px {x} → {rgb:?} is not a VGA table color"
            );
        }
    }

    // The canvas's top-left pixel maps with zero incoming dither error: the
    // source corner is (0,0,0), whose nearest VGA entry is black, exactly.
    let top_left = plte[buf[16] as usize];
    assert_eq!(top_left, [0, 0, 0]);
}

/// `export --palette off` stays plain RGBA — the v0.1 shape.
#[test]
fn export_palette_off_is_rgba() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "fixture"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    let dest = home.root.join("off.png");
    let (code, out, err) = home.cli(&[
        "export",
        id.as_str(),
        "320x180",
        "--palette",
        "off",
        "--out",
        dest.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "export failed:\n{out}\n{err}");
    let bytes = std::fs::read(&dest).unwrap();
    let dec = png::Decoder::new(std::io::Cursor::new(&bytes));
    let reader = dec.read_info().unwrap();
    let info = reader.info().clone();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!((info.width, info.height), (320, 180));
}

/// `export` without `--palette` (i.e. `auto`, the config default) also stays
/// RGBA: 255 + bg would not fit the ≤255 v0.1 scheme.
#[test]
fn export_default_auto_is_rgba() {
    if skip_without_cli_binary() {
        return;
    }
    let home = Home::new();
    let (code, out, err) = home.cli(&["add", fixture().to_str().unwrap(), "--title", "fixture"]);
    assert_eq!(code, 0, "add failed:\n{out}\n{err}");
    let id = home.entry_id(0);
    let dest = home.root.join("auto.png");
    let (code, out, err) = home.cli(&[
        "export",
        id.as_str(),
        "320x180",
        "--out",
        dest.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "export failed:\n{out}\n{err}");
    let bytes = std::fs::read(&dest).unwrap();
    let dec = png::Decoder::new(std::io::Cursor::new(&bytes));
    let reader = dec.read_info().unwrap();
    let info = reader.info().clone();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!((info.width, info.height), (320, 180));
}

/// The fixture [`fixture`] falls back to (`tests/fixtures/fixture.png`) is
/// the one shipped inside the published crate, where the repo-level
/// `test/fixture.png` does not exist. This guard keeps the two
/// byte-identical in the checkout that *does* have both: a silent drift
/// would mean the published crate tests a different image than the repo
/// suite does. In a crate tarball there is nothing to compare against, so
/// the check reports itself as skipped rather than passing unnoticed.
#[test]
fn bundled_fixture_matches_repo_fixture() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let shared = manifest.join("../../test/fixture.png");
    let bundled = manifest.join("tests/fixtures/fixture.png");
    if !shared.exists() {
        assert!(
            bundled.exists(),
            "crate tarball must ship its own fixture at {}",
            bundled.display()
        );
        eprintln!("note: fixture drift guard skipped — no repo fixture in a crate tarball");
        return;
    }
    assert_eq!(
        std::fs::read(&shared).unwrap(),
        std::fs::read(&bundled).unwrap(),
        "{} must stay byte-identical to the canonical {}",
        bundled.display(),
        shared.display()
    );
}
