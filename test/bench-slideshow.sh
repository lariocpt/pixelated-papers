#!/usr/bin/env bash
# Idle cost of the daemon with the slideshow OFF vs ON.
#
#   test/bench-slideshow.sh
#
# The poll loop is a pure renderer: with the slideshow OFF it wakes on a 300ms
# timer (the historical idle cost); with it ON it instead sleeps to the next
# scheduled change, so between changes it does no polling work at all. This
# measures steady-state idle %CPU (from utime+stime in /proc) and VmRSS for
# both states over the same window, against a hermetic library — never the
# live desktop.
#
# The render a change triggers is the slideshow's actual work, not idle
# overhead, so the ON case is measured at a long interval where no change
# lands inside the window. Run on the box the daemon runs on.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="./target/release/pixpapers"
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 1; }

TMP="$(mktemp -d /tmp/pixpapers-slideshow-bench.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

export PIXPAPERS_HOME="$TMP/home"
export XDG_RUNTIME_DIR="$TMP/xdg"
export PIXPAPERS_BACKEND=V1        # the pure-X11 backend; no root, no portals
unset WAYLAND_DISPLAY
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"

# A few distinct wallpapers so the playlist has entries to cycle between.
python3 - "$TMP/wall" <<'PY'
import sys, os
from PIL import Image
d = sys.argv[1]; os.makedirs(d, exist_ok=True)
for i, c in enumerate([(40,60,120),(200,120,40),(40,160,110)]):
    Image.new("RGB", (1920, 1080), c).save(f"{d}/wall{i}.png")
PY

for i in 0 1 2; do
  "$BIN" add "$TMP/wall/wall$i.png" --title "wall$i" >/dev/null
done

HZ=$(getconf CLK_TCK)
sample() { awk '{print $14 + $15}' "/proc/$1/stat"; }   # utime+stime, clock ticks

measure() {  # $1 = label; runs a foreground daemon for 6s and prints idle %CPU + VmRSS
  local label="$1"
  "$BIN" daemon >/dev/null 2>&1 &
  local pid=$!
  sleep 0.4
  local t0 t1 rss
  t0=$(sample "$pid")
  sleep 6
  t1=$(sample "$pid")
  rss=$(awk '/VmRSS/{print $2}' "/proc/$pid/status" 2>/dev/null || echo '?')
  kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true
  local win=6
  local pct
  pct=$(awk -v d=$((t1 - t0)) -v hz="$HZ" -v w="$win" \
        'BEGIN { printf "%.3f", (d/hz)/w*100 }')
  printf "%-12s idle CPU : %s%% of one core (%ss window)\n" "$label" "$pct" "$win"
  printf "%-12s VmRSS    : %s kB\n" "" "$rss"
}

echo "=== slideshow OFF (300ms poll loop) ==="
# `slideshow off` is mutating and auto-spawns a daemon; quit it so the
# measured foreground run can bind the socket (same as the ON case).
"$BIN" slideshow off >/dev/null
"$BIN" stop 2>/dev/null || true
sleep 0.3
measure off

echo
echo "=== slideshow ON  (interval 300s, sleeping to the next change) ==="
# `slideshow on` auto-spawns a daemon; quit it so the measured foreground run
# can bind the socket. The config persists either way.
"$BIN" slideshow on 300 >/dev/null
"$BIN" stop 2>/dev/null || true
sleep 0.3
measure on
