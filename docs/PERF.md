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
  quantized ≈ 8× for photos, 1× for well-compressed art). It exists to
  not lie about pixelation, not to save bytes.

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
WAYLAND_DISPLAY=… test/bench-daemon.sh target/release/pixpapers 25
pixpapers status --bench                  # daemon's own counters
```
