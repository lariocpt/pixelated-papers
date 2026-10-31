# pixelated-papers — implementation plan

A Rust wallpaper tool for Wayland compositors (niri first, other
wlr-layer-shell compositors second) that **forces a pixelated look** on every
wallpaper it sets, places it with one of four stretch/fit modes, keeps the
**original** in a local library, and applies pixelation **lazily, at
wallpaper-change time**, so switching and steady-state cost stay small.

This document is the map. Each numbered step has its own detailed spec in
[`plan/step-N.md`](plan/). Steps are sequential: every step ends in a working,
tested increment.

---

## 1. Product requirements (as requested)

| # | Requirement | Consequence for the design |
|---|-------------|----------------------------|
| R1 | The "bit" look is selectable: **8, 16, 32, 64, 128, normal**. The names are pure vibe (8-bit NES-ish, 16-bit Sega-ish, …) — nothing console-accurate. | The single real parameter is the **pixel block size B** (physical px): `8→8px … 128→128px, normal→1px`. `16` = a 64×64 screen area contains 16 blocks (4×4 of 16px) — the "game" look. |
| R2 | Default look = **16**. | Config `default_pixel = "16"`; any wallpaper without an override renders at 16. |
| R3 | Placement modes (all four are options): **fit** (whole image visible, leftover area = wallpaper `bg_color`, default black — *current default*), **cover-gutter** (cover an inset content box, gutter ring filled with `bg_color`), **cover-crop** (classic centered cover crop), **stretch** (fill exactly, no fixed aspect ratio, distortion allowed). | A `Placement` enum with a unified window→dest mapping (§2). `bg_color` and `gutter_px` are per-wallpaper settings. |
| R4 | **Pixelation is forced** on everything this tool sets. | `set` never ships the raw original. The only exits are R5 and R6. |
| R5 | Controls live **per wallpaper**: pixel size, placement, bg color, gutter width — each wallpaper entry carries its own settings (pixel `normal` = no pixelation for that wallpaper). | Library entries store `pixel`, `placement`, `bg_color`, `gutter_px`; CLI subcommand per knob, applied immediately. |
| R6 | If the app is **disabled**, pixelation is off. | `pixpapers disable` → config `enabled=false`; renders use the original, placement rules unchanged. `enable` restores forcing. `stop` kills the daemon (compositor's own backdrop shows). |
| R7 | The machine **stores the original** and applies pixelation **on image/solution change**. | Library keeps an optimized original per wallpaper. No upfront batch: variants are produced when a wallpaper is set, one of its settings changes, or an output geometry changes; results are cached. |
| R8 | The solution must **improve performance, even negligibly**. | Variants are *absolute minimum-size* palette PNGs (typically 5–50 KB vs MB originals). Wins: switch-time CPU (decode KB not MB), daemon RAM (tiny variants resident), disk. Steady-state GPU blit ~unchanged — honest accounting in §7. |
| R9 | Works in niri, ideally other Linux UIs. | Rendering = **wlr-layer-shell background surface** (niri, sway, hyprland, river, cosmic, wlroots generally). niri first-class (backdrop/overview, multi-output). X11 out of scope for v1. |
| R10 | Implemented in **Rust**. | Workspace: `pixcore` (pure logic) + `pixpapers` (CLI + daemon). |

### Non-goals (v1)

- No animated/live wallpapers. No GUI/TUI (CLI + config; a TUI is v1.1 material).
- No X11 backend (wayland-only).
- No color-extraction/theme sync.
- No "console-accurate" palettes or dithering — the labels are vibe; we do
  block-size pixelation, palette-reduced only to shrink variant files.

---

## 2. How the look works (concrete model)

```
original (stored, optimized at add-time, e.g. 3840×2160)
        │
        │  1. placement math → content box C (Wc×Hc) + dest rect (dx,dy,Wc,Hc)
        │     + optional source window (crop) — §2.2
        │  2. downscale: iterative box/area to  ⌈Wc/B⌉ × ⌈Hc/B⌉
        │     (optional median-cut palette ≤256 for tiny PNGs)
        ▼
variant (absolute minimum canvas for this output+mode+preset; a few KB)
        │
        │  3. paint dest canvas with bg_color, then nearest-neighbor stretch
        │     the variant into its dest rect
        ▼
layer-shell background buffer (W×H physical px) → niri renders under everything
```

### 2.1 The pixel look (R1)

- **Variant canvas is per-output, per-mode**: for content box `Wc×Hc` and
  block `B` the variant is exactly `⌈Wc/B⌉ × ⌈Hc/B⌉` — the *absolute minimum*
  image that still covers the content box at block size B. 1080p @ 16 ⇒
  120×67 ⇒ a few KB.
- **Downscale filter**: iterative halving box-average — cheap, clean block
  averages, no ringing (ringing would read as *bad* pixelation).
- **Palette reduction** (optional, default **on** for variants): median-cut to
  ≤256 colors before PNG encode. Tiny files + a subtly retro palette, for free.
  Originals are never palette-reduced.
- **Stretch = nearest**: the only scaling at display time; it keeps blocks
  crisp. Non-integer scales are fine (a block can be 16×17 px — still reads as
  a grid).
- **`normal`**: skip pixelation entirely; nearest-scale the optimized original
  (no smoothing — one aesthetic, one code path).
- **Visibility floor** (optional, default on, conservative): if a content box
  is small enough that the *effective* on-screen block would drop below
  `min_visible_px` (default 2), bump the block to the smallest preset ≥ floor.
  Mechanical form of "minimum image size without noticing it's just blur".

### 2.2 Placement modes (R3)

All four modes reduce to one mapping the renderer consumes:

```
View { window: (sx, sy, sw, sh)   // sub-rect of the source (whole by default)
     , dest:  (dx, dy, Wc, Hc) }  // rect of the output (content box)
// remainder of the output = bg_color
```

| Mode | Scale | Source window | Dest rect | Background fill |
|------|-------|---------------|-----------|-----------------|
| `fit` (default) | uniform `s = min(W/Ws, H/Hs)` | whole source | centered, `s·(Ws×Hs)` | everything outside dest = `bg_color` (black default) |
| `cover-gutter` | uniform, cover of **inset box** `W-2g × H-2g` | centered crop of aspect `(W-2g):(H-2g)` | centered inset box (`gutter_px = g`) | gutter ring = `bg_color` |
| `cover-crop` | uniform `s = max(W/Ws, H/Hs)` | centered crop, aspect `W:H` | full output | none |
| `stretch` | independent `sx=W/Ws, sy=H/Hs` | whole source | full output | none |

Notes:

- **Pixel grid is defined on the content box**, not the full output: the
  variant is sized `⌈Wc/B⌉ × ⌈Hc/B⌉`, so an on-screen block is exactly B px in
  *every* mode (black borders never "absorb" blocks).
- `bg_color`: `#rgb`/`#rrggbb`, per wallpaper, default `#000000`.
- **Open question** (flagged, cheap to change): `cover-gutter` is implemented
  as a *mat margin* (uniform `gutter_px` ring in `bg_color`). If the intent
  was instead a *blurred bleed* of the image outside the sharp cover image,
  that is a contained change in `pixcore::place` + one extra pipeline step
  (see plan/step-6.md §3).
- Rounding: dest rect uses integer arithmetic (floor for sizes, round for
  offsets); ≤1 px slivers are absorbed into bg_color. Documented in step-2.

### 2.3 Where the perf win is (R8, honest accounting)

| Path | Without tool (raw full-res bg) | With tool | Δ |
|------|--------------------------------|-----------|---|
| Switch wallpaper | decode MB PNG/JPEG + compositor scale | decode KB palette PNG + nearest stretch | faster, less transient RAM |
| Daemon RAM | n/a (no daemon) / full-res image | variant resident (120×67×4ch ≈ 30 KB) | ~100× less per output |
| Disk | originals only | originals + KB variants (~1–2% each) | negligible |
| Steady-state GPU | 1 texture upload + blit | same (buffer is output-sized) | ~wash — stated honestly |

---

## 3. System layout

```
~/.local/share/pixpapers/          (XDG_DATA_HOME/pixpapers)
├── library/
│   └── <id>/
│       ├── original.<ext>             # R7: the stored original (optimized once)
│       └── variants/
│           └── v<PIPELINE_VER>-<preset>-<mode>[-g<g>]-<W>x<H>.png
├── library.json                       # manifest: entries {id, title, pixel, placement, bg_color, gutter_px, hash, added}
~/.config/pixpapers/config.toml        # default_pixel="16", default_placement="fit", enabled=true,
│                                      # min_visible_px=2, palette=true, daemon opts
~/.local/state/pixpapers/              (XDG_STATE_HOME/pixpapers)
├── sock                               # CLI ↔ daemon unix socket (JSON lines)
├── daemon.pid
└── daemon.log
```

**Runtime shape**: one long-lived **daemon** (the only Wayland client). The
CLI is a thin client: `set`/`pixel`/`placement`/`enable`/`disable` send a
message over the socket; the daemon owns generation + rendering. If no daemon
is running, `set` **auto-spawns** it (detached; pidfile + stale-socket
cleanup). `pixpapers export` works fully offline (no daemon) for scripting.

```
┌────────────┐  UDS (JSON lines)  ┌──────────────────────────────┐
│  pixpapers │ ─────────────────► │ daemon (single event loop)   │
│   (CLI)    │                    │  • sctk registry + outputs   │
└────────────┘                    │  • per-output layer surface  │
                                  │  • pixcore pipeline + cache  │
                                  └──────────────┬───────────────┘
                                                 │ wlr-layer-shell (Background layer)
                                                 ▼
                                        niri / sway / hyprland / …
```

---

## 4. CLI surface (v1)

```
pixpapers add <path>... [--title T]                 # store optimized original in library
pixpapers list                                      # id, title, pixel, placement, sizes
pixpapers set <id>                                  # apply now (auto-spawn daemon)
pixpapers pixel <id> <8|16|32|64|128|normal>        # per-wallpaper look (R5)
pixpapers placement <id> <fit|cover-gutter|cover-crop|stretch>
pixpapers bg <id> <#rrggbb>                         # border/gutter color
pixpapers gutter <id> <px>                          # gutter width (cover-gutter)
pixpapers default pixel <…> | placement <…>         # defaults for new entries (R2)
pixpapers enable | disable                          # global pixelation switch (R6)
pixpapers cycle [n]                                 # rotate library, apply
pixpapers remove <id>                               # delete entry + variants
pixpapers status [--bench]                          # daemon state, outputs, cache, timings
pixpapers export <id> [--preset P] [--mode M] <W>x<H> [-o file]   # offline, no daemon
pixpapers daemon [--foreground]                     # run daemon
pixpapers stop                                      # clean shutdown
```

Behavior notes:

- `set` resolves the effective look: entry settings (R5) → config defaults
  (R2); then `enabled=false` (R6) ⇒ skip pixelation (placement unchanged).
- Any per-wallpaper knob (`pixel`/`placement`/`bg`/`gutter`) applies
  immediately to the current wallpaper if that id is showing (R7: regenerate
  on settings change).
- `set` with no daemon ⇒ auto-spawn, then send `Set`.
- All state writes are atomic (temp + rename); a crash never corrupts
  `library.json` or config.

---

## 5. Architecture (Rust workspace)

```
Cargo.toml                     # workspace + shared lint profile
crates/
├── pixcore/                   # PURE logic — no wayland, fully unit-testable
│   ├── preset.rs              # Pixel enum: B8..B128, Normal; block px; parse/display
│   ├── placement.rs           # Placement enum + View {window, dest} math (all 4 modes)
│   ├── config.rs              # config.toml (serde), defaults, validation
│   ├── manifest.rs            # library.json model + atomic load/save
│   ├── optimize.rs            # original → optimized master (cap size, re-encode)
│   ├── pipeline.rs            # iterative-box downscale, median-cut, nearest stretch
│   └── variant.rs             # cache-key/path scheme, generate, visibility floor
└── pixpapers/                 # binary
    ├── main.rs                # clap wiring
    ├── cli/                   # thin commands
    ├── ipc.rs                 # UDS client + protocol types (JSON lines)
    └── daemon/
        ├── mod.rs             # event loop (sctk), state machine
        ├── output.rs          # per-output layer surfaces, shm buffer ring, geometry/scale
        ├── render.rs          # variant + View → output buffer (bg fill + nearest stretch)
        └── spawn.rs           # auto-spawn, pidfile, stale socket, systemd unit emit
```

Key dependency choices:

| Concern | Crate | Why |
|---------|-------|-----|
| Wayland client | `smithay-client-toolkit` (sctk) | ecosystem standard |
| layer-shell | `layer-shell` (wlr-layer-shell) | background surfaces = how niri & wlroots compositors take wallpapers (also what `wallr`, linux-wallpaperengine do) |
| Image | `image` | decode png/jpeg/webp/bmp/gif/ico/tiff; encode palette PNG |
| CLI | `clap` (derive) | |
| Config/manifest | `serde` + `toml` + `serde_json` | |
| IPC | JSON lines over `UnixStream` | trivial, debuggable with `socat` |
| Paths | `dirs` | XDG |
| Logging | `tracing` + `tracing-subscriber` | daemon.log |
| Concurrency | none — single event-loop thread | race-free buffers |
| Tests | `proptest` (pipeline/place properties), `snapbox` (CLI) | |

No GPU in the daemon. CPU nearest-stretch 120×67 → 3840×2160 is a few ms and
happens once per (wallpaper, settings, output-geometry) change, throttled by
frame callbacks. (GPU stretch = possible v2 optimization, not needed.)

---

## 6. niri specifics (R9)

- Wallpaper = **background layer surface**; niri renders it under all windows,
  and it is what shows behind the **overview/backdrop** (niri can blur it —
  free win, zero work for us; document the config in COMPOSITORS.md).
- Surface hygiene: layer `Background`, `set_size(W,H)` in buffer px,
  **exclusive zone 0**, no anchors needed when size == output size.
- Multi-output: one surface per `wl_output`; each output gets the same
  wallpaper with its own placement math (variants per-output — they're KB).
- Hotplug / mode change / fractional scale: re-run placement + regenerate
  variant + re-render on geometry events; drop surface on output removal.
  We render at **buffer (physical) px** and let niri scale logically — keeps
  blocks crisp under fractional scaling.
- Verification matrix in step 8: surface visible, switch, hotplug, overview,
  no input leak (background layer takes no input per protocol), clean `stop`.

---

## 7. Performance strategy (R8)

1. **Tiny variants**: absolute-minimum canvas per (output, mode, preset) +
   palette PNG.
2. **Lazy + cached** (R7): generated only on change; cache key includes a
   pipeline version so upgrades invalidate automatically.
3. **Switch path**: daemon reads only the KB variant (the original never
   enters RAM at runtime unless preset=`normal`); nearest stretch is O(W·H)
   with perfect row-repeating cache behavior; 4-slot shm ring, attach-on-frame
   (no tearing, no allocation in steady state).
4. **No idle work**: event loop blocks on the wayland fd + IPC fd; zero
   timers; redraw only when the compositor or CLI asks.
5. **Bounded memory**: current + previous variant per output (smooth
   `cycle`); everything else is on disk.
6. **Measurable**: `status --bench` prints generate/stretch timings and RSS;
   step 9 captures before/after numbers as proof for R8.

---

## 8. Testing strategy

- **pixcore**: unit + `proptest` properties:
  - variant dims == `⌈Wc/B⌉ × ⌈Hc/B⌉` for every mode/output combination;
  - `fit` never crops, `cover-*` never leaves gaps inside the content box;
  - dest rect never exceeds output; sliver rule (≤1 px → bg) holds;
  - pipeline deterministic (same input ⇒ same bytes);
  - `normal` = identity + nearest stretch; visibility floor never lets the
    effective block drop below the floor.
- **CLI** (`snapbox`, temp `XDG_*` homes): add/list/pixel/placement/disable
  round-trips; atomic-write survives simulated failure.
- **Daemon** (manual + scripted, real niri): step-8 matrix — initial surface,
  perceived switch < 100 ms, hotplug add/remove, mode change, overview,
  fractional scale, `stop` leaves no residue, killed daemon leaves no stale
  socket and auto-recovers on next `set`.
- **Bench** (step 9): timings per preset × output; daemon idle RSS vs a
  full-res background baseline.

---

## 9. Step index

Each step file ends with **Acceptance criteria** — the step is done when all
hold. Commit after each step.

| Step | File | Deliverable (usable increment) |
|------|------|--------------------------------|
| 1 | [plan/step-1.md](plan/step-1.md) | Workspace + `pixcore` skeleton: presets, placement math (pure), config, manifest; `pixpapers add`/`list` work (no rendering yet) |
| 2 | [plan/step-2.md](plan/step-2.md) | Optimization + pipeline + `export`/`--preview`: full pixelation pipeline testable offline for any W×H, mode, preset |
| 3 | [plan/step-3.md](plan/step-3.md) | Daemon skeleton on niri: layer-shell background, solid color → rendered `normal` original, all four placements, per-output |
| 4 | [plan/step-4.md](plan/step-4.md) | Full loop over UDS: `set`/`pixel`/`placement`/`bg`/`gutter`/`enable`/`disable`/`cycle`/`stop`; lazy variant generation + cache (R4–R7 done) |
| 5 | [plan/step-5.md](plan/step-5.md) | Robustness: auto-spawn, stale socket/pidfile, crash recovery, log hygiene, `status`, systemd user unit |
| 6 | [plan/step-6.md](plan/step-6.md) | Edge cases: fractional scaling, hotplug storms, tiny/huge outputs, 4K `normal`, memory eviction, visibility floor, gutter-vs-blur open question settled |
| 7 | [plan/step-7.md](plan/step-7.md) | Docs & packaging: README, COMPOSITORS.md (niri/sway/hyprland), man page, `cargo install`, example configs |
| 8 | [plan/step-8.md](plan/step-8.md) | niri verification matrix + cross-compositor smoke (sway, hyprland); polish observed issues |
| 9 | [plan/step-9.md](plan/step-9.md) | Perf proof (R8): benchmarks, `status --bench`, before/after numbers; tag v0.1.0, changelog |

---

## 10. Risks & open questions

| Risk / question | Mitigation / decision |
|------|------------|
| **"cover-gutter" semantics**: implemented as *mat margin* (uniform `gutter_px` ring in `bg_color`). If the intent is a *blurred bleed* of the image instead, change is contained to `pixcore::place` + one pipeline step. **Confirm by end of step 2** (cheap: pipeline not yet welded to daemon). |
| `layer-shell`/sctk API drift or ergonomics | step 3 is deliberately the first Wayland touch; all sctk code quarantined in `daemon/output.rs` so a swap (raw wayland-client, `wallr`-style) stays contained |
| niri overview/backdrop surprises | step 8 matrix; our surface is a plain background layer, so compositor-side options (blur etc.) remain available |
| Fractional scaling + non-integer stretch ⇒ block edges slightly uneven | we render in buffer px; "pixelated" tolerates this; step 6 defines the acceptable-ugly bar |
| Daemon/niri restart interplay | wayland connection drop ⇒ daemon exits; next `set` auto-respawns; stale socket detection; optional systemd unit for auto-restart (step 5) |
| `image` crate has no median-cut | implement it (~80 lines, deterministic, property-tested) in `pixcore::pipeline` |
| Scope creep (TUI, X11, live wallpapers, palettes/dither per "era") | non-goals §1; v1.1 backlog section in README |

---

## 11. Retro — what actually happened (v0.1.0, 2026-10-24)

Decisions and discoveries the steps above didn't anticipate. Full test
record: [VERIFICATION.md](VERIFICATION.md); numbers: [PERF.md](PERF.md).

| Item | Where the plan said → what is |
|------|-------------------------------|
| **cover-gutter semantics** (§10 Q1) | **Shipped as the mat margin** (ring in `bg_color`) — the blurred-bleed alternative was never confirmed, and the mat is honest to the name; if the bleed is ever wanted it stays a contained pipeline change, exactly as §10 predicted. |
| **sctk drift risk** (§10) | moot — raw `wayland-client` 0.31 + `wayland-protocols-wlr` was chosen in step 3 (sctk dropped layer-shell conveniences we needed); quarantined in `daemon/wayland.rs` as planned. |
| **Fractional scale** (§10) | the dev session *is* fractional 1.5: legacy `wl_output.scale=2` ⇒ buffers oversampled 5120×2880, compositor downscales, visually correct. SCALING.md documents the exactness bar + v0.2 viewporter path. |
| **`dispatch_pending` trap** | `EventQueue::dispatch_pending` **never reads the socket** — the loop must be `prepare_read → poll → guard.read() → dispatch_pending` (commit `f5228d3`); a layer daemon that only dispatches pending hangs silently waiting for globals. |
| **Compositor loss** | exit **43** + teardown + never-restart-on-43 (unit ships `RestartPreventExitStatus=43`); plan only said "daemon exits". |
| **signal-hook silently broken here** | `flag::register` handlers *never fired* on the dev machine (V17); v0.1 dropped the dep for a 10-line raw `sigaction`. Lesson: measure your deps, even the boring ones. |
| **`xtask`** (steps 7/9 mentions) | dropped — shell scripts (`test/bench-daemon.sh`) + clap's built-in `completions` + a hand-rolled man page covered the same jobs with zero extra workspace members. |
| **`--cache-dir`** | skipped — hermetic `PIXPAPERS_HOME` already isolates the variant cache for benches. |
| **§9.4 stretch optimization** | **skipped with evidence**: hot switch p95 45 ms « 100 ms bar; 4K stretch 11.6 ms inside it. The 4K@8 cold 105 ms is median-cut, one-time per new combo. |
| **Palette variants** (§3, "subtly retro palette") | implemented and default-on — but VERIFICATION/PERF state plainly: `normal` is 1× (no magic), flat-art ratios are modest; the ≥100× headline is photographic sources only. |

---

## 12. Retro — what actually happened (v0.2, 2026-10-31)

The plan: [PLAN-30-ct.md](../PLAN-30-ct.md) — four workstreams (A
palettes & dithering, B X11, C TUI, D waybar module) run as steps
S1–S9; the self-contained per-step prompts live in
`SESSION-PROMPTS.md`. Full test record: [VERIFICATION.md](VERIFICATION.md)
§S1–S9; numbers: [PERF.md](PERF.md).

**Key decisions, as shipped:**

- **Cache key v2, no migration** (A.2): the variant filename carries the
  look — `…-{Q}[-{D}].png` — and `PIPELINE_VERSION` went 1→2, so v0.1
  caches are simply invalidated and reclaimed by the existing
  `prune_variants`. The variant cache is disposable by design; there is
  no migration code. The v0.1 default look `(auto, none)` stayed
  byte-identical — pinned by goldens captured from the untouched v0.1
  worktree (`29ef2b4`).
- **Backend seam, double-pump** (B.1): the supervisor pumps *before and
  after* the poll. The plan's literal reading — one `pump()` after the
  poll — is measurably wrong: the cold-start handshake slips one tick per
  hop, and a cold `set` returns a no-op ack (52–53 ms) while the render
  finishes in the background. The double pump restores the pre-seam
  ordering at the cost of one would-block read per idle tick
  (VERIFICATION "Re-run after S4": cold `set` back to the 91 ms before
  brackets).
- **TUI: render only when supported** (C.3): protocol detection at
  startup (kitty `ESC_G`, iTerm2 OSC 1337, sixel, else text fallback)
  shown once in the status bar, one env var (`PIX_PAPERS_IMAGE`) from
  forcing anything — never a fake image. Every action calls the same
  `commands::*` the CLI subcommand calls (invariant #1), so a TUI edit
  is byte-identical to the CLI edit.
- **Waybar as the GUI** (D.2): `pixpapers bar` — exactly one line,
  always exit 0 (the module's auto-hide contract) — plus a drop-in
  snippet whose `on-click` opens the TUI in the user's terminal. A
  pixpapers-owned toolbar button was rejected: font rasterization plus a
  second input path to keep alive, for a label waybar provides for
  free. The `on-click` is the slot a real GUI window flips into later
  (PLAN-30-ct §9): same snippet slot, `commands::*` still the only state
  path, `status --json` still the only read face.

**Settled differently than proposed** — each corrected in the plan file
or the step prompt as it was found; VERIFICATION.md records the
evidence:

| Item | Where the plan said → what shipped |
|------|------------------------------------|
| **kitty image protocol** (C.3) | the table cell said "OSC 6337" — folklore; kitty has used the APC `G` protocol (`ESC_G … ESC \`) since 0.4.0 and never OSC 6337 (verified against kitty sources v0.4.0 → master). Cell corrected in place at S6 (`aff70d6`); wire capture: VERIFICATION §S6 V-T0b. |
| **`normal` preset** (A.1) | the plan said `normal` dithers the full-size canvas; the v0.1 code never ran the quantize stage on it (plain crop, palette-encoded at encode only) — a wiring gap S3's bench exposed. S3.5 made the code match the intent: the plain-crop early return survives **only** for `(auto, none)` (the v0.1 default, byte-identical), everything else runs the full-canvas quantize before the unchanged encode. Plan amended in place (`694a248`); the 4K one-time costs documented in PERF.md. |
| **remove key** (C.4) | the action table said `rm` — the TUI would read that as `r` (refresh) then `m`; remove is the single key `D`. Table corrected in place at S7 (`433c764`), with add-by-path's result line (the CLI's verbatim wording, not the sketch) and its never-Applies semantics. |
| **waybar `format`** (D.1) | the snippet pinned `format "{ }"` — Arch's waybar 0.15.0 links the system libfmt 12, which rejects the space after `{` as an invalid automatic argument reference; the module's event box then stayed *silently hidden*. Ships as `"{text}"` (`96460e1`); live A/B in VERIFICATION §S8 "Format finding". |
| **wheel direction** (D.1) | the plan pinned only the `on-scroll-up: cycle 1` example, and the snippet as drafted ran `cycle 1` for *both* directions — directionally inert. Shipped direction-aware: up = `cycle 1`, down = `cycle -1` with wrap (`8c2f171` corrected the step prompt; `14cfa89` gave `cycle` a signed step). |
| **S5 without xvfb** (§6 S5 row) | the acceptance ran `verify-x11.sh` "under xvfb" — a host without it would have made S5 a *named skip*. The step was amended to **hard-stop** instead (`8b2891d`): an unverified x11 backend is not a mergeable S5, so the session stops and asks for the one package rather than degrading. This box got `xvfb` installed (user-approved) and the full suite ran — VERIFICATION §S5. |
| **`DISPLAY` in offline tests** (test infra) | the plan never covered it because auto-spawn only watched `WAYLAND_DISPLAY`; when S5 made `DISPLAY` alone sufficient, the *offline* test layers (which stripped only `WAYLAND_DISPLAY`) let test subprocesses spawn real daemons against the user's live X session. `smoke.sh` and the harness now strip both (`20e73ce`). |
| **exit-code contract** (S9 task 7) | S8 found a pre-existing violation: `cycle` on an empty library bailed with a bare message `is_user_error` did not recognise — a user condition exiting 2. The audit (`dde4d1f`) moved the user-condition paths to 1 (empty library; unknown ids on `set`/knobs/`export`; unparseable sizes — messages byte-identical) and reported, without fixing, the three out-of-scope exits-2 paths: `export -o` write failure (io), clap usage errors (framework), and the duplicate-foreground-`daemon` bail that has been 2 since v0.1 `10dbc8b` — whose doc claim ("exits 1") was the actual bug, corrected doc-side. |
