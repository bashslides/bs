# `web/` — the browser tools

A dependency-free static site: no framework, no bundler, no npm. One page
with four views, plain HTML/CSS/JS, plus the engine compiled to WebAssembly.

**One page, four views** — a title page plus instructions → compile → present,
in that order — so the WebAssembly engine, the loaded deck and the saved-deck
library are shared state: compiling and then presenting reloads nothing. The
`bs` wordmark in the bar is the title page: one paragraph on what the tool is
and how to use it.

The topmost bar holds the wordmark, the tabs and the present view's **library**
and **open** buttons; fullscreen hides it along with the footer. There is no
separate deck-title bar — the document title carries the deck name. That bar has
to fit a 320px phone, so the two action labels shorten to `lib` / `open` below
430px and the type tightens below 400px (measured, not guessed).

```
web/
  index.html            the shell: tab bar + the three views
  app.js                routing, the deck library UI, drag-drop, SW registration
  instructions.js       the format reference, copy-all
  compile.js            source deck → playable deck (wasm)
  viewer.js             canvas playback (a port of the player's pure logic)
  store.js              the deck library, gzip-compressed in IndexedDB
  wasm.js               loader/glue for the compiler module
  style.css             terminal styling, mobile-first
  sw.js                 service worker: offline + installable
  manifest.webmanifest  PWA metadata
  icon-*.png            app icons (generated, checked in)
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
no manual version bumping, and once a new version is active it never serves an
old build's files. A new version
waits until every open bs tab of the old one is closed (a reload alone keeps
the old one) and then takes over on the next visit, so a running page never
mixes its engine with a newer build's sample, docs or `bs.wasm`. If any file the
app needs to start or compile fails to download, the update's install fails and
the previous version stays in place; only the sample, docs and icons may be missing.

## The deck library

Compiled and imported decks are kept in IndexedDB (`store.js`) so they survive
a reload, a restart and being offline. A compiled deck is mostly the same cell
records repeated frame after frame, so each one is stored gzip-compressed
(`CompressionStream`; the demo deck goes from 125 KB to under 2 KB) and only
inflated when it is opened; metadata and bodies are separate object stores, so
listing the library never reads a body. The 100 newest decks are kept, within
50 MB of *stored* (compressed) bytes in total — older decks are evicted to stay
under it, a deck over the budget on its own still plays but is not kept, a save
that runs out of quota keeps fewer older decks and retries, and same-name saves
replace rather than accumulate. IndexedDB's quota is a share of free disk space
rather than localStorage's ~5 MB, but it is still per origin, and every GitHub
Pages project site of a user shares one; so the database is named after the
deployment's path (`bs:decks:/bs/`), and two bs copies on one origin keep
separate libraries. When that database is first created it adopts the library
an earlier build kept in `localStorage` (the deployment's `bs:decks:/bs/` key,
else the old unscoped `bs:decks`), uncompressed, in the same transaction, and
then frees that key. Every library call is async and never throws. A deck keeps its file's
name only while the source is that file's unedited text; pasted or edited
source is saved as `pasted-<time>`. The **library**
button in the present view lists them with their frame count, size (and stored
size) and age, and
offers per-deck delete plus delete-all. Storage being unavailable (private mode,
`file://`) is reported in the panel rather than failing silently.

Every page is single-column and works on a phone: `100dvh` shells that track
mobile browser chrome, 16px form controls (below that iOS zooms on focus),
40px-minimum touch targets (the present view's footer buttons 48px on phones
and touch screens), `env(safe-area-inset-bottom)` padding, and no
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
input yields a message rather than a wasm trap, that the module stays usable
after an error and is replaced by a fresh one after a trap, that library-panel
clicks never step the present view, that cells paint the same one-column
chars as `bs play` (wide chars as `?`), and
that a playable missing a field the viewer relies on (a cell, a delay) is
rejected with a message instead of being saved to the library, that the library
is stored compressed, adopts an old `localStorage` library and keeps a deck far
past the old 1.5M-character budget, that an animation is one frame-bar tick and
`←` inside one lands on the slide before it, and that the footer keeps `←` `→`
at the right edge, 48px tall, on one row at a 320px phone width. The verdict travels out of the browser as a request to a magic
URL that lands in the static server's log, so the script has a real exit code —
no screenshots to eyeball. `KEEP=1` preserves the browser/server logs.

## The compile tool

`wasm/src/lib.rs` is a thin `extern "C"` shim over `bs::compile::compile_json`,
the same function the CLI calls — deliberately raw pointers over linear memory
instead of wasm-bindgen, so the build is a plain `cargo build` with no
`wasm-pack`, Node or npm anywhere. About 110 KB gzipped.

Opened and dropped files (source decks and compiled ones) are read as strict
UTF-8, exactly as `bs compile`/`bs play` read them: a file saved in another
encoding (e.g. Windows-1252) is refused with a message instead of being
compiled with its accented characters turned into `?`, and a file starting
with a byte-order mark is refused too (the CLI fails to parse one), with a
message saying to save it without one.

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
crisp on high-DPI screens. The cell size is whatever makes the whole grid fit
the stage, with no minimum: a deck larger than the screen gets small cells
(rotate or go fullscreen for more room) rather than losing its edge columns
or rows.

`viewer.js` is also a port of the terminal player's *pure* parts, so playback matches:

| Rust | JS |
|------|----|
| `PlayablePresentation::grid_at` | `gridAt` (replays `full` + `diff` frames) |
| `player::loop_next` | `loopNext` (bounce / restart / `count`) |
| `player::loop_step` | `loopTick`'s pass counting |
| `Player::loop_wait` | `loopWait` |
| `Player::auto_advance_delay` | `autoAdvanceDelay` (min over auto-play animations) |
| `Player::animation_cluster` | `animationCluster` (overlap-merged skip target) |
| `Player::frame_auto_advance_delay` | `frameAutoAdvanceDelay` |
| `Player::effective_auto_delay` | `effectiveAutoDelay` |
| `Cell::printable` / `narrow_char` (`src/types.rs`) | `printable` / `narrowChar` (the char a cell paints) |

Being a port, it *can* drift — change the player and mirror it here.

A deck reaches the present view from the compile view's **present →**, from the
**library**, from **open** (a compiled `.json` on the device), or by dropping a
file anywhere in the app — a compiled deck plays, a source deck is routed to
compile instead.

Compiling hands the deck straight to the present view in memory — no storage
round-trip, no navigation — and saves a copy to the library on the way past.

| Key | Action |
|-----|--------|
| <kbd>→</kbd> / <kbd>Space</kbd> / <kbd>Enter</kbd> | next frame (skips a whole loop or auto-play animation); with keyboard focus on a button, link or the library panel, Space/Enter (and PageUp/PageDown) are that control's instead |
| <kbd>←</kbd> | previous frame; inside a loop or any animation, the slide right before it |
| <kbd>Shift</kbd>+<kbd>←</kbd>/<kbd>→</kbd> | jump ±10 frames |
| <kbd>Home</kbd> / <kbd>End</kbd> | first / last frame |
| <kbd>f</kbd> | fullscreen; <kbd>Esc</kbd> leaves |
| <kbd>o</kbd> | open a compiled `.json` |

`→` on an auto-play animation lands on the animation's **own last frame** — its
finished state — rather than stepping past it; pressing it again continues. (The
terminal player still jumps one frame further, to the first slide after the
span.) A run of quick auto-advances (markers of a second or less,
`MOTION_DELAY_MS` — how generated decks animate) counts as an animation too:
`→` inside one lands on the frame it stops on. `←` anywhere inside an
animation — auto-play, stepped by hand, or a quick auto-advance run — goes to
the slide right before it (`motionRuns`/`runAt`); the terminal player does that
only for auto-play animations and steps back one frame elsewhere.

**Nothing is keyboard-only.** A phone has no keyboard, so every action also has a
tap target: the footer carries real buttons — `⇤` `⇥` `full` at the left, the
single steps `←` `→` at the right edge, under a right thumb — the top
bar carries `library` and `open`, tapping the canvas steps (left third back, the
rest forward), and the frame bar jumps to any frame.

Fullscreen shows the deck and nothing else: both bars go, and with them the deck
name, the `animating` / `loop` status note, the counter, the frame bar and the
library panel. All that is left is a tiny, faint `✕` in the top-right corner —
the only route back on a device with no <kbd>Esc</kbd>. It is deliberately small
and dim, with generous padding so it stays a comfortable tap target.

On iPhone, `requestFullscreen` is refused for non-video elements, so `full`
hides the bars without entering browser fullscreen. That is the useful half, and
the exit button works the same either way.

The frame bar shows one tick per *step*: a frame, or a whole animation — an
animation range (overlapping ones merged, auto-play or not) or a quick
auto-advance run, including the frame it stops on — since an animation reads as
one slide that moves; clicking an animation's tick goes to its first frame. A
tick cannot render below about a pixel, so past roughly 120 steps on a phone the
ticks would overflow the bar, widen the page, and — because the horizontal
scrollbar then steals width from the stage — shrink the deck itself. Beyond that
point each tick stands for a contiguous run of steps instead, so the bar always
fits exactly and a 2000-frame deck renders at the same scale as a 15-frame one.
Clicking still seeks; loops and animations are still marked.

A small slide number (`12/15`) sits just left of `←` `→`. It disappears in
fullscreen along with the rest of the chrome, which is the point — nothing but
the deck.

`Command` objects cannot run in a browser. The compiler bakes their placeholder
box into the static frames, so the slide renders correctly — the viewer names
the command in the status bar instead of executing it.

## Deployment

`.github/workflows/pages.yml` runs `scripts/build-web.sh` and uploads `_site/`
on every push to `main` touching the engine, `web/`, `wasm/`, a file the build
copies in (format reference, packaged skill, sample deck), the build script or
the workflow. The wasm build runs *before* the upload, so a broken
build fails the deploy and Pages keeps serving the previous version.
