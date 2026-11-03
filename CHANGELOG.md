# Changelog

All notable changes. Format follows Keep a Changelog; this project follows
Semantic Versioning. The build log behind each area lives in
[docs/plan/](docs/plan/).

## [Unreleased]

### Added
- **`auto-content` pixel preset** — auto-preset **by image content**, opt-in
  as its own token so plain `auto` is untouched. Where `auto` picks a preset
  from each output's *size* alone, `auto-content` also reads the picture: it
  takes the size-derived preset as a **floor** and can only push it **coarser**
  from the image's own detail — a calm, flat graphic on a big screen pixelates
  harder than `auto` would, while a busy photo stays byte-identical to what
  `auto` picks (content can never refine below the size floor). Accepted
  everywhere a preset is — `pixpapers pixel <ID> auto-content` (aliases
  `content`, `auto content`), `pixpapers default pixel auto-content`, `export
  --pixel auto-content` (render-only), and the TUI `p` cycle — and it works for
  per-entry and default knobs. The picture is measured **once, on the
  CLI/state side** (at `pixel`/`default pixel`/`add` time) and the outcome is
  *persisted* with the entry as a `{rule, density}` probe; **the daemon is a
  pure renderer and never re-analyses**. The statistic is integer-only for
  determinism: the colour image is downscaled to a 512 px analysis edge,
  converted to Rec.601 luma, and scored by the mean absolute horizontal+vertical
  neighbour gradient (×16); `content_preset` maps density ≥64→8, ≥32→16,
  ≥16→32, ≥8→64, else 128, then `resolve_content_for` clamps it against the
  size floor. Same source bytes + same rule version ⇒ same preset. Missing or
  stale-rule probes fall back to the plain size floor, so an entry with no
  probe resolves **byte-identically to `auto`**. Backward compatible with no
  manifest/config version bump: the probe is a skip-if-`none` field, `auto`
  exports are byte-for-byte unchanged (asserted against the pinned concrete
  preset), and `status --json` gains only an additive, skip-if-absent
  `current_auto_detail` label (`auto→N @WxH` / `auto-content→N @WxH`). Cache
  filenames and the export wrote-line (`auto-content→N`) always name the
  concrete preset, never the token. Aliasing is the documented limitation: a
  1 px checkerboard averages to flat and reads as calm, so the coarse
  fallback is the conservative outcome (tested). No new runtime dependency
  (the existing `image` crate + pipeline only). Measured per-image analysis
  cost and the combine-rule evidence in
  [docs/VERIFICATION.md](docs/VERIFICATION.md#content-based-auto-preset);
  user docs in [docs/CLI.md](docs/CLI.md#content-based-auto-preset). This is
  the AGENTS.md "Deferred by decision" item, built on explicit request.
- **Media-key control surface** — `pixpapers next` / `prev` / `play` / `pause`
  as transport-agnostic aliases so a key binder (sxhkd, a niri `bind`,
  keyboardctl) drives the slideshow from transport keys **without a D-Bus
  MPRIS service**. `next`/`prev` are byte-identical to `cycle 1`/`cycle -1`
  (same `cycle_current` path); `play`/`pause` are `slideshow on`/`off`. The
  daemon stays a pure renderer — there is **no in-daemon pause flag**: `pause`
  persists slideshow-off and `play` re-derives the active wallpaper from the
  wall clock (resume lands on the current slot, no frozen epoch). Position is
  now observable: `status --json` gains additive, skip-if-absent `current_index`
  (0-based) + `library_len`, and `status` prints a 1-based `position: N / M`
  line, so a poller renders an OSD by diffing two snapshots (the pattern
  keyboardctl's `browserctl` already uses). **A real D-Bus MPRIS service
  (zbus) stays deferred and measured, not hand-waved:** a throwaway probe
  (`zbus = "5"` behind an off feature, `--release`) grew the tree
  289 → 429 nodes (+140), added **+1,061,056 B / ~20%** to the release binary
  for just a connection probe, pulled an async-io/futures reactor against the
  daemon's single-threaded `poll(2)` posture, and zbus 5.13.2 needs Rust 1.87
  vs this project's MSRV 1.85 — all rejected on numbers. The B surface adds
  9,728 B (~9.5 KiB) to the default release binary. Tests in
  `crates/pixpapers/tests/media_keys.rs`; decision and measurements in
  [docs/plan/step-10.md](docs/plan/step-10.md); user docs in
  [docs/CLI.md](docs/CLI.md#media-key-control-transport-agnostic). No new
  runtime dependency.
- **Exact fractional buffers** (viewporter/fractional-scale) — on compositors
  that offer **both** `wp_viewporter` (stable) and `wp_fractional_scale_v1`
  (staging) and a non-integer `preferred_scale`, buffers are now sized
  `round_half_away(logical × frac)` with the viewporter destination at the
  logical size and buffer scale 1: the buffer **is** the physical pixel grid
  — crisp 1:1 blocks, no compositor resample of an oversampled buffer, lower
  RSS. The rounding rule is the protocol's own (nearest, halves away from
  zero) in exact integer math; `preferred_scale` is the merged spec's u32
  numerator over denominator 120 (1.5 ⇒ 180), not the wl_fixed encoding from
  the drafts. The legacy integer path stays the fallback and is the code path
  when a protocol is missing or the scale is integer — behavior at 1.0/2.0 is
  byte-for-byte unchanged, and compositors without fractional scale (most of
  them) keep working untouched. No new dependency: both protocols live in the
  in-tree `wayland-protocols` 0.32 (fractional-scale behind its `staging`
  feature) — cargo tree 391 → 391 nodes (+0), release binary +35,200 B
  (~34.4 KiB). `status --json` outputs gain additive, skip-if-absent
  `scale_path` (`"exact"`/`"legacy"`) + `frac_scale` (`"1.50"`) per output;
  the human view annotates only the exact path (`scale 1.50 exact`). Proven
  end-to-end in a committed headless-sway Docker testbed
  (`test/verify-wayland-fractional.sh`, skips without Docker): 3840×2160 @
  1.5 → exact path, buffer 3840×2160 (not the legacy 5120×2880), stable keys;
  1.0/2.0 → legacy, unchanged; pre-feature binary at the same output
  oversamples and uses 120,504 kB RSS vs the exact path's 70,128 kB. Numbers
  and the operator live-panel checklist in [docs/VERIFICATION.md](docs/VERIFICATION.md);
  design in [docs/SCALING.md](docs/SCALING.md). The daemon stays a pure
  renderer: no new persisted state, no manifest change, no new knob.


### Added
- **Native GUI** (`pixpapers gui`) — a native egui/`eframe` control window, shipped behind a **default-off** `gui` cargo feature so the default binary keeps its exact dependency set and does not list the subcommand in `--help`. It is a client only: every action calls the same `commands::*` + `cli::apply_now` the CLI and TUI use, so a set from the window writes a byte-for-byte identical manifest to `pixpapers set` (asserted in-process in `src/gui/model.rs`), and thumbnails reuse the cached variant PNGs (no full-size render in the GUI process). The app logic lives in a headless, unit-tested view-model; `eframe` is a thin paint layer. `cargo build --release --features gui` = 12,138,488 B vs 4,907,968 B default (+7.2 MiB of eframe/glow/winit); with the feature off the GUI source adds **0 bytes** — the sole OFF delta vs the pre-GUI binary is the `image` 0.25.9→0.25.10 patch bump eframe's dependency constraint forces (+60 KiB), proven by a byte-identical control build of the HEAD source on the same lock. Gates, the control build, and one real Xvfb screenshot in [docs/VERIFICATION.md](docs/VERIFICATION.md#native-gui).
- **Live wallpapers (slideshow)** — a still-image slideshow through the existing library. `pixpapers slideshow on [SECONDS]` starts cycling the library's entries in order, `pixpapers slideshow off` stops, and `pixpapers slideshow interval <SECONDS>` changes the cadence (default 300 s). No epoch or counter is stored: the active wallpaper is derived at render time from the wall clock — `active = (unix_now / interval) % entry_count` — so the daemon stays a pure renderer, the phase survives a restart, and the poll loop simply wakes at the next boundary (idle cost unchanged when off; measured in docs/VERIFICATION.md). `status` and `status --json` show `slideshow: on · next change in Ns`. TOML: `[slideshow] enabled = true, interval_secs = 300`. Backward compatible (no manifest or config version bump; old files load unchanged). This is a deferred idea built on explicit request — it cycles *still* wallpapers only; decoding **animated/video** sources stays out (see AGENTS.md). New CLI: `pixpapers slideshow on|off|interval <SECONDS>`. See [docs/CLI.md](docs/CLI.md#slideshow-live-wallpapers).
- **`auto` pixel preset**: the pixel look can pick itself from the output's
  size instead of being hand-set per wallpaper. `auto` is accepted wherever a
  preset is — `pixpapers pixel <ID> auto`, `pixpapers default pixel auto`,
  `export --pixel auto` (render-only), and the TUI `p` cycle — and resolves
  per output at render time to **the smallest of 8/16/32/64/128 whose grid
  keeps the output's long edge at ≤ 120 blocks**: ≤960→8, ≤1920→16,
  ≤3840→32, ≤7680→64, else 128. Bigger screen ⇒ coarser blocks, so the
  blocky look stays the same size everywhere; 1920×1080 resolves to 16,
  exactly the shipped default (the 120 budget is `1920/16`). `auto` never
  picks `normal`. The rule is a pure function of the output size
  (`pixcore::Pixel::resolve_for`, proptested monotone + concrete), the
  variant cache is keyed by the *resolved* preset (filenames stay concrete,
  `auto` shares the pinned preset's cache entry), and the resolved preset is
  observable: the daemon's per-output status label (`32/fit-auto` on 4K, in
  `status` and `status --json`), the export wrote-line (`auto→32`), and the
  TUI caption (`pixel auto→8` at the preview canvas). TOML:
  `default_pixel = "auto"`; manifests store `"pixel": "auto"`. Old configs
  and manifests load byte-identically; no version bump. This is the
  AGENTS.md "Deferred by decision" item, built on explicit request — it is
  auto-preset **by output size**, the "by image content" idea stays out.
- **`gutter-style`** on a `cover-gutter` wallpaper: how the ring around the
  picture is filled. `solid` (default) is the flat `bg_color` mat; `bleed`
  fills the ring with the wallpaper's own edges, blurred (a box-downscale)
  **before** quantization and snapped to the *same* palette as the centre, so
  the ring stays blocky and pixelated instead of a photographic gaussian blur.
  The centre is unchanged and byte-identical to `solid` — only the ring
  differs. Set it per wallpaper with `pixpapers gutter-style <ID>
  <solid|bleed>`, globally with `pixpapers default gutter-style
  <solid|bleed>`, or one-off with `pixpapers export … --gutter-style
  <solid|bleed>` (render-only, not stored). In the TUI, `G` cycles it on the
  selected wallpaper. TOML: `gutter_style = "bleed"` (and
  `default_gutter_style`); manifests store `gutter_style` for the cache key.
- **Bleed layer cache**: the blurred bleed is wallpaper geometry, not ring
  width or gutter style, so it is cached once as a sibling
  `bleed-v2-<pixel>-<W>x<H>-<Q>[-<dither>].png` and reused across ring widths
  and across `solid`→`bleed` toggles; the variant cache gained a `-bleed`
  token only for the bleed style, so `solid` cache bytes and filenames stay
  byte-identical to before. Measured on the render path: bleed adds ≈4 ms to a
  1080p export and is noise at 4K (≈0–3 ms on a ≈387 ms render) — the bleed
  layer is built at `output/pixel` resolution, not full output; see
  [docs/VERIFICATION.md](docs/VERIFICATION.md).
- An interactive **before/after** comparison of the two gutter styles in the
  website (`docs/index.html`), with the gutter magnified and the ring-vs-centre
  pixel difference computed live in the browser from the demo images.

### Changed
- **MSRV claim raised 1.85 → 1.88 (measured, CI-enforced).** The declared
  floor had drifted: the resolved lockfile cannot build on 1.85 — bisected
  with real toolchains on this lockfile: **1.85.1 / 1.86.0 / 1.87.0 all
  refuse** (`image 0.25.10`, `ratatui 0.30.2` + family, `time 0.3.55`
  declare `rust-version` 1.88.0), **1.88.0 builds green**
  (`cargo +1.88.0 build --locked --workspace`). Downgrading to 1.85 was
  measured infeasible, not untried: the whole `ratatui` 0.30 line declares
  ≥ 1.86 (0.30.0 → 1.86), and `egui` 0.37 (the `gui` feature) requires
  `image ^0.25.10`, so the lockfile can never resolve an image below the
  1.88-declaring 0.25.10. The `gui` feature's own floor is **1.95**
  (eframe 0.37 declares it; 1.94 refuses, 1.95 checks green). A new `msrv`
  CI job pins the toolchain **read from the Cargo.toml claim itself** and
  builds `--locked --workspace` — an unenforced claim is how the drift
  happened. The honest floor also unlocked clippy's MSRV-aware lints
  (let-chains at 1.88, `is_multiple_of` at 1.87): 20 call sites collapsed
  per clippy's own suggestions, clippy green again.
  Measurements in [docs/VERIFICATION.md](docs/VERIFICATION.md#msrv-honesty--185--188-measured--ci-enforced).

### CI
- **Compositor gates on every push/PR** — a parallel `compositor` job in
  `.github/workflows/ci.yml` (the fast `check` job is untouched):
  `test/verify-tui.sh` headless, `test/verify-x11.sh` under Xvfb with the
  screen depth pinned to 24-bit (the workflow greps the script's
  `1280x1024x24` pin so a silent fallback to a runner default fails
  loudly — depth 16 refuses the truecolor backend), and
  `test/verify-wayland-fractional.sh` in Docker with the sway image's
  build layers cached via buildx `gha` cache (repeat runs don't rebuild
  Arch+sway; `PIXPAPERS_IMAGE_BUILT=1` makes the script use the
  pre-loaded image). `PIXPAPERS_BASELINE_BIN` is deliberately unset —
  CI does not rebuild old commits, so the legacy+RSS sub-step skips as
  the script already designs. All three gates start **soft-fail**
  (continue-on-error, per the cargo-deny precedent); graduation
  criterion: hard-fail after 5 consecutive green compositor jobs on
  main. Status and run evidence in
  [docs/VERIFICATION.md](docs/VERIFICATION.md#ci-compositor-gates--first-runs).

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
- **Published on crates.io** (2026-11-01): `pixcore` and `pixpapers`
  0.2.0 are on the registry — `cargo install pixpapers` installs the
  release from crates.io (verified with an isolated registry install
  that printed `pixpapers 0.2.0`). The workspace `pixcore` dependency
  gained an explicit `version` and both manifests gained registry
  metadata (`repository`, `keywords`, `categories`) as required
  (`c60f2ed`).
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

## [0.2.1] - 2026-11-01

### Added
- **App icon + launcher entry**: `icons/pixpapers.png` (1024 master) with
  LANCZOS hicolor sizes {16,24,32,48,64,128,256,512}, palette-matched to
  the site (#0f1114/#f7b32b/#39c08f); `contrib/pixpapers.desktop`
  (`Exec=foot -e pixpapers tui`, `Icon=pixpapers`; `desktop-file-validate`
  clean). The PKGBUILD installs the desktop entry to
  `usr/share/applications` and the PNGs to the matching
  `usr/share/icons/hicolor` paths; its `pkgver` now tracks the release
  whose checkout first carries the icon tree. The site favicon is the
  logo itself — the 48px app icon inlined as a `data:` URI (byte-identical
  to the shipped PNG; still one file, zero external requests).

### Fixed
- **`cargo test` inside a published crate tarball no longer fails** (35
  failures → 0). The suite used two files that live *outside*
  `crates/pixpapers` and therefore ship nowhere: the repo fixture
  `test/fixture.png` and the staleness guard's sibling `../pixcore/src`
  walk (a `read_dir` panic that took down every CLI golden first). The
  crate now ships its own byte-identical fixture copy
  (`tests/fixtures/fixture.png`, drift-guarded by a test); the guard
  skips the sibling walk when there is no workspace; and the 33 goldens
  that shell out to the *built* binary skip with a printed note in a
  tarball, where a plain `cargo test` produces no such artifact.
  Workspace behavior is unchanged — a missing or stale binary stays a
  loud failure there. Proven on the real 0.2.0 registry `.crate`: bare
  `cargo test` all green with the notes; `cargo build && cargo test`
  runs the goldens for real inside the tarball, against the registry
  pixcore.
