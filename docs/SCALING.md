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

## Fractional scale — exact where offered, oversampled as fallback

On a compositor without fractional-scale (most of them), legacy clients
get `wl_output.scale = 2` for a 1.5 panel (compositor-reported) and
logical 2560×1440, so the fallback buffer is **5120×2880 — 1.78× the
panel**. niri downscales it to fit the real output. The result is
visually crisp at preset sizes (downsampling a blocky image keeps
blocks; only block edges soften slightly — invisible above 8px blocks,
near-invisible at 8), but it costs extra memory/CPU: ~121 MB RSS
measured on the dev box, most of it the two oversampled buffers.
Where the compositor offers viewporter + fractional-scale, the section
below replaces this with the exact physical buffer.

This fallback is **correct at every scale tested** (effective 1.0 and 1.5
live; legacy scale events 1 and 2 — V6/V7); it's merely not *optimal* at
fractional scales.

## Exact fractional buffers — shipped ([Unreleased])

The daemon now binds `wp_viewporter` (stable) + `wp_fractional_scale_v1`
(staging) when the compositor offers **both**, and when the negotiated
`preferred_scale` is non-integer, sizes buffers as
`round_half_away(logical × frac)` with the viewporter destination set to
the logical size and buffer scale 1 — the buffer **is** the physical
pixel grid: crisp 1:1 blocks, no compositor resample of an oversampled
buffer, lower RSS. Rounding is nearest, halves away from zero (the
protocol's own rule for surface sizes), in exact integer math.

The legacy integer path above stays the fallback and is the code path
whenever a protocol is missing or the scale is integer — behavior at
1.0/2.0 is byte-for-byte unchanged (the legacy geometry formula is
untouched; a viewport whose destination was never set is inert).

**Proven in a headless-sway container** (`test/verify-wayland-fractional.sh`,
sway 1.12 on `WLR_BACKENDS=headless`, numbers in VERIFICATION.md): at
3840×2160 @ scale 1.5 the daemon reports `scale_path: "exact"`,
`frac_scale: "1.50"` and a **3840×2160 buffer** (logical 2560×1440 × 1.5)
— the panel grid, not the legacy 5120×2880 oversample; at 1.0 and 2.0 it
reports `legacy` with the unchanged buffers; the pre-feature binary at
the same virtual output oversamples to 5120×2880 and uses **120,504 kB
RSS vs the exact path's 70,128 kB**. Variant cache keys stayed concrete
(`v2-16-fit-3840x2160-auto.png`) and stable across statuses — path
selection is a pure function of (globals, negotiated scale), so a settled
output cannot flip paths between renders.

**Awaiting the operator's live panel** (niri ≥ 26.04 exposes both
protocols): the same 3840×2160 @ 1.5 case on real hardware, confirming
`status --json` shows `scale_path: "exact"` and RSS drops from the
measured ~121 MB toward ~70 MB. Copy-pasteable check in VERIFICATION.md.

## Multi-output

Each output is independent: its own surfaces, its own double-buffered
pair, its own configure→render. Variant canvases are keyed by output
buffer size, so a 4K + 1080p mix caches two variants per
(preset, placement) and switches both in one apply. **Structurally
handled, untested on real hardware** (single-output dev machine — see
VERIFICATION.md V4/V5). Multi-monitor bug reports are the fastest way to
change that.
