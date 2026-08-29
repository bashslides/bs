# `web/` — the browser tools

A dependency-free static site: no framework, no bundler, no npm. Four pages,
plain HTML/CSS/JS, plus the engine compiled to WebAssembly.

**One page, three views** — instructions → compile → present, in that order —
so the WebAssembly engine, the loaded deck and the saved-deck library are shared
state: compiling and then presenting reloads nothing.

```
web/
  index.html            the shell: tab bar + the three views
  app.js                routing, the deck library UI, drag-drop, SW registration
  instructions.js       the format reference, copy-all
  compile.js            source deck → playable deck (wasm)
  viewer.js             canvas playback (a port of the player's pure logic)
  store.js              the deck library, in localStorage
  wasm.js               loader/glue for the compiler module
  style.css             terminal styling, mobile-first
  sw.js                 service worker: offline + installable
  manifest.webmanifest  PWA metadata
  icon-*.png            app icons (generated, checked in)
  presentation.json     the deck present loads by default
```

## Installing it

The site is a PWA: `manifest.webmanifest` plus `sw.js`, which caches the shell,
`bs.wasm` and the reference docs, so the whole thing — **including compiling** —
works with no network. Browsers offer "Install"/"Add to Home Screen" once it has
been served over HTTPS (GitHub Pages qualifies) or from `localhost`.

`start_url` and `scope` are relative (`.`), and the worker is registered by
relative path, so it installs correctly under a project subpath (`…/bs/`) as
well as at a domain root. `scripts/build-web.sh` stamps `sw.js` with a hash of
the built files, so each deploy installs a fresh cache and evicts the old one —
no manual version bumping, and no stale app after an update.

## The deck library

Compiled and imported decks are kept in `localStorage` (`store.js`) so they
survive a reload, a restart and being offline. It is a *recents* list, not an
archive: the 20 newest are kept, a save that runs out of quota evicts the oldest
and retries, and same-name saves replace rather than accumulate. The **library**
button in the present view lists them with their frame count, size and age, and
offers per-deck delete plus delete-all. Storage being unavailable (private mode,
`file://`) is reported in the panel rather than failing silently.

Every page is single-column and works on a phone: `100dvh` shells that track
mobile browser chrome, 16px form controls (below that iOS zooms on focus),
40px-minimum touch targets, `env(safe-area-inset-bottom)` padding, and no
horizontal page scroll. On `present`, tapping the left third of the canvas steps
back and the rest steps forward.

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
| `bs-deck-skill.md` | `.claude/skills/bs-deck/SKILL.md` | the packaged Claude skill, offered for download (as `SKILL.md`) |
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

The deck is drawn on a `<canvas>`, **one `fillText` per cell at an exact cell
origin** — never as runs of text. A run of text only lines up if the device
resolves a genuinely monospaced font that also covers every character used, and
on mobile neither is guaranteed: the platform "monospace" can be a proportional
face, and box-drawing characters often come from a fallback font with different
metrics. Either one shears the grid apart — the giveaway is spaces looking
narrower than other characters. Positioning every cell ourselves makes the
layout independent of whatever font the device picks (verified by forcing a
proportional serif and checking the grid still lands on the lattice). Characters
in the box-drawing block are stretched to the cell width so long runs of `─` or
`█` tile without gaps, and the canvas is sized in device pixels so it stays
crisp on high-DPI screens.

`app.js` is also a port of the terminal player's *pure* parts, so playback matches:

| Rust | JS |
|------|----|
| `PlayablePresentation::grid_at` | `gridAt` (replays `full` + `diff` frames) |
| `player::loop_next` | `loopNext` (bounce / restart / `count`) |
| `Player::auto_advance_delay` | `autoAdvanceDelay` (min over auto-play animations) |
| `Player::animation_cluster` | `animationCluster` (overlap-merged skip target) |
| `Player::frame_auto_advance_delay` | `frameAutoAdvanceDelay` |
| `Player::effective_auto_delay` | `effectiveAutoDelay` |

Being a port, it *can* drift — change the player and mirror it here.

A deck reaches the present view from the compile view's **present →**, from the
**library**, from **open** (a compiled `.json` on the device), or by dropping a
file anywhere in the app — a compiled deck plays, a source deck is routed to
compile instead.

Compiling hands the deck straight to the present view in memory — no storage
round-trip, no navigation — and saves a copy to the library on the way past.

| Key | Action |
|-----|--------|
| <kbd>→</kbd> / <kbd>Space</kbd> / <kbd>Enter</kbd> | next frame (skips a whole loop or auto-play animation) |
| <kbd>←</kbd> | previous frame (same skipping) |
| <kbd>Shift</kbd>+<kbd>←</kbd>/<kbd>→</kbd> | jump ±10 frames |
| <kbd>Home</kbd> / <kbd>End</kbd> | first / last frame |
| <kbd>f</kbd> | fullscreen; <kbd>Esc</kbd> leaves |
| <kbd>o</kbd> | open a compiled `.json` |

`→` on an auto-play animation lands on the animation's **own last frame** — its
finished state — rather than stepping past it; pressing it again continues. (The
terminal player still jumps one frame further, to the first slide after the
span.)

**Nothing is keyboard-only.** A phone has no keyboard, so every action also has a
tap target: the footer carries real buttons (`←` `→` `⇤` `⇥` `full`), the deck
bar carries `library` and `open`, tapping the canvas steps (left third back, the
rest forward), and the frame bar jumps to any frame. Fullscreen hides both bars, so it gets its own way out — a
dim `✕ full` button pinned to the top-right corner, the only route back on a
device with no <kbd>Esc</kbd>.

On iPhone, `requestFullscreen` is refused for non-video elements, so `full`
hides the bars without entering browser fullscreen. That is the useful half, and
the exit button works the same either way.

The frame bar shows one tick per frame only while that fits. A tick cannot
render below about a pixel, so past roughly 120 frames on a phone the ticks
would overflow the bar, widen the page, and — because the horizontal scrollbar
then steals width from the stage — shrink the deck itself. Beyond that point
each tick stands for a contiguous *range* of frames instead, so the bar always
fits exactly and a 2000-frame deck renders at the same scale as a 15-frame one.
Clicking still seeks; loops and animations are still marked.

A small slide number (`12/15`) sits at the right of the footer. It disappears in
fullscreen along with the rest of the chrome, which is the point — nothing but
the deck.

`Command` objects cannot run in a browser. The compiler bakes their placeholder
box into the static frames, so the slide renders correctly — the viewer names
the command in the status bar instead of executing it.

## Deployment

`.github/workflows/pages.yml` runs `scripts/build-web.sh` and uploads `_site/`
on every push to `main` touching the engine, `web/`, `wasm/`, the format
reference or the workflow. The wasm build runs *before* the upload, so a broken
build fails the deploy and Pages keeps serving the previous version.
