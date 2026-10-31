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
# that the deck library is scoped to the deployment and kept within its budget,
# that a deck larger than the stage (or under a long status note) is scaled
# to fit rather than clipped, and that opened files are read as strict UTF-8.
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
    (await import('./store.js')).save('selftest', got, 'compiled');
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
    tap(q('#library .deck-open'));
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
    if (storeMod.list().some((d) => d.name === 'bad-cell')) {
      throw new Error('a malformed playable was saved to the library');
    }
    log('a malformed playable is rejected and not saved');

    // (i) The library is scoped to this deployment's path and stays under its
    // byte budget: nothing under the old origin-wide key, and a deck larger
    // than the budget on its own is not kept (the rest of the library is).
    const lib = await import('./store.js');
    if (localStorage.getItem('bs:decks') !== null || localStorage.getItem('bs:decks:/') === null) {
      throw new Error('the library is not stored under the deployment-scoped key');
    }
    const huge = JSON.stringify({ contract: { width: 1, height: 1 }, frames: [], pad: 'x'.repeat(1600000) });
    if (lib.save('too-big', huge, 'compiled') !== null) throw new Error('a deck over the budget was saved');
    if (!lib.list().some((d) => d.name === 'selftest')) throw new Error('an over-budget save evicted the library');
    // A realistic, quote-heavy deck just under the budget (~1.38M characters,
    // ~1.9M once escaped in storage) is kept: the budget counts its own size.
    const near = JSON.stringify({ contract: { width: 1000, height: 125 },
      frames: [{ type: 'full', cells: Array(125).fill(Array(1000).fill({ ch: ' ' })) }] });
    const kept = lib.save('near-limit', near, 'compiled');
    if (!kept || near.length > 1500000) throw new Error(`a ${near.length}-character deck under the budget was not kept`);
    lib.remove(kept.id);
    // Migration: with only an old unscoped library, a fresh load adopts it into
    // the scoped key and frees the old key.
    localStorage.setItem('bs:decks', JSON.stringify(lib.list()));
    localStorage.removeItem('bs:decks:/');
    const migrated = await import('./store.js?migrate');
    if (localStorage.getItem('bs:decks') !== null) throw new Error('the old library key was not freed');
    if (!migrated.list().some((d) => d.name === 'selftest')) throw new Error('the old library was not adopted');
    log('the library is deployment-scoped and within its budget');

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
    const libNow = (await import('./store.js')).list();
    if (libNow.some((d) => d.name === 'latin1' || d.name === 'bom')) {
      throw new Error('a rejected source was saved to the library');
    }
    log('non-UTF-8 and BOM sources are rejected like bs compile');

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
