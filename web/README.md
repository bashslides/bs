# `web/` — the browser tools

A dependency-free static site: no framework, no bundler, no npm. Four pages,
plain HTML/CSS/JS, plus the engine compiled to WebAssembly.

```
web/
  index.html                     home — pick a tool
  present.html      + app.js     play a compiled deck
  compile.html      + compile.js source deck → playable deck (wasm)
  instructions.html + instructions.js   the format reference, copy-all
  wasm.js                        loader/glue for the compiler module
  style.css                      shared black-and-white terminal chrome
  presentation.json              the deck present.html loads by default
```

## Build and run it

Never open `web/` directly — the site needs three files assembled into it (the
`.wasm`, the format reference, the sample deck). Use the build script, which is
the same one CI runs:

```bash
./scripts/build-web.sh --serve          # build _site/ and serve on :8000
./scripts/build-web.sh --serve --port=9000
./scripts/build-web.sh                  # build only
```

It writes `_site/` (gitignored) containing `web/` plus:

| Added file | From | Why |
|------------|------|-----|
| `bs.wasm` | `cargo build -p bs-wasm --target wasm32-unknown-unknown --profile wasm-release` | the compile tool's engine |
| `presentation-format.md` | `PRESENTATION_FORMAT.md` | so the instructions page can't drift from the repo's reference |
| `demo.json` | `examples/demo.json` | the compile page's `sample` button |

Needs the `wasm32-unknown-unknown` target — `./scripts/install-toolchain.sh`
installs it.

## Test it

```bash
./scripts/test-web.sh        # headless, exits non-zero on failure (needs firefox)
```

This drives a real browser against the built site and checks the claim that
justifies the WebAssembly approach: **the browser compiler and `bs compile`
produce byte-identical output.** It also verifies the deck shape, that malformed
input yields a message rather than a wasm trap, and that the module stays usable
after an error. The verdict travels out of the browser as a request to a magic
URL that lands in the static server's log, so the script has a real exit code —
no screenshots to eyeball. `KEEP=1` preserves the browser/server logs.

## The compile tool

`wasm/src/lib.rs` is a thin `extern "C"` shim over `bs::compile::compile_json`,
the same function the CLI calls — deliberately raw pointers over linear memory
instead of wasm-bindgen, so the build is a plain `cargo build` with no
`wasm-pack`, Node or npm anywhere. About 110 KB gzipped.

Browser support is universal in practice: WebAssembly has shipped everywhere
since 2017, and on iOS every browser is WebKit, so Safari's support covers the
platform. The module is single-threaded, which matters — threads would need
`SharedArrayBuffer` and COOP/COEP headers that GitHub Pages cannot set. If the
module fails to load at all (iOS Lockdown Mode disables WebAssembly), the page
says so and points at the CLI rather than silently doing nothing.

## The present tool

`app.js` is a port of the terminal player's *pure* parts, so playback matches:

| Rust | JS |
|------|----|
| `PlayablePresentation::grid_at` | `gridAt` (replays `full` + `diff` frames) |
| `player::loop_next` | `loopNext` (bounce / restart / `count`) |
| `Player::auto_advance_delay` | `autoAdvanceDelay` (min over auto-play animations) |
| `Player::animation_cluster` | `animationCluster` (overlap-merged skip target) |
| `Player::frame_auto_advance_delay` | `frameAutoAdvanceDelay` |
| `Player::effective_auto_delay` | `effectiveAutoDelay` |

Being a port, it *can* drift — change the player and mirror it here.

Loading a deck, in priority order: `?deck=<url>`; `?deck=session` (the hand-off
from the compile page, via `sessionStorage`); `presentation.json` next to the
page; or drag-drop / <kbd>o</kbd>.

| Key | Action |
|-----|--------|
| <kbd>→</kbd> / <kbd>Space</kbd> / <kbd>Enter</kbd> | next frame (skips a whole loop or auto-play animation) |
| <kbd>←</kbd> | previous frame (same skipping) |
| <kbd>Shift</kbd>+<kbd>←</kbd>/<kbd>→</kbd> | jump ±10 frames |
| <kbd>Home</kbd> / <kbd>End</kbd> | first / last frame |
| <kbd>f</kbd> | fullscreen; <kbd>Esc</kbd> leaves |
| <kbd>o</kbd> | open a compiled `.json` |

`Command` objects cannot run in a browser. The compiler bakes their placeholder
box into the static frames, so the slide renders correctly — the viewer names
the command in the status bar instead of executing it.

## Deployment

`.github/workflows/pages.yml` runs `scripts/build-web.sh` and uploads `_site/`
on every push to `main` touching the engine, `web/`, `wasm/`, the format
reference or the workflow. The wasm build runs *before* the upload, so a broken
build fails the deploy and Pages keeps serving the previous version.
