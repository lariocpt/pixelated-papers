# Step 4 — Full control loop: IPC, lazy generation, forced-pixelation policy

**Goal**: the product loop closes. `pixpapers set <id>` (from any shell)
applies the wallpaper everywhere with the entry's resolved settings;
per-wallpaper knobs and enable/disable take effect instantly; variants are
generated **lazily on change** and cached. After this step, the tool is
daily-drivable.

**Depends on**: steps 1–3.
**Requirements covered**: R2, R4, R5, R6, R7 (full), R8 (lazy/cache paths).

---

## 4.1 IPC protocol — `pixpapers::ipc`

Unix socket at `$XDG_STATE_HOME/pixpapers/sock` (`0600`), **JSON-lines**:

```jsonc
// client → daemon requests (id echoed):
{"cmd":"set","id":"a1b2c3d4"}
{"cmd":"pixel","id":"a1b2c3d4","pixel":"8"}
{"cmd":"placement","id":"…","placement":"stretch"}
{"cmd":"bg","id":"…","bg":"#001122"}
{"cmd":"gutter","id":"…","px":24}
{"cmd":"enable"} {"cmd":"disable"}
{"cmd":"cycle","by":1}
{"cmd":"status"}
{"cmd":"quit"}
// daemon → reply (one line):
{"ok":true,"data":{...}}   or   {"ok":false,"error":"entry not found: a1b2…"}
```

- `status` reply: current entry + resolved settings per output, variant cache
  stats (hits/misses, sizes), daemon pid/uptime, n outputs — feeds the
  `pixpapers status` table (step 5 adds `--bench`).
- Server = `UnixListener` bound into the sctk event loop via
  `EventQueue::handle().insert_source(fd, …)` — **one thread, two fds**, no
  async runtime (R8: zero idle cost).
- Client helper: `connect_or_spawn()` (spawn logic step 5; step 4 = clear
  error "daemon not running; start with `pixpapers daemon`").
- Malformed JSON / unknown cmd → error reply, connection stays open.
  Read timeout (10 s idle) on client connections — no fd creep.

## 4.2 Command semantics (the policy core — R4/R5/R6/R7)

Single resolver, used by every mutating command:

```rust
fn apply(entry_id, settings_edit) {
    let cfg = config.load();
    let resolved = entry.settings.resolve(&cfg);          // R5 → R2
    let effective = if cfg.enabled { resolved } else {
        ResolvedSettings { pixel: Pixel::Normal, ..resolved } // R6: placement untouched
    };
    for out in outputs {
        let view = place(src_dims, out.phys, effective.placement, effective.gutter_px);
        let vpath = variant::generate_or_cache(orig, effective, out.phys)?; // R7: lazy here
        schedule_render(out, vpath, view, effective.bg_color);
    }
    manifest.set_current(entry_id);                       // for crash-restore, step 5
}
```

- **`set`**: entry → resolver → per-output generate-or-cache → render.
  Perceived switch target < 100 ms (variant cache hit) — verified step 8.
- **`pixel|placement|bg|gutter`**: mutate the **entry** in manifest (R5),
  then if entry is current → `apply` immediately (R7: "on image/solution
  change").
- **`enable/disable`**: write config `enabled`, then re-`apply` current entry
  (forces pixelated ↔ original, R6).
- **`cycle [n]`**: current index ± n in `library.entries` order (wrap), then
  `set`.
- Failed generation (corrupt original) → error reply + **keep last good frame
  on screen** (never blank the desktop).
- `quit`: render teardown then exit 0 (same path as SIGTERM).

## 4.3 Lazy generation + cache policy (R7/R8)

- Nothing is pre-generated at `add` beyond the optimized original (step 2).
- `variant::generate` (step 2) is the whole story: stat cache → miss → run
  pipeline → atomic write → return path. First `set` at 16/fit pays the
  pipeline (~20–40 ms, step 9 measures); every repeat is a read + decode of a
  few KB.
- **Eviction** (R8 §7.5): `OutState.keep: VecDeque<(key, path)>` — current +
  previous key per output (smooth `cycle`); on push-back, delete evicted
  variant file if no other output references it. Disk stays ≈ a handful of KB
  per wallpaper ever shown.
- Original never decoded at runtime unless `pixel == Normal` (cache the
  decoded original *between* renders only while `Normal` is active; drop on
  switch — bounded RAM).

## 4.4 CLI wiring (replaces step-3's flags)

- `set|pixel|placement|bg|gutter|enable|disable|cycle|stop` now send IPC;
  `daemon` gains no new flags (test flags deprecated, removed).
- `status`: pretty table + `--json`.
- Offline commands (`export`, `add`, `optimize --all`) keep working without a
  daemon (pixcore-only path — already true since step 2; add a test).
- `pixpapers list` marks the current entry (`*`).

## 4.5 Tests

- **Policy table (the important one)** — `#[test]` matrix over
  `{enabled, entry.pixel, cfg.default_pixel}` × expected effective pixel:
  - entry set → entry wins (R5);
  - entry null → config default (R2);
  - `enabled=false` → `Normal` regardless, placement/bg preserved (R6);
  - entry explicitly `normal` → never pixelated (R5's "off per wallpaper").
- **IPC integration** (no wayland): daemon core behind a trait
  (`trait ApplyTarget { fn apply_render(&mut self, …) }`) with a
  `RecorderTarget` mock; run the real UDS server + real client over a tmp
  socket; assert exact render-schedule events per command (set/pixel/disable/
  cycle/quit…), error replies (bad id, bad color, bad JSON), and manifest/
  config persistence after each.
- **Cache/eviction**: tmp home; set A→B→A asserts 2 variant files survive,
  3rd deleted; A→B→C keeps exactly B,A files… (two-output variant included).
- Golden from step 2 asserted again via mock target (daemon cannot drift from
  export).

## 4.6 Manual niri pass

1. Terminal 1: `pixpapers daemon --foreground`. Terminal 2: `pixpapers set <id>`
   → instant change; `pixpapers pixel <id> 8` → instant; `disable` → original
   (still stretched/bordered); `enable` → pixelated again.
2. `cycle 1` repeatedly → wraps; `status` shows truth; `stop` exits cleanly.
3. Kill -9 daemon → next `set` errors cleanly (spawn comes step 5); surfaces
   gone with it (compositor behavior — expected, document).
4. Two different entries with different placements on the same output in a
   row — correct each time (no stale view/bg state).

## 4.7 Acceptance criteria

1. Full loop works on niri per §4.6 (demo gif/screenshot committed).
2. Policy matrix + IPC integration + eviction tests green.
3. Cache-hit switch feels instant (< 100 ms; `status` reports hits; exact
   numbers captured for step 9 baseline).
4. Error paths never blank the screen or corrupt manifest/config.
5. R4–R7 demonstrably enforced in one resolver function (review: grep-able
   call sites — `apply()` is the only path to surfaces).

**Commit**: `step-4: IPC control loop, lazy variant generation, forced-pixelation policy`
