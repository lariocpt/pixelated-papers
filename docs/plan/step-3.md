# Step 3 — Daemon skeleton: layer-shell background on niri

**Goal**: a Wayland client that puts a **background layer surface** on every
output in niri and shows a rendered wallpaper buffer — initially driven by
flags only (`pixpapers daemon --test-image <path> --pixel 16`), no CLI/IPC
loop yet (that's step 4). All sctk/wayland code gets written once here and
quarantined.

**Depends on**: step 2 (renders the exact `pixcore::render::composite` output).
**Requirements covered**: R3 (on-screen), R9 (niri), groundwork for R7.

---

## 3.1 Protocol stack

- **wlr-layer-shell-unstable-v1**, layer `Background` — this is how every
  wlroots-family compositor (niri included) takes wallpapers; no compositor
  shell extensions needed.
- **xdg_wl_output** via sctk for geometry + scale; **wl_shm** for buffers.
- Crates: `smithay-client-toolkit` (registry, outputs, shm, seat-free) +
  `layer-shell` (the wlr-layer-shell wrapper crate). If the wrapper fights us
  (API drift risk from PLAN §10), fallback is `wayland-client` +
  `wayland-protocols-wlr` raw — this decision is made *inside* `output.rs`
  only; the daemon state machine must not know which is used.

## 3.2 Per-output surface — `daemon/output.rs`

For each `wl_output` (and on hotplug-add):

```
new_layer_surface:
  layer = Background
  namespace = "pixelated-papers"
  size = (output.buffer_w, output.buffer_h)   // physical px (mode.w, scale-aware)
  exclusive_zone = 0                          // never push panels/windows
  anchor = []          // size == output ⇒ anchors cosmetic; set all 4 with margin 0
  keyboard_interactivity = None
  mouse enabled? NO-OP — background layer takes no input per protocol
```

- **SHM ring**: 4 buffers × `W*H*4` ARGB8888, allocated lazily per output and
  reused; on geometry change → reallocate. `create_pool` via sctk shm; buffer
  attach → `surface.commit()` → `frame.callback()`; we attach the *next*
  buffer only after the previous frame callback fires (no tearing, no
  allocation in steady state — R8 §7.3).
- Buffer fill = `pixcore::render::composite_into(&variant_img, &view, bg,
  &mut [u8])` — step-2's composite writes **directly into the shm mapping**
  (zero intermediate copy).
- Scale: render in **buffer/physical px**; set surface buffer scale = 1 and
  size = buffer px / output.scale as *logical* size where the protocol wants
  logical. With fractional scaling (wp-fractional-scale not required for
  layer-shell on niri — verify in step 6) let niri do the final scale; blocks
  stay crisp because the grid is defined in buffer px (PLAN §6).

## 3.3 Event loop — `daemon/mod.rs`

- `sctk::EventQueue<state>` + the IPC listener fd **added in step 4**; for
  now: wayland fd only + `SIGTERM/SIGINT` → graceful teardown
  (destroy surfaces, flush log).
- State:
  ```
  Daemon { outputs: Slab<OutputId, OutState>, current: Option<CurrentWall>,
           config: Config, lib: LibraryHandle }
  CurrentWall { entry_id: String, resolved: ResolvedSettings }
  ```
- Output events:
  - `output_added` → create `OutState` + surface + render current;
  - `geometry_changed` / `scale_changed` / `mode_changed` → recompute
    `View`, regenerate variant (pixcore cache — likely a miss only if size
    truly changed), re-render; **debounce**: coalesce events per output
    within one loop iteration (natural with event queue draining; storms in
    step 6);
  - `output_removed` → destroy surface, drop `OutState`.

## 3.4 Test drive (pre-IPC)

```
pixpapers daemon --foreground [--test-image path | --entry id] [--pixel P] [--placement M] [--bg C] [--gutter G]
```

- No daemon-side generation policy yet: it renders that one image through
  pixcore for every output; `Ctrl-C`/SIGTERM exits cleanly (surfaces vanish,
  niri shows its backdrop — proof of protocol hygiene).
- This flag surface is temporary; step 4 replaces it with IPC commands and
  keeps `--foreground` for debugging.

## 3.5 Manual verification on niri (run on the real machine)

Checklist (repeat on each output of the dev machine):

1. Start daemon → wallpaper appears **under all windows**, no window layout
   shift (exclusive zone 0 → niri windows don't move — check `niri msg
   outputs`/visual), no panel displacement.
2. Presets 8/16/32/64/128/normal cycle visually via flag restarts — crisp
   blocks, correct bg borders on `fit`.
3. All four placements: `fit` (borders = bg color), `cover-gutter` (mat ring),
   `cover-crop` (centered crop, no borders), `stretch` (fills, distorted ok).
4. niri **overview**: wallpaper visible behind overview (and blurred if the
   user's niri config blurs the backdrop — capture screenshot for docs).
5. Kill daemon (`kill -TERM`) → surfaces gone, no ghost, niri unaffected.
6. `niri msg layer-shell` (or `weston-info`/`niri msg outputs`) shows our
   namespace on Background layer.
7. Suspend/resume (if laptop test surface exists): surfaces survive; if niri
   re-creates outputs, geometry path (3.3) handles it.

## 3.6 Unit-ish tests (what can be tested headless)

- `composite_into` vs `composite` (step 2) byte-identical (daemon and export
  cannot diverge — this is the "export is the truth" property).
- Buffer stride handling: non-trivial stride (pad rows) composited correctly
  (unit-testable with a fake `&mut [u8]` of `stride*H`).
- `View` recompute on fake geometry change (no wayland needed — pure).
- A `mock-output` feature-gated test harness: `OutState` transitions driven
  by hand (added→geometry→removed) assert surface create/destroy call counts
  via a trait `SurfaceFactory` (mock in tests, real impl in step 3 wiring).

## 3.7 Acceptance criteria

1. On this machine's niri: `pixpapers daemon --foreground --entry <id>
   --pixel 16` shows the pixelated wallpaper on **all outputs**, stretched
   per mode, borders per settings; switches via flag restart look right.
2. Checklist §3.5 all green (screenshots committed under `docs/screens/`).
3. SIGTERM → clean exit, zero residue; repeated start/stop 10× leaks no fd
   (`ss`/`lsof` check documented once).
4. `cargo test/clippy/fmt` green; wayland surface tests via mock pass.
5. All sctk/layer-shell imports live **only** in `daemon/output.rs`
   (`cargo tree`/grep check in CI later — step 7).

**Commit**: `step-3: layer-shell daemon skeleton — per-output background surfaces, direct shm composite`
