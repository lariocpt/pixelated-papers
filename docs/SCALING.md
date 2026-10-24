# Scaling & buffer sizing

## The contract

Layer-shell configure sizes are **logical**; buffers are physical. For an
output niri gives us as *logical W×H with legacy `wl_output.scale = S`*,
the daemon allocates a **(W·S) × (H·S)** XRGB buffer and sets
`wl_surface.set_buffer_scale(S)`. The composite (`view` computed from the
**original** image, nearest stretch of the variant into `view.dest`) runs
in physical pixels, so a 16px block is 16 physical pixels, crisp.

## Integer scale — exact

On an output at scale 1 or 2 the buffer is exactly the panel size:
pixel-perfect, no compositor resampling. Verified at legacy scale 1
(effective 1.0, buffer 3840×2160) during the V6 restore.

## Fractional scale — correct, slightly oversampled (the honest bit)

The dev session runs 3840×2160 at niri **scale 1.5**. Legacy clients get
`wl_output.scale = 2` (compositor-reported) and logical 2560×1440, so our
buffer is **5120×2880 — 1.78× the panel**. niri downscales it to fit the
real output. The result is visually crisp at preset sizes (downsampling a
blocky image keeps blocks; only block edges soften slightly — invisible
above 8px blocks, near-invisible at 8), but it costs extra memory/CPU:
~121 MB RSS showing on this box, most of it the two oversampled buffers.

This is **correct at every scale tested** (effective 1.0 and 1.5 live;
legacy scale events 1 and 2 — V6/V7); it's merely not *optimal* at
fractional scales.

## v0.2: exact fractional buffers

Plan: bind `wp_viewporter` + `wp_fractional_scale_v1` when present, size
buffers as `round(logical × frac_scale)`, keep the legacy integer path as
fallback (compositors without fractional scale — most of them — keep
working unchanged). Acceptance: same visual at 1.5 with a 3840×2160
buffer on the dev panel; RSS drop to ~70 MB confirms it landed.

## Multi-output

Each output is independent: its own surfaces, its own double-buffered
pair, its own configure→render. Variant canvases are keyed by output
buffer size, so a 4K + 1080p mix caches two variants per
(preset, placement) and switches both in one apply. **Structurally
handled, untested on real hardware** (single-output dev machine — see
VERIFICATION.md V4/V5). Multi-monitor bug reports are the fastest way to
change that.
