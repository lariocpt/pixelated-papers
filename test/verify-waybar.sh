#!/usr/bin/env bash
# S8 live verification: the waybar module against a scratch home, on the
# live seat. Run from the repo root:
#
#   cargo build --release
#   WAYLAND_DISPLAY=wayland-1 bash test/verify-waybar.sh
#
# What it does:
#   1. Seeds a scratch PIXPAPERS_HOME with three small 16x16 entries
#      (alpha, beta with explicit pixel 32, gamma) and checks each one's
#      exact bar label.
#   2. Starts a transient waybar at the BOTTOM (the user's own config is
#      top-placed and is never loaded or modified), configured with the
#      shipped module (contrib/waybar/config.snippet), against the
#      scratch home. Its on-click "${TERMINAL:-foot} -e pixpapers tui"
#      resolves to exactly "foot -e pixpapers tui" here, since TERMINAL
#      is unset in this session. No pixpapers daemon is ever started:
#      with WAYLAND_DISPLAY set, `set`/`cycle` would auto-spawn one
#      (S5), and a daemon bound to this scratch home would repaint the
#      real wallpaper with 16x16 test images. Every state-mutating call
#      below therefore runs with WAYLAND_DISPLAY+DISPLAY stripped,
#      exactly like the offline smoke layer ("recorded — will show when
#      the daemon runs"); `bar` needs no stripping (it never spawns)
#      and runs in the real environment, like waybar's exec children.
#      The entry label is daemon-state-independent anyway (S8 golden 4
#      pins exactly that).
#      Note: the transient waybar inherits the user's
#      ~/.config/waybar/style.css (read-only; it is the standard style
#      lookup for any waybar instance). The bar background therefore
#      blends into the wallpaper, so label presence is asserted from
#      the text ink, not the background.
#   3. Polls until the label first renders, screenshots the bar, then
#      runs the snippet's exact on-click command (foot -e pixpapers tui)
#      and screenshots the TUI.
#   4. Flips the entry twice — cycle -1 (wrap to gamma), then set beta —
#      screenshotting at +2s (stale expected; a lucky 30s tick phase that
#      already flipped is caught by the pixel asserts below and fails)
#      and at +33s (at least one interval has elapsed, so the label MUST
#      have flipped).
#
# Flip evidence: the vision fleet (OCR) was down on the verification
# date, so label content is established deterministically: the diff
# between consecutive strip crops is measured INSIDE the label's ink
# bounding box (any pixel brighter than the bar background), which is
# immune to the mouse pointer and wallpaper elsewhere in the strip.
# Required outcomes: 1c == 3a (both alpha, no tick between),
# 3a != 3b (alpha -> gamma within one interval), 3b == 4a (both gamma,
# the +2s stale shot), 4a != 4b (gamma -> beta within one interval).
# The CLI `expect_bar` checks pin the exact strings; the pixel diffs
# prove the rendered bar actually moved.
#
# Physical click/scroll cannot be injected from a script on this
# session; the on-click command run here (step 3) and the cycle/set
# commands (step 4) are the named substitutes — they are the identical
# commands the module's on-click/on-scroll keys execute. Do not park
# the pointer over the right end of the bar during the run: the label
# sits right-aligned there, and the asserts are scoped to its ink box.
#
# Screenshots land in docs/screens/s8/ (shot 1: full + strip crop,
# shot 2: full, label-flip pairs: strip crops only, so the label is
# legible).

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
[ -n "${WAYLAND_DISPLAY:-}" ] || { echo "WAYLAND_DISPLAY not set — run from the live session" >&2; exit 1; }
[ -x "$REPO/target/release/pixpapers" ] || { echo "target/release/pixpapers missing — run cargo build --release first" >&2; exit 1; }
for tool in waybar foot grim python3; do
  command -v "$tool" >/dev/null || { echo "$tool not found" >&2; exit 1; }
done
python3 -c "import PIL" 2>/dev/null || { echo "python3-Pillow needed for the strip crops" >&2; exit 1; }

OUT="$REPO/docs/screens/s8"
mkdir -p "$OUT"
SCRATCH="$(mktemp -d /tmp/pixpapers-s8-XXXXXX)"
# The waybar process (and its on-click/on-scroll children) must resolve
# the current release binary, not the stale pre-TUI one in ~/.cargo/bin.
export PATH="$REPO/target/release:$PATH"
export PIXPAPERS_HOME="$SCRATCH"
# waybar height in logical px (config below); used to size strip crops.
BAR_H=48
# This machine's single output: 3840x2160 @ scale 1.5 => 2560x1440 logical.
# The crop math divides by 1440, which works whether grim reports buffer
# or logical pixels.
LOGICAL_H=1440
# How long to wait for the first label render (the exec module's first
# tick fires at startup, so this only guards a slow machine).
WAIT_MAX=15
# A flip must change at least this many pixels inside the label's ink
# box (four glyph positions changing at 13px monospace is ~150-400).
FLIP_MIN=50

FOOT_PID=""
WAYBAR_PID=""
cleanup() {
  # Kill the TUI child (direct child of foot), then foot itself.
  [ -n "$FOOT_PID" ] && pkill -P "$FOOT_PID" 2>/dev/null || true
  [ -n "$FOOT_PID" ] && kill "$FOOT_PID" 2>/dev/null || true
  [ -n "$WAYBAR_PID" ] && kill "$WAYBAR_PID" 2>/dev/null || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

# shot FULL CROP: capture one frame with grim. FULL empty -> the capture
# goes to a temp file that is deleted (CROP-only); CROP empty -> only
# FULL is kept; both non-empty -> both are kept.
shot() {
  # Separate local statements: a multi-word `local` evaluates every word
  # against the pre-assignment bindings, so tmp="$full" would trip set -u.
  local full="$1"
  local crop="${2:-}"
  local tmp="$full"
  if [ -n "$crop" ] && [ -z "$full" ]; then
    tmp="$(mktemp /tmp/s8-shot.XXXXXX)"
  fi
  grim "$tmp"
  if [ -n "$crop" ]; then
    python3 - "$tmp" "$crop" "$BAR_H" "$LOGICAL_H" <<'PY'
import sys
from PIL import Image
full, crop, bar_h, logical_h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
im = Image.open(full)
strip = max(1, round(bar_h * im.height / logical_h))
im.crop((0, im.height - strip, im.width, im.height)).save(crop)
PY
  fi
  if [ "$tmp" != "$full" ]; then
    rm -f "$tmp"
  fi
}

# inkcount FULL: number of label-ink pixels in the bottom strip. Ink is
# any pixel brighter than the bar background: the blended background
# measures at most (47,47,62) and the pointer bump at most (47,47,62),
# while the label color is #cdd6f4 = (205,214,244).
inkcount() {
  python3 - "$1" "$BAR_H" "$LOGICAL_H" <<'PY'
import sys
from PIL import Image
full, bar_h, logical_h = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
im = Image.open(full)
strip = max(1, round(bar_h * im.height / logical_h))
crop = im.crop((0, im.height - strip, im.width, im.height))
w, h = crop.size
px = list(crop.get_flattened_data())
n = 0
for y in range(h):
    row = px[y*w:(y+1)*w]
    for p in row:
        if p[0] > 80 or p[1] > 80 or p[2] > 80:
            n += 1
print(n)
PY
}

# wait_for_label: poll grim until the bottom strip carries label ink
# (the module's first tick has rendered). Returns 1 after WAIT_MAX.
wait_for_label() {
  local tmp
  local n
  local deadline
  tmp="$(mktemp /tmp/s8-poll.XXXXXX)"
  deadline=$((SECONDS + WAIT_MAX))
  while :; do
    grim "$tmp"
    n="$(inkcount "$tmp")"
    if [ "$n" -gt 50 ]; then
      rm -f "$tmp"
      return 0
    fi
    if [ "$SECONDS" -ge $deadline ]; then
      rm -f "$tmp"
      return 1
    fi
    sleep 2
  done
}

# diffbox A B: differing pixels inside A's label ink box (0-based).
# Prints NOMARK if A has no ink.
diffbox() {
  python3 - "$1" "$2" <<'PY'
import sys
from PIL import Image
ia, ib = Image.open(sys.argv[1]), Image.open(sys.argv[2])
w, h = ia.size
pa = list(ia.get_flattened_data())
pb = list(ib.get_flattened_data())
def inkbox(px):
    minx, miny, maxx, maxy = w, h, -1, -1
    for y in range(h):
        row = px[y*w:(y+1)*w]
        for x, p in enumerate(row):
            if p[0] > 80 or p[1] > 80 or p[2] > 80:
                if x < minx: minx = x
                if x > maxx: maxx = x
                if y < miny: miny = y
                if y > maxy: maxy = y
    if maxx < 0:
        return None
    return (minx, miny, maxx, maxy)
box = inkbox(pa)
if box is None:
    print("NOMARK")
    sys.exit(0)
diff = 0
for y in range(box[1], box[3] + 1):
    ra = pa[y*w:(y+1)*w]
    rb = pb[y*w:(y+1)*w]
    for x in range(box[0], box[2] + 1):
        if ra[x] != rb[x]:
            diff += 1
print(diff)
PY
}

# assert_same A B C: A and B are strip crops whose labels must be
# identical (C is a human-readable pair name).
assert_same() {
  local a="$1"
  local b="$2"
  local c="$3"
  local d
  d="$(diffbox "$a" "$b")"
  if [ "$d" = "NOMARK" ]; then
    echo "no label ink in $(basename "$a") — cannot verify '$c'" >&2
    exit 1
  fi
  if [ "$d" -ne 0 ]; then
    echo "expected identical labels for '$c': $(basename "$a") vs $(basename "$b") differ by $d px inside the label ink box" >&2
    exit 1
  fi
  echo "  pixel-diff $c: 0 px inside the label ink box — identical"
}

# assert_flip A B C: A and B are strip crops whose labels must have
# changed (C is a human-readable pair name).
assert_flip() {
  local a="$1"
  local b="$2"
  local c="$3"
  local d
  d="$(diffbox "$a" "$b")"
  if [ "$d" = "NOMARK" ]; then
    echo "no label ink in $(basename "$a") — cannot verify '$c'" >&2
    exit 1
  fi
  if [ "$d" -lt "$FLIP_MIN" ]; then
    echo "flip not visible for '$c': $(basename "$a") vs $(basename "$b") differ by only $d px inside the label ink box (want >= $FLIP_MIN)" >&2
    exit 1
  fi
  echo "  pixel-diff $c: $d px inside the label ink box (>= $FLIP_MIN) — label changed"
}

expect_bar() {
  local got
  got="$(pixpapers bar)"
  if [ "$got" != "$1" ]; then
    echo "bar mismatch: got '$got' want '$1'" >&2
    exit 1
  fi
  echo "  bar: $got"
}

# State mutations with the compositor env stripped: `set`/`cycle`/knob
# edits call apply_now, which auto-spawns the daemon when
# WAYLAND_DISPLAY or DISPLAY is present — see the header.
pp_mutate() { env -u WAYLAND_DISPLAY -u DISPLAY pixpapers "$@"; }

echo "=== S8 live verification ==="
echo "scratch home: $SCRATCH"
echo "waybar: $(waybar --version | head -1)"
echo "screenshots: $OUT"
echo

# -- 1. Seed three entries --------------------------------------------------
# 16x16 flat-color PNGs: tiny on disk, and the upscaled thumbnails stay
# legible in the bar strip crops.
python3 - "$SCRATCH" <<'PY'
import os, sys
from PIL import Image
home = sys.argv[1]
for name, rgb in [("alpha", (200, 220, 255)), ("beta", (0, 180, 60)), ("gamma", (200, 0, 180))]:
    Image.new("RGB", (16, 16), rgb).save(os.path.join(home, name + ".png"))
PY
pp_mutate add "$SCRATCH/alpha.png" --title alpha >/dev/null
pp_mutate add "$SCRATCH/beta.png"  --title beta  >/dev/null
pp_mutate add "$SCRATCH/gamma.png" --title gamma >/dev/null
pp_mutate pixel beta 32 >/dev/null   # explicit: beta renders 32/fit
pp_mutate set alpha >/dev/null       # current = alpha (16/fit, resolved default)

echo "seeded 3 entries (alpha current, beta explicit pixel 32, gamma)"
echo "expected labels:"
for want in "▞ alpha · 16/fit" "▞ beta · 32/fit" "▞ gamma · 16/fit"; do
  case "$want" in
    *"beta"*) pp_mutate set beta >/dev/null ;;
    *"gamma"*) pp_mutate set gamma >/dev/null ;;
    *) pp_mutate set alpha >/dev/null ;;
  esac
  expect_bar "$want"
done
pp_mutate set alpha >/dev/null
expect_bar "▞ alpha · 16/fit"

# -- 2. Transient waybar with the shipped module ----------------------------
# Flat waybar schema: the custom module's settings live at the top level
# under "custom/pixpapers"; the module list only references its name.
# position bottom: the user's own bar config (top) is never loaded.
# format: the named {text} placeholder — a positional form such as
# "{ }" is rejected by the newer libfmt (waybar 0.15.0 links fmt 12)
# with "invalid format string", leaving the module silently hidden.
cat > "$SCRATCH/waybar.jsonc" <<CFG
{
  "position": "bottom",
  "height": $BAR_H,
  "modules-right": ["custom/pixpapers"],
  "custom/pixpapers": {
    "format": "{text}",
    "exec": "pixpapers bar",
    "interval": 30,
    "on-click": "foot -e pixpapers tui",
    "on-scroll-up": "pixpapers cycle 1",
    "on-scroll-down": "pixpapers cycle -1"
  }
}
CFG
waybar -c "$SCRATCH/waybar.jsonc" >/dev/null 2>&1 &
WAYBAR_PID=$!
sleep 2
kill -0 "$WAYBAR_PID" || { echo "waybar exited early" >&2; exit 1; }
echo "transient waybar pid $WAYBAR_PID (bottom, scratch home, interval 30s)"

# -- 3. Bar renders; the snippet's exact on-click opens the TUI --------------
if ! wait_for_label; then
  echo "label never rendered within ${WAIT_MAX}s (module hidden?) — check the waybar log" >&2
  exit 1
fi
shot "$OUT/1-bar-alpha.png" "$OUT/1-bar-alpha-crop.png"
echo "shot 1-bar-alpha{,-crop}.png (label ink confirmed present)"

foot -e pixpapers tui >/dev/null 2>&1 &
FOOT_PID=$!
sleep 3
shot "$OUT/2-tui-in-foot.png"
echo "shot 2-tui-in-foot.png (ran the snippet's exact on-click: foot -e pixpapers tui)"
pkill -P "$FOOT_PID" 2>/dev/null || true
kill "$FOOT_PID" 2>/dev/null || true
FOOT_PID=""

# -- 4. Label flips within one 30s interval ----------------------------------
# The module ticks every 30s after start (exec-on-event re-runs it on
# click/scroll, but no physical input can be injected from here).
# Measured per flip: +2s (stale expected) and +33s (>= one interval has
# elapsed, so the label must show the new entry).
pp_mutate cycle -1 >/dev/null      # from alpha wraps to gamma
echo "cycle -1: current is now gamma"
sleep 2
shot "" "$OUT/3a-cycle-gamma-stale-crop.png"
echo "shot 3a (at +2s: stale '▞ alpha · 16/fit' expected)"
assert_same "$OUT/1-bar-alpha-crop.png" "$OUT/3a-cycle-gamma-stale-crop.png" "1c vs 3a (both alpha)"
sleep 31
shot "" "$OUT/3b-cycle-gamma-flipped-crop.png"
echo "shot 3b (at +33s: must show '▞ gamma · 16/fit')"
assert_flip "$OUT/3a-cycle-gamma-stale-crop.png" "$OUT/3b-cycle-gamma-flipped-crop.png" "3a vs 3b (alpha -> gamma)"
expect_bar "▞ gamma · 16/fit"

pp_mutate set beta >/dev/null
echo "set beta: current is now beta"
sleep 2
shot "" "$OUT/4a-set-beta-stale-crop.png"
echo "shot 4a (at +2s: stale '▞ gamma · 16/fit' expected)"
assert_same "$OUT/3b-cycle-gamma-flipped-crop.png" "$OUT/4a-set-beta-stale-crop.png" "3b vs 4a (both gamma)"
sleep 31
shot "" "$OUT/4b-set-beta-flipped-crop.png"
echo "shot 4b (at +33s: must show '▞ beta · 32/fit')"
assert_flip "$OUT/4a-set-beta-stale-crop.png" "$OUT/4b-set-beta-flipped-crop.png" "4a vs 4b (gamma -> beta)"
expect_bar "▞ beta · 32/fit"

echo
echo "=== S8 live verification done ==="
echo "shots in $OUT:"
ls -lh "$OUT"
echo
echo "Physical click/scroll were not injectable: substitutes were the"
echo "snippet's exact on-click command (step 3) and the cycle/set"
echo "commands (step 4) — the same commands the module runs."
echo "Flip evidence is the pixel diffs above (vision fleet down); the"
echo "exact label strings were pinned by expect_bar."
