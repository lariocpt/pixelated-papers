//! The C.4 action keys, wired (PLAN-30 §C.4): every mutation calls the
//! *same* `commands::*` function the CLI subcommand calls, then the CLI's
//! `apply_now` seam (invariant #1 — the TUI never reimplements
//! spawn/notify). Action errors never propagate to the event loop (that
//! would kill the TUI); they land in `app.last`, the status bar. Only
//! `KeyOutcome::Quit` reaches the loop.
//!
//! The CLI prints from inside `commands::*` — on the alternate screen
//! those bytes would smear the frame, so every call runs under [`Quiet`]
//! (fd 1+2 → pipe, captured, drained, restored on drop).
//!
//! `rm` is a two-key sequence and `r` is taken by refresh, so remove is
//! bound to single-key `D` (deviation recorded in CLI.md and PLAN-30 §C.4).

use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use pixcore::{Dither, GutterStyle, PaletteMode, Pixel, Placement, RgbCss, Settings};

use super::App;
use crate::commands;

/// What a handled key did to the event loop.
pub(crate) enum KeyOutcome {
    /// Stays in the loop.
    Handled,
    /// Leaves the loop (terminal restored by the guard in `run`).
    Quit,
}

/// What the input line is collecting.
#[derive(Clone, Copy)]
pub(crate) enum InputKind {
    /// `a` — a file path for `commands::add`.
    Add,
    /// `b` — a `#rrggbb` border/gutter colour.
    Bg,
    /// `g` — a gutter width in px.
    Gutter,
}

impl InputKind {
    pub(crate) fn prompt(self) -> &'static str {
        match self {
            InputKind::Add => "add: ",
            InputKind::Bg => "bg: ",
            InputKind::Gutter => "gutter: ",
        }
    }
}

/// The shared input line (a/b/g): a buffer + cursor column. Printable
/// chars insert at the cursor; Backspace deletes; Left/Right/Home/End move
/// the cursor; a paste arrives as a burst of Press events and assembles
/// for free. Enter submits, Esc/Ctrl-C cancel, and `q` cancels too — it
/// does NOT quit while the line is up.
pub(crate) struct InputLine {
    pub kind: InputKind,
    pub buf: String,
    pub pos: usize,
}

impl InputLine {
    fn new(kind: InputKind) -> Self {
        Self {
            kind,
            buf: String::new(),
            pos: 0,
        }
    }

    /// The status-row text with a `█` cursor drawn in the buffer.
    fn prompt_text(&self) -> String {
        let mut chars = self.buf.chars();
        let before: String = chars.by_ref().take(self.pos).collect();
        let after: String = chars.collect();
        format!("{}{}█{}", self.kind.prompt(), before, after)
    }
}

/// Redirect fd 1 and 2 to a pipe for the lifetime of the guard so the CLI's
/// `println!`/`eprintln!` inside `commands::*` (and `apply_now`'s spawn
/// notes) can never touch the alternate screen. `take` drains the capture
/// and restores the real fds; `Drop` makes the restore panic-safe.
///
/// Compiled out under `cfg(test)` entirely — see the `with_quiet` note.
#[cfg(not(test))]
struct Quiet {
    read_fd: libc::c_int,
    write_fd: libc::c_int,
    saved: [libc::c_int; 2],
    live: bool,
}

#[cfg(not(test))]
impl Quiet {
    fn arm() -> Self {
        let mut fds = [0 as libc::c_int; 2];
        let pipe = unsafe { libc::pipe(fds.as_mut_ptr()) };
        debug_assert_eq!(pipe, 0, "pipe() failed in Quiet::arm");
        // CLOEXEC on both ends: a child spawned while armed — the daemon
        // `apply_now` auto-spawns, or any future spawn — must not inherit
        // the pipe's write end, or the drain waits for an EOF that child
        // never delivers. (The daemon redirects its own stdio to its log
        // file, so with the raw pipe fds closed at exec it holds nothing
        // of ours.)
        for &fd in &fds {
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        // fds 1 (stdout) and 2 (stderr) — the doc contract. (`saved` is
        // indexed by slot; the loop value is the fd number.)
        let mut saved = [0 as libc::c_int; 2];
        for (slot, &fd) in [1, 2].iter().enumerate() {
            saved[slot] = unsafe { libc::dup(fd) };
            debug_assert!(saved[slot] >= 0);
            let _ = unsafe { libc::dup2(fds[1], fd) };
        }
        Self {
            read_fd: fds[0],
            write_fd: fds[1],
            saved,
            live: true,
        }
    }

    /// Drain the captured bytes and restore the real fds.
    fn take(&mut self) -> String {
        if !self.live {
            return String::new();
        }
        self.live = false;
        for (slot, &fd) in [1, 2].iter().enumerate() {
            unsafe { libc::dup2(self.saved[slot], fd) };
            unsafe { libc::close(self.saved[slot]) };
        }
        unsafe { libc::close(self.write_fd) };
        // Bounded drain: the closure's writes are synchronous and flushed
        // before `take` runs, so EOF arrives at once in every supported
        // case. The deadline is insurance against a holder we cannot see
        // (a child that inherited fd 1/2 directly): after 2 s, quit
        // quietly with what we have rather than hang forever.
        let mut out = String::new();
        let mut tmp = [0u8; 8192];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            let mut pfd = libc::pollfd {
                fd: self.read_fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut pfd, 1, left.as_millis().min(100) as i32) };
            if ready < 0 {
                break; // EINTR or error
            }
            if ready == 0 {
                continue; // silent, not EOF — poll again until deadline
            }
            let n = unsafe {
                libc::read(
                    self.read_fd,
                    tmp.as_mut_ptr() as *mut libc::c_void,
                    tmp.len(),
                )
            };
            if n <= 0 {
                break; // 0 = EOF, <0 = error/HUP
            }
            out.push_str(&String::from_utf8_lossy(&tmp[..n as usize]));
        }
        unsafe { libc::close(self.read_fd) };
        out
    }
}

#[cfg(not(test))]
impl Drop for Quiet {
    fn drop(&mut self) {
        let _ = self.take();
    }
}

/// Run `f` with stdout+stderr drained into a pipe; returns the value plus
/// the drained text. The status lines themselves are built from the same
/// *data* the CLI's prints use (so they are identical in wording and do
/// not depend on what a test harness intercepts from fd 1). The drain
/// still matters: Rust's stdout is line-buffered in userspace, and the
/// buffered bytes must land in the pipe *before* the fds are restored, or
/// the CLI's own `println!` output would smear the alternate screen.
/// (Stderr is unbuffered — nothing to flush there.)
#[cfg(not(test))]
fn with_quiet<R>(f: impl FnOnce() -> R) -> (R, String) {
    let mut q = Quiet::arm();
    let r = f();
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    (r, q.take())
}

/// Test build: a no-op. libtest already intercepts `println!` at the Rust
/// level (in-memory capture), so an fd-level drain would capture nothing
/// — and worse, a process-global dup2 racing with another test thread's
/// subprocess spawn is a cross-test deadlock: the child inherits the
/// armed pipe's write end and the drain waits for an EOF that child never
/// delivers (observed: the whole libtest pool wedging on a loaded host).
/// The status lines are data-derived, so no test depends on the capture.
#[cfg(test)]
fn with_quiet<R>(f: impl FnOnce() -> R) -> (R, String) {
    (f(), String::new())
}

/// Tilde expansion — a local copy of pixcore's private one (pixcore stays
/// untouched by S7).
fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    Path::new(s).to_path_buf()
}

/// The export destination directory: `$XDG_DOWNLOAD_DIR` (tilde-expanded)
/// when non-empty, else `~/Downloads`, else `.` (no home).
fn download_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("XDG_DOWNLOAD_DIR") {
        if !v.is_empty() {
            return expand_tilde(&v.to_string_lossy());
        }
    }
    dirs::home_dir()
        .map(|h| h.join("Downloads"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Next element after `cur` in the cycle list (wraps to the front).
fn next_in<const N: usize, T: Copy + PartialEq>(all: &[T; N], cur: T) -> T {
    for (i, v) in all.iter().enumerate() {
        if *v == cur {
            return all[(i + 1) % N];
        }
    }
    all[0]
}

/// The placement cycle order — pixcore has no `Placement::ALL`, so the
/// order is defined here (S7's one additive table).
const PLACEMENTS: [Placement; 4] = [
    Placement::Fit,
    Placement::CoverGutter,
    Placement::CoverCrop,
    Placement::Stretch,
];

/// The status-row text while an input line is up (replaces the normal
/// status spans for its lifetime).
pub(crate) fn input_prompt(app: &App) -> Option<String> {
    app.input.as_ref().map(InputLine::prompt_text)
}

/// The status-row text while a remove confirmation is up.
pub(crate) fn confirm_prompt(app: &App) -> Option<String> {
    app.confirm_remove
        .as_deref()
        .map(|id| format!("remove {id}? y/N"))
}

/// Route one raw key event (`run` already filters to Press kind) through
/// the whole C.4 table. Infallible by contract: action errors land in
/// `app.last` (status bar); only a quit reaches the loop.
pub(crate) fn handle(app: &mut App, code: KeyCode, ctrl: bool) -> KeyOutcome {
    // A pending confirmation swallows the very next key: y/Y confirms,
    // everything else cancels (plan §C.4).
    if let Some(id) = app.confirm_remove.take() {
        let yes = !ctrl && matches!(code, KeyCode::Char('y') | KeyCode::Char('Y'));
        if yes {
            do_remove(app, &id);
        } else {
            app.last = "remove cancelled".into();
        }
        return KeyOutcome::Handled;
    }
    // An active input line swallows every key until Enter or a cancel.
    if app.input.is_some() {
        step_input(app, code, ctrl);
        return KeyOutcome::Handled;
    }
    match code {
        KeyCode::Char('q') | KeyCode::Esc => KeyOutcome::Quit,
        KeyCode::Char('c') if ctrl => KeyOutcome::Quit,
        // `a` opens the add input line.
        KeyCode::Char('a') => {
            app.input = Some(InputLine::new(InputKind::Add));
            KeyOutcome::Handled
        }
        // `b` / `g` open the colour / gutter input lines.
        KeyCode::Char('b') => {
            app.input = Some(InputLine::new(InputKind::Bg));
            KeyOutcome::Handled
        }
        KeyCode::Char('g') => {
            app.input = Some(InputLine::new(InputKind::Gutter));
            KeyOutcome::Handled
        }
        KeyCode::Char('s') => {
            do_set(app);
            KeyOutcome::Handled
        }
        KeyCode::Char('c') => {
            do_cycle(app);
            KeyOutcome::Handled
        }
        KeyCode::Char('p') => {
            // Pre-clones so the closure doesn't borrow `app` while cycle_knob
            // mutates it: auto-content needs a probe computed from the stored
            // original (the daemon never analyses).
            let paths = app.paths.clone();
            let sel = app.selected().map(|e| e.id.clone());
            cycle_knob(app, move |r, s| {
                let v = next_in(&Pixel::ALL, r.pixel);
                s.pixel = Some(v);
                // Keep the persisted content probe consistent with the preset:
                // choosing `auto-content` measures the stored original once
                // here; any other look clears a stale probe.
                s.content = if v == Pixel::AutoContent {
                    sel.as_deref()
                        .and_then(|id| commands::content_probe_for(&paths, id))
                } else {
                    None
                };
                format!("pixel {v}")
            });
            KeyOutcome::Handled
        }
        KeyCode::Char('l') => {
            cycle_knob(app, |r, s| {
                let v = next_in(&PLACEMENTS, r.placement);
                s.placement = Some(v);
                format!("placement {v}")
            });
            KeyOutcome::Handled
        }
        KeyCode::Char('t') => {
            cycle_knob(app, |r, s| {
                let v = next_in(&PaletteMode::ALL, r.palette);
                s.palette = Some(v);
                format!("palette {v}")
            });
            KeyOutcome::Handled
        }
        KeyCode::Char('d') => {
            cycle_knob(app, |r, s| {
                let v = next_in(&Dither::ALL, r.dither);
                s.dither = Some(v);
                format!("dither {v}")
            });
            KeyOutcome::Handled
        }
        // `G` cycles the cover-gutter ring fill: solid mat → blurred bleed.
        KeyCode::Char('G') => {
            cycle_knob(app, |r, s| {
                let v = next_in(&GutterStyle::ALL, r.gutter_style);
                s.gutter_style = Some(v);
                format!("gutter-style {v}")
            });
            KeyOutcome::Handled
        }
        // `D` (single key — `rm` is two keys and `r` is refresh): the
        // confirmation line appears in the status row.
        KeyCode::Char('D') => {
            if let Some(entry) = app.selected() {
                app.confirm_remove = Some(entry.id.clone());
            } else {
                app.last = "nothing selected".into();
            }
            KeyOutcome::Handled
        }
        KeyCode::Char('e') => {
            do_toggle(app);
            KeyOutcome::Handled
        }
        KeyCode::Char('x') => {
            do_export(app);
            KeyOutcome::Handled
        }
        // `r` refresh: re-read manifest + config + probe (no IPC) —
        // refresh_state already marks the preview dirty.
        KeyCode::Char('r') => {
            match app.refresh_state() {
                Ok(()) => app.last = "refreshed".into(),
                Err(e) => app.last = format!("refresh failed: {e}"),
            }
            KeyOutcome::Handled
        }
        _ => KeyOutcome::Handled,
    }
}

/// Edit the shared input line: printable insert, Backspace, Left/Right,
/// Home/End. Enter submits; Esc / Ctrl-C cancel; `q` cancels too but does
/// NOT quit while the line is up (Ctrl-Q is ignored). A paste arrives as
/// a burst of Press events and assembles without special handling.
fn step_input(app: &mut App, code: KeyCode, ctrl: bool) {
    let cancel = matches!(code, KeyCode::Esc)
        || (ctrl && matches!(code, KeyCode::Char('c')))
        || (!ctrl && matches!(code, KeyCode::Char('q')));
    if matches!(code, KeyCode::Enter) {
        let line = app.input.take().expect("input is active");
        match line.kind {
            InputKind::Add => action_add(app, line.buf),
            InputKind::Bg => action_bg(app, line.buf),
            InputKind::Gutter => action_gutter(app, line.buf),
        }
        return;
    }
    if cancel {
        app.input = None;
        app.last = "input cancelled".into();
        return;
    }
    let Some(line) = app.input.as_mut() else {
        return;
    };
    match code {
        KeyCode::Char(c) if !ctrl && !c.is_control() => {
            line.buf.insert(line.pos, c);
            line.pos += 1;
        }
        KeyCode::Backspace => {
            if line.pos > 0 {
                line.buf.remove(line.pos - 1);
                line.pos -= 1;
            }
        }
        KeyCode::Left => line.pos = line.pos.saturating_sub(1),
        KeyCode::Right => {
            if line.pos < line.buf.chars().count() {
                line.pos += 1;
            }
        }
        KeyCode::Home => line.pos = 0,
        KeyCode::End => line.pos = line.buf.chars().count(),
        _ => {}
    }
}

/// After any mutation: cheap state reload (manifest + config + daemon
/// probe); `refresh_state` already marks the preview dirty.
fn after_mutation(app: &mut App) {
    if let Err(e) = app.refresh_state() {
        app.last = format!("reload failed: {e}");
    }
}

/// Move the selection onto `id` after a set/cycle/add (clamped by the
/// reload).
fn select_entry(app: &mut App, id: &str) {
    if let Some(i) = app.lib.entries.iter().position(|e| e.id == id) {
        app.sel = i;
    }
}

/// `s` — set the selected entry current: the CLI's exact path
/// (`set_current` + `apply_now`, auto-spawning the daemon when dead) with
/// the CLI's exact status line.
fn do_set(app: &mut App) {
    let Some(entry) = app.selected() else {
        app.last = "nothing selected".into();
        return;
    };
    let id = entry.id.clone();
    let (res, _out) = with_quiet(|| {
        let e = commands::set_current(&app.paths, &id)?;
        crate::cli::apply_now(&app.paths);
        Ok::<_, anyhow::Error>((e, crate::cli::state_line(&app.paths)))
    });
    match res {
        Ok((e, state)) => {
            app.last = format!("set {} · {} ({})", e.id, e.title, state);
            after_mutation(app);
            select_entry(app, &e.id);
        }
        Err(e) => app.last = format!("set failed: {e}"),
    }
}

/// `c` — rotate to the next wallpaper (by 1), same seam as CLI `cycle`.
fn do_cycle(app: &mut App) {
    if app.lib.entries.is_empty() {
        app.last = "library empty — add one first".into();
        return;
    }
    let (res, _out) = with_quiet(|| {
        let e = commands::cycle_current(&app.paths, 1)?;
        crate::cli::apply_now(&app.paths);
        Ok::<_, anyhow::Error>((e, crate::cli::state_line(&app.paths)))
    });
    match res {
        Ok((e, state)) => {
            app.last = format!("cycle → {} · {} ({})", e.id, e.title, state);
            after_mutation(app);
            select_entry(app, &e.id);
        }
        Err(e) => app.last = format!("cycle failed: {e}"),
    }
}

/// The confirmed remove: `commands::remove` (entry dir + manifest row) and
/// `apply_now` only when the removed entry was the current one. The status
/// line is the CLI's `removed <id> (<title>)`, built from the entry's own
/// data (same wording, no stdout capture).
fn do_remove(app: &mut App, id: &str) {
    let was_current = app.lib.current.as_deref() == Some(id);
    let title = app
        .lib
        .entries
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.title.clone());
    let (res, _out) = with_quiet(|| {
        commands::remove(&app.paths, id)?;
        if was_current {
            crate::cli::apply_now(&app.paths);
        }
        Ok::<(), anyhow::Error>(())
    });
    match res {
        Ok(()) => {
            app.last = match title {
                Some(t) => format!("removed {id} ({t})"),
                None => format!("removed {id}"),
            };
            after_mutation(app);
        }
        Err(e) => app.last = format!("remove failed: {e}"),
    }
}

/// `e` — toggle pixelation on/off (config), then Apply. Status line is the
/// CLI's `set_enabled` wording, built from the toggle value itself.
fn do_toggle(app: &mut App) {
    let on = !app.cfg.enabled;
    let (res, _out) = with_quiet(|| {
        let mut cfg = app.cfg.clone();
        commands::set_enabled(&app.paths, &mut cfg, on)?;
        crate::cli::apply_now(&app.paths);
        Ok::<(), anyhow::Error>(())
    });
    match res {
        Ok(()) => {
            app.last = if on {
                "pixelation enabled".into()
            } else {
                "pixelation disabled — originals only (still stretched/placed)".into()
            };
            after_mutation(app);
        }
        Err(e) => app.last = format!("toggle failed: {e}"),
    }
}

/// `x` — export the selected entry at the preview canvas (1920×1080
/// before the first frame) to the download dir. The status line is the
/// CLI's `wrote …` line, built from the export's own data (dest, canvas,
/// resolved knobs, bytes on disk). Export is not a mutation: no reload,
/// no Apply.
fn do_export(app: &mut App) {
    let Some(entry) = app.selected() else {
        app.last = "nothing selected".into();
        return;
    };
    let r = crate::commands::resolve_effective(entry, &app.cfg);
    let canvas = if app.canvas.0 > 0 && app.canvas.1 > 0 {
        app.canvas
    } else {
        (1920, 1080)
    };
    let dest = download_dir().join(format!(
        "pixpapers-{}-{}-{}-{}x{}.png",
        entry.id, r.pixel, r.placement, canvas.0, canvas.1
    ));
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            app.last = format!("export failed: cannot create {}: {e}", parent.display());
            return;
        }
    }
    let (res, _out) = with_quiet(|| {
        commands::export(
            &app.paths,
            &app.cfg,
            &entry.id,
            &Settings::default(),
            canvas,
            Some(&dest),
        )
    });
    match res {
        Ok(()) => {
            let kb = std::fs::metadata(&dest)
                .map(|m| m.len() as f64 / 1024.0)
                .unwrap_or(0.0);
            app.last = format!(
                "wrote {} · {}×{} · {} {} · {:.0} KB",
                dest.display(),
                canvas.0,
                canvas.1,
                r.pixel,
                r.placement,
                kb
            );
        }
        Err(e) => app.last = format!("export failed: {e}"),
    }
}

/// Image extensions `a` accepts (the `image` crate's readable set for
/// this project; decode itself still runs via `commands::add`).
const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tiff", "tif"];

/// `a` submit — validation (exists / regular file / image extension),
/// then `commands::add` exactly like the CLI. The CLI `add` neither sets
/// current nor Applies, so the TUI matches that; the selection moves to
/// the new (or deduped) entry. The status line is the CLI's own wording
/// (new `added …` / dedupe `already in library: …`), built from the
/// entry's data — re-adding the same bytes is not an error.
fn action_add(app: &mut App, raw: String) {
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        app.last = "add: empty path".into();
        return;
    }
    let path = expand_tilde(&raw);
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => {
            app.last = format!("no such file: {}", path.display());
            return;
        }
    };
    if meta.is_dir() {
        app.last = "not a file — pick a single image".into();
        return;
    }
    if !meta.is_file() {
        app.last = format!("not a regular file: {}", path.display());
        return;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let ext = ext.to_ascii_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        app.last = format!("no image extension (got {ext:?}) — bulk add is a CLI job");
        return;
    }
    // Dedupe is detected from data, not from captured stdout (libtest
    // intercepts `println!` at the Rust level, so an fd-level capture is
    // empty in tests): `commands::add` returns the *existing* entry when
    // the hash is already in the library, so compare against the pre-add
    // state and print the CLI's own line from the entry's fields.
    let pre_hashes: Vec<String> = app.lib.entries.iter().map(|e| e.hash.clone()).collect();
    let (res, _out) = with_quiet(|| commands::add(&app.paths, &app.cfg, &path, None));
    match res {
        Ok(entry) => {
            app.last = if pre_hashes.contains(&entry.hash) {
                format!("already in library: {} ({})", entry.id, entry.title)
            } else {
                format!(
                    "added {} · {} · {}×{} · {} · {:.0} KB",
                    entry.id,
                    entry.title,
                    entry.width,
                    entry.height,
                    entry.optimize_note,
                    entry.original_bytes as f64 / 1024.0
                )
            };
            after_mutation(app);
            select_entry(app, &entry.id);
        }
        Err(e) => app.last = format!("add failed: {e}"),
    }
}

/// `b` submit — parse `#rrggbb` (or `#rgb`) and write the entry's bg.
fn action_bg(app: &mut App, raw: String) {
    let raw = raw.trim();
    if raw.is_empty() {
        app.last = "bg: empty colour".into();
        return;
    }
    let color = match raw.parse::<RgbCss>() {
        Ok(c) => c,
        Err(e) => {
            app.last = format!("bg failed: {e}");
            return;
        }
    };
    let Some(entry) = app.selected().cloned() else {
        app.last = "nothing selected".into();
        return;
    };
    edit_knob(app, &entry, |_, s| {
        s.bg_color = Some(color);
        format!("bg {color}")
    });
}

/// `g` submit — parse a pixel count and write the entry's gutter.
fn action_gutter(app: &mut App, raw: String) {
    let raw = raw.trim();
    if raw.is_empty() {
        app.last = "gutter: empty value".into();
        return;
    }
    let px = match raw.parse::<u32>() {
        Ok(v) => v,
        Err(_) => {
            app.last = format!("gutter failed: {raw:?} is not a pixel count");
            return;
        }
    };
    let Some(entry) = app.selected().cloned() else {
        app.last = "nothing selected".into();
        return;
    };
    edit_knob(app, &entry, |_, s| {
        s.gutter_px = Some(px);
        format!("gutter {px}px")
    });
}

/// One-step knob edit shared by the `p`/`l`/`t`/`d` cycles and the
/// `b`/`g` submits: `f` computes the next value from the entry's
/// *effective* settings, writes it explicitly to the stored settings,
/// and returns the status-line suffix (the CLI's exact wording). Apply
/// happens only when the edited entry is the current one.
fn edit_knob(
    app: &mut App,
    entry: &pixcore::Entry,
    f: impl FnOnce(&pixcore::ResolvedSettings, &mut Settings) -> String,
) {
    let id = entry.id.clone();
    let r = crate::commands::resolve_effective(entry, &app.cfg);
    let (res, _out) = with_quiet(|| {
        let mut suffix = String::new();
        let (e, cur) = commands::edit_entry(&app.paths, &id, |s| {
            suffix = f(&r, s);
        })?;
        if cur {
            crate::cli::apply_now(&app.paths);
        }
        Ok::<_, anyhow::Error>((e, cur, suffix))
    });
    match res {
        Ok((e, cur, suffix)) => {
            let note = if cur {
                ""
            } else {
                " (not current — will apply on set)"
            };
            app.last = format!("{} → {suffix}{note}", e.id);
            after_mutation(app);
        }
        Err(e) => app.last = format!("edit failed: {e}"),
    }
}

/// `p` — cycle the pixel preset 8→16→32→64→128→normal→auto→auto-content→8…
/// (`auto` picks from the output size; `auto-content` also reads the stored
/// picture's detail, measured once on selection).
fn cycle_knob(app: &mut App, f: impl FnOnce(&pixcore::ResolvedSettings, &mut Settings) -> String) {
    let Some(entry) = app.selected().cloned() else {
        app.last = "nothing selected".into();
        return;
    };
    edit_knob(app, &entry, f);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{DaemonHarness, Home};
    use crate::ipc::Request;

    /// A fresh hermetic home + app. Actions that Apply need
    /// [`DaemonHarness`] started first — never rely on a real daemon.
    fn app_for(home: &Home) -> App {
        App::new(home.paths.clone(), pixcore::Config::default()).unwrap()
    }

    /// A home with the fixture + a second, distinct source (two entries).
    fn two_entry_home() -> (Home, String, String) {
        let home = Home::new();
        let src = crate::harness::fixture();
        let (code, out, err) = home.cli(&["add", src.to_str().unwrap(), "--title", "fixture"]);
        assert_eq!(code, 0, "add fixture failed:\n{out}\n{err}");
        let small = home.small_source();
        let (code, out, err) = home.cli(&["add", small.to_str().unwrap(), "--title", "other"]);
        assert_eq!(code, 0, "add small failed:\n{out}\n{err}");
        let id0 = home.entry_id(0);
        let id1 = home.entry_id(1);
        (home, id0, id1)
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            let o = handle(app, KeyCode::Char(c), false);
            assert!(
                matches!(o, KeyOutcome::Handled),
                "typing {c:?} left the loop"
            );
        }
    }

    fn key(app: &mut App, code: KeyCode) -> KeyOutcome {
        handle(app, code, false)
    }

    fn manifest_entries(home: &Home) -> Vec<pixcore::Entry> {
        // A home that never saved a manifest has no file at all (Library::load
        // treats that as empty) — mirror that here.
        let Ok(text) = std::fs::read_to_string(home.paths.manifest_file()) else {
            return Vec::new();
        };
        let lib: pixcore::Library = serde_json::from_str(&text).unwrap();
        lib.entries
    }

    /// The Apply requests the harness saw (Status probes from `state_line`
    /// and `refresh_state` also land there — only Apply is a mutation).
    fn applies(h: &DaemonHarness) -> usize {
        h.requests()
            .iter()
            .filter(|r| matches!(r, Request::Apply))
            .count()
    }

    // ── input line ──────────────────────────────────────────────────────

    #[test]
    fn input_q_cancels_but_does_not_quit() {
        let home = Home::new();
        let mut app = app_for(&home);
        assert!(matches!(
            key(&mut app, KeyCode::Char('a')),
            KeyOutcome::Handled
        ));
        type_str(&mut app, "/tmp/x");
        // `q` while the line is up cancels it and stays in the loop.
        assert!(matches!(
            key(&mut app, KeyCode::Char('q')),
            KeyOutcome::Handled
        ));
        assert!(app.input.is_none());
        assert_eq!(app.last, "input cancelled");
        assert!(manifest_entries(&home).is_empty());
    }

    #[test]
    fn input_esc_and_ctrl_c_cancel() {
        let home = Home::new();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('b'));
        type_str(&mut app, "#aabbcc");
        assert!(matches!(key(&mut app, KeyCode::Esc), KeyOutcome::Handled));
        assert!(app.input.is_none());
        assert_eq!(app.last, "input cancelled");

        key(&mut app, KeyCode::Char('g'));
        type_str(&mut app, "40");
        assert!(matches!(
            handle(&mut app, KeyCode::Char('c'), true),
            KeyOutcome::Handled
        ));
        assert!(app.input.is_none());
        assert_eq!(app.last, "input cancelled");
    }

    #[test]
    fn input_editing_moves_and_deletes() {
        let home = Home::new();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('a'));
        type_str(&mut app, "abcd");
        let line = app.input.as_mut().unwrap();
        assert_eq!((line.buf.as_str(), line.pos), ("abcd", 4));
        // left, insert, end, home, backspace
        key(&mut app, KeyCode::Left);
        type_str(&mut app, "X"); // "abcXd", pos 4
        key(&mut app, KeyCode::End);
        key(&mut app, KeyCode::Home);
        key(&mut app, KeyCode::Backspace); // at 0: no-op
        key(&mut app, KeyCode::Right); // pos 1
        key(&mut app, KeyCode::Backspace); // deletes 'a'
        let (buf, pos) = (
            app.input.as_ref().unwrap().buf.clone(),
            app.input.as_ref().unwrap().pos,
        );
        assert_eq!((buf.as_str(), pos), ("bcXd", 0));
        // Ctrl-Q is ignored (neither insert nor cancel).
        assert!(matches!(
            handle(&mut app, KeyCode::Char('q'), true),
            KeyOutcome::Handled
        ));
        assert_eq!(app.input.as_ref().unwrap().buf, "bcXd");
        // non-ASCII text inserts too (paste of a unicode path); the cursor
        // is still at 0 after the failed Backspace, so "é" goes first
        type_str(&mut app, "é");
        let line = app.input.as_ref().unwrap();
        assert_eq!((line.buf.as_str(), line.pos), ("ébcXd", 1));
    }

    // ── add by path ─────────────────────────────────────────────────────

    #[test]
    fn add_missing_path_is_a_status_error() {
        let home = Home::new();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('a'));
        type_str(&mut app, "/definitely/not/here.png");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert!(app.input.is_none());
        assert!(app.last.contains("no such file"), "{}", app.last);
        assert!(manifest_entries(&home).is_empty());
    }

    #[test]
    fn add_directory_is_a_status_error() {
        let home = Home::new();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('a'));
        // "/tmp" — a directory, and deliberately q-free: the input line
        // cancels on `q` (spec), so typed paths here must never contain
        // one (a random tempdir suffix would make this test flaky).
        type_str(&mut app, "/tmp");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert_eq!(app.last, "not a file — pick a single image");
        assert!(manifest_entries(&home).is_empty());
    }

    #[test]
    fn add_bad_extension_is_a_status_error() {
        let home = Home::new();
        // fixed q-free path (the input line cancels on `q` — see the
        // note in add_directory_is_a_status_error)
        let dir = std::env::temp_dir().join("pixpapers-badext-test");
        std::fs::create_dir_all(&dir).unwrap();
        let txt = dir.join("notes.txt");
        std::fs::write(&txt, "not an image").unwrap();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('a'));
        type_str(&mut app, &txt.to_string_lossy());
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert!(app.last.contains("no image extension"), "{}", app.last);
        assert!(manifest_entries(&home).is_empty());
    }

    #[test]
    fn add_by_path_creates_the_entry() {
        let home = Home::new();
        let src = crate::harness::fixture();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('a'));
        type_str(&mut app, &src.to_string_lossy());
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert!(app.input.is_none());

        let entries = manifest_entries(&home);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        // the CLI's own `add` line, verbatim (W×H, optimize note and KB
        // from the entry's data — not from captured stdout)
        assert_eq!(
            app.last,
            format!(
                "added {} · {} · {}×{} · {} · {:.0} KB",
                e.id,
                e.title,
                e.width,
                e.height,
                e.optimize_note,
                e.original_bytes as f64 / 1024.0
            ),
            "{}",
            app.last
        );
        // selection moves to the new entry; `add` does not set current
        assert_eq!(app.sel, 0);
        assert_eq!(app.lib.current, None);
    }

    #[test]
    fn add_dedupe_is_not_an_error() {
        let home = Home::new();
        let src = crate::harness::fixture();
        let mut app = app_for(&home);
        for _ in 0..2 {
            key(&mut app, KeyCode::Char('a'));
            type_str(&mut app, &src.to_string_lossy());
            assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        }
        let entries = manifest_entries(&home);
        assert_eq!(entries.len(), 1, "dedupe must keep one entry");
        // the CLI's own line, verbatim (the image is already in the library)
        assert_eq!(
            app.last,
            format!(
                "already in library: {} ({})",
                entries[0].id, entries[0].title
            ),
            "{}",
            app.last
        );
        assert_eq!(app.sel, 0);
    }

    /// Add-by-path is byte- and hash-identical to CLI `add`: same id/hash,
    /// same manifest row (modulo the added_unix timestamp) and the stored
    /// original file is byte-identical (deterministic re-encode).
    #[test]
    fn add_by_path_matches_cli_add() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let cli_home = Home::new();
        let tui_home = Home::new();
        let src = crate::harness::fixture();

        let (code, out, err) = cli_home.cli(&["add", src.to_str().unwrap(), "--title", "fixture"]);
        assert_eq!(code, 0, "cli add failed:\n{out}\n{err}");
        let cli_e = manifest_entries(&cli_home).pop().unwrap().clone();

        let mut app = app_for(&tui_home);
        key(&mut app, KeyCode::Char('a'));
        type_str(&mut app, &src.to_string_lossy());
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        let tui_e = manifest_entries(&tui_home).pop().unwrap();

        assert_eq!(tui_e.id, cli_e.id);
        assert_eq!(tui_e.hash, cli_e.hash);
        assert_eq!(tui_e.title, cli_e.title);
        assert_eq!(tui_e.ext, cli_e.ext);
        assert_eq!(tui_e.width, cli_e.width);
        assert_eq!(tui_e.height, cli_e.height);
        assert_eq!(tui_e.original_bytes, cli_e.original_bytes);
        assert_eq!(tui_e.optimize_note, cli_e.optimize_note);
        assert_eq!(tui_e.settings, cli_e.settings);
        // added_unix is a wall-clock stamp — allowed to differ

        let read_orig = |h: &Home, id: &str, ext: &str| {
            std::fs::read(h.paths.entry_dir(id).join(format!("original.{ext}"))).unwrap()
        };
        assert_eq!(
            read_orig(&tui_home, &tui_e.id, &tui_e.ext),
            read_orig(&cli_home, &cli_e.id, &cli_e.ext)
        );
    }

    // ── set / cycle (Apply via the throwaway listener) ──────────────────

    #[test]
    fn set_current_applies_and_moves_selection() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, id1) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        app.sel = 1;
        assert!(matches!(
            key(&mut app, KeyCode::Char('s')),
            KeyOutcome::Handled
        ));
        let text = home.manifest_text();
        let lib: pixcore::Library = serde_json::from_str(&text).unwrap();
        assert_eq!(lib.current.as_deref(), Some(id1.as_str()));
        assert_eq!(app.sel, 1);
        // state_line sees the (harness) daemon: "applied"
        assert_eq!(
            app.last,
            format!("set {} · other (applied)", id1),
            "{}",
            app.last
        );
        // exactly one Apply (Status probes from state_line/refresh also land)
        assert_eq!(applies(&harness), 1, "{:?}", harness.requests());
    }

    /// Strip the display env for the test's duration so `ensure_running`
    /// can never spawn (it would run *this test binary* with a libtest
    /// name filter — the S7 fork-bomb). The original values are captured
    /// and restored exactly on drop, so the guard leaks nothing into the
    /// other (parallel) tests in this process.
    struct HeadlessGuard {
        display: Option<std::ffi::OsString>,
        wayland: Option<std::ffi::OsString>,
    }
    impl HeadlessGuard {
        fn new() -> Self {
            let display = std::env::var_os("DISPLAY");
            let wayland = std::env::var_os("WAYLAND_DISPLAY");
            unsafe {
                std::env::remove_var("DISPLAY");
                std::env::remove_var("WAYLAND_DISPLAY");
            }
            Self { display, wayland }
        }
    }
    impl Drop for HeadlessGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.display {
                    Some(v) => std::env::set_var("DISPLAY", v),
                    None => std::env::remove_var("DISPLAY"),
                }
                match &self.wayland {
                    Some(v) => std::env::set_var("WAYLAND_DISPLAY", v),
                    None => std::env::remove_var("WAYLAND_DISPLAY"),
                }
            }
        }
    }

    #[test]
    fn set_with_no_listener_records_only() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let _headless = HeadlessGuard::new();
        let (home, id0, id1) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        // no listener: the daemon is dead and cannot be spawned headlessly
        let mut app = app_for(&home);
        app.sel = 1;
        assert!(matches!(
            key(&mut app, KeyCode::Char('s')),
            KeyOutcome::Handled
        ));
        let lib: pixcore::Library = serde_json::from_str(&home.manifest_text()).unwrap();
        assert_eq!(lib.current.as_deref(), Some(id1.as_str()));
        assert_eq!(
            app.last,
            format!(
                "set {} · other (recorded — will show when the daemon runs)",
                id1
            ),
            "{}",
            app.last
        );
    }

    #[test]
    fn cycle_rotates_and_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, id1) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        assert!(app.sel == 0);
        assert!(matches!(
            key(&mut app, KeyCode::Char('c')),
            KeyOutcome::Handled
        ));
        let lib: pixcore::Library = serde_json::from_str(&home.manifest_text()).unwrap();
        assert_eq!(lib.current.as_deref(), Some(id1.as_str()));
        assert_eq!(app.sel, 1, "selection follows the rotated entry");
        assert!(
            app.last.starts_with(&format!("cycle → {id1} · ")),
            "{}",
            app.last
        );
        assert_eq!(applies(&harness), 1, "{:?}", harness.requests());
        // and back around the ring
        assert!(matches!(
            key(&mut app, KeyCode::Char('c')),
            KeyOutcome::Handled
        ));
        let lib: pixcore::Library = serde_json::from_str(&home.manifest_text()).unwrap();
        assert_eq!(lib.current.as_deref(), Some(id0.as_str()));
        assert_eq!(applies(&harness), 2, "each cycle Applies once");
    }

    #[test]
    fn cycle_on_empty_library_is_a_note() {
        let home = Home::new();
        let mut app = app_for(&home);
        assert!(matches!(
            key(&mut app, KeyCode::Char('c')),
            KeyOutcome::Handled
        ));
        assert_eq!(app.last, "library empty — add one first");
    }

    // ── knob cycles (Apply only when the edited entry is current) ───────

    fn assert_manifest_knob(home: &Home, key_name: &str, want: &str) {
        let m = home.manifest_text();
        assert!(
            m.contains(&format!("{key_name}\": \"{want}\"")),
            "manifest does not store {key_name} {want}:\n{m}"
        );
    }

    #[test]
    fn pixel_cycle_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        // effective 16 (config default) → 32
        assert!(matches!(
            key(&mut app, KeyCode::Char('p')),
            KeyOutcome::Handled
        ));
        assert_manifest_knob(&home, "pixel", "32");
        assert_eq!(app.last, format!("{id0} → pixel 32"), "{}", app.last);
        assert_eq!(applies(&harness), 1, "{:?}", harness.requests());
        // wraps: 32 → 64 → … → normal → auto → auto-content → 8. The
        // `auto-content` step also persists a content probe for the entry;
        // the manifest knob check reads the stored pixel token.
        for want in ["64", "128", "normal", "auto", "auto-content", "8"] {
            assert!(matches!(
                key(&mut app, KeyCode::Char('p')),
                KeyOutcome::Handled
            ));
            assert_manifest_knob(&home, "pixel", want);
        }
        assert_eq!(
            applies(&harness),
            7,
            "every step on the current entry Applies"
        );
    }

    #[test]
    fn placement_cycle_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        // fit → cover-gutter → cover-crop → stretch → fit
        for want in ["cover-gutter", "cover-crop", "stretch", "fit"] {
            assert!(matches!(
                key(&mut app, KeyCode::Char('l')),
                KeyOutcome::Handled
            ));
            assert_manifest_knob(&home, "placement", want);
        }
        assert_eq!(app.last, format!("{id0} → placement fit"), "{}", app.last);
        assert_eq!(
            applies(&harness),
            4,
            "every step on the current entry Applies"
        );
    }

    #[test]
    fn gutter_style_cycle_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        // gutter-style cycle: solid (default) → bleed → solid (wraps)
        for want in ["bleed", "solid"] {
            assert!(matches!(
                key(&mut app, KeyCode::Char('G')),
                KeyOutcome::Handled
            ));
            assert_manifest_knob(&home, "gutter_style", want);
        }
        assert_eq!(
            app.last,
            format!("{id0} → gutter-style solid"),
            "{}",
            app.last
        );
        assert_eq!(
            applies(&harness),
            2,
            "every step on the current entry Applies"
        );
    }

    #[test]
    fn palette_cycle_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        // auto → nes → … → gen16 → off → auto
        for want in ["nes", "gb", "cga", "ega", "vga", "gen16", "off", "auto"] {
            assert!(matches!(
                key(&mut app, KeyCode::Char('t')),
                KeyOutcome::Handled
            ));
            assert_manifest_knob(&home, "palette", want);
        }
        assert_eq!(app.last, format!("{id0} → palette auto"), "{}", app.last);
        assert_eq!(
            applies(&harness),
            8,
            "every step on the current entry Applies"
        );
    }

    #[test]
    fn dither_cycle_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        // none → fs → … → bayer8 → none
        for want in ["fs", "atkinson", "bayer2", "bayer4", "bayer8", "none"] {
            assert!(matches!(
                key(&mut app, KeyCode::Char('d')),
                KeyOutcome::Handled
            ));
            assert_manifest_knob(&home, "dither", want);
        }
        assert_eq!(app.last, format!("{id0} → dither none"), "{}", app.last);
        assert_eq!(
            applies(&harness),
            6,
            "every step on the current entry Applies"
        );
    }

    #[test]
    fn knob_cycle_on_non_current_records_only() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, id1) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        app.sel = 1; // edit the OTHER entry
        assert!(matches!(
            key(&mut app, KeyCode::Char('p')),
            KeyOutcome::Handled
        ));
        assert_manifest_knob(&home, "pixel", "32");
        assert_eq!(
            app.last,
            format!("{id1} → pixel 32 (not current — will apply on set)"),
            "{}",
            app.last
        );
        assert_eq!(
            applies(&harness),
            0,
            "non-current edit must not Apply: {:?}",
            harness.requests()
        );
    }

    // ── bg / gutter input lines ─────────────────────────────────────────

    #[test]
    fn bg_input_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('b'));
        type_str(&mut app, "#123456");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert_manifest_knob(&home, "bg_color", "#123456");
        assert_eq!(app.last, format!("{id0} → bg #123456"), "{}", app.last);
        assert_eq!(applies(&harness), 1, "{:?}", harness.requests());
        // the short #rgb spelling expands to the same stored bytes
        key(&mut app, KeyCode::Char('b'));
        type_str(&mut app, "#abc");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert_manifest_knob(&home, "bg_color", "#aabbcc");
        assert_eq!(applies(&harness), 2);
    }

    #[test]
    fn bg_input_bad_value_is_a_status_error() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('b'));
        type_str(&mut app, "red");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert!(app.last.starts_with("bg failed:"), "{}", app.last);
        assert!(
            home.manifest_text().contains("\"settings\": {}"),
            "bad value must not be stored"
        );
        assert_eq!(applies(&harness), 0);
    }

    #[test]
    fn gutter_input_on_current_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('g'));
        type_str(&mut app, "40");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        // gutter_px is numeric in JSON, not a token string
        assert!(
            home.manifest_text().contains("\"gutter_px\": 40"),
            "manifest does not store gutter_px 40:\n{}",
            home.manifest_text()
        );
        assert_eq!(app.last, format!("{id0} → gutter 40px"), "{}", app.last);
        assert_eq!(applies(&harness), 1, "{:?}", harness.requests());
    }

    #[test]
    fn gutter_input_bad_value_is_a_status_error() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, _id0, _) = two_entry_home();
        let mut app = app_for(&home);
        key(&mut app, KeyCode::Char('g'));
        type_str(&mut app, "forty");
        assert!(matches!(key(&mut app, KeyCode::Enter), KeyOutcome::Handled));
        assert!(app.last.contains("not a pixel count"), "{}", app.last);
        assert!(home.manifest_text().contains("\"settings\": {}"));
    }

    // ── remove (confirm line) ───────────────────────────────────────────

    #[test]
    fn remove_confirmed_clears_dir_and_manifest() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        let dir = home.paths.entry_dir(&id0);
        assert!(dir.exists());
        assert!(matches!(
            key(&mut app, KeyCode::Char('D')),
            KeyOutcome::Handled
        ));
        assert_eq!(app.confirm_remove.as_deref(), Some(id0.as_str()));
        assert!(matches!(
            key(&mut app, KeyCode::Char('y')),
            KeyOutcome::Handled
        ));
        assert!(app.confirm_remove.is_none());
        assert_eq!(app.last, format!("removed {id0} (fixture)"), "{}", app.last);
        assert!(!dir.exists(), "entry dir must be gone");
        let lib: pixcore::Library = serde_json::from_str(&home.manifest_text()).unwrap();
        assert!(lib.entries.iter().all(|e| e.id != id0));
        assert_eq!(lib.current, None, "removing current clears current");
        assert_eq!(applies(&harness), 1, "removing the current entry Applies");
    }

    #[test]
    fn remove_cancelled_touches_nothing() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, _) = two_entry_home();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        assert!(matches!(
            key(&mut app, KeyCode::Char('D')),
            KeyOutcome::Handled
        ));
        // anything but y/Y cancels — here `n`
        assert!(matches!(
            key(&mut app, KeyCode::Char('n')),
            KeyOutcome::Handled
        ));
        assert_eq!(app.last, "remove cancelled");
        assert!(home.paths.entry_dir(&id0).exists());
        assert_eq!(manifest_entries(&home).len(), 2);
        assert_eq!(applies(&harness), 0);
    }

    #[test]
    fn remove_non_current_entry_no_apply() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, id1) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        app.sel = 1;
        assert!(matches!(
            key(&mut app, KeyCode::Char('D')),
            KeyOutcome::Handled
        ));
        assert!(matches!(
            key(&mut app, KeyCode::Char('Y')),
            KeyOutcome::Handled
        ));
        assert_eq!(app.last, format!("removed {id1} (other)"), "{}", app.last);
        assert_eq!(manifest_entries(&home).len(), 1);
        let lib: pixcore::Library = serde_json::from_str(&home.manifest_text()).unwrap();
        assert_eq!(
            lib.current.as_deref(),
            Some(id0.as_str()),
            "current survives"
        );
        assert_eq!(applies(&harness), 0, "no Apply for a non-current remove");
    }

    // ── enable / disable toggle ─────────────────────────────────────────

    #[test]
    fn toggle_enable_disable_applies() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let home = Home::new();
        let (code, out, err) = home.cli(&[
            "add",
            crate::harness::fixture().to_str().unwrap(),
            "--title",
            "f",
        ]);
        assert_eq!(code, 0, "add failed:\n{out}\n{err}");
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);
        assert!(app.cfg.enabled);
        // off
        assert!(matches!(
            key(&mut app, KeyCode::Char('e')),
            KeyOutcome::Handled
        ));
        let cfg = pixcore::Config::load(&home.paths.config_file).unwrap();
        assert!(!cfg.enabled);
        assert!(!app.cfg.enabled, "app cfg stays in sync");
        assert_eq!(
            app.last,
            "pixelation disabled — originals only (still stretched/placed)"
        );
        assert_eq!(applies(&harness), 1, "{:?}", harness.requests());
        // and back on
        assert!(matches!(
            key(&mut app, KeyCode::Char('e')),
            KeyOutcome::Handled
        ));
        let cfg = pixcore::Config::load(&home.paths.config_file).unwrap();
        assert!(cfg.enabled);
        assert_eq!(app.last, "pixelation enabled");
        assert_eq!(applies(&harness), 2);
    }

    // ── export ──────────────────────────────────────────────────────────

    #[test]
    fn export_goes_to_the_download_dir() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let home = Home::new();
        let (code, out, err) = home.cli(&[
            "add",
            crate::harness::fixture().to_str().unwrap(),
            "--title",
            "f",
        ]);
        assert_eq!(code, 0, "add failed:\n{out}\n{err}");
        let id = home.entry_id(0);
        let dl = home.root.join("downloads");
        unsafe { std::env::set_var("XDG_DOWNLOAD_DIR", &dl) };
        let mut app = app_for(&home);
        assert!(matches!(
            key(&mut app, KeyCode::Char('x')),
            KeyOutcome::Handled
        ));
        unsafe { std::env::remove_var("XDG_DOWNLOAD_DIR") };
        // canvas is (0,0) before any frame → the 1920×1080 fallback
        let want = dl.join(format!("pixpapers-{id}-16-fit-1920x1080.png"));
        assert!(want.is_file(), "missing {}: {}", want.display(), app.last);
        assert!(
            app.last.starts_with(&format!("wrote {}", want.display())),
            "{}",
            app.last
        );
        assert!(app.last.contains("16 fit"), "{}", app.last);
        // export is not a mutation: no reload churn, nothing applied
        assert_eq!(app.canvas, (0, 0));
    }

    // ── refresh ─────────────────────────────────────────────────────────

    #[test]
    fn refresh_picks_up_external_changes() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let home = Home::new();
        let mut app = app_for(&home);
        assert!(app.lib.entries.is_empty());
        // a CLI add lands while the TUI has a stale (empty) view
        let (code, out, err) = home.cli(&[
            "add",
            crate::harness::fixture().to_str().unwrap(),
            "--title",
            "f",
        ]);
        assert_eq!(code, 0, "add failed:\n{out}\n{err}");
        let harness = DaemonHarness::start(&home.paths);
        assert!(app.lib.entries.is_empty(), "still stale before r");
        assert!(matches!(
            key(&mut app, KeyCode::Char('r')),
            KeyOutcome::Handled
        ));
        assert_eq!(app.lib.entries.len(), 1);
        assert_eq!(app.last, "refreshed");
        assert!(app.daemon_up, "the harness listener reads as a live daemon");
        let _ = harness.requests(); // the probe consumed its Status
    }

    // ── mid-apply safety ────────────────────────────────────────────────

    /// The plan §8 row: with the (harness) daemon answering Applies
    /// repeatedly, a full mutation sequence must leave the manifest
    /// parseable and no `*.tmp` residue — pixcore's temp+rename writes.
    #[test]
    fn mid_apply_sequence_stays_consistent() {
        if crate::harness::skip_without_cli_binary() {
            return;
        }
        let (home, id0, id1) = two_entry_home();
        crate::commands::set_current(&home.paths, &id0).unwrap();
        let harness = DaemonHarness::start(&home.paths);
        let mut app = app_for(&home);

        app.sel = 1;
        key(&mut app, KeyCode::Char('s')); // set other
        key(&mut app, KeyCode::Char('c')); // cycle back to fixture
        key(&mut app, KeyCode::Char('p')); // pixel 16 → 32
        key(&mut app, KeyCode::Char('d')); // dither none → fs
        key(&mut app, KeyCode::Char('e')); // disable
        key(&mut app, KeyCode::Char('e')); // enable
        key(&mut app, KeyCode::Char('D'));
        key(&mut app, KeyCode::Char('y')); // remove the current (fixture)

        let text = home.manifest_text();
        let lib: pixcore::Library = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("manifest unparseable after the sequence: {e}\n{text}"));
        assert_eq!(lib.entries.len(), 1);
        assert_eq!(lib.entries[0].id, id1);
        assert_eq!(lib.current, None);
        assert!(app.last.starts_with("removed "), "{}", app.last);

        // no temp residue anywhere under the home
        let mut tmp_files = Vec::new();
        let mut stack = vec![home.root.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "tmp") {
                    tmp_files.push(p);
                }
            }
        }
        assert!(tmp_files.is_empty(), "temp residue: {tmp_files:?}");
        let applies = harness
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::Apply))
            .count();
        assert!(applies >= 3, "expected several Applies, got {applies}");
    }

    // ── quit keys ───────────────────────────────────────────────────────

    #[test]
    fn quit_keys_leave_the_loop() {
        let home = Home::new();
        for (name, code) in [("q", KeyCode::Char('q')), ("esc", KeyCode::Esc)] {
            let mut app = app_for(&home);
            assert!(
                matches!(key(&mut app, code), KeyOutcome::Quit),
                "{name} must quit"
            );
        }
        let mut app = app_for(&home);
        assert!(
            matches!(handle(&mut app, KeyCode::Char('c'), true), KeyOutcome::Quit),
            "ctrl-c must quit"
        );
        // unknown keys are inert, not quits
        let mut app = app_for(&home);
        assert!(matches!(
            key(&mut app, KeyCode::Char('z')),
            KeyOutcome::Handled
        ));
        assert_eq!(app.last, "ready");
    }
}
