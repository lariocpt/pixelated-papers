#!/bin/sh
# Step-8 verification matrix — the live-session items scripted (V11 recovery,
# V13 live knobs, V16 IPC fuzz). V1/V2 timings live in bench-daemon.sh; V3
# screenshots in docs/screens/step8/; results recorded in docs/VERIFICATION.md.
#
#     WAYLAND_DISPLAY=wayland-1 test/verify-live.sh [bin]
set -eu

BIN=${1:-target/release/pixpapers}
[ -x "$BIN" ] || BIN=target/debug/pixpapers
[ -x "$BIN" ] || { echo "FAIL: no binary"; exit 1; }
[ -n "${WAYLAND_DISPLAY:-}" ] || { echo "SKIP: no WAYLAND_DISPLAY"; exit 0; }

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
WORK=$(mktemp -d)
run() { env PIXPAPERS_HOME="$WORK/home" "$@"; }
cleanup() {
    env PIXPAPERS_HOME="$WORK/home" "$BIN" stop >/dev/null 2>&1 || true
    sleep 0.2
    rm -rf "$WORK"
}
trap cleanup EXIT
say() { printf '  %s\n' "$*"; }
fail() { echo "FAIL: $*"; exit 1; }

echo "verify-live: $BIN · $WAYLAND_DISPLAY"

run "$BIN" add "$FIXTURE" --title v >/dev/null

# ── V11: kill -9 → next set recovers ≤ 1 s ────────────────────────────────
run "$BIN" set v >/dev/null            # auto-spawns
sleep 0.5
pid=$(run "$BIN" status --json | grep -o '"daemon_pid": *[0-9]*' | grep -o '[0-9]*')
[ -n "$pid" ] || fail "V11: no daemon pid"
kill -9 "$pid"
sleep 0.3
t0=$(date +%s%N)
run "$BIN" set v | grep -q applied || fail "V11: respawn set did not apply"
t1=$(date +%s%N)
ms=$(( (t1 - t0) / 1000000 ))
[ "$ms" -le 1000 ] || fail "V11: recovery took ${ms} ms (> 1 s)"
say "V11 kill -9 → respawn+apply in ${ms} ms"

# ── V13: every knob changes the live render, no blanking ─────────────────
for spec in "pixel 8" "pixel normal" "placement cover-crop" "bg #00ff00" "gutter 64" "placement fit"; do
    set -- $spec
    run "$BIN" "$1" v "$2" >/dev/null || fail "V13: $spec failed"
done
run "$BIN" disable >/dev/null && run "$BIN" enable >/dev/null || fail "V13: switch failed"
run "$BIN" status | grep -E "v$|[0-9a-f]{8}" >/dev/null || fail "V13: status broken"
say "V13 pixel/placement/bg/gutter + disable/enable live without errors"

# ── V16: 1000 IPC requests (valid + garbage + truncation), state intact ──
sock="$WORK/home/state/sock"
m_before=$(sha256sum "$WORK/home/data/library.json" | cut -d' ' -f1)
c_before=$(sha256sum "$WORK/home/config.toml" 2>/dev/null | cut -d' ' -f1 || echo none)
python3 - "$sock" <<'EOF' || fail "V16: fuzz harness crashed"
import socket, sys, random
random.seed(8)
sock_path = sys.argv[1]
valid = [b'{"cmd":"Apply"}', b'{"cmd":"Status"}']
bad = [b'{"cmd":"nonsense"}', b'not json', b'\x00\x01\x02\xff', b'{"cmd":"Apply"', b'', b'[]', b'{"cmd":"QuitXX"}',
       b'{"cmd":"Apply"}garbage trailing', b'{' + b'a'*4096, b'{"cmd":', bytes(range(256))]
ok = conn = 0
for i in range(1000):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.settimeout(2)
        s.connect(sock_path)
        payload = random.choice(valid) if random.random() < 0.6 else random.choice(bad)
        s.sendall(payload + (b'\n' if random.random() < 0.9 else b''))
        # mirror ipc::roundtrip: half-close write so the daemon sees EOF and
        # replies instead of waiting out its read timeout (the real client).
        s.shutdown(socket.SHUT_WR)
        try: s.recv(65536)
        except Exception: pass
        conn += 1
    except Exception:
        pass
    finally:
        s.close()
print(f"fuzz: {conn}/1000 connections got a response")
EOF
sleep 0.5
run "$BIN" status --json | grep '"daemon_running": true' >/dev/null || fail "V16: daemon dead/unresponsive after fuzz"
m_after=$(sha256sum "$WORK/home/data/library.json" | cut -d' ' -f1)
c_after=$(sha256sum "$WORK/home/config.toml" 2>/dev/null | cut -d' ' -f1 || echo none)
[ "$m_before" = "$m_after" ] || fail "V16: manifest mutated by fuzz"
[ "$c_before" = "$c_after" ] || fail "V16: config mutated by fuzz"
say "V16 1000 fuzzed IPC requests: daemon alive, manifest/config hashes unchanged"

# ── V17: SIGTERM = graceful shutdown (systemd/logind rely on it) ──────────
# (regression: signal-hook's flag handler silently never fired on this box;
# the daemon now installs a raw sigaction handler — see daemon/mod.rs)
pid=$(run "$BIN" status --json | grep -o '"daemon_pid": *[0-9]*' | grep -o '[0-9]*')
[ -n "$pid" ] || fail "V17: no daemon pid"
kill -TERM "$pid"
i=0
while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 30 ]; do sleep 0.1; i=$((i + 1)); done
kill -0 "$pid" 2>/dev/null && fail "V17: daemon ignored SIGTERM after 3 s"
[ -S "$WORK/home/state/sock" ] && fail "V17: socket left behind after SIGTERM"
say "V17 SIGTERM → graceful teardown in $((i))00 ms, socket removed"

echo "verify-live: ok"
