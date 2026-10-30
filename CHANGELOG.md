# Changelog

All notable changes. Format follows Keep a Changelog; this project follows
Semantic Versioning. The build log behind each area lives in
[docs/plan/](docs/plan/).

## [0.1.0] - 2026-10-24

First release. Forced-pixelation wallpapers (presets `8/16/32/64/128/
normal`, default 16) on a wlr-layer-shell Background surface, niri-first.

### Added
- **pixcore** (no-wayland core): atomic TOML config + versioned JSON
  manifest; blake3-deduped library with lossless-optimized originals;
  iterative-box downscale + nearest composite + median-cut palette
  quantization; placement math (`fit`, `cover-gutter`, `cover-crop`,
  `stretch`) with proptest coverage; on-disk variant cache keyed by
  (id, preset, placement, gutter, output size).
- **CLI**: `add/list/remove/set/cycle/enable/disable`, per-wallpaper
  `pixel/placement/bg/gutter`, `default pixel|placement`, `status
  [--json|--bench]`, offline `export` (exact daemon composite path),
  `optimize`, `completions`.
- **Daemon**: single-threaded poll loop (wayland + IPC + signals); one
  double-buffered shm Background surface per output; auto-spawn from any
  mutating command; IPC JSON-lines (`Apply/Status/Quit`); graceful IPC
  `stop`; exit 43 on compositor loss.
- `pixpapers status --bench`: daemon-self-timed switch/render latencies,
  variant-cache hit rate, RSS/fd — zero cost when idle.
- Evidence tooling: criterion benches, `variant_sizes` example,
  `test/smoke.sh` (offline+live), `test/verify-live.sh` (V11/V13/V16/V17),
  `test/bench-daemon.sh`; docs/PERF.md, docs/VERIFICATION.md.
- v1.0 backlog documented in README ("Deliberately not in v0.1").

### Fixed (found during step-8 verification)
- IPC clients that sent no newline stalled the daemon loop (read timeout
  per client); daemon now answers EOF/empty requests immediately (V16).
- SIGTERM/SIGINT silently never reached the shutdown flag through
  signal-hook on the dev machine; replaced with a raw async-signal-safe
  sigaction handler and one fewer dependency (V17).
- Layer surfaces now carry an empty input region (belt on lax
  compositors, V9).

### Known limitations (v0.1, documented)
- Multi-output & physical hotplug: structurally handled, hardware-untested.
- Fractional scale renders via the legacy oversampled path (exact-size
  buffers via viewporter/fractional-scale = v0.2).
- `stop`/daemon must be re-run per session (exit 43 on session end is
  intentional).
- 72 h soak (V15) pending.

## [Unreleased]
- v0.2 targets: wp_viewporter + wp_fractional_scale_v1 exact buffers;
  headless-sway CI smoke; multi-monitor hardening from field reports.
- pixcore: quantize stage — palette modes `auto / nes / gb / cga / ega /
  vga / gen16` × dithers `none / fs / atkinson / bayer2 / bayer4 / bayer8`,
  deterministic integer pipeline (12 golden cells checked in).
  Config `palette` is now a string (`auto` default; legacy `true`/`false`
  TOML bools still load) and config/manifest gain `dither`; variant cache
  keys are v2 (v0.1 caches invalidate — they are disposable). `auto` +
  `none` renders **byte-identical to v0.1**, verified against goldens
  captured from the pre-change tree.
