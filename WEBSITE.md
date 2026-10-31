# The website

`docs/index.html` is the whole site: GitHub Pages builds `main:/docs`
with no toolchain, so what you see in the repo is what ships. This file
is the contract for keeping it that way. (Structure follows the
color-terminal project page; the one documented deviation is §Images.)

## Hard rules

- **One file.** `docs/index.html`, everything inline: CSS in `<style>`,
  JS in `<script>`, favicon as a `data:` URI. No build step, no bundler,
  no framework. If you can't express it inline, it doesn't ship.
- **Zero external requests.** No webfonts, no CDNs, no analytics, no
  remote images or fonts in SVG. A visitor with GitHub blocked still
  gets the full page. Check with the Network panel before pushing.
- **Complete without JavaScript.** The only script is the hero palette
  cycler. With JS disabled the page must still read perfectly — static
  hero image, all content, working links. Test:
  `chromium --disable-javascript docs/index.html`.
- **Theme-aware, quiet chrome.** `:root` carries the light palette;
  `@media (prefers-color-scheme: dark)` swaps it. Muted greys, one
  accent; the *images* are the colour, not the CSS. Respects
  `prefers-reduced-motion` (cycler doesn't auto-play).
- **360 px wide with no horizontal scroll.** Body text ≥ 15 px, code
  blocks scroll inside themselves (`overflow-x: auto`), images are
  `max-width: 100%`.
- **No marketing voice.** Every number on the page is a measured number
  that also lives in `docs/PERF.md` / `docs/VERIFICATION.md` with its
  provenance; every honest limit stated there is stated here. If the
  docs walk a claim back, the page walks it back in the same commit.

## Images (the documented deviation)

color-terminal's page ships no images; this tool is visual, so the page
commits its renders under `docs/img/` — same-origin, in the repo, no
CDN, `image-rendering: pixelated`. Rules for that folder:

- Images must be **produced by the tool itself** (`pixpapers export`
  for the palette grid and close-up; real screenshots for the waybar
  strip) or be the declared synthetic source (`img/source.png`).
- Regenerating the grid after palette/pipeline changes:

  ```sh
  W=$(mktemp -d); PIXPAPERS_HOME=$W/home ./target/release/pixpapers add docs/img/source.png --title sunset
  for p in nes gb cga vga ega gen16; do
    for d in none fs bayer8; do
      PIXPAPERS_HOME=$W/home ./target/release/pixpapers export sunset 480x270 \
        --palette $p --dither $d -o docs/img/grid-$p-$d.png
    done
  done; rm -rf "$W"
  ```

- Keep the folder small (everything together is ~450 KB today); no
  screenshots containing personal content — hermetic homes only, never
  the maintainer's live desktop.
- `*.png` is git-ignored repo-wide; `docs/img/**` is explicitly
  un-ignored in `.gitignore` (alongside `docs/screens/**`).

## Verifying a change

1. `python3 -m http.server -d docs 8000` → open
   `http://localhost:8000/` and view source — every referenced
   `img/…` and `*.md` must exist under `docs/`.
2. Visual pass at 360 px and 1920 px, light and dark, JS off and on.
3. If a visual-testing rig is available, run
   `web_check`/`web_accessibility` against the local server before
   pushing; otherwise eyeball keyboard focus (Tab reaches the cycler
   buttons and every link).
4. Content claims follow the repo rule: a number on the page needs its
   measurement in the docs, or it comes off the page.
