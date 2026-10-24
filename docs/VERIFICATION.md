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
3. **Empty input region** (`daemon/wayland.rs`) — belt for lax compositors
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
