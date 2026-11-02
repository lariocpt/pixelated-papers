# CLI reference — pixpapers v0.2

Everything the binary accepts. Kept in sync with `pixpapers --help` by
hand — the clap definitions are the source of truth, so check flags here
against `--help` in the same tree before relying on them.

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
| `remove <ID>` | Delete entry: original + all cached variants; no dir or manifest residue. Removing the current entry is allowed — it clears `current`, and both backends then skip rendering (no current entry), so the screen keeps the last rendered wallpaper until you `set` another. The TUI's `D` confirms first and Applies only when the removed entry was current. |

## Applying

| Command | Meaning |
|---|---|
| `set <ID>` | Make it the current wallpaper and apply now. **Auto-starts the daemon** when `WAYLAND_DISPLAY` or `DISPLAY` is set; without a session the config is still written (honest error note). |
| `cycle [BY]` | Rotate the current pointer by N (default 1) through the library. |
| `enable` / `disable` | Global pixelation policy. `disable` = render originals only (placement/bg still honored). No other way to escape pixelation except per-wallpaper `pixel normal`. |
| `stop` | Ask the running daemon to quit (IPC `Quit`). Surfaces, socket and pidfile are removed. |

## Per-wallpaper knobs (apply immediately if current)

| Command | Values | Notes |
|---|---|---|
| `pixel <ID> <P>` | `8 16 32 64 128 normal auto` | Block size in pixels. `normal` = no pixelation for this wallpaper. `auto` = the preset is picked per output at render time: the smallest of 8/16/32/64/128 whose grid keeps the output's long edge at ≤ 120 blocks (≤960→8, ≤1920→16, ≤3840→32, ≤7680→64, else 128 — so 1920×1080 gets 16, the shipped default). Bigger output ⇒ coarser blocks, same blocky look; `auto` never picks `normal`. The resolved preset shows in the daemon's status label and in `export` output; the variant cache always stores concrete presets. |
| `placement <ID> <PL>` | `fit cover-gutter cover-crop stretch` | See below. |
| `bg <ID> <COLOR>` | `#rrggbb` | Letterbox / gutter color. |
| `gutter <ID> <PX>` | `0..=4096` | Mat width for `cover-gutter` (px in output space). |
| `gutter-style <ID> <ST>` | `solid bleed` | How the `cover-gutter` ring is filled. `solid` = the flat `bg` colour (default); `bleed` = the wallpaper's own edges blurred and snapped to the picture's palette, so the ring stays pixelated. The centre is byte-identical either way. Aliases: `mat`/`flat`→`solid`, `blur`/`blurred`→`bleed`. |
| `palette <ID> <M>` | `off auto nes gb cga ega vga gen16` | Quantization look. `off` = plain RGBA; `auto` = per-image median-cut ≤ 256; `nes` = the 64-entry 2C02 master table (the canonical 192-byte `.pal`); `gb` = DMG four greens; `cga`/`ega`/`vga` = fixed tables; `gen16` = per-image 16, snapped onto the RGB555 grid. Vibe, not PPU emulation. On `normal` the look is applied to the **full-size canvas** (one-time, then cached like any variant); `auto` + `none` is the v0.1-identical default (plain crop, palette-encoded at encode). |
| `dither <ID> <D>` | `none fs atkinson bayer2 bayer4 bayer8` | Applied after palette mapping, in canvas space (deterministic, integer math). `none` = pure nearest (the v0.1 look); `fs` = Floyd–Steinberg; `atkinson` = Atkinson; `bayer2/4/8` = ordered screens. On `normal` the dither runs over the full-size canvas (one-time, then cached); with palette `off` it is inert (no palette to diffuse into). |

## Defaults

| Command | Meaning |
|---|---|
| `default pixel <P>` | Default preset for new entries and entries never set explicitly. Accepts `auto` (see `pixel`). |
| `default placement <PL>` | Same for placement. |
| `default palette <M>` | Same for the palette look. |
| `default dither <D>` | Same for the dither. |
| `default gutter-style <ST>` | Same for how the `cover-gutter` ring is filled (`solid`/`bleed`). |

Changing a default re-applies to any *unset* current wallpaper immediately.

## Slideshow (live wallpapers)

A still-image slideshow cycling the library's entries in `list` order. State is *not* kept in the daemon: the active wallpaper is computed at render time from the wall clock, `active = (unix_now / interval_secs) % entry_count`, so the phase is stable across restarts and the daemon stays a pure renderer. Because the phase is aligned to the Unix epoch, enabling mid-slot shows that slot and the first change can land in under a full interval.

| Command | Meaning |
|---|---|
| `slideshow on [SECONDS]` | Turn the cycle on. Optional `SECONDS` sets the interval in the same call. Auto-starts the daemon like `set`. |
| `slideshow off` | Stop cycling; the current wallpaper stays put. |
| `slideshow interval <SECONDS>` | Change the cadence (1–86400). `0` is a user error (exit 1). |

TOML (`config.toml`): `[slideshow]` with `enabled` (bool, default `false`) and `interval_secs` (u64, default `300`). `status` prints `slideshow: on · next change in Ns` (and `status --json` gains `slideshow_enabled` + `next_change`) only while it is on — the off output is unchanged. Only **still** images cycle; animated/video sources are out of scope (no decoder dependency).

## Diagnostics & offline

| Command | Meaning |
|---|---|
| `status [--json] [--bench]` | Daemon truth when running (pid, outputs with live render label, enabled policy); filesystem view otherwise. The label is `{pixel}/{placement}-{palette}[-{dither}]` — e.g. `16/fit-auto`, `16/fit-vga-fs` — the same tokens as the variant cache key (the dither segment is omitted when `none`). For an `auto` entry the label's pixel slot names the preset resolved for that output (e.g. `32/fit-auto` on a 4K screen), so text and `--json` both show the look actually on screen. `--bench` adds self-timed switch/render latencies, variant-cache hit rate, RSS + fd count. |
| `bar` | One line for a bar — the [waybar module's](contrib/waybar/README.md) text source. Character-exact: `▞ <title> · <pixel>/<placement-short>` while a current entry exists (even with the daemon dead — config+manifest truth through the daemon's own resolution chain, so the label never shows a look the daemon would not render), `▞ pixpapers — set one` with no entry and the daemon alive, `▞ pixpapers · daemon down` with the socket dead. Always exactly one line, always exit 0 (waybar's custom module auto-hides on empty output or a non-zero exit, so the module can never blink). Never writes, never spawns; works offline. |
| `export <ID> <WxH> [--pixel P] [--placement PL] [--bg C] [--gutter N] [--gutter-style ST] [--palette M] [--dither D] [-o OUT]` | Offline render to PNG using the daemon's exact composite path — a preview that cannot lie. No daemon needed. `--palette`/`--dither`/`--gutter-style` override this render only (never persisted). `--pixel` accepts `auto` too: it resolves against the requested `WxH` (the daemon's exact rule), the wrote-line shows `auto→N`, and the default filename carries the concrete preset. Named modes write a palette PNG (PLTE = the table deduped by first occurrence, a transparent placeholder at index 0, and the bg colour merged in when absent); `off`/`auto` stay plain RGBA. |
| `optimize [--all]` | Re-run the storage optimizer (PNG re-encode / strip cruft) over the library; `--all` includes entries unchanged since import. |
| `completions <bash|zsh|fish>` | Shell completion script to stdout. |

## TUI

| Command | Meaning |
|---|---|
| `tui` | Interactive terminal front-end (PLAN-30 §C): library list left, **offline preview** right (the exact `export` render path — a preview that cannot lie), status bar `daemon:up/down · img:<proto> · last action`. Works with the daemon dead (the status bar says so). Ships behind the default-on `tui` cargo feature. |

Keys — the full action table (S7). Every action goes through the same
`commands::*` function the CLI subcommand calls, then the same `Apply`
seam — a knob edited here is byte-identical to the same edit via CLI,
and a dead daemon only changes the Apply wording, never the file
writes. All results land in the status bar; on the alternate screen
nothing is printed to the terminal (errors included).

| Key | Action | Status line (success) |
|---|---|---|
| `a` | Add by path — input line; type/paste an image path (tilde-expanded), Enter | the CLI's own line: `added <id> · <title> · <W>×<H> · <optimize note> · <KB> KB`. Re-adding the same bytes is not an error: `already in library: <id> (<title>)`. `add` does not set current and does not Apply (CLI semantics) |
| `s` | Set the selected entry current | `set <id> · <title> (applied)` — `(recorded — will show when the daemon runs)` when the daemon is down and cannot be spawned |
| `c` | Cycle current to the next entry (1 step) | `cycle → <id> · <title> (<state>)` |
| `p` | Cycle pixel `8→16→32→64→128→normal→auto` | `<id> → pixel <v>` (plus ` (not current — will apply on set)` when the edited entry is not current). The caption shows an `auto` entry's resolved preset for the preview canvas (`pixel auto→8`) |
| `l` | Cycle placement `fit→cover-gutter→cover-crop→stretch` | `<id> → placement <v>` |
| `t` | Cycle palette `off→auto→nes→gb→cga→ega→vga→gen16` | `<id> → palette <v>` |
| `d` | Cycle dither `none→fs→atkinson→bayer2→bayer4→bayer8` | `<id> → dither <v>` |
| `b` | Prompt bg colour — input line, `#rgb` or `#rrggbb` | `<id> → bg <#rrggbb>` (`#abc` stores `#aabbcc`) |
| `g` | Prompt gutter px — input line | `<id> → gutter <n>px` |
| `G` | Cycle gutter style `solid→bleed` — the `cover-gutter` ring fill | `<id> → gutter-style <v>` (plus ` (not current — will apply on set)` when the edited entry is not current) |
| `D` | Remove the selected entry — confirm line in the status row: `remove <id>? y/N`; `y`/`Y` confirms, anything else cancels | `removed <id> (<title>)` — `Apply` only when the removed entry was current. Single key on purpose: `rm` would collide with `r` (refresh) |
| `e` | Toggle pixelation on/off (config) + Apply | `pixelation enabled` / `pixelation disabled — originals only (still stretched/placed)` |
| `x` | Export the selected entry at the preview canvas to the download dir (`$XDG_DOWNLOAD_DIR` → `~/Downloads` → `.`) | the CLI's `wrote <dest> · <W>×<H> · <pixel> <placement> · <KB> KB` (not a mutation: no reload, no Apply) |
| `r` | Refresh — re-read manifest + config, re-probe the daemon (no mutation, no IPC) | `refreshed` |

Navigation: `↑/k` `↓/j` select · `Ctrl-L` repaint · `q` / `Esc` /
`Ctrl-C` quit (exit 0, alternate screen restored).

The input line (`a` / `b` / `g`) is one shared buffer: printable keys
insert at the cursor; `Backspace`, `←`/`→`, `Home`/`End` edit; **Enter
submits; `Esc` / `Ctrl-C` cancel; `q` cancels the line too but does NOT
quit** while it is up (`Ctrl-Q` is ignored). A paste arrives as a burst
of key presses and assembles as-is. Bad values (no such file, wrong
extension, unparsable colour, non-numeric gutter) are status-bar
errors — nothing is stored, nothing is applied.

Preview: generated only when the terminal speaks an image protocol
(table below); re-rendered on selection change, `r` refresh, or resize —
never on a timer. The caption block (output size · pixel · placement /
palette · dither / bg · gutter / gutter style) is always visible, image or not. An
encoded preview over **256 KB** is refused with the reason (keeps a
25-column SSH terminal from exploding). Without an image protocol the
pane shows one honest line instead: `image preview unsupported here —
pixpapers export <id> WxH -o /tmp/prev.png`. No fake art.

Image-protocol detection (startup only; result shown as `img:` in the
status bar, never guessed silently):

| Signal | Protocol |
|---|---|
| `KITTY_WINDOW_ID` / `TERM_PROGRAM = kitty` | kitty graphics APC `ESC_G … ESC \` (inline PNG, `a=T,f=100`) |
| `ITERM_SESSION_ID` | iTerm2 inline: `OSC 1337;File=inline=1` |
| `SIXEL` set / `$TERM` contains `sixel` | sixel: `ESC P q … ESC \` |
| anything else | text fallback (`img:none`) |

`PIX_PAPERS_IMAGE = none|kitty|iterm2|sixel` overrides detection — forces
a protocol or the fallback (the test lever and the escape hatch for a
false positive).

## Native GUI

| Command | Meaning |
|---|---|
| `gui` | Native egui/`eframe` control window: Library tab (list + cached thumbnail + set/cycle/remove), a per-wallpaper knob panel, a Status tab, the global Pixelation switch and the slideshow interval. **Ships behind the default-off `gui` cargo feature** — the default binary has no `eframe` dependency and does not list this subcommand in `--help`. Build it with `cargo build --release --features gui`. |

Like the TUI, the GUI is a **client, not a second owner of state**: every
button routes through the same `commands::*` function the matching CLI
subcommand calls, then the same `cli::apply_now` seam — a wallpaper set
from the window writes a byte-for-byte identical manifest to `pixpapers
set` (asserted in `src/gui/model.rs` tests), and a dead daemon only changes
the wording, never the file writes. The window never renders a full-size
image: thumbnails reuse the cached variant PNGs. A dirty edit gates Save
and reloads from disk on refresh (the on-disk manifest is authoritative).

## Waybar

`contrib/waybar/` ships a toolbar module (config snippet + README):
one button showing the current wallpaper — `▞ <title> ·
<pixel>/<placement-short>` (e.g. `▞ nebula · 16/fit`), the two `bar`
no-entry states otherwise. Click opens `pixpapers tui` in
`${TERMINAL:-foot}`; the mouse wheel cycles (`on-scroll-up cycle 1`,
`on-scroll-down cycle -1` — direction-aware, signed step, wrap-around).
The label re-runs every 30 s and again on each click/scroll
(`exec-on-event`) — not push-based, so a CLI/TUI `set`/`cycle` flips
the label on the next tick, within at most one interval. No daemon
required: `bar` is an offline config+manifest read.

The snippet targets waybar ≥ 0.14 (tested on 0.15.0): the standalone
`exec` module type was removed there, so the object lives under
`custom/pixpapers` (0.13 and older: the same object under the bare
`pixpapers` key). `format` must stay `"{text}"` — positional forms
like `"{ }"` are rejected by the newer libfmt that current waybar
links against (fmt 12 on the tested 0.15.0), and the module then stays
**silently hidden** (it only logs `invalid format string`). Install,
terminal, refresh, auto-hide and escape notes:
[contrib/waybar/README.md](contrib/waybar/README.md).

## Daemon (foreground)

| Command | Meaning |
|---|---|
| `daemon [--backend <auto\|layer\|x11>]` | Run the daemon in the foreground. You rarely type this: `set`/`cycle`/knobs spawn it detached. Foreground use is for logs/debug (`PIXPAPERS_LOG=debug`). `--backend` overrides the `daemon.backend` config for testing. |

```
pixpapers daemon
pixpapers daemon --backend x11   # force the X11 backend (testing)
```

Backend selection (`daemon.backend` in config.toml, default `auto`):

| Value | Behavior |
|---|---|
| `auto` | Probe at daemon start: `WAYLAND_DISPLAY` set ⇒ `layer`; else `DISPLAY` set ⇒ `x11`; else exit 1 naming both. A wrong guess is a loud user error — there is no silent fallback chain. |
| `layer` | wlr-layer-shell Background surfaces (wayland compositors: niri & friends). |
| `x11` | Root-window wallpaper via PutImage (XRGB8888, depth-24 roots; a non-24-depth root is a loud exit 1). One daemon renders the same wallpaper on every screen; reacts to Expose and RFPWM resize hints, never initiates a resize. |

`status` reports the active backend (`backend` field / `daemon: running (pid N, backend x11)`).

## Exit codes

The classifier is `is_user_error` (crates/pixpapers/src/commands.rs): a
typed `PixError` exits 1 unless it is the catch-all `Other`;
`DaemonStartupError` and `UserError` exit 1; a config-load failure in
`main` exits 1. Everything else exits 2.

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | user error — the user said or provided something wrong: no entry matching an id, empty library, bad size `W×H` / colour / value, missing or undecodable image file, bad config or manifest, no display session (auto-detect names both variables), non-24-depth root (x11), daemon startup failure (refused connection, no backend) |
| 2 | internal error — no user mistake: encoder failures (`PixError::Other`: PNG/JPEG re-encode at `add`, the variant encoder), bare I/O outside the typed paths (e.g. `export -o` cannot write its destination), a second **foreground** `daemon` finding the live socket (`already running`), clap usage/parse errors (framework convention) |
| 43 | **daemon only**: the display server went away (wayland compositor / niri restart / X server killed). Teardown ran; socket/pid removed. A supervisor should NOT restart the daemon on 43 — there is no session to draw on. |

## Environment

| Var | Effect |
|---|---|
| `WAYLAND_DISPLAY` | wayland session; with `daemon.backend = auto` it wins over `DISPLAY`. Absence of both = headless config-only mode for non-daemon commands |
| `DISPLAY` | x11 session; used by auto-detect when `WAYLAND_DISPLAY` is unset |
| `PIXPAPERS_HOME` | hermetic root replacing data+config+state (tests, portable setups): `<root>/{config.toml,data/,state/}` |
| `PIXPAPERS_LOG` | daemon log filter (`error..trace`, default `info`) |
| `PIX_PAPERS_IMAGE` | `tui` only: force the image protocol (`none\|kitty\|iterm2\|sixel`), overriding auto-detection |
| `XDG_*`/`HOME` | standard placement (`~/.config/pixpapers/config.toml`, `~/.local/share/pixpapers`, `~/.local/state/pixpapers`) |

## Files

```
~/.config/pixpapers/config.toml          defaults + policy (written atomically)
~/.local/share/pixpapers/library.json    manifest (versioned; atomic)
~/.local/share/pixpapers/library/<id>/   original.* + variants/v2-16-fit-g24-120x68-auto.png …
~/.local/state/pixpapers/sock            IPC (JSON-lines: Apply/Status/Quit)
~/.local/state/pixpapers/pid             pidfile
~/.local/state/pixpapers/log             auto-spawn log
```

Variant keys are `v2-<pixel>-<placement>[-g<g>]-<W>x<H>-<Q>[-<D>].png`:
`<Q>` is the palette token (always present — `-auto`, `-nes`, …) and
`<D>` the dither token, present only when the dither isn't `none`. The
`v2` prefix invalidates v0.1 caches (they are disposable). `<pixel>` is
always a concrete preset: an `auto` entry resolves per output size
before the key is built, so `auto` shares the concrete preset's cache
entry and never names a file.
