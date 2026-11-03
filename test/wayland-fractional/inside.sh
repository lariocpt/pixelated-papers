#!/bin/sh
# Runs ONE headless-sway scenario inside the container built from
# test/wayland-fractional/Dockerfile. Started by
# test/verify-wayland-fractional.sh with:
#   /usr/local/bin/pixpapers  the host-built binary (ro mount)
#   /fixture.png              the repo test fixture (ro mount)
#   /inside.sh <mode> <output-scale>
#
# Prints machine-readable lines the host script asserts on:
#   RESULT scale=<s> mode=<m> width=<w> height=<h> legacy_scale=<n> path=<exact|legacy|absent> frac=<f> rss_kb=<n>
#   stable yes|no          (buffer dims identical across two statuses)
#   CACHE <variant cache file names>
#   EVIDENCE <daemon log line>   (configure / fractional-scale lines)
set -eu

SCALE=${2:?usage: inside.sh <mode> <output-scale>}
MODE=${1:?usage: inside.sh <mode> <output-scale>}
BIN=${PIXPAPERS_BIN:-pixpapers}

export WLR_BACKENDS=headless
export WLR_RENDERER=pixman          # software: no GPU in the container
export XDG_RUNTIME_DIR=/run/xdg
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
export PIXPAPERS_HOME=/tmp/pixhome  # hermetic, per container
export PIXPAPERS_LOG=info           # the configure/fractional log is the evidence

cat > /tmp/sway.config <<CFG
output HEADLESS-1 mode $MODE
output HEADLESS-1 scale $SCALE
default_border none
CFG

sway -c /tmp/sway.config > /tmp/sway.log 2>&1 &
sway_pid=$!
trap 'kill "$sway_pid" 2>/dev/null || true' EXIT

# Wait for sway's socket (headless startup is fast; 10 s ceiling).
WAYLAND_DISPLAY=
i=0
while [ $i -lt 50 ]; do
    for s in "$XDG_RUNTIME_DIR"/wayland-*; do
        [ -e "$s" ] && WAYLAND_DISPLAY=$(basename "$s") && break
    done
    [ -n "$WAYLAND_DISPLAY" ] && break
    i=$((i + 1)); sleep 0.2
done
if [ -z "$WAYLAND_DISPLAY" ]; then
    echo "FAIL: sway produced no wayland socket"; sed 's/^/  sway: /' /tmp/sway.log; exit 1
fi
# A config error would silently leave the output at sway's default mode —
# that would fake the whole scenario, so treat it as a hard failure.
if grep -q "Error on line" /tmp/sway.log; then
    echo "FAIL: sway rejected its config"; sed 's/^/  sway: /' /tmp/sway.log; exit 1
fi
export WAYLAND_DISPLAY

# Explicit daemon (the smoke.sh pattern: exec so the PID is the daemon).
( exec "$BIN" daemon ) > /tmp/daemon.log 2>&1 &
dpid=$!
sleep 1
kill -0 "$dpid" 2>/dev/null || { echo "FAIL: daemon died at once"; sed 's/^/  daemon: /' /tmp/daemon.log; exit 1; }

"$BIN" add /fixture.png --title ff >/dev/null
"$BIN" set ff >/dev/null
sleep 2   # let configure + preferred_scale + first render settle

get() { sed -n "s/.*\"$1\": *\"\{0,1\}\([^\",]*\)\"\{0,1\}.*/\1/p" /tmp/status.json | head -1; }
"$BIN" status --json > /tmp/status.json
W=$(get width); H=$(get height); LS=$(get scale)
SP=$(get scale_path); FS=$(get frac_scale)
[ -n "$SP" ] || SP=absent; [ -n "$FS" ] || FS=none
RSS=$(awk '/VmRSS/{print $2}' "/proc/$dpid/status" 2>/dev/null || echo 0)
echo "RESULT scale=$SCALE mode=$MODE width=$W height=$H legacy_scale=$LS path=$SP frac=$FS rss_kb=$RSS"

# No-thrash probe: a settled output must report the same buffer dims.
sleep 1
"$BIN" status --json > /tmp/status2.json
W2=$(sed -n 's/.*"width": *\([^",]*\).*/\1/p' /tmp/status2.json | head -1)
H2=$(sed -n 's/.*"height": *\([^",]*\).*/\1/p' /tmp/status2.json | head -1)
[ "$W$H" = "$W2$H2" ] && echo "stable=yes" || echo "stable=no"

# Variant cache keys must name the concrete buffer size (no `auto`, no
# path-dependent naming): the file names are the evidence.
echo "CACHE $(find "$PIXPAPERS_HOME" -path '*variants*' -type f -printf '%f ' 2>/dev/null)"

# The daemon's own words: path negotiation lines from the log.
grep -E "configure |fractional scale |viewporter|fractional-scale" /tmp/daemon.log \
    | sed 's/^/EVIDENCE /' || true

"$BIN" stop >/dev/null 2>&1 || true
kill "$sway_pid" 2>/dev/null || true
