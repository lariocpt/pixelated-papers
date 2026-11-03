# Agent notes

Conventions for anyone (agent or human) working in this repo. The design
and build history live in [docs/PLAN.md](docs/PLAN.md) and
[docs/plan/step-*.md](docs/plan/); read the relevant step before touching
its area — each step ends in a tested increment and the "why" is there.

## Commands

```sh
cargo build / cargo test --workspace / cargo bench -p pixcore --bench pipeline
cargo clippy --workspace --all-targets -- -D warnings   # must stay green
cargo fmt --check
env -u WAYLAND_DISPLAY test/smoke.sh        # offline layer (CI-safe, deterministic)
WAYLAND_DISPLAY=wayland-1 test/verify-live.sh target/release/pixpapers
test/verify-x11.sh target/release/pixpapers # x11 layer under Xvfb (self-wraps; skips by name)
test/verify-tui.sh target/release/pixpapers # TUI on a real pty (headless env)
test/verify-wayland-fractional.sh target/release/pixpapers # headless-sway fractional buffers in Docker (skips without Docker; PIXPAPERS_BASELINE_BIN adds the legacy+RSS pair)
WAYLAND_DISPLAY=wayland-1 test/verify-waybar.sh
WAYLAND_DISPLAY=wayland-1 test/bench-daemon.sh target/release/pixpapers 25
```

Tests: 116 pixcore (config atomicity + legacy palette bool, manifest
version guard + palette/dither knobs, placement proptests + short tokens,
bar-label goldens, quantize goldens + v0.1 byte-identity goldens
(presets and `normal`), versioned variant cache, preview canvas/guard,
sixel encoder, auto-preset rule (proptest) + cache-name concreteness, the
content statistic + content-preset combine rule (determinism, floor-is-a-floor,
aliasing-is-the-documented-limitation, downscale-still-discriminates), and
the slideshow time-model (`active_index`/`secs_to_next_change`/`active_entry`,
fake clock — no sleeps). The daemon's IPC/loop logic runs against
in-process harnesses — headless is normal. `crates/pixpapers/tests/slideshow.rs`
drives the real CLI/status face over `slideshow` + the `[slideshow]` config
in a hermetic `PIXPAPERS_HOME`.
The CLI goldens run the **built** `pixpapers` binary and refuse one that
is stale relative to the sources that compile into it (`cargo test` alone
does not rebuild the plain bin for in-bin unit tests) — run
`cargo build` first after edits. Test-only modules (a file behind a
file-level `#[cfg(test)] mod x;`, e.g. the harness itself) are excluded
from the check: they never compile into the plain binary, and
`cargo build` correctly leaves its mtime alone, so tracking them would
demand a relink no build can produce.
The TUI is golden-tested headlessly (10 `TestBackend` frames at a pinned
100×30 under `crates/pixpapers/tests/golden/tui/`, regenerate with
`PIX_UPDATE_GOLDENS=1`).

## Hard rules

- **`pkill -f "pixpapers daemon"` kills your own shell.** Use `pkill -x
  pixpapers`.
- The harness shell's compositor env (`WAYLAND_DISPLAY`, `DISPLAY`) is
  **not stable** — after a server restart both were set; in an earlier
  session neither was. Offline tests strip both explicitly (smoke.sh's
  offline layer, `Home::cli`) — keep it that way: a `set`/`cycle` that
  can see a compositor auto-spawns a daemon on the *user's real* home
  (S5) and repaints the live desktop (that is how a debug `cycle -1`
  probe leaked a wayland daemon over the user's wallpaper). Live scripts
  assert the display env they need.
- **Benchmarks run against `--release` only.** Debug timings are
  misleading; `status --bench` counts real renders, nothing else.
- The live dev session is the maintainer's desktop. `test/verify-live.sh`
  and `test/verify-waybar.sh` are safe (hermetic `PIXPAPERS_HOME`, exact
  PIDs only, clean up). `wlr-randr` mode/scale changes are **not** — they
  silently reset output scale (an incident cost the user's scale 1.5). If
  you must touch outputs, restore the previous state in the same command
  and say so in your summary.
- No `git push`/tags unless the user asked. Commit often, small, with the
  measured evidence in the message when behavior claims ride on it.
- Perf/size claims in docs need a number + its source (which script,
  which commit). If a number disagrees with a claim, fix the claim
  (see docs/PERF.md's walk-backs).

## Dependencies

Discipline over convenience. v0.1 runs on: image, serde(+json), toml,
thiserror, anyhow, clap(+complete), dirs, blake3, tempfile, png,
wayland-client, wayland-protocols(-wlr), libc, log/env_logger, and
ratatui+crossterm (default-on `tui` feature; +270 KiB release, measured).
The `gui` feature (default-**off**) pulls eframe/egui/glow/winit only when
built with `--features gui` (+7.2 MiB release, measured); with the feature
off the binary is unchanged by the GUI source — the only OFF delta is the
`image` 0.25.10 patch bump eframe's dependency constraint forces (+60 KiB;
see docs/VERIFICATION.md). Dev-only: proptest, snapbox, criterion. `signal-hook` was **removed on
purpose** (its flag handlers silently never fired here — daemon/mod.rs has
the raw sigaction replacement; don't re-add it without re-testing V17).
Runtime deps still need a numbers-based justification; dev-deps just need
to be honest tools.

## Architecture in four lines

The **CLI owns all state** (atomic writes); the **daemon is a pure
renderer** that re-reads config+manifest on every `Apply`. `view` is
always computed from the **original** dimensions; the cached variant
canvas is only ever nearest-stretched into `view.dest`. Exit codes:
1 = user error, 2 = internal, **43 = daemon lost its compositor** (never
restart-on-43).

## Docs are claims

`README.md` and `docs/*.md` are evidence-backed; `docs/VERIFICATION.md`
records what was actually run (and what wasn't). New user-visible
behavior → update CLI.md + CHANGELOG.md + (if claimed) VERIFICATION.md in
the same change.

## Deferred by decision, not by omission

Still deferred: headless-sway
CI (the committed `test/verify-wayland-fractional.sh` container testbed
now bootstraps exactly that — an image + one script, skipped-with-message
without Docker — so wiring it into CI is the remaining step). Don't
"helpfully" implement the rest of these without asking; they're v0.2+
scoped on purpose. **Exact fractional buffers shipped** after it was
asked for: viewporter + fractional-scale engaged when both exist and the
scale is fractional, legacy integer path byte-for-byte unchanged
otherwise; proven in the headless-sway container (exact 3840×2160 buffer
at 1.5, RSS 70,128 kB vs the pre-feature oversample's 120,504 kB at the
same virtual output — numbers in docs/VERIFICATION.md, design in
docs/SCALING.md). (The blurred-bleed gutter shipped at 25f9a90 as `gutter-style
bleed`; the by-output-size `auto` preset shipped after it; and auto-preset
**by image content** shipped as the separate opt-in `auto-content` token —
plain `auto` still picks from the output size only, and the picture is read
only when you explicitly ask `auto-content`, and then once, on the CLI (the
daemon never re-analyses). All three were on this list until they shipped.
"Live wallpapers" shipped as a still-image
**slideshow** (`pixpapers slideshow`, render-time clock model) — but
**animated/video** wallpaper sources stay deliberately deferred: there is
no video-decoder dependency, the slideshow only cycles still images.)
"MPRIS-ish hooks" split in two and were
handled on numbers, not a hunch: the **media-key command surface shipped**
(`pixpapers next`/`prev`/`play`/`pause` as aliases for `cycle ±1` /
`slideshow on`/`off`, plus observable `current_index`+`library_len` in
`status --json`), because a key binder can call the CLI directly and that
covers the whole intent with ~9.5 KiB and no dependency. A real **D-Bus MPRIS
service stays deliberately deferred**: a throwaway `zbus = "5"` probe grew the
release binary ~20% for just a connection, pulled an async runtime against the
daemon's single-threaded `poll(2)` posture, and needed Rust 1.87 vs this
project's 1.85 MSRV — and its only real payoff (a cover thumbnail in a
GNOME/KDE widget) is moot for a still-image renderer with no cover pipeline
(see docs/plan/step-10.md).)
