//! Image emission (PLAN-30 §C.3): the protocol writers + a sink so the
//! golden tests capture bytes instead of touching a terminal.
//!
//! Wire formats (verified against upstream sources, see commit message):
//! - **kitty**: graphics APC `ESC_G key=value,…;base64(payload) ESC\`.
//!   `a=T` transmit-and-display, `f=100` PNG, `i=<id>` client id,
//!   `c`/`r` scale-to-fit a cell box, `C=1` keep the cursor still,
//!   `q=2` silent. Re-render = `a=d,i=<old>` delete, then display anew.
//!   (The plan's "OSC 6337" was folklore — kitty has used APC `G` since
//!   0.4.0; verified against kitty v0.4.0 → master.)
//! - **iTerm2**: `ESC]1337;File=inline=1;base64;<b64> BEL`.
//! - **sixel**: the `pixcore::sixel` stream, pre-resized to pane px.
//!
//! Cursor safety: every frame's first write is an absolute `MoveTo`
//! (ratatui 0.30 `draw()` semantics), so emitting between frames after
//! moving to the anchor cannot desync the renderer — no restore needed.

use std::io::Write;

/// Where emitted bytes go: the live terminal, or a test buffer.
pub enum Sink {
    Live,
    /// Constructed only by the golden/emit tests (C4) — never in a build.
    #[allow(dead_code)]
    Test(Vec<u8>),
}

impl Sink {
    fn write_all(&mut self, bytes: &[u8]) {
        match self {
            Sink::Live => {
                let mut out = std::io::stdout();
                let _ = out.write_all(bytes);
                let _ = out.flush();
            }
            Sink::Test(buf) => buf.extend_from_slice(bytes),
        }
    }

    #[cfg(test)]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Sink::Test(b) => b,
            Sink::Live => &[],
        }
    }
}

/// Move the live cursor to the pane anchor before an emission (test
/// sinks record nothing — the golden captures layout, not cursor moves).
pub fn move_to(sink: &mut Sink, x: u16, y: u16) {
    match sink {
        Sink::Live => {
            use crossterm::QueueableCommand as _;
            let mut out = std::io::stdout();
            let _ = out.queue(crossterm::cursor::MoveTo(x, y));
        }
        Sink::Test(_) => {}
    }
}

/// Delete the previously displayed kitty image (no-op for other protos).
pub fn clear_prev(sink: &mut Sink, proto: super::detect::ImageProto, prev_id: u32) {
    if proto == super::detect::ImageProto::Kitty && prev_id > 0 {
        sink.write_all(format!("\x1b_Ga=d,i={prev_id},q=2\x1b\\").as_bytes());
    }
}

/// Emit the preview PNG at the anchor, scaled to `cols × rows` cells
/// (kitty) or at native px (iTerm2/sixel — the canvas is already pane
/// px under the documented 2:1 cell assumption). Returns the kitty
/// image id to delete before the next render (0 for other protos).
pub fn emit(
    sink: &mut Sink,
    proto: super::detect::ImageProto,
    png: &[u8],
    id: u32,
    cols: u16,
    rows: u16,
) {
    match proto {
        super::detect::ImageProto::None => {}
        super::detect::ImageProto::Kitty => {
            let b64 = base64(png);
            sink.write_all(
                format!("\x1b_Ga=T,f=100,i={id},c={cols},r={rows},C=1,q=2;{b64}\x1b\\").as_bytes(),
            );
        }
        super::detect::ImageProto::Iterm2 => {
            let b64 = base64(png);
            sink.write_all(format!("\x1b]1337;File=inline=1;base64;{b64}\x07").as_bytes());
        }
        super::detect::ImageProto::Sixel => {
            if let Ok(img) = pixcore::variant::decode_any(png) {
                sink.write_all(&pixcore::sixel::encode(&img));
            }
        }
    }
}

/// Standard base64 with padding (RFC 4648), hand-rolled: one less
/// dependency, and the TUI is the only consumer (dependency discipline).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        s.push(T[(n >> 18 & 63) as usize] as char);
        s.push(T[(n >> 12 & 63) as usize] as char);
        s.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        s.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, want) in cases {
            assert_eq!(base64(input.as_bytes()), want, "input {input:?}");
        }
        // full byte range round-trips through the image decoder's base64
        // counterpart only in a live terminal; pin the alphabet instead:
        assert_eq!(base64(&[0xff, 0xff, 0xff]), "////");
        assert_eq!(base64(&[0x00]), "AA==");
    }

    #[test]
    fn kitty_sequence_shape() {
        let mut sink = Sink::Test(Vec::new());
        emit(
            &mut sink,
            super::super::detect::ImageProto::Kitty,
            b"PNG",
            3,
            40,
            10,
        );
        let s = String::from_utf8(sink.bytes().to_vec()).unwrap();
        assert_eq!(s, "\x1b_Ga=T,f=100,i=3,c=40,r=10,C=1,q=2;UE5H\x1b\\");
        // delete-previous for the next render
        let mut sink2 = Sink::Test(Vec::new());
        clear_prev(&mut sink2, super::super::detect::ImageProto::Kitty, 3);
        assert_eq!(
            String::from_utf8(sink2.bytes().to_vec()).unwrap(),
            "\x1b_Ga=d,i=3,q=2\x1b\\"
        );
        // id 0 = nothing to delete
        let mut sink3 = Sink::Test(Vec::new());
        clear_prev(&mut sink3, super::super::detect::ImageProto::Kitty, 0);
        assert!(sink3.bytes().is_empty());
    }

    #[test]
    fn iterm2_sequence_shape() {
        let mut sink = Sink::Test(Vec::new());
        emit(
            &mut sink,
            super::super::detect::ImageProto::Iterm2,
            b"PNG",
            0,
            0,
            0,
        );
        assert_eq!(
            String::from_utf8(sink.bytes().to_vec()).unwrap(),
            "\x1b]1337;File=inline=1;base64;UE5H\x07"
        );
    }
}
