# Verification record — v0.1 step-8 matrix + the v0.2 workstreams (S1–S9)

Every claim in README/PLAN checked against a real compositor. The v0.1
matrix below was recorded 2026-10-24 on the dev machine, **niri 26.04
(8ed0da4)**, single output DP-1 (Samsung U28E590) at 3840×2160@59.997,
niri **fractional scale 1.5** (logical 2560×1440, legacy
`wl_output.scale` = 2 ⇒ our buffers 5120×2880). Release build. Scripts:
`test/smoke.sh` (offline+live layers), `test/verify-live.sh`
(V11/V13/V16/V17), `test/bench-daemon.sh` (V1/V2 timings + footprint).
Re-running any of them re-produces its rows.

The v0.2 workstream sections follow the same rule (S1–S3.5 before the
S4 re-run, S5 after it, S6–S9 at the end): every number names the
script that produced it and the commit it was measured at, and a named
skip says exactly what is missing and why.

Legend: ✅ passed · ⚠️ passed with limitation · ⏸ manual/deferred · 🐞 bug found+fixed

| # | Scenario | Result | Evidence / notes |
|---|----------|--------|------------------|
| V1 | cold `set` auto-spawns ≤ 1 s | ✅ | **91 ms** (bench-daemon, release; spawn + first variant + render + commit) |
| V2 | switch A↔B ×25 (n=50) | ✅ | hot p50 **23 ms**, p95 **45 ms**, max 45 ms; daemon self-timed median 20 ms. Double-buffered: buffer written *before* attach+commit ⇒ no torn frame observed |
| V3 | 6 presets × 4 placements | ✅📸 | `docs/screens/step8/grid-6x4.png` (export path = the daemon's exact `composite_into` codepath); live spot-checks of combinations via V13 |
| V4 | multi-output mixed | ⚠️ | **single-output dev host.** Per-output surfaces/buffers/configure are independent by construction, but multi-monitor is *untested* — first field reports wanted (see COMPOSITORS.md) |
| V5 | hotplug unplug/replug | ⚠️ | niri refuses disabling the **only** output ("failed to apply configuration") — true hot-unplug not scriptable here. Surface-lifecycle code exercised via V6 reconfigure storms; `wl_output` GlobalRemove teardown implemented but **not run live**. Accepted limitation, v0.2 |
| V6 | mode change (res/refresh/scale events) | ✅ | wlr-randr 3840×2160→1920×1080→3840×2160, scale events 2→1: daemon reconfigured and re-rendered each time, **8–48 ms** (daemon log) |
| V7 | fractional scale | ✅⚠️ | the dev session *is* fractional: effective 1.5, legacy path renders correct output (buffer 5120×2880 oversampled, niri downscales — visually crisp at 16px blocks). 1.0/2.0 verified during V6. Exact-size buffers (`wp_viewporter` + `wp_fractional_scale_v1`) = v0.2 backlog, SCALING.md |
| V8 | niri overview/backdrop | ⏸ | needs interactive overview + visual check; recommended config stanza documented in COMPOSITORS.md, not screenshot-proofed |
| V9 | no input interception | ✅ | Background layer + `KeyboardInteractivity::None` + empty input region (wl_region with zero rectangles) ⇒ protocol-excluded from input; live session used normally around the running daemon throughout V1–V17 |
| V10 | fullscreen window above wallpaper | ⏸ | manual (game/video not scriptable); z-order follows from Background layer semantics |
| V11 | `kill -9` → next `set` recovers ≤ 1 s | ✅ | **54 ms** respawn+apply (verify-live.sh) |
| V12 | niri restart → exit 43 | ⏸ | would kill the dev session. Exit path implemented (compositor-loss → teardown + socket/pid removal + **exit 43**), not exercised live. TROUBLESHOOTING documents it |
| V13 | all knobs + `disable`/`enable` live | ✅ | pixel/normal/placement/bg/gutter ×7 changes + switch off/on: no errors, no blanking (re-render replaces previous frame), verify-live.sh |
| V14 | `stop` | ✅ | surfaces gone, daemon gone, socket/pid gone (smoke.sh live layer) |
| V15 | 72 h idle soak | ⏳ | **pending.** fd/RSS spot-checks healthy (7 fds, 4.4 MB idle / ~121 MB displaying 5K buffers); soak start = `nohup pixpapers daemon` over a weekend, then compare `status --bench` |
| V16 | 1000× IPC fuzz (valid+garbage+truncated) | ✅🐞 | 1000/1000 connections answered; manifest+config sha256 unchanged. **Found:** clients that never sent a newline stalled the loop 2 s each (read timeout) — daemon now answers on EOF/empty instead of closing silently |
| V17 | SIGTERM/SIGINT graceful stop | ✅🐞 | **Found:** `signal-hook`'s `flag::register` installed handlers that *silently never fired on this machine* (SIGTERM/SIGINT ignored; kill -9 only way out; SIGCgt says caught, hook flag never flips; raw `libc::sigaction` handler in the same binary fires fine). **Fix:** dropped signal-hook, raw async-signal-safe handler (single atomic store). Teardown + socket/pid removal in 100 ms |

## What the bugs above changed in code

1. **EOF/empty replies** (`daemon/mod.rs::handle_client`) — always answer so
   a client never waits out its own timeout; found by the V16 fuzzer.
2. **Raw signal handler** (`daemon/mod.rs`) — replaces signal-hook entirely
   (dependency count −1). Root cause on this box not chased upstream: the
   empirical difference was reproduced with a 30-line standalone test.
3. **Empty input region** (`daemon/backend/layer.rs`) — belt for lax compositors
   on V9; redundant on niri, protocol-cheap.

## Cross-compositor smoke (step 8.2)

None installed on the dev machine → **untested**, honestly. layer-shell v1
is the same wire protocol everywhere (niri, sway, hyprland, river, …) and
gesso uses the identical setup, so expectations are identical; README says
"designed for niri (primary dev target), other wlroots-family compositors
should work — untested". v0.2: headless sway smoke in CI.

## Accepted limitations for v0.1.0 (filed as v0.2 work)

- Multi-output and physical hotplug behavior (V4/V5): single-output host.
- niri-restart exit-43 (V12) and fullscreen/overview (V8/V10): manual, not
  machine-checked.
- Fractional-scale exact buffers (V7): correct via legacy oversampled path.
- 72 h soak (V15) pending a weekend.

## S1 — palettes & dithering: model, quantize, cache key v2 (2026-10-31)

PLAN-30-ct workstream A (`82dba3d` → `344f302`); pixcore tests 37 → 63
across the step, gate green at each commit.

**Model + tables** (`82dba3d`): PaletteMode {Off, Auto, Nes, Gb, Cga,
Ega, Vga, Gen16} + Dither {None, FloydSteinberg, Atkinson, Bayer2,
Bayer4, Bayer8} with lowercase serde tokens, Display/FromStr. Console
tables with provenance pinned by unit tests: NES = the 64-entry 2C02
master table — the canonical 192-byte `.pal` (a 192-entry NES table
does not exist; the plan's "nes 192" was the `.pal` byte size, retro
`f55170a`); GB DMG greens; CGA 16 (brown at 6, cross-pinned to VGA's
first 16); EGA 64 (classic 4×4×4); VGA 256 (BIOS/Allegro default
palette); gen16 = median-cut-16 snapped onto the RGB555 grid
(`snap_rgb555`, applied in the quantize stage — not a table).

**Quantize stage** (`4873099`): `pipeline::quantize(img, mode, dither)`
— Off is identity; Auto = median-cut + nearest (the exact v0.1 call
sequence when dither is None); Gen16 = median-cut-16 snapped to
RGB555; named modes map onto their fixed tables. All diffusion is u16
integer math (row-major, truncated weights) — no floats anywhere in a
dither path (determinism contract). 12 golden cells — auto × all
dithers + every named mode × none — under `tests/golden/quantize/`
(regenerate deliberately via `PIX_UPDATE_GOLDENS=1`; a re-run verified
byte-equal).

**Config/manifest/resolve** (`22fd5d5`): `palette` (default `auto` =
the v0.1 median-cut look) + `dither` (default `none` = the v0.1 look)
knobs; legacy `palette = true/false` configs still load through the
serde shim (true→auto, false→off; new saves write the string); old
manifests without the fields parse unchanged.

**Cache key v2** (`414c4d1`): variant paths now carry the look —
`v2-{pixel}-{placement}[-g g]-{W}x<H>-{Q}[-{D}].png`, every (Q,D) cell
its own file (proptest pins all 48 distinct). `PIPELINE_VERSION` 1→2:
v0.1 caches invalidate, no migration code (the cache is disposable).
The encode re-runs the exact v0.1 `median_cut(255)` + index-0-
transparent scheme for Auto/Off, so those bytes are frozen; named
modes encode exactly their table entries, deduplicated (NES's 12×
black collapses — PLTE pinned by decoding the GB/NES variants).
**v0.1 byte-identity guard**: four capture files taken from the
untouched v0.1 tree (worktree at `29ef2b4`, public API only) —
photo128x80/gradient64 × auto/off — compared byte-for-byte by
`auto_none_matches_v01_bytes`.

**Goldens checked in** (`309d970`): `crates/pixcore/goldens/` had been
silently `.gitignore`d (the repo's image exception is
`crates/**/tests/golden/**`); the 12 quantize cells + 4 v0.1 captures
moved so `git ls-files` lists all 16.

**Acceptance contract** (`344f302`): the A.4 palette-subset contract as
a proptest — xorshift noise images (every RGBA byte reachable, alpha
included) through all 8 modes × 6 dithers; output colors ⊆ the mode's
acceptance set. (Off is identity, so its low-alpha pixels pass
through untouched — only quantizing modes normalise alpha ≤ 8 to
[0,0,0,0]; the assertion splits accordingly.) Docs wiring `2e089bf`
(README mode list, CHANGELOG, AGENTS count).

## S2 — CLI surface for the new knobs (2026-10-31)

PLAN-30-ct workstream A.3–A.4 CLI half (`8c3560e` → `6e05665`).

**Knob commands** (`8c3560e`): `pixpapers palette <id>
<off|auto|nes|gb|cga|ega|vga|gen16>`, `pixpapers dither <id>
<none|fs|atkinson|bayer2|bayer4|bayer8>` and `default palette|dither`
— same behavior contract as the existing knobs: atomic write; Apply IPC
+ auto-spawn only when the edited entry is current; entries keep
None = inherit the config default. No new IPC surface, no daemon
change.

**Export overrides** (`675b12c`): `--palette`/`--dither` as render-only
overrides (never persisted) on the entry→overrides→config chain. Also
fixes a v0.2-1 bug in the same function: the resolved palette was
unconditionally clobbered with the config default, ignoring the
entry's stored knob. Named modes write true palette PNGs (exact table,
deduped by first occurrence, index-0 transparent placeholder, bg color
merged when absent); off/auto stay plain RGBA, so the v0.1 default
export shape is byte-identical.

**list/status columns** (`25e4c81`): list gains palette/dither columns
(same `*` inherit marker as pixel/placement); the status policy line
ends with `· palette {p} · dither {d}` and `status --json` gains
`default_palette`/`default_dither` (CLI-side merge — the daemon's
StatusReport shape is untouched).

**Variant label** (`6e05665`): the per-output label now carries the
look — `{pixel}/{placement}-{Q}` (Q always) / `-{Q}-{D}` (D only when
dither ≠ none) — display-only; grep-confirmed no script or harness
parses it.

**First tests in the pixpapers crate** (`f5e09a9`): 8 — the in-process
`DaemonHarness` (throwaway socket listener recording every Request;
S7's TUI tests reuse it) + subprocess round-trips against the built
binary with `WAYLAND_DISPLAY` removed (headless-safe). They cover:
every token round-trips with the edit line verbatim and the manifest
bytes carrying the value; exactly one `Apply` — and it is last, after
the `daemon_alive` Status probe — on a current-entry edit, zero on a
non-current one; the VGA+fs export PLTE is exactly [placeholder
000000] + first-occurrence dedup of the VGA table (247 entries) with
sampled rows decoding to table colors; off/auto exports are plain
RGBA. CLI.md updated in `411e944` (checked against `--help`).

Gate: workspace tests green (63 pixcore + 8 pixpapers), clippy `-D
warnings`, `fmt --check`, offline `smoke.sh`.

## S3 — measured evidence: dither cost + palette sizes (2026-10-31)

PLAN-30-ct workstream A.4 bench half (`89fd275` → `d335cd6`). Every
number in the resulting PERF.md sections is the scripts' own verbatim
output; the photo fixture is deterministic and mirrors
`examples/variant_sizes.rs`, so the numbers line up with the existing
tables.

**Dither cost** (`89fd275` → `6d7d90d`): `test/bench-dither.sh` drives
the test-only helper crate `test/bench-quantize/` (own workspace, not
a member; release profile mirrors the root one, encoder versions
pinned by its committed Cargo.lock) through `pipeline::quantize`
timings: FS and bayer8 on the 120×68 / 240×135 / 480×270 preset
canvases plus the one-time full-res 1920×1080 case. Result: ≤ +5.6 ms
one-time on the preset canvases (then cached); the 1080p full-res
budget row (vga+fs 0.61 s) sits inside the plan's 1 s line.

**Palette sizes** (`45c9aa6` → `6d7d90d`): `test/bench-sizes.sh`
measures variant bytes + blake3 for auto/named palettes on the 1080p/4K
fixtures, then rebuilds the v0.1 auto baseline from a throwaway git
worktree at `29ef2b4` (parent of the first S1 commit — the same base
the byte-identity goldens came from) via a probe example dropped into
the worktree, removed on exit. The join: v2 auto+none is
byte-identical to v0.1 auto at fixture scale, not just on the goldens.
Named-palette deltas vs auto: gb −77% … vga −18% — no palette bloats a
variant (README size table, `92b3534`).

**Walk-back** (`6d7d90d`): the plan's "normal" bullet is corrected —
`normal` is palette-*encoded*, never quantized/dithered (the quantize
stage never ran on it; PLAN-30 A.1 described the opposite — S3
measures, does not change behavior). That gap becomes S3.5.

CHANGELOG measured bullet `d335cd6`. Gate: workspace tests/clippy/
fmt/offline smoke green.

## S3.5 — `normal` honors palette/dither (2026-10-31)

Settled rule (`0fc5d72` plan, `1318d60` code): for an identity pixel
(`normal`), the plain-crop early return now applies **only to
(Auto, None)** — the v0.1 default, whose bytes stay identical (encode
still does its `median_cut(255)`, exactly as v0.1 did). Every other
(palette, dither) combination runs the crop through the same
`pipeline::quantize` stage the pixelated presets use, before the
unchanged `encode_variant`: (Off, \*) is identity — dither has no
palette to diffuse into, same crop bytes as v0.1 `palette = false`
normal; (named, none) is full-canvas nearest (encode's re-map is
idempotent on-table); (\*, dither≠none) is full-canvas quantize +
diffusion/ordered dither. No new stages, no new encode logic,
pixelated path untouched.

**Tests** (`28da6dd`): `auto_none_matches_v01_bytes` extends the v0.1
identity fixture from pixel 8 to the normal preset — 4 new goldens
captured from the untouched v0.1 worktree at `29ef2b4` (Fit, 320×180,
sha256-pinned in the commit); a proptest pins every named mode × every
dither's output colors inside the table over arbitrary pixel content;
dither changes bytes but stays in-palette (vga+fs ≠ vga+none, colors
still in the VGA table); two-run determinism per new normal cell;
end-to-end goldens for {auto-fs, vga-fs, nes-none, gen16-none} under
`tests/golden/normal/`; the v2 key-shape check for normal. pixcore
63 → 68 (AGENTS count).

**Bench + PERF** (`4f58e63`): `test/bench-normal.sh` — 4K one-time
full-canvas quantize medians: vga+none 2124 ms, vga+fs 2448 ms,
auto+none 7311 ms, auto+fs 7405 ms (4× the pixels of S3's 1080p rows ⇒
medians scale 4.0–4.1×). vga+fs 2.45 s and auto+fs 7.4 s are over the
plan's 1 s line at 4K — documented with the number and kept (one-time,
then cached; the risk row's call). Normal-variant PNG sizes
(blake3-pinned): 1080p auto 247 350 B / vga 191 888 / nes 100 081; 4K
auto 967 337 / vga 727 276 / nes 398 673.

**Docs** (`694a248`): CLI.md rows (on `normal` the look runs on the
full-size canvas, one-time then cached; auto+none the v0.1-identical
default; dither inert with palette off), the PLAN A.4 bench line
updated to the measured 3840×2160, CHANGELOG bullet.

Gate (this tree): workspace 68 pixcore + 8 pixpapers, clippy `-D
warnings`, `fmt --check`, offline `smoke.sh` — all green.

## Re-run after S4 (backend seam) — 2026-10-31

S4 (PLAN-30 §B.1) moved `daemon/wayland.rs` to `daemon/backend/layer.rs`
behind the new `Backend` trait and reshaped the supervisor around it.
Gate: no behavior diff — switch latencies within run-to-run noise, V-matrix
re-run recorded here. The rows above are the untouched 2026-10-24 records;
everything below is the same host (niri 26.04, single output, fractional
1.5) and the same scripts.

**Bench brackets** — `test/bench-daemon.sh target/release/pixpapers 25`:

| run | cold spawn+apply | preset change | hot p50 | hot p95 | hot max | self-timed median | self-timed max | variants | RSS · fds |
|-----|-----------------|---------------|---------|---------|---------|-------------------|----------------|----------|-----------|
| before #1 (694a248 binary) | 91 ms | 32 ms | 23 ms | 25 ms | 45 ms | 20 ms | 77 ms | 49/3 (94%) | 121224 KB · 9 (7 after sweep) |
| before #2 (worktree @ 694a248) | 91 ms | 34 ms | 23 ms | 25 ms | 43 ms | 20 ms | 77 ms | 50/3 (94%) | 120948 KB · 9 (7) |
| before #3 (worktree @ 694a248) | 93 ms | 31 ms | 24 ms | 28 ms | 43 ms | 20 ms | 79 ms | 49/3 (94%) | 120916 KB · 9 (7) |
| **final (double-pump)** | **89 ms** | **31 ms** | **24 ms** | **24 ms** | **25 ms** | **21 ms** | **77 ms** | **52/3 (95%)** | **121104 KB · 9 (7)** |

Every final number falls inside — mostly at the better edge of — the before
brackets. Same numbers, same logs.

**Interlude — the single-pump trap (rejected).** The first cut of the
supervisor called `pump()` once per tick, *after* the poll — the literal
reading of the new trait. That is measurably wrong: the first poll then ran
*before* the first flush, so the cold-start handshake (registry → globals →
binds → configure) slipped one tick per hop. The CLI's `set` (50 ms
granularity) landed its `Apply` at t≈50, *before* the compositor's
`configure` had been dispatched — the daemon answered an instant no-op ack
(`set` returned **53/52 ms** vs 91/91/93 before) while the real 38 ms render
finished in the background (first frame at t≈96 ms instead of t≈46 ms).
Instrumented timelines (`t=` markers from both binaries, same session state)
proved the mechanism: pre-seam, the pre-poll flush sent `get_registry` and
niri answered *within the same tick*, so the whole handshake was done by
tick 2 and `Apply` hit a configured surface — a redundant 38 ms cache-hit
re-render, `set` back at t≈90. Distribution was cleanly separated:
91–92 ms ×5 vs 52–53 ms ×5, reproduced across warm/cold session states.

**Resolution.** The supervisor now pumps **before and after** the poll:
`backend.pump()` → `poll2` → `backend.pump()`. §B.1 fixes the *trait*, not
the loop shape, and the gate is zero behavior diff — so the pre-poll pump
restores the pre-seam flush-before-poll ordering (flush the previous tick's
queued requests, get the compositor's answer within this tick's poll). Its
read only drains data already waiting on the fd; a would-block is a no-op.
Same messages, same order, same tick; cost is one would-block read and two
mutex ops per idle tick. Cold `set` returned to 91 ms and the bench to the
before brackets (final row above). Do not "simplify" this back to a single
post-poll pump.

**V-matrix re-run** (2026-10-31, same host/niri as the table above;
`test/verify-live.sh target/release/pixpapers`): V11 `kill -9` →
respawn+apply **91 ms** (2026-10-24: 54 ms — respawn is machine-load
dependent; the cold *spawn+first-apply* path in the bench bracket is the
stable number and it is unchanged) · V13 all knobs + disable/enable live, no
errors, no blanking · V16 **1000/1000** fuzzed IPC requests answered,
manifest/config hashes unchanged · V17 SIGTERM → teardown + socket removal
**100 ms** → `ok`.

## S5 — X11 backend (2026-10-31)

PLAN-30-ct §B (`85e59ed` → `1821986`). Policy first (`8b2891d`): S5
**hard-stops** on a machine without xvfb — the honest named skip is for
CI machines, not for this one; an unverified x11 backend is not a
mergeable S5. This box got `xvfb` installed (user-approved) and the
full suite ran.

**Backend** (`85e59ed`): x11rb (pure-Rust XCB, pollable fd) as an
optional feature `x11`, on by default; `--no-default-features` keeps a
wayland-only slim build — measured delta 4,429,600 B default vs
4,304,640 B slim = **+124,960 B (+2.9%)** (release, strip+lto). One
wallpaper on every screen's root (mirrors the layer backend's
multi-output behavior); full-root XRGB8888 client buffer in the
display's byte order; composite via the *same* `composite_into` the
layer backend uses (invariant #2); `PutImage` ZPixmap depth 24 in
BigReq-aware row bands. Non-24-depth root ⇒ loud exit 1 at connect.
Expose ⇒ re-render; RFPWM RESIZE ⇒ re-render at the new size (react
only, never initiate, no RFPWM emission). Connection error ⇒
`connection_lost` ⇒ exit 43 (same contract as wayland). Daemon start
failures (no session, bad depth, refused connection) classify as user
error exit 1 (`DaemonStartupError`; was 2 for the layer path). Status:
bench counters moved behind the trait, `StatusReport` gains the
additive `backend` field, `--backend` override. Verified by a manual
xvfb round-trip at the backend commit (set → root pixmap AE=0 vs
`export`, clean stop), then pinned by the script below.

**Auto-detect** (`b549390`): `daemon.backend = auto|layer|x11`
(default auto); auto order `WAYLAND_DISPLAY` ⇒ layer, else `DISPLAY` ⇒
x11, else exit 1 naming both; a wrong explicit guess is a loud user
error (exit 1), never a silent fallback chain; auto-spawn now also
fires under `DISPLAY` alone. Headless unit tests: the `pick_auto` env
matrix (WAYLAND_DISPLAY × DISPLAY truth table), backend-name parsing,
the depth-24 gate (24 ok; 16/32 refused with the loud message).
Boundary counts: pixcore 68 (S3.5) → 76 (S6 gate) and pixpapers 8 → 23
— S4 adds no tests (seam only), so the growth is S5's.

**Side fix** (`20e73ce`): S5 made `DISPLAY` sufficient for
auto-spawn, but the offline layers only stripped `WAYLAND_DISPLAY` —
on a host with a live X session that let test subprocesses auto-spawn
real daemons bound to deleted temp homes (two such debug daemons were
found running and killed with `pkill -x pixpapers`). `smoke.sh`
`run()` and the harness `cli()` now strip both, restoring the
documented promise: offline commands record but never spawn.

Live rows:

| Row | What ran | Result |
|---|---|---|
| xvfb suite (**real**) | `test/verify-x11.sh` — always self-wraps in `xvfb-run -a -s "-screen 0 1280x1024x24"` (never against an inherited `DISPLAY`: test 5 kills its X server); hermetic `PIXPAPERS_HOME`; honest named skip when xvfb-run/ImageMagick/xrandr/python-Xlib are absent | six assertions, all passing: `DISPLAY`-only start ⇒ backend `x11` (status --json); root pixmap byte-identical to the `export` oracle (AE=0) at 1280×1024; RFPWM resize hint (synthetic `_NET_WM_MOVERESIZE` ClientMessage via python-Xlib — Xorg does not broadcast one for `xrandr --fb` itself) ⇒ re-render at 1024×768, pixmap == oracle; `stop` removes socket+pid and leaves the X server up; X server killed ⇒ daemon **exit 43** (identical contract to wayland); depth-16 root ⇒ exit 1 with the loud depth message |
| Live Xwayland session (**real**; added at S9 close-out, same day) | the harness shell's own `DISPLAY=:0` (Xwayland over the niri desktop — the root pixmap is not composited, so the desktop is unaffected); `WAYLAND_DISPLAY` unset for the probe; hermetic `PIXPAPERS_HOME`, foreground daemon, exact-PID cleanup in one shot (the AGENTS.md safe pattern) | auto-detect: `DISPLAY` only ⇒ backend `x11` (status --json); `set` applied against the live Xwayland root; `stop` clean — process gone, socket + pidfile removed, hermetic home deleted |

Blit evidence: `test/bench-x11.sh` (`1821986`) — a fresh Xvfb per size,
50 hot applies (two entries alternating, variant cache hot): wall
p50/p95 **7/7 ms** @ 1080p, **22/24 ms** @ 4K; implied PutImage blit
**~1 ms / ~8 ms** (switch − render). Full table + reading in PERF.md
("X11 blit").

Gate: workspace tests, clippy `-D warnings`, `fmt --check` and offline
`smoke.sh` all green — the smoke run with both display vars present in
the parent env is the `20e73ce` assertion.

## S6 — TUI core (2026-10-31)

`pixpapers tui` (PLAN-30 §C.1–C.3): layout + list + offline preview +
detection. Unit plane: 76 pixcore + 23 pixpapers tests, including 7
ratatui `TestBackend` goldens at a pinned 100×30
(`crates/pixpapers/tests/golden/tui/`) and byte-exact sixel/kitty/
iTerm2/base64 wire tests. Live rows:

| Row | What ran | Result |
|---|---|---|
| V-T0 plain-terminal fallback (**real**) | real pty (python `pty.fork`, winsize 100×30, `TERM=xterm-256color`, no image env vars — foot-class terminal), release binary | both panes + caption + fallback line render; `j`/`k`/`r`/`q` behave; exit 0; alternate screen entered **and** left |
| V-T0b wire capture (**real**) | same pty with `PIX_PAPERS_IMAGE=kitty` / `iterm2` / `sixel` | kitty: `ESC_G a=T,f=100,i=1,c=58,r=22,C=1,q=2;<png b64>` observed on the wire; iTerm2: `ESC]1337;File=inline=1;base64;…BEL`; sixel: `ESC P q … ESC \`; status `preview 116×22 · 1 KB · <proto>`; all exit 0 |
| V-T1 kitty on-screen | **skip (named)**: kitty is not installed on this host (foot only) — emission bytes are captured above; on-hardware display unverified | — |
| V-T2 iTerm2 on-screen | **skip (named)**: macOS-only terminal, not available here | — |
| V-T3 sixel terminal on-screen | **skip (named)**: no sixel-capable terminal installed (foot has none; xterm not installed) | — |
| V-T4 export byte-identity (**real**) | 6 export variants (auto / vga+fs / gen16 / off / cover-gutter / normal+nes) `cmp` against the pre-refactor binary (564a966 worktree) | all byte-identical — the preview shares the export encoder verbatim |
| V-T5 action keys | **skip (named) as of S6** — wired in S7: see the §S7 live row below (real pty, full key table, post-exit file truth) | — |

Gate at `343bc9a`: `cargo test --workspace` 76+23 green · clippy
`-D warnings` green · `fmt --check` clean · offline `smoke.sh` green ·
`--no-default-features` checks. Side finding fixed on the way
(`20e73ce`): S5 made `DISPLAY` sufficient for daemon auto-spawn, so the
offline layers (smoke.sh, harness) now strip `DISPLAY` too — two test-
spawned daemons bound to deleted temp homes were found running and were
killed with `pkill -x pixpapers`.

## S7 — TUI actions (2026-10-31)

`pixpapers tui` action table (PLAN-30-ct §C.4/§C.5 — the V-T5 skip
above, now replaced): the full key table `a s c p l t d b g D e x r`
on the S6 shell. Every action goes through the same `commands::*`
function the CLI subcommand calls, then the same `cli::apply_now` seam
(invariant #1 — the TUI never reimplements spawn/notify/manifest
writes). Status lines carry the CLI's own wording, built from the same
*data* the CLI's prints use (id/title/knob values/bytes), not from
stdout capture: libtest intercepts `println!` at the Rust level, so an
fd-level capture is empty in-process tests; on a real terminal the
`Quiet` drain (fd 1+2 → pipe, restored on drop) keeps the CLI's own
prints off the alternate screen.

Unit plane: 76 pixcore + 53 pixpapers tests green (23 S6 + 30 action
tests: per-action against temp `PIXPAPERS_HOME` + throwaway socket
listener — add-by-path byte/hash-identical to CLI `add` of the same
file; remove-current clears `current` and leaves no dir/manifest
residue; remove-non-current does not Apply; mid-apply sequence stays
consistent; knob cycles Apply only on the current entry; bad input is a
status error, nothing stored). Goldens: 10 `TestBackend` frames at the
pinned 100×30 (7 S6 + 3 new: `add_input`, `remove_confirm`,
`knob_cycle`).

Live row:

| Row | What ran | Result |
|---|---|---|
| V-T5 action keys (**real**) | `test/verify-tui.sh` (new; driver `test/tui-pty.py` — the committed form of S6's ad-hoc pty driver): real pty (`pty.fork`, winsize 100×30, `TERM=xterm-256color`); pty env strips `DISPLAY`/`WAYLAND_DISPLAY` + the image-protocol vars (headless — no daemon spawn, Applies record, file writes real); release binary; scratch `PIXPAPERS_HOME` seeded via the real CLI (A = fixture, B = generated PNG, `current = A`; C's id learned in a throwaway home). One key burst: `c` (cycle A→B) · `a <path>\r` (add C) · `k` (select B) · `p` `l` · `b #00ff00\r` · `g 40\r` · `t` `d` · `s` (set B current) · `e` (toggle enabled) · `j` (select C) · `x` (export C to the scratch `$XDG_DOWNLOAD_DIR`) · `Dy` (remove C — no space: the confirm line swallows the very next key, a space would cancel) · `r` (refresh) · `q` (quit) | exit 0; alternate screen entered **and** left; post-exit truth asserted from files, not screen: `current` A→B; B's knobs all changed as driven (32 / cover-gutter / nes / fs / `#00ff00` / 40px); A byte-identical to pre-run; C gone from manifest **and** dir; config `enabled` true→false; exactly one export PNG in the scratch download dir (`normal-fit` — `e` disabled pixelation before `x`, and the export honestly reflects the originals-only state); the wire renders the `added`/`removed`/`wrote` status lines (ratatui's cell diff means the wire is rendering evidence, not a verbatim transcript — the verbatim strings are pinned by the unit tests) |

Gate (this tree): `cargo test -p pixpapers` 53 passed (parallel and
`--test-threads=1`) · `cargo test --workspace` 76 pixcore + 53 pixpapers
green · clippy `--workspace --all-targets -- -D warnings` green ·
`fmt --check` clean · offline `smoke.sh` ok. Release binary
(`cargo build --release`, same host, this tree): **4,751,720 B** vs the
S6 baseline 4,706,216 B — **+45,504 B (+44.4 KiB)** for the action
table + drain.

Hardening found by running the suite for real: the fd-level drain
(`Quiet`) is compiled out under `cfg(test)` — libtest already captures
`println!` at the Rust level, and a process-global `dup2` racing with
another test thread's subprocess spawn is a cross-test deadlock (the
child inherits the armed pipe's write end and the drain's read never
sees EOF — observed wedging the whole libtest pool on a loaded host).
In the real binary both pipe ends are `CLOEXEC` (the daemon `apply_now`
auto-spawns must not inherit them) and the drain is bounded (poll-based,
2 s deadline) so no holder can hang the TUI. The status lines are
data-derived, so no test ever depended on the capture.

## S8 — bar + waybar module (2026-10-31)

`pixpapers bar` (PLAN-30-ct §D; `14cfa89`): exactly one line, always
exit 0 — the contract waybar's custom-module auto-hide relies on (the
module hides itself on empty output or a non-zero exit; `bar` does
neither, so the button never blinks). Three character-exact states (4
hermetic goldens + the `test/smoke.sh` offline checks; the entry case
is proven identical with the daemon dead and alive — the liveness probe
only disambiguates the no-entry case):

  ▞ <title> · <pixel>/<placement-short>   e.g. `▞ nebula · 16/fit`
  ▞ pixpapers — set one                   no current entry, daemon alive
  ▞ pixpapers · daemon down               no current entry, socket dead

A current entry wins even with a dead daemon: the label is
config+manifest truth through the daemon's own resolution chain
(`resolve_effective`), so it can never show a look the daemon would
not render. Formatting is pure (pixcore `bar::bar_label`/`entry_line`);
the manifest read + `daemon_alive` probe live in `commands::bar`, which
never writes and never spawns. `cycle` takes a signed step
(`allow_negative_numbers`) so `on-scroll-down cycle -1` works — the
S8 wheel direction (`8c2f171`).

Live row — `test/verify-waybar.sh` (in `14cfa89`): scratch
`PIXPAPERS_HOME` seeded with three entries (alpha current / beta pixel
32 / gamma) through the real CLI, every state mutation run via
`pp_mutate` (`WAYLAND_DISPLAY`+`DISPLAY` stripped — since S5 a
mutating call that can see a compositor auto-spawns a daemon that
would repaint the user's real wallpaper); `bar` runs unstripped, the
way waybar's exec children run it. A transient waybar 0.15.0 is
pinned to the **bottom** edge of the real DP-1 output, clear of the
user's top bar; `interval 30`; shots in `docs/screens/s8/`.

| Row | What ran | Result |
|---|---|---|
| Render (**real**) | transient waybar `custom/pixpapers` (`exec "pixpapers bar"`, `interval 30`, `format "{text}"`); polled ≤ 15 s until the label's ink appeared in the bottom strip | `▞ alpha · 16/fit` bottom-right — `1-bar-alpha.png` + crop |
| Click | **skip (named)**: no physical click can be injected into the user's live niri seat from the harness | substitute = the snippet's exact `on-click` command, run verbatim: `foot -e pixpapers tui` — `2-tui-in-foot.png`: the TUI up on the scratch library, the exact on-screen form the click would produce |
| Scroll | **skip (named)**: no scroll injection, same reason | substitute = the exact commands the `on-scroll-up`/`on-scroll-down` bindings execute — `pixpapers cycle 1` / `pixpapers cycle -1` — plus `set`, all via `pp_mutate`: the state flips the wheel would drive |
| Flip within one interval (**real**) | after each flip, a 72-px bottom-strip shot at +2 s (stale) and +33 s (past the next 30 s tick) | pixel identity/difference **scoped to the label's ink bbox** (any channel > 80; bar background ≤ (47,47,62), label ink ≈ 577 px per 16 chars — immune to pointer/wallpaper motion; the vision fleet was down — gdp7900xt cold, rtx3060ti hardware_unavailable — so no OCR): `1c≡3a` **0 px** · `3a→3b` **540 px** (`▞ alpha` → `▞ gamma`) · `3b≡4a` **0 px** · `4a→4b` **1 079 px** (`▞ gamma` → `▞ beta · 32/fit`); the exact label strings pinned at each step by the CLI's own `bar` output (`expect_bar`) |

Style inheritance: every waybar instance loads the user's
`~/.config/waybar/style.css` read-only — the transient bar's background
is the user's own `window#waybar` rule: `rgba(30,30,46,0.92)` at 0.92
alpha over the wallpaper measures (27,27,42)–(48,48,63), matching the
shots exactly. The user's bar config and style were never touched.

**Format finding** (why the snippet in `96460e1` ships `{text}`, not
the plan's `{ }`): Arch's waybar 0.15.0-3.1 links the **system**
libfmt 12.2.0 (0.13/0.14 pin fmt 11.0.2 via wrapdb). `"{ }"` — a space
after `{` — is an invalid automatic argument reference, and fmt 12
rejects it mixed with the module's named arguments: `fmt::format`
throws on every tick, waybar's dp handler catches it, logs
`custom/pixpapers: invalid format string`, and the module's event box
stays **silently hidden** (no crash, no error in the output — a run
that discards waybar's stderr sees a bar background with no text).
A/B on the live seat: with identical configs the `"{ }"` module never
rendered (the log line repeated per tick) while the `"{text}"` module
rendered a 16-char label. `custom.cpp update()` is identical across
0.13–0.15; only the distro's fmt differs. `"{text}"` is the form
waybar documents and works across all three.

Pre-existing at S8, fixed in S9 (`dde4d1f`): `cycle` on an **empty**
library exited 2 (`error: library empty — add one with: pixpapers add
<image>`, empty stdout) — a user condition classified as internal, the
AGENTS.md exit-code contract violation S8 found and S9's authorized
task 7 fixed: it now exits **1**, message byte-identical. Harmless to
the module either way: a binding's exit status cannot change the
label — the text is supplied only by the module's own `exec`/`interval`
re-run of `pixpapers bar`, which always prints one of the two no-entry
lines.

Gate (final tree): `cargo test --workspace` 83 pixcore + 58 pixpapers
green · clippy `--workspace --all-targets -- -D warnings` green ·
`fmt --check` clean · offline `smoke.sh` green (both bar sections,
exactly-one-line assertion). Release binary
(`cargo build --release`, this tree): **4,755,048 B** vs the S7
4,751,720 B — **+3,328 B (+3.2 KiB)** for the `bar` subcommand +
`Placement::short`.

## S9 — polish & docs close-out (2026-10-31)

**Task 7 — exit-code audit** (pre-authorized in `6eeb424`, fixed in
`dde4d1f`): audit the CLI surface for user conditions exiting 2
(internal) instead of 1, and fix the classified ones. Every message
stays byte-identical; only the classification changes.

- **Fixed (2 → 1)**: `cycle` on an empty library — the only bare
  `bail!` on the CLI surface (now `pixcore::PixError::Empty`, a new
  variant + `empty()` constructor carrying the exact wording `list`
  prints for the same condition); `set` / the six per-entry knob
  commands (shared `edit_entry`) / `export` on an unknown id (bare
  anyhow "no entry matching …" → `commands::UserError`); `export` with
  an unparseable size (three `parse_size` messages → `UserError`).
- **Already 1, unchanged**: `add` (hash/optimize/dir writes raise
  `PixError`), `remove`, `Library` load/save, `status` (never errors),
  `bar` (never errors), `stop` (always Ok), config load (`main`
  hardcodes 1), daemon start failures (`DaemonStartupError` since
  `85e59ed`).
- **Reported, not fixed (outside the authorized scope)**: `export -o`
  write failure keeps exit 2 (io trouble, not a user condition); clap
  usage/parse errors exit 2 (framework convention, pre-routing); a
  second **foreground** `daemon` while one runs exits 2 via a bare
  `bail!` (`daemon/mod.rs prepare_socket`, bare since v0.1 `10dbc8b`) —
  COMPOSITORS.md's "exits 1" claim for it was a doc bug, corrected in
  the S9 docs pass. `pixcore` is touched by the `Empty` variant +
  constructor only.

Tests + smoke: unit — cycle on an empty home classifies as user error
with the exact `list` wording; unknown ids on `set`/`edit_entry`/
`export` and bad sizes classify as user error with the pre-audit
messages (3 new tests); `smoke.sh` offline layer asserts the
exit-1 wording on the emptied library before any spawn.

**Fresh x11 re-run** (same day, debug binary):
`test/verify-x11.sh target/debug/pixpapers` — all six §S5 assertions
pass again (auto-detect, export-oracle AE=0, RFPWM re-render, clean
stop, X-kill ⇒ exit 43, depth-16 ⇒ exit 1); the §S5 live Xwayland row
was captured in the same session.

**Docs close-out** — the audit surfaced four doc-vs-code mismatches,
all corrected doc-side (no behavior change beyond task 7):

1. COMPOSITORS.md (and TROUBLESHOOTING's flap section): the duplicate
   foreground `daemon` "exits 1" — the code exits 2 (bare
   `anyhow::bail!` since v0.1 `10dbc8b`; the audit deliberately kept
   it at 2).
2. CLI.md: claimed a docs-drift CI check and a generated man page
   `contrib/pixpapers.1` — neither exists anywhere in the repo; both
   claims removed (the clap definitions are the source of truth).
3. CLI.md `remove` row: claimed removing the current entry is refused —
   the code allows it and clears `current` (the §S7 row documents the
   behavior; both backends then skip rendering, so the screen keeps
   the last frame until the next `set`).
4. CLI.md exit-code table: had io/decode/config under 2 and
   remove-current under 1 — rewritten to the actual
   `is_user_error` classifier (1 = every typed `PixError` except the
   catch-all `Other` + `DaemonStartupError` + `UserError` +
   config-load; 2 = `Other` encoder failures + bare anyhow + clap
   usage; 43 = wayland or X server loss).

Plus the S8 note above: the empty-library `cycle` it recorded as
"exits 2 / not fixed" now exits 1 (`dde4d1f`).

Gate (final tree): `cargo test --workspace` **83 pixcore + 61
pixpapers** green (the +3 are the audit's new tests) · clippy
`--workspace --all-targets -- -D warnings` green · `fmt --check`
clean · `env -u WAYLAND_DISPLAY test/smoke.sh` green (including the
new empty-cycle line).

## x11: stop clears the roots (2026-11-01, `1e79d00`)

Post-close-out fix. The x11 `close` now clears every root window to
solid black before disconnecting (best-effort on the exit-43 path,
where the server is already gone), so a stopped daemon leaves no stray
frame on the X server.

- `test/verify-x11.sh` — new assertion: after `stop`, `import
  -window root` is pixel-identical to a 1024×768 black reference
  (AE=0). Full suite green on this machine (xvfb, no skips). The
  script now also prefers `magick` over IM7's deprecated `convert` so
  its output stays clean.
- Stop latency — the daemon's process exit measured after the `stop`
  ack (the ack is sent before teardown, so this is the only cost that
  exists), 1024×768 Xvfb, release, 5 runs each: before
  2/2/8/8/8 ms · after 2/7/7/7/8 ms (median 8 → 7) — one batched
  CreateGC + PolyFillRectangle + FreeGC per screen rides the existing
  flush; no measurable cost.
- A hard kill (`kill -9`, power loss) skips teardown and leaves the
  last frame — unchanged by design: the clear is a graceful-shutdown
  courtesy, not a persistence guarantee (docs/TROUBLESHOOTING).

## release: v0.2.1 — icons, desktop entry, site logo (2026-11-01)

Patch release over `2e2dd0c`: the app icon set (`9eb49ac`) and nothing
else. No runtime code changed — pixcore is untouched and stays published
at 0.2.0; pixpapers 0.2.1 declares `pixcore = "^0.2.0"`.

- Icon tree audited: all 9 PNGs valid RGBA with dims matching their
  hicolor dirs; `desktop-file-validate` on `contrib/pixpapers.desktop`
  exits clean; the site favicon's embedded bytes are byte-identical to
  the shipped `icons/hicolor/48x48/apps/pixpapers.png` (sha256
  `004ee868…`).
- Workspace gates at the release tree: 83 pixcore + 62 pixpapers tests
  green · `clippy --workspace --all-targets -- -D warnings` clean ·
  `fmt --check` clean · `env -u WAYLAND_DISPLAY test/smoke.sh` ok
  (daemon layer skipped offline by design) · `test/verify-x11.sh` ok ·
  `test/verify-tui.sh` ok (xvfb, all post-exit assertions held).
- Tarball self-test behavior (`cargo test` inside the published crate)
  is unchanged from the `2e2dd0c` proof: green with 33 skip notes.

### Release-chain evidence (post-tag)

- The first real `makepkg` run caught the draft PKGBUILD's `cd` bug
  (`$pkgname-$pkgver` doesn't exist for a `git+` source — the working
  dir is `$srcdir/$pkgname`). Fixed in `3004d81`; the v0.2.1 tag was
  force-moved to it before anything consumed the old one.
- `makepkg -d` from the pushed tag: `check()` ran the full workspace
  suite green (83+62, release, `--locked`); package() completed; the
  pkg tar contains `usr/share/applications/pixpapers.desktop` and all
  8 `usr/share/icons/hicolor/…/apps/pixpapers.png` paths (listed).
- GitHub Release `v0.2.1` created; source tarball sha256
  `9349ef0b…d4eef9de` (recorded in the release notes).
- `cargo publish -p pixpapers` → pixpapers 0.2.1 live on crates.io
  (pixcore stays 0.2.0). `cargo install pixpapers --root …` resolves
  and reports **0.2.1**.
- Registry `.crate` extracted, bare `cargo test`: 62 passed / 0 failed
  with exactly 33 `note: skipping CLI golden` lines (`--nocapture`) —
  the `2e2dd0c` self-test signature holds on the real 0.2.1 crate.
- Pages rebuilt: live site serves the PNG `data:` favicon and the
  v0.2.1 strings (release link + footer).

## gutter-style: blurred-bleed gutter (2026-11-01, this change)

`cover-gutter` grew a `gutter-style` knob: `solid` (default) keeps the flat
`bg_color` mat; `bleed` fills the ring with the wallpaper's own edges,
blurred (box-downscale) **before** quantization and snapped to the *same*
palette as the centre, so the ring stays pixelated rather than turning into a
photographic blur. This is the one item AGENTS.md listed under "Deferred by
decision" — built here on explicit request, not proactively.

Design contract, all verified by test:

- **Backward-compatible.** `GutterStyle` is an optional serde field with
  `#[default] Solid` on `Settings` and a plain default on `Config`; existing
  TOML and JSON manifests load byte-identically. No `PIPELINE_VERSION` /
  `MANIFEST_VERSION` bump (adding an optional defaulted field is not a format
  change). The solid cache filename and bytes are unchanged — `Solid` omits
  its cache token exactly the way `none` omits the dither token; `variant_path`
  appends `-bleed` only for `Bleed`.
- **Blur before quantize, same palette.** `build_bleed` crops+downscales the
  *original* to the `output/pixel` grid, runs one guarded `box_halve` pass
  (pure code reusing the pipeline's box-downscale, no new deps), then
  re-applies the wallpaper's own palette. The bleed is quantized to the same
  table as the centre.
- **The centre never changes.** `composite`/`composite_into` are thin wrappers
  over `composite_*_with_bleed`; the bleed path nearest-stretches the bleed
  layer over the full output *before* the variant is stamped, so the centre
  pixels are bit-for-bit the solid render. `bleed_composite_agrees_daemon_and_png`
  proves the daemon path and the offline PNG agree.
- **Ring fills differ; centre does not.** `bleed_fills_the_ring_solid_fills_bg`
  and `render_offline_bleed_paints_ring_solid_paints_bg` assert a ring pixel
  changes under `bleed` while a centre pixel does not, and that `solid` paints
  the ring with `bg_color`.
- **Bleed layer cache.** The blur is wallpaper geometry, not ring width or
  gutter style, so it is cached once as a sibling
  `bleed-v2-<pixel>-<W>x<H>-<Q>[-<dither>].png` (no gutter/placement/style
  token) and reused across ring widths and `solid`↔`bleed` toggles.
  `bleed_variant_token_is_conditional_and_bleed_cache_ignores_ring` covers the
  naming, the `-bleed` conditional token, and ring-width independence.

### Blur cost (release binary, hermetic `PIXPAPERS_HOME`, offline)

`export` renders with the non-persisting `Offline` policy, so it builds the
bleed layer on every run — the solid→bleed delta is therefore the per-render
worst case. Median of 5, everything equal but `--gutter-style`:

| output | source | solid | bleed | Δ = build_bleed + composite |
|---|---|---|---|---|
| 1920×1080 | 1600×900 | ≈ 104 ms | ≈ 108 ms | **+≈4 ms** |
| 3840×2160 | 3840×2160 | ≈ 387 ms | ≈ 387–390 ms | **≈ 0–3 ms** (noise) |

The bleed layer is built at `output/pixel` resolution (120×68 at 1080p,
240×135 at 4K), not full output — a crop+downscale to that grid plus one halve
pass plus a palette re-snap — so its cost is small and does not grow with the
ring width. The live daemon path caches the layer (see the naming test above),
so steady-state re-renders amortize it away.

### Demo + gates at this tree

- Two real 1920×1080 renders generated with the release binary from
  `docs/img/source.png` (`c8a5a7e1`, `cover-gutter · gutter 72 · pixel 16 ·
  nes · fs · bg #0f1114`), differing only in `--gutter-style`:
  `docs/img/gutter-solid.png` vs `docs/img/gutter-bleed.png`. Whole-image
  pixel diff = **411,264 / 2,073,600 (19.83 %)**, exactly the 72-px gutter ring
  (`1920×1080 − 1776×936`); **0 centre pixels differ**. A gutter pixel is
  `#0f1114` under `solid` and a blurred-wallpaper colour (e.g. `#787878`) under
  `bleed`. The interactive section in `docs/index.html` recomputes a magnified
  gutter/edge/centre region live in the browser from these same files inlined as
  `data:` URIs (no server, no network, no external requests — verified: the only
  non-`data:`/`img/` links on the page are the existing relative `.md` doc links;
  no `fetch`/XHR).
- Workspace gates: 92 pixcore + 63 pixpapers tests green ·
  `cargo build`/`build --release` clean · `clippy --workspace --all-targets --
  -D warnings` clean · `cargo fmt --check` clean ·
  `env -u WAYLAND_DISPLAY test/smoke.sh` ok · `test/verify-tui.sh
  target/release/pixpapers` ok (the `B`-knob set-equality golden stays at its 6
  keys — the TUI driver never presses `G`, so `gutter_style` never leaks into the
  settings snapshot) · `test/verify-x11.sh` ok (root pixmap byte-identical to the
  oracle under the `solid` default). TUI goldens regenerated (10 frames; caption
  gained the `gutter style solid` row, HINTS gained `G style`).

## auto-preset: preset chosen from output size (20-11-01, this change)

### The rule (pixcore, pure)

`pixcore::Pixel::resolve_for(out)` — the smallest preset from
`8/16/32/64/128` whose grid keeps the output's **long edge** at
≤ `AUTO_MAX_BLOCKS = 120` blocks: `L ≤ 960 → 8`, `≤ 1920 → 16`,
`≤ 3840 → 32`, `≤ 7680 → 64`, else `128`. The budget is `1920/16`, so
1920×1080 resolves to the shipped default preset exactly. `auto` never
picks `normal`; concrete presets pass through (idempotent). Grounded in
the plan's parked idea (step-6 §6.4: the floor/auto question, "bigger
output ⇒ coarser pixel" for a consistent blocky look); it is auto by
**output size**, not by image content (that stays deferred).

### Cache integrity

Every `variant.rs` entry point (`variant_path`, `generate`,
`generate_from_image`, `render_offline`, `bleed_path`, `bleed_layer`)
resolves `auto` **before the cache key is built** — filenames always name
a concrete preset (an `auto` pixel slot would also be ambiguous with the
`-auto` palette token). Proven by `auto_pixel_never_leaks_into_cache_names`
(path equality with the pinned preset) and
`auto_generates_the_bytes_of_its_resolved_preset` (same file, same bytes,
second pass is a cache hit).

### Resolution is observable

- daemon `render_into` resolves per output; the status label names the
  resolved preset (`8/fit-auto` at 320×180 in
  `render_label_names_the_resolved_preset_for_auto`; `32/fit-auto` on a
  4K output in the field) — visible in `status` text and
  `status --json` `outputs[].variant`.
- `export --pixel auto` prints `auto→N` and the default filename carries
  the concrete preset (`export_pixel_auto_resolves_to_concrete_bytes`).
- TUI caption shows `pixel auto→N` for the preview canvas
  (`caption_resolves_auto_for_the_canvas`); `p` cycle reaches `auto`.

### Demo + gates at this tree

- Three real `export --pixel auto` renders of `docs/img/source.png`
  (release binary, hermetic `PIXPAPERS_HOME`):
  `docs/img/auto-960x540.png` (14 KB, `auto→8`),
  `docs/img/auto-1920x1080.png` (45 KB, `auto→16`),
  `docs/img/auto-3840x2160.png` (165 KB, `auto→32`).
  **Byte-identity proof:** each is `cmp`-clean against the same export
  with the preset pinned by hand (`--pixel 8/16/32` at the same size).
  **Block-width proof:** modal constant-colour run length along sampled
  rows = exactly 8 / 16 / 32 output px respectively; all three are 120
  blocks across (the rule's budget). The website section shows these
  files with their resolved presets; zero-external-requests re-verified
  (all `src` are `img/*.png` or `data:`; no fetch/XHR/@import;
  `web_check` at 360 px: no console errors, no failed requests, no
  horizontal overflow; `web_performance`: `horizontal_overflow: 0`, no
  image over 300 KB).
- Workspace gates: **99 pixcore + 66 pixpapers tests green** (was 92+63;
  +7 pixcore: parse/serde, ladder table, pass-through/idempotence,
  proptest monotone+concrete+idempotent, resolve_policy, config +
  manifest round-trips, bar; +3 pixpapers: export byte-identity, daemon
  label, caption). `cargo build --release` clean ·
  `clippy --workspace --all-targets -- -D warnings` clean ·
  `cargo fmt --check` clean · `env -u WAYLAND_DISPLAY test/smoke.sh` ok ·
  `test/verify-x11.sh` ok (root pixmap still byte-identical to the
  oracle — default path untouched) · `test/verify-tui.sh` ok (B-knob
  golden stays at its 6 keys; `p` one step from 16 is still 32). TUI
  goldens **not** regenerated — no caption row or HINTS change, the 10
  pinned 100×30 frames are byte-identical (goldens test green).

## live wallpapers: slideshow (2026-11-01, this change)

Still-image slideshow cycling the library in `list` order, derived from
the wall clock at render time — `active = (unix_now / interval_secs) %
entry_count`. The daemon stays a pure renderer; the poll loop replaces its
idle 300 ms timer with a wake-up at the next boundary only while the
slideshow is on, so the OFF idle cost is byte-for-byte the historical one.
No epoch/counter is stored, so the phase survives a restart (the honest
caveat: the phase is aligned to the Unix epoch, so enabling mid-slot shows
that slot and the first change can land in under a full interval).

- **Unit (fake clock, no sleeps)** — `pixcore::slideshow`: `active_index`
  (incl. wrap ordering + empty-list guard), `secs_to_next_change` (always
  `1..=interval`, never 0), `next_change_unix`, `active_entry` (falls back
  to `manifest.current` when off, `None` on an empty library). 9 tests.
- **Config/back-compat** — `[slideshow]` `{enabled, interval_secs}`,
  `#[serde(default)]` (off, 300 s). Old configs load unchanged; no manifest
  or config version bump. 3 IPC round-trip tests: a report with the new
  optional fields parses, an old report (fields absent) reads as `None`,
  and the fields re-serialize only when present.
- **CLI/status (real binary, hermetic `PIXPAPERS_HOME`, display env
  cleared)** — `crates/pixpapers/tests/slideshow.rs`, 10 tests: `on`/`off`/
  `interval` round-trips, `interval 0` is a user error (exit 1), off output
  unchanged, the on `status` line and the `status --json`
  `slideshow_enabled`/`next_change` fields, and daemon-down honest note.

### Idle cost (on vs off) — release build, throwaway Xvfb, hermetic home

Command (on the box the daemon runs on; the x11 backend needs a 24-bit
truecolor display, hence the explicit Xvfb screen):
`xvfb-run -s "-screen 0 1024x768x24" test/bench-slideshow.sh`

| state | idle CPU (6 s window) | VmRSS |
|---|---|---|
| slideshow **off** (300 ms poll) | 0.000 % of one core | 4980 kB |
| slideshow **on** (interval 300 s) | 0.000 % of one core | 5116 kB |

The ON case is measured at a long interval so no change lands inside the
window — the render a change *does* trigger is the slideshow's real work,
not idle overhead, and the timer advance is deliberately **not** folded
into the interactive `--bench` switch latency (that stays a render timing).

### Website (`docs/index.html`, one file)

Added a "live wallpapers" section with **3 real `pixpapers export` PNGs**
into `docs/img/slideshow-{1,2,3}.png` (960×540 from the release binary).
The delivering session did not record the exact per-entry knobs; measured
from the delivered PNGs instead: 16 px block grid (constant-colour runs
along block boundaries = exactly 16), images 1 and 3 quantized to exactly
32 colours, image 2 is a **cover-gutter bleed** render — its ring holds 47
distinct block colours where a solid mat would hold 1. (This paragraph
first claimed all three were `fit --palette vga`; that was measured wrong
— none is the VGA palette — corrected 2026-11-01 at review.) Each has
descriptive `alt`. The auto-cycling viewer is **pure
CSS** (`steps()` keyframes), shown only under `@media
(prefers-reduced-motion: no-preference)`; under reduce-motion, or with JS
off, it degrades to a readable 3-up strip (`role="img"` + one `aria-label`,
individual `alt` kept, the decorative cycle `aria-hidden`). Re-verified
**zero external subresource requests** after the edit: every `src` is a
relative `img/*.png`, both `<script>` blocks are inline, the favicon is a
`data:` URI; the only `https://` strings are pre-existing `<a href>`
navigation links (not fetched on load).

### Workspace gates

**108 pixcore + 69 pixpapers + 10 slideshow-integration tests green**
(pixcore +9: the slideshow time-model; pixpapers +3: the IPC back-compat;
+10 in `tests/slideshow.rs`). `cargo build --release` clean · `cargo clippy
--workspace --all-targets --all-features -- -D warnings` clean · `cargo fmt
--all --check` clean · `env -u WAYLAND_DISPLAY -u DISPLAY test/smoke.sh` ok
· `test/verify-x11.sh target/release/pixpapers` ok (root pixmap still
byte-identical to the export oracle; stop clears roots; exit-43 contract
unchanged) · `test/verify-tui.sh target/release/pixpapers` ok (B-knob stays
6 keys; **no new TUI key or caption** — the 10 pinned 100×30 goldens are
byte-identical, goldens test green).

### Not run / deferred

- **Vision-rig visual pass: NOT RUN** — the screenshot/analysis rig refused
  the capture at run time; the website was checked structurally instead
  (zero external requests, reduced-motion gating, JS-off strip, real alt).
- **Animated / video wallpaper sources remain deferred on purpose** — the
  slideshow cycles **still** images only; there is no video-decoder
  dependency (no decoder dependency was introduced). AGENTS.md names this
  explicitly.

## Native GUI

Opt-in `gui` cargo feature (`gui = ["dep:eframe"]`, **default off**) adding
`pixpapers gui`: a native egui/`eframe` control window. The view-model
(`src/gui/model.rs`) is headless and unit-tested; `app.rs` is a thin eframe
paint layer. The GUI is a **client** — it mutates state only through the
existing `commands::*` + `cli::apply_now`, never owning state itself.

### Dependencies + justification (numbers)

The only new runtime dependency is `eframe` (0.37,
`default-features=false, features=["default_fonts","glow","wayland","x11"]`,
`optional=true`) — glow/OpenGL only, no wgpu/http/persistence. Thumbnails
reuse the existing `image` dep (already a direct non-optional workspace
dependency); no new crate decodes the cached variants. The OFF binary pulls
none of this tree — `eframe` is `optional` and the `gui` module is
`#[cfg(feature = "gui")]`.

### Size + clean build time (exact commands)

Both from a full `cargo clean` on this tree (rustc/cargo 1.97.1):

| Build | Command | Size | Clean build | SHA-256 |
|---|---|---|---|---|
| GUI **off** (default) | `cargo clean && cargo build --release` | 4,907,968 B | ~30 s | `f01aeb4416d4b84e…` |
| GUI **on** | `cargo clean && cargo build --release --features gui` | 12,138,488 B | ~75 s | `2aa40aaad494173c…` |

The ON−OFF delta (+7,230,520 B ≈ 6.9 MiB) is the eframe/egui/glow/winit tree.

### The GUI source is OFF-neutral: 0 bytes (control build)

The pre-GUI HEAD baseline binary was 4,847,016 B (`ca503d56e40395ed…`). My
GUI-off binary is 4,907,968 B — **+60,952 B**. That delta is **not** GUI code:
eframe 0.37 hard-requires `image ≥ 0.25.10`, which forces the shared `image`
dep up from HEAD's 0.25.9 (`cargo update -p image --precise 0.25.9` fails:
*"required by package eframe v0.37.0"*), and that patch bump is in the
dependency tree even when `gui` is off.

Proof the GUI *source* adds nothing when the feature is off — a control build
of the **pristine HEAD source** (`51c910c`, no `src/gui/`) checked out in a
worktree, compiled **with my `Cargo.lock`** (eframe + image 0.25.10) and
`cargo build --release` (no `--features gui`) produced `f01aeb4416d4b84e…`
(4,907,968 B) — **byte-for-byte identical** to my GUI-off binary. So the
feature-off binary is exactly "HEAD + the forced image bump"; the GUI code
contributes 0 bytes off. Full byte-identity to the pre-GUI `ca503d56`
baseline is impossible **only** because of that eframe-forced `image` patch,
disclosed here rather than hidden.

### View-model tests (headless)

`cargo test --features gui` runs 5 new tests in `src/gui/model::tests`:
`dirty_state_lifecycle`, `validate_gates_save`, `global_switches_update_config`,
and two byte-identity equivalence tests — `gui_set_matches_cli_set_bytes` and
`gui_save_matches_cli_knob_bytes` — that drive an in-process `Gui` and the real
CLI **subprocess** against the **same** hermetic `PIXPAPERS_HOME` and assert the
manifest bytes are identical (invariant: a GUI set == a CLI set). Test #1 is
proven, not asserted by construction.

### Feature matrix (all green)

- `cargo build --release` — ok (OFF binary `f01aeb44`).
- `cargo test --workspace` — ok (187: 108 pixcore + 69 pixpapers + 10 sixel).
- `cargo test --workspace --features gui` — ok (192: +5 `gui::model` tests).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- `cargo fmt --all --check` — clean.
- `env -u WAYLAND_DISPLAY -u DISPLAY test/smoke.sh` — ok.
- `test/verify-x11.sh target/release/pixpapers` — ok.
- `test/verify-tui.sh target/release/pixpapers` — ok (10 TUI goldens byte-identical; `B` knob set still exactly 6 keys).
- Help drift: `pixpapers --help` on the OFF binary does **not** list `gui`;
  `pixpapers gui` on the OFF binary is an unknown subcommand.

### Real screenshot (not a mockup)

`docs/img/gui-control.png` is one real capture of `pixpapers gui` on a hermetic
`PIXPAPERS_HOME` under Xvfb with software GL:

```sh
env -u WAYLAND_DISPLAY DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 \
  GALLIUM_DRIVER=llvmpipe pixpapers gui        # GUI-on binary
# variant cache populated first by a real daemon render on :99 (pixpapers daemon --backend x11 + set)
DISPLAY=:99 import -window root win-shot.png
```

The window shows the Library tab, the selected entry with its cached-variant
thumbnail, the action row (set now / cycle / remove), and the per-wallpaper
knob panel headed `This wallpaper — Sunset Gradient`. It is **not** a full
accessibility or platform-matrix claim: it is a single Xvfb software-GL frame.

### Website

`docs/index.html` gained a "native window" section referencing the real
screenshot via the relative `img/gui-control.png`. Re-verified the site
contract: one file, zero external asset requests (all `src=` are `img/*.png`
or `data:` URIs; external refs are only `<a href>` navigation to
github/crates.io), palette (`#0f1114`/`#f7b32b`/`#39c08f`) unchanged, favicon
untouched.

### Not run / deferred (GUI)

- The GUI was **not** exercised under a real Wayland/niri compositor or with a
  GPU — only Xvfb software GL. `eframe`'s wayland feature is compiled in but
  untested here.
- No screen-reader / keyboard-only accessibility pass was run against the
  window; the view-model is unit-tested, not the widget a11y tree.

## release: v0.3.0 — bleed, auto preset, slideshow, native GUI (2026-11-02)

Minor release over `9f0b893`: four additive user-facing features —
gutter-style **bleed**, the by-output-size **`auto`** pixel preset, the
still-image **slideshow**, and the default-off **native GUI**. `pixcore`
bumps to 0.3.0 on genuinely new public surface (`Pixel::resolve_for`, the
`slideshow` time-model module, the bleed gutter ring); `pixpapers` 0.3.0
declares `pixcore = "0.3.0"`. No manifest or config version bump — every
feature is backward compatible.

- Workspace gates at the release tree (`6086ba1`): 108 pixcore + 69
  pixpapers + 10 slideshow green (187; **192** with `--features gui`, +5
  `gui::model`) · `clippy --workspace --all-targets -- -D warnings` clean ·
  `fmt --check` clean · `env -u WAYLAND_DISPLAY test/smoke.sh` ok ·
  `test/verify-x11.sh` ok · `test/verify-tui.sh` ok.
- Cargo.lock churn was only `pixcore`/`pixpapers` `0.2.1 → 0.3.0`.

### Release-chain evidence (post-tag)

- `main` fast-forwarded `9267746..6086ba1`; annotated tag `v0.3.0` (object
  `889c8e7`, peels to `6086ba1` = pushed HEAD) pushed to origin.
- `cargo publish -p pixcore` — dry-run first (50 files, no stray local/
  secret files, verified build compiled), then real: **pixcore 0.3.0 live**
  on crates.io. Confirmed queryable before publishing the dependent crate.
- `cargo publish -p pixpapers` — dry-run resolved `pixcore 0.3.0` **from the
  registry** and compiled, then real: **pixpapers 0.3.0 live**.
- Fresh install under a hermetic `CARGO_HOME` + `PIXPAPERS_HOME`:
  `cargo install pixpapers --version 0.3.0` resolves `pixcore 0.3.0` +
  `pixpapers 0.3.0` from the registry (not the git source) and reports
  **0.3.0**; an `add`/`list`/`export` smoke on the installed binary passes,
  and `export --pixel auto --gutter-style bleed` wrote a
  `v2-16-fit-1920x1080-auto-bleed.png` variant (`auto→16`) — the auto and
  bleed features run in the *published* crate.
- Published bare `.crate` self-test: unpacked `pixpapers-0.3.0` (pixcore
  resolved from the registry), `cargo test` = 69 (`main.rs`) + 10
  (`slideshow.rs`) = **79 passed / 0 failed**. This run emitted **0**
  `note: skipping CLI golden` lines — `cargo test` materialised
  `target/debug/pixpapers`, so `skip_without_cli_binary` found a binary and
  the CLI goldens **executed against the published artifact** rather than
  skipping. That is a *stronger* signature than v0.2.1's "62 passed / 33
  skips"; 0 failures either way. `.crate` sha256: pixcore
  `1d973063…c9113` (50 files), pixpapers `83851d48…885d` (38 files).
- GitHub Release **`v0.3.0`** published (converted the workflow's draft to
  published with a hand-shaped title/body; the binary + `SHA256SUMS` assets
  were removed to match v0.2.1's asset-free shape). Source tarball sha256
  `8f90478c…61998` (recorded in the notes). No lingering `Draft v0.3.0`
  remains — this release reused the workflow draft rather than leaving it
  alongside a second published release the way the v0.2.1 trail did.
- Pages rebuilt: the live site serves the `v0.3.0` strings (release link +
  footer). Site contract re-verified — `index.html` with zero external-origin
  asset requests (all `src=` are same-origin `img/*.png` or `data:` URIs;
  external refs are only `<a href>` to github/crates.io), favicon `data:`
  URI untouched, palette unchanged.
- `contrib/packaging/PKGBUILD` `pkgver` bumped to `0.3.0`. It stays a
  declared draft — no AUR checkout with an `aur.archlinux.org` remote exists
  on this box, and nothing was pushed to any new remote. AUR publication is
  a manual follow-up.

### Disclosed, not silently changed

- `README.md` still lists "Native GUI" under a "Shipped in v0.2" heading —
  pre-existing drift (the GUI shipped after 0.2.1), left as-is rather than
  rewritten during the release sweep. (Corrected after this release in the
  `auto-content` change below: the heading now reads "Shipped since v0.2".)

## Content-based auto-preset (2026-11-02, this change)

`auto-content` — auto-preset **by image content**, landing under
[Unreleased] with **no crate version bump** (additive, backward compatible).
`auto` (size-only) is untouched byte-for-byte; content is a separate opt-in
token.

### The combine rule (pixcore, pure)

`Pixel::AutoContent` → `resolve_content_for(out, density)`: resolve the same
preset `auto` would from the output size (`floor = Auto.resolve_for(out)`),
then `if content_preset(density).block_px() > floor.block_px() { content }
else { floor }`. The size pick is a **floor** — content can only push
**coarser**, never refine. A busy image resolves to the finest content preset
(8) and is therefore clamped back to the floor, so it renders byte-identically
to plain `auto`; a calm image resolves coarser than the floor and stays there.
A **missing** or **stale-rule** probe (`None`, or `probe.rule != CONTENT_RULE_VERSION`)
falls back to the floor, so such an entry is byte-identical to `auto`.
`resolve_for` guards out both `Auto` and `AutoContent` (neither is a concrete
size preset). Thresholds (calibrated + locked at the 512 px edge): density
`≥64→8`, `≥32→16`, `≥16→32`, `≥8→64`, else `128`.

### The statistic (integer-only ⇒ deterministic)

`pixcore::content`: downscale the **colour** image to `ANALYSIS_EDGE = 512`
(the long edge; never upscales) via `pipeline::downscale`, convert to Rec.601
integer luma, and score the mean absolute horizontal + vertical neighbour
gradient scaled ×16 (range `0..4080`). Persisted as a `{rule, density}`
`ContentProbe` in `Settings.content` (`skip`-if-`none`). Same source bytes +
same `CONTENT_RULE_VERSION` ⇒ same density ⇒ same preset (no floats, no
threading dependence).

### Per-image analysis cost — release build, throwaway bench, median of 15 (warm)

Median wall time of `analyze_file` (the whole cost the CLI pays: PNG decode +
downscale + gradient) and, isolated, `content_density` alone (downscale +
gradient on a pre-decoded frame), across real fixtures including 4K:

| fixture | size | density | `analyze_file` median | statistic-only median |
|---|---|---|---|---|
| `test/fixture.png` | 96×60 | 33 | **0.044 ms** | 0.014 ms |
| `docs/img/source.png` (sunset) | 1600×900 (16-bit) | 11 | 12.49 ms | 3.50 ms |
| busy 1080p (4 px checker) | 1920×1080 | 906 | 6.82 ms | 4.27 ms |
| flat 1080p (ramp) | 1920×1080 | 4 | 5.49 ms | 4.31 ms |
| busy 4K (4 px checker) | 3840×2160 | 38 | **30.14 ms** | 12.91 ms |
| flat 4K (ramp) | 3840×2160 | 4 | 24.64 ms | 12.61 ms |

Median across the set: **~9.7 ms** full `analyze_file`, **~4.3 ms** the
statistic alone. `analyze_file` is dominated by the **PNG decode** (the CLI
reads the file once anyway); the gradient statistic is bounded by the 512²
edge (≤ ~13 ms even at 4K, because the downscale output is capped regardless of
source size). The analysis runs **once, on the CLI**, at `pixel`/`default
pixel`/`add` time — never on the daemon's render path — and is on the order of
a **single cold render** (PERF.md: 1080p@16 ≈ 10.9 ms, 4K@8 ≈ 105 ms), so it is
negligible in the interactive budget and happens off the hot path.

### Combine-rule + cache evidence (release binary, hermetic `PIXPAPERS_HOME`)

Real `export` renders of the two website sources at 1920×1080 (floor = 16):

- **busy dot pattern** (`busy_photo.png`, detail 123) → `auto-content→16`:
  `cmp`-identical to `--pixel 16` pinned by hand — content read a fine preset
  (8) but the floor held. `detail 123 → 8 … (on a big display)` shown on the
  set line; the export line names the concrete `16`.
- **calm sunset** (`docs/img/source.png`, detail 11) → `auto-content→64`:
  `cmp`-identical to `--pixel 64` pinned by hand — content **pushed coarser**
  than the 16 that plain `auto` gives the same image at the same size.
- variant cache names the concrete preset: only `v2-16-fit-1920x1080-auto.png`
  and `v2-64-fit-1920x1080-auto.png` were written — **no `v2-auto-content`**
  ever names a file (the trailing `-auto` is the palette token, not the pixel).
- the missing/stale-probe fallback (`auto` on the same sunset) → `auto→16`,
  byte-identical to the size floor.

### Where it is computed / daemon purity

The picture is measured once on the CLI/state side and *persisted* with the
entry: `pixel <id> auto-content` (choosing any other preset clears the stale
probe), `default pixel auto-content` (backfills every inheriting entry, and
future `add`s), `add` **iff** the default pixel is `auto-content`. The daemon
is a pure renderer: `render_into` only calls
`s.pixel.resolve_content_for(out, s.content_density)` — it never decodes an
image to re-measure it. `export --pixel auto-content` reads the persisted probe
(render-only; with no probe it equals `auto`).

### Aliasing — the documented limitation (measured, tested)

A **full-frame pattern whose features rescale with the frame** is not
resolution-invariant under the 512-edge downscale: in the table above the same
4 px checkerboard reads density **906** at 1080p but only **38** at 4K —
downscaling 3840→512 averages 1 px–scale cells toward flat, so a per-pixel
checkerboard reads as **calm** and falls back **coarse** (the conservative
outcome: it over- rather than under-pixelates real detail). A real photograph
keeps its feature size across resolutions, so it reads consistently. Tested by
`aliasing_is_the_documented_limitation` (pixel-art-scale cells survive the
downscale and stay fine; 1 px cells alias to coarse).

### Observability + backward compatibility

- `status --json` gains an **additive**, skip-if-absent `current_auto_detail`
  (`auto→N @WxH` / `auto-content→N @WxH`); a report without it parses, a stale
  one ignores it. 3 backward-compat IPC tests.
- TUI caption shows `pixel auto-content→N` for the preview canvas; the `p`
  cycle reaches `auto-content` (appended to `Pixel::ALL`, 7 → 8). The 10 pinned
  100×30 TUI goldens are **unchanged** (goldens green, not regenerated — the
  `B-knob` golden stays at its 6 keys; `auto-content` is reached by cycling, not
  by the pinned initial frame).
- No manifest/config version bump: the probe is a `skip`-if-`none` field;
  plain `auto` exports are byte-for-byte unchanged (asserted against the pinned
  concrete preset). No new runtime dependency (existing `image` crate +
  pipeline only).

### Tests at this tree

- **pixcore**: content statistic + preset combine rule — determinism (same
  bytes ⇒ same density), `floor_is_a_floor` (content never refines below the
  size preset), the aliasing limitation, downscale-still-discriminates,
  missing/stale-probe falls back to the floor. 116 pixcore (was 108; **+8**).
- **pixpapers integration** (`crates/pixpapers/src/harness.rs`): end-to-end
  via the real CLI in a hermetic home — busy `auto-content` is byte-identical
  to plain `auto` (content never refines below the floor); the flat ramp pushes
  coarser; the persisted probe JSON round-trips; a non-content preset clears
  the probe. Threshold-free assertions (expected preset derived from the public
  resolver, not a hard-coded number).
- Workspace totals: **200** without features (116 pixcore + 74 pixpapers bin +
  10 slideshow) · **205** with `--features gui` (+5 `gui::model`). Both green.

### Gates run

`cargo build --release` clean · `cargo test --workspace` green (200) ·
`cargo test --workspace --features gui` green (205) ·
`clippy --workspace --all-targets --all-features -- -D warnings` clean ·
`cargo fmt --all --check` clean · `env -u WAYLAND_DISPLAY test/smoke.sh` ok ·
`test/verify-x11.sh` ok (default path untouched, root pixmap byte-identical) ·
`test/verify-tui.sh` ok.

### Website (`docs/index.html`, one file)

Added an `#content-auto` section (after `#auto`) with **3 real `pixpapers
export` PNGs** into `docs/img/` (release binary, hermetic home):
`content-auto-busy.png` (busy pattern, `auto-content→16`, floor held),
`content-auto-flat.png` (sunset, `auto-content→64`, pushed coarse),
`content-auto-flat-auto.png` (same sunset, plain `auto→16`, for contrast) —
busy and calm `cmp`-identical to their pinned presets as above. Each `alt`
describes the actual subject (verified with the vision `image_describe` rig, not
assumed). Same contract as the rest of the site: all `src` are same-origin
`img/*.png`, favicon `data:` URI untouched, palette unchanged (`#f7b32b`
accent), explicit `width`/`height` on every `<img>`. Re-verified after the
edit: `web_check` @ 360 px → **0 console errors, 0 failed requests**;
`web_find_overflows` @ 360/400/720/1080 → **0 overflow**; `web_performance` @
1440 → `horizontal_overflow: 0`, **0 images over 300 KB** (new PNGs are 43–80
KB), 0 images missing dimensions, 0 problems.

### Not run / deferred

- The full **vision test rig** (`verify-*.sh` screenshot-diff pipeline) was
  **not** run for this change — `image_describe` was used only to confirm the
  three images' subjects for honest `alt` text.
- README drift: the `## Shipped in v0.2` heading (which the v0.3.0 release
  disclosed as stale, listing the Native GUI that shipped after 0.2.1) was
  retitled `## Shipped since v0.2`, the "Auto-preset by image content" entry
  was moved from "Deliberately not in v0.2" to a shipped `auto-content` bullet,
  and the "not included" list now states plain `auto` stays size-only while
  `auto-content` is the explicit opt-in.

## Media-key control surface (2026-11-03, this change)

The AGENTS.md "Deferred by decision" item **MPRIS-ish hooks** split into two
interpretations and was decided on numbers, not a hunch:

- **Interpretation B — a transport-agnostic command surface — SHIPPED.**
  `pixpapers next` / `prev` / `play` / `pause` are aliases so a key binder
  (sxhkd, a niri `bind`, keyboardctl) drives the slideshow from transport keys
  with **no new runtime dependency**, plus observable position
  (`status --json` → `current_index` + `library_len`; human `status` → a
  1-based `position: N / M` line). This was chosen because every media verb is
  already a pure state-owning CLI command — the only genuinely missing pieces
  were the named verbs and observable position, both additive.
- **Interpretation A — a real D-Bus MPRIS service (`zbus`) — stays deferred,
  measured.** The decision and full probe are in
  [docs/plan/step-10.md](plan/step-10.md); the measured numbers are below.

### The `zbus` footprint probe (throwaway, HEAD `36c8db6`, MSRV 1.85)

`zbus = "5"` behind a default-**off** `mpris` feature, `--release`, `lto=true`:

| Measurement | Result |
|---|---|
| Dependency-tree nodes, feature OFF | 289 |
| Dependency-tree nodes, feature ON | 429 (**+140**, ~135 new packages) |
| Release binary, feature OFF | `4f9bb498…`, 5,145,752 B — **byte-identical** to the pre-probe baseline, proving the entire ON delta is `zbus`, not the probe code |
| Release binary, feature ON | 6,206,808 B (**+1,061,056 B, ≈ +20%**) |
| What that +20% bought | a *connection probe only* — no UI, no object path served |
| Toolchain | `zbus 5.13.2` needs Rust **1.87** vs this project's **1.85** MSRV (**+2 versions**) |
| Architecture | pulls `async-io` / `async-executor` / `futures-lite` / `event-listener` — a reactor that collides with the daemon's deliberate single-threaded, no-tokio `poll(2)` posture |

Rejected on those numbers: the size/MSRV/architecture costs are all real, and
the sole payoff a bus name buys (a cover-art thumbnail in a GNOME/KDE widget)
is moot for a still-image renderer with no cover pipeline. The probe crate was
deleted; nothing from it is in the shipped tree.

### What B actually added, measured

Default (`--release`, no features) release binary: baseline 5,145,752 B →
**5,155,480 B** (`sha256 e18a999ed6b0…`) = **+9,728 B (~9.5 KiB)** for the
four verbs + two observable fields.

### Proofs run (hermetic `PIXPAPERS_HOME`, no live display)

A real library (3 entries, generated PNGs) drove the actual release binary
with `WAYLAND_DISPLAY`/`DISPLAY` unset (so the daemon refuses to spawn — see
"Not run"), and state still recorded correctly:

- `slideshow on` → `slideshow_enabled: true`; `pause` → `slideshow_enabled:
  false` — **Play/Pause ↔ slideshow on/off** confirmed through the JSON face.
- `next` → `current: c2138381`, `current_index: 1`, `library_len: 3`; `prev` →
  `current_index: 0`; the `… → <id> · <title> (recorded …)` notes match the
  `cycle` path — **Next/Previous byte-identical to `cycle ±1`**.
- `current_index` is **0-based** in `--json`; the human `status` printed
  `position: 1 / 3` (1-based). With a library but no current entry,
  `library_len` is present and `current_index` is **absent** (not `0`), so a
  "nothing set" state stays distinguishable from "index 0".
- Backward compatibility: the two new `--json` fields are additive,
  skip-if-absent (`serde skip_serializing_if`), so an old client ignores them —
  covered by an extended `ipc::tests` round-trip.

`crates/pixpapers/tests/media_keys.rs` (new, 10 tests) asserts all of the above
end-to-end in a hermetic `PIXPAPERS_HOME`: the four verbs round-trip through
`status --json`, Next/Previous equal `cycle ±1`, Play/Pause flip `slideshow`,
stale/degraded behaves like the existing commands, and stale/degraded daemon
states surface like existing verbs.

### Gates run

`cargo build --release` clean · `cargo test --workspace` green (**210** =
116 pixcore + 74 pixpapers bin + 10 slideshow + 10 media_keys) ·
`cargo test --workspace --features gui` green (**215**, +5 `gui::model`) ·
`clippy --workspace --all-targets --all-features -- -D warnings` clean ·
`cargo fmt --all --check` clean · `env -u WAYLAND_DISPLAY test/smoke.sh` ok ·
`test/verify-x11.sh` ok · `test/verify-tui.sh` ok — and the four new top-level
verbs deliberately stay **out** of the TUI keymap, so the TUI keymap is
unchanged (verify-tui leaks no B-knob keys).

### Website (`docs/index.html`, one file)

Added an `#media-keys` section (after `#slideshow`) — **text + one `<pre>`
excerpt only, zero new images and zero new CSS** (existing classes/vars). The
excerpt is a **real** `pixpapers status --json` capture over the hermetic home
above (`current_index: 1`, `library_len: 3`, `slideshow_enabled: true`; the
`…` is unchanged status fields, labelled as such in the caption). **No
`playerctl` transcript** — Interpretation A did not ship, so a bus-client
session would have been fabricated. Re-verified after the edit: `web_check` @
360 px → **0 console errors, 0 failed requests**; `web_find_overflows` @
360/400/720/1080 → **0 overflow**; `web_performance` @ 1440 →
`horizontal_overflow: 0`, **0 images over 300 KB**, 0 images missing
dimensions, 0 problems. `requests_total: 11` (1 document + 10 same-origin
`img/*.png`) confirms **zero external-origin fetches** — the `github.com` /
`crates.io` entries the checker lists are `<a href>` anchor destinations, not
subresources. Palette unchanged (CSS vars), favicon `data:` URI untouched,
motion unchanged (this section is not animated).

### Not run / deferred

- **No D-Bus / `playerctl` transcript.** B is a command surface, not a bus
  service; there is nothing to transcript. A (real MPRIS over `zbus`) remains
  deferred on the measured numbers above.
- **No live-desktop render.** Captures ran headless (no `WAYLAND_DISPLAY` /
  `DISPLAY` in scope) so the daemon refuses to spawn and never touches the
  maintainer's wallpaper — position/slideshow state is a filesystem fact the
  CLI owns, which is exactly the design.
- The full **vision test rig** was not run (this change adds no imagery).

## Exact fractional buffers — viewporter + fractional-scale (2026-11-03, this change)

`wp_viewporter` + `wp_fractional_scale_v1` engaged when the compositor offers
**both** and `preferred_scale` is non-integer: buffer =
`round_half_away(logical × frac)`, viewporter destination = the logical size,
buffer scale 1. The legacy integer path is the fallback and the code path
whenever a protocol is missing or the scale is integer — its geometry formula
is untouched, so 1.0/2.0 behavior is byte-for-byte unchanged. Design in
[SCALING.md](SCALING.md).

### Binding decision, with numbers

Both protocols live in the **already-in-tree** `wayland-protocols` 0.32:
`wp::viewporter` (stable, no feature) and `wp::fractional_scale` (staging,
behind the crate's own `staging` feature). `wayland-protocols-experimental`
was **not needed** — enabling a feature of an in-tree crate costs nothing at
runtime:

| Measure | Before | After | Delta |
|---|---|---|---|
| `cargo tree --workspace` nodes | 391 | 391 | **0** (feature flag, no new crate; `Cargo.lock` untouched) |
| Default `--release` binary | 5,155,480 B (`e18a999ed6b0…`) | **5,190,680 B** (`18c99f347f81…`) | **+35,200 B (~34.4 KiB)** |
| MSRV | 1.85 (workspace) | unchanged | `wayland-protocols` 0.32.12 (lockfile pin) declares `rust-version` **1.71** ≤ 1.85; the new code is plain integer arithmetic + existing bindings |

MSRV caveat (pre-existing, not introduced here): the lockfile as resolved
already cannot *build* on 1.85 — `image 0.25.10`, `ratatui 0.30.2` and
`time 0.3.55` declare `rust-version` 1.88 (verified with a real 1.85.1
toolchain on this same lockfile, which my diff does not touch). The 0.32.13
release of `wayland-protocols` declares 1.86; the lockfile pin (0.32.12) is
what keeps the declared MSRV story — a `cargo update` would break it
regardless of this change.

Protocol note: `preferred_scale` is the merged spec's **u32 numerator over
denominator 120** (1.5 ⇒ 180) — the wl_fixed encoding from the drafts never
shipped (verified against the wayland-protocols XML and sway/niri on the
wire). Rounding rule: nearest, halves **away from zero** (the spec's own rule
for surface sizes), exact integer math `(2·logical·n + 120) / 240`.

### Path-selection rule (unit-tested truth table)

| viewporter | fractional-scale | negotiated scale | path |
|---|---|---|---|
| ✓ | ✓ | non-integer ≥ 1.0 (e.g. 180/120) | **exact** |
| ✓ | ✓ | 120/120 (1.0) or 240/120 (2.0) | legacy |
| ✓ | ✓ | no `preferred_scale` yet | legacy |
| ✓ | ✗ | any | legacy |
| ✗ | ✓ | any | legacy |
| ✗ | ✗ | any | legacy |
| ✓ | ✓ | degenerate 0 or < 1.0 | legacy |

Unit tests (`layer.rs scale_tests`): `buffer_dims_exact_at_common_scales`,
`viewport_destination_is_the_logical_size`,
`buffer_dims_round_half_away_from_zero` (2×1.25=2.5→3, 6×1.75=10.5→11,
191×1.5=286.5→287, 190×1.75=332.5→333),
`path_selection_requires_both_protocols_and_fractional_scale` (full table),
`integer_scales_keep_the_legacy_geometry_byte_for_byte`,
`path_selection_is_a_pure_function_of_its_inputs` (no cache thrash: the path
is a pure function of (globals, scale), so a settled output cannot flip
paths between renders; cache keys name concrete sizes — container-confirmed
`v2-16-fit-3840x2160-auto.png`). Wire contract (`ipc.rs`):
`output_scale_fields_are_skipped_when_absent`,
`old_output_shape_deserializes_to_none`, `exact_path_output_round_trips`.

### `status --json` field shape (additive, skip-if-absent — media-keys pattern)

```json
"outputs": [{
  "name": "HEADLESS-1", "width": 3840, "height": 2160, "scale": 2,
  "variant": "16/fit-auto",
  "scale_path": "exact",
  "frac_scale": "1.50"
}]
```

`width`/`height` are the buffer dims; `scale_path` is `"exact"` or
`"legacy"` on the layer backend, absent on x11; `frac_scale` appears once
the compositor sent `preferred_scale`. A pre-feature daemon omits both; an
old client ignores them. The human `status` annotates only the exact path
(`scale 1.50 exact`); every other line is unchanged.

### Container proof — headless sway (this box, sway 1:1.12-4, wlroots 0.20)

`test/verify-wayland-fractional.sh` + `test/wayland-fractional/{Dockerfile,inside.sh}`
(Arch container, `WLR_BACKENDS=headless WLR_RENDERER=pixman`, hermetic
`PIXPAPERS_HOME`, host-built binary mounted ro; skips with a message when
Docker is absent/unreachable; containers `--rm`, named `pixpapers-ff-*`).
Reproduce:

```sh
cargo build --release
PIXPAPERS_BASELINE_BIN=/path/to/pre-feature/pixpapers \
  test/verify-wayland-fractional.sh target/release/pixpapers
```

| case (sway `mode` @ `scale`) | `scale_path` | `frac_scale` | buffer | daemon RSS |
|---|---|---|---|---|
| 1920x1080 @ 1.0 | `legacy` | 1.00 | 1920×1080 | 21,604 kB |
| **3840x2160 @ 1.5** | **`exact`** | **1.50** | **3840×2160** = round(2560×1440 × 1.5) | **70,128 kB** |
| 1920x1080 @ 2.0 | `legacy` | 2.00 | 1920×1080 | 21,428 kB |
| 3840x2160 @ 1.5, **pre-feature binary** (`e92a54a`) | field absent | — | 5120×2880 (oversample) | **120,504 kB** |

RSS pair at the **same virtual output** (same container, mode, scale;
`awk '/VmRSS/{print $2}' /proc/<daemon_pid>/status` inside the container):
**exact 70,128 kB < legacy 120,504 kB**. Container-relative — the dev
panel's ~121 MB measured / ~70 MB predicted figures reproduce almost
exactly here, but the panel numbers remain the operator's confirmation.
Buffer dims identical across two `status --json` calls per case (`stable=yes`
in the script output) — no path thrash. sway's `mode` is physical: at scale
1.5 the logical size is 2560×1440, so the exact buffer lands on the panel
grid — the dev-panel case reproduced. The pre-feature binary's log shows the
old `(scale 2)` configure line and no fractional negotiation — the legacy
path is what a compositor without the protocols will drive.

### Gates run

`cargo build` + `cargo build --release` clean · `cargo test --workspace`
green (**219** = 116 pixcore + 83 pixpapers bin + 10 slideshow + 10
media_keys; +9 this change) · `cargo test --workspace --features gui` green
(**224**, +5 `gui::model`) · `clippy --workspace --all-targets
--all-features -- -D warnings` clean · `cargo fmt --all --check` clean ·
`env -u WAYLAND_DISPLAY test/smoke.sh` ok · `test/verify-x11.sh` ok (x11
backend untouched — its `OutputStatus` gains only the two `None` fields) ·
`test/verify-tui.sh` ok (10 goldens green; no new keys leaked into the TUI
keymap) · `test/verify-wayland-fractional.sh` ok (table above).

### Website (`docs/index.html`, one file)

Text-only touch: the four images that shipped with empty `alt=""` (three
slideshow-cycle duplicates + the gutter wipe overlay) got real descriptions
— the pre-existing a11y nit (20 images, 4 empty alts → **0 missing/empty**).
No content section added (the site has no scaling section; contract
unchanged). Re-verified after the edit: `web_check` @ 360 px → 0 console
errors, 0 failed requests, `img_missing_alt: 0`; `web_find_overflows` @
360/400/720/1080 → 0 overflow; `web_performance` @ 1440 →
`requests_total: 11` (1 document + 10 same-origin `img/*.png`), **zero
external-origin fetches**, 0 images over 300 KB, 0 missing dimensions,
palette/favicons/motion untouched.

### Operator live-panel checklist (niri ≥ 26.04 exposes both protocols)

Nothing here was run on the live desktop by this session — this box had no
live Wayland session. Copy-paste, read-only:

```sh
niri --version                                   # needs >= 26.04 for fractional-scale
pixpapers status --json | python3 -c 'import json,sys; o=json.load(sys.stdin)["outputs"]; print([(x["name"],x["width"],x["height"],x.get("scale_path"),x.get("frac_scale")) for x in o])'
# expect on the 3840x2160 @ 1.5 panel: ("…", 3840, 2160, "exact", "1.50")
pid=$(pixpapers status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["daemon_pid"])')
grep VmRSS /proc/$pid/status                   # expect ~70 MB, down from the measured ~121 MB
```

If `scale_path` reads `"legacy"` with `frac_scale` absent, the compositor is
older than its fractional-scale support — behavior is the unchanged v0.2 path.

### Not run / deferred

- **The operator's live niri panel** (checklist above): visual crispness at
  1.5 on real hardware and the live RSS drop are the operator's confirmation;
  the container proves the protocol negotiation, buffer math, path selection,
  stability and the RSS direction, not pixels on the panel.
- **Compositors without fractional-scale** (river, weston, cage…) were not
  booted; the legacy path there is the untouched pre-feature code path plus
  two `None` fields, covered by the byte-identical legacy geometry tests and
  the 1.0/2.0 container cases.
- **Multi-output** remains structurally handled, untested on real hardware
  (unchanged caveat; the container testbed is single-output).
- **headless-sway CI**: shipped after this section — the `compositor` job
  wired all three headless gates into CI; see "CI compositor gates" below.

## MSRV honesty — 1.85 → 1.88 (measured, CI-enforced) (2026-11-03, this change)

The declared floor was a lie: `rust-version = "1.85"` while the resolved
lockfile cannot build on 1.85 (the caveat the fractional-buffers section
above recorded). One honest option had to ship. **Chosen: raise the claim
to the measured floor, 1.88, and enforce it in CI.**

### The bisect (real toolchains, this lockfile, `--locked`)

| Toolchain | `cargo build --locked --workspace` (default features) |
|---|---|
| 1.85.1 | **refused** — `image@0.25.10`, `ratatui@0.30.2` (+`ratatui-core`/`-crossterm`/`-widgets`), `time@0.3.55`, `time-core@0.1.9` require rustc 1.88.0 |
| 1.86.0 | **refused** — same seven packages |
| 1.87.0 | **refused** — same seven packages |
| **1.88.0** | **green** — `Finished` dev profile, no warnings |

The `gui` feature's own floor, measured the same way
(`cargo check --locked -p pixpapers --features gui`):

| Toolchain | gui feature |
|---|---|
| 1.94.0 | **refused** — `eframe`/`egui`/`ecolor`/`egui-winit`/`egui_glow` 0.37.0 require rustc 1.95 |
| 1.95.0 | **green** — `Finished` dev profile |

### Why "downgrade back to 1.85" was measured infeasible, not waved away

- `ratatui` **0.30.0**, the oldest of the 0.30 line the TUI is built
  against, already declares `rust-version` **1.86** (crates.io API) — the
  whole line is above 1.85; reaching 1.85 means rewriting the TUI against
  0.29, not `--precise`-ing a patch.
- `egui` 0.37 (the `gui` feature) depends on `image ^0.25.10` (crates.io
  dependency API) — the lockfile can **never** resolve an `image` below
  the 1.88-declaring 0.25.10 while `gui` is a workspace feature. This is
  the same constraint AGENTS.md already records for the 0.25.9→0.25.10
  bump.
- `time` (pulled by `ratatui-widgets`) declares 1.88 across 0.3.51–0.3.55.

So option (a) was a TUI+GUI rewrite dressed as a `cargo update`; option
(b) is the honest number. Dev-only tooling stays under the floor
(`criterion` 0.8.2 declares 1.86 ≤ 1.88).

### What shipped

- `rust-version = "1.88"` in the workspace `Cargo.toml` (both crates
  inherit); README install line now says **≥ 1.88** and names the `gui`
  feature's 1.95 floor; AGENTS.md's historical 1.85 mentions are marked
  as then-declared.
- A `msrv` CI job that **reads the claim from Cargo.toml** and pins that
  toolchain for `cargo build --locked --workspace` — hard-fail. The job
  cannot drift from the claim because it parses it; a dependency bump
  that raises the true floor turns CI red and forces claim + mentions to
  move together. First runs recorded in the CI section below.

### The bump's own tax: MSRV-aware clippy lints fired (and were paid)

Raising the declared floor to 1.88 unlocked clippy's MSRV-aware lints —
with let-chains (1.88) and `u32::is_multiple_of` (1.87) now legal at the
floor, `collapsible_if` and `manual_is_multiple_of` fired across the
tree: **20 call sites** (pixcore: color/paths/pipeline/content/variant;
pixpapers: commands/layer/gui-model/status/tui-actions). All collapsed
mechanically per clippy's own suggestions; `clippy --all-targets
--all-features -D warnings` green again, and the new let-chain code
compiles on the real 1.88 toolchain (re-verified after the edits).

## CI compositor gates — design + graduation criterion (2026-11-03, this change)

The `compositor` job (`.github/workflows/ci.yml`) runs on every push/PR
in parallel with the untouched fast `check` job:

| Gate | Environment | Status at ship |
|---|---|---|
| `test/verify-tui.sh` | plain runner, real pty | **hard-fail** (graduated at first runner run — see evidence) |
| `test/verify-x11.sh` | Xvfb, **depth pinned 24-bit** — the script self-wraps `xvfb-run -s "-screen 0 1280x1024x24"`; the workflow greps that pin so a silent fallback to a runner default fails loudly (depth 16 refuses the truecolor backend — the script's own test 6) | **hard-fail** (graduated at first runner run — see evidence) |
| sway image build + `test/verify-wayland-fractional.sh` | Docker, sway 1.12 headless; buildx docker-container driver, layers cached with `gha` cache (`docker/build-push-action`, `load: true`); `PIXPAPERS_IMAGE_BUILT=1` makes the script use the pre-loaded image instead of rebuilding | soft-fail (continue-on-error) |
| `msrv` job | pinned toolchain read from Cargo.toml | **hard-fail** |

`PIXPAPERS_BASELINE_BIN` is deliberately unset in CI: the legacy+RSS pair
needs a pre-feature binary and CI does not rebuild old commits — the
script skips that sub-step by design (its output says so).

**Operator-only, still open after this wiring** (unchanged from the
fractional-buffers section): the **live niri panel crispness check** at
scale 1.5 and the **live RSS checklist** (`scale_path: "exact"` on the
real panel, VmRSS ~70 MB) — the container proves negotiation, buffer
math and RSS direction, not pixels on the maintainer's panel.

**Graduation criterion (written down, MUST-CARRY comments in the
workflow):** a soft-fail gate graduates to hard-fail after **5
consecutive runs on main where that gate's step succeeded** (a
continue-on-error step that failed shows a warning — warnings do not
count toward the streak). Each MUST-CARRY comment is removed in the
commit that graduates its gate.

### First runs (evidence)

| Run | HEAD | Result |
|---|---|---|
| [41704233611](https://github.com/lariocpt/pixelated-papers/actions/runs/41704233611) | `6010fa3` | `check` ✅ 2m32s · `install-check` ✅ 1m49s · `msrv` ✅ 1m10s (first enforcement of the 1.88 claim: green) · `compositor` ❌ — **verify-tui ✅ and verify-x11 ✅** on their first runner run, no flakes → graduated to hard-fail immediately, run cited in the workflow comments. The job's red came from the image-build step: `Cache export is not supported for the docker driver` — build-push-action needs a docker-container builder for `cache-to type=gha`. Fixed with `docker/setup-buildx-action@v3`; the build + fractional steps run soft-fail while the record builds. |
| [41706415063](https://github.com/lariocpt/pixelated-papers/actions/runs/41706415063) | `5743aa2` | **all four jobs green**: `check` ✅ 3m · `msrv` ✅ 1m · `install-check` ✅ 2m · `compositor` ✅ 6m (uncached sway-image build on the new buildx builder — the gha cache was empty). Fractional gate genuinely exercised on the runner: pre-built image used, `3840x2160 @ 1.5 → exact path, buffer 3840x2160 … no thrash`, baseline+RSS sub-step skipped by design. |
| [41708852977](https://github.com/lariocpt/pixelated-papers/actions/runs/41708852977) | `701b10a` | **all four jobs green** — `compositor` ✅ **2m** with `importing cache manifest from gha` (the Arch+sway layers were NOT rebuilt: 6m → 2m, the layer-cache design goal proven); fractional gate green again (`verify-wayland-fractional: ok`). **Fractional-gate streak on main: 2/5 as of this commit** (41706415063, 41708852977); subsequent green runs extend the streak toward graduation. |
