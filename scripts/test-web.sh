#!/usr/bin/env bash
#
# End-to-end test of the browser toolchain, headless and machine-checked.
#
#   ./scripts/test-web.sh
#
# The claim worth testing is the whole reason the engine is built to wasm rather
# than ported to JavaScript: **the browser compiler and `bs compile` must agree
# byte for byte.** So this script compiles the same deck both ways and diffs.
#
# It also exercises the loaders the compile/present pages depend on: the wasm
# glue, the source→playable→render path, and the frame replay — plus that the
# library panel's clicks never reach the present view's tap-to-step handler,
# and that Enter on a focused button activates it instead of stepping the deck,
# that a delay too long for setTimeout does not fire at once, and that a
# malformed playable is rejected with a message instead of being saved, and
# that the deck library is scoped to the deployment, stored compressed,
# adopts an old localStorage library and stays within its budget, that a deck
# larger than the stage (or under a long status note) is scaled to fit rather
# than clipped, that opened files are read as strict UTF-8, that an animation
# is one frame-bar tick and ← inside one lands on the slide before it, that
# the present view's open compiles a source deck on the fly and presents it,
# and that the footer keeps ←/→ at the right on a desktop, right after `full`
# on a 320px phone, and fits one row there.
#
# How the result gets out of the browser: the test page reports its verdict by
# fetching a magic URL, which lands in the static server's request log. No
# screenshots, no OCR, no manual reading — a real exit code.
set -euo pipefail

cd "$(dirname "$0")/.."

OUT=_site
PORT=${PORT:-8771}
BROWSER=${BROWSER:-firefox}
TIMEOUT=${TIMEOUT:-90}
KEEP=${KEEP:-0}          # KEEP=1 preserves the temp dir (browser + server logs)

say()  { printf '==> %s\n' "$*"; }
fail() { printf '!!! %s\n' "$*" >&2; exit 1; }

# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || fail "cargo not found — run ./scripts/install-toolchain.sh"
command -v "$BROWSER" >/dev/null || fail "$BROWSER not found (override with BROWSER=…)"

work="$(mktemp -d)"
srvlog="$work/server.log"
cleanup() {
  [ -n "${FF_PID:-}" ] && kill "$FF_PID" 2>/dev/null || true
  [ -n "${SRV_PID:-}" ] && kill "$SRV_PID" 2>/dev/null || true
  if [ "$KEEP" = 1 ]; then
    printf '    logs kept in %s\n' "$work" >&2
  else
    rm -rf "$work"
  fi
  rm -f "$OUT/__ref.json" "$OUT/__selftest.html"
}
trap cleanup EXIT

# --- 1. build the site + a CLI reference --------------------------------------
./scripts/build-web.sh >/dev/null
say "Compiling the reference deck with the CLI"
cargo run --quiet -- compile examples/demo.json "$OUT/__ref.json" 2>/dev/null

# --- 2. the test page ---------------------------------------------------------
cat > "$OUT/__selftest.html" <<'HTML'
<!doctype html><meta charset="utf-8"><title>bs selftest</title>
<body style="background:#000;color:#ddd;font:13px monospace;padding:20px">
<pre id="log">running…</pre>
<script type="module">
import { compile, loadCompiler, wasmSupported, withCompiler } from './wasm.js';

const lines = [];
const log = (s) => { lines.push(s); document.getElementById('log').textContent = lines.join('\n'); };

// Report the verdict by fetching a magic URL: it shows up in the static
// server's request log, which the shell script greps for an exit code.
const report = (ok, detail) =>
  fetch(`/__verdict?ok=${ok ? 1 : 0}&detail=${encodeURIComponent(detail)}`).catch(() => {});

(async () => {
  try {
    if (!wasmSupported()) throw new Error('WebAssembly unavailable in this browser');

    // Check res.ok explicitly: a 404's HTML body would otherwise sail through
    // as "content" and surface as a bogus byte-0 mismatch.
    const get = async (url) => {
      const res = await fetch(url, { cache: 'no-cache' });
      if (!res.ok) throw new Error(`fetch ${url} failed: HTTP ${res.status}`);
      return res.text();
    };
    const source = await get('demo.json');
    const reference = await get('__ref.json');

    const t0 = performance.now();
    const got = await compile(source);
    const ms = (performance.now() - t0).toFixed(1);
    log(`compiled in ${ms} ms`);

    // (a) The headline claim: identical to what the CLI produced.
    if (got !== reference) {
      for (let i = 0; i < Math.max(got.length, reference.length); i++) {
        if (got[i] !== reference[i]) {
          log(`MISMATCH at byte ${i}`);
          log(`  wasm: ${JSON.stringify(got.slice(i - 40, i + 40))}`);
          log(`  cli : ${JSON.stringify(reference.slice(i - 40, i + 40))}`);
          return report(false, `output differs from the CLI at byte ${i}`);
        }
      }
      return report(false, `output length differs: wasm ${got.length}, cli ${reference.length}`);
    }
    log(`byte-identical to the CLI (${got.length} bytes)`);

    // (b) The output is actually a usable deck.
    const deck = JSON.parse(got);
    if (deck.frames.length !== 15) throw new Error(`expected 15 frames, got ${deck.frames.length}`);
    if (deck.frames[0].type !== 'full') throw new Error('first frame must be a full frame');
    if (!deck.animations?.length) throw new Error('demo deck should carry an animation region');
    log(`deck: ${deck.frames.length} frames, ${deck.contract.width}x${deck.contract.height}`);

    // (c) Errors surface as messages, not as wasm traps that kill the page.
    let threw = false;
    try { await compile('{ not json'); } catch (e) { threw = true; log(`bad input rejected: ${e.message.slice(0, 60)}`); }
    if (!threw) throw new Error('malformed input should have been rejected');

    // (d) The module survives being reused after an error.
    const again = await compile(source);
    if (again !== reference) throw new Error('second compile after an error did not match');
    log('compiler reusable after an error');

    // (d2) A trap (forced here by reading input far out of bounds) drops the
    // instance; the next compile runs on a fresh one and still matches.
    const before = await loadCompiler();
    let trapMsg = '';
    try { await withCompiler((ex) => ex.bs_compile(0xfffffff0, 64)); } catch (e) { trapMsg = e.message; }
    if (!trapMsg.includes('restarted')) throw new Error(`a trap should restart the compiler, got: ${trapMsg}`);
    if ((await loadCompiler()) === before) throw new Error('the trapped instance was reused');
    if ((await compile(source)) !== reference) throw new Error('compile after a trap did not match');
    log('compiler restarted after a trap');

    // (e) Library clicks bubble through #stage but must not step the deck or
    // reopen the panel. Taps land on the right, where the stage would step forward.
    // First, an old localStorage library (as earlier builds kept it) is put in
    // place: the page's first use of the library adopts it into IndexedDB.
    const oldDeck = { id: 'old-1', name: 'from-localstorage', savedAt: 1, frames: 15,
      width: deck.contract.width, height: deck.contract.height, bytes: got.length, origin: 'compiled', json: got };
    localStorage.setItem('bs:decks:/', JSON.stringify([oldDeck]));
    const lib = await import('./store.js');
    if (!(await lib.save('selftest', got, 'compiled'))) throw new Error('saving to the library failed');
    const ifr = document.createElement('iframe');
    ifr.style.cssText = 'width:900px;height:700px';
    ifr.src = 'index.html#present';
    await new Promise((r) => { ifr.onload = r; document.body.appendChild(ifr); });
    const q = (s) => ifr.contentDocument.querySelector(s);
    const tap = (node) => {
      const b = q('#stage').getBoundingClientRect();
      node.dispatchEvent(new ifr.contentWindow.MouseEvent('click',
        { bubbles: true, clientX: b.right - 2, clientY: b.top + 2 }));
    };
    q('#btn-library').click();
    tap(q('#library-close'));
    if (!q('#library').hidden) throw new Error('library close (no deck) reopened the library');
    q('#btn-library').click();
    await new Promise((r) => setTimeout(r, 200));   // the list is read asynchronously
    tap(q('#library .deck-open'));
    await new Promise((r) => setTimeout(r, 200));   // so is the deck it opens
    const counter = q('#counter').textContent;
    if (counter !== '01/15') throw new Error(`deck opened from the library shows ${counter}, not 01/15`);
    log('library clicks do not reach the stage');

    // (f) Space/Enter on a keyboard-focused button are the button's, not a deck
    // step; on the page they still step. A synthetic keydown never clicks, so
    // check that the deck didn't move and the default action wasn't cancelled.
    const doc = ifr.contentDocument;
    const press = (node, key) => {
      const ev = new ifr.contentWindow.KeyboardEvent('keydown',
        { key, bubbles: true, cancelable: true });
      node.dispatchEvent(ev);
      return ev.defaultPrevented;
    };
    q('#btn-library').focus();
    for (const key of ['Enter', ' ']) {
      if (press(q('#btn-library'), key)) throw new Error(`${JSON.stringify(key)} on a focused button was cancelled`);
    }
    if (q('#counter').textContent !== '01/15') throw new Error('a key on a focused button stepped the deck');
    // A mouse click must not leave its button holding Space/Enter: the click
    // steps (01 -> 02), then Space with focus back on the page steps again.
    const next = q('#btn-next');
    next.focus();
    next.dispatchEvent(new ifr.contentWindow.MouseEvent('click', { bubbles: true, detail: 1 }));
    if (doc.activeElement === next) throw new Error('a mouse click left the button focused');
    press(doc.activeElement || doc.body, ' ');
    if (q('#counter').textContent !== '03/15') {
      throw new Error(`click then Space should reach 03/15, got ${q('#counter').textContent}`);
    }
    log('Space/Enter press a keyboard-focused button and step the deck after a click');

    // (g) A delay too long for setTimeout (>= 2^31 ms wraps, often to 0) must
    // wait for the presenter instead of advancing at once.
    const long = JSON.parse(got);
    long.auto_advances = [{ start_frame: 0, end_frame: 15, delay_ms: 3000000000 }];
    const viewerMod = await ifr.contentWindow.eval("import('./viewer.js')");
    viewerMod.load(long, 'long-delay');
    await new Promise((r) => setTimeout(r, 200));
    if (q('#counter').textContent !== '01/15') {
      throw new Error(`a 3e9 ms auto-advance fired early: ${q('#counter').textContent}`);
    }
    log('a delay past setTimeout range does not fire early');

    // (h) A playable `bs play` would refuse (a diff change without its cell) is
    // rejected with a message when opened, and never reaches the library.
    const bad = { contract: { width: 2, height: 1 }, frames: [
      { type: 'full', cells: [[{ ch: 'a' }, { ch: 'b' }]] },
      { type: 'diff', changes: [{ x: 0, y: 0 }] }] };
    const dt = new ifr.contentWindow.DataTransfer();
    dt.items.add(new ifr.contentWindow.File([JSON.stringify(bad)], 'bad-cell.json'));
    q('#p-file').files = dt.files;
    q('#p-file').dispatchEvent(new ifr.contentWindow.Event('change'));
    await new Promise((r) => setTimeout(r, 200));
    if (q('#msg').hidden || !q('#msg').textContent.includes('malformed')) {
      throw new Error('a malformed playable was not rejected with a message');
    }
    // The message replaces the deck on screen: the old canvas must not stay
    // laid out (an author `display` would beat the hidden attribute).
    if (ifr.contentWindow.getComputedStyle(q('#screen')).display !== 'none') {
      throw new Error('the previous deck stays painted over the rejection message');
    }
    const storeMod = await ifr.contentWindow.eval("import('./store.js')");
    if ((await storeMod.summaries()).some((d) => d.name === 'bad-cell')) {
      throw new Error('a malformed playable was saved to the library');
    }
    log('a malformed playable is rejected and not saved');

    // (i) The library lives in an IndexedDB database named after this
    // deployment's path, stores decks compressed, adopted the old
    // localStorage library (and freed its key), and round-trips a deck intact.
    const names = (await indexedDB.databases?.())?.map((d) => d.name);
    if (names && !names.includes('bs:decks:/')) throw new Error(`no deployment-scoped database: ${names}`);
    if (localStorage.getItem('bs:decks:/') !== null) throw new Error('the old localStorage library was not freed');
    const all = await lib.summaries();
    const adopted = all.find((d) => d.name === 'from-localstorage');
    if (!adopted || (await lib.get(adopted.id))?.json !== got) throw new Error('the old localStorage library was not adopted intact');
    const mine = all.find((d) => d.name === 'selftest');
    if (!mine || (await lib.get(mine.id))?.json !== got) throw new Error('a saved deck does not round-trip');
    if (!(mine.stored * 20 < got.length)) throw new Error(`the deck is not stored compressed: ${mine.stored} of ${got.length} bytes`);
    // A deck far past the old 1.5M-character budget is kept, and costs little.
    const near = JSON.stringify({ contract: { width: 1000, height: 125 },
      frames: Array(4).fill({ type: 'full', cells: Array(125).fill(Array(1000).fill({ ch: ' ' })) }) });
    const kept = await lib.save('large', near, 'compiled');
    if (!kept || near.length < 5000000) throw new Error(`a ${near.length}-character deck was not kept`);
    if ((await lib.get(kept.id))?.json !== near) throw new Error('a large deck does not round-trip');
    if (!(await lib.summaries()).some((d) => d.name === 'selftest')) throw new Error('saving a large deck evicted the library');
    await lib.remove(kept.id);
    // Same name replaces the older entry.
    await lib.save('selftest', got, 'compiled');
    if ((await lib.summaries()).filter((d) => d.name === 'selftest').length !== 1) throw new Error('a same-name save duplicated the deck');
    log(`the library is deployment-scoped and compressed (${got.length} -> ${mine.stored} bytes)`);

    // (j) A deck too large for the stage at a legible size still fits whole:
    // the canvas never overflows #stage (whose overflow is hidden).
    const wide = { contract: { width: 400, height: 120 }, frames: [
      { type: 'full', cells: Array.from({ length: 120 }, () => Array(400).fill({ ch: 'x' })) }] };
    viewerMod.load(wide, 'wide');
    await new Promise((r) => setTimeout(r, 50));
    const box = q('#screen').getBoundingClientRect();
    const stage = q('#stage').getBoundingClientRect();
    if (box.width > stage.width + 0.5 || box.height > stage.height + 0.5) {
      throw new Error(`a 400x120 deck overflows the stage: ${box.width}x${box.height} > ${stage.width}x${stage.height}`);
    }
    log('a deck larger than the stage is scaled to fit, never clipped');

    // A long status note (a command the browser cannot run) on a short, wide
    // frame must not grow the footer under a height-bound deck's canvas.
    ifr.style.height = '320px';
    const noted = JSON.parse(got);
    noted.commands = [{ command: `./${'very-long-demo-script-name-'.repeat(4)}.sh`, args: [],
      start_frame: 0, end_frame: 15, x: 0, y: 0, w: 4, h: 2, status_x: 0, status_y: 0 }];
    viewerMod.load(noted, 'noted');
    await new Promise((r) => setTimeout(r, 200));
    if (!q('#note').textContent.includes('not run in the browser')) throw new Error('the command note is missing');
    const nb = q('#screen').getBoundingClientRect();
    const ns = q('#stage').getBoundingClientRect();
    if (nb.top < ns.top - 0.5 || nb.bottom > ns.bottom + 0.5) {
      throw new Error(`a long note clips the deck: canvas ${nb.top}-${nb.bottom}, stage ${ns.top}-${ns.bottom}`);
    }
    log('a long status note keeps the deck fitted');

    // (k) Cells paint what `bs play` prints (Cell::printable): wide chars as
    // `?`, control and zero-width chars as blanks, the rest unchanged.
    const cases = [['你', '?'], ['✅', '?'], ['😀', '?'], ['\u0301', ' '], ['\u001b', ' '],
      ['\u0085', ' '], ['a', 'a'], ['é', 'é'], ['─', '─'], ['✓', '✓'], ['⌀', '⌀']];
    for (const [ch, want] of cases) {
      const shown = viewerMod.printable(ch);
      if (shown !== want) throw new Error(`printable(${JSON.stringify(ch)}) is ${JSON.stringify(shown)}, not ${JSON.stringify(want)}`);
    }
    log('cells paint the same one-column chars as bs play');

    // (l) Source files are read as strict UTF-8, like `bs compile`: a Latin-1
    // file is refused with a message (not compiled with U+FFFD, not saved),
    // and a UTF-8 file with a byte-order mark is refused as the CLI refuses it.
    const openSource = async (name, bytes) => {
      const t = new ifr.contentWindow.DataTransfer();
      t.items.add(new ifr.contentWindow.File([bytes], name));
      q('#c-file').files = t.files;
      q('#c-file').dispatchEvent(new ifr.contentWindow.Event('change'));
      await new Promise((r) => setTimeout(r, 300));
      return q('#c-status').textContent;
    };
    const deckText = '{"width":8,"height":1,"frame_count":1,"objects":[{"type":"label","text":"Caf\u00e9",'
      + '"position":{"x":{"fixed":0},"y":{"fixed":0}},"frames":{"start":0,"end":1}}]}';
    const latin1 = new Uint8Array(Array.from(deckText, (c) => c.charCodeAt(0)));
    if (!(await openSource('latin1.json', latin1)).includes('not valid UTF-8')) {
      throw new Error('a Latin-1 source was not refused as invalid UTF-8');
    }
    const bom = new Uint8Array([0xef, 0xbb, 0xbf, ...new TextEncoder().encode(deckText)]);
    if (!(await openSource('bom.json', bom)).includes('byte-order mark')) {
      throw new Error('a source with a byte-order mark was not rejected like the CLI does');
    }
    const libNow = await (await import('./store.js')).summaries();
    if (libNow.some((d) => d.name === 'latin1' || d.name === 'bom')) {
      throw new Error('a rejected source was saved to the library');
    }
    log('non-UTF-8 and BOM sources are rejected like bs compile');

    // (m) An animation range is one frame-bar tick, and ← inside any animation
    // (auto-play or stepped by hand) lands on the slide right before it.
    ifr.style.height = '700px';
    q('#tab-present').click();
    const anim = JSON.parse(got);   // demo: one auto-play animation over frames 3..12
    const [a] = anim.animations;
    viewerMod.load(anim, 'anim');
    await new Promise((r) => setTimeout(r, 50));
    const ticks = q('#framebar').children.length;
    const want = anim.frames.length - (a.end_frame - a.start_frame) + 1;
    if (ticks !== want) throw new Error(`the frame bar has ${ticks} ticks, not ${want} (one per animation)`);
    const key = (k) => doc.body.dispatchEvent(new ifr.contentWindow.KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
    const at = () => Number(q('#counter').textContent.split('/')[0]);
    a.auto_play = false;            // stepped by hand: → walks into it one frame at a time
    viewerMod.load(anim, 'manual');
    for (let i = 0; i < a.start_frame + 3; i++) key('ArrowRight');
    if (at() !== a.start_frame + 4) throw new Error(`stepping into a manual animation reached ${at()}`);
    key('ArrowLeft');
    if (at() !== a.start_frame) throw new Error(`← inside a manual animation went to ${at()}, not slide ${a.start_frame}`);
    // A run of quick auto-advances (generated decks animate this way) is one
    // animation too: one tick, → lands on the frame it stops on, ← leaves it.
    // A slow marker (a timed slide) is not.
    const aa = JSON.parse(got);
    delete aa.animations;
    aa.auto_advances = [{ start_frame: 5, end_frame: 10, delay_ms: 150 },
      { start_frame: 12, end_frame: 14, delay_ms: 5000 }];
    viewerMod.load(aa, 'auto-advance');
    await new Promise((r) => setTimeout(r, 50));
    if (q('#framebar').children.length !== 10) throw new Error(`a quick auto-advance run is not one tick: ${q('#framebar').children.length} ticks`);
    for (let i = 0; i < 5; i++) key('ArrowRight');
    if (at() !== 6) throw new Error(`stepping onto the run reached ${at()}, not 6`);
    key('ArrowRight');
    if (at() !== 11) throw new Error(`→ inside a quick auto-advance run went to ${at()}, not its last slide 11`);
    key('ArrowLeft');
    if (at() !== 5) throw new Error(`← on a quick auto-advance run went to ${at()}, not slide 5 before it`);
    // ← while the run is playing (its 150 ms timer has moved it on) leaves it
    // for good: slide 5 does not auto-advance back into the run.
    viewerMod.load(aa, 'auto-advance');
    for (let i = 0; i < 5; i++) key('ArrowRight');
    await new Promise((r) => setTimeout(r, 400));
    if (at() <= 6) throw new Error(`the quick auto-advance run did not play: at ${at()}`);
    key('ArrowLeft');
    await new Promise((r) => setTimeout(r, 400));
    if (at() !== 5) throw new Error(`← while the run plays went to ${at()} (after 400 ms), not slide 5`);
    log('an animation or a quick auto-advance run is one tick; ← inside it goes to the slide before');

    // (n) The footer: on a desktop ←/→ at the right edge; at a 320px phone
    // width right after `full` (the counter at the end), every control on one
    // row at 48px (20% over 40px).
    const order = ['btn-first', 'btn-last', 'btn-full', 'btn-prev', 'btn-next'];
    const footRight = () => q('#foot').getBoundingClientRect().right;
    const right = (n) => q(`#${n}`).getBoundingClientRect().right;
    if (footRight() - right('btn-next') > 14) throw new Error('on a desktop → is not at the right edge');
    ifr.style.width = '320px';
    await new Promise((r) => setTimeout(r, 100));
    const rects = order.map((n) => q(`#${n}`).getBoundingClientRect());
    if (rects.some((r) => Math.round(r.height) < 48)) throw new Error(`phone buttons are not 48px tall: ${rects.map((r) => r.height)}`);
    if (new Set(rects.map((r) => Math.round(r.top))).size !== 1) throw new Error(`the footer controls wrap onto two rows at 320px: ${JSON.stringify(rects.map((r) => [Math.round(r.left), Math.round(r.top), Math.round(r.width)]))} counter ${q('#counter').getBoundingClientRect().width} foot ${q('#foot').getBoundingClientRect().width}`);
    for (let i = 1; i < rects.length; i++) {
      if (rects[i].left < rects[i - 1].right || rects[i].left - rects[i - 1].right > 4) {
        throw new Error(`on a phone ${order[i]} does not follow ${order[i - 1]}: ${JSON.stringify(rects.map((r) => Math.round(r.left)))}`);
      }
    }
    const counterBox = q('#counter').getBoundingClientRect();
    if (counterBox.left < rects[4].right || footRight() - counterBox.right > 12) throw new Error('on a phone the counter is not at the end of the row');
    if (doc.documentElement.scrollWidth > 320) throw new Error(`the page scrolls sideways at 320px (${doc.documentElement.scrollWidth})`);
    // A three-digit counter still fits the row.
    const long3 = { contract: { width: 4, height: 1 }, frames: [{ type: 'full', cells: [[{ ch: 'x' }]] },
      ...Array(119).fill({ type: 'diff', changes: [] })] };
    viewerMod.load(long3, 'long');
    key('End');
    if (q('#counter').textContent !== '120/120') throw new Error(`End shows ${q('#counter').textContent}`);
    const tops = order.map((n) => Math.round(q(`#${n}`).getBoundingClientRect().top));
    if (new Set(tops).size !== 1) throw new Error(`a three-digit counter wraps the footer at 320px: ${JSON.stringify(order.map((n) => q(`#${n}`).getBoundingClientRect().width))} counter ${q('#counter').getBoundingClientRect().width}`);
    log('footer: steps at the right on a desktop, after full on a phone, one row at 320px');

    // (o) open in the present view takes a source deck: it compiles on the fly
    // and plays; one that does not compile lands in the compile view with
    // the compiler's message.
    ifr.style.width = '900px';
    const openDeck = async (name, text) => {
      const t = new ifr.contentWindow.DataTransfer();
      t.items.add(new ifr.contentWindow.File([text], name));
      q('#p-file').files = t.files;
      q('#p-file').dispatchEvent(new ifr.contentWindow.Event('change'));
      await new Promise((r) => setTimeout(r, 600));
    };
    q('#tab-present').click();
    await openDeck('fresh-source.json', source);
    if (q('#view-present').hidden || q('#screen').hidden) throw new Error('opening a source deck did not present it');
    if (q('#counter').textContent !== '01/15') throw new Error(`a source deck opened in present shows ${q('#counter').textContent}`);
    if (!(await storeMod.summaries()).some((d) => d.name === 'fresh-source')) throw new Error('a source deck opened in present was not saved');
    q('#tab-present').click();
    await openDeck('broken-source.json', '{"width": 8}');
    if (q('#view-compile').hidden) throw new Error('a source deck that does not compile did not go to the compile view');
    if (!q('#c-status').classList.contains('bad')) throw new Error('the compile error is not shown');
    log('open compiles a source deck and presents it; a broken one goes to compile');

    log('\nALL CHECKS PASSED');
    report(true, 'all checks passed');
  } catch (err) {
    log(`FAILED: ${err.message}`);
    report(false, err.message);
  }
})();
</script>
HTML

# --- 3. serve + drive a real browser ------------------------------------------
python3 -m http.server -d "$OUT" "$PORT" >"$srvlog" 2>&1 &
SRV_PID=$!
for _ in $(seq 1 40); do
  curl -sf -o /dev/null "http://127.0.0.1:$PORT/__selftest.html" && break
  sleep 0.25
done

say "Driving $BROWSER headless against the built site"
# HOME points at the temp dir so the browser builds a throwaway profile there;
# passing --profile/--no-remote explicitly makes Firefox hang in this setup.
HOME="$work" MOZ_HEADLESS=1 "$BROWSER" --headless \
  "http://127.0.0.1:$PORT/__selftest.html" >"$work/browser.log" 2>&1 &
FF_PID=$!

# --- 4. wait for the verdict --------------------------------------------------
verdict=""
for _ in $(seq 1 $((TIMEOUT * 4))); do
  if grep -q '__verdict' "$srvlog" 2>/dev/null; then
    verdict=$(grep -o '__verdict?[^ ]*' "$srvlog" | head -1)
    break
  fi
  sleep 0.25
done

if [ -z "$verdict" ]; then
  echo "--- server log ---" >&2; tail -20 "$srvlog" >&2 || true
  echo "--- browser log ---" >&2; tail -20 "$work/browser.log" >&2 || true
  fail "no verdict after ${TIMEOUT}s — the test page never reported"
fi

detail=$(python3 -c "
import sys, urllib.parse as u
q = u.parse_qs(sys.argv[1].split('?', 1)[1])
print(q.get('detail', [''])[0])
" "$verdict")

if [[ "$verdict" == *"ok=1"* ]]; then
  say "PASS — $detail"
  say "The browser compiler and \`bs compile\` produce identical output."
  exit 0
fi

printf '!!! FAIL — %s\n' "$detail" >&2
exit 1
