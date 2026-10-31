#!/bin/sh
# Variant byte sizes for the v0.2 palette modes vs the v0.1 auto-palette
# baseline (PLAN-30 §A.4). Byte-size facts, no timing; deterministic
# sources (the same photo() generator as examples/variant_sizes.rs, so
# the rows line up with the existing PERF.md table).
#
#     test/bench-sizes.sh
#
# The v0.1 baseline is measured from the untouched v0.1 code in a
# throwaway `git worktree` at 29ef2b4 — the parent of the first S1 commit,
# the same base the v0.1 byte-identity goldens were captured from (see the
# auto_none_matches_v01_bytes test). A probe example is dropped into the
# worktree (untracked, builds against the v0.1 Cargo.lock), and the whole
# worktree is removed on exit — the repo tree is never touched.
#
# The final section joins the two tables: v2 auto+dither-none must be
# byte-identical to v0.1 auto at fixture scale, not just on the goldens.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
V01_BASE=${V01_BASE:-29ef2b4}

cargo build --release --manifest-path "$HERE/bench-quantize/Cargo.toml" 1>&2

WORK=$(mktemp -d)
WT="$WORK/v01"
cleanup() {
    git -C "$ROOT" worktree remove --force "$WT" 2>/dev/null || rm -rf "$WT"
    git -C "$ROOT" worktree prune
    rm -rf "$WORK"
}
trap cleanup EXIT

"$HERE/bench-quantize/target/release/bench-quantize" sizes | tee "$WORK/v2.txt"

git -C "$ROOT" worktree add --detach "$WT" "$V01_BASE" >/dev/null
cat > "$WT/crates/pixcore/examples/v01_size_probe.rs" <<'PROBE'
//! S3 bench probe (untracked; dropped in by test/bench-sizes.sh): the v0.1
//! auto-palette variant sizes + blake3 hashes on the S3 fixtures. Same
//! photo() generator as examples/variant_sizes.rs.
use image::{ImageBuffer, Rgba, RgbaImage};
use pixcore::{Placement, ResolvedSettings, RgbCss, variant};

fn photo(w: u32, h: u32) -> RgbaImage {
    let mut img = ImageBuffer::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let xf = x as f64;
        let yf = y as f64;
        let r = ((xf * 0.013).sin() * 70.0 + ((yf * 0.004).cos() * 40.0) + 130.0) as u8;
        let g = ((yf * 0.017).cos() * 60.0 + ((xf * 0.009).sin() * 30.0) + 120.0) as u8;
        let b = (((xf + yf) * 0.011).sin() * 65.0 + 125.0) as u8;
        *px = Rgba([r, g, b, 255]);
    }
    img
}

fn main() {
    println!("# v0.1 baseline (auto palette) — evidence for docs/PERF.md\n");
    println!("| fixture | preset | canvas | palette | bytes | blake3/12 |");
    println!("|---|---|---|---|---|---|");
    for (fx, w, h) in [("photo 1080p", 1920u32, 1080u32), ("photo 4K", 3840, 2160)] {
        let orig = photo(w, h);
        for preset in ["16", "8"] {
            let s = ResolvedSettings {
                pixel: preset.parse().unwrap(),
                placement: Placement::CoverCrop,
                bg_color: RgbCss::BLACK,
                gutter_px: 24,
                palette: true,
            };
            let canvas = variant::generate_from_image(&orig, &s, (w, h));
            let bytes = variant::encode_variant(&canvas, true);
            let hash = blake3::hash(&bytes).to_hex()[..12].to_string();
            println!("#row|{fx}|{preset}|auto|{}|{hash}", bytes.len());
            println!(
                "| {fx} | {preset} | {}×{} | auto | {} | {hash} |",
                canvas.width(),
                canvas.height(),
                bytes.len()
            );
        }
    }
}
PROBE
cargo run -q --release --manifest-path "$WT/Cargo.toml" -p pixcore \
    --example v01_size_probe > "$WORK/v01.txt"
cat "$WORK/v01.txt"

echo
echo "== byte-identity: v2 auto/none vs v0.1 auto (fixture scale) =="
grep '^#row|' "$WORK/v01.txt" | while IFS='|' read -r _ fx preset mode bytes hash; do
    v2=$(grep "^#row|$fx|$preset|auto|" "$WORK/v2.txt")
    v2bytes=$(echo "$v2" | cut -d'|' -f5)
    v2hash=$(echo "$v2" | cut -d'|' -f6)
    if [ "$bytes" = "$v2bytes" ] && [ "$hash" = "$v2hash" ]; then
        echo "  $fx @ $preset: identical — $bytes B, blake3 $hash"
    else
        echo "  $fx @ $preset: DIFFERS — v0.1 $bytes B/$hash vs v2 $v2bytes B/$v2hash"
    fi
done
