# Step 7 — Docs & packaging

**Goal**: someone else (or future-you) can install, understand, and trust
this. README that answers the first ten questions, compositor docs with
the niri details front and center, a man page, and a clean `cargo install`
path.

**Depends on**: step 6 (features frozen for v0.1).

---

## 7.1 README.md (front page)

Structure:

1. **One-liner + screenshot wall** (step-3/6 captures: 8/16/32/64/128 strip,
   four placements strip, niri overview shot).
2. What it is: forced-pixelation wallpaper daemon for wayland (niri-first),
   originals kept, variants generated lazily at absolute-minimum size.
3. Quickstart:
   ```sh
   cargo install --path .        # or dist tarball
   pixpapers add ~/Pictures/nebula.png
   pixpapers set <id>            # auto-starts the daemon
   ```
4. Concepts in 10 lines: presets are vibe block sizes (8…128/normal);
   placement modes (fit/cover-gutter/cover-crop/stretch) + `bg_color`;
   per-wallpaper controls; `disable` = pixelation off, originals only;
   "the look" = nearest-neighbor, always crisp blocks.
5. CLI reference (generated from `clap` — `xtask docs` prints markdown;
   committed as `docs/CLI.md`).
6. Config reference (every key, default, one-line meaning).
7. **TROUBLESHOOTING**: no daemon running / niri restarted / stale socket /
   wallpaper not visible (layer conflict with another wallpaper tool — kill
   it) / fractional-scaling block fuzz (→ SCALING.md).
8. v1.1 backlog (what we deliberately didn't do): TUI, X11, live wallpapers,
   auto-preset, palettes/dither "console eras", MPRIS-ish hooks, hyprland
   `hyprpaper` backend option.
9. Why-it's-fast box (R8 summary numbers from step-9 bench).

## 7.2 COMPOSITORS.md

- **niri** (first section, real depth):
  - how background layer surfaces work here; backdrop/overview interaction +
    the **recommended niri config** for the blurred-overview look:
    ```toml
    # niri config — wallpaper shows in overview backdrop; blur is niri's, free
    layer {
      matches = [{ namespace = "^pixelated-papers$" }]
      # example: put our surface into the overview backdrop, ignore zones
      config {
        column-width = 0   # n/a — see notes; keep defaults
      }
    }
    ```
    (step 8 verifies the exact stanza for the shipping niri version — docs
    state the tested version explicitly)
  - recommended `bindm`/autostart: either systemd unit (step 5) or
    `spawn = pixpapers daemon` in config — with the anti-flap note.
- **sway**, **hyprland** (smoke-level): works via same protocol; hyprland
  note about its own wallpaper daemon (`hyprpaper`) conflict — pick one;
  layer rules pointer.
- Generic wlroots table: layer Background + exclusive-zone-0 expectations.
- X11: explicitly "not supported, use `feh --bg-fill` + `pixpapers export`"
  — with a worked example (export is the universal escape hatch: any tool
  that takes a file can use our pixelated render).

## 7.3 Man page + shell completions

- `docs/man/pixpapers.1.md` (roff via `md2man` in `xtask`, or hand-rolled
  mmd) → installed by packagers; covers synopsis of every subcommand, exit
  codes (0/1/2/3 semantics from steps 1/5), env vars (`PIXPAPERS_HOME`,
  `WAYLAND_DISPLAY`, `RUST_LOG`).
- `clap_complete` generation: `pixpapers completions <bash|zsh|fish>` hidden
  subcommand + `_pixpapers` files under `contrib/`.

## 7.4 Install & distribution

- `cargo install --path .` works from a clean clone (integration test in CI).
- Dist: `cargo-dist` vs hand-rolled release assets — **decision this step**,
  bias: hand-rolled GitHub release with `x86_64-linux` tarball + SHA256SUMS
  via a release workflow (fewer moving parts for one maintainer).
- Arch: PKGBUILD draft in `contrib/packaging/` (source = repo tag, no AUR
  publish in this step — packaging-while-tagging is how projects die).
- Nix flake: skip (backlog), but keep `Cargo.lock` + no git-deps so
  packagers never suffer.

## 7.5 CI completion (builds on step 1's trio)

```yaml
- fmt, clippy(-D warnings), test (ubuntu-latest + pinned toolchain)
- build matrix: debug, release (no-run)
- integration: mock-wayland suites (already headless)
- docs job: xtask docs check (CLI.md drift), mdbook-free (plain md links check)
- release workflow (tag v*): build release, checksums, GitHub release draft
```

## 7.6 Repo polish

- LICENSE (pick one — **MIT OR Apache-2.0** default unless user objects;
  noted in PLAN §10), CONTRIBUTING-lite (how steps/commits map to this plan),
  issue template ("compositor + version + `pixpapers status` output").
- `rust-toolchain.toml` pins tested stable; `deny.toml` (cargo-deny: bans +
  licenses + minimal advisories) wired into CI (soft-fail first, hard after
  first green).
- Git history: steps were committed as `step-N: …` (kept in history — the
  plan files explain each; no force-rebase required).

## 7.7 Acceptance criteria

1. Fresh VM/container: follow README quickstart verbatim → works (niri) —
   dry-run on the dev machine in a clean user.
2. `man docs/man/pixpapers.1` renders; every subcommand + flag present and
   matching `--help`.
3. COMPOSITORS.md niri stanza copy-pasteable and **tested** (screenshot).
4. CI green on the full suite incl. docs-drift check.
5. `cargo install --path .` from clean clone produces a working binary
   (CI job proves).
6. LICENSE/CONTRIBUTING present; backlog lists exactly the deferred items
   from PLAN §1/§10.

**Commit**: `step-7: README, COMPOSITORS/SCALING docs, man page, completions, dist + CI completion`
