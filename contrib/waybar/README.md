# pixpapers waybar module

One button in the toolbar showing the current wallpaper:
`▞ <title> · <pixel>/<placement-short>` (e.g. `▞ nebula · 16/fit`);
`▞ pixpapers — set one` when nothing is set, `▞ pixpapers · daemon down`
when the daemon is unreachable. Click opens `pixpapers tui` in a
terminal; the mouse wheel cycles the wallpaper (up = next, down =
previous, with wrap-around).

**Install** (tested on waybar 0.15.0, niri): copy `config.snippet`
into `~/.config/waybar/config` (or `.jsonc`) and add
`"custom/pixpapers"` to `modules-center` or `modules-right`; restart
waybar. On waybar 0.13 or older use the bare key `"pixpapers"` with the
same contents — 0.14 removed the standalone `exec` module type but kept
the keys.

**Format**: `format` uses the named `{text}` placeholder (the form
waybar documents). Do not switch it to a positional form such as
`{ }` — older tutorials use that idiom, but the newer libfmt that
current waybar links against (fmt 12 on the tested 0.15.0) rejects
mixing it with the module's named arguments: the module logs
`invalid format string` to the waybar log and then stays **silently
hidden** (no crash, no error in the output). `{text}` works across
versions, which is why the snippet ships with it.

**Terminal**: `on-click` runs `${TERMINAL:-foot} -e pixpapers tui`.
Set `TERMINAL` in the environment waybar is started from (or edit the
key) to your terminal — `foot`, `kitty` and `ghostty` all accept
`-e`; the fallback is `foot`. Waybar has no "spawn in my terminal"
magic; the terminal is yours to name.

**Refresh**: the label is re-run every `interval` (30 s) and again on
every click/scroll (`exec-on-event` defaults to true). It is not
push-based: a `set`/`cycle` from the CLI or TUI flips the label on the
next tick, i.e. within at most one interval. `pixpapers bar` is an
offline config+manifest read, so it works (and stays truthful) with or
without a daemon.

**Label text**: waybar renders the label as Pango markup. Plain titles
render as-is; a title containing markup-sensitive characters (`&`, `<`,
`>`) would not render. Add `"escape": true` to the module to escape
the text first — the module's `format` itself can still carry markup.

**Auto-hide**: a custom module hides itself on empty output or a
non-zero exit. `pixpapers bar` never does either — always exactly one
line, always exit 0 — so the button never blinks.

**Stop**: remove the module from your config and restart waybar.
