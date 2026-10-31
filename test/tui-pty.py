#!/usr/bin/env python3
"""Drive a child on a real pty: fixed winsize, one key burst, wire capture.

Committed form of the S6 live-row driver (V-T0/V-T0b ran the ad-hoc copy);
S7's V-T5 row drives the TUI action keys through it.

usage: tui-pty.py <keys> -- <cmd...>

<keys> is ASCII with backslash escapes: \r = Enter, \x1b = Esc, \t, \\
= backslash; everything else is literal. The burst is sent once output
arrives (proof the TUI is up and in raw mode), or after the grace
period (PTY_GRACE, default 2 s) whichever comes first — the pty input
buffer makes an early burst safe anyway. The environment passes through
unchanged: the caller scrubs what must be scrubbed (image env vars,
DISPLAY, WAYLAND_DISPLAY).

Exits with the child's exit code (0 = clean quit). A child still alive
after the deadline (PTY_DEADLINE, default 15 s) is SIGKILLed and the
script exits nonzero — a hung session is a failed row, not a silent
pass. Prints the exit code, raw size, alternate-screen flags and the
last 40 ANSI-stripped lines; PTY_RAW=<path> dumps the raw wire.
"""
import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time

ANSI_RE = re.compile(
    r"\x1b\[[0-9;?]*[A-Za-z]"
    r"|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?"
    r"|\x1b[()][A-Za-z0-9]"
    r"|\x1b[_^][^\x1b\x07]*\x1b\\?"
)


def unescape(s: str) -> bytes:
    s = re.sub(r"\\x([0-9a-fA-F]{2})", lambda m: chr(int(m.group(1), 16)), s)
    for esc, ch in [("\\r", "\r"), ("\\n", "\n"), ("\\t", "\t"), ("\\\\", "\\")]:
        s = s.replace(esc, ch)
    return s.encode()


def main() -> int:
    if len(sys.argv) < 4 or sys.argv[2] != "--":
        print(__doc__)
        return 2
    keys = unescape(sys.argv[1])
    cmd = sys.argv[3:]
    rows = int(os.environ.get("PTY_ROWS", "30"))
    cols = int(os.environ.get("PTY_COLS", "100"))
    grace = float(os.environ.get("PTY_GRACE", "2"))
    deadline = float(os.environ.get("PTY_DEADLINE", "15"))

    pid, fd = pty.fork()
    if pid == 0:
        # size the slave before exec, so the child's first size() sees it
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.execve(cmd[0], cmd, dict(os.environ))

    out = b""
    sent = False
    saw_output = False
    t0 = time.time()
    while time.time() - t0 < deadline:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try:
                d = os.read(fd, 65536)
            except OSError:
                break
            if not d:
                break
            out += d
            saw_output = True
        if not sent and (saw_output or time.time() - t0 > grace):
            os.write(fd, keys)
            sent = True
    if not sent:
        os.write(fd, keys)

    try:
        pid_done, status = os.waitpid(pid, os.WNOHANG)
        if pid_done == 0:
            os.kill(pid, 9)
            _, status = os.waitpid(pid, 0)
        code = os.waitstatus_to_exitcode(status)
    except ChildProcessError:
        code = 1

    if os.environ.get("PTY_RAW"):
        with open(os.environ["PTY_RAW"], "wb") as f:
            f.write(out)
    data = out.decode("utf-8", "replace")
    print(f"=== exit={code} raw={len(out)}B alt-in={'1049h' in data} "
          f"alt-out={'1049l' in data} sent={sent} ===")
    lines = [l.rstrip() for l in ANSI_RE.sub("", data).splitlines()]
    for l in lines[-40:]:
        print(l)
    return code if code >= 0 else 1


if __name__ == "__main__":
    sys.exit(main())
