# Step 8 — Verification matrix: niri deep + cross-compositor smoke

**Goal**: the feature-freeze gate. Every claim in README/PLAN is checked
against a real compositor, niri hardest. Issues found are fixed here (small,
contained) — this step ends with a release candidate.

**Depends on**: step 7 (docs exist so claims are checkable). **Freeze**: no
new features after this step's start; fixes only.

---

## 8.1 niri verification matrix (primary — run on the dev machine, logged)

Legend: ✅ scripted/manual-pass · 📸 screenshot committed `docs/screens/step8/`

| # | Scenario | Pass bar |
|---|----------|----------|
| V1 | cold: `set` auto-spawns, wallpaper ≤ 1 s (spawn+gen+render) | timed, log-stamped |
| V2 | switch A→B→A…×20 | each < 100 ms perceived; no flicker (attach-on-frame) |
| V3 | every preset × every placement (6×4) on main output | matches step-2 goldens visually; 📸 grid |
| V4 | multi-output mixed (4K + 1080p, different scales if available) | each output correct; `status` truthful |
| V5 | hotplug: unplug/replug DP; add via `wlr-randr` if GPU allows | surface follows; no orphan (niri layer info / visual) |
| V6 | mode change via niri (res/refresh) | re-render ≤ 1 frame; variant cache correct |
| V7 | fractional scale 1.0/1.25/1.5/2.0 | per SCALING.md bar; 📸 |
| V8 | niri overview/backdrop | wallpaper visible + blur follows niri config; recommended stanza from COMPOSITORS.md applied |
| V9 | input: click where wallpaper is (desktop) | no input interception — clicks reach windows/behaviour identical to no-daemon |
| V10 | fullscreen window (video/game, `niri` fullscreen) | wallpaper stays below; no z-order weirdness on exit-fullscreen |
| V11 | daemon `kill -9` → `set` | recovery ≤ 1 s (step-5 path proven live) |
| V12 | niri restart (kill + relaunch session) | daemon exits 43; next `set` respawns; no loop; 📸 TROUBLESHOOTING accurate |
| V13 | `disable`/`enable`/`pixel`/`placement`/`bg`/`gutter` live | instant, no blanking, no layout shift |
| V14 | `stop` | surfaces gone, daemon gone, socket/pid gone |
| V15 | 72 h idle soak (dev machine overnight+ over days) | RSS growth < 2 MB; log ≤ cap; no fd creep (`ls /proc/<pid>/fd | wc -l` daily) |
| V16 | 1000× IPC command fuzz (random valid+invalid sequences, mock + real) | no panic, no corrupt manifest/config (hash-compare before/after) |

## 8.2 Cross-compositor smoke (secondary)

- **sway** (headless `sway -s` config or real): V1–V3, V9, V14 — layer-shell
  is the same protocol; expectations identical. Notes into COMPOSITORS.md.
- **hyprland**: same subset + the "don't run hyprpaper too" note verified
  (run both → last-wins visual documented).
- **river/cosmic**: opportunistic (if installed quickly); "reported working
  via layer-shell" in README, no more.
- Record tested-compositor + version table in README ("Tested with" —
  honesty over marketing).

## 8.3 Fix policy inside this step

- Any V-failure: minimal fix + regression test added in the owning module
  (reuses the mock-wayland/IPC harnesses — this is why they exist).
- If a failure is a protocol/crate rabbit-hole (layer-shell wrapper bugs):
  document + workaround in COMPOSITORS.md, keep for v0.1.1 — unless it
  breaks V1–V3/V9/V14 (release blockers).

## 8.4 Release-candidate checklist (end of step)

- [ ] V1–V16 all ✅ (or documented accepted-limitation + issue filed)
- [ ] smoke matrix recorded
- [ ] all fixes landed with tests; suite green; `-D warnings`
- [ ] goldens/screenshots committed; README claims now all evidence-backed
- [ ] `CHANGELOG.md` drafted from step commits (`git log --reverse` walk)
- [ ] version bump chore commit `chore: release v0.1.0-rc1`, tag — step 9
      cuts the real tag after perf proof

**Commit**: `step-8: niri verification matrix + cross-compositor smoke (+fixes)`
