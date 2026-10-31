#!/bin/sh
# The `normal` preset's S3.5 cost: full-canvas quantize on a 3840×2160
# canvas (a normal crop of a 4K original is 4K — S3 measured 1920×1080)
# for vga and auto with fs, plus full-size variant PNG byte sizes at
# 1920×1080 and 3840×2160 for {auto, vga, nes} — the disk cost of cached
# normal variants. Pure CPU, headless-safe, deterministic sources.
#
#     test/bench-normal.sh
#
# Release only (AGENTS.md: debug timings are misleading). Prints a
# markdown table for docs/PERF.md; min/median over fixed iteration counts
# (the helper warms up first). ~3–4 min wall on the dev machine.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

cargo build --release --manifest-path "$HERE/bench-quantize/Cargo.toml" 1>&2
"$HERE/bench-quantize/target/release/bench-quantize" normal-cost
echo
"$HERE/bench-quantize/target/release/bench-quantize" normal-sizes
