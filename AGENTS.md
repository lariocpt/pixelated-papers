# Agent notes

Conventions for anyone (agent or human) working in this repo. The design
and build history live in [docs/PLAN.md](docs/PLAN.md) and
[docs/plan/step-*.md](docs/plan/); read the relevant step before touching
its area — each step ends in a tested increment and the "why" is there.

## Commands

```sh
cargo build / cargo test --workspace / cargo bench -p pixcore --bench pipeline
cargo clippy --workspace --all-targets -- -D warnings   # must stay green
cargo fmt --check
env -u WAYLAND_DISPLAY test/smoke.sh        # offline layer (CI-safe, deterministic)
WAYLAND_DISPLAY=wayland-1 test/verify-live.sh target/release/pixpapers
WAYLAND_DISPLAY=wayland-1 test/bench-daemon.sh target/release/pixpapers 25
```

Tests: 37 pixcore (config atomicity, manifest version guard, placement
proptests, pipeline goldens, versioned variant cache). The daemon's
IPC/loop logic runs against in-process harnesses — headless is normal.

## Hard rules

- **`pkill -f "pixpapers daemon"` kills your own shell.** Use `pkill -x
  pixpapers`.
- The harness shell has **no `WAYLAND_DISPLAY`**; live scripts require it
  explicitly. Don't "fix" this by exporting it in tests that must be
  headless-deterministic (smoke.sh's offline layer strips it on purpose).
- **Benchmarks run against `--release` only.** Debug timings are
  misleading; `status --bench` counts real renders, nothing else.
- The live dev session is the maintainer's desktop. `test/verify-live.sh`
  is safe (hermetic `PIXPAPERS_HOME`, cleans up). `wlr-randr` mode/scale
  changes are **not** — they silently reset output scale (an incident cost
  the user's scale 1.5). If you must touch outputs, restore the previous
  state in the same command and say so in your summary.
- No `git push`/tags unless the user asked. Commit often, small, with the
  measured evidence in the message when behavior claims ride on it.
- Perf/size claims in docs need a number + its source (which script,
  which commit). If a number disagrees with a claim, fix the claim
  (see docs/PERF.md's walk-backs).

## Dependencies

Discipline over convenience. v0.1 runs on: image, serde(+json), toml,
thiserror, anyhow, clap(+complete), dirs, blake3, tempfile, png,
wayland-client, wayland-protocols(-wlr), libc, log/env_logger. Dev-only:
proptest, snapbox, criterion. `signal-hook` was **removed on purpose**
(its flag handlers silently never fired here — daemon/mod.rs has the raw
sigaction replacement; don't re-add it without re-testing V17). Runtime
deps still need a numbers-based justification; dev-deps just need to be
honest tools.

## Architecture in four lines

The **CLI owns all state** (atomic writes); the **daemon is a pure
renderer** that re-reads config+manifest on every `Apply`. `view` is
always computed from the **original** dimensions; the cached variant
canvas is only ever nearest-stretched into `view.dest`. Exit codes:
1 = user error, 2 = internal, **43 = daemon lost its compositor** (never
restart-on-43).

## Docs are claims

`README.md` and `docs/*.md` are evidence-backed; `docs/VERIFICATION.md`
records what was actually run (and what wasn't). New user-visible
behavior → update CLI.md + CHANGELOG.md + (if claimed) VERIFICATION.md in
the same change.

## Deferred by decision, not by omission

Blurred-bleed gutter variant (plan §10 open question — `cover-gutter`
shipped as mat-margin), TUI/X11/live wallpapers/palettes/auto-preset,
exact fractional buffers, headless-sway CI. Don't "helpfully" implement
these without asking; they're v0.2+ scoped on purpose.
