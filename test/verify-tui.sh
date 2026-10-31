#!/bin/sh
# S7 live row — the TUI action keys on a real pty (V-T5: the named skip
# S6 left; this replaces it). Drives the full key table against a
# scratch PIXPAPERS_HOME, then asserts the post-exit file truth: the
# wire capture is the record, the assertions are the row.
#
#     test/verify-tui.sh [bin]        (release preferred, debug fallback)
#
# Deliberately headless: the pty env strips DISPLAY/WAYLAND_DISPLAY and
# the image-protocol vars, so no daemon is auto-spawned against the real
# compositor — the Apply seam records (the status bar says so) while
# every file write (manifest, config, entry dirs, export) is real. The
# live *render* of the TUI is S6's V-T0/V-T0b rows (run under wayland).
set -eu

BIN=${1:-target/release/pixpapers}
[ -x "$BIN" ] || BIN=target/debug/pixpapers
[ -x "$BIN" ] || { echo "FAIL: no binary"; exit 1; }

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixture.png"
WORK=$(mktemp -d)
HOME_="$WORK/home"
DL="$WORK/downloads"
# c.png is TYPED into the input line, which cancels on `q` (spec) — so it
# lives at a fixed q-free path, not under $WORK's random suffix.
C_DIR="/tmp/pixpapers-verify-tui.$$"
mkdir -p "$C_DIR"
# nothing this script spawns sees a compositor or an image protocol
headless() {
    env -u DISPLAY -u WAYLAND_DISPLAY -u PIX_PAPERS_IMAGE \
        -u KITTY_WINDOW_ID -u ITERM_SESSION_ID -u SIXEL "$@"
}
run() { headless env PIXPAPERS_HOME="$HOME_" "$@"; }
cleanup() { rm -rf "$WORK" "$C_DIR"; }
trap cleanup EXIT
say() { printf '  %s\n' "$*"; }
fail() { echo "FAIL: $*"; exit 1; }

echo "verify-tui: $BIN · pty 100x30 · headless"

# ── B/C: generated PNGs. Different bytes matter — dedupe is by blake3
# hash, so a byte-flipped fixture would fail decode, not dedupe.
python3 - "$WORK/b.png" "$C_DIR/c.png" <<'EOF'
import struct, sys, zlib
def png(path, rgb, w=64, h=32):
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    def chunk(t, d):
        c = t + d
        return struct.pack(">I", len(d)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw)))
        f.write(chunk(b"IEND", b""))
png(sys.argv[1], (40, 200, 40))
png(sys.argv[2], (200, 40, 40))
EOF

# ── seed: A (fixture) + B via the real CLI, current = A. C's id is
# blake3-deterministic, so it is learned in a throwaway home.
run "$BIN" add "$FIXTURE" --title A >/dev/null
run "$BIN" add "$WORK/b.png" --title B >/dev/null
headless env PIXPAPERS_HOME="$WORK/cseed" "$BIN" add "$C_DIR/c.png" >/dev/null
python3 - "$HOME_" "$WORK/cseed" <<'EOF' > "$WORK/ids"
import json, sys
lib = json.load(open(sys.argv[1] + "/data/library.json"))
seed = json.load(open(sys.argv[2] + "/data/library.json"))
print(next(e["id"] for e in lib["entries"] if e["title"] == "A"))
print(next(e["id"] for e in lib["entries"] if e["title"] == "B"))
print(seed["entries"][0]["id"])
EOF
ID_A=$(sed -n 1p "$WORK/ids")
ID_B=$(sed -n 2p "$WORK/ids")
C_ID=$(sed -n 3p "$WORK/ids")
run "$BIN" set "$ID_A" >/dev/null
CUR=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1] + "/data/library.json"))["current"])' "$HOME_")
[ "$CUR" = "$ID_A" ] || fail "seed: current is $CUR, not A ($ID_A)"
mkdir -p "$DL"
cp "$HOME_/data/library.json" "$WORK/lib_before.json"
cp "$HOME_/config.toml" "$WORK/config_before.toml" 2>/dev/null || : > "$WORK/config_before.toml"
say "seeded A=$ID_A B=$ID_B C=$C_ID (current=A)"

# ── the key burst. Order: c (cycle A→B) · a<path>\r (add C, selection
# jumps to it) · k (select B) · p l b#…\r g40\r t d (B's knobs) · s
# (set B current) · e (toggle enabled) · j (select C) · x (export C) ·
# Dy (remove C — NO space between D and y: the confirm line swallows
# the very next key, a space would cancel) · r (refresh) · q (quit).
# Spaces in normal mode are ignored by the dispatch, so separators are
# cosmetic everywhere else.
KEYS="c a$C_DIR/c.png\r k p l b #00ff00\r g 40\r t d s e j x Dy r q"

if ! headless env PIXPAPERS_HOME="$HOME_" XDG_DOWNLOAD_DIR="$DL" TERM=xterm-256color \
        PTY_RAW="$WORK/pty.raw" \
        python3 "$HERE/tui-pty.py" "$KEYS" -- "$BIN" tui > "$WORK/pty.out" 2>&1; then
    sed -n '1,12p' "$WORK/pty.out"
    fail "pty session exited nonzero (output above)"
fi
sed -n '1p' "$WORK/pty.out"
grep -q "exit=0 " "$WORK/pty.out" || fail "driver did not report a clean exit"

# ── post-exit truth (the assertions are the row) ───────────────────────
python3 - "$HOME_" "$WORK" "$ID_A" "$ID_B" "$C_ID" <<'EOF' || fail "post-exit truth failed"
import json, os, re, sys, tomllib
home, work, id_a, id_b, c_id = sys.argv[1:6]
ANSI = re.compile(
    r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?"
    r"|\x1b[()][A-Za-z0-9]|\x1b[_^][^\x1b\x07]*\x1b\\?")
def chk(cond, msg):
    if not cond:
        print("ASSERT:", msg)
        sys.exit(1)

before = json.load(open(work + "/lib_before.json"))
after = json.load(open(home + "/data/library.json"))
a_a0 = next(e for e in before["entries"] if e["id"] == id_a)
a_b0 = next(e for e in before["entries"] if e["id"] == id_b)
a_a1 = next(e for e in after["entries"] if e["id"] == id_a)
a_b1 = next(e for e in after["entries"] if e["id"] == id_b)

chk(before["current"] == id_a, f"pre: current {before['current']} != A")
chk([e["id"] for e in after["entries"]] == [id_a, id_b],
    f"entries after: {[e['id'] for e in after['entries']]}")
chk(c_id not in [e["id"] for e in after["entries"]], f"C ({c_id}) still in manifest")
chk(after["current"] == id_b, f"current must be B, got {after['current']}")
chk(a_a1 == a_a0, f"A entry must be untouched: {a_a1} vs {a_a0}")
# one cycle step each from the fresh-config defaults (16/fit/auto/none/
# black/24px), bg + gutter set explicitly:
want = {"pixel": "32", "placement": "cover-gutter", "palette": "nes",
        "dither": "fs", "bg_color": "#00ff00", "gutter_px": 40}
got = a_b1.get("settings", {})
for k, v in want.items():
    chk(got.get(k) == v, f"B.settings.{k}: want {v!r}, got {got.get(k)!r}")
chk(set(got) == set(want), f"B.settings keys: {sorted(got)}")
chk(not os.path.exists(home + f"/data/library/{c_id}"), f"C dir still present")

def cfg(path):
    try:
        if os.path.getsize(path) == 0:
            return {}
    except OSError:
        return {}
    return tomllib.load(open(path, "rb"))
e0 = cfg(work + "/config_before.toml").get("enabled", True)
e1 = cfg(home + "/config.toml").get("enabled", True)
chk(e0 is True and e1 is False, f"enabled must flip true→false, got {e0} → {e1}")

# `e` (pixelation off) ran before `x`, so the export honestly reflects
# the originals-only state: pixel resolves to `normal`, not the 16 default.
dls = [f for f in os.listdir(work + "/downloads")
       if f.startswith(f"pixpapers-{c_id}-normal-fit-") and f.endswith(".png")]
chk(len(dls) == 1, f"exactly one C export in downloads, got {dls}")
with open(work + "/downloads/" + dls[0], "rb") as f:
    chk(f.read(8) == b"\x89PNG\r\n\x1a\n", "export is not a PNG")

raw = open(work + "/pty.raw", "rb").read().decode("utf-8", "replace")
stripped = ANSI.sub("", raw)
# ratatui's cell diff only rewrites cells that changed since the last
# frame, so the wire is evidence the status lines rendered — not a
# verbatim transcript (the verbatim strings are pinned by the unit
# tests). The verbs are enough; the ids are proven by the file truth.
chk("added" in stripped, "wire: no 'added' status rendering observed")
chk("removed" in stripped, "wire: no 'removed' status rendering observed")
chk("wrote" in stripped, "wire: no 'wrote' (export) status rendering observed")
print(f"  truth: current {id_a}→{id_b} · B knobs {want} · C gone (dir too) · "
      f"enabled {e0}→{e1} · export {dls[0]} · wire add/remove/export rendered")
EOF

say "exit 0 · alt screen entered and left · all post-exit assertions held"
echo "verify-tui: ok"
