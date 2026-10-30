# PLAN-30-ct — v0.2 upgrades: palettes & dithering, X11, TUI, waybar

Working plan for the next increment of `pixpapers`. Four workstreams, each
shipped as small tested steps in the house style: every step ends green
(`cargo test --workspace`, `clippy -D warnings`, `fmt --check`, offline
`test/smoke.sh`), and any performance or size claim ships with a number +
its source (script/commit).

```
A. Retro palettes & dithering   (pixcore — pure, headless)
B. X11 support                  (daemon — backend seam + x11rb backend)
C. TUI with image preview       (new `pixpapers tui` command)
D. Waybar toolbar module        (wallpaper button → opens TUI in a terminal)
```

Reading order: §1 (invariants) → the workstream you are implementing →
§6 (steps) → §7 (deps) → §8 (risks).

**Out of scope for this plan** (still deferred, deliberately):
live/animated wallpapers, auto-preset by image content, MPRIS-ish hooks,
exact fractional buffers (separate v0.2 viewporter work), multi-backend
simultaneity (a daemon renders on exactly one backend at a time),
**hyprpaper interop** — explicitly dropped: Hyprland is out of scope, the
target remains niri/Wayland. The backend seam (§B.1) is shaped so a
delegating backend could be added later without rework, but none is planned.
A full GUI **window** (native or web) is future work; this plan ships the
TUI + a waybar button that opens it in a terminal.

---

## 1. Invariants that must survive all four workstreams

From the shipped v0.1 architecture (AGENTS.md, docs/PLAN.md §11). A step
that touches one of these adds a retro-table row, it does not silently
change it.

1. **The CLI owns all state** (atomic writes); **the daemon is a pure
   renderer** that re-reads config+manifest on every `Apply`. The TUI,
   waybar module, and any future client are *clients* exactly like the CLI:
   they mutate state through the existing `commands::*` functions (atomic
   manifest/config writes) and then send `Apply` over the same socket. No
   daemon-side state writes, no new "partial update" IPC, no cross-process
   writes.
2. **`view` is always computed from the original dimensions**; the variant
   canvas is only ever nearest-stretched into `view.dest`. New palette/
   dither stages operate on the canvas *before* stretch — they never change
   the placement math.
3. **Determinism**: same input bytes ⇒ same output bytes, for every new
   pipeline stage. Cache keys stay versioned (`PIPELINE_VERSION`); behavior
   changes bump the version and let the existing prune clean up.
4. **Exit codes**: 1 = user error, 2 = internal, **43 = connection to the
   display server lost** (Wayland *or* X server gone). Never
   restart-on-43; the unit's `RestartPreventExitStatus=43` stays correct.
5. **`pixcore` stays wayland-free and fully unit-testable.** Everything
   pure (palettes, dither, preview sizing, label formatting) lives in
   pixcore with proptest/golden coverage; daemon modules only adapt it.
6. **Dependency discipline**: every new runtime dependency ships with a
   numbers-based justification (size, API surface used, alternative
   considered). Dev-only deps just need to be honest tools.
7. **Docs are claims**: any new user-visible behavior updates `CLI.md` +
   `CHANGELOG.md` + (if claimed) `VERIFICATION.md` in the same change.

---

## 2. Workstream A — Retro palettes & dithering (pixcore)

Today: variants get an optional median-cut-to-256 palette (`palette` bool,
default on) with **nearest** mapping and **no dither** (pipeline.rs
documents it: "no dither — blocky aesthetic"). This workstream turns that
into explicit, per-wallpaper knobs with console palettes and classic
dithering. Names stay vibe, not console-accurate — the README says so and
it stays true: we do palette quantization + dithering, not NES OAM/VRAM
quirks.

### A.1 Model (new types in `pixcore`)

```rust
pub enum PaletteMode { Off, Auto, Nes, Gb, Cga, Ega, Vga, Gen16 }
// serde: "off" | "auto" | "nes" | "gb" | "cga" | "ega" | "vga" | "gen16"

pub enum Dither { None, FloydSteinberg, Atkinson, Bayer2, Bayer4, Bayer8 }
// serde: "none" | "fs" | "atkinson" | "bayer2" | "bayer4" | "bayer8"
```

- **Built-in palettes** (static tables, `crates/pixcore/src/palette/`):
  - `nes` 192 fixed palette (the well-known NES master table; per-sprite
    4-color subsets are *not* emulated — documented)
  - `gb` 4 DMG greens
  - `cga` 16 · `ega` 64 (classic 4×4×4) · `vga` 256 (standard VGA palette)
  - `gen16`: **per-image** — median-cut to 16, snapped to the 15-bit
    (RGB555) grid. Captures the Genesis/Mega Drive flavor (16 colors per
    palette, 5-bit depth) without claiming master-palette fidelity;
    documented as an approximation.
- **`Auto`** is exactly today's behavior (median-cut ≤256, nearest).
  **`Off`** is today's `palette = false`.
- **Dither** (all deterministic at fixed input + row-major scan):
  - `fs` Floyd–Steinberg, `atkinson` — error diffusion with u16
    accumulators over R,G,B (no float; no overflow at 256-level input)
  - `bayer2/4/8` — ordered, fixed 0..N matrices, threshold by
    `(x & (n-1)), (y & (n-1))`
  - Dither operates in **canvas space** (block space for presets): a
    1080p@16 variant dithers a 120×68 image — this is where the retro look
    lives, and it stays cheap. `normal` presets dither the full-size canvas
    (photo → console without block pixelation); cost is one-time + cached,
    measured in A.4.
- **Plumbing**:
  - `Settings` (manifest) gains `palette: Option<PaletteMode>`,
    `dither: Option<Dither>` (None = inherit), serde `skip_serializing_if`
    like the other knobs — old manifests parse unchanged.
  - `Config` gains `palette: PaletteMode` (default **Auto** — today's
    default-on is preserved) and `dither: Dither` (default **None**).
    The `Config.palette` field gets a custom `Deserialize` that also
    accepts the legacy TOML bool (`true` → `Auto`, `false` → `Off`) so
    existing config files keep working; new saves serialize the string.
  - `ResolvedSettings` resolves entry → config for both knobs, and the
    `unpixelated()` (R6) path is untouched (disable still = originals).

### A.2 Pipeline & cache

- New stage in `variant::generate_from_image`, between downscale and
  encode: `pipeline::quantize(canvas, mode, dither)` (pure, tested):
  - `Off` → identity
  - `Auto` → existing `median_cut(256)` + `apply_palette` (byte-identical
    to v0.1 when `dither = none`)
  - named → `apply_palette` on the fixed table; `Gen16` → median_cut(16)
    snapped to RGB555
  - then dither, if not `None` (FS/Atkinson/Bayer on the nearest-mapped
    canvas)
- **Encode**: named/auto palettes keep the existing palette-PNG path
  (index-0-transparent convention); a named table encodes with *exactly*
  that table (fewer entries than 255 ⇒ ≤ current sizes); `Off` stays RGBA.
- **Cache key**: filename becomes
  `v{VER}-{pixel}-{placement}[-g{g}]-{W}x{H}-{Q}[-{D}].png` where
  `Q ∈ {off,auto,nes,gb,cga,ega,vga,gen16}` and `D` is the dither name,
  omitted when `none`. **Bump `PIPELINE_VERSION` 1 → 2** — old variants are
  invalidated automatically and `prune_variants` (already wired into
  set/remove) reclaims them on next use. No migration code.
- `export` (offline) passes the same knobs, so terminal previews, waybar,
  and any PNG consumer get the exact look.

### A.3 CLI

```
pixpapers palette <id> <off|auto|nes|gb|cga|ega|vga|gen16>
pixpapers dither  <id> <none|fs|atkinson|bayer2|bayer4|bayer8>
pixpapers default palette <mode>          # defaults for new/unset entries
pixpapers default dither  <algo>
# export gains: --palette <mode>  --dither <algo>
```

Same behavior contract as the existing knobs: editing a knob on the
*current* entry re-applies immediately (Apply IPC); on a non-current entry
it records ("will apply on set"). `list`/`status` show per-entry
palette+dither.

### A.4 Tests & evidence (step acceptance)

- Unit (pixcore): table sizes + spot-checked RGB rows (GB greens, NES row 0,
  VGA `#FF0000`); `quantize` output colors ⊆ chosen palette (proptest over
  all modes); mean-color drift bound for FS/Atkinson on a ramp; byte
  determinism (two runs → equal bytes); `Gen16` output ⊆ RGB555 grid;
  legacy TOML `palette = true/false` deserializes to `Auto`/`Off`;
  distinct `(Q, D)` pairs ⇒ distinct cache paths (proptest); `Auto`+`None`
  is **byte-identical to the v0.1 output** on the shared test sources
  (guards the "no surprise change" claim).
- Goldens: one pipeline golden per (palette × dither) cell on
  `testsrc::photo128x80` (12 cells — small, checked in).
- Bench (`--release`, numbers → PERF.md with the script + commit): FS and
  bayer8 on 120×68 and 240×135 canvases; `normal`+`vga`+`fs` on 1920×1080
  (one-time); variant byte sizes vs v0.1 auto-palette on the 1080p/4K
  fixtures (1080p @16 ≈ 6 KB today — table updated with the new default
  path included). If a named palette blows any size budget (e.g. NES on a
  photo), the README size table says so with the number.

---

## 3. Workstream B — X11 support

Goal: the same daemon, same CLI, same look — on X11. One daemon still
renders on exactly one backend; auto-detection picks it.

### B.1 Backend seam (refactor first, zero behavior change)

- New module tree in `crates/pixpapers`:
  ```
  daemon/
  ├── mod.rs            # backend-agnostic supervisor: poll loop, IPC, signals
  └── backend/
      ├── mod.rs        # `trait Backend`
      └── layer.rs      # today's daemon/wayland.rs, moved + reshaped
  ```
- Trait (deliberately small — the supervisor already knows the loop shape):
  ```rust
  pub trait Backend {
      fn name(&self) -> &'static str;
      fn poll_fd(&self) -> Option<RawFd>;        // None ⇒ supervisor polls only IPC
      fn pump(&mut self);                        // drain connection events
      fn render_all(&mut self, paths: &Paths);   // (re)render current wallpaper
      fn snapshot(&self) -> Vec<OutputStatus>;   // feeds StatusReport
      fn connection_lost(&self) -> bool;         // true ⇒ exit 43
      fn close(&mut self);                       // teardown before socket/pid cleanup
  }
  ```
  The supervisor in `daemon/mod.rs` keeps its exact contract: `poll` over
  `[backend.poll_fd(), sock_fd]` + SIGTERM/INT, `Apply` → `render_all`,
  teardown → `close`, exit 43 on `connection_lost`. The `dispatch_pending`
  trap, the raw sigaction, stale-socket handling, and exit-43 unit-file
  note all stay as-is (they are loop-level, not backend-level).
- **Gate for this step**: `WAYLAND_DISPLAY=wayland-1 test/verify-live.sh`
  and `test/bench-daemon.sh` pass with no behavior diff (switch latencies
  within run-to-run noise; V-matrix re-run recorded in VERIFICATION.md).
  The move must be boring: same numbers, same logs, one fewer file in the
  old spot.

### B.2 The `x11` backend

- **Crate**: `x11rb` (pure-Rust XCB, no X11 headers, pollable
  `AsRawFd`). Justification row for AGENTS.md: X11 *is* the requirement;
  x11rb is the only maintained pure-Rust option; API surface used is
  ~20 functions (connect, query screens, put/copy image, event fd).
- **Per screen** (X11 has no "outputs" — screens are the analog, and one
  daemon renders the same wallpaper on all of them, mirroring the
  Wayland multi-output behavior):
  - size from `GetScreen`/`QueryScreen` (root `width × height`);
  - buffer = full-root XRGB8888 (native byte order per
    `QueryExtension`-reported visual; depth-24 assumption tested, not
    hoped for — a non-24-depth root is a *user error* exit 1 with a clear
    message, matching v0.1's "one look, done fast" posture);
  - render = the **same `pixcore::render::composite_into`** the layer
    backend uses (invariant #2) into a client-side buffer, then
    `PutImage`/`CopyPlane` to the root window; no GPU, no Xv extension —
    if Xv happens to be present it may be used for the blit later, v0.2
    does not depend on it.
- **Events**: poll the x11rb fd in the supervisor loop;
  - root `Expose` → re-render;
  - `ClientMessage` RFPWM/RRC (resolution-change hints) → re-render at new
    size (X11 "resize" is compositor-driven; we react, we never
    initiate);
  - `ResizeNotify` on our view (we have no window; root is the window)
    → covered by Expose in practice;
  - connection error / `ConnectionClosed` → `connection_lost()` ⇒ exit 43,
    identical contract to the Wayland path.
- **Scale**: X11 has no fractional scale; logical = physical. If
  `Xft.dpi` is set we *may* treat it as scale for buffer sizing, but the
  default and the tested path is 1:1 — documented in COMPOSITORS.md
  (X11 section, replacing the "not supported" note).
- **No RFPWM emission** (we don't change sizes); we only react to others.

### B.3 Selection & config

- `Config.daemon.backend: "auto" | "layer" | "x11"` (default `auto`),
  plus `pixpapers daemon --backend <b>` override (test/dev).
- **auto order**: `WAYLAND_DISPLAY` set ⇒ `layer`; else `DISPLAY` set ⇒
  `x11`; else exit 1 with the existing "no wayland session" wording
  extended to name both. The probe happens at `daemon` start; a wrong
  guess is a loud user error, not a silent fallback chain.
- `status` reports the active backend name + per-backend detail in
  `StatusReport` (new optional field `backend: String` — additive, old
  clients ignore it).

### B.4 Tests & evidence

- **Headless (CI-safe)**: `test/verify-x11.sh` under
  `xvfb-run -a` (hermetic `PIXPAPERS_HOME`):
  - daemon starts with `DISPLAY` only (no `WAYLAND_DISPLAY`) ⇒ backend
    `x11`;
  - `set <id>` changes the root pixmap (compare `xwd`/`import` capture vs
    `pixpapers export` of the same size — the export contract is the
    oracle, exactly like `export` is today's daemon oracle);
  - RFPWM broadcast triggers a re-render at the new size;
  - `stop` removes socket/pid, leaves the X server running;
  - killing the X server ⇒ daemon exits 43.
  CI: runs under `xvfb-run` if present, else skipped *by name* in
  VERIFICATION.md (honest skip, same policy as the live Wayland layer).
- **Live**: maintainer's X11 session (if/when available) gets a manual
  V-row; niri stays the primary measured target.
- **Bench**: `PutImage` 1080p + 4K cost vs the Wayland stretch path
  (expect same order — same composite, different blit; number goes to
  PERF.md either way).
- **Regression gate**: the B.1 refactor step re-runs the full existing
  offline + live Wayland suites; nothing in §A changes daemon code, and
  nothing in §C/D does either.

---

## 4. Workstream C — TUI (`pixpapers tui`)

An interactive terminal front-end to the *exact same* state the CLI owns:
browse the library, **add a wallpaper by file path**, tune the look
per-wallpaper, **set the active wallpaper**, **remove** entries — with a
live preview that renders the wallpaper image **when the terminal supports
terminal image protocols**, and a text fallback when it does not.

### C.1 Command & terminal model

- `pixpapers tui` — new clap subcommand; runs in the current terminal,
  no daemon required (it previews offline from the library and only talks
  IPC when applying).
- Dependencies (discipline rows, §7): **`ratatui`** (layout/widgets) +
  **`crossterm`** (raw mode, events, alternate screen). Both CPU-only,
  no GPU, no event-loop crate of their own; the TUI owns its own single-
  threaded loop (consistent with the daemon's no-tokio posture).
- Terminal hygiene: alternate screen + raw mode on, restored on exit
  (Drop-based, panic-safe); `q`/Ctrl-C/`Esc` quit; `Ctrl-L` refresh.
  Resize → re-render preview + relayout (cell size from
  `terminal.size()`).

### C.2 Layout

```
┌ library ──────────────────────┐ ┌ preview ────────────────────────┐
│▸ 4a1c9f nebula     16 fit     │ │                                │
│  7c2e10 dunes      32 cover   │ │      [image, if supported]     │
│  9f00b1 grid       8  cover   │ │                                │
│  …                            │ │  1920×1080 · pixel 16 · fit    │
│                               │ │  palette auto · dither none    │
│  (a)dd  (s)et  (c)ycle  (x)   │ │  bg #000000 · gutter 24px      │
└───────────────────────────────┘ └──────────────────────────────────┘
  key hint bar: a add · s set · c cycle · p pixel · l place · b bg ·
  g gutter · t palette · d dither · r refresh · x export · rm remove · q
```

- **Library list**: id, title, per-entry pixel/placement, `*`/`▸` on the
  current entry (from `library.current`), disabled entries dimmed when
  `enabled = false`. Selection = arrow keys / `j` `k`.
- **Preview pane** (right, ~60% width): the actual rendered wallpaper at
  the current entry's resolved settings (entry → config → `enabled`
  switch, i.e. the exact `ResolvedSettings` the daemon uses), computed
  *offline* via the pixcore pipeline at the preview canvas size. Caption
  line: output size, pixel, placement, palette, dither, bg, gutter —
  always visible, image or not.
- **Status bar**: daemon alive? (socket probe), current backend-agnostic
  note, last action result (e.g. `added 4a1c9f · applied` /
  `removed 7c2e10` / error text) — one line, most recent wins.

### C.3 Image preview — render only when supported

**Detection** (startup + env override; documented in CLI.md):
| Terminal | Signal | Protocol |
|---|---|---|
| kitty | `KITTY_WINDOW_ID` / `TERM_PROGRAM = kitty` | OSC 6337 inline PNG |
| iTerm2 | `ITERM_SESSION_ID` | OSC 1337 `File=inline=1` |
| sixel-capable | `SIXEL` env or `$TERM` contains `sixel` | `ESC q … ESC \\` |
| none | anything else | **text fallback** |

- Override: `PIX_PAPERS_IMAGE = none|kitty|iterm2|sixel` (also lets the
  same terminal be forced into fallback for tests). Detection result is
  shown once in the status bar (`img:kitty` / `img:none`), never guessed
  silently.
- **Fallback (no protocol)**: the preview pane shows the caption block
  only, plus one honest line: `image preview unsupported here —
  pixpapers export <id> 1280x720 -o /tmp/prev.png`. No fake art.
- **Render path** (shared with export, so preview == screen):
  - preview canvas size from the pane's cells: `pw × ph` pixels using the
    terminal's cell aspect when known, else `pw = 2·cells_w`,
    `ph = cells_h` (16:9-ish assumption documented);
  - run `variant::generate_from_image` at `(pw, ph)` with the entry's
    `ResolvedSettings` (this is the *same* function export uses — the
    preview is literally a small export, not a reimplementation);
  - downscale to ≤ the pane at cell granularity (kitty: emit with
    `resize=` to fit the pane exactly; sixel/iTerm2: pre-resize to pane
    px to bound size);
  - throttle: re-render on selection change, on knob change, on resize —
    not on a timer; generation hits the variant cache when possible (a
    preview size that matches a real output size is a cache hit for free).
- **Size guard**: refuse (fallback line instead) if the generated preview
  PNG would exceed 256 KB — keeps a 25-column SSH terminal from
  exploding; number chosen from the bench, stated in CLI.md.

### C.4 Actions (all through `commands::*`, then IPC)

| Key | Action | State path |
|---|---|---|
| `a` | **Add by path** — input line `add: `, type/paste a file path (tilde-expanded), Enter | `commands::add` (same as CLI: dedupe hash, optimize, manifest entry) → if it becomes/keeps current per user, `Apply` |
| `s` | **Set active** — make selected entry current | `commands::set_current` + `Apply` (auto-spawn daemon, exactly like CLI `set`) |
| `c` | Cycle (next) | `commands::cycle_current` + `Apply` |
| `p` | Cycle pixel preset `8→16→32→64→128→normal` | `commands::edit_entry` (same as `pixpapers pixel`) + Apply if current |
| `l` | Cycle placement | `commands::edit_entry` + Apply if current |
| `b` | Prompt bg color `#rrggbb` (input line) | `commands::edit_entry` + Apply if current |
| `g` | Prompt gutter px | `commands::edit_entry` + Apply if current |
| `t` | Cycle palette `off→auto→nes→gb→cga→ega→vga→gen16` | §A knobs + Apply if current |
| `d` | Cycle dither `none→fs→atkinson→bayer2→4→8` | §A knobs + Apply if current |
| `rm` | **Remove** selected (confirm line: `remove <id>? y/N`) | `commands::remove` (deletes entry dir + prunes) + `Apply` if it was current |
| `e` | Enable/disable toggle | `commands::set_enabled` + `Apply` |
| `x` | Export selected to `$XDG_DOWNLOAD`-style path (printed) | `commands::export` |
| `r` | Refresh (re-read manifest/config) | no IPC |

- Every mutation: **the same function the CLI subcommand calls**
  (invariant #1) — so a knob edited in the TUI is byte-identical to the
  same edit via CLI, and a half-written manifest is impossible (atomic
  writes already guarantee this).
- **Add-by-path details**: `a` shows a normal input line (crossterm raw
  key events assembled into a line buffer — backspace, left/right,
  delete, paste as one chunk where the terminal allows). Validation before
  calling `commands::add`: exists, regular file, extension in the image
  set (or: trust `image` crate decode and surface its error in the status
  bar). Result line: `added <id> <title> · <WxH> · <bytes>` or the error.
  Directory pasted ⇒ status-bar error `not a file — pick a single image`
  (bulk-add is a CLI job, `pixpapers add dir/*.png`).
- **Daemon coupling**: if the daemon is dead and the user does `s`, the
  TUI auto-spawns it (same `daemon::spawn::ensure_running` path as the
  CLI) — no special-casing.

### C.5 Tests & evidence

- **pixcore**: preview-size computation (cell grid → canvas dims) pure +
  unit-tested, incl. degenerate panes (3×3 cells) and the 256 KB guard
  boundary.
- **pixpapers unit**: the action table is a `match` over key events →
  (command, target); test each key against a temp `PIXPAPERS_HOME`
  fixture library (add-by-path creates a real entry with the fixture
  image; remove deletes dir + manifest row; `s` updates `current` and
  writes the socket *request* — with the socket pointed at a throwaway
  listener, so no real daemon in unit tests).
- **Snapshot**: render the ratatui UI to a `ratatui::backend::TestBackend`
  for a fixed 100×30 terminal + fixture library; snapbox golden for
  (a) initial view, (b) after `a` + path entry, (c) no-image fallback
  pane. This is the CI-safe proof that layout and labels hold up.
- **Live (manual, VERIFICATION row)**: real session — kitty (image
  shows, switch < 200 ms perceived), a sixel terminal (e.g.
  `mlterm`/`aom` if available; else *named* skip), plain xterm
  (fallback line shows). Detection table spot-checked: kitty env,
  ITERM env, none.
- No bench claim expected (preview reuses the measured pipeline); if the
  preview add a new hot path > 50 ms p50 in the live test, it gets a
  number + fix-or-document pass.

---

## 5. Workstream D — Waybar toolbar module

A **waybar module** for the desktop toolbar: one button/label showing the
current wallpaper, clicking it opens `pixpapers tui` in the user's
terminal. This is the whole of v0.2's "GUI": no new window, no new
server — waybar already owns the toolbar and the terminal already owns
the window.

**A real GUI window (native or web) is explicitly future work** (§9):
this design keeps that door open by putting every state mutation behind
`commands::*` (already true) and keeping `status --json` the single
machine-readable face of state.

### D.1 What ships

- **`contrib/waybar/pixpapers.sh`** — the module script (POSIX sh):
  - prints the label: `▞ <title> · <pixel>/<placement-short>` e.g.
    `▞ nebula · 16/fit`; when no current entry: `▞ pixpapers — set one`;
    when daemon dead: `▞ pixpapers · daemon down`.
  - sourced data = `pixpapers status --json` (already exists; one parse
    via the existing JSON; no new IPC surface). A small new flag is
    acceptable if parsing proves clumsy: `pixpapers bar` printing exactly
    the one line above (still offline, no daemon needed for the label
    part). **Prefer `bar`** — keeps the module script ~10 lines.
  - exit/refresh policy: waybar `exec` interval **30 s** (label changes
    only on user action; 30 s bounds staleness without polling cost —
    the label read is a config+manifest read, ~0 ms, but we don't pay for
    a live daemon probe more often than needed).
- **`contrib/waybar/config.snippet`** — drop-in waybar module:
  ```jsonc
  "pixpapers": {
    "format": "{ }",
    "exec": "pixpapers bar",
    "interval": 30,
    "on-click": "${TERMINAL:-foot} -e pixpapers tui",
    "scroll-step": 1            // optional: wheel = cycle wallpaper
  },
  ```
  - `on-click` opens the TUI in the user's terminal. Snippet documents:
    set `TERMINAL` (or edit `on-click`) to your terminal; on niri,
    `foot`/`kitty`/whatever is running; waybar itself has no "spawn in
    my terminal" magic — the terminal is user-configured, we don't guess
    beyond `$TERMINAL` with a documented fallback list.
  - **Scroll = cycle** (nice-to-have, same workstream): waybar exec
    modules can emit `on-scroll-up/down`; the script maps
    `waybar-click`-style env (`WAYBAR_CLICK`? — use the documented
    `WAYBAR_*` variables the module receives: `WAYBAR_EXEC` … actually:
    waybar passes the button/action via `$WAYBAR_…`? **Verify at
    implementation time against the installed waybar's module docs**; the
    fallback is a separate `on-scroll-up: pixpapers cycle 1` binding —
    waybar supports `on-scroll-up`/`on-scroll-down` directly on exec
    modules, so no script acrobatics needed: `on-scroll-up: pixpapers
    cycle 1`.)
- **`contrib/waybar/README.md`** — 20 lines: install (copy snippet into
  `~/.config/waybar/config`'s `modules-center`/`modules-right`),
  terminal env, how the label updates, how to stop (remove module), and
  the honest note: label updates on 30 s interval or on the next
  `pixpapers` CLI action + waybar's next tick (not push-based).

### D.2 Why this shape (and not the alternatives)

- waybar modules are scripts/config — zero new Rust deps, zero daemon
  changes, works on any wlm compositors (niri included; waybar is
  layer-shell Top layer, we're Background — no arbitration fight, unlike
  two wallpaper daemons).
- A native layer-shell "toolbar button" of our own (we *could* — we own
  raw wayland-client) was considered and rejected for v0.2: it would mean
  font rasterization for a label we can get for free from waybar's
  markup, plus a second input-handling path to keep alive. Revisit with
  the GUI-window work if/when a pixpapers-owned widget earns its keep.
- The TUI is the window for now: `on-click` → terminal → `pixpapers
  tui` gets full interactivity (add-by-path, knobs, preview) with none of
  the GUI framework cost. The §9 window design must stay compatible with
  this: button opens TUI today, opens window later, same snippet slot.

### D.3 Tests & evidence

- `pixpapers bar` (if it lands): snapbox golden — with current entry,
  without, with daemon down (hermetic `PIXPAPERS_HOME`, dead socket);
  output is exactly one line, no trailing junk (waybar chokes on
  multi-line exec output).
- `contrib/waybar/pixpapers.sh` runs in the offline smoke script
  (`env -u WAYLAND_DISPLAY test/smoke.sh` gets a `pixpapers bar`
  call — label correctness with no daemon is the CI-checkable part).
- **Live (manual, VERIFICATION row)**: niri + waybar (the maintainer's
  actual toolbar): button renders, click opens TUI in terminal, scroll
  cycles, `set` from TUI flips the label within one interval (measured:
  "next tick", number = interval). Screenshot of the toolbar goes in
  docs/screens/ like the other evidence.
- Waybar version note: record the tested waybar version (label/scroll
  keys differ across versions — the snippet targets the maintainer's
  version first, compatibility notes added as issues arrive, same
  posture as COMPOSITORS.md).

---

## 6. Steps & order

Sequential, each ending in a tested increment (house style: commit after
each step; small; evidence in the message when a claim rides on it).
A and B are independent of each other; C depends on A (its knobs) — order
below is the recommended path.

| # | Step | Deliverable | Acceptance (all must hold) |
|---|------|-------------|----------------------------|
| S1 | A.1+A.2 model & pipeline | `PaletteMode`/`Dither`, tables, `quantize`, cache key `-{Q}[-{D}]`, `PIPELINE_VERSION=2` | All A.4 unit/golden tests green; `Auto`+`none` byte-identical to v0.1 on testsrc; old variants pruned on next set; offline smoke green |
| S2 | A.3 CLI knobs | `palette`/`dither`/`default …`/export flags | snapbox CLI round-trips (temp homes); current-entry edit re-applies (harness socket); `export --palette vga --dither fs` produces palette PNG with ≤256 exact entries |
| S3 | A.4 bench + docs | PERF.md numbers, README size table update, CLI.md | Every number has script+commit; size/latency claims match measurement (walk-back rule applies) |
| S4 | B.1 backend seam | `daemon/backend/{mod,layer}.rs`, supervisor in `mod.rs` | `verify-live.sh` + `bench-daemon.sh` pass unchanged on niri; clippy/fmt green; diff review shows move+reshape only |
| S5 | B.2+B.3 x11 backend | `x11` backend, auto-detect, `--backend`, status field | `test/verify-x11.sh` under xvfb: start/set/RFPWM/stop/exit-43 all pass; Wayland suite re-green; auto-detect table unit-tested (env matrix) |
| S6 | C.1–C.3 TUI core | `pixpapers tui`: layout, list, preview (image + fallback), status bar | TestBackend snapbox goldens (initial/add/fallback); preview-size unit tests; live: kitty image + xterm fallback rows in VERIFICATION |
| S7 | C.4 actions | full key table incl. add-by-path, set, remove, palette/dither cycles | unit tests per action against fixture library; add-by-path creates entry identical to CLI `add` of same file (hash+bytes); remove leaves no dir/manifest residue |
| S8 | D waybar module | `pixpapers bar` + `contrib/waybar/` | `bar` goldens (3 states) in offline smoke; live niri+waybar row: click→TUI, scroll→cycle, label flip ≤ interval |
| S9 | Polish & docs | README "Deliberately not in v0.1" pruned (TUI/X11/palettes done → listed under shipped; rest stays), COMPOSITORS.md X11 section, TROUBLESHOOTING (x11 exit-43, terminal no-image line), CHANGELOG `[Unreleased]` | docs are claims: every statement traceable to a script/output; no dead links |

Cross-cutting gates after *every* step: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --check`, `env -u WAYLAND_DISPLAY test/smoke.sh`.

---

## 7. New dependencies (justified)

| Crate | Used by | Why (numbers/scope) | Alternatives considered |
|---|---|---|---|
| `ratatui` | C | the maintained tui-rs successor; layout+widgets only; CPU | `tui` (predecessor, slower release cadence), hand-rolled (weeks, zero win) |
| `crossterm` | C | raw mode/events/alt-screen; pairs with ratatui; no event-loop of its own | `termion` (older, less maintained) |
| `x11rb` | B | pure-Rust XCB; pollable fd fits the existing `poll(2)` loop; ~20 fns used | `x11-clipboard`-style direct XCB (C header build), `xcb` raw (unsafe, more work, same API) |
| *(none)* | A, D | A is ~pure math on existing buffers; D is sh + waybar config | — |

- `ratatui`/`crossterm`/`x11rb` are **optional features** of `pixpapers`
  (`tui`, `x11` — both on by default, off for a minimal build): keeps the
  default `cargo install` honest while allowing a slim binary; the
  `deny.toml`/lockfile impact measured at S5/S6 (binary size delta in the
  step message).
- No new deps in pixcore for A (median_cut already ships; dither is
  hand-rolled — a dep like `quantities`/`dithering` crates would import
  float paths we don't want; 80–120 lines of integer math is testable and
  stays inside the determinism contract).

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| `PIPELINE_VERSION` bump orphans old variants on disk | existing `prune_variants` already reclaims on set/remove; S1 includes a one-off disk-delta note in the step message |
| Dithering on `normal` (full-res canvas) is slow on 4K | one-time + cached; S3 measures; if > 1 s at 4K, document with the number and keep (it's a user-requested look) — do not silently skip |
| X11 depth/byte-order variety (24 vs 32, BGR) | depth-24 assumption tested at connect (loud exit 1 otherwise); S5 xvfb covers the common case; field reports handled like the compositor matrix |
| `PutImage` blit cost on big screens | same composite as Wayland path; bench in S5; if it's an outlier, note in PERF.md, optimize later with evidence |
| Terminal image detection false-positives (e.g. kitty-inside-tty-forwarding) | env-override `PIX_PAPERS_IMAGE`, detection shown in status bar, fallback is always one env away; documented |
| ratatui TestBackend goldens rot on terminal resize math | goldens pinned to a fixed 100×30 backend; real-terminal rows are manual VERIFICATION entries, not CI |
| waybar version drift (module keys, scroll env) | snippet targets maintainer's waybar, version recorded in VERIFICATION; compat notes accumulate like COMPOSITORS.md |
| TUI editing a file while daemon mid-apply | already safe: manifest writes are atomic, daemon re-reads per Apply (invariant #1); S7 asserts the sequence |
| Scope creep back in (live wallpapers, window, hyprpaper) | §9 is the only place those live; a step that adds one without a plan change is a plan violation |

---

## 9. Future work (explicitly not in this plan)

- **GUI window** (native egui/`eframe` or a local web view): the waybar
  button's `on-click` flips from terminal to the window; design must keep
  `commands::*` as the only state path and `status --json` the only
  read face (both already true).
- Live/animated wallpapers; auto-preset by image content; MPRIS-ish hooks
  (README "Deliberately not in v0.1" — untouched by this plan).
- Custom palette *files* (`palette = "file:my.pal"`, ≤256 `#rrggbb` lines):
  cheap to add once A ships; deferred to keep A's tables reviewable.
- Exact fractional buffers (viewporter/fractional-scale) — the existing
  separate v0.2 track.
- Multi-output-per-backend hardening beyond what B ships (X11 multi-
  screen is in; exotic setups wait for field reports).

---

## 10. Confirm before S4/S5/S6 (cheap now, expensive later)

1. **`gen16` semantics** — per-image 16-color snapped to 15-bit
   (this plan) vs. a fixed 16-color table. Per-image is the more
   "Genesis" behavior; fixed is simpler to review.
2. **Dither default stays `none`** — yes/no (this plan: yes, so the
   default look is byte-identical to v0.1).
3. **TUI add-by-path** accepts only single files (this plan) — or should
   directory paste expand to all supported images?
4. **Waybar scroll** — cycle wallpaper (this plan) vs. nothing.
5. **`daemon.backend` config key name** — `backend` under `daemon`
   (this plan) vs. top-level.
6. TUI dependency feature-flags (`--no-default-features` slim build) —
   ship with flags (this plan) vs. unconditional deps.
