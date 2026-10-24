use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{PixError, Result};

/// CSS-style color (`#rgb` or `#rrggbb`), serialized lowercase `#rrggbb`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RgbCss(pub [u8; 3]);

impl RgbCss {
    pub const BLACK: RgbCss = RgbCss([0, 0, 0]);

    /// Value for an `Xrgb8888` / `Argb8888` little-endian buffer word.
    pub fn to_buffer_word(self) -> u32 {
        let [r, g, b] = self.0;
        0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }
}

impl fmt::Display for RgbCss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

impl FromStr for RgbCss {
    type Err = PixError;
    fn from_str(s: &str) -> Result<Self> {
        let t = s.trim().to_ascii_lowercase();
        let hex = t
            .strip_prefix('#')
            .ok_or_else(|| PixError::Invalid(format!("color must start with '#' (got {s:?})")))?;
        let decode = |h: &str| -> Option<u8> {
            if h.len() != 2 {
                return None;
            }
            u8::from_str_radix(h, 16).ok()
        };
        if hex.len() == 3 {
            let chars: Vec<char> = hex.chars().collect();
            let exp = |c: char| decode(&c.to_string().repeat(2));
            if let (Some(r), Some(g), Some(b)) = (exp(chars[0]), exp(chars[1]), exp(chars[2])) {
                return Ok(RgbCss([r, g, b]));
            }
        } else if hex.len() == 6 {
            if let (Some(r), Some(g), Some(b)) =
                (decode(&hex[0..2]), decode(&hex[2..4]), decode(&hex[4..6]))
            {
                return Ok(RgbCss([r, g, b]));
            }
        }
        Err(PixError::Invalid(format!(
            "color must be #rgb or #rrggbb (got {s:?})"
        )))
    }
}

impl Serialize for RgbCss {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for RgbCss {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays() {
        assert_eq!("#001122".parse::<RgbCss>().unwrap().0, [0x00, 0x11, 0x22]);
        assert_eq!("#ABC".parse::<RgbCss>().unwrap().0, [0xAA, 0xBB, 0xCC]);
        assert_eq!(RgbCss([0x0a, 0x0b, 0x0c]).to_string(), "#0a0b0c");
        assert!("red".parse::<RgbCss>().is_err());
        assert!("#12".parse::<RgbCss>().is_err());
        assert!("#12345".parse::<RgbCss>().is_err());
    }

    #[test]
    fn buffer_word_is_opaque() {
        assert_eq!(RgbCss([0x11, 0x22, 0x33]).to_buffer_word(), 0xFF11_2233);
    }

    #[test]
    fn serde_round_trip() {
        let c = RgbCss([1, 2, 3]);
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(j, "\"#010203\"");
        assert_eq!(serde_json::from_str::<RgbCss>(&j).unwrap(), c);
    }
}
