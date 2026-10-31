//! Preview canvas math for the TUI (PLAN-30 §C.3) — pure, unit-tested.
//!
//! The TUI's preview pane is a *small export*: the exact pipeline the
//! daemon runs (`variant::generate` + `render::composite`), rendered at a
//! canvas size derived from the pane's cell grid. Everything here is
//! deterministic and headless (invariant #5).

/// Inline-preview PNG budget: an encoded preview larger than this makes
/// the TUI refuse the image and show the text fallback instead (PLAN-30
/// §C.3 — keeps a 25-column SSH terminal from exploding). The number
/// comes from the A.4 size bench: preview canvases (pane-sized, ≤ ~4 KB
/// for the quantized presets) sit far below it, while a `normal`-preset
/// photo on a large terminal can pass it.
pub const PREVIEW_PNG_MAX_BYTES: u64 = 256 * 1024;

/// Does this encoded preview exceed the inline budget? The boundary is
/// inclusive: exactly [`PREVIEW_PNG_MAX_BYTES`] passes, one byte more
/// refuses (unit-tested).
pub fn png_over_budget(bytes: &[u8]) -> bool {
    (bytes.len() as u64) > PREVIEW_PNG_MAX_BYTES
}

/// Canvas size (px) for a preview pane of `cells_w × cells_h` cells.
///
/// The terminal's cell aspect is not queryable, so each cell is assumed
/// **2 px wide × 1 px tall** — the common ~2:1 cell aspect (documented,
/// not measured; a 60×30 pane ⇒ a 120×30 canvas, i.e. 4:1 — the preview
/// is a *look* check, not a pixel-true miniature, and `fit` letterboxing
/// keeps the visible image inside either assumption).
///
/// Degenerate panes (0 cells on any axis — very small terminals after
/// the borders are subtracted) clamp to 1×1: the preview is always a
/// non-empty image, and whether the pane can actually display it is the
/// caller's call.
pub fn preview_canvas(cells_w: u32, cells_h: u32) -> (u32, u32) {
    (cells_w.saturating_mul(2).max(1), cells_h.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_is_2x1_per_cell() {
        // the plan's documented assumption
        assert_eq!(preview_canvas(60, 30), (120, 30));
        assert_eq!(preview_canvas(50, 25), (100, 25));
        assert_eq!(preview_canvas(1, 1), (2, 1));
        assert_eq!(preview_canvas(10, 1), (20, 1));
    }

    #[test]
    fn degenerate_panes_clamp_to_1x1() {
        // S6 acceptance: degenerate panes (3×3 cells → 6×3; zero axes → 1×1)
        assert_eq!(preview_canvas(3, 3), (6, 3));
        assert_eq!(preview_canvas(0, 3), (1, 3));
        assert_eq!(preview_canvas(3, 0), (6, 1));
        assert_eq!(preview_canvas(0, 0), (1, 1));
        // no overflow: a u32::MAX-wide pane stays finite
        assert!(preview_canvas(u32::MAX, 1).0 > 0);
    }

    #[test]
    fn guard_boundary() {
        // S6 acceptance: the 256 KB boundary — inclusive below, exclusive above
        assert_eq!(PREVIEW_PNG_MAX_BYTES, 256 * 1024);
        assert!(!png_over_budget(&vec![
            0;
            (PREVIEW_PNG_MAX_BYTES - 1) as usize
        ]));
        assert!(!png_over_budget(&vec![0; PREVIEW_PNG_MAX_BYTES as usize])); // == passes
        assert!(png_over_budget(&vec![
            0;
            PREVIEW_PNG_MAX_BYTES as usize + 1
        ]));
        assert!(!png_over_budget(&[]));
    }
}
