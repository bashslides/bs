// Loader for the bs compiler compiled to WebAssembly (`wasm/src/lib.rs`).
//
// The module is self-contained — no imports, no wasm-bindgen, no npm. Every
// byte of compilation logic is the same Rust `bs compile` runs, so the browser
// cannot disagree with the CLI.
//
// The exported ABI is documented in wasm/src/lib.rs. Two rules matter here:
//   1. Pointers are offsets into `exports.memory`.
//   2. Allocating can GROW that memory, which DETACHES any existing ArrayBuffer
//      view. So every read re-derives its view from `exports.memory.buffer`
//      rather than caching one.

'use strict';

const WASM_URL = 'bs.wasm';

let modPromise = null;

/** Load (once) and return the instantiated module's exports. */
export function loadCompiler() {
  if (!modPromise) modPromise = instantiate();
  return modPromise;
}

async function instantiate() {
  const res = await fetch(WASM_URL, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`could not fetch ${WASM_URL} (HTTP ${res.status})`);

  // instantiateStreaming needs an application/wasm content type. GitHub Pages
  // sends it, but a plain local file server may not — fall back to buffering.
  let instance;
  if (WebAssembly.instantiateStreaming) {
    try {
      ({ instance } = await WebAssembly.instantiateStreaming(res.clone(), {}));
    } catch {
      ({ instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {}));
    }
  } else {
    ({ instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {}));
  }
  return instance.exports;
}

/**
 * Compile source JSON to playable JSON.
 * Throws an Error carrying the compiler's own message when the deck is invalid.
 */
export async function compile(sourceText) {
  const ex = await loadCompiler();
  const input = new TextEncoder().encode(sourceText);

  const ptr = ex.bs_alloc(input.length);
  if (!ptr) throw new Error('wasm: allocation failed');
  try {
    new Uint8Array(ex.memory.buffer, ptr, input.length).set(input);
    const len = ex.bs_compile(ptr, input.length);
    const ok = ex.bs_ok() === 1;
    // Re-derive the view: bs_compile allocated, so the buffer may have moved.
    const out = new Uint8Array(ex.memory.buffer, ex.bs_result_ptr(), len);
    const text = new TextDecoder().decode(out);
    if (!ok) throw new Error(text);
    return text;
  } finally {
    ex.bs_free(ptr, input.length);
  }
}

/** True if this browser can run the compiler at all. */
export function wasmSupported() {
  return typeof WebAssembly === 'object' && typeof WebAssembly.instantiate === 'function';
}
