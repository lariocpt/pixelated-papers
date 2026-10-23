#!/bin/sh
# The check a green compile cannot give you: does the manifest survive real
# edits, do the exports actually land as PNGs, and — when run inside a wayland
# session — does a layer-shell surface really appear and vanish again?
#
# Run by hand or in CI:
#
#     test/smoke.sh target/debug/pixpapers
#
# The offline layers run everywhere against a throwaway PIXPAPERS_HOME. The
# daemon layer runs only with a live WAYLAND_DISPLAY and skips loudly
# everywhere else: a headless CI box is not a broken compositor.
set -eu

BIN=${1:-target/debug/pixpapers}
[ -x "$BIN" ] || { echo "FAIL: $BIN is not executable"; exit 1; }
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
[ -f "$FIXTURE" ] || { echo "FAIL: test/fixture.png missing"; exit 1; }

# Offline commands never see WAYLAND_DISPLAY: without it `set` records but
# cannot auto-spawn, so the filesystem-truth layers stay deterministic even
# when the suite runs inside a live session. The daemon layer opts back in.
run() { env -u WAYLAND_DISPLAY PIXPAPERS_HOME="$WORK/home" "$@"; }
# Stop any daemon auto-spawn got us a hold of, whatever phase failed.
trap 'env -u WAYLAND_DISPLAY PIXPAPERS_HOME="$WORK/home" "$BIN" stop >/dev/null 2>&1; rm -rf "$WORK"' EXIT
say() { printf '  %s\n' "$*"; }
fail() { echo "FAIL: $*"; [ -f "$WORK/out.txt" ] && sed 's/^/> /' "$WORK/out.txt"; exit 1; }

echo "smoke: $BIN"

# ------------------------------------------------------------- library basics
run "$BIN" add "$FIXTURE" --title first > "$WORK/out.txt" || fail "add failed"
grep -q 'added' "$WORK/out.txt" || fail "add printed no confirmation"
grep -q ' first ' "$WORK/out.txt" || fail "title missing from add output"
say "add stores an entry"

run "$BIN" add "$FIXTURE" > "$WORK/out.txt" || fail "re-add failed"
grep -q 'already in library' "$WORK/out.txt" || fail "hash dedupe did not engage"
say "identical bytes dedupe by hash"

run "$BIN" list > "$WORK/out.txt" || fail "list failed"
grep -q 'first' "$WORK/out.txt" || fail "list does not show the entry"
COUNT=$(grep -c 'first' "$WORK/out.txt")
[ "$COUNT" = "1" ] || fail "dedupe failed: entry appears $COUNT times"
say "list shows exactly one entry"

# ----------------------------------------------------------------- policy edit
run "$BIN" set first || fail "set failed (no daemon must be fine)"
run "$BIN" pixel first 64 > "$WORK/out.txt" || fail "pixel edit failed"
grep -q '64' "$WORK/out.txt" || fail "pixel edit output wrong"
run "$BIN" placement first cover-crop > /dev/null || fail "placement edit failed"
run "$BIN" bg first "#ff0000" > /dev/null || fail "bg edit failed"
run "$BIN" gutter first 12 > /dev/null || fail "gutter edit failed"
run "$BIN" list > "$WORK/out.txt" || fail "list after edits failed"
grep -q '64 .*cover-crop' "$WORK/out.txt" || fail "edits not visible in list"
say "per-wallpaper knobs persist"

run "$BIN" default pixel 32 > /dev/null || fail "default pixel failed"
run "$BIN" disable > /dev/null || fail "disable failed"
run "$BIN" enable > /dev/null || fail "enable failed"
say "global defaults and the master switch round-trip"

# --------------------------------------------------------------------- exports
for p in 8 16 64 128 normal; do
    run "$BIN" export first 800x500 --pixel "$p" -o "$WORK/e-$p.png" \
        > /dev/null || fail "export pixel=$p failed"
    head -c 4 "$WORK/e-$p.png" | grep -q 'PNG' || fail "export pixel=$p is not a PNG"
done
run "$BIN" export first 800x500 --placement cover-gutter --gutter 20 \
    --bg "#102030" -o "$WORK/e-gut.png" > /dev/null || fail "export cover-gutter failed"
[ -s "$WORK/e-gut.png" ] || fail "export cover-gutter wrote an empty file"
say "exports: every preset and every placement produce PNGs"

# ----------------------------------------------------------------- status json
run "$BIN" status --json > "$WORK/status.json" || fail "status --json failed"
grep -q '"daemon_running": false' "$WORK/status.json" \
    || fail "offline status claims a daemon"
grep -Eq '"current": *"?[0-9a-f]{6,12}"?' "$WORK/status.json" \
    || fail "status json lost current"
say "status --json reports filesystem truth offline"

# ------------------------------------------------------------------- cycle/rem
run "$BIN" cycle > /dev/null || fail "cycle failed (single entry must not divide by zero)"
run "$BIN" remove first > "$WORK/out.txt" || fail "remove failed"
run "$BIN" list > "$WORK/out.txt" || fail "list after remove failed"
grep -q 'library empty' "$WORK/out.txt" || fail "remove left the entry behind"
say "cycle survives a one-entry library; remove clears it"

# ----------------------------------------------------------------- daemon live
if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    say "SKIP: no WAYLAND_DISPLAY — daemon/layer-shell layer not exercised"
else
    # `exec` so $! names the daemon itself, not a wrapper that swallows the kill.
    ( exec env PIXPAPERS_HOME="$WORK/home" PIXPAPERS_LOG=error "$BIN" daemon ) \
        > "$WORK/daemon.txt" 2>&1 &
    dpid=$!
    sleep 2
    if ! kill -0 "$dpid" 2>/dev/null; then
        say "SKIP: daemon exited at once (compositor without wlr-layer-shell?):"
        sed 's/^/    /' "$WORK/daemon.txt"
    else
        run "$BIN" add "$FIXTURE" --title live > /dev/null || fail "live add failed"
        run "$BIN" set live > "$WORK/out.txt" || fail "live set failed"
        grep -q 'applied' "$WORK/out.txt" \
            || { kill "$dpid" 2>/dev/null || true; fail "set did not report applied with a daemon up"; }
        run "$BIN" status --json > "$WORK/status.json" || fail "live status failed"
        grep -q '"daemon_running": true' "$WORK/status.json" \
            || { kill "$dpid" 2>/dev/null || true; fail "daemon did not report running"; }
        grep -Eq '"name": *"[A-Za-z]' "$WORK/status.json" \
            || { kill "$dpid" 2>/dev/null || true; fail "daemon reports no outputs"; }
        say "daemon applies over IPC and reports outputs"

        run "$BIN" stop > /dev/null || fail "stop failed"
        sleep 1
        if kill -0 "$dpid" 2>/dev/null; then
            kill -9 "$dpid" 2>/dev/null || true
            fail "daemon survived pixpapers stop"
        fi
        say "stop takes the daemon down cleanly"
    fi
fi

echo "smoke: ok"
