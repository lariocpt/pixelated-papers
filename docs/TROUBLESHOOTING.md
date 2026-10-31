# Troubleshooting

### `pixpapers status` says "not running" but I set a wallpaper

`set` auto-spawns only when the spawning shell/autostart has a display
variable in its environment — `WAYLAND_DISPLAY` (wayland, wins the
auto-detect) or `DISPLAY` (x11; since v0.2 `DISPLAY` alone is
sufficient). With neither, the config is still written but nothing is
applied (honest error note). Check:

```sh
echo $WAYLAND_DISPLAY          # e.g. wayland-1 — systemd user services often DON'T have it
echo $DISPLAY                  # e.g. :0 — an X/Xwayland session; auto-detect's second place
pixpapers status
```

If both are missing in your autostart context, either export one there
(niri `spawn` inherits the session env and is fine) or start the daemon
once from a session shell (`systemctl --user import-env WAYLAND_DISPLAY`
can help user services — see contrib unit notes).

### No wallpaper, daemon runs, `status` shows outputs

- **Another wallpaper daemon owns the Background layer** (noctalia-wallpaper,
  hyprpaper, swaybg, gdesklets-era junk). Compositors don't arbitrate the
  layer: last rendered frame wins. `niri msg layers` / `swaymsg -t
  get_layers` shows every namespace on Background; stop the other daemon
  AND its autostart, then `pixpapers stop && pixpapers set <id>`.
- **Pixelation off**: `pixpapers status` prints the policy line — if it
  says `OFF (originals only)` that's `disable` mode; `pixpapers enable`.
- **The entry is `normal`**: status output per-output label shows
  `normal/fit` — that wallpaper opted out. `pixpapers pixel <id> 16`.

### Daemon won't start: "a pixpapers daemon is already running"

Usually true, sometimes stale (machine lost power mid-frame). Test it:

```sh
pixpapers status        # answers → real daemon, use `pixpapers stop`
```

If status says not running yet `daemon` complains, the socket is a
corpse: the daemon probes it at startup (round-trip) and unlinks stale
ones automatically — if you still hit it, `rm ~/.local/state/pixpapers/sock`
is always safe while no daemon runs.

### Exit code 43 in logs ("compositor lost")

**Normal at logout / niri restart / X server death.** The daemon tears
down its surfaces, socket and pidfile and exits 43 so supervisors can
tell "session ended" from "crashed". On the wayland backend that is a
compositor exit or niri restart (live exercise is the manual V12); on
the x11 backend it is the X server itself dying (Xorg/Xvfb killed,
display-manager bounce) — that path is machine-tested:
`test/verify-x11.sh` kills the Xvfb out from under the daemon and
asserts exit 43 (VERIFICATION §S5). It must NOT be restarted into a
dead session: the contrib systemd unit sets
`RestartPreventExitStatus=43`. Your next `set` in the next session
respawns it.

### systemd unit flaps (start/stop/start…)

You put `Restart=always` without `RestartPreventExitStatus=43`, or two
autostart mechanisms race (niri `spawn` *and* the unit). Keep one —
the unit — or accept the loser of the socket race exiting 2 (harmless;
a duplicate start is an environment race, not a user condition — the
S9 exit-code audit kept it at 2).
On logout, `ExecStop` politely stops it; `StopExec` on session teardown
is covered by the unit stopping with the user slice.

### Config corruption

Both `config.toml` and `library.json` are written atomically (tmp+rename)
and version-guarded: a half-written or future-version file is refused
with an error, never silently rewritten. If you hand-edited into a bad
state: fix the file (it's TOML/JSON), or as a last resort
`pixpapers default …` regenerates config; library manifest survives
independently. Originals in the library dir are only ever replaced by
`optimize`, never rewritten lossy.

### The TUI preview shows a text line, not an image

`image preview unsupported here — pixpapers export <id> WxH -o
/tmp/prev.png` is the honest fallback, not a failure: your terminal
does not speak an image protocol the TUI can use (kitty's graphics
APC, iTerm2's inline file, or sixel — the status bar shows which one
won as `img:`; a plain xterm-256color-class terminal gets `img:none`
and this line). The caption block (size · look · colours) is still
there, and the line itself is the workaround: `pixpapers export`
writes the exact preview to a file any image viewer can open. If you
believe your terminal should qualify, `PIX_PAPERS_IMAGE=kitty|iterm2|sixel`
(or `none`) forces a protocol for testing — the detection table is in
CLI.md.

### It looks soft/blurry on a fractional-scale display

Expected micro-softness on the legacy path when the compositor reports a
rounded-up integer scale while the real scale is fractional (1.5 etc):
we oversample and the compositor downsamples once. Blocks stay blocks;
edges soften slightly. Details and the v0.2 fix: [SCALING.md](SCALING.md).

### Where's everything on disk?

`~/.local/share/pixpapers/` (library + variants), `~/.config/pixpapers/config.toml`,
`~/.local/state/pixpapers/` (sock, pid, log). `PIXPAPERS_HOME=<dir>` moves
all of it (hermetic tests/benchmarks). Deleting `variants/` is safe — they
regenerate lazily from originals.

### Gather this before filing a bug

`pixpapers status` + `pixpapers status --bench --json`, the daemon log
(`~/.local/state/pixpapers/log`), compositor + version, output
mode/scale (`wlr-randr` or `niri msg outputs`), and whether another
Background-layer surface exists (`niri msg layers`).
