# Performance (R8: "faster, even in a negligible way")

All numbers measured on the dev machine, release build, niri 26.04,
3840×2160@60 at fractional scale 1.5 (buffers 5120×2880 — see
SCALING.md). Re-run recipes at the bottom; `pixpapers status --bench`
prints the daemon's live counters anytime.

## The R8 headline: variants at the absolute minimum size

Palette-PNG variants vs the stored original (`variant_sizes` example):

| source @ preset | variant | original | ratio |
|---|---|---|---|
| photo 1080p @ 16 (the default) | **6.3 KB** | 1.94 MB | **310×** |
| photo 1080p @ 8 | 15.1 KB | 1.94 MB | 129× |
| photo 1080p @ 64 | 1.4 KB | 1.94 MB | 1435× |
| photo 4K @ 16 | 22.3 KB | 7.80 MB | 350× |
| flat-art 1080p @ 16 | 2.8 KB | 42 KB | 15× |
| `normal` preset (any) | ≈ original | — | **1×** |

**Honest reading.** The "≥100× smaller" claim is true for photographic
sources (what wallpapers usually are) at pixel presets. Two walk-backs,
stated plainly:

- **Flat art** that PNG already crushes (42 KB) only gains 8–67×. The
  tool is still right — a 2.8 KB canvas is the *minimum to display at all*
  — but the ratio sounds sexier with photos.
- **`normal`** does no shrinking magic (it's the "no pixelation" escape,
  palette-*encoded* ≈ 8× for photos — a PLTE over the full canvas, 1× for
  well-compressed art; the quantize/dither stage never runs on `normal`).
  It exists to not lie about pixelation, not to save bytes.

## Switch latency (what a human feels)

`test/bench-daemon.sh`, full CLI→IPC→render→commit wall time, hot cache:

| | ms |
|---|---|
| cold: spawn + first variant + render + commit | **91** |
| hot switch p50 (n=50 alternating) | **23** |
| hot switch p95 / max | **45 / 45** |
| daemon self-timed render median / max | 20 / 78 |

The plan's bar was "each < 100 ms perceived" — met with 2× margin. Note
the wall time includes ~10 ms of CLI process spawn; the daemon's own work
is the median-20 line.

## CPU pipeline (criterion, `cargo bench -p pixcore`)

| stage | input | time |
|---|---|---|
| iterative-box downscale (any preset) | 1280×800 source | ~3.0 ms |
| median-cut → 256 | 1080p-class canvas | **4.0 ms** |
| median-cut → 256 | 4K @ 8px canvas (130k px) | 63 ms |
| composite nearest-stretch | → 1920×1080 | **2.7 ms** |
| composite nearest-stretch | → 3840×2160 | 11.6 ms |
| cold switch (canvas+palette+composite) | 1080p @ 16 | **10.9 ms** |
| cold switch | 4K @ 8 | 105 ms |

Reading: a cold switch at the *smallest* preset on 4K is a one-time ~0.1 s
(median-cut dominates — 130k pixels of quantization). Everything the hot
path touches — variant read (KB), decode, 11.6 ms stretch — stays deep
under the 100 ms bar.

## v0.2 palettes & dither (PLAN-30 A.4)

Measured at commit `45c9aa6` on the same dev machine, release build, by
two pure-CPU headless scripts: `test/bench-dither.sh` (stage timings) and
`test/bench-sizes.sh` (byte sizes + hashes). Both drive the test-only
helper `test/bench-quantize/` over the same deterministic `photo()`
fixture as the `variant_sizes` example, so these rows line up with the
R8 table above. Timing medians move a few % run-to-run; the size table is
deterministic (hashes reproduce exactly).

### Dither cost in canvas space (`test/bench-dither.sh`)

`pipeline::quantize` on the preset canvas shapes, auto palette, min /
median ms (warmup excluded; Δ vs the `none` median of the same row group):

| canvas | shape | dither | min ms | median ms | Δ vs none |
|---|---|---|---|---|---|
| 120×68 | 1080p @ 16 (the default) | none | 5.35 | 6.61 | — |
| 120×68 | 1080p @ 16 (the default) | fs | 5.86 | 6.93 | +0.32 ms |
| 120×68 | 1080p @ 16 (the default) | bayer8 | 5.34 | 6.62 | +0.01 ms |
| 240×135 | 1080p @ 8 · 4K @ 16 | none | 22.69 | 25.10 | — |
| 240×135 | 1080p @ 8 · 4K @ 16 | fs | 23.81 | 26.31 | +1.21 ms |
| 240×135 | 1080p @ 8 · 4K @ 16 | bayer8 | 22.69 | 25.13 | +0.02 ms |
| 480×270 | 4K @ 8 | none | 89.09 | 97.37 | — |
| 480×270 | 4K @ 8 | fs | 91.92 | 102.99 | +5.62 ms |
| 480×270 | 4K @ 8 | bayer8 | 92.16 | 97.74 | +0.37 ms |

**Reading.** Dithering in canvas space is noise at the preset shapes: FS
adds ≤ 5.6 ms even on the largest preset canvas (4K@8), bayer8 ≤ 0.4 ms
everywhere. The stage cost is dominated by median-cut (`auto`), not by
the dither — which is exactly why dithering lives in canvas space
(plan §A.1). And it is one-time + cached either way: a dithered variant
is generated once and served from the hot path forever after.

### The one-time full-res case

A `normal`-sized canvas (1920×1080) through `quantize` directly — the
plan's `normal`+`vga`+`fs` case and its worst-case neighbour:

| palette | dither | min ms | median ms | Δ vs none |
|---|---|---|---|---|
| vga | none | 510.20 | 527.60 | — |
| vga | fs | 479.17 | 614.25 | +86.65 ms |
| auto | none | 1721.78 | 1745.92 | — |
| auto | fs | 1763.77 | 1820.14 | +74.22 ms |

(min is noisy at these durations; median is the honest column.)

Two honest caveats:

- **`normal` never reaches this code today.** `generate_from_image`
  returns the crop *before* the quantize stage for identity pixels, so
  the palette/dither knobs are no-ops on `normal`. PLAN-30 §A.1 describes
  `normal` as dithering the full-size canvas; the shipped wiring does not
  (S3 measures, it does not change behavior — flagged for a plan/step
  decision). The rows above are the stage cost measured directly: what
  that wiring would pay, and what the plan's risk row budgets against.
- **Budget check (plan §8: “> 1 s at 4K → document with the number,
  keep”):** the plan's named case (`vga`+`fs` on the full-res canvas) is
  **0.61 s** — inside budget. `auto`+`fs` at full res is **1.82 s** —
  over the 1 s line, and it is median-cut at 2.07 M px that puts it there
  (`auto`+`none` alone is 1.75 s; the dither adds only +74 ms). Documented
  with the number per the risk row; not reachable via `normal` today, and
  one-time + cached wherever a full-res canvas is ever quantized.

### Variant sizes: v0.1 baseline vs v2 default vs named palettes (`test/bench-sizes.sh`)

Same deterministic photo fixtures as the R8 table (1.94 MB / 7.80 MB
originals), dither `none`. Table verbatim from the script (blake3/12
included so any rerun can be pinned byte-for-byte):

| fixture | preset | canvas | palette | variant | bytes | blake3/12 | Δ vs auto |
|---|---|---|---|---|---|---|---|
| photo 1080p | 16 | 120×68 | auto | 6.3 KB | 6430 | 597ffb135c19 | — |
| photo 1080p | 16 | 120×68 | nes | 3.5 KB | 3535 | 4c51e4de20be | −45% |
| photo 1080p | 16 | 120×68 | gb | 1.4 KB | 1476 | 45542a234644 | −77% |
| photo 1080p | 16 | 120×68 | cga | 2.2 KB | 2264 | 1dba39c0a31d | −65% |
| photo 1080p | 16 | 120×68 | ega | 3.1 KB | 3215 | 2f29c6dd40cd | −50% |
| photo 1080p | 16 | 120×68 | vga | 5.2 KB | 5277 | 04dbf92fda9c | −18% |
| photo 1080p | 16 | 120×68 | gen16 | 2.6 KB | 2705 | bf2c5e10c11b | −58% |
| photo 1080p | 8 | 240×135 | auto | 15.1 KB | 15499 | 456d816cf0b3 | — |
| photo 1080p | 8 | 240×135 | nes | 8.3 KB | 8546 | 6ef897a1a5f5 | −45% |
| photo 1080p | 8 | 240×135 | gb | 3.3 KB | 3355 | 0f94064c78d2 | −78% |
| photo 1080p | 8 | 240×135 | cga | 5.2 KB | 5286 | 054e2129dada | −66% |
| photo 1080p | 8 | 240×135 | ega | 7.5 KB | 7641 | cfef9c8b7e51 | −51% |
| photo 1080p | 8 | 240×135 | vga | 12.7 KB | 13054 | a6a679caefb5 | −16% |
| photo 1080p | 8 | 240×135 | gen16 | 7.0 KB | 7118 | 61bd727f37ff | −54% |
| photo 4K | 16 | 240×135 | auto | 22.3 KB | 22847 | 06f306dfb293 | — |
| photo 4K | 16 | 240×135 | nes | 12.7 KB | 13011 | 91996f8e329a | −43% |
| photo 4K | 16 | 240×135 | gb | 5.3 KB | 5467 | 7658cf375489 | −76% |
| photo 4K | 16 | 240×135 | cga | 7.8 KB | 7990 | 260c7501af9c | −65% |
| photo 4K | 16 | 240×135 | ega | 11.1 KB | 11322 | b5ce4594b8f3 | −50% |
| photo 4K | 16 | 240×135 | vga | 17.4 KB | 17786 | ba89002eef36 | −22% |
| photo 4K | 16 | 240×135 | gen16 | 9.5 KB | 9752 | 3a3b5733e5c5 | −57% |
| photo 4K | 8 | 480×270 | auto | 58.7 KB | 60094 | 88cc240742f0 | — |
| photo 4K | 8 | 480×270 | nes | 32.3 KB | 33069 | b19906727f41 | −45% |
| photo 4K | 8 | 480×270 | gb | 12.9 KB | 13163 | d3f35038e8b5 | −78% |
| photo 4K | 8 | 480×270 | cga | 18.9 KB | 19390 | 0281810283dd | −68% |
| photo 4K | 8 | 480×270 | ega | 27.5 KB | 28181 | 5ff1dbbe236a | −53% |
| photo 4K | 8 | 480×270 | vga | 46.7 KB | 47858 | 7d5d32543a08 | −20% |
| photo 4K | 8 | 480×270 | gen16 | 26.6 KB | 27208 | e47c300fc73c | −55% |

**v0.1 baseline.** The script rebuilds the untouched v0.1 code in a
throwaway `git worktree` at `29ef2b4` (parent of the first S1 commit —
the same base the `tests/golden/v01/` byte-identity goldens were captured
from) and joins the tables. At fixture scale, as on the goldens, the v2
default path is **byte-identical** to v0.1, not merely equal in size:

```
photo 1080p @ 16: identical — 6430 B, blake3 597ffb135c19
photo 1080p @  8: identical — 15499 B, blake3 456d816cf0b3
photo 4K    @ 16: identical — 22847 B, blake3 06f306dfb293
photo 4K    @  8: identical — 60094 B, blake3 88cc240742f0
```

**Reading.** No named palette bloats a variant — every fixed table lands
16–78% *below* auto, because a small fixed PLTE means fewer index values
in the index stream (the plan's “NES on a photo” size risk did not
materialize; the worst named case, VGA, is still −16% vs auto). The v0.1
headline numbers (6.3 KB @ 1080p/16, 22.3 KB @ 4K/16) stand unchanged:
the default path did not move a single byte.

## Memory & fds

| state | RSS | fds |
|---|---|---|
| daemon idle (no output configured) | **4.4 MB** | 5 |
| displaying (2× 5120×2880 shm buffers) | ~121 MB | 7 after a 50-switch sweep |

**The steady-state wash, stated honestly.** While *displaying*, a
wallpaper daemon holds output-sized buffers — every wallpaper tool does,
including any that streams a 4K original to the compositor every frame.
Our win is not "smaller than everyone" at steady state; it's:
idle 4.4 MB, disk ~KB/wallpaper/preset, and switches that never touch the
original or re-decode more than a canvas. Compositor-side copies are
outside our accounting and theirs.

## Optimizations considered and skipped (with evidence)

- **Row/column-duplicated stretch** (plan §9.4): 4K stretch is 11.6 ms
  inside a 45 ms p95 hot switch. A 2–4× win on half a stage doesn't move
  the perceived number. Skipped per plan's evidence gate.
- **Parallel median-cut**: only matters for cold 4K@8 (105 ms one-time).
  Not worth a dependency; revisit if field reports disagree.

## Reproduce

```sh
cargo bench -p pixcore                    # criterion suites
cargo run --release -p pixcore --example variant_sizes
test/bench-dither.sh                      # v0.2 quantize/dither stage timings
test/bench-sizes.sh                       # v0.2 sizes vs v0.1 baseline (CPU)
WAYLAND_DISPLAY=… test/bench-daemon.sh target/release/pixpapers 25
pixpapers status --bench                  # daemon's own counters
```
