#!/bin/sh
# PLAN-30 §B.4 — x11 blit bench: the full hot apply path (composite_into +
# PutImage) at 1080p and 4K under Xvfb, so the x11 blit can be compared
# against the Wayland stretch/commit numbers in docs/PERF.md.
#
#     test/bench-x11.sh [bin] [alternations]
#
# Wall time is the CLI→IPC→render→PutImage round trip (same accounting as
# bench-daemon.sh: includes ~10 ms of CLI spawn). The daemon's own
# counters split it: render = composite_into only, switch = composite +
# PutImage + flush, so switch − render ≈ the blit.
#
# Honest skip by name when xvfb-run is absent.
set -eu

BIN=${1:-target/release/pixpapers}
N=${2:-25}   # A↔B alternations → 2N applies per size
command -v xvfb-run >/dev/null 2>&1 || { echo "SKIP: xvfb-run not installed"; exit 0; }
[ -x "$BIN" ] || { echo "FAIL: $BIN not executable"; exit 1; }

# Outer: one fresh Xvfb per size (the daemon sizes itself at connect).
if [ -z "${PIX_X11_SIZE:-}" ]; then
    rc=0
    for size in 1920x1080 3840x2160; do
        PIX_X11_SIZE=$size xvfb-run -a -s "-screen 0 ${size%x*}x${size#*x}x24" \
            "$0" "$BIN" "$N" || rc=1
    done
    exit $rc
fi

# Inner: DISPLAY belongs to an Xvfb at PIX_X11_SIZE; no wayland near it.
SIZE=$PIX_X11_SIZE
unset WAYLAND_DISPLAY
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
WORK=$(mktemp -d)
run() { env PIXPAPERS_HOME="$WORK/home" "$@"; }
cleanup() {
    run "$BIN" stop >/dev/null 2>&1 || true
    sleep 0.2
    rm -rf "$WORK"
}
trap cleanup EXIT

echo "bench-x11: $BIN · Xvfb $SIZE · $N alternations"

# Two visually distinct entries (same recipe as bench-daemon.sh), so the
# variant cache holds both and every apply is a hot decode+stretch+blit.
run "$BIN" add "$FIXTURE" --title A >/dev/null
run "$BIN" export A "$SIZE" --pixel 8 --bg "#304050" -o "$WORK/alt.png" >/dev/null
run "$BIN" add "$WORK/alt.png" --title B >/dev/null

t0=$(date +%s%N)
run "$BIN" set A >/dev/null
t1=$(date +%s%N)
echo "cold path (spawn+first variant+first blit): $(( (t1 - t0) / 1000000 )) ms"

run "$BIN" set B >/dev/null   # warm both variants
: > "$WORK/lat.txt"
i=0
while [ "$i" -lt "$N" ]; do
    for w in A B; do
        t0=$(date +%s%N)
        run "$BIN" set "$w" >/dev/null
        t1=$(date +%s%N)
        echo $(( (t1 - t0) / 1000000 )) >> "$WORK/lat.txt"
    done
    i=$((i + 1))
done

sort -n "$WORK/lat.txt" | awk -v n=$((2 * N)) '
    { v[NR] = $1 }
    END {
        i50 = int(n * 0.50) + 1; if (i50 > n) i50 = n;
        i95 = int(n * 0.95) + 1; if (i95 > n) i95 = n;
        printf "switch hot: p50 %d ms · p95 %d ms · max %d ms (n=%d)\n",
               v[i50], v[i95], v[n], n
    }'

# The daemon's own split: render (composite) vs switch (composite+blit).
run "$BIN" status --bench | sed 's/^/  /'
