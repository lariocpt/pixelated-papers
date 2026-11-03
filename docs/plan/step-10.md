# Step 10 — MPRIS-ish media control (A/B decision + the B increment)

**Goal**: the slideshow should be controllable/observable from a desktop
media-key setup (transport keys, a playerctl-style client, a small media
widget) without a user typing `pixpapers cycle 1`. Resolve the two competing
readings of "MPRIS-ish", state which ships and why in
[AGENTS.md](../../AGENTS.md) dependency-discipline terms, and record the
measurement that settled it. This step closes the MPRIS item parked in
[step-7.md](step-7.md) §7.1(8).

**Depends on**: the slideshow (`pixpapers slideshow`, round-30) and the
additive `status` fields (`slideshow_enabled` / `next_change`, `ad1f8d5`).

---

## The two interpretations

- **A — a real D-Bus MPRIS 2.0 service.** A `zbus`-backed player object on the
  session bus so any MPRIS client (GNOME/KDE media widget, `playerctl`,
  `playerctld`) sees pixpapers as a media player: title, a cover, Play/Pause/
  Next/Previous over the bus, `PropertiesChanged` signals.
- **B — a transport-agnostic command surface.** The media verbs as first-class
  CLI commands (`next`/`prev`/`play`/`pause`) plus the read-only "track N of M"
  metadata in `status`, so a media-key daemon binds transport keys straight to
  them. No bus, no new dependency.

They are not mutually exclusive — A would build on B's clean command surface.
**The outcome is B only.** A stays deferred, *measured not hand-waved*.

## Why B and not A — the numbers

A throwaway probe added `zbus = "5"` behind an off-by-default `mpris` feature
with a reachable session-connection code path, built `--release` on this repo's
`lto = true` / `codegen-units = 1` profile (HEAD `36c8db6`, MSRV `1.85`).
Every number below is from that build; the probe is deleted, not shipped.

| Cost of shipping A (real MPRIS service) | Measured |
| --- | --- |
| Dependency-tree size | **289 → 429 nodes** (`+140`); ~135 new unique packages (`zbus`, `zvariant`, `zbus_names`, `enumflags2`, `ordered-stream`, `uuid`, `tracing`, …) |
| Release-binary size | **+1,061,056 B (+1.01 MiB, +20%)** — 5,145,752 → 6,206,808 B — for *only a single connection probe*; an object-server with properties and signals is strictly larger |
| Feature-off invariance | control build with `zbus` in `Cargo.lock` + probe source, feature **off** = `4f9bb498…`, **byte-identical** to the baseline binary → the ON delta above is purely zbus, no shared-dep drift |
| MSRV | `zbus 5.13.2` requires **Rust 1.87**; the project floor is **1.85** → adopting A raises the declared MSRV by two versions |
| Runtime shape | pulls the **async-io / async-executor / futures-lite / event-listener** reactor. The daemon is a single-threaded synchronous `poll(2)` loop with an explicit no-tokio posture (see the `gui`/`tui` feature notes in `Cargo.toml`); the `zbus::blocking` facade runs on that async reactor regardless, so A would embed a foreign event loop in the renderer |

Against that, **B covers the stated intent at zero runtime-dependency cost**:
every media verb is *already* a pure, state-owning CLI command
(`cycle ±1`, `slideshow on/off`), so the only genuinely missing piece was a
discoverable named verb + observable position, both additive. A real MPRIS
service buys one thing B cannot — a cover thumbnail inside a GNOME/KDE widget —
and this project renders *still* wallpapers with no video/cover pipeline, so
that payoff is outside its scope ("no video-decoder dependency", AGENTS.md).
The desktop-integration claim in the README is therefore stated honestly:
**command-surface media control, not a D-Bus media player.**

## What B ships (this increment)

1. **Named media verbs** (`cli.rs` `Cmd::{Next,Prev,Play,Pause}`) that route
   through the *exact same* `commands::cycle_current` / `commands::set_slideshow`
   + `cli::apply_now` seam as `cycle ±1` / `slideshow on/off`. They are aliases,
   not parallel implementations, so the resulting state is byte-identical.
2. **Read-only position metadata** in `StatusReport`
   (`ipc.rs` `current_index` / `library_len`), additive `skip_serializing_if`
   fields following the `slideshow_enabled`/`backend` precedent: a pre-feature
   daemon omits both keys (wire bytes unchanged), an old client ignores them, a
   new client reading an old report sees `None`. Populated on **both** sides
   (daemon `status_report` and CLI `local_report`), so `status` works with or
   without a live daemon. Human status shows a `position: N / M` line only when
   a current entry is in the library.

## Architecture invariants kept

- **Daemon stays a pure renderer.** The new status fields are pure observation
  (the daemon re-reads the manifest to render anyway); there is no in-daemon
  pause/position flag. `play`/`pause` persist the *slideshow enabled* state via
  the state-owning CLI path exactly like `slideshow on/off` — media "pause" is
  `slideshow off`, never a daemon-local toggle.
- **CLI owns all state; `apply_now`/auto-spawn unchanged.** The aliases call
  the same seams; mutating commands still auto-spawn, knob-style commands still
  don't. `exit 43` (daemon lost its compositor) untouched — nothing here changes
  the render/exit path.
- **Play-after-Pause resumes by the clock, no epoch.** The active index is
  `(unix_now / interval) % len` (a pure function of the wall clock,
  `pixcore::slideshow`). Turning the slideshow back on does not reset a start
  time; the next change is the next clock boundary. `tests/media_keys.rs`
  asserts a play→pause→play cycle only ever flips `enabled` and never writes
  any epoch/resume field into `[slideshow]`.
- **JSON-lines is backward-compatible.** Additive optional fields only; no
  `Request`/`Response` shape change, no version bump.

## Tests & proofs (`crates/pixpapers/tests/media_keys.rs`, hermetic `PIXPAPERS_HOME`)

- `next` / `prev` leave a manifest **byte-identical** to `cycle 1` / `cycle -1`
  from the same seed; wraparound matches `cycle` at both ends.
- `play` / `pause` leave `config.toml` **byte-identical** to `slideshow on` /
  `slideshow off`; `play_pause_only_flips_the_enabled_flag` proves the pure-clock
  model (only `enabled` changes; no epoch key survives).
- `status --json` reports `current_index`/`library_len`; `current_index` is
  absent (not `0`) when no entry is current — the degraded shape.
- `next` on an empty library fails with the **same exit code and error text** as
  `cycle` (shared `cycle_current`) — stale/degraded behaves like the command it
  aliases.
- `ipc::tests` round-trip the two new fields and prove a hand-written
  pre-feature report deserializes to `None`/`None` and serializes without the
  keys (wire-contract guarantee).

## Gate matrix run (B, this change)

`cargo build` → `cargo test --workspace` (**210** = 116 pixcore + 74 bin +
10 slideshow + 10 media_keys) → `cargo test --features gui` (**215**, +5
`gui::model`) → `cargo clippy --workspace --all-targets --all-features -- -D
warnings` (clean) → `cargo fmt --all --check` (clean) → `env -u WAYLAND_DISPLAY
test/smoke.sh`, `test/verify-x11.sh`, `test/verify-tui.sh` (10 goldens; the new
top-level verbs do **not** enter the TUI keymap, so no B-knob leak).

## What was *not* run (honesty)

- **No live media-key demo and no D-Bus transcript.** B is not a bus service, so
  there is no `playerctl` output to show — claiming one would be a fake. The
  desktop recipe is documented (README/CLI.md): a key binder maps transport keys
  to the commands; observation is `pixpapers status --json`.
- The A-probe numbers are from a throwaway checkout; the `zbus` dependency is
  **not** in this repo's tree (A did not ship).

## Deferred, unchanged

Interpretation A (real MPRIS service over a bus) stays deferred — now on the
measured record, not by omission. Revisit only if a bus-backed media player
becomes a real goal *and* the +1 MiB / MSRV-1.87 / async-runtime costs above are
accepted by decision.
