//! In-process test harness for the CLI: a throwaway unix listener standing
//! in for the daemon, a hermetic `PIXPAPERS_HOME`, and subprocess
//! round-trips. S7's TUI tests reuse [`DaemonHarness`] the same way.
//!
//! Every subprocess runs with `PIXPAPERS_HOME` pointed at a fresh tempdir
//! and both `WAYLAND_DISPLAY` and `DISPLAY` removed, so the CLI can never
//! auto-spawn a real daemon (S5 made `DISPLAY` sufficient for the x11
//! backend); the only socket traffic it can produce goes to our listener.

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pixcore::{Dither, PaletteMode, Paths};

use crate::ipc::Request;

/// The repo's shared test fixture (`crates/pixpapers` → `test/fixture.png`).
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixture.png")
}

/// A hermetic `PIXPAPERS_HOME` (tempdir, required dirs created).
struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
}

impl Home {
    fn new() -> Self {
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

    /// The real `pixpapers` binary. `cargo test` sets `CARGO_BIN_EXE_pixpapers`
    /// for this package's test targets; the fallback reconstructs
    /// `target/<profile>/pixpapers` from the test binary's location for
    /// direct invocations.
    fn bin() -> PathBuf {
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

    /// Run the CLI as a subprocess against this home (headless-safe).
    fn cli(&self, args: &[&str]) -> (i32, String, String) {
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

    fn entry_id(&self, idx: usize) -> String {
        let text = std::fs::read_to_string(self.paths.manifest_file()).unwrap();
        let lib: pixcore::Library = serde_json::from_str(&text).unwrap();
        lib.entries[idx].id.clone()
    }

    fn manifest_text(&self) -> String {
        std::fs::read_to_string(self.paths.manifest_file()).unwrap()
    }

    fn config_text(&self) -> String {
        std::fs::read_to_string(&self.paths.config_file).unwrap()
    }

    /// A second, distinct source (16×16 solid) for multi-entry homes — the
    /// manifest dedupes by content hash, so it must differ from the fixture;
    /// 16 is the exact minimum short side `optimize` accepts.
    fn small_source(&self) -> PathBuf {
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
    fn assert_line(&self, actual: &str, expected: &str) {
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

// ---------------------------------------------------------------------------
// Export encoding
// ---------------------------------------------------------------------------

/// `export --palette vga --dither fs` writes a true palette PNG whose PLTE
/// is exactly the VGA table (first-occurrence dedup, placeholder black, bg
/// merged only when absent) and whose pixels all live in that table.
#[test]
fn export_palette_vga_dither_fs() {
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
