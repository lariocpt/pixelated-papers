# Contributing (lite)

This repo was built as nine steps, each ending in a tested increment —
[docs/PLAN.md](docs/PLAN.md) is the map and [docs/plan/](docs/plan/) has
the per-step design. Read the step that owns the area you're touching;
the *why* is there.

- Branch off `main`; one concern per PR; commit style: `area: summary`
  (see git log).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test` green before review; CI enforces.
- New user-visible behavior → update `docs/CLI.md`, `CHANGELOG.md`, and
  if it backs a claim, `docs/VERIFICATION.md` — docs are evidence here.
- Perf/size claims in docs need the measuring script + numbers (AGENTS.md
  "Hard rules" applies to humans too).
- New runtime dependencies: say what they replace and why (v0.1 shipped
  with signal-hook *removed* based on measurement, not vibes).
- Bugs: compositor + version, `pixpapers status` (+ `--bench`), output
  mode/scale, layer info (`niri msg layers`). Multi-monitor reports
  especially welcome (see VERIFICATION V4/V5).
