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
# glue, the source→playable→render path, and the frame replay.
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
import { compile, wasmSupported } from './wasm.js';

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
