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
WAYLAND_DISPLAY=wayland-1 test/verify-waybar.sh
WAYLAND_DISPLAY=wayland-1 test/bench-daemon.sh target/release/pixpapers 25
```

Tests: 99 pixcore (config atomicity + legacy palette bool, manifest
version guard + palette/dither knobs, placement proptests + short tokens,
bar-label goldens, quantize goldens + v0.1 byte-identity goldens
(presets and `normal`), versioned variant cache, preview canvas/guard,
sixel encoder, auto-preset rule (proptest) + cache-name concreteness). The daemon's
IPC/loop logic runs against in-process harnesses — headless is normal.
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
Dev-only: proptest, snapbox, criterion. `signal-hook` was **removed on
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

GUI window, live wallpapers, auto-preset **by image content** (the
shipped `auto` preset picks from the output size only — the picture
itself is never analysed), MPRIS-ish hooks, exact fractional buffers,
headless-sway CI. Don't "helpfully" implement these without asking;
they're v0.2+ scoped on purpose. (The blurred-bleed gutter shipped at
25f9a90 as `gutter-style bleed`; the by-output-size auto-preset shipped
after it — both were on this list until they shipped.)
