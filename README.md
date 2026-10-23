# pixelated-papers

Forced-pixelation wallpapers for [niri](https://github.com/niri-wm/niri) and
other Wayland compositors. Written in Rust.

**Status: planning complete, implementation not started.**

- The look: every wallpaper you set is pixelated by default — block sizes
  `8 / 16 / 32 / 64 / 128 / normal` (the "bit" names are pure vibe: 8-bit
  Nintendo, 16-bit Sega, 64-bit… — nothing console-accurate). Default: `16`.
- Placement: `fit` (black/any-color borders — default), `cover-gutter`,
  `cover-crop`, `stretch`. Border/gutter color is a per-wallpaper setting.
- Pixelation is *forced* unless you change the per-wallpaper pixel size or
  disable the app.
- Originals are stored; pixelated variants are generated at the absolute
  minimum size on change (a few KB) and cached — faster switches, tiny RAM.
- Rendering: a tiny daemon puts a wlr-layer-shell **background** surface on
  every output (this is how niri takes wallpapers).

See **[docs/PLAN.md](docs/PLAN.md)** for the full design and
**[docs/plan/](docs/plan/)** for the sequential implementation steps
(step-1 → step-9); every step ends in a tested, usable increment.

## Planned quickstart

```sh
cargo install --path .
pixpapers add ~/Pictures/nebula.png
pixpapers set <id>          # auto-starts the daemon, pixelates, stretches
```
