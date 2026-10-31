# Compositors

## niri (primary dev target)

**Tested with niri 26.04 (8ed0da4)** — everything claimed in
[VERIFICATION.md](VERIFICATION.md) was measured on it. Older niri builds
served the same `zwlr_layer_shell_v1` version we bind (we use ≤ v5
requests); nothing niri-specific is version-sensitive in v0.1, but say so
in bug reports anyway.

### How the wallpaper lands

`pixpapers daemon` binds `wl_compositor`, `wl_shm`, `zwlr_layer_shell_v1`
and creates **one Background-layer surface per output**, anchored on all
four sides, `set_size(0,0)` (fill the output), `exclusive_zone(-1)` (create
no reserved zone — a wallpaper shouldn't shrink the panel gap),
`KeyboardInteractivity::None`, and an empty input region. niri composites
it below everything; `niri msg layers` shows:

```
  layer Background at 100:
    surface 6@(0,0) namespace "pixelated-papers" ...
```

If another Background surface exists (see below), niri keeps both — the
**topmost commits last wins visually**.

### Conflicts you should know about

- **noctalia-wallpaper / hyprpaper / swaybg / any other layer-shell
  wallpaper**: same layer, same geometry. They fight; the last rendered
  frame wins and can reassert on every monitor event. Run exactly one
  wallpaper daemon. To switch to pixpapers: stop the other one *and*
  remove its autostart entry, then `pixpapers set <id>`.
- **Fullscreen games**: Background layer stays below; verified
  structurally (protocol), not game-tested (V10 manual).
- **Overview/backdrop**: niri draws Background layers behind the overview
  with its own blur. We did not screenshot-proof this (V8 deferred);
  users report it behaves like any wallpaper. No niri config stanza was
  required for the tested setup — we don't recommend config we haven't
  tested.

### Autostart

Either niri autostart:

```kdl
// ~/.config/niri/cfg/autostart.kdl (niri ≥ 48ish syntax)
spawn "pixpapers" "daemon"
```

or the systemd user unit in [`contrib/pixpapers.service`](../contrib) —
the unit matters on niri where `spawn` entries get re-run on config
reload: if both fire, the second `daemon` loses the socket race and
exits 2 with "a pixpapers daemon is already running" (harmless, logged;
the S9 exit-code audit kept this at 2 — a duplicate start is an
environment race, not a user condition. The "exits 1" this section used
to claim was a doc bug, bare `anyhow::bail!` since v0.1 `10dbc8b`).

**Do not** add `Restart=always` without `RestartPreventExitStatus=43`
(see unit): on session end the daemon exits 43 on purpose; restarting it
there flaps against a dead session. On logout, `systemctl --user stop
pixpapers` (the unit ships `ExecStop` doing the polite IPC stop).

## sway — untested here

Same protocol. Expect identical behavior; if it misbehaves, open an issue
with `sway -v` + `pixpapers status`. Headless sway smoke is v0.2 CI work.

## hyprland — untested here

Same protocol. Two hyprland-specific notes:

- **`hyprpaper` will fight us** (same background layer). Pick one;
  `hypaperctl`/`hyprpaper` unload all + autostart edit.
- If a layer rule misbehaves, hyprland's `layerrule` can pin
  `pixelated-papers` below everything — start without rules first; we
  don't ship rules for an untested compositor.

## Generic wlroots (river, labwc, wayfire, cosmic…)

Expectation: works — Background + no-input is core protocol surface used
by every wallpaper tool. Untested. `pixpapers export` (any PNG-consumer)
is the universal fallback.

## X11 (root window) — shipped in v0.2

`85e59ed` (backend), `b549390` (auto-detect + config), `1821986`
(evidence); PLAN-30-ct §B.2/§B.4. The daemon renders the same wallpaper
on **every screen's root window** — X11's analog of the layer
backend's multi-output behavior — through the *same*
`composite_into` the wayland path uses (invariant #2), into a
client-side XRGB8888 buffer, blitted with `PutImage` (BigReq-aware row
bands). No Xv, no GPU — the backend does not depend on either.

- **Auto-detect** (`daemon.backend`, default `auto`;
  `daemon --backend <layer|x11>` overrides for testing):
  `WAYLAND_DISPLAY` set ⇒ layer; else `DISPLAY` set ⇒ x11; else exit 1
  naming both. A wrong explicit guess is a loud user error (exit 1) —
  there is no silent fallback chain. Since v0.2, `set`/`cycle`/knobs
  also auto-spawn the daemon under `DISPLAY` alone.
- **Depth-24 contract**: the buffer is XRGB8888 on a depth-24 root.
  Any other depth is refused at connect — loud exit 1: "screen N root
  depth is D, but pixpapers' x11 backend needs a 24-bit truecolor
  display — configure your X server for depth 24 and restart".
  Tested, not hoped for.
- **Scale**: X11 has no fractional scale — logical = physical, so the
  buffer is the full root at 1:1. `Xft.dpi` is *not* read (the plan
  allowed treating it as a scale; the shipped default and the tested
  path is 1:1).
- **Resize**: we never initiate one. An `Expose` (a root resize always
  exposes) or an RFPWM/RRC resize ClientMessage makes the daemon
  re-query `GetGeometry` and re-render at the new size.
- **Compositor loss**: the X server dying (Xorg/Xvfb killed,
  display-manager bounce) ⇒ teardown — socket + pidfile removed — and
  **exit 43**, the identical contract to the wayland path (the same
  `RestartPreventExitStatus=43` note above applies).

Evidence: `test/verify-x11.sh` self-wraps in Xvfb and asserts six
things — `DISPLAY`-only start ⇒ backend `x11`; the root pixmap
byte-identical to the `export` oracle (AE=0); an RFPWM hint triggers a
re-render at the new size, again oracle-identical; clean stop leaves
the X server up; killing the X server ⇒ exit 43; a depth-16 root ⇒
exit 1 with the loud depth message. The live-session row (a real
Xwayland `DISPLAY=:0`) is recorded in
[VERIFICATION.md](VERIFICATION.md) §S5. Blit numbers are in
[PERF.md](PERF.md) ("X11 blit"). Multi-screen oddities beyond the
tested single/few-screen setups wait for field reports.

`export` remains the universal fallback for exotic X setups — it
produces exactly what the daemon would draw:

```sh
pixpapers export <id> 3840x2160 -o /tmp/wp.png   # or --pixel/--placement…
feh --bg-fill /tmp/wp.png                         # or xwallpaper, nitrogen…
```

Re-run on knob changes (`pixpapers export` is offline and instant for
cached variants).

## Filing a compositor bug

Include: compositor + version, output resolution/scale, `pixpapers
status` (+ `--bench`), `niri msg layers` / `swaymsg -t get_layers`
output, and whether another wallpaper daemon ever ran in that session.
