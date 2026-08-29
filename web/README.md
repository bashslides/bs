# `web/` — the browser viewer

A dependency-free static page that renders a **compiled** presentation — the
output of `bs compile source.json out.json` — in the browser. No build step, no
framework: `index.html` + `style.css` + `app.js`, served as-is.

```
web/
  index.html        markup
  style.css         black-and-white terminal chrome
  app.js            frame replay + playback (a port of the player's pure logic)
  presentation.json the deck loaded by default
  .nojekyll         tell GitHub Pages to serve files starting with `_` as-is
```

## Run it locally

Any static server works (the page `fetch`es the deck, so `file://` will not do):

```bash
python3 -m http.server -d web 8000    # then open http://localhost:8000
```

## Loading a deck

In priority order:

1. `?deck=<url>` — e.g. `index.html?deck=decks/talk.json` (same-origin, or a
   CORS-enabled URL).
2. `presentation.json` next to `index.html` (the default).
3. Drag a compiled `.json` onto the page, or press <kbd>o</kbd> to pick one.

To publish your own deck, compile it over the default:

```bash
cargo run -- compile my-talk.json web/presentation.json
```

## Keys

| Key | Action |
|-----|--------|
| <kbd>→</kbd> / <kbd>Space</kbd> / <kbd>Enter</kbd> | next frame (skips a whole loop or auto-play animation, like the terminal player) |
| <kbd>←</kbd> | previous frame (same skipping) |
| <kbd>Shift</kbd>+<kbd>←</kbd>/<kbd>→</kbd> | jump ±10 frames |
| <kbd>Home</kbd> / <kbd>End</kbd> | first / last frame |
| <kbd>f</kbd> | fullscreen (bars hidden); <kbd>Esc</kbd> leaves |
| <kbd>o</kbd> | open a compiled `.json` |

Clicking the canvas steps too (left third back, the rest forward), and the frame
bar is clickable.

## What it does and does not do

`app.js` is a direct port of the terminal player's *pure* parts, so playback
matches the terminal:

| Rust | JS |
|------|----|
| `PlayablePresentation::grid_at` | `gridAt` (replays `full` + `diff` frames) |
| `player::loop_next` | `loopNext` (bounce / restart / `count`) |
| `Player::auto_advance_delay` | `autoAdvanceDelay` (min over auto-play animations) |
| `Player::animation_cluster` | `animationCluster` (overlap-merged skip target) |
| `Player::frame_auto_advance_delay` | `frameAutoAdvanceDelay` |
| `Player::effective_auto_delay` | `effectiveAutoDelay` |

The one runtime feature a browser cannot provide is the `Command` object: it
runs a local binary. The compiler already bakes its placeholder box into the
static frames, so the slide still renders correctly — the viewer names the
command in the status bar instead of executing it.

## Deployment

`.github/workflows/pages.yml` uploads this directory to GitHub Pages on every
push to `main` that touches `web/` (or the workflow file itself). It passes
`enablement: true` to `actions/configure-pages`, which creates the Pages site
over the API on the first run — so the deploy does not depend on
**Settings → Pages → Build and deployment → Source: GitHub Actions** having been
saved by hand. Setting it in the UI works too, and is equivalent.
