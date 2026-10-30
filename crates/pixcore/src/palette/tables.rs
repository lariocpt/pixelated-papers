//! Static palette tables for the named [`PaletteMode`]s.
//!
//! Every table is documented with its provenance. The values are canonical
//! hardware/BIOS tables, transcribed mechanically from the cited references
//! and pinned by the unit tests in this module — do not "tweak" them.

use image::Rgba;

/// Game Boy (DMG) four greens, stored dark→light so palette index order
/// matches luminance order: `0F380F`, `306230`, `8BAC0F`, `9BBC0F` — the
/// canonical greens captured off a DMG screen.
pub static GB: [Rgba<u8>; 4] = [
    Rgba([0x0F, 0x38, 0x0F, 255]),
    Rgba([0x30, 0x62, 0x30, 255]),
    Rgba([0x8B, 0xAC, 0x0F, 255]),
    Rgba([0x9B, 0xBC, 0x0F, 255]),
];

/// IBM CGA 16-colour mode 0E/D/E set (RGBI: 2 levels per gun, brown instead
/// of dark yellow at index 6). Identical to the first 16 entries of [`VGA`].
pub static CGA: [Rgba<u8>; 16] = [
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0xAA, 255]),
    Rgba([0x00, 0xAA, 0x00, 255]),
    Rgba([0x00, 0xAA, 0xAA, 255]),
    Rgba([0xAA, 0x00, 0x00, 255]),
    Rgba([0xAA, 0x00, 0xAA, 255]),
    Rgba([0xAA, 0x55, 0x00, 255]),
    Rgba([0xAA, 0xAA, 0xAA, 255]),
    Rgba([0x55, 0x55, 0x55, 255]),
    Rgba([0x55, 0x55, 0xFF, 255]),
    Rgba([0x55, 0xFF, 0x55, 255]),
    Rgba([0x55, 0xFF, 0xFF, 255]),
    Rgba([0xFF, 0x55, 0x55, 255]),
    Rgba([0xFF, 0x55, 0xFF, 255]),
    Rgba([0xFF, 0xFF, 0x55, 255]),
    Rgba([0xFF, 0xFF, 0xFF, 255]),
];

/// EGA 64-colour set — the classic 4×4×4 cube: two digital bits per gun,
/// levels `00`/`55`/`AA`/`FF` (the 6-bit `0RRGGBB0` DAC encoding of the EGA
/// and VGA 64-colour text/graph modes). Index = `R·16 + G·4 + B` over the
/// four levels; entry 0 is black, entry 63 white.
pub static EGA: [Rgba<u8>; 64] = [
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x55, 255]),
    Rgba([0x00, 0x00, 0xAA, 255]),
    Rgba([0x00, 0x00, 0xFF, 255]),
    Rgba([0x00, 0x55, 0x00, 255]),
    Rgba([0x00, 0x55, 0x55, 255]),
    Rgba([0x00, 0x55, 0xAA, 255]),
    Rgba([0x00, 0x55, 0xFF, 255]),
    Rgba([0x00, 0xAA, 0x00, 255]),
    Rgba([0x00, 0xAA, 0x55, 255]),
    Rgba([0x00, 0xAA, 0xAA, 255]),
    Rgba([0x00, 0xAA, 0xFF, 255]),
    Rgba([0x00, 0xFF, 0x00, 255]),
    Rgba([0x00, 0xFF, 0x55, 255]),
    Rgba([0x00, 0xFF, 0xAA, 255]),
    Rgba([0x00, 0xFF, 0xFF, 255]),
    Rgba([0x55, 0x00, 0x00, 255]),
    Rgba([0x55, 0x00, 0x55, 255]),
    Rgba([0x55, 0x00, 0xAA, 255]),
    Rgba([0x55, 0x00, 0xFF, 255]),
    Rgba([0x55, 0x55, 0x00, 255]),
    Rgba([0x55, 0x55, 0x55, 255]),
    Rgba([0x55, 0x55, 0xAA, 255]),
    Rgba([0x55, 0x55, 0xFF, 255]),
    Rgba([0x55, 0xAA, 0x00, 255]),
    Rgba([0x55, 0xAA, 0x55, 255]),
    Rgba([0x55, 0xAA, 0xAA, 255]),
    Rgba([0x55, 0xAA, 0xFF, 255]),
    Rgba([0x55, 0xFF, 0x00, 255]),
    Rgba([0x55, 0xFF, 0x55, 255]),
    Rgba([0x55, 0xFF, 0xAA, 255]),
    Rgba([0x55, 0xFF, 0xFF, 255]),
    Rgba([0xAA, 0x00, 0x00, 255]),
    Rgba([0xAA, 0x00, 0x55, 255]),
    Rgba([0xAA, 0x00, 0xAA, 255]),
    Rgba([0xAA, 0x00, 0xFF, 255]),
    Rgba([0xAA, 0x55, 0x00, 255]),
    Rgba([0xAA, 0x55, 0x55, 255]),
    Rgba([0xAA, 0x55, 0xAA, 255]),
    Rgba([0xAA, 0x55, 0xFF, 255]),
    Rgba([0xAA, 0xAA, 0x00, 255]),
    Rgba([0xAA, 0xAA, 0x55, 255]),
    Rgba([0xAA, 0xAA, 0xAA, 255]),
    Rgba([0xAA, 0xAA, 0xFF, 255]),
    Rgba([0xAA, 0xFF, 0x00, 255]),
    Rgba([0xAA, 0xFF, 0x55, 255]),
    Rgba([0xAA, 0xFF, 0xAA, 255]),
    Rgba([0xAA, 0xFF, 0xFF, 255]),
    Rgba([0xFF, 0x00, 0x00, 255]),
    Rgba([0xFF, 0x00, 0x55, 255]),
    Rgba([0xFF, 0x00, 0xAA, 255]),
    Rgba([0xFF, 0x00, 0xFF, 255]),
    Rgba([0xFF, 0x55, 0x00, 255]),
    Rgba([0xFF, 0x55, 0x55, 255]),
    Rgba([0xFF, 0x55, 0xAA, 255]),
    Rgba([0xFF, 0x55, 0xFF, 255]),
    Rgba([0xFF, 0xAA, 0x00, 255]),
    Rgba([0xFF, 0xAA, 0x55, 255]),
    Rgba([0xFF, 0xAA, 0xAA, 255]),
    Rgba([0xFF, 0xAA, 0xFF, 255]),
    Rgba([0xFF, 0xFF, 0x00, 255]),
    Rgba([0xFF, 0xFF, 0x55, 255]),
    Rgba([0xFF, 0xFF, 0xAA, 255]),
    Rgba([0xFF, 0xFF, 0xFF, 255]),
];

/// Standard VGA 256-colour palette (mode 13h DAC default): the 16-colour
/// set (0-15, = [`CGA`]), a 16-step grey ramp (16-31), nine 24-colour
/// hue cycles in three value groups (32-247), and eight trailing blacks
/// (248-255). Matches the Allegro 4.x `default_palette` / DOSBox
/// `palette_256` (6-bit values upconverted `v<<2 | v>>4`). Anchors:
/// pure red `#FF0000` at 40, green at 48, blue at 32, grey 0 and white at
/// 16 and 31, brown `#AA5500` at 6.
pub static VGA: [Rgba<u8>; 256] = [
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 170, 255]),
    Rgba([0, 170, 0, 255]),
    Rgba([0, 170, 170, 255]),
    Rgba([170, 0, 0, 255]),
    Rgba([170, 0, 170, 255]),
    Rgba([170, 85, 0, 255]),
    Rgba([170, 170, 170, 255]),
    Rgba([85, 85, 85, 255]),
    Rgba([85, 85, 255, 255]),
    Rgba([85, 255, 85, 255]),
    Rgba([85, 255, 255, 255]),
    Rgba([255, 85, 85, 255]),
    Rgba([255, 85, 255, 255]),
    Rgba([255, 255, 85, 255]),
    Rgba([255, 255, 255, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([20, 20, 20, 255]),
    Rgba([32, 32, 32, 255]),
    Rgba([44, 44, 44, 255]),
    Rgba([56, 56, 56, 255]),
    Rgba([69, 69, 69, 255]),
    Rgba([81, 81, 81, 255]),
    Rgba([97, 97, 97, 255]),
    Rgba([113, 113, 113, 255]),
    Rgba([130, 130, 130, 255]),
    Rgba([146, 146, 146, 255]),
    Rgba([162, 162, 162, 255]),
    Rgba([182, 182, 182, 255]),
    Rgba([203, 203, 203, 255]),
    Rgba([227, 227, 227, 255]),
    Rgba([255, 255, 255, 255]),
    Rgba([0, 0, 255, 255]),
    Rgba([65, 0, 255, 255]),
    Rgba([125, 0, 255, 255]),
    Rgba([190, 0, 255, 255]),
    Rgba([255, 0, 255, 255]),
    Rgba([255, 0, 190, 255]),
    Rgba([255, 0, 125, 255]),
    Rgba([255, 0, 65, 255]),
    Rgba([255, 0, 0, 255]),
    Rgba([255, 65, 0, 255]),
    Rgba([255, 125, 0, 255]),
    Rgba([255, 190, 0, 255]),
    Rgba([255, 255, 0, 255]),
    Rgba([190, 255, 0, 255]),
    Rgba([125, 255, 0, 255]),
    Rgba([65, 255, 0, 255]),
    Rgba([0, 255, 0, 255]),
    Rgba([0, 255, 65, 255]),
    Rgba([0, 255, 125, 255]),
    Rgba([0, 255, 190, 255]),
    Rgba([0, 255, 255, 255]),
    Rgba([0, 190, 255, 255]),
    Rgba([0, 125, 255, 255]),
    Rgba([0, 65, 255, 255]),
    Rgba([125, 125, 255, 255]),
    Rgba([158, 125, 255, 255]),
    Rgba([190, 125, 255, 255]),
    Rgba([223, 125, 255, 255]),
    Rgba([255, 125, 255, 255]),
    Rgba([255, 125, 223, 255]),
    Rgba([255, 125, 190, 255]),
    Rgba([255, 125, 158, 255]),
    Rgba([255, 125, 125, 255]),
    Rgba([255, 158, 125, 255]),
    Rgba([255, 190, 125, 255]),
    Rgba([255, 223, 125, 255]),
    Rgba([255, 255, 125, 255]),
    Rgba([223, 255, 125, 255]),
    Rgba([190, 255, 125, 255]),
    Rgba([158, 255, 125, 255]),
    Rgba([125, 255, 125, 255]),
    Rgba([125, 255, 158, 255]),
    Rgba([125, 255, 190, 255]),
    Rgba([125, 255, 223, 255]),
    Rgba([125, 255, 255, 255]),
    Rgba([125, 223, 255, 255]),
    Rgba([125, 190, 255, 255]),
    Rgba([125, 158, 255, 255]),
    Rgba([182, 182, 255, 255]),
    Rgba([199, 182, 255, 255]),
    Rgba([219, 182, 255, 255]),
    Rgba([235, 182, 255, 255]),
    Rgba([255, 182, 255, 255]),
    Rgba([255, 182, 235, 255]),
    Rgba([255, 182, 219, 255]),
    Rgba([255, 182, 199, 255]),
    Rgba([255, 182, 182, 255]),
    Rgba([255, 199, 182, 255]),
    Rgba([255, 219, 182, 255]),
    Rgba([255, 235, 182, 255]),
    Rgba([255, 255, 182, 255]),
    Rgba([235, 255, 182, 255]),
    Rgba([219, 255, 182, 255]),
    Rgba([199, 255, 182, 255]),
    Rgba([182, 255, 182, 255]),
    Rgba([182, 255, 199, 255]),
    Rgba([182, 255, 219, 255]),
    Rgba([182, 255, 235, 255]),
    Rgba([182, 255, 255, 255]),
    Rgba([182, 235, 255, 255]),
    Rgba([182, 219, 255, 255]),
    Rgba([182, 199, 255, 255]),
    Rgba([0, 0, 113, 255]),
    Rgba([28, 0, 113, 255]),
    Rgba([56, 0, 113, 255]),
    Rgba([85, 0, 113, 255]),
    Rgba([113, 0, 113, 255]),
    Rgba([113, 0, 85, 255]),
    Rgba([113, 0, 56, 255]),
    Rgba([113, 0, 28, 255]),
    Rgba([113, 0, 0, 255]),
    Rgba([113, 28, 0, 255]),
    Rgba([113, 56, 0, 255]),
    Rgba([113, 85, 0, 255]),
    Rgba([113, 113, 0, 255]),
    Rgba([85, 113, 0, 255]),
    Rgba([56, 113, 0, 255]),
    Rgba([28, 113, 0, 255]),
    Rgba([0, 113, 0, 255]),
    Rgba([0, 113, 28, 255]),
    Rgba([0, 113, 56, 255]),
    Rgba([0, 113, 85, 255]),
    Rgba([0, 113, 113, 255]),
    Rgba([0, 85, 113, 255]),
    Rgba([0, 56, 113, 255]),
    Rgba([0, 28, 113, 255]),
    Rgba([56, 56, 113, 255]),
    Rgba([69, 56, 113, 255]),
    Rgba([85, 56, 113, 255]),
    Rgba([97, 56, 113, 255]),
    Rgba([113, 56, 113, 255]),
    Rgba([113, 56, 97, 255]),
    Rgba([113, 56, 85, 255]),
    Rgba([113, 56, 69, 255]),
    Rgba([113, 56, 56, 255]),
    Rgba([113, 69, 56, 255]),
    Rgba([113, 85, 56, 255]),
    Rgba([113, 97, 56, 255]),
    Rgba([113, 113, 56, 255]),
    Rgba([97, 113, 56, 255]),
    Rgba([85, 113, 56, 255]),
    Rgba([69, 113, 56, 255]),
    Rgba([56, 113, 56, 255]),
    Rgba([56, 113, 69, 255]),
    Rgba([56, 113, 85, 255]),
    Rgba([56, 113, 97, 255]),
    Rgba([56, 113, 113, 255]),
    Rgba([56, 97, 113, 255]),
    Rgba([56, 85, 113, 255]),
    Rgba([56, 69, 113, 255]),
    Rgba([81, 81, 113, 255]),
    Rgba([89, 81, 113, 255]),
    Rgba([97, 81, 113, 255]),
    Rgba([105, 81, 113, 255]),
    Rgba([113, 81, 113, 255]),
    Rgba([113, 81, 105, 255]),
    Rgba([113, 81, 97, 255]),
    Rgba([113, 81, 89, 255]),
    Rgba([113, 81, 81, 255]),
    Rgba([113, 89, 81, 255]),
    Rgba([113, 97, 81, 255]),
    Rgba([113, 105, 81, 255]),
    Rgba([113, 113, 81, 255]),
    Rgba([105, 113, 81, 255]),
    Rgba([97, 113, 81, 255]),
    Rgba([89, 113, 81, 255]),
    Rgba([81, 113, 81, 255]),
    Rgba([81, 113, 89, 255]),
    Rgba([81, 113, 97, 255]),
    Rgba([81, 113, 105, 255]),
    Rgba([81, 113, 113, 255]),
    Rgba([81, 105, 113, 255]),
    Rgba([81, 97, 113, 255]),
    Rgba([81, 89, 113, 255]),
    Rgba([0, 0, 65, 255]),
    Rgba([16, 0, 65, 255]),
    Rgba([32, 0, 65, 255]),
    Rgba([48, 0, 65, 255]),
    Rgba([65, 0, 65, 255]),
    Rgba([65, 0, 48, 255]),
    Rgba([65, 0, 32, 255]),
    Rgba([65, 0, 16, 255]),
    Rgba([65, 0, 0, 255]),
    Rgba([65, 16, 0, 255]),
    Rgba([65, 32, 0, 255]),
    Rgba([65, 48, 0, 255]),
    Rgba([65, 65, 0, 255]),
    Rgba([48, 65, 0, 255]),
    Rgba([32, 65, 0, 255]),
    Rgba([16, 65, 0, 255]),
    Rgba([0, 65, 0, 255]),
    Rgba([0, 65, 16, 255]),
    Rgba([0, 65, 32, 255]),
    Rgba([0, 65, 48, 255]),
    Rgba([0, 65, 65, 255]),
    Rgba([0, 48, 65, 255]),
    Rgba([0, 32, 65, 255]),
    Rgba([0, 16, 65, 255]),
    Rgba([32, 32, 65, 255]),
    Rgba([40, 32, 65, 255]),
    Rgba([48, 32, 65, 255]),
    Rgba([56, 32, 65, 255]),
    Rgba([65, 32, 65, 255]),
    Rgba([65, 32, 56, 255]),
    Rgba([65, 32, 48, 255]),
    Rgba([65, 32, 40, 255]),
    Rgba([65, 32, 32, 255]),
    Rgba([65, 40, 32, 255]),
    Rgba([65, 48, 32, 255]),
    Rgba([65, 56, 32, 255]),
    Rgba([65, 65, 32, 255]),
    Rgba([56, 65, 32, 255]),
    Rgba([48, 65, 32, 255]),
    Rgba([40, 65, 32, 255]),
    Rgba([32, 65, 32, 255]),
    Rgba([32, 65, 40, 255]),
    Rgba([32, 65, 48, 255]),
    Rgba([32, 65, 56, 255]),
    Rgba([32, 65, 65, 255]),
    Rgba([32, 56, 65, 255]),
    Rgba([32, 48, 65, 255]),
    Rgba([32, 40, 65, 255]),
    Rgba([44, 44, 65, 255]),
    Rgba([48, 44, 65, 255]),
    Rgba([52, 44, 65, 255]),
    Rgba([60, 44, 65, 255]),
    Rgba([65, 44, 65, 255]),
    Rgba([65, 44, 60, 255]),
    Rgba([65, 44, 52, 255]),
    Rgba([65, 44, 48, 255]),
    Rgba([65, 44, 44, 255]),
    Rgba([65, 48, 44, 255]),
    Rgba([65, 52, 44, 255]),
    Rgba([65, 60, 44, 255]),
    Rgba([65, 65, 44, 255]),
    Rgba([60, 65, 44, 255]),
    Rgba([52, 65, 44, 255]),
    Rgba([48, 65, 44, 255]),
    Rgba([44, 65, 44, 255]),
    Rgba([44, 65, 48, 255]),
    Rgba([44, 65, 52, 255]),
    Rgba([44, 65, 60, 255]),
    Rgba([44, 65, 65, 255]),
    Rgba([44, 60, 65, 255]),
    Rgba([44, 52, 65, 255]),
    Rgba([44, 48, 65, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
    Rgba([0, 0, 0, 255]),
];

/// Nintendo 2C02 master palette — the well-known **192-byte** emulator
/// `.pal` (64 entries × 3 bytes, the standard `.pal` layout; see
/// <https://www.nesdev.org/wiki/.pal>). Row-major 6-bit colour codes
/// `$00..$3F`; the values are the classic "2C02G" master table shipped as
/// the default by classic emulators and pixel-art tools.
///
/// Not emulated (documented, PLAN-30 §2): per-sprite 4-colour sub-palette
/// and backdrop quirks, emphasis bits, and the fact that the codes share
/// values (55 distinct RGB entries; `$0D`/`$0E`/`$0F`, `$1D`-`$1F`,
/// `$2E`/`$2F`, `$3E`/`$3F` are the PPU's black/forbidden slots).
pub static NES: [Rgba<u8>; 64] = [
    Rgba([0x7C, 0x7C, 0x7C, 255]),
    Rgba([0x00, 0x00, 0xFC, 255]),
    Rgba([0x00, 0x00, 0xBC, 255]),
    Rgba([0x44, 0x28, 0xBC, 255]),
    Rgba([0x94, 0x00, 0x84, 255]),
    Rgba([0xA8, 0x00, 0x20, 255]),
    Rgba([0xA8, 0x10, 0x00, 255]),
    Rgba([0x88, 0x14, 0x00, 255]),
    Rgba([0x50, 0x30, 0x00, 255]),
    Rgba([0x00, 0x78, 0x00, 255]),
    Rgba([0x00, 0x68, 0x00, 255]),
    Rgba([0x00, 0x58, 0x00, 255]),
    Rgba([0x00, 0x40, 0x58, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0xBC, 0xBC, 0xBC, 255]),
    Rgba([0x00, 0x78, 0xF8, 255]),
    Rgba([0x00, 0x58, 0xF8, 255]),
    Rgba([0x68, 0x44, 0xFC, 255]),
    Rgba([0xD8, 0x00, 0xCC, 255]),
    Rgba([0xE4, 0x00, 0x58, 255]),
    Rgba([0xF8, 0x38, 0x00, 255]),
    Rgba([0xE4, 0x5C, 0x10, 255]),
    Rgba([0xAC, 0x7C, 0x00, 255]),
    Rgba([0x00, 0xB8, 0x00, 255]),
    Rgba([0x00, 0xA8, 0x00, 255]),
    Rgba([0x00, 0xA8, 0x44, 255]),
    Rgba([0x00, 0x88, 0x88, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0xF8, 0xF8, 0xF8, 255]),
    Rgba([0x3C, 0xBC, 0xFC, 255]),
    Rgba([0x68, 0x88, 0xFC, 255]),
    Rgba([0x98, 0x78, 0xF8, 255]),
    Rgba([0xF8, 0x78, 0xF8, 255]),
    Rgba([0xF8, 0x58, 0x98, 255]),
    Rgba([0xF8, 0x78, 0x58, 255]),
    Rgba([0xFC, 0xA0, 0x44, 255]),
    Rgba([0xF8, 0xB8, 0x00, 255]),
    Rgba([0xB8, 0xF8, 0x18, 255]),
    Rgba([0x58, 0xD8, 0x54, 255]),
    Rgba([0x58, 0xF8, 0x98, 255]),
    Rgba([0x00, 0xE8, 0xD8, 255]),
    Rgba([0x78, 0x78, 0x78, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0xFC, 0xFC, 0xFC, 255]),
    Rgba([0xA4, 0xE4, 0xFC, 255]),
    Rgba([0xB8, 0xB8, 0xF8, 255]),
    Rgba([0xD8, 0xB8, 0xF8, 255]),
    Rgba([0xF8, 0xB8, 0xF8, 255]),
    Rgba([0xF8, 0xA4, 0xC0, 255]),
    Rgba([0xF0, 0xD0, 0xB0, 255]),
    Rgba([0xFC, 0xE0, 0xA8, 255]),
    Rgba([0xF8, 0xD8, 0x78, 255]),
    Rgba([0xD8, 0xF8, 0x78, 255]),
    Rgba([0xB8, 0xF8, 0xB8, 255]),
    Rgba([0xB8, 0xF8, 0xD8, 255]),
    Rgba([0x00, 0xFC, 0xFC, 255]),
    Rgba([0xD8, 0xD8, 0xD8, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
    Rgba([0x00, 0x00, 0x00, 255]),
];

/// Snap an RGB triple onto the 15-bit (RGB555) grid: 5 bits per channel,
/// the Genesis/Mega Drive colour depth. `gen16` runs median-cut to 16 and
/// snaps the result with this — flavour, not master-palette fidelity
/// (documented approximation, PLAN-30 §2).
pub fn snap_rgb555(c: Rgba<u8>) -> Rgba<u8> {
    // 8→5 bits: keep the top 5, replicate them into the low bits so 0xFF
    // stays 0xFF and 0x00 stays 0x00 (the classic full-range scale).
    let q = |v: u8| {
        let hi = v >> 3;
        (hi << 3) | (hi >> 2)
    };
    Rgba([q(c.0[0]), q(c.0[1]), q(c.0[2]), c.0[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sizes() {
        // PLAN-30 A.4: "nes 192" = the 192-byte master .pal (64 entries).
        assert_eq!(NES.len(), 64);
        assert_eq!(NES.len() * 3, 192);
        assert_eq!(GB.len(), 4);
        assert_eq!(CGA.len(), 16);
        assert_eq!(EGA.len(), 64);
        assert_eq!(VGA.len(), 256);
        assert!(NES.iter().all(|c| c.0[3] == 255));
        assert!(GB.iter().all(|c| c.0[3] == 255));
        assert!(CGA.iter().all(|c| c.0[3] == 255));
        assert!(EGA.iter().all(|c| c.0[3] == 255));
        assert!(VGA.iter().all(|c| c.0[3] == 255));
    }

    #[test]
    fn gb_dmg_greens() {
        // DMG greens, dark -> light.
        assert_eq!(GB[0].0, [0x0F, 0x38, 0x0F, 255]);
        assert_eq!(GB[1].0, [0x30, 0x62, 0x30, 255]);
        assert_eq!(GB[2].0, [0x8B, 0xAC, 0x0F, 255]);
        assert_eq!(GB[3].0, [0x9B, 0xBC, 0x0F, 255]);
    }

    #[test]
    fn nes_master_row0() {
        // Row $0x of the classic 2C02 master table.
        assert_eq!(NES[0x00].0, [0x7C, 0x7C, 0x7C, 255]);
        assert_eq!(NES[0x01].0, [0x00, 0x00, 0xFC, 255]);
        assert_eq!(NES[0x02].0, [0x00, 0x00, 0xBC, 255]);
        assert_eq!(NES[0x06].0, [0xA8, 0x10, 0x00, 255]);
        assert_eq!(NES[0x0C].0, [0x00, 0x40, 0x58, 255]);
        // $0D..$0F are the black/forbidden slots.
        for i in [0x0D, 0x0E, 0x0F] {
            assert_eq!(NES[i].0, [0, 0, 0, 255], "#{i:02X}");
        }
        // Row anchors: $10 light grey, $11 light blue, $30 near-white.
        assert_eq!(NES[0x10].0, [0xBC, 0xBC, 0xBC, 255]);
        assert_eq!(NES[0x11].0, [0x00, 0x78, 0xF8, 255]);
        assert_eq!(NES[0x30].0, [0xFC, 0xFC, 0xFC, 255]);
        assert_eq!(NES[0x3D].0, [0xD8, 0xD8, 0xD8, 255]);
    }

    #[test]
    fn cga_set() {
        assert_eq!(CGA[0].0, [0, 0, 0, 255]);
        assert_eq!(CGA[1].0, [0x00, 0x00, 0xAA, 255]);
        assert_eq!(CGA[6].0, [0xAA, 0x55, 0x00, 255]); // brown
        assert_eq!(CGA[15].0, [0xFF, 0xFF, 0xFF, 255]);
    }

    #[test]
    fn ega_is_classic_4x4x4() {
        // Index = R*16 + G*4 + B over the four binary levels.
        assert_eq!(EGA[0].0, [0, 0, 0, 255]);
        assert_eq!(EGA[63].0, [255, 255, 255, 255]);
        assert_eq!(EGA[16].0, [0x55, 0, 0, 255]);
        assert_eq!(EGA[32].0, [0xAA, 0, 0, 255]);
        assert_eq!(EGA[48].0, [0xFF, 0, 0, 255]);
        assert_eq!(EGA[4].0, [0, 0x55, 0, 255]);
        assert_eq!(EGA[1].0, [0, 0, 0x55, 255]);
        // All 64 combinations are distinct.
        let mut uniq: Vec<[u8; 4]> = EGA.iter().map(|c| c.0).collect();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 64);
    }

    #[test]
    fn vga_standard_anchors() {
        assert_eq!(VGA[0].0, [0, 0, 0, 255]);
        assert_eq!(VGA[6].0, [0xAA, 0x55, 0x00, 255]); // brown, not dark yellow
        assert_eq!(VGA[15].0, [255, 255, 255, 255]);
        // Grey ramp occupies 16..=31, black to white.
        assert_eq!(VGA[16].0, [0, 0, 0, 255]);
        assert_eq!(VGA[31].0, [255, 255, 255, 255]);
        for i in 17..31 {
            let c = VGA[i].0;
            assert_eq!(c[0], c[1]);
            assert_eq!(c[1], c[2]);
            assert!(c[0] > VGA[i - 1].0[0], "grey ramp not monotonic at {i}");
        }
        // The acceptance anchor: pure red present and pinned.
        assert_eq!(VGA[40].0, [255, 0, 0, 255]);
        let red = Rgba([255u8, 0, 0, 255]);
        assert_eq!(VGA.iter().position(|c| *c == red), Some(40));
        assert_eq!(VGA[48].0, [0, 255, 0, 255]);
        assert_eq!(VGA[32].0, [0, 0, 255, 255]);
        // Last eight entries are black (BIOS default).
        for c in VGA.iter().skip(248) {
            assert_eq!(c.0, [0, 0, 0, 255]);
        }
    }

    #[test]
    fn vga_first16_equal_cga() {
        for i in 0..16 {
            assert_eq!(VGA[i].0, CGA[i].0, "entry {i}");
        }
    }

    #[test]
    fn snap_rgb555_grid() {
        assert_eq!(snap_rgb555(Rgba([0, 0, 0, 255])).0, [0, 0, 0, 255]);
        assert_eq!(
            snap_rgb555(Rgba([255, 255, 255, 255])).0,
            [255, 255, 255, 255]
        );
        // 128 -> 5-bit 17 (0b10000) -> 0b10000100 = 132
        assert_eq!(snap_rgb555(Rgba([128, 0, 0, 255])).0, [132, 0, 0, 255]);
        // Idempotent + alpha preserved.
        let c = Rgba([12u8, 200, 77, 42]);
        let s = snap_rgb555(c);
        assert_eq!(snap_rgb555(s), s);
        assert_eq!(s.0[3], 42);
        // Every output is a grid point: high 5 bits replicated in low 3.
        for v in s.0.iter().take(3) {
            let hi = *v >> 3;
            assert_eq!(*v, (hi << 3) | (hi >> 2));
        }
    }
}
