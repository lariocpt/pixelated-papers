# SESSION-PROMPTS — v0.2 step prompts (companion to PLAN-30-ct.md)

One session per step, in order. Each prompt below is self-contained: paste it
wholesale into a fresh session of this repo. Every step ends with the
cross-cutting gate green and one or more small commits; nothing here may start
the next step's work. If a step's acceptance criterion can't be met, stop and
report — a plan change is a plan edit, not a step hack.

Cross-cutting gate after **every** step:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
env -u WAYLAND_DISPLAY test/smoke.sh
```

Benchmarks are `--release` only. No `git push`. Commit small and often; measured
evidence in the message when a behavior/size claim rides on it.

---

## S1 — Palette & dither model + pipeline (pixcore)

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S1 of
PLAN-30-ct.md (v0.2 workstream A). Before writing code, read, in order:

  AGENTS.md
  PLAN-30-ct.md          — §1 (invariants) and §2, especially A.1, A.2 and the A.4
                           test list (A.4's bench half is S3, not you)
  crates/pixcore/src/pipeline.rs   — downscale/median_cut/apply_palette; the "no dither"
                                     comment is the status quo you are changing
  crates/pixcore/src/variant.rs    — generate_from_image, the variant cache-key scheme,
                                     prune_variants
  crates/pixcore/src/config.rs     — Config; the legacy TOML `palette` bool lives here
  crates/pixcore/src/manifest.rs   — Settings/Entry/Library; new per-wallpaper knobs land here
  crates/pixcore/src/render.rs     — ResolvedSettings
  docs/plan/ (the step docs for the pipeline/variant area)

TASK — build exactly the A.1+A.2 model and pipeline, nothing else:

1. New pixcore types (serdable, lowercase strings):
     PaletteMode { Off, Auto, Nes, Gb, Cga, Ega, Vga, Gen16 }   — "off"|"auto"|"nes"|"gb"|"cga"|"ega"|"vga"|"gen16"
     Dither        { None, FloydSteinberg, Atkinson, Bayer2, Bayer4, Bayer8 }
                                                           — "none"|"fs"|"atkinson"|"bayer2"|"bayer4"|"bayer8"
2. Static palette tables in a new pixcore palette module:
     nes 192 (standard NES master table — per-sprite 4-color subsets are NOT emulated),
     gb 4 (DMG greens), cga 16, ega 64 (classic 4x4x4), vga 256 (standard VGA palette).
   Gen16 is not a table: median_cut to 16, snapped onto the 15-bit (RGB555) grid.
3. pipeline::quantize(canvas, mode, dither) — pure, deterministic, called from
   variant::generate_from_image between downscale and encode:
     Off  -> identity
     Auto -> existing median_cut(256) + apply_palette  (byte-identical to v0.1 when dither = none)
     named-> apply_palette on the fixed table; Gen16 -> median_cut(16) snapped RGB555
     then, if dither != None, dither in canvas space:
       fs/atkinson: error diffusion, u16 integer accumulators over R,G,B, row-major;
       bayer2/4/8: ordered matrices, threshold by (x & (n-1)), (y & (n-1)).
   No floats in the dither paths; determinism is a contract.
4. Config gains: palette: PaletteMode (default Auto) and dither: Dither (default None).
   Config.palette keeps a custom Deserialize that ALSO accepts the legacy TOML bool
   (true -> Auto, false -> Off); new saves serialize the string form.
   Manifest Settings gains palette: Option<PaletteMode>, dither: Option<Dither>
   (None = inherit; skip_serializing_if like the other knobs) — old manifests must
   parse unchanged.
5. ResolvedSettings resolves entry -> config for both knobs; the unpixelated()
   (R6) path is untouched.
6. Variant cache key: filename becomes
       v{VER}-{pixel}-{placement}[-g{g}]-{W}x{H}-{Q}[-{D}].png
   Q in {off,auto,nes,gb,cga,ega,vga,gen16}, D omitted when none.
   Bump PIPELINE_VERSION 1 -> 2. prune_variants already reclaims old files on
   set/remove — no migration code.
7. Encode: named/auto keep the existing palette-PNG path; a named table encodes
   with exactly that table's entries; Off stays RGBA.

DO NOT TOUCH in this step: any cli.rs/commands.rs surface (S2), the daemon
(anything in B), anything in contrib/, docs/VERIFICATION.md.

ACCEPTANCE — all must hold before you call it done:
- All A.4 unit tests green: table sizes + spot-checked RGB rows (GB greens,
  NES row 0, VGA #FF0000); quantize output colors subset of the chosen palette
  (proptest over all modes); FS/Atkinson mean-color drift bound on a ramp;
  byte determinism (two runs -> equal bytes); Gen16 output subset of RGB555
  grid; legacy TOML palette=true/false deserializes to Auto/Off; distinct
  (Q,D) pairs -> distinct cache paths (proptest); Auto+None byte-identical to
  the v0.1 pipeline on the shared testsrc sources.
- Goldens: one pipeline golden per palette x dither cell (12 cells) on
  testsrc::photo128x80, checked in.
- Cross-cutting gate green.
- Commits small (e.g. "pixcore: palette/dither model + tables", "pixcore:
  quantize stage", "pixcore: cache key v2").

When done, print: a diffstat, the list of new/changed pixcore public items, and
the golden files you added.
```

---

## S2 — Palette/dither CLI knobs

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S2 of
PLAN-30-ct.md (v0.2 workstream A, CLI half). S1 is already merged: the pixcore
model (PaletteMode/Dither, quantize stage, cache key v2) exists. Before writing
code, read, in order:

  AGENTS.md
  PLAN-30-ct.md          — §1, §2 A.3, and the S2 row of the §6 step table
  crates/pixpapers/src/cli.rs      — clap surface + existing knob commands (pixel/placement/
                                     bg/gutter) to mirror
  crates/pixpapers/src/commands.rs — the state-mutating functions (edit_entry, export, ...);
                                     the TUI and waybar later reuse these unchanged
  crates/pixcore/src/config.rs     — the default knobs you're editing
  docs/CLI.md                  — command docs to extend (docs are claims)

TASK — expose the S1 model through the CLI, mirroring the existing knob
commands' shape and behavior contract exactly:

  pixpapers palette <id> <off|auto|nes|gb|cga|ega|vga|gen16>
  pixpapers dither  <id> <none|fs|atkinson|bayer2|bayer4|bayer8>
  pixpapers default palette <mode>     # config default for new/unset entries
  pixpapers default dither  <algo>
  export gains: --palette <mode>  --dither <algo>  (override, not persisted)

- Per-knob behavior contract, same as the existing knobs: editing a knob on the
  CURRENT entry re-applies immediately (Apply IPC, auto-spawn daemon exactly
  like the existing commands); on a non-current entry it just records.
- list/status show per-entry palette+dither like they show pixel/placement.
- The `default` subcommands edit the config defaults; entries keep None=inherit.
- All mutations go through the existing commands::* pattern (atomic writes);
  no new IPC surface, no daemon changes.

DO NOT TOUCH: pixcore (if you find a bug there, fix it as a separate small
commit with its own message, say so in your summary); the daemon;
docs/VERIFICATION.md; bench numbers (S3).

ACCEPTANCE:
- snapbox CLI round-trips in a temp PIXPAPERS_HOME: add a fixture image, set
  each palette value and each dither value, verify manifest bytes; default
  commands verify config bytes; both serialize the string forms.
- Harness-socket test: editing the current entry sends an Apply request
  (same pattern the existing knob tests use).
- `export --palette vga --dither fs` produces a palette PNG whose palette table
  is exactly the VGA table (entry count + sampled rows) and whose pixels are
  within it; `--palette off` produces RGBA.
- docs/CLI.md updated for all five surfaces in the same change; CHANGELOG.md
  [Unreleased] entry.
- Cross-cutting gate green.
- Commits small.

When done, print: the new clap surface (help text), the CLI.md diff summary,
and the test names you added.
```

---

## S3 — Bench + perf/size docs (workstream A close-out)

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S3 of
PLAN-30-ct.md — the measured-evidence close-out for workstream A. S1 (pipeline)
and S2 (CLI) are merged. Before starting, read:

  AGENTS.md                  — "Docs are claims", benchmarks --release only
  PLAN-30-ct.md              — §2 A.4 (the bench bullet list) and the S3 row of §6
  docs/PERF.md               — existing bench layout, the walk-back rule
  README.md                  — the size table this step updates
  test/bench-daemon.sh       — how benches are scripted in this repo

TASK — measure, then document. No behavior changes, no new user-visible
anything; if a number forces a code change, stop and report instead.

Bench (release only, scripted so the numbers are reproducible — put the
scripts under test/ with the repo's script style, e.g. test/bench-dither.sh):
- FS and bayer8 dither cost on the 120x68 and 240x135 canvases
  (the 1080p@16 and 4K@8 shapes).
- normal + vga + fs on a 1920x1080 canvas (the one-time full-res case).
- Variant byte sizes for the 1080p and 4K test fixtures: v0.1 auto-palette
  baseline vs the v2 default path (auto, dither none) vs a couple of
  representative named palettes. The v0.1 baseline is measured from the v0.1
  code (git worktree or stash the S1 commits temporarily — measure both,
  restore state cleanly).

DOCUMENT:
- docs/PERF.md: new section with every number above, the script name and the
  commit for each; apply the walk-back rule (if a number contradicts an
  existing claim, fix the claim).
- README.md size table: updated with the new default path and the named-
  palette deltas, with the script/commit source line.
- CHANGELOG.md [Unreleased]: "measured" bullet referencing PERF.md.

If any number blows a budget (e.g. >1 s one-time at 4K, or a named palette
that bloats variant size badly): document it with the number, per the plan's
risk row — do not silently skip or "fix" the pipeline.

ACCEPTANCE:
- Every number in the new docs has script + commit; rerunning the scripts
  reproduces the numbers within run-to-run noise.
- No source changes (test/ scripts and docs/ only; a test-only helper is
  allowed, clearly named).
- Cross-cutting gate green.
- Commits small: "test: bench-dither script", "docs: PERF dither section",
  "docs: README size table v2".

When done, print: the full numbers table as it landed in PERF.md.
```

---

## S4 — Daemon backend seam (refactor, zero behavior change)

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S4 of
PLAN-30-ct.md (v0.2 workstream B, the refactor half). This is a pure move+reshape:
the daemon must be behavior-identical before and after. Before touching anything,
read, in order:

  AGENTS.md                  — the four-line architecture, exit codes, hard rules
  PLAN-30-ct.md              — §1, §3 (B.1 in full, B.3 for the config shape it
                               will host), and the S4 row of §6
  crates/pixpapers/src/daemon/mod.rs    — the supervisor loop: poll(2) over socket,
                                           raw sigaction signals, dispatch_pending
                                           trap, stale-socket handling, exit-43 path
  crates/pixpapers/src/daemon/wayland.rs — everything that is display-specific today

TASK — introduce the backend seam exactly as B.1 specifies, nothing more:

1. New module tree:
     crates/pixpapers/src/daemon/mod.rs          — backend-agnostic supervisor
     crates/pixpapers/src/daemon/backend/mod.rs  — pub trait Backend
     crates/pixpapers/src/daemon/backend/layer.rs — today's wayland.rs, moved+reshaped
   (If the code is organized so a top-level daemon/wayland.rs is referenced from
   elsewhere, keep the move clean; delete the old file, don't leave a re-export
   shim.)
2. The trait, deliberately small (see B.1):
     name() / poll_fd() -> Option<RawFd> (None => supervisor polls only the
     socket) / pump() / render_all(paths) / snapshot() -> Vec<OutputStatus> /
     connection_lost() -> bool (true => exit 43) / close()
3. The supervisor keeps its EXACT contract: poll over [backend fd, socket fd]
   + SIGTERM/SIGINT; Apply -> render_all; teardown via close before
   socket/pid cleanup; exit 43 on connection_lost; the unit-file
   RestartPreventExitStatus=43 note unchanged.
4. The dispatch_pending trap, the raw sigaction setup, stale-socket handling
   and every log line stay as-is — they are loop-level, not backend-level.

FORBIDDEN in this step: the x11 backend or any x11rb dependency (S5); the
daemon.backend config key and --backend flag (S5); the StatusReport backend
field (S5); any change to pixcore; any "while I'm here" fix to the loop —
if you find a real bug, note it in your summary instead.

ACCEPTANCE — the gate is the live suite, run on the real niri session:
- WAYLAND_DISPLAY=wayland-1 test/verify-live.sh target/release/pixpapers green.
- WAYLAND_DISPLAY=wayland-1 test/bench-daemon.sh target/release/pixpapers 25
  green, with switch latencies within run-to-run noise of the pre-refactor
  numbers (capture both runs; if you didn't capture the pre-refactor run,
  capture it now from the parent commit in a worktree).
- The V-matrix is re-run and recorded in docs/VERIFICATION.md (a move+reshape
  step is the one that must prove boring).
- cargo clippy/fmt green; offline smoke green.
- The diff, read top to bottom, is move+reshape only.

Commit small: e.g. "daemon: backend seam — trait + layer move (no behavior
change)". In the commit message: the live numbers (before/after) and that
verify-live.sh re-ran.

When done, print: the trait definition as merged, the mod.rs poll-loop hunk,
and the live bench numbers before/after.
```

---

## S5 — X11 backend + auto-detect

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S5 of
PLAN-30-ct.md (v0.2 workstream B, the x11 half). S4 (the backend seam) is merged:
daemon/backend/{mod,layer}.rs exist and the supervisor is backend-agnostic.
Before writing code, read, in order:

  AGENTS.md
  PLAN-30-ct.md              — §3 B.2, B.3, B.4 in full, and the S5 row of §6
  crates/pixpapers/src/daemon/backend/mod.rs  — the trait you implement
  crates/pixpapers/src/daemon/backend/layer.rs — the pattern to mirror
  crates/pixcore/src/render.rs — composite_into: the SAME composite the layer
                                 backend uses (invariant #2), into a client-side
                                 buffer here
  docs/COMPOSITORS.md        — the "X11 — not supported" section this step
                               eventually replaces (the docs write-up itself is
                               S9; you may leave a TODO note, not the section)

TASK — implement the x11 backend per B.2:

1. Dependency: x11rb (pure-Rust XCB; pollable fd fits the existing poll(2)
   loop). Register it as an OPTIONAL feature of pixpapers named `x11`, on by
   default (AGENTS.md dependency-discipline row: X11 is the requirement;
   x11rb is the only maintained pure-Rust option; ~20 fns of API surface).
   Record the binary-size delta in the commit message (measure with
   cargo build --release with and without the feature).
2. Backend behavior (one daemon renders the same wallpaper on all screens,
   mirroring the Wayland multi-output behavior):
   - size from the root window of each screen (QueryScreens);
   - buffer = full-root XRGB8888, native byte order; a non-24-depth root is a
     LOUD user error, exit 1 with a clear message — tested, not hoped for;
   - render: composite_into into a client-side buffer, then PutImage to the
     root. No Xv, no GPU — v0.2 does not depend on Xv.
   - events on the x11rb fd: root Expose -> re-render; ClientMessage RFPWM /
     RRC -> re-render at the new size (we react, we never initiate);
     connection error / ConnectionClosed -> connection_lost() == true => exit
     43, identical contract to the Wayland path.
   - No RFPWM emission.
3. Selection per B.3:
   - Config.daemon.backend: "auto" | "layer" | "x11" (default auto) under the
     existing daemon config area;
   - pixpapers daemon --backend <b> override for testing;
   - auto order: WAYLAND_DISPLAY set => layer; else DISPLAY set => x11; else
     exit 1 with the existing "no wayland session" wording extended to name
     both. A wrong guess is a loud user error, not a silent fallback chain.
   - status reports the active backend name (additive StatusReport field;
     old clients ignore it).
4. test/verify-x11.sh under xvfb-run -a, hermetic PIXPAPERS_HOME, asserting:
   start with DISPLAY only (no WAYLAND_DISPLAY) => backend x11;
   set <id> changes the root pixmap (oracle: pixpapers export at the same
   size, exactly like the Wayland suite uses export as oracle);
   RFPWM broadcast => re-render at the new size;
   stop removes socket/pid and leaves the X server running;
   killing the X server => daemon exits 43.
   CI-safe: the script runs under xvfb-run when present, else is skipped BY
   NAME (honest skip, same policy as the live Wayland layer) — record the skip
   in VERIFICATION.md if it triggers here.
5. Unit tests: the auto-detect env matrix (WAYLAND_DISPLAY x DISPLAY truth
   table) and the depth-24 refusal, both headless.

DO NOT TOUCH: the layer backend's behavior (you may not even reformat its
lines); pixcore; anything in workstream A/C/D.

ACCEPTANCE:
- verify-x11.sh passes under xvfb-run on this machine (install xvfb if it is
  missing — it's a test tool, note it in your summary).
- The full existing Wayland offline suite re-green (the seam's contract is
  unchanged): offline smoke + (on the live session, if you have it)
  verify-live.sh; if this box has no live Wayland session, record that as a
  named skip, don't fake it.
- Bench: PutImage blit cost 1080p + 4K vs the Wayland stretch path, release,
  script under test/, numbers -> docs/PERF.md (expect same order — same
  composite, different blit; the number goes either way).
- Binary-size delta (default vs --no-default-features) in the commit message.
- Cross-cutting gate green.

Commits small: "pixpapers: x11 feature + backend", "pixpapers: backend
auto-detect + config", "test: verify-x11 under xvfb", "docs: PERF x11 blit".

When done, print: the feature/config surface, the verify-x11.sh result lines,
the blit numbers, and the size delta.
```

---

## S6 — TUI core: layout, list, image-aware preview

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S6 of
PLAN-30-ct.md (v0.2 workstream C, core). S1 (the palette/dither knobs) is merged,
so the preview can show them. Before writing code, read, in order:

  AGENTS.md
  PLAN-30-ct.md              — §4 C.1, C.2, C.3 in full, and the S6 row of §6
  crates/pixpapers/src/cli.rs — where `tui` lands in the clap surface
  crates/pixcore/src/variant.rs — generate_from_image: the SAME function export
                                  uses; the preview is a small export, not a
                                  reimplementation
  crates/pixcore/src/render.rs — ResolvedSettings (entry -> config resolution)

TASK — `pixpapers tui`, core only (the action keys are S7; in this step the
keys may exist as no-ops that print "(S7)" to the status bar):

1. Dependencies (AGENTS.md discipline rows, both optional features of
   pixpapers named `tui`, on by default — the feature is shared with S7's
   work, create it now): ratatui (layout/widgets only) + crossterm (raw mode,
   events, alternate screen). Both CPU-only; the TUI owns its own
   single-threaded loop (no event-loop crate, consistent with the daemon's
   no-tokio posture). Binary-size delta measured in the commit message.
2. Terminal hygiene: alternate screen + raw mode on, restored on exit
   (Drop-based, panic-safe); q / Ctrl-C / Esc quit; Ctrl-L refresh; resize ->
   re-render preview + relayout from terminal.size().
3. Layout per C.2 (two panes + status bar + key-hint bar):
   - left: library list — id, title, per-entry pixel/placement, marker on
     library.current, dimmed when enabled = false; selection = arrows / j k.
   - right (~60%): preview pane — the rendered wallpaper at the selected
     entry's resolved settings, computed OFFLINE via the pixcore pipeline at
     the preview canvas size; caption line ALWAYS visible: output size, pixel,
     placement, palette, dither, bg, gutter — image or not.
   - status bar: daemon alive? (socket probe), image-mode result
     (img:kitty / img:none), last action result (one line, most recent wins).
4. Image preview per C.3 — render ONLY when the terminal supports it:
   - detection table: kitty (KITTY_WINDOW_ID / TERM_PROGRAM=kitty -> OSC 6337
     inline PNG), iTerm2 (ITERM_SESSION_ID -> OSC 1337 File=inline=1),
     sixel (SIXEL env or $TERM contains sixel -> ESC q ... ESC \\); else text
     fallback. Env override PIX_PAPERS_IMAGE = none|kitty|iterm2|sixel wins
     (also the test lever). The detection result is shown in the status bar,
     never guessed silently.
   - Fallback: caption block + one honest line pointing at
     `pixpapers export <id> WxH -o /tmp/prev.png`. No fake art.
   - Render path: preview canvas from pane cells (pw = 2*cells_w, ph = cells_h
     when cell aspect unknown — 16:9-ish, documented); generate_from_image at
     (pw, ph) with the entry's ResolvedSettings; downscale to <= pane at cell
     granularity (kitty: emit with resize= to fit; sixel/iTerm2: pre-resize);
     re-render on selection change / knob change / resize only — never on a
     timer; the variant cache gives cache hits for free when the preview size
     matches a real output size.
   - Size guard: refuse (fallback line instead) if the preview PNG would
     exceed 256 KB — boundary unit-tested.

DO NOT TOUCH: commands.rs state mutations (S7 wires the keys); the daemon;
workstreams A/B/D. The TUI in this step must work with a DEAD daemon (it
previews offline; the status bar says so).

ACCEPTANCE:
- pixcore unit tests: preview-size computation (cell grid -> canvas dims),
  degenerate panes (3x3), the 256 KB guard boundary.
- ratatui TestBackend snapbox goldens at a pinned 100x30 backend with a
  fixture library: (a) initial view, (b) no-image fallback pane, (c) detection
  result line for each of the three protocols + none (drive via env).
- Live rows (manual, docs/VERIFICATION.md): a kitty/iterm2/sixel-capable
  terminal if one exists on this machine (image shows); a plain terminal
  (fallback line shows). If a protocol can't be exercised here, record a
  NAMED skip with the terminal you tried — don't fake the row.
- Cross-cutting gate green (the tui feature must not break the default
  build, and --no-default-features must still compile).

Commits small: "pixpapers: tui feature + deps", "pixpapers: tui layout +
list", "pixpapers: tui image preview + detection", "test: tui goldens".

When done, print: the detection table as implemented, the golden file list,
and which live rows are real vs named skips.
```

---

## S7 — TUI actions: add-by-path, set, remove, knob cycles

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S7 of
PLAN-30-ct.md (v0.2 workstream C, actions). S6 is merged: the TUI renders,
previews, and its keys are no-ops. Before writing code, read, in order:

  AGENTS.md                  — invariant #1: CLI owns state; the TUI is a
                               client exactly like the CLI
  PLAN-30-ct.md              — §4 C.4 (the key table) in full, and the S7 row of §6
  crates/pixpapers/src/commands.rs — add / set_current / cycle_current /
                                     edit_entry / set_enabled / remove / export:
                                     the TUI calls THESE, byte-identically to
                                     the CLI; no state path may be reimplemented
  crates/pixpapers/src/daemon (spawn/ensure path) — auto-spawn on set, same as CLI

TASK — wire the full C.4 key table. Every mutation: the same commands::*
function the CLI subcommand calls, then Apply over the same socket if the
entry is (or becomes) current — the half-written manifest is already
impossible via atomic writes; assert the sequence, don't invent new IPC.

  a    add by path — input line "add: ", type/paste a path (tilde-expanded),
       Enter -> commands::add (dedupe hash, optimize, manifest entry) ->
       Apply if it becomes/keeps current. Validation before the call: exists,
       regular file, decodable (trust the image crate; surface its error in
       the status bar). Directory pasted => status error "not a file — pick a
       a single image" (bulk-add stays a CLI job).
  s    set active — commands::set_current + Apply (auto-spawn daemon, exactly
       like CLI `set`)
  c    cycle — commands::cycle_current + Apply
  p    cycle pixel preset 8->16->32->64->128->normal (commands::edit_entry,
       same as `pixpapers pixel`) + Apply if current
  l    cycle placement + Apply if current
  b    prompt bg color #rrggbb (input line) + Apply if current
  g    prompt gutter px + Apply if current
  t    cycle palette off->auto->nes->gb->cga->ega->vga->gen16 + Apply if
       current
  d    cycle dither none->fs->atkinson->bayer2->4->8 + Apply if current
  rm   remove selected — confirm line "remove <id>? y/N" -> commands::remove
       (deletes entry dir + prunes) + Apply if it was current
  e    enable/disable toggle + Apply
  x    export selected to a printed path (commands::export)
  r    refresh (re-read manifest/config, no IPC)

- The input line is a crossterm raw-event line buffer: backspace, left/right,
  delete, paste-as-one-chunk where the terminal allows.
- Status line after every action: the result or the error, one line.

DO NOT TOUCH: the daemon (it re-reads state per Apply — no changes needed);
pixcore; anything in workstreams A/B/D.

ACCEPTANCE:
- Unit tests per action against a temp PIXPAPERS_HOME fixture library, with
  the socket pointed at a throwaway listener (no real daemon in unit tests):
  - add-by-path creates an entry BYTE-IDENTICAL to CLI `pixpapers add` of the
    same file (hash, optimized bytes, manifest row);
  - directory paste is rejected with the exact status line;
  - s updates library.current and puts an Apply request on the throwaway
    socket;
  - each of p/l/b/g/t/d cycles exactly one step and writes the manifest row;
  - rm deletes the entry dir and the manifest row and leaves no residue
    (walk the fixture home and assert);
  - e flips enabled; r reloads (mutate the manifest under the TUI and assert
    the list updates).
- The TestBackend goldens from S6 still hold (the status bar now carries real
  action results — add a golden for "after s on a non-current entry").
- docs/CLI.md: the tui section gains the key table (docs are claims, same
  change); CHANGELOG [Unreleased] updated.
- Cross-cutting gate green.

Commits small: "pixpapers: tui add-by-path", "pixpapers: tui set/cycle",
"pixpapers: tui knob cycles", "pixpapers: tui remove/enable/export".

When done, print: the key table as wired, the test list, and the byte-identical
add-by-path proof (hashes).
```

---

## S8 — Waybar toolbar module

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S8 of
PLAN-30-ct.md (v0.2 workstream D). A (palettes), B (X11) and C (TUI) are merged.
Before writing code, read, in order:

  AGENTS.md
  PLAN-30-ct.md              — §5 (D.1, D.2, D.3) in full, and the S8 row of §6
  crates/pixpapers/src/commands.rs / cli.rs — status --json today (the read face)
  contrib/ (if present)      — the convention for shipped integration files

TASK — ship the waybar module per D.1. This is the whole of v0.2's "GUI":
no new window, no new server, zero new Rust dependencies.

1. pixpapers bar — a new subcommand printing EXACTLY ONE line (waybar chokes
   on multi-line exec output):
     ▞ <title> · <pixel>/<placement-short>      e.g. "▞ nebula · 16/fit"
     ▞ pixpapers — set one                       when there is no current entry
     ▞ pixpapers · daemon down                   when the socket is dead
   Offline (reads config+manifest; the daemon-down line comes from the socket
   probe). Keep the module script ~10 lines by making this the data source.
2. contrib/waybar/config.snippet — drop-in waybar module:
     "pixpapers": {
       "format": "{ }",
       "exec": "pixpapers bar",
       "interval": 30,
       "on-click": "${TERMINAL:-foot} -e pixpapers tui",
       "on-scroll-up": "pixpapers cycle 1",
       "on-scroll-down": "pixpapers cycle 1"
     }
   - VERIFY at implementation time against the INSTALLED waybar's module docs
     (waybar --version; check its exec-module keys for scroll — the plan
     already flagged that key names drift across versions): adjust the snippet
     to the installed version, and note any delta in the README below. The
     scroll behavior is the plan's default: wheel cycles the wallpaper; if the
     installed waybar can't do scroll on exec modules, drop those two keys and
     say so in the README (the fallback is a documented keybind, not a
     script).
   - on-click opens the TUI in $TERMINAL (documented fallback list: foot,
     then the user's own choice); waybar has no "spawn in my terminal" magic —
     the terminal is user-configured, never guessed beyond $TERMINAL.
3. contrib/waybar/README.md (~20 lines): install (copy the snippet into
   modules-center/modules-right), the TERMINAL env, how/when the label
   updates (30 s interval or the next waybar tick — NOT push-based, say so
   honestly), how to stop (remove the module), and the tested waybar version.
4. No pixpapers.sh script is needed if `bar` is one line — the exec key IS
   the module; if you find parsing demands a script, keep it POSIX sh and
   under 20 lines.

DO NOT TOUCH: the daemon; IPC (no new surface — bar reads the same state
status --json reads); pixcore; workstreams A–C.

ACCEPTANCE:
- snapbox goldens for `bar`, hermetic PIXPAPERS_HOME: with a current entry,
  without one, daemon down (dead socket). Output is exactly one line each,
  no trailing junk.
- Offline smoke gains the `bar` call (env -u WAYLAND_DISPLAY
  test/smoke.sh): label correctness with no daemon is the CI-checkable part.
- docs/CLI.md: the bar subcommand + contrib/waybar reference; CHANGELOG
  [Unreleased].
- Live row (manual, docs/VERIFICATION.md): niri + the maintainer's waybar —
  button renders, click opens the TUI in the terminal, scroll cycles, a TUI
  `set` flips the label within one interval (state the interval as the
  number). Screenshot of the toolbar -> docs/screens/ like the other
  evidence. If no waybar session exists on this machine, record a NAMED skip
  with what you did check instead (e.g. the snippet validated against the
  installed waybar's config parser by running waybar against a test config
  under xvfb if available).
- Cross-cutting gate green.

Commits small: "pixpapers: bar label", "contrib: waybar module + README",
"docs: CLI bar + waybar".

When done, print: the three golden lines, the snippet as shipped (with the
installed waybar version and any scroll-key delta), and the live row status.
```

---

## S9 — Polish, docs sweep, changelog

```
Work in /home/lario/Projects/personal/pixelated-papers. You are executing step S9 of
PLAN-30-ct.md — the v0.2 close-out. All of A–D (S1–S8) is merged; this step is
docs-as-claims: every statement traceable to a script/output, no dead links.
Before starting, read, in order:

  AGENTS.md                  — "Docs are claims"
  PLAN-30-ct.md              — §6 (S9 row), §9 (future work — this plan's
                               wording for what stays deferred)
  README.md, docs/CLI.md, docs/COMPOSITORS.md, docs/VERIFICATION.md,
  docs/PERF.md, docs/PLAN.md, CHANGELOG.md

TASK — close the documentation loop:

1. README.md:
   - prune "Deliberately not in v0.1": palettes/dithering, X11, TUI, waybar
     are shipped — move them to the shipped features with one line each +
     pointers; live wallpapers / auto-preset / MPRIS / fractional buffers /
     GUI window STAY deferred (word them as PLAN-30-ct §9 does: the window
     is future work, the waybar button's on-click is the slot it flips into).
   - the size table reflects the v2 default path (S3's numbers — verify the
     table still matches docs/PERF.md; if S3 landed after your read, use
     S3's commit as the source line).
2. docs/COMPOSITORS.md: replace the "X11 — not supported" section with the
   shipped reality (S5's backend: auto-detect order, depth-24 contract,
   exit-43 on X server loss, scale 1:1 default + Xft.dpi note) — every
   claim backed by the verify-x11.sh script name + commit.
3. docs/VERIFICATION.md: the v0.2 matrix — collect the rows S1–S8 recorded
   (goldens, bench scripts, the live rows incl. NAMED SKIPS — a named skip is
   recorded as a skip, not deleted); the x11 row (xvfb run + any live session
   row); the TUI rows (per-terminal, per-protocol); the waybar live row;
   the V-matrix re-run from S4.
4. docs/PLAN.md: append the v0.2 section (or a pointer block) — the four
   workstreams, the key decisions (cache key v2, backend seam shape, TUI
   render-only-when-supported, waybar-as-GUI), and the retro pointers for
   anything that settled differently than PLAN-30-ct proposed (if you found
   one in S1–S8, the plan doc records it the way the "plan retro (11)"
   commit did for v0.1).
5. CHANGELOG.md [Unreleased]: the full v0.2 entry — A/B/C/D bullets, the
   exit-43 extension to X server loss, the dependency rows (ratatui,
   crossterm, x11rb — features tui/x11), the config additions
   (palette/dither defaults, daemon.backend), and a "measured" pointer to
   PERF.md. If a version number is due per the repo's changelog discipline,
   use it; otherwise keep [Unreleased].
6. Sweep: dead links, stale "v0.1" framing, the AGENTS.md "Deferred by
   decision" list (update it: TUI/X11/palettes leave the deferred list;
   window/live/auto-preset stay — this file is convention, keep the edit to
   the list itself).

FORBIDDEN: any behavior change. If a doc claim reveals a real bug in the
shipped code, STOP and report — you do not fix behavior in the close-out.

ACCEPTANCE:
- Every new/changed doc statement traces to a script, a commit, or a
  VERIFICATION row; grep the diff for "TODO"/"fixme" you introduced (there
  should be none).
- Cross-cutting gate green (docs-only, but run it — the smoke script reads
  README claims in two places).
- Commits small per surface: "docs: README v0.2", "docs: COMPOSITORS x11",
  "docs: VERIFICATION v0.2 matrix", "docs: PLAN v0.2 + retro",
  "chore: CHANGELOG v0.2", "docs: AGENTS deferred list".

When done, print: the final README feature list, the VERIFICATION matrix as
shipped (rows + skips), and any doc-vs-code mismatches you found (even if you
only reported them).
```

