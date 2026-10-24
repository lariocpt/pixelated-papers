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
reload: if both fire, the second `daemon` loses the socket race and exits
1 with "already running" (harmless, logged).

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

## X11 — not supported (by design)

No X11 backend in v0.1. The escape hatch is the export command, which
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
