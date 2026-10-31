// Loader for the bs compiler compiled to WebAssembly (`wasm/src/lib.rs`).
//
// The module is self-contained — no imports, no wasm-bindgen, no npm. Every
// byte of compilation logic is the same Rust `bs compile` runs, so the browser
// cannot disagree with the CLI.
//
// The exported ABI is documented in wasm/src/lib.rs. Three rules matter here:
//   1. Pointers are offsets into `exports.memory`.
//   2. Allocating can GROW that memory, which DETACHES any existing ArrayBuffer
//      view. So every read re-derives its view from `exports.memory.buffer`
//      rather than caching one.
//   3. wasm32 returns pointers/lengths as i32, which JS reads as SIGNED; one at
//      or above 2 GiB would go negative. Normalise each with `>>> 0`.
//   4. The module is built with panic = "abort", so a panic or an allocation
//      failure is a trap (WebAssembly.RuntimeError) that unwinds nothing: the
//      aborted call's heap and shadow stack stay used. A trapped instance is
//      therefore dropped and the next call instantiates a fresh one
//      (`withCompiler`).

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
 * Run `fn(exports)` on the compiler. If it traps (rule 4), drop the instance so
 * the next call starts from a fresh one, and throw an Error saying so.
 */
export async function withCompiler(fn) {
  const ex = await loadCompiler();
  try {
    return fn(ex);
  } catch (err) {
    if (!(err instanceof WebAssembly.RuntimeError)) throw err;
    modPromise = null;
    throw new Error(
      `the compiler crashed (${err.message}), probably out of memory; ` +
      'it has been restarted for the next compile');
  }
}

/**
 * Compile source JSON to playable JSON.
 * Throws an Error carrying the compiler's own message when the deck is invalid.
 */
export async function compile(sourceText) {
  const input = new TextEncoder().encode(sourceText);
  const [ok, text] = await withCompiler((ex) => {
    const ptr = ex.bs_alloc(input.length) >>> 0;
    if (!ptr) throw new Error('wasm: allocation failed');
    let result;
    try {
      new Uint8Array(ex.memory.buffer, ptr, input.length).set(input);
      const len = ex.bs_compile(ptr, input.length) >>> 0;
      const ok = ex.bs_ok() === 1;
      // Re-derive the view: bs_compile allocated, so the buffer may have moved.
      const out = new Uint8Array(ex.memory.buffer, ex.bs_result_ptr() >>> 0, len);
      result = [ok, new TextDecoder().decode(out)];
    } catch (err) {
      // A trapped instance is dropped whole; don't call into it again.
      if (!(err instanceof WebAssembly.RuntimeError)) ex.bs_free(ptr, input.length);
      throw err;
    }
    ex.bs_free(ptr, input.length);
    return result;
  });
  if (!ok) throw new Error(text);
  return text;
}

/** True if this browser can run the compiler at all. */
export function wasmSupported() {
  return typeof WebAssembly === 'object' && typeof WebAssembly.instantiate === 'function';
}
