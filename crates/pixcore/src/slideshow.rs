//! The slideshow time model — a **pure, stateless function of the wall
//! clock**. This is the whole feature in one place.
//!
//! The daemon owns no slideshow state: *which* wallpaper is showing is
//! recomputed from `(playlist, now)` at every tick. `now` is injected so
//! tests drive a fake clock with no sleeps and no wall-clock assertions.
//!
//! The playlist is the manifest's `entries` order. The active slot is
//! `(now / interval) % len`, so the phase is aligned to the Unix epoch:
//! enabling mid-slot shows that slot's entry and the first change can be
//! less than a full interval away. There is no drift to accumulate and
//! nothing to persist — a restart lands on the same entry the clock implies
//! (like `cron`), because it is derived from the clock, not from a counter.

use crate::config::Config;
use crate::manifest::{Entry, Library};

/// Wall-clock now, unix seconds. `0` if the clock is before the epoch
/// (never in practice; keeps the pure fns infallible).
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Which playlist slot is showing at `now` (unix seconds). `None` when there
/// is nothing to show (`len == 0`) or the interval is degenerate (`0`), so
/// this never divides by zero.
pub fn active_index(now: u64, interval_secs: u64, len: usize) -> Option<usize> {
    if len == 0 || interval_secs == 0 {
        return None;
    }
    let slot = now / interval_secs;
    Some((slot % len as u64) as usize)
}

/// Whole seconds from `now` until the next slot boundary — always in
/// `1..=interval_secs`. Never `0`, so a poll loop that sleeps this long is
/// never busy-spinning; never longer than a full interval, so it wakes
/// promptly for every real change. Exactly `interval_secs` on a boundary
/// (that boundary's change is *now*; the next one is a full interval away).
pub fn secs_to_next_change(now: u64, interval_secs: u64) -> u64 {
    if interval_secs == 0 {
        return 1;
    }
    let rem = now % interval_secs;
    if rem == 0 {
        interval_secs
    } else {
        interval_secs - rem
    }
}

/// Unix second of the next slot boundary strictly after `now`, or `None` if
/// the interval is degenerate. Used to report "next change" to the user.
pub fn next_change_unix(now: u64, interval_secs: u64) -> Option<u64> {
    if interval_secs == 0 {
        return None;
    }
    Some((now / interval_secs + 1) * interval_secs)
}

/// The entry that should be on screen at `now`. With the slideshow enabled
/// this is the clock-selected slot; with it disabled it falls back to the
/// manifest's `current` (the manual `set`/`cycle` target), exactly as
/// before the feature existed.
pub fn active_entry<'a>(cfg: &Config, lib: &'a Library, now: u64) -> Option<&'a Entry> {
    if cfg.slideshow.enabled {
        let idx = active_index(now, cfg.slideshow.interval_secs, lib.entries.len())?;
        lib.entries.get(idx)
    } else {
        lib.entries
            .iter()
            .find(|e| lib.current.as_deref() == Some(e.id.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Settings;

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            title: id.into(),
            hash: id.into(),
            ext: "png".into(),
            added_unix: 0,
            width: 1,
            height: 1,
            original_bytes: 1,
            optimize_note: String::new(),
            settings: Settings::default(),
        }
    }

    fn lib_with(ids: &[&str]) -> Library {
        Library {
            version: crate::MANIFEST_VERSION,
            current: ids.first().map(|s| s.to_string()),
            entries: ids.iter().map(|id| entry(id)).collect(),
        }
    }

    #[test]
    fn slot_advances_then_wraps_in_order() {
        // The core property: across the wrap the ORDER is preserved —
        // 0,1,2,0,1,2 — never skipped, never a partial cycle.
        let seq: Vec<_> = (0..6)
            .map(|i| active_index(i * 10, 10, 3).unwrap())
            .collect();
        assert_eq!(seq, vec![0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn wrap_boundary_is_the_exact_first_second_of_next_slot() {
        // interval 10, len 3: [0,10) -> 0, [10,20) -> 1, [20,30) -> 2, [30,..) -> 0.
        assert_eq!(active_index(9, 10, 3), Some(0));
        assert_eq!(active_index(10, 10, 3), Some(1));
        assert_eq!(active_index(19, 10, 3), Some(1));
        assert_eq!(active_index(20, 10, 3), Some(2));
        assert_eq!(active_index(29, 10, 3), Some(2));
        assert_eq!(active_index(30, 10, 3), Some(0)); // the wrap lands on 0
    }

    #[test]
    fn degenerate_inputs_are_none_not_panic() {
        assert_eq!(active_index(0, 10, 0), None); // empty playlist
        assert_eq!(active_index(0, 0, 3), None); // zero interval -> no div by zero
    }

    #[test]
    fn single_entry_always_slot_zero() {
        for t in [0, 7, 100, 999] {
            assert_eq!(active_index(t, 10, 1), Some(0));
        }
    }

    #[test]
    fn secs_to_next_change_is_one_through_interval() {
        // never 0 (no busy-spin), never > interval.
        assert_eq!(secs_to_next_change(0, 10), 10); // on a boundary
        assert_eq!(secs_to_next_change(1, 10), 9);
        assert_eq!(secs_to_next_change(9, 10), 1); // the tightest wake-up
        assert_eq!(secs_to_next_change(10, 10), 10);
        assert_eq!(secs_to_next_change(59, 10), 1);
        assert_eq!(secs_to_next_change(0, 0), 1); // degenerate, but still >= 1
    }

    #[test]
    fn next_change_is_strictly_future_and_aligned() {
        assert_eq!(next_change_unix(0, 10), Some(10));
        assert_eq!(next_change_unix(3, 10), Some(10));
        assert_eq!(next_change_unix(10, 10), Some(20)); // a boundary's *next* is a full interval on
        assert_eq!(next_change_unix(0, 0), None);
        for now in 0..100 {
            let n = next_change_unix(now, 7).unwrap();
            assert!(n > now, "next change {n} must be after {now}");
            assert_eq!(n % 7, 0, "next change {n} must be a slot boundary");
        }
    }

    #[test]
    fn active_entry_follows_clock_when_enabled_and_current_when_off() {
        let lib = lib_with(&["zero", "one", "two"]);

        // Off: shows `current` (== "zero" here), regardless of the clock.
        let off = Config::default();
        assert!(!off.slideshow.enabled);
        assert_eq!(active_entry(&off, &lib, 12345).unwrap().id, "zero");

        // On: shows the clock slot, ignoring `current`.
        let on = Config {
            slideshow: crate::config::Slideshow {
                enabled: true,
                interval_secs: 10,
            },
            ..Default::default()
        };
        assert_eq!(active_entry(&on, &lib, 0).unwrap().id, "zero");
        assert_eq!(active_entry(&on, &lib, 10).unwrap().id, "one");
        assert_eq!(active_entry(&on, &lib, 20).unwrap().id, "two");
        assert_eq!(active_entry(&on, &lib, 30).unwrap().id, "zero"); // wrap
    }

    #[test]
    fn active_entry_is_none_for_empty_library() {
        let lib = lib_with(&[]);
        assert_eq!(
            active_entry(&Config::default(), &lib, 5).map(|e| e.id.as_str()),
            None
        );
        let on = Config {
            slideshow: crate::config::Slideshow {
                enabled: true,
                interval_secs: 10,
            },
            ..Default::default()
        };
        assert_eq!(active_entry(&on, &lib, 5).map(|e| e.id.as_str()), None);
    }
}
