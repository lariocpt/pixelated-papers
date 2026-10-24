# CLI reference — pixpapers v0.1.0

Everything the binary accepts. This file is kept in sync with
`pixpapers --help` by hand (a docs-drift CI check compares subcommand
names). The man page (`contrib/pixpapers.1`) is generated from the same
clap definitions, so flags here match the binary exactly.

## Global

```
pixpapers <COMMAND>
pixpapers -V | -h
```

## Library

| Command | Meaning |
|---|---|
| `add <PATH> [--title T]` | Add a wallpaper. The image is decoded, re-encoded optimized, content-hashed (blake3) and stored under `~/.local/share/pixpapers/library/<id>/original.*`. Duplicate content → existing id, no copy. Ids are the first 8 hex chars of the hash. |
| `list` | Library table: id, title, size, pixel, placement, current marker. |
| `remove <ID>` | Delete entry: original + all cached variants. Refuses to remove the current wallpaper while set (clear it first with another `set`). |

## Applying

| Command | Meaning |
|---|---|
| `set <ID>` | Make it the current wallpaper and apply now. **Auto-starts the daemon** when `WAYLAND_DISPLAY` is set; without a session the config is still written (honest error note). |
| `cycle [BY]` | Rotate the current pointer by N (default 1) through the library. |
| `enable` / `disable` | Global pixelation policy. `disable` = render originals only (placement/bg still honored). No other way to escape pixelation except per-wallpaper `pixel normal`. |
| `stop` | Ask the running daemon to quit (IPC `Quit`). Surfaces, socket and pidfile are removed. |

## Per-wallpaper knobs (apply immediately if current)

| Command | Values | Notes |
|---|---|---|
| `pixel <ID> <P>` | `8 16 32 64 128 normal` | Block size in pixels. `normal` = no pixelation for this wallpaper. |
| `placement <ID> <PL>` | `fit cover-gutter cover-crop stretch` | See below. |
| `bg <ID> <COLOR>` | `#rrggbb` | Letterbox / gutter color. |
| `gutter <ID> <PX>` | `0..=4096` | Mat width for `cover-gutter` (px in output space). |

## Defaults

| Command | Meaning |
|---|---|
| `default pixel <P>` | Default preset for new entries and entries never set explicitly. |
| `default placement <PL>` | Same for placement. |

Changing a default re-applies to any *unset* current wallpaper immediately.

## Diagnostics & offline

| Command | Meaning |
|---|---|
| `status [--json] [--bench]` | Daemon truth when running (pid, outputs with live render label `16/fit`, enabled policy); filesystem view otherwise. `--bench` adds self-timed switch/render latencies, variant-cache hit rate, RSS + fd count. |
| `export <ID> <WxH> [--pixel P] [--placement PL] [--bg C] [--gutter N] [-o OUT]` | Offline render to PNG using the daemon's exact composite path — a preview that cannot lie. No daemon needed. |
| `optimize [--all]` | Re-run the storage optimizer (PNG re-encode / strip cruft) over the library; `--all` includes entries unchanged since import. |
| `completions <bash|zsh|fish>` | Shell completion script to stdout. |

## Daemon (foreground)

```
pixpapers daemon
```

You rarely type this: `set`/`cycle`/knobs spawn it detached. Foreground use
is for logs/debug (`PIXPAPERS_LOG=debug`).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | user error (bad id, bad value, missing file, remove-current) |
| 2 | internal error (io, decode, config corruption) |
| 43 | **daemon only**: the compositor went away (session end / niri restart). Teardown ran; socket/pid removed. A supervisor should NOT restart the daemon on 43 — there is no session to draw on. |

## Environment

| Var | Effect |
|---|---|
| `WAYLAND_DISPLAY` | required to spawn/render; absence = headless config-only mode for non-daemon commands |
| `PIXPAPERS_HOME` | hermetic root replacing data+config+state (tests, portable setups): `<root>/{config.toml,data/,state/}` |
| `PIXPAPERS_LOG` | daemon log filter (`error..trace`, default `info`) |
| `XDG_*`/`HOME` | standard placement (`~/.config/pixpapers/config.toml`, `~/.local/share/pixpapers`, `~/.local/state/pixpapers`) |

## Files

```
~/.config/pixpapers/config.toml          defaults + policy (written atomically)
~/.local/share/pixpapers/library.json    manifest (versioned; atomic)
~/.local/share/pixpapers/library/<id>/   original.* + variants/v1[-pal]-16-fit-g24-120x68.png …
~/.local/state/pixpapers/sock            IPC (JSON-lines: Apply/Status/Quit)
~/.local/state/pixpapers/pid             pidfile
~/.local/state/pixpapers/log             auto-spawn log
```
