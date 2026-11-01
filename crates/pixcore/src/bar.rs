//! The one-line bar label for the waybar module (workstream D, S8).
//! Pure formatting — invariant #5 keeps it here, wayland-free. The
//! manifest read and the daemon liveness probe stay in the `pixpapers`
//! client, which hands in the resolved pieces.
//!
//! Contract: `pixpapers bar` prints exactly one line and exits 0, always.
//! Waybar renders line 1 of a custom module's output and *hides* the
//! module on empty output or a non-zero exit — a multi-line or failing
//! label would make the button blink.

use crate::render::ResolvedSettings;

/// `▞ <title> · <pixel>/<placement-short>` — e.g. `▞ nebula · 16/fit`.
/// Placement uses the short tokens (`fit`, `gutter`, `crop`, `stretch`)
/// so the label stays narrow at toolbar heights.
pub fn entry_line(title: &str, r: &ResolvedSettings) -> String {
    format!("▞ {title} · {}/{}", r.pixel, r.placement.short())
}

/// The full bar line for the three states (PLAN-30 D.1).
///
/// Precedence: a current entry wins even when the daemon is down — the
/// label is config+manifest truth (the exact resolution the daemon uses,
/// so a recorded wallpaper keeps its name offline). The liveness probe
/// only disambiguates the no-entry case: `set one` when a daemon can
/// apply a set, `daemon down` when it cannot.
pub fn bar_label(current: Option<(&str, &ResolvedSettings)>, daemon_alive: bool) -> String {
    match current {
        Some((title, r)) => entry_line(title, r),
        None if daemon_alive => "▞ pixpapers — set one".to_string(),
        None => "▞ pixpapers · daemon down".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::manifest::Settings;
    use crate::placement::Placement;
    use crate::preset::Pixel;

    fn resolved(pixel: Pixel, placement: Placement) -> ResolvedSettings {
        ResolvedSettings::resolve(
            &Settings {
                pixel: Some(pixel),
                placement: Some(placement),
                ..Default::default()
            },
            &Config::default(),
        )
    }

    #[test]
    fn entry_line_shape_matches_plan_example() {
        let r = resolved(Pixel::B16, Placement::Fit);
        assert_eq!(entry_line("nebula", &r), "▞ nebula · 16/fit");
    }

    #[test]
    fn every_placement_uses_its_short_token() {
        let cases = [
            (Placement::Fit, "fit"),
            (Placement::CoverGutter, "gutter"),
            (Placement::CoverCrop, "crop"),
            (Placement::Stretch, "stretch"),
        ];
        for (p, tok) in cases {
            let r = resolved(Pixel::B32, p);
            assert_eq!(entry_line("w", &r), format!("▞ w · 32/{tok}"));
        }
    }

    #[test]
    fn unpixelated_entry_says_normal() {
        let r = resolved(Pixel::B16, Placement::Fit).unpixelated();
        assert_eq!(entry_line("w", &r), "▞ w · normal/fit");
    }

    #[test]
    fn auto_entry_says_auto() {
        // The bar has no output size to resolve `auto` against, so it
        // honestly shows the setting; the daemon's per-output render
        // label is where the resolved preset shows up.
        let r = resolved(Pixel::Auto, Placement::Fit);
        assert_eq!(entry_line("w", &r), "▞ w · auto/fit");
    }

    #[test]
    fn no_entry_alive_is_set_one() {
        assert_eq!(bar_label(None, true), "▞ pixpapers — set one");
    }

    #[test]
    fn no_entry_dead_is_daemon_down() {
        assert_eq!(bar_label(None, false), "▞ pixpapers · daemon down");
    }

    #[test]
    fn current_entry_wins_over_dead_daemon() {
        let r = resolved(Pixel::B8, Placement::CoverCrop);
        assert_eq!(bar_label(Some(("a", &r)), false), "▞ a · 8/crop");
    }
}
