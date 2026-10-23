# Step 6 — Edge cases: scaling, hotplug storms, odd geometries, eviction, floor

**Goal**: the long tail made explicit and mostly tested. Fractional scaling,
hotplug chaos, absurd gutter widths, tiny outputs, 4K `normal`, memory
boundaries, visibility floor. Every item here either ships fixed, gets a
documented accepted-limitation, or (rarely) becomes a tracked issue — none
shippable as "surprises the user".

**Depends on**: step 5.
**Requirements covered**: R3 details, R8 boundaries, R9 robustness, PLAN §10
risk closures.

---

## 6.1 Fractional & per-output scaling (R9)

- niri supports fractional scaling; layer-shell surfaces on wlroots get a
  **logical** size + `scale`. Strategy (PLAN §6): compute `View` in
  **buffer px** (mode.w × actual scale), composite at buffer px, submit with
  `wl_surface::set_buffer_scale(1)` + buffer sized to what niri expects for
  that output's effective geometry. Verify empirically on niri at
  {1.0, 1.25, 1.5, 1.75, 2.0} and record in `docs/SCALING.md`:
  - blocks stay square at fractional scale? (expected: niri's final scale of
    our buffer-px grid slightly resamples block edges — acceptable-ugly bar:
    **blocks remain visually square ±1 px at 1.5×**; if niri exposes
    wp-fractional-scale to layer surfaces, attach with the right scale instead
    — the empirical note decides.)
  - mixed-scale outputs each get their own variant (cache key already
    per-output px — no code change, verify).
- `status` shows effective buffer size + scale truthfully.

## 6.2 Hotplug storms & geometry churn (R9/R8)

- **Storm test (niri + `wlr-randr`/`niri output` cycling, or sway + env
  vars)**: rapid enable/disable cycles of an output while daemon runs:
  - surfaces track reality (no orphans — check compositor layer info);
  - no variant churn *renders*: coalesce per-output geometry events within a
    loop drain (step 3.3 debounce) — assert render count ≤ event count / 4
    in the storm log;
  - eviction handles churn (variant files for a vanished output mode get
  cleaned: LRU of generated-but-unreferenced keys per boot, cap 8 files).
- **Resolution change on a live output** (`niri` mode switch, xrandr-ish):
  re-View + regenerate (cache miss expected) + re-render ≤ 1 frame.
- **Output gone mid-generate**: variant write completes, render skipped, no
  panic (surface lookup None-safe) — unit test with mock target ordering.

## 6.3 Odd geometries (R3 boundaries)

Test matrix (mock-wayland, `View` + `composite` golden):

| Case | Expected |
|------|----------|
| output 800×600, preset 128 | canvas 7×4 (ceil), blocks 128×128-ish — deliberately chunky, no panics |
| output 1920×1080, gutter 1919 | content box 2×1080 clamped ≥ (1,1): gutter eats everything except a stripe — **ugly by construction**, documented as user's choice; no crash |
| gutter ≥ W | clamp content box to (1,1); note in CLI when applying (`gutter 3000 > width 1920 — clamped`) |
| portrait src → landscape out, all modes | fit = pillarbox bg; cover-crop = top/bottom crop centered; stretch = squash; gutter = same with ring |
| 1×1, 1×N degenerate source | clear add-time rejection (min side 16) — CLI error, not render-time |
| 8K source, `normal`, 4K output | capped at add (max_side 3840) ⇒ decode ≤ 4K; RAM peak asserted (< 150 MB) via bench in step 9 |
| preset 128 with fit borders on ultrawide | blocks exactly 128 on content box (ceil canvas) — assert with corner probes |

## 6.4 Visibility floor (PLAN §2.1) — decide + wire

- Behavior: effective block = `B × (output_phys / canvas)` — canvas ceil()
  makes it ≥ B already; floor matters only if a future "auto" preset is
  added. **v1 decision: keep floor OFF by default** (`min_visible_px = 0`
  default — config key reserved), with the mechanism unit-tested and the
  "auto" preset left as a v1.1 idea in README backlog. Rationale: presets are
  exact numbers the user picks (their list: 8/16/32/64/128); silently bumping
  contradicts "controls are per wallpaper" (R5). Revisit with user **before
  step 8** — logged decision, one-line change either way.

## 6.5 Memory & eviction boundaries (R8)

- Assert `OutState.keep = 2` behavior under: 100× `cycle` on 4 outputs →
  on-disk variants bounded (≤ 8 files + referenced), daemon RSS flat (bench
  plot step 9), original decoded only in `Normal` windows (heap-prof spot
  check with `dhat` optional feature — run once, commit numbers).
- `variant::generate` under ENOSPC / read-only lib dir → IPC error, screen
  unchanged (mock fs test).

## 6.6 IPC edge behavior

- Client disconnect mid-command → command completes atomically (one command
  = one `apply()` — no partial multi-output state).
- Two concurrent `set A`, `set B` from two clients → serialized by event
  loop; final state == last-arrived; no interleaved per-output mix (assert in
  §4.5-style test with injected delays).
- `bg "#12"` (bad length), `gutter -3`, `pixel 7` → clap/parse rejects client
  side (exit 1, helpful message) **and** daemon rejects (defense in depth —
  IPC bypass tested).

## 6.7 Settle cover-gutter "blur" alternative (if user chose it in step 2)

If step 2's decision was "blur" (instead of mat): implement
`pipeline::bleed(img, k)` — scale edge strip ×k with box-blur (fast
separable), fill outside-content-box, cache-key suffix `-blur`; goldens +
perf assertion (blur of a 120×67 canvas is trivial — do it on the **variant**
canvas before stretch, keeps it KB-cheap). Otherwise: skip, mark resolved.

## 6.8 Acceptance criteria

1. §6.1 scaling matrix verified on niri at ≥ three scale factors; findings
   in `docs/SCALING.md` with screenshots; behavior matches the stated bar.
2. Hotplug storm: no orphan surfaces, render-count bound asserted in log
   analysis, variant files bounded.
3. §6.3 matrix: zero panics (mock-wayland fuzz over geometry × settings —
   `proptest` driving `apply()` with the recorder target); clamps logged.
4. Eviction bounds: 100-cycle run shows flat RSS + bounded disk (numbers
   into step-9 bench doc).
5. IPC edge tests green (concurrency serialization, input validation both
   sides).
6. Visibility-floor decision recorded in PLAN §10 (default chosen, rationale
   one-liner).
7. `cargo test/clippy/fmt` + all prior acceptance stay green.

**Commit**: `step-6: scaling notes, hotplug hardening, geometry edge matrix, eviction bounds, IPC hardening`
