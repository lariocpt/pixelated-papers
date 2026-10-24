# Troubleshooting

### `pixpapers status` says "not running" but I set a wallpaper

`set` auto-spawns only when the spawning shell/autostart has
`WAYLAND_DISPLAY` in its environment. Check:

```sh
echo $WAYLAND_DISPLAY          # e.g. wayland-1 — systemd user services often DON'T have it
pixpapers status
```

If it's missing in your autostart context, either export it there
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

**Normal at logout / niri restart.** The daemon tears down its surfaces,
socket and pidfile and exits 43 so supervisors can tell "session ended"
from "crashed". It must NOT be restarted into a dead session: the contrib
systemd unit sets `RestartPreventExitStatus=43`. Your next `set` in the
next session respawns it.

### systemd unit flaps (start/stop/start…)

You put `Restart=always` without `RestartPreventExitStatus=43`, or two
autostart mechanisms race (niri `spawn` *and* the unit). Keep one —
the unit — or accept the loser of the socket race exiting 1 (harmless).
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
