#!/bin/sh
# PLAN-30 §B.4 — X11 backend verification under Xvfb (hermetic
# PIXPAPERS_HOME). Asserts: DISPLAY-only start ⇒ backend x11; set ⇒
# root pixmap byte-identical to the export oracle; RFPWM broadcast ⇒
# re-render at the new size; stop ⇒ socket/pid gone, X server alive,
# root cleared to solid black (no stray wallpaper);
# X server killed ⇒ daemon exits 43. Bonus: non-24-depth root ⇒ exit 1.
#
#     test/verify-x11.sh [bin]        # self-wraps in xvfb-run if DISPLAY unset
#
# Honest skip by name when the X tooling is absent (same policy as the
# live Wayland layer): CI machines without xvfb-run skip; a machine that
# claims to test x11 must install it.
set -eu

command -v xvfb-run >/dev/null 2>&1 || { echo "SKIP: xvfb-run not installed (pacman -S xorg-server-xvfb / apt install xvfb)"; exit 0; }
command -v import >/dev/null 2>&1 || { echo "SKIP: import (ImageMagick) not installed — no root-capture oracle"; exit 0; }
command -v compare >/dev/null 2>&1 || { echo "SKIP: compare (ImageMagick) not installed"; exit 0; }
# IM7 renamed convert to magick (the bare name still works but warns);
# prefer magick so the suite output stays clean.
MAGICK=$(command -v magick || command -v convert) \
    || { echo "SKIP: ImageMagick (magick/convert) not installed"; exit 0; }
command -v xrandr >/dev/null 2>&1 || { echo "SKIP: xrandr not installed"; exit 0; }
python3 -c 'import Xlib' 2>/dev/null || { echo "SKIP: python3-xlib not installed (RFPWM broadcaster)"; exit 0; }

BIN=${1:-target/release/pixpapers}
[ -x "$BIN" ] || BIN=target/debug/pixpapers
[ -x "$BIN" ] || { echo "FAIL: no binary (build release or pass a path)"; exit 1; }

# Always run under a fresh, killable Xvfb — never against an inherited
# DISPLAY (that could be someone's live Xwayland; test 5 kills its X
# server). The marker env var guards against re-exec loops. The marker
# file carries the inner verdict: test 5 kills the Xvfb out from under
# xvfb-run, whose own cleanup then exits 1 despite a green suite.
if [ -z "${PIX_X11_WRAPPED:-}" ]; then
    MARK=$(mktemp)
    PIX_X11_WRAPPED=1 PIX_X11_MARK="$MARK" xvfb-run -a -s "-screen 0 1280x1024x24" "$0" "$BIN" && rc=0 || rc=$?
    if [ -s "$MARK" ]; then
        rm -f "$MARK"
        exit 0
    fi
    rm -f "$MARK"
    exit "$rc"
fi

# The x11 path must win with DISPLAY alone — no wayland anywhere near it.
unset WAYLAND_DISPLAY

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
WORK=$(mktemp -d)
run() { env PIXPAPERS_HOME="$WORK/home" "$@"; }
say() { printf '  %s\n' "$*"; }
fail() { echo "FAIL: $*"; exit 1; }
cleanup() {
    run "$BIN" stop >/dev/null 2>&1 || true
    sleep 0.2
    kill "$DPID" >/dev/null 2>&1 || true
    rm -rf "$WORK"
}
DPID=""
trap cleanup EXIT

echo "verify-x11: $BIN · DISPLAY=$DISPLAY"

# ── 1. start with DISPLAY only (no WAYLAND_DISPLAY) ⇒ backend x11 ─────
run "$BIN" add "$FIXTURE" --title v >/dev/null
run "$BIN" daemon >"$WORK/daemon.log" 2>&1 &
DPID=$!
sleep 1.2
run "$BIN" status --json | grep -q '"backend": *"x11"' \
    || fail "auto-detect: backend is not x11 (log: $(tail -2 "$WORK/daemon.log" 2>/dev/null))"
say "auto-detect: DISPLAY only ⇒ backend x11"

# ── 2. set <id> changes the root pixmap (export is the oracle) ────────
run "$BIN" set v | grep -q applied || fail "set did not apply"
sleep 0.7
import -window root "$WORK/root-1280.png" 2>/dev/null || fail "root capture failed"
run "$BIN" export v 1280x1024 -o "$WORK/oracle-1280.png" >/dev/null
ae=$(compare -metric AE "$WORK/root-1280.png" "$WORK/oracle-1280.png" null: 2>&1) || true
ae=${ae%% *}
[ "$ae" = "0" ] || fail "root pixmap differs from export oracle (AE=$ae)"
say "set: root pixmap byte-identical to export oracle (AE=0)"

# ── 3. RFPWM broadcast ⇒ re-render at the new size ─────────────────────
# The server-side resize (Xvfb keeps its CRTC; the fb resize lands and
# xrandr's follow-up errors are expected) plus the RFPWM ClientMessage
# broadcast to the root — the daemon reacts, it never initiates.
xrandr --fb 1024x768 >/dev/null 2>&1 || true
sleep 0.5
python3 - <<'PYEOF' || fail "RFPWM broadcast failed"
from Xlib import X, display
from Xlib.protocol import event
d = display.Display()
root = d.screen().root
mr = d.intern_atom("_NET_WM_MOVERESIZE")
ar = d.intern_atom("_NET_WM_ACTION_RESIZE")
ev = event.ClientMessage(window=root.id, client_type=mr, data=(32, [ar, 0, 0, 0, 0]))
root.send_event(event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask
                | X.StructureNotifyMask, event=ev, propagate=False)
d.sync()
PYEOF
sleep 0.7
grep -q "RFPWM resize hint" "$WORK/daemon.log" || fail "daemon never saw the RFPWM hint"
run "$BIN" status --json | grep -q '"width": *1024' || fail "daemon did not adopt the new size"
import -window root "$WORK/root-1024.png" 2>/dev/null || fail "root capture failed"
run "$BIN" export v 1024x768 -o "$WORK/oracle-1024.png" >/dev/null
ae=$(compare -metric AE "$WORK/root-1024.png" "$WORK/oracle-1024.png" null: 2>&1) || true
ae=${ae%% *}
[ "$ae" = "0" ] || fail "root pixmap at new size differs from export (AE=$ae)"
say "RFPWM: hint received, re-rendered at 1024×768, pixmap == export oracle"

# ── 4. stop removes socket/pid and leaves the X server running ────────
run "$BIN" stop
sleep 0.3
[ ! -S "$WORK/home/state/sock" ] || fail "socket left behind after stop"
[ ! -f "$WORK/home/state/daemon.pid" ] || fail "pidfile left behind after stop"
kill -0 "$DPID" 2>/dev/null && fail "daemon still running after stop"
xrandr >/dev/null 2>&1 || fail "X server died with the daemon"
say "stop: socket+pid removed, X server still up"
# The daemon takes the wallpaper with it: stop clears every root to
# solid black, so no stray frame survives on the X server. The root is
# 1024x768 here (test 3 resized the fb).
import -window root "$WORK/root-after-stop.png" 2>/dev/null || fail "root capture after stop failed"
"$MAGICK" -size 1024x768 xc:black "$WORK/black-ref.png" || fail "black reference failed"
ae=$(compare -metric AE "$WORK/root-after-stop.png" "$WORK/black-ref.png" null: 2>&1) || true
ae=${ae%% *}
[ "$ae" = "0" ] || fail "root not cleared to solid black after stop (AE=$ae)"
say "stop: root cleared to solid black (AE=0) — no stray wallpaper left"

# ── 5. killing the X server ⇒ daemon exits 43 ──────────────────────────
run "$BIN" daemon >"$WORK/daemon2.log" 2>&1 &
DPID=$!
sleep 1.2
run "$BIN" set v >/dev/null
sleep 0.5
XVFB_PID=$(pgrep -x Xvfb | head -1)
[ -n "$XVFB_PID" ] || fail "Xvfb pid not found"
kill "$XVFB_PID"
rc=0
wait "$DPID" || rc=$?
DPID=""
[ "$rc" = "43" ] || fail "daemon exited $rc after X server killed, expected 43"
[ ! -S "$WORK/home/state/sock" ] || fail "socket left behind after exit 43"
say "X server killed ⇒ daemon exit 43 (identical contract to wayland)"

# ── 6. non-24-depth root ⇒ loud user error (exit 1) ───────────────────
# Nested xvfb-run: its -a picks a different display than ours.
rc=0
env -u WAYLAND_DISPLAY PIXPAPERS_HOME="$WORK/home2" \
    xvfb-run -a -s "-screen 0 640x480x16" "$BIN" daemon >"$WORK/depth.log" 2>&1 || rc=$?
[ "$rc" = "1" ] || fail "depth-16 root: exit $rc, expected 1"
grep -q "depth is 16" "$WORK/depth.log" || fail "depth-16 refusal message missing: $(cat "$WORK/depth.log")"
say "depth-16 root ⇒ exit 1 with the loud depth message"

if [ -n "${PIX_X11_MARK:-}" ]; then
    printf 'ok\n' > "$PIX_X11_MARK"
fi
echo "verify-x11: ok"
