# Step 5 — Robustness: auto-spawn, sockets, crash recovery, systemd

**Goal**: the daemon is invisible infrastructure. `pixpapers set <id>` on a
cold machine (no daemon) just *works*; crashes, stale sockets, niri restarts,
and reboots all self-heal. Log hygiene good enough to debug from a month
later.

**Depends on**: step 4.
**Requirements covered**: operational side of everything; R9 (niri restart
behavior).

---

## 5.1 Auto-spawn — `daemon/spawn.rs`

`connect_or_spawn()` on the client side:

1. `connect(sock)`: success → use it.
2. `ENOENT`/`ECONNREFUSED` (stale or absent) → validate-and-clean:
   - if `daemon.pid` exists and `/proc/<pid>` matches a live *pixpapers*
     process (exe + cmdline check, not just pid reuse) → report "daemon
     starting, retry in a moment" (exit 3, retryable — CLI retries 3× with
     50 ms backoff internally before surfacing);
   - else: `unlink(sock)`, `unlink stale pid`, `fork` the current exe
     (`std::process::Command::new(current_exe).arg("daemon").arg("--spawned")`)
     with `setsid`-style detachment (`process_group(0)`), stdout/err →
     `daemon.log` (append), then poll connect ≤ 2 s.
3. Spawn race: two `set`s at once → both may spawn; the second's `bind()`
   fails `EADDRINUSE` → it must **exit 0 silently** (another daemon won).
   pidfile write is O_EXCL|O_CREAT — losers exit.
4. `--spawned` mode: no tty assumptions, env check (`WAYLAND_DISPLAY`
   missing → log + exit 1; client surfaces "no wayland session" clearly).

## 5.2 Crash & restart semantics

- **Daemon crash (kill -9)**: wayland drops → niri removes surfaces (desktop
  shows backdrop — acceptable, documented). Next `set` respawns and
  re-applies from `manifest.current` **plus** the new command (state replay:
  on start, daemon renders `current` before accepting IPC — no "daemon
  running but blank" window).
- **Niri restart**: daemon's wayland connection dies → daemon exits with
  code 43 (sentinel, logged). Next CLI `set` respawns. `pixpapers status`
  pre-respawn says "daemon dead (wayland gone)". No zombie loop: respawn is
  CLI-triggered, never self-triggered (prevents tight loop if niri flapping —
  step-6 stress verifies).
- **Start-time validation ordering**: load config → load library → connect
  wayland → bind socket (fail-fast on bad config, clear log, exit 1; never
  half-bound socket).
- **Socket permission check on connect**: if an existing socket is owned by
  another uid (weird multi-user `/tmp` setups), refuse with clear error
  (security, R-hygiene).

## 5.3 Logging

- `tracing` → `daemon.log` (append, `RUST_LOG`-style filter via config
  `[daemon] log = "info"`), + `--foreground` mirrors stderr.
- `logrotate`-free size cap: daemon checks own log size on startup; > 1 MiB →
  rotate to `daemon.log.1` (keep 1).
- Log lines to include: startup banner (version, profile paths), per-output
  geometry events, per-render timing (DEBUG), IPC command (INFO, no payloads
  beyond ids), errors with cause chain (anyhow at bin boundary — pixcore
  stays `thiserror`).

## 5.4 systemd user unit (optional, documented, not required)

`contrib/systemd/user/pixpapers.service`:

```ini
[Unit]
Description=pixelated-papers wallpaper daemon
PartOf=graphical-session.target
After=graphical-session.target
ReloadPropagatedFrom=graphical-session.target   # niri restart → restart daemon

[Service]
Type=simple
ExecStart=pixpapers daemon
Restart=on-failure
RestartMaxDelaySec=10s   # or StartBurst limits — anti-flap
Environment=RUST_LOG=info

[Install]
WantedBy=graphical-session.target
```

- `systemctl --user enable pixpapers` = the "always on" option; CLI still
  works identically (daemon already running → plain IPC path).
- `pixpapers service enable|disable|status` helper (idempotent: writes unit
  drop-in dir, `daemon-reload`, enable/start). Kept in contrib + helper — if
  unit-file generation fights us, ship the .service file + docs only
  (decision point inside step, low risk).
- Interaction rule: manual daemon **and** unit both present → bind race
  (§5.1.3) resolves it silently; documented.

## 5.5 `status` (full)

```
daemon: running pid 4242 (spawned tty?, uptime 2h)  wayland: niri 26.x
policy: enabled=true  default=16/fit
current: a1b2c3 "mountain" pixel=16 placement=fit bg=#000 gutter=24
outputs:
  DP-1   3840x2160@144 scale 1.0   variant 16-fit 240x135 (8 KB, hit)  
  eDP-1  1920x1080@60  scale 1.0   variant 16-fit 120x67 (5 KB, hit)
cache:   4 files, 31 KB   hits 87%  last_switch 12 ms
```

`--json` mirrors exactly; `--bench` arrives step 9.

## 5.6 Tests

- **spawn integration** (real processes, `PIXPAPERS_HOME` tmp, headless via a
  `--mock-wayland` daemon mode — renders into `OutState` recorder, no
  compositor needed): cold spawn, double-spawn race (2 concurrent → exactly
  1 live daemon), stale socket + dead pidfile → recovery, live pidfile +
  dead socket (pid reused by fake sleep process → treat via exe check),
  connect after daemon exit sentinel(43).
- Replay-on-start: manifest.current set, daemon start (mock) → asserts
  render applied before accepting first IPC command (order asserted via
  recorder sequence).
- Log rotation on startup; status output golden (mock daemon) with/without
  `--json`.
- systemd unit: shellcheck-free static lint (`systemd-analyze verify` when
  available in CI — soft check); helper subcommands unit-tested against a
  fake `systemctl` on PATH.

## 5.7 Acceptance criteria

1. Cold machine sim: `PIXPAPERS_HOME=tmp pixpapers set <id>` with no daemon
   → daemon appears, wallpaper applies, second `set` IPC-fast — all without
   any manual `daemon` invocation (mock-wayland test + real niri check).
2. `kill -9` daemon → `set` recovers within ≤ 1 s including re-render.
3. Niri restart on real machine: daemon exits with 43, no respawn loop, next
   `set` works; documented in TROUBLESHOOTING section of README.
4. systemd unit: `systemctl --user start pixpapers` + CLI interop verified;
   anti-flap (`Restart=` limits) observed with a crashing fake.
5. `status` output matches reality (spot-checked against `niri msg outputs`
   and filesystem).
6. All §5.6 tests green; `cargo clippy -D warnings` clean.

**Commit**: `step-5: auto-spawn, crash recovery, log rotation, systemd unit, full status`
