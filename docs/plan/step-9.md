# Step 9 — Performance proof (R8) & v0.1.0 release

**Goal**: numbers on paper that the marginal perf promise (R8) is real,
`status --bench` exists, and the release ships.

**Depends on**: step 8 (RC frozen).

---

## 9.1 Benchmark harness — `benches/` + `pixpapers status --bench`

`pixcore` criterion benches (CPU side — the whole interesting surface):

| Bench | Measures | Inputs |
|-------|----------|--------|
| `optimize` | decode→cap→re-encode | 8K photo, 4K flat-art, 1080p |
| `downscale` | iterative-box → variant canvas | 4K→{1080p,4K} canvas × presets {8,16,32,128} |
| `median_cut` | quantize to 256 | 1080p/4K canvases |
| `composite_stretch` | nearest variant→full buffer (+bg fill) | 120×67→1920×1080; 240×135→3840×2160; per placement |
| `switch_path` | end-to-end: cache-hit read+decode vs cache-miss generate+composite | the real `apply()` core (mock target) |
| `variant_size` | bytes of variant PNGs | matrix: preset × {photo, flat-art} × 1080p/4K — **the R8 "absolute minimum size" table** |

Daemon-side one-offs (scripted, not criterion — `xtask bench-daemon`):

- idle RSS: daemon alone vs *reference* (`wallpaper running as raw full-res`
  via `wallpaper`-style shm client or simple `swaybg` full-res) — same
  outputs, same session.
- switch latency p50/p95: `time (pixpapers set X)` × 50 alternating entries
  (cache hot & cold runs separately).
- fd/handle counts cold vs after 1000 switches (assertion, not plot).

## 9.2 The R8 honesty doc — `docs/PERF.md`

Publishes: variant-size table (KB vs original MB — the headline), switch
latency (hot: ms; cold: first-time generation ms), idle RSS delta, pipeline
timings, and the explicit **steady-state wash** statement (buffer is
output-sized either way; the wins are switch-time CPU, RAM-per-daemon, and
disk). Any number that turned out *worse* than promised gets fixed or the
README claim gets walked back — docs before marketing.

## 9.3 `status --bench`

`pixpapers status --bench` prints: last-switch ms, cache hits/misses + sizes,
daemon RSS, and a stored snapshot of the pipeline timings from the daemon's
own renders (rolling max/median per output — it already times composite;
surface it, zero extra cost).

## 9.4 Optimize what the numbers say (timeboxed 1 day)

Candidates pre-ranked (do **only** if benches justify):
1. row-duplicated stretch (write each source row B times via `copy_within`)
   — likely 2–4× on integer scales;
2. `fast_image_resize` behind a feature flag (avoid by default — dependency
   discipline; decide on numbers);
3. mmap variant reads (>10 KB variants only benefit — probably skip);
4. GPU path — **out of scope v1** unless stretch is embarrassing at 4K/8K.

## 9.5 Release v0.1.0

- Final gate: full suite + step-8 matrix re-run (quick pass on V1–V3, V13,
  V14 only — RC unchanged since 8.4 except opt-landed fixes, each with
  tests).
- `CHANGELOG.md` final (keep-a-changelog format, generated from step
  commits + fix commits, human-passed).
- `git tag v0.1.0` → release workflow (step 7) publishes: source tarball +
  x86_64 binary + SHA256SUMS + docs PDF-less (plain md fine).
- PKGBUILD updated to the tag; `cargo install` verified from the *published*
  artifact set once (fresh user, fresh VM).
- README: "Tested with" versions, perf box filled with §9.2 numbers, status:
  **v0.1 — daily-driver on niri; sway/hyprland smoke-tested**.
- Retro: one paragraph in `docs/PLAN.md` footer — what the plan got right,
  where reality bent it (gutter semantics, floor default, layer-shell crate
  experience) — so the next project starts smarter.

## 9.6 Acceptance criteria

1. `docs/PERF.md` complete: variant-size table, switch latencies, RSS delta,
   pipeline ms — every R8 claim in README traceable to a line there.
2. Variant sizes demonstrate "absolute minimum" (e.g. 1080p @ 16 ≤ 10–20 KB
   on both photo and flat-art) and beat originals by ≥ 100×.
3. Switch latency p95 < 100 ms cache-hot (V2 bar, now numerically logged).
4. `status --bench` output verified against `bench-daemon` script numbers.
5. Any §9.4 optimization: gated behind evidence, benchmarked before/after,
   tested; otherwise explicitly skipped.
6. v0.1.0 tagged, artifacts published + checksummed, install-from-artifact
   verified, CHANGELOG human-passed.

**Commit**: `step-9: benches, PERF.md, status --bench, v0.1.0 release`
