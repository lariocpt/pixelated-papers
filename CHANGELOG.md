# Changelog

All notable changes. Format follows Keep a Changelog; this project follows
Semantic Versioning. The build log behind each area lives in
[docs/plan/](docs/plan/).

## [0.1.0] - 2026-10-24

First release. Forced-pixelation wallpapers (presets `8/16/32/64/128/
normal`, default 16) on a wlr-layer-shell Background surface, niri-first.

### Added
- **pixcore** (no-wayland core): atomic TOML config + versioned JSON
  manifest; blake3-deduped library with lossless-optimized originals;
  iterative-box downscale + nearest composite + median-cut palette
  quantization; placement math (`fit`, `cover-gutter`, `cover-crop`,
  `stretch`) with proptest coverage; on-disk variant cache keyed by
  (id, preset, placement, gutter, output size).
- **CLI**: `add/list/remove/set/cycle/enable/disable`, per-wallpaper
  `pixel/placement/bg/gutter`, `default pixel|placement`, `status
  [--json|--bench]`, offline `export` (exact daemon composite path),
  `optimize`, `completions`.
- **Daemon**: single-threaded poll loop (wayland + IPC + signals); one
  double-buffered shm Background surface per output; auto-spawn from any
  mutating command; IPC JSON-lines (`Apply/Status/Quit`); graceful IPC
  `stop`; exit 43 on compositor loss.
- `pixpapers status --bench`: daemon-self-timed switch/render latencies,
  variant-cache hit rate, RSS/fd — zero cost when idle.
- Evidence tooling: criterion benches, `variant_sizes` example,
  `test/smoke.sh` (offline+live), `test/verify-live.sh` (V11/V13/V16/V17),
  `test/bench-daemon.sh`; docs/PERF.md, docs/VERIFICATION.md.
- v1.0 backlog documented in README ("Deliberately not in v0.1").

### Fixed (found during step-8 verification)
- IPC clients that sent no newline stalled the daemon loop (read timeout
  per client); daemon now answers EOF/empty requests immediately (V16).
- SIGTERM/SIGINT silently never reached the shutdown flag through
  signal-hook on the dev machine; replaced with a raw async-signal-safe
  sigaction handler and one fewer dependency (V17).
- Layer surfaces now carry an empty input region (belt on lax
  compositors, V9).

### Known limitations (v0.1, documented)
- Multi-output & physical hotplug: structurally handled, hardware-untested.
- Fractional scale renders via the legacy oversampled path (exact-size
  buffers via viewporter/fractional-scale — planned, still deferred;
  see [docs/SCALING.md](docs/SCALING.md)).
- `stop`/daemon must be re-run per session (exit 43 on session end is
  intentional).
- 72 h soak (V15) pending.

## [0.2.0] - 2026-11-01

v0.2: retro palettes & dithering, an X11 backend, a terminal UI and a
waybar toolbar module — the four PLAN-30-ct workstreams, landed as
steps S1–S9. Every number below names the script and commit that
produced it; the full record is in
[docs/VERIFICATION.md](docs/VERIFICATION.md) (§S1–S9) and
[docs/PERF.md](docs/PERF.md).

Not in this entry (still deferred — README "Deliberately not in v0.2",
AGENTS.md deferred list): exact fractional buffers (the plan stands in
[docs/SCALING.md](docs/SCALING.md)), a GUI window (the waybar button's
`on-click` is the slot it flips into), live wallpapers, auto-preset,
MPRIS-ish hooks, headless-sway CI. Multi-output/multi-monitor behavior
remains field-report-driven (VERIFICATION V4/V5), as before.

### Added
- pixcore: quantize stage — palette modes `auto / nes / gb / cga / ega /
  vga / gen16` × dithers `none / fs / atkinson / bayer2 / bayer4 / bayer8`,
  deterministic integer pipeline (12 golden cells checked in).
  Config `palette` is now a string (`auto` default; legacy `true`/`false`
  TOML bools still load) and config/manifest gain `dither`; variant cache
  keys are v2 (v0.1 caches invalidate — they are disposable). `auto` +
  `none` renders **byte-identical to v0.1**, verified against goldens
  captured from the pre-change tree.
- pixcore: the `normal` preset now honors `palette`/`dither` (S3.5 — S3's
  quantize stage never ran for identity pixels). Settled rule: the
  plain-crop early return survives **only** for `auto` + `none` (the v0.1
  default, byte-identical — encode still does its `median_cut(255)`);
  every other (palette, dither) combination quantizes the **full-size
  canvas** before the unchanged encode. `off` + dither is inert (no
  palette to diffuse into); named + `none` is full-canvas nearest
  (encode's re-map is idempotent). Measured (PLAN-30 S3.5,
  `test/bench-normal.sh` @ `4f58e63`, numbers in
  [docs/PERF.md](docs/PERF.md)): 4K one-time `vga`+`fs` 2.4 s /
  `auto`+`fs` 7.4 s — over the plan's 1 s line, median-cut-dominated,
  documented and kept; cached variant PNGs 1080p 98–242 KB / 4K
  389–945 KB for `auto`/`vga`/`nes`.
- pixpapers CLI: per-wallpaper `palette <ID> <M>` / `dither <ID> <D>`
  knobs and `default palette|dither` (entries keep `None` = inherit);
  `list`/`status` show the new knobs. `export` gains `--palette`/`--dither`
  render-only overrides: named modes write a palette PNG (PLTE = the
  table deduped by first occurrence, a transparent placeholder at index
  0, and the bg colour merged in when absent); `off`/`auto` stay RGBA.
  First tests in the crate: CLI round-trips against a hermetic
  `PIXPAPERS_HOME` plus an in-process daemon-socket harness.
- **X11 backend** (PLAN-30 §B.2/§B.3): the same daemon, same CLI, same
  look on X11 — one wallpaper on every screen's root window, composited
  through the identical `composite_into` path and PutImage'd (ZPixmap,
  depth 24; no Xv, no GPU). Selection: `daemon.backend = "auto" | "layer"
  | "x11"` (default `auto`: `WAYLAND_DISPLAY` set ⇒ layer, else `DISPLAY`
  set ⇒ x11, else exit 1 naming both — a wrong explicit guess is a loud
  user error, never a silent fallback), plus `daemon --backend <b>` for
  testing. `status` reports the active backend (additive `backend`
  field). A non-24-depth root is a loud exit 1. Daemon start failures
  (no session, bad depth, refused connection) now exit 1 (user error)
  instead of 2. An X server loss (connection error) is the same
  **exit 43** contract as the Wayland compositor loss — teardown,
  socket/pidfile removed, never restart on 43 (machine-tested:
  `test/verify-x11.sh` kills the Xvfb out from under the daemon).
  Ships behind the default-on `x11` cargo feature
  (`--no-default-features` = wayland-only slim build; +122 KiB binary).
- Measured (PLAN-30 S3, numbers in [docs/PERF.md](docs/PERF.md), scripts
  `test/bench-dither.sh` / `test/bench-sizes.sh` @ `45c9aa6`): dithering
  in canvas space costs ≤ +5.6 ms one-time on the largest preset canvas
  (bayer8 ≤ +0.4 ms); the one-time full-res `vga`+`fs` case is 0.61 s
  (`auto`+`fs` 1.82 s — over the plan's 1 s line, median-cut-dominated,
  documented and kept); named palettes land 16–78% *below* auto (none
  bloats); the v2 default path is byte-identical to v0.1 at fixture
  scale (blake3-pinned), not just on the goldens.
- **TUI core** (PLAN-30 §C.1–C.3): `pixpapers tui` — library list +
  offline preview + status bar, single-threaded crossterm loop, alternate
  screen restored via a panic-safe `Drop` guard. The preview is literally
  a small export: `pixcore::variant::render_offline` is now the one
  render path shared by `export` and the TUI (export bytes proven
  byte-identical across the refactor). Terminal image protocols detected
  at startup (kitty graphics APC / iTerm2 OSC 1337 / sixel / none) with
  `PIX_PAPERS_IMAGE` override; a pure sixel encoder lives in pixcore
  (verified against libsixel 1.8.6); 256 KB inline-PNG guard refuses
  with the reason. Ships behind the default-on `tui` feature
  (ratatui + crossterm, +270 KiB release binary, measured). Test
  plane: 7 ratatui `TestBackend` goldens at a pinned 100×30 +
  detection/emit/canvas unit tests; live rows in
  [docs/VERIFICATION.md](docs/VERIFICATION.md).
- **TUI actions** (PLAN-30-ct §C.4–C.5): the full key table on
  `pixpapers tui` — add-by-path (`a`), set (`s`), cycle (`c`),
  pixel/placement/palette/dither cycles (`p`/`l`/`t`/`d`), bg/gutter
  input lines (`b`/`g`), remove behind a confirm line on the single key
  `D` (`remove <id>? y/N` — `rm` would collide with `r`, refresh),
  pixelation toggle (`e`), export to the download dir (`x`), refresh
  (`r`). Every action runs the same `commands::*` function the CLI
  subcommand calls and the same Apply seam (invariant #1); status lines
  carry the CLI's own wording, built from the same data the CLI's
  prints use — not from stdout capture (libtest intercepts `println!`
  at the Rust level, so an fd-level capture is empty in-process; the
  alt-screen drain of fd 1+2 stays so the CLI's own prints never smear
  the frame). 30 unit tests (temp `PIXPAPERS_HOME` + throwaway socket
  listener; add-by-path proven byte/hash-identical to CLI `add`; remove
  leaves no dir/manifest residue) and 3 new `TestBackend` goldens (10
  total) at the pinned 100×30; live pty row — `test/verify-tui.sh`
  drives the full key table on a real pty against a scratch home
  (headless: no daemon spawn), then asserts the post-exit manifest and
  config truth — in [docs/VERIFICATION.md](docs/VERIFICATION.md) §S7.
- **bar + waybar module** (PLAN-30-ct §D): `pixpapers bar` — one line,
  always exit 0, for a toolbar: `▞ <title> · <pixel>/<placement-short>`
  with a current entry (even with the daemon dead — config+manifest
  truth through the daemon's own resolution chain, so the label never
  shows a look the daemon would not render), `▞ pixpapers — set one`
  with no entry and the daemon alive, `▞ pixpapers · daemon down` with
  the socket dead. Pure formatting in pixcore; `Placement::short`
  keeps the label narrow at toolbar heights; `cycle` takes a signed
  step, so `on-scroll-down cycle -1` works. `contrib/waybar/` ships
  the module for waybar ≥ 0.14 (tested 0.15.0: `custom/pixpapers`,
  `exec "pixpapers bar"`, `interval 30`, `${TERMINAL:-foot} -e
  pixpapers tui` on click, direction-aware wheel); `format` stays
  `"{text}"` — the positional `"{ }"` idiom is rejected by fmt 12,
  which the tested 0.15.0 links, and the module then silently hides.
  No daemon changes, no new IPC, no new dependencies. 4 hermetic
  goldens + offline `smoke.sh` checks; live row (render + flip real,
  click/scroll named skips with exact-command substitutes) in
  [docs/VERIFICATION.md](docs/VERIFICATION.md) §S8.

### Config (all additive — old configs and manifests load unchanged)
- `palette` is now a string (`auto` default = the v0.1 median-cut look);
  legacy `palette = true/false` TOML bools still load (true→auto,
  false→off) and new saves write the string.
- `dither` is new (`none` default = the v0.1 look).
- `[daemon] backend` is new (`auto` default: `WAYLAND_DISPLAY` ⇒ layer,
  else `DISPLAY` ⇒ x11, else exit 1 naming both; explicit `layer`/`x11`
  override).
- Manifest entries gain optional `palette`/`dither` (`None` = inherit
  the config default); old manifests without the fields parse unchanged.

### Dependencies
- `ratatui` + `crossterm` behind the default-on `tui` feature: **+270
  KiB** release binary (measured).
- `x11rb` behind the default-on `x11` feature: **+124,960 B** (+122
  KiB, +2.9%) release binary (4,304,640 → 4,429,600 B, strip+lto);
  `--no-default-features` = wayland-only slim build.
- No new dependencies for the palettes workstream (median-cut already
  ships; dithering is hand-rolled integer math) or for the waybar
  module (sh + config).

### Changed
- **x11 stop clears the roots** (`1e79d00`): a graceful stop (and the
  best-effort exit-43 teardown) now clears every X root window to solid
  black, so a stopped daemon leaves no stray wallpaper frame on the X
  server (previously the last frame persisted — e.g. behind X11 windows
  on Xwayland — until something else drew the root). Teardown cost is
  one batched fill per screen — measured stop latency unchanged
  (2/7/7/7/8 ms vs 2/2/8/8/8 ms before, 1024×768 Xvfb, release;
  `test/verify-x11.sh` asserts AE=0 against a black reference). A hard
  kill still skips teardown, as before.

### Fixed
- **Exit-code audit** (S9 task 7, `dde4d1f`): user conditions now exit
  **1** instead of 2 — `cycle` on an empty library (the violation S8
  recorded), unknown ids on `set` / the per-entry knob commands /
  `export`, and `export` with an unparseable size. Messages are
  byte-identical; only the classification changed. Reported, not fixed
  (outside the authorized scope — VERIFICATION §S9): `export -o` write
  failure (io) stays 2; clap usage errors stay 2 (framework
  convention); a second foreground `daemon` while one runs stays 2
  (bare `bail!` since v0.1 `10dbc8b`) — whose doc claim ("exits 1")
  was the bug, corrected doc-side in the S9 pass.
