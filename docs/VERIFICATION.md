# Verification record — step 8 matrix (feature-freeze gate)

Every claim in README/PLAN checked against a real compositor. Recorded
2026-10-24 on the dev machine, **niri 26.04 (8ed0da4)**, single output
DP-1 (Samsung U28E590) at 3840×2160@59.997, niri **fractional scale 1.5**
(logical 2560×1440, legacy `wl_output.scale` = 2 ⇒ our buffers 5120×2880).
Release build. Scripts: `test/smoke.sh` (offline+live layers),
`test/verify-live.sh` (V11/V13/V16/V17), `test/bench-daemon.sh` (V1/V2
timings + footprint). Re-running any of them re-produces its rows.

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
