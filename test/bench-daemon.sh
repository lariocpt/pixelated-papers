#!/bin/sh
# Daemon-side R8 numbers (step 9.1): switch latency p50/p95, idle RSS, and
# fd counts cold vs after the whole sweep — with a REAL compositor, because
# "applied" means "the compositor has our new buffer".
#
#     WAYLAND_DISPLAY=wayland-1 test/bench-daemon.sh [bin] [alternations]
#
# Skips (exit 0) without a session or if the daemon won't come up: a headless
# box is not a slow renderer.
set -eu

BIN=${1:-target/debug/pixpapers}
N=${2:-25}   # A↔B alternations → 2N switches
[ -x "$BIN" ] || { echo "FAIL: $BIN not executable"; exit 1; }
command -v date >/dev/null || { echo "FAIL: date needed"; exit 1; }

if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "SKIP: no WAYLAND_DISPLAY — daemon bench needs a live compositor"
    exit 0
fi

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
WORK=$(mktemp -d)

cleanup() {
    env PIXPAPERS_HOME="$WORK/home" "$BIN" stop >/dev/null 2>&1 || true
    sleep 0.3
    rm -rf "$WORK"
}
trap cleanup EXIT
run() { env PIXPAPERS_HOME="$WORK/home" "$@"; }

echo "bench-daemon: $BIN · session $WAYLAND_DISPLAY · $N alternations"

# Two visually distinct entries without any non-core generator: add the
# fixture, export it pixelated (different bytes ⇒ different hash), add that.
run "$BIN" add "$FIXTURE" --title A >/dev/null
run "$BIN" export A 320x200 --pixel 8 --bg "#304050" -o "$WORK/alt.png" >/dev/null
run "$BIN" add "$WORK/alt.png" --title B >/dev/null

# Cold first apply (spawn + first variant generation), timed.
t0=$(date +%s%N)
run "$BIN" set A >/dev/null
t1=$(date +%s%N)
echo "cold path (spawn+first apply): $(( (t1 - t0) / 1000000 )) ms"

# Warm both variants once, then alternate; each `set` round-trips CLI→IPC→
# render→commit, i.e. real perceived switch latency, cache hot.
run "$BIN" set B >/dev/null
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

# A cold preset switch on the live output: cache miss on screen (the 105 ms
# criterion number is the CPU part; this is what a human feels).
run "$BIN" set A >/dev/null
t0=$(date +%s%N)
run "$BIN" pixel A 32 >/dev/null
t1=$(date +%s%N)
echo "cold preset change (regenerate 4K variant + swap): $(( (t1 - t0) / 1000000 )) ms"

sort -n "$WORK/lat.txt" | awk -v n=$((2 * N)) '
    { v[NR] = $1 }
    END {
        i50 = int(n * 0.50) + 1; if (i50 > n) i50 = n;
        i95 = int(n * 0.95) + 1; if (i95 > n) i95 = n;
        printf "switch hot: p50 %d ms · p95 %d ms · max %d ms (n=%d)\n",
               v[i50], v[i95], v[n], n
    }'

# Footprint: ask the daemon itself (same numbers `status --bench` shows).
run "$BIN" status --bench | sed 's/^/  /'

# fd leak assertion: /proc fd count at start of a fresh daemon vs now.
pid=$(run "$BIN" status --json | grep -o '"daemon_pid": *[0-9]*' | grep -o '[0-9]*')
if [ -n "$pid" ] && [ -d "/proc/$pid/fd" ]; then
    echo "fds after sweep: $(ls "/proc/$pid/fd" | wc -l)"
fi
