#!/bin/sh
# Headless-sway acceptance for exact fractional buffers (docs/SCALING.md
# "Exact fractional buffers"). Boots sway on the headless backend inside a
# named Docker container and asserts from `status --json` (the observable
# proof — scale_path + frac_scale + buffer dims) that:
#
#   1920x1080 @ 1.0 → legacy path, buffer 1920x1080 (unchanged behavior)
#   3840x2160 @ 1.5 → exact path, buffer 3840x2160: the panel's own grid
#                     (logical 2560x1440 × 1.5), NOT the legacy 5120x2880
#                     oversample — the dev-panel case reproduced
#   1920x1080 @ 2.0 → legacy path, buffer 1920x1080 (unchanged behavior)
#
# With PIXPAPERS_BASELINE_BIN set to a pre-feature binary, additionally
# runs the 3840x2160 @ 1.5 case on it: the old oversampled path
# (5120x2880 at the same virtual output) and the RSS pair exact < legacy.
#
#     test/verify-wayland-fractional.sh target/release/pixpapers
#     PIXPAPERS_BASELINE_BIN=/path/to/old/pixpapers test/verify-wayland-fractional.sh
#
# Skips with a message when Docker is absent or unreachable — a box without
# Docker is not a broken compositor. Containers are --rm and named; the one
# image built here is the committed Dockerfile's build. CI (which caches the
# image's build layers itself) may pre-build the image and set
# PIXPAPERS_IMAGE_BUILT=1 to skip the build step here; the image must then
# already be loaded, or this script fails rather than silently rebuilding.
set -eu

BIN=${1:-target/release/pixpapers}
BASELINE=${PIXPAPERS_BASELINE_BIN:-}
[ -x "$BIN" ] || { echo "FAIL: $BIN is not executable"; exit 1; }
BIN=$(readlink -f "$BIN")

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
INSIDE="$HERE/wayland-fractional/inside.sh"
IMAGE=pixpapers-wayland-fractional

say() { printf '  %s\n' "$*"; }
fail() { echo "FAIL: $*"; exit 1; }

command -v docker >/dev/null 2>&1 \
    || { echo "SKIP: docker not installed — headless-sway fractional acceptance not exercised"; exit 0; }
docker info >/dev/null 2>&1 \
    || { echo "SKIP: docker daemon not reachable — headless-sway fractional acceptance not exercised"; exit 0; }

echo "verify-wayland-fractional: $BIN"
if [ -n "${PIXPAPERS_IMAGE_BUILT:-}" ]; then
    docker image inspect "$IMAGE" >/dev/null 2>&1 \
        || fail "PIXPAPERS_IMAGE_BUILT set but image $IMAGE is not loaded"
    say "image pre-built (PIXPAPERS_IMAGE_BUILT): $IMAGE"
else
    docker build -q -t "$IMAGE" "$HERE/wayland-fractional" >/dev/null \
        || fail "docker build of test/wayland-fractional failed"
fi

OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

# run_case <tag> <mode> <scale> <binary> — one container, prints RESULT lines.
run_case() {
    tag=$1; mode=$2; scale=$3; bin=$4
    docker run --rm --name "pixpapers-ff-$tag" \
        -v "$bin":/usr/local/bin/pixpapers:ro \
        -v "$FIXTURE":/fixture.png:ro \
        -v "$INSIDE":/inside.sh:ro \
        -e PIXPAPERS_BIN=/usr/local/bin/pixpapers \
        "$IMAGE" sh /inside.sh "$mode" "$scale" > "$OUT/$tag.txt" 2>&1
    sed 's/^/  /' "$OUT/$tag.txt"
}

field() { sed -n "s/.*$1=\([^ ]*\).*/\1/p" "$OUT/$2.txt" | head -1; }

check() { # check <tag> <field> <expected>
    got=$(field "$2" "$1")
    [ "$got" = "$3" ] || fail "$1: $2 = '$got', expected '$3'"
}

# ── 1920x1080 @ scale 1.0: legacy path, unchanged ──────────────────────
run_case s100 1920x1080 1.0 "$BIN"
check s100 path legacy
check s100 width 1920
check s100 height 1080
check s100 stable yes
say "1920x1080 @ 1.0 → legacy path, buffer 1920x1080 (unchanged)"

# ── 3840x2160 @ scale 1.5: exact path, the dev-panel case ─────────────
run_case s150 3840x2160 1.5 "$BIN"
check s150 path exact
check s150 frac 1.50
check s150 width 3840
check s150 height 2160
check s150 stable yes
say "3840x2160 @ 1.5 → exact path, buffer 3840x2160 = round(2560x1440 × 1.5), no thrash"

# ── 1920x1080 @ scale 2.0: legacy path, unchanged ──────────────────────
run_case s200 1920x1080 2.0 "$BIN"
check s200 path legacy
check s200 width 1920
check s200 height 1080
check s200 stable yes
say "1920x1080 @ 2.0 → legacy path, buffer 1920x1080 (unchanged)"

# ── optional: pre-feature binary at 1.5 → oversampled legacy + RSS pair ─
if [ -n "$BASELINE" ]; then
    [ -x "$BASELINE" ] || fail "PIXPAPERS_BASELINE_BIN=$BASELINE is not executable"
    run_case base150 3840x2160 1.5 "$(readlink -f "$BASELINE")"
    check base150 path absent
    check base150 width 5120
    check base150 height 2880
    say "pre-feature binary at 1.5 → legacy oversample 5120x2880 (no scale_path field)"
    RSS_EXACT=$(field rss_kb s150)
    RSS_LEGACY=$(field rss_kb base150)
    [ "$RSS_EXACT" -lt "$RSS_LEGACY" ] \
        || fail "RSS: exact ${RSS_EXACT} kB is not below legacy ${RSS_LEGACY} kB at the same output"
    say "RSS at the same virtual output: exact ${RSS_EXACT} kB < legacy ${RSS_LEGACY} kB"
else
    say "SKIP: PIXPAPERS_BASELINE_BIN unset — legacy-at-1.5 + RSS pair not exercised"
fi

echo "verify-wayland-fractional: ok"
