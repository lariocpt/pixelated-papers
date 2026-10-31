//! Terminal image-protocol detection (PLAN-30 §C.3). Startup + env
//! override; the result is shown in the status bar (`img:kitty` …),
//! never guessed silently.
//!
//! Detection table (as implemented — see the S6 note in PLAN-30 §C.3):
//! | signal                                   | protocol                       |
//! |------------------------------------------|--------------------------------|
//! | `KITTY_WINDOW_ID` / `TERM_PROGRAM=kitty` | kitty graphics APC `ESC_G`     |
//! | `ITERM_SESSION_ID`                       | iTerm2 OSC 1337 `File=inline`  |
//! | `SIXEL` set / `$TERM` contains `sixel`   | sixel `ESC P q … ESC \\`       |
//! | anything else                            | text fallback                  |
//!
//! `PIX_PAPERS_IMAGE = none|kitty|iterm2|sixel` wins over everything
//! (also the test lever and the escape hatch for false positives).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageProto {
    None,
    Kitty,
    Iterm2,
    Sixel,
}

impl ImageProto {
    pub fn token(self) -> &'static str {
        match self {
            ImageProto::None => "none",
            ImageProto::Kitty => "kitty",
            ImageProto::Iterm2 => "iterm2",
            ImageProto::Sixel => "sixel",
        }
    }
}

/// Pure detection over an env lookup — the unit-testable core (the
/// golden tests drive every row through here via `PIX_PAPERS_IMAGE`).
pub fn detect_from_env(get: impl Fn(&str) -> Option<String>) -> ImageProto {
    if let Some(v) = get("PIX_PAPERS_IMAGE") {
        return match v.trim().to_ascii_lowercase().as_str() {
            "kitty" => ImageProto::Kitty,
            "iterm2" => ImageProto::Iterm2,
            "sixel" => ImageProto::Sixel,
            // "none" and anything unrecognized: honest text fallback
            _ => ImageProto::None,
        };
    }
    if get("KITTY_WINDOW_ID").is_some() || get("TERM_PROGRAM").as_deref() == Some("kitty") {
        return ImageProto::Kitty;
    }
    if get("ITERM_SESSION_ID").is_some() {
        return ImageProto::Iterm2;
    }
    if get("SIXEL").is_some() || get("TERM").is_some_and(|t| t.contains("sixel")) {
        return ImageProto::Sixel;
    }
    ImageProto::None
}

/// Detection against the live process environment.
pub fn detect() -> ImageProto {
    detect_from_env(|k| std::env::var(k).ok())
}

/// Convenience for tests: an env map as the lookup.
#[cfg(test)]
pub fn detect_from_map(env: &std::collections::HashMap<String, String>) -> ImageProto {
    detect_from_env(|k| env.get(k).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn detection_table_rows() {
        // kitty: either signal
        assert_eq!(
            detect_from_map(&env(&[("KITTY_WINDOW_ID", "7")])),
            ImageProto::Kitty
        );
        assert_eq!(
            detect_from_map(&env(&[("TERM_PROGRAM", "kitty")])),
            ImageProto::Kitty
        );
        // iterm2
        assert_eq!(
            detect_from_map(&env(&[("ITERM_SESSION_ID", "w0t0p0:ABC")])),
            ImageProto::Iterm2
        );
        // sixel: env flag or $TERM substring
        assert_eq!(detect_from_map(&env(&[("SIXEL", "1")])), ImageProto::Sixel);
        assert_eq!(
            detect_from_map(&env(&[("TERM", "xterm-sixel")])),
            ImageProto::Sixel
        );
        // none: a plain terminal
        assert_eq!(
            detect_from_map(&env(&[("TERM", "xterm-256color")])),
            ImageProto::None
        );
        assert_eq!(detect_from_map(&env(&[])), ImageProto::None);
    }

    #[test]
    fn override_wins_and_is_the_test_lever() {
        // override beats every signal, including a kitty env
        for want in ["none", "kitty", "iterm2", "sixel"] {
            let e = env(&[
                ("PIX_PAPERS_IMAGE", want),
                ("KITTY_WINDOW_ID", "7"),
                ("ITERM_SESSION_ID", "x"),
                ("SIXEL", "1"),
            ]);
            assert_eq!(detect_from_map(&e).token(), want);
        }
        // unknown override values fall back to text (never a guess)
        assert_eq!(
            detect_from_map(&env(&[("PIX_PAPERS_IMAGE", "weird")])).token(),
            "none"
        );
    }

    #[test]
    fn precedence_kitty_then_iterm_then_sixel() {
        // all signals at once: kitty wins (table order)
        let all = env(&[
            ("KITTY_WINDOW_ID", "1"),
            ("ITERM_SESSION_ID", "1"),
            ("SIXEL", "1"),
        ]);
        assert_eq!(detect_from_map(&all), ImageProto::Kitty);
        // iterm + sixel: iterm wins
        assert_eq!(
            detect_from_map(&env(&[("ITERM_SESSION_ID", "1"), ("SIXEL", "1")])),
            ImageProto::Iterm2
        );
    }
}
