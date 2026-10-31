#!/usr/bin/env bash
#
# Assemble the static site into _site/ — the exact tree GitHub Pages serves.
#
#   ./scripts/build-web.sh                 # build
#   ./scripts/build-web.sh --serve         # build, then serve on :8000
#
# Three ingredients:
#   1. web/                     the pages themselves (checked in)
#   2. wasm/ → bs.wasm          the engine, so the compile page runs the real
#                               compiler rather than a re-implementation
#   3. PRESENTATION_FORMAT.md   copied from the repo root, so the instructions
#                               page can never drift from the reference
#   4. .claude/skills/…/SKILL.md the packaged Claude skill, offered for download
#   5. examples/demo.json       the sample source deck the compile view loads
#
# It also stamps sw.js with a build hash so the PWA cache invalidates on deploy.
#
# The same script runs in CI (.github/workflows/pages.yml), so a green local
# build is the thing that gets deployed.
set -euo pipefail

cd "$(dirname "$0")/.."

OUT=_site
WASM_TARGET=wasm32-unknown-unknown
WASM_PROFILE=wasm-release
SERVE=0
PORT=8000

for arg in "$@"; do
  case "$arg" in
    --serve) SERVE=1 ;;
    --port=*) PORT="${arg#--port=}" ;;
    -h|--help) sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//;$d'; exit 0 ;;
    *) echo "error: unknown option $arg (try --help)" >&2; exit 1 ;;
  esac
done

say() { printf '==> %s\n' "$*"; }

# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || {
  echo "error: cargo not found — run ./scripts/install-toolchain.sh first" >&2
  exit 1
}

# --- 1. the engine, as WebAssembly -------------------------------------------
if ! rustup target list --installed 2>/dev/null | grep -qx "$WASM_TARGET"; then
  echo "error: the $WASM_TARGET target is missing — run ./scripts/install-toolchain.sh" >&2
  exit 1
fi

say "Building the compiler for $WASM_TARGET"
cargo build -p bs-wasm --target "$WASM_TARGET" --profile "$WASM_PROFILE"

WASM_SRC="target/$WASM_TARGET/$WASM_PROFILE/bs_wasm.wasm"
[ -f "$WASM_SRC" ] || { echo "error: $WASM_SRC was not produced" >&2; exit 1; }

# --- 2. assemble --------------------------------------------------------------
say "Assembling $OUT/"
rm -rf "$OUT"
mkdir -p "$OUT"
cp -R web/. "$OUT"/
cp "$WASM_SRC" "$OUT/bs.wasm"
cp PRESENTATION_FORMAT.md "$OUT/presentation-format.md"
cp .claude/skills/bs-deck/SKILL.md "$OUT/bs-deck-skill.md"
cp examples/demo.json "$OUT/demo.json"
touch "$OUT/.nojekyll"

# Stamp the service worker's cache name with a hash of every file we just built
# (names and contents, sw.js still unstamped), so any deploy — even a docs- or
# sample-only one — installs a fresh cache instead of serving stale files.
# Portable to BSD/macOS userlands: `shasum -a 256` where `sha256sum` is missing
# (same output format, so the same hash), and no GNU-only in-place `sed -i`.
if command -v sha256sum >/dev/null; then SHA256=(sha256sum); else SHA256=(shasum -a 256); fi
BUILD=$(cd "$OUT" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 "${SHA256[@]}" \
  | "${SHA256[@]}" | cut -c1-12)
sed "s/__BUILD__/$BUILD/" "$OUT/sw.js" > "$OUT/sw.js.tmp"
mv "$OUT/sw.js.tmp" "$OUT/sw.js"
say "Service worker build: $BUILD"

# --- 3. report ----------------------------------------------------------------
wasm_bytes=$(wc -c < "$OUT/bs.wasm")
wasm_gz=$(gzip -9 -c "$OUT/bs.wasm" | wc -c)
say "bs.wasm: $((wasm_bytes / 1024)) KB raw, $((wasm_gz / 1024)) KB gzipped"
say "Site ready in $OUT/ ($(find "$OUT" -type f | wc -l) files)"

if [ "$SERVE" = 1 ]; then
  say "Serving http://localhost:$PORT  (Ctrl-C to stop)"
  exec python3 -m http.server -d "$OUT" "$PORT"
fi
