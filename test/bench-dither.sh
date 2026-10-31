#!/bin/sh
# Quantize-stage cost for the v0.2 palette/dither knobs (PLAN-30 §A.4):
# FS and bayer8 on the preset canvas shapes (120×68 = 1080p@16, 240×135 =
# 1080p@8 / 4K@16, 480×270 = 4K@8) plus the one-time full-res case on a
# 1920×1080 canvas. Pure CPU, headless-safe, deterministic sources.
#
#     test/bench-dither.sh
#
# Release only (AGENTS.md: debug timings are misleading). Prints a
# markdown table for docs/PERF.md; min/median over fixed iteration counts
# (the helper warms up first). ~1 min wall on the dev machine.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

cargo build --release --manifest-path "$HERE/bench-quantize/Cargo.toml" 1>&2
"$HERE/bench-quantize/target/release/bench-quantize" quantize-cost
