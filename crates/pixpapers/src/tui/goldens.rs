//! TestBackend goldens at a pinned 100×30 (PLAN-30 §C.5): the whole UI
//! is a pure function of `App`, so the frames are checked in as text
//! files under `tests/golden/tui/` and regenerated with
//! `PIX_UPDATE_GOLDENS=1` (same pattern as pixcore's quantize goldens).
//!
//! The dump marks the selected row with a trailing `«sel»` (the
//! highlight is a style, invisible in plain text) so selection movement
//! is pinned too.

use std::collections::HashMap;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

use pixcore::{Config, Entry, Library, Paths, Settings};

use super::detect::detect_from_map;
use super::{App, ImageProto, PreviewState, draw};

fn entry(id: &str, title: &str) -> Entry {
    Entry {
        id: id.into(),
        title: title.into(),
        hash: "0".repeat(64),
        ext: "png".into(),
        added_unix: 0,
        width: 96,
        height: 60,
        original_bytes: 1024,
        optimize_note: "golden fixture".into(),
        settings: Settings::default(),
    }
}

fn app(entries: Vec<Entry>, current: Option<&str>, proto: ImageProto) -> App {
    App {
        paths: Paths::with_root(std::path::Path::new("/golden-nonexistent")),
        cfg: Config::default(),
        lib: Library {
            version: pixcore::MANIFEST_VERSION,
            entries,
            current: current.map(str::to_string),
        },
        sel: 0,
        last: "ready".into(),
        daemon_up: false,
        proto,
        preview: PreviewState::Idle,
        preview_dirty: false,
        emit_pending: false,
        image_on_screen: false,
        kitty_id: 0,
        prev_kitty_id: 0,
    }
}

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Render one frame at the pinned 100×30 and dump the buffer to text,
/// one line per row (trailing spaces trimmed, `«sel»` on the
/// highlighted row).
fn render(app: &App) -> String {
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| {
        draw::frame(f, app);
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    let area = buf.area;
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        let mut sel = false;
        for x in 0..area.width {
            let cell = buf.cell((x, y)).unwrap();
            line.push_str(cell.symbol());
            if cell.style().bg == Some(Color::Gray) {
                sel = true;
            }
        }
        out.push_str(line.trim_end());
        if sel {
            out.push_str("  \u{00ab}sel\u{00bb}");
        }
        out.push('\n');
    }
    out
}

#[test]
fn tui_goldens() {
    let two = || vec![entry("d5113bb7", "nebula"), entry("dc731e25", "dunes")];
    let mut selected = app(two(), Some("d5113bb7"), ImageProto::None);
    selected.sel = 1;
    let cases: Vec<(&str, App)> = vec![
        // initial view in a plain terminal: caption + honest fallback
        ("initial", app(two(), Some("d5113bb7"), ImageProto::None)),
        // the no-image pane with nothing to preview
        ("empty", app(vec![], None, ImageProto::None)),
        // selection moved: the «sel» marker follows the cursor
        ("selected", selected),
        // detection lines, driven through the env map (the same lookup
        // the live detect() runs) — status bar shows img:<proto>
        (
            "detect_kitty",
            app(
                two(),
                Some("d5113bb7"),
                detect_from_map(&env(&[("KITTY_WINDOW_ID", "7")])),
            ),
        ),
        (
            "detect_iterm2",
            app(
                two(),
                Some("d5113bb7"),
                detect_from_map(&env(&[("ITERM_SESSION_ID", "w0t0p0:ABC")])),
            ),
        ),
        (
            "detect_sixel",
            app(
                two(),
                Some("d5113bb7"),
                detect_from_map(&env(&[("TERM", "xterm-sixel")])),
            ),
        ),
        (
            "detect_none",
            app(
                two(),
                Some("d5113bb7"),
                detect_from_map(&env(&[("TERM", "xterm-256color")])),
            ),
        ),
    ];

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/tui");
    let regenerate = std::env::var_os("PIX_UPDATE_GOLDENS").is_some();
    if regenerate {
        std::fs::create_dir_all(&dir).unwrap();
    }
    for (name, app) in cases {
        let actual = render(&app);
        let path = dir.join(format!("{name}.txt"));
        if regenerate {
            std::fs::write(&path, &actual).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "missing golden {}: {e} (write it with PIX_UPDATE_GOLDENS=1)",
                path.display()
            )
        });
        snapbox::Assert::new().eq(actual, snapbox::Data::text(want));
    }
}
