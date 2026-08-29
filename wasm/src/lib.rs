//! `bs` compiler as a WebAssembly module — the browser's compile tool.
//!
//! This is a thin ABI shim, nothing more: every byte of compilation logic lives
//! in [`bs::compile::compile_json`], the same function `bs compile` calls. That
//! is the point of building the real engine to wasm instead of porting it to
//! JavaScript — the browser cannot disagree with the CLI.
//!
//! # ABI
//!
//! Deliberately raw `extern "C"` over linear memory rather than wasm-bindgen,
//! so the build is a plain `cargo build --target wasm32-unknown-unknown` with no
//! `wasm-pack`, Node or npm anywhere in the toolchain. The JS side is ~40 lines
//! (`web/wasm.js`).
//!
//! Call sequence:
//!
//! 1. `bs_alloc(len)` → pointer to a `len`-byte input buffer; JS writes the
//!    UTF-8 source JSON into it.
//! 2. `bs_compile(ptr, len)` → length of the result. `bs_ok()` then reports
//!    whether the result is playable JSON (`1`) or an error message (`0`).
//! 3. `bs_result_ptr()` → pointer to those result bytes, valid until the next
//!    `bs_compile` call.
//! 4. `bs_free(ptr, len)` → releases the input buffer.
//!
//! Every pointer is an offset into the module's exported `memory`. Growing that
//! memory detaches JS's `ArrayBuffer` view, so the JS side re-reads
//! `exports.memory.buffer` after each call — see `web/wasm.js`.

use std::cell::RefCell;

thread_local! {
    /// The most recent result (playable JSON, or an error message), kept alive
    /// for the `bs_result_ptr` read that follows `bs_compile`. wasm is
    /// single-threaded, so a thread-local is simply a global here.
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Whether `RESULT` holds output (`true`) or an error message (`false`).
    static OK: RefCell<bool> = const { RefCell::new(false) };
}

/// Allocate `len` bytes for JS to write the source JSON into.
///
/// # Safety
/// The returned pointer must be handed back to [`bs_free`] with the same `len`.
#[unsafe(no_mangle)]
pub extern "C" fn bs_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Release a buffer obtained from [`bs_alloc`].
///
/// # Safety
/// `ptr`/`len` must come from a single [`bs_alloc`] call and not be reused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bs_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
    }
}

/// Compile the UTF-8 source JSON at `ptr[..len]`; returns the result's length.
///
/// Never traps on bad input: invalid UTF-8, malformed JSON and rejected decks
/// all come back as an error message with `bs_ok() == 0`.
///
/// # Safety
/// `ptr[..len]` must be readable — i.e. a buffer from [`bs_alloc`] that JS has
/// filled with exactly `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bs_compile(ptr: *const u8, len: usize) -> usize {
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (ok, bytes) = match std::str::from_utf8(input) {
        Err(e) => (false, format!("source is not valid UTF-8: {e}").into_bytes()),
        Ok(text) => match bs::compile::compile_json(text) {
            Ok(json) => (true, json.into_bytes()),
            // `{:#}` renders anyhow's full context chain, matching the CLI's
            // error output.
            Err(e) => (false, format!("{e:#}").into_bytes()),
        },
    };
    let n = bytes.len();
    RESULT.with(|r| *r.borrow_mut() = bytes);
    OK.with(|f| *f.borrow_mut() = ok);
    n
}

/// Pointer to the bytes produced by the last [`bs_compile`] call.
#[unsafe(no_mangle)]
pub extern "C" fn bs_result_ptr() -> *const u8 {
    RESULT.with(|r| r.borrow().as_ptr())
}

/// `1` if the last [`bs_compile`] produced playable JSON, `0` if it produced an
/// error message.
#[unsafe(no_mangle)]
pub extern "C" fn bs_ok() -> u32 {
    OK.with(|f| u32::from(*f.borrow()))
}
