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
