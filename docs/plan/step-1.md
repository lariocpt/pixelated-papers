# Step 1 — Workspace, core types, library manifest

**Goal**: a buildable Rust workspace where `pixpapers add <img>` and
`pixpapers list` manage a real on-disk library with per-wallpaper settings.
No image processing beyond decode-and-measure, no Wayland yet. At the end of
this step the data model for every later step exists and is tested.

**Depends on**: nothing.
**Requirements covered**: R1, R2, R3 (type level), R5 (storage level), R10.

---

## 1.1 Workspace scaffold

```
Cargo.toml                 # [workspace] members = ["crates/pixcore", "crates/pixpapers"]
                           # [workspace.package] edition = "2021", rust-version, version = "0.1.0"
                           # [workspace.lints.clippy] warn-level pedantry: unwrap_used, etc. → deny in CI later
rust-toolchain.toml        # stable
.gitignore                 # /target, *.png previews, .direnv
Cargo.lock                 # committed (binary workspace)
```

- `crates/pixcore` — pure library, **zero** tokio/wayland deps. Deps: `image`,
  `serde`, `thiserror`, `dirs`, `hex` (color parsing).
- `crates/pixpapers` — binary. Deps: `pixcore`, `clap`, `tracing`,
  `tracing-subscriber`. (wayland deps arrive in step 3 — keep this step's
  compile surface small.)

CI (step 1 minimal, extended in step 7): `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`.

## 1.2 Preset type — `pixcore::preset`

```rust
/// The vibe knob. Block size in physical pixels. Names are vibe-only.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pixel { B8, B16, B32, B64, B128, Normal }

impl Pixel {
    pub const ALL: [Pixel; 6] = [B8, B16, B32, B64, B128, Normal];
    pub fn block_px(self) -> u32 { /* 8,16,32,64,128,1 */ }
    pub fn is_identity(self) -> bool { matches!(self, Normal) }
}
// FromStr: "8"|"16"|"32"|"64"|"128"|"normal" (also accept "8bit"/"16-bit" spellings)
// Display: "8", "16", …, "normal"
```

- Tests: parse round-trips for every spelling; `block_px` mapping table;
  ordering is by block size (used by the step-6 visibility floor bump).

## 1.3 Placement type — `pixcore::placement`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement { Fit, CoverGutter, CoverCrop, Stretch }   // default: Fit

/// Result of placement math for one (source, output, placement, gutter) tuple.
pub struct View {
    pub window: Rect,   // sub-rect of the *variant/source* image (whole = default)
    pub dest:   Rect,   // rect of the output buffer that the image occupies
    pub output: (u32, u32),
}

pub fn view(src: (u32,u32), out: (u32,u32), p: Placement, gutter_px: u32) -> View;
```

Math (integer; PLAN §2.2 table). Details to pin in tests now, even before the
renderer exists:

- `Fit`: `s = min(W·Ws⁻¹, H·Hs⁻¹)` as rational; dest size =
  `(⌊Ws·s⌋, ⌊Hs·s⌋)`, dest offset = centered `(⌊(W-dest_w)/2⌋, ⌊(H-dest_h)/2⌋)`.
  Never crops: window = whole source.
- `CoverCrop`: `s = max(...)`, window = centered sub-rect of source with
  aspect ratio `W:H` (computed in source space: crop the *larger* axis), dest
  = full output.
- `CoverGutter`: content box `Cb = (W-2g, H-2g)` clamped to `≥ (1,1)`; same
  cover math against Cb; dest = inset by `g`; gutter ring = bg fill (renderer's
  job later).
- `Stretch`: window = whole source, dest = full output (x/y independent).
- Sliver rule: if dest_w/dest_h computed as 0 (pathological gutter), clamp
  to 1; any 1-px remainder from centering goes to the *right/bottom* bg side.
- `proptest`: for all src 1..8192², out 1..8192², g 0..=256:
  dest ⊆ output; Fit ⇒ window==whole ∧ no gap-free guarantee is NOT required
  (bg allowed); Cover* ⇒ dest covers its content box exactly; View is
  deterministic.

## 1.4 Color + per-wallpaper settings types — `pixcore::config` / `manifest`

```rust
/// #rgb / #rrggbb, serialized lowercase "#rrggbb".
pub struct RgbCss(pub [u8;3]);   // FromStr + Display + Serialize

pub struct Settings {            // per-wallpaper knobs (R5) — all optional;
    pub pixel: Option<Pixel>,    // None = inherit config default
    pub placement: Option<Placement>,
    pub bg_color: Option<RgbCss>,
    pub gutter_px: Option<u32>,  // meaningful for CoverGutter only
}
impl Settings { pub fn resolve(&self, cfg:&Config) -> ResolvedSettings; }
// ResolvedSettings { pixel, placement, bg_color, gutter_px } — no Options; this is
// the single struct every later stage consumes.
```

## 1.5 Config — `pixcore::config`

`~/.config/pixpapers/config.toml` (create-with-defaults on first run):

```toml
enabled         = true        # R6 master switch
default_pixel   = "16"        # R2
default_placement = "fit"     # R3
default_bg_color  = "#000000"
default_gutter_px = 24        # only used by cover-gutter
min_visible_px  = 2           # visibility floor (0 = off)  — step 6 wires behavior; parsed now
palette         = true        # median-cut variants (step 2)
[daemon]
idle_autoexit_min = 0         # 0 = never (v1)
```

- Load: missing file ⇒ defaults (no error, no write); malformed ⇒ hard error
  with path (never silently overwrite user config).
- Save: atomic (serialize → `config.toml.tmp` → rename).
- Tests: round-trip; default file; malformed → error mentions path.

## 1.6 Manifest + library layout — `pixcore::manifest`

```rust
pub struct Entry {
    pub id: String,            // 8-hex blake3 of content hash prefix — stable, short
    pub title: String,         // --title or filename stem
    pub settings: Settings,    // per-wallpaper knobs (R5)
    pub hash: String,          # full blake3 of stored original bytes
    pub added_unix: i64,
    pub width: u32, pub height: u32,   // cached dims of the stored original
    pub original_bytes: u64,
}
pub struct Library { version: u32, entries: Vec<Entry> }
```

- Paths: `pixcore::paths` exposing `data_dir() (= XDG_DATA_HOME/pixpapers)`,
  `lib_dir()`, `entry_dir(id)`, `original_path(id)`, `variants_dir(id)` —
  **all** later path logic funnels through here (step-2 variant naming reuses
  it).
- `Library::load()` / `save()` — atomic; unknown-version ⇒ error, not wipe.
- `add(path, title) -> Result<Entry>`:
  1. decode header with `image` (`ImageReader::into_dimensions`) — reject
     undecodable with a clear error;
  2. hash bytes (blake3); id = first 8 hex; duplicate hash ⇒ return existing
     entry with `duplicate: true` signal (CLI prints "already in library");
  3. **this step**: copy file bytes verbatim to `entry_dir(id)/original.<ext>`;
     (step 2 replaces the copy with the optimization pass);
  4. insert entry, atomic save.
- `remove(id)` — manifest entry + recursive delete of entry dir; missing id ⇒
  error with did-you-mean over titles.
- **Env pinning for tests/daemon**: every path honors `PIXPIPAPERS_HOME`
  override (single dir replacing data+config+state) — makes CLI tests hermetic
  and is reused by step-4/5 tests. (Name: `PIXPIPAPERS_HOME`? — use
  `PIXPAPERS_HOME`.)

## 1.7 CLI (this step's surface)

```
pixpapers add <path>... [--title T]
pixpapers list [--json]
pixpapers remove <id>
pixpapers pixel|placement|bg|gutter <id> <value>     # edit manifest only for now
pixpapers default pixel <v> | placement <v>          # edit config only for now
pixpapers version
```

- `clap` derive, subcommand per verb (the same shape step 4 will wire to the
  daemon; step 4 adds `set` etc.).
- `list` default table: `id  title  pixel  placement  WxH  size  file`; `--json`
  for scripting.
- Exit codes: 0 ok, 1 user error (clear message, no backtrace), 2 internal.

## 1.8 Tests

- `preset`: parse/display/block table, ordering.
- `placement::view`: the proptest + a worked-example table (1920×1080 output
  with 3840×2160, 1080×1920 portrait, 2000×1000 weird src × all 4 placements,
  gutters {0, 24, 500}) — golden expected rects in the test file.
- `config`: round-trip, defaults, malformed error text.
- `manifest`: add/list/remove against `PIXPAPERS_HOME=tempdir`; duplicate
  detection; atomic save (simulate write failure: read-only tmp → original
  file intact).
- CLI (`snapbox`): happy paths + error paths above using the env pin.

## 1.9 Acceptance criteria

1. `cargo test --workspace` green, `cargo clippy -D warnings` clean.
2. `pixpapers add ~/some/img.png && pixpapers list` shows the entry with
   correct dims; second `add` of the same file reports duplicate, no second
   copy.
3. `pixpapers pixel id 8`, `placement id stretch`, `bg id "#001122"`,
   `gutter id 16` persist across runs; `default pixel 32` lands in config.
4. `pixpapers remove id` deletes manifest entry **and** `library/<id>/`.
5. Malformed config file ⇒ clear error naming the path, nothing overwritten.
6. `View` proptest passes with the sliver and containment rules.

**Commit**: `step-1: workspace, presets, placement math, config, manifest, add/list/remove`
