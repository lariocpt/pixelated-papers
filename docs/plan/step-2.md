# Step 2 — Original optimization + pixelation pipeline + offline export

**Goal**: the entire image pipeline exists and is provable **offline** — no
daemon, no Wayland. `pixpapers export <id> 16 stretch 1920x1080 -o out.png`
renders exactly what step 3+4 will put on screen, so everything downstream is
just transport. Also: `add` now stores an *optimized* original (R7's storage
side).

**Depends on**: step 1.
**Requirements covered**: R1, R3 (render side), R7 (storage), R8 (variant
size discipline), and settles the **cover-gutter open question** (§3 here).

---

## 2.1 Original optimization — `pixcore::optimize`

On `add` (replaces step-1's verbatim copy):

1. Decode fully (`image` crate; png/jpeg/webp/bmp/gif-first-frame/ico-largest/
   tiff). Animated GIF: first frame, note in CLI output.
2. **Cap** to `max_side` (config `[optimize] max_side = 3840`, clamp
   1080..=7680): if larger, iterative-halving box downscale to the cap
   (aspect preserved — this caps the *worst-case* decode for `normal` mode
   and bounds pipeline cost; a 12K photo has no business being a wallpaper).
3. Re-encode by content heuristics, keep original in `~/.cache` only during
   add (never deleted from user's dir — we copy into the library):
   - ≤ 512 colors after sampling **or** was originally PNG with α →
     **PNG** (lossless; pixel-art sources stay perfect);
   - else → **JPEG q=87** (subsample 4:4:4 when small, 4:2:0 when large) or
     WebP q=82 if the `webp` encoder feature is on — pick one default (JPEG,
     universally safe) and keep WebP behind a feature flag.
   - Record `optimize: {filter, quality, caps}` in the entry for honesty.
4. Write `library/<id>/original.jpg|png` atomically; entry dims/bytes update.

- `pixpapers optimize --all` re-runs the pass over the library (useful when
  defaults change) — reports per-file before/after sizes.
- Tests: oversized synthetic image caps correctly; PNG with α stays α-PNG;
  photo → JPEG; idempotent (re-optimize = no change).

## 2.2 Downscale to variant canvas — `pixcore::pipeline`

```rust
pub fn downscale_to_variant(img: &RgbaImage, view: &View, b: u32) -> RgbaImage
```

- Target canvas: `(⌈dest.w / B⌉, ⌈dest.h / B⌉)` — absolute minimum for
  content-box coverage (R1/R8). B=1 (`Normal`) ⇒ canvas == dest (handled by
  the stretch step directly; no downscale).
- The **window** from `View` selects the source sub-rect first (cover modes
  crop here), then the downscale maps window → canvas.
- Method: repeated 2×2 box-halving until ≤ 2× target, then final box to exact
  canvas size. No Lanczos (ringing reads as dirt in blocky output). Alpha:
  premultiplied during averaging (correct for α-PNG pixel art).
- Performance note: box-halving of a 4K source to 120×67 is ~2–3 passes of a
  trivially cache-friendly loop; expected < 15 ms. Bench lands in step 9.

## 2.3 Median-cut quantization — `pixcore::pipeline::quant`

```rust
pub fn median_cut(img: &RgbaImage, max_colors: usize) -> Vec<Rgb>   // ≤256
pub fn apply_palette(img: &RgbaImage, pal: &[Rgb]) -> Indexed    // nearest-in-RGB
```

- Classic median cut: longest-axis split, ≤ log2(256) splits; deterministic
  (sort ties by channel value then insertion order) → **same bytes every
  run** (proptest).
- α handling: colors with α=0 collapse to one transparent palette entry;
  α∉{0,255} → treated as opaque against black (documented; wallpaper α is a
  non-case mostly, keep it simple & predictable).
- Config `palette=false` ⇒ skip (variants encode as RGBA PNG — still small).
- Tests: ≤ max_colors; determinism; known-image golden (gradient in → 8-color
  ramp out); perf smoke (4K → 256 in < 120 ms debug-free).

## 2.4 Variant PNG encode + naming — `pixcore::variant`

```rust
pub struct VariantKey<'a> { id:&'a str, pixel: Pixel, placement: Placement,
                            gutter_px: u32, out: (u32,u32) }
pub fn variant_path(key: &VariantKey) -> PathBuf
// variants/v{PIPELINE_VER}-{pixel}-{placement}[-g{g}]-{W}x{H}.png
// e.g. variants/v3-16-cover-gutter-g24-1920x1080.png
pub fn generate(orig: &Path, settings: &ResolvedSettings, out: (u32,u32))
    -> Result<PathBuf>      // cache hit ⇒ return path; miss ⇒ pipeline → encode → atomic write
```

- Encode: `image`'s PngEncoder with palette (`ImageOutputFormat::Png` +
  `DynamicImage::into_palette_rgba8` after §2.3); compression level max — a
  120×67 palette PNG ≈ 3–10 KB (measure in step 9).
- `PIPELINE_VER: u32` const bumps on any behavior change (filter, quantizer,
  placement rounding) — stale cache impossible without deletion.
- Cache-hit rule: file exists && size > 0 ⇒ use it (cheap stat; pipeline-ver
  in the name makes invalidation automatic).

## 2.5 `export` / `--preview` — the offline proof

```
pixpapers export <id> [--pixel P] [--placement M] [--bg C] [--gutter G] <W>x<H> [-o out.png]
```

- Runs the **exact** renderer path: View → downscale → (palette) → nearest
  stretch into an output-sized RGBA buffer **painted bg_color first**
  (`pixcore::render::composite(variant, &view, bg) -> RgbaImage` lives in
  pixcore so the daemon reuses it verbatim — step 3/4 attach its output
  unchanged).
- `add` gains `--preview` flag: prints a short ascii/size summary of the
  16-fit variant for the current primary output guess (skip if no wayland
  env: offline by design).

## 2.6 Tests (the strongest gate in the project)

- **proptest (pipeline)**:
  - variant canvas dims == `⌈dest.w/B⌉ × ⌈dest.h/B⌉` for all modes/outputs;
  - `export` bytes deterministic across runs (with and without palette);
  - `Normal` ⇒ output == nearest-stretch(original→view) (identity check);
  - every output pixel of a `Stretch`/`CoverCrop` export ∈ variant palette
    ∪ {bg} (no interpolation artifacts possible);
  - bg border pixel check: `Fit` corners == bg_color when aspect mismatch.
- **Golden images** (checked-in, tiny): 4 placement × preset 16 renders of a
  deterministic 64×64 gradient source at 320×180 — PNG golden diff (exact
  bytes) catches any accidental pipeline drift in later steps (this is the
  regression net for step 6's tuning).
- **Size budget**: 1920×1080 @ B=16 palette variant of the golden source
  ≤ 20 KB (R8 smoke); printed as part of the test output.
- `optimize` tests from §2.1; `add` now stores optimized original (manifest
  records it), duplicates still by source-hash (pre-optimization).

## 2.7 Settle the cover-gutter question

Build `export` for cover-gutter with both candidate readings behind
`[preview] gutter_mode = "mat" | "blur"` ("mat" implemented now; "blur"
sketched: cover-crop the image, then fill the outside with a blurred,
enlarged edge-strip of the image instead of `bg_color` — ~40 lines in
pipeline + its own cache-key suffix). Show the user the two exports; pick;
update PLAN §10. **Do not** build "blur" unless chosen.

## 2.8 Acceptance criteria

1. All step-1 criteria still pass (nothing regressed).
2. `pixpapers add big.png` stores a capped/re-encoded original with sane
   bytes; `optimize --all` reports and converges.
3. `pixpapers export <id> 8 fit 1920x1080 -o a.png` … for every
   preset × placement combination produces a valid PNG matching the visual
   expectation (human eyeball + goldens).
4. Palette variants at 1080p/B=16 are ≤ 20 KB on photographic and flat-art
   test images.
5. Golden tests green; pipeline-determinism proptest green.
6. cover-gutter semantics decision recorded in PLAN.md §10 (user confirmed
   "mat" or "blur").

**Commit**: `step-2: optimize, pixelation pipeline, palette variants, composite, export`
