pub mod art_library;
pub mod compile;
pub mod engine;
pub mod migrate;
pub mod renderer;
pub mod types;

// The terminal UI. Gated behind the default-on `tui` feature so the engine can
// be built for targets without a terminal — notably `wasm32-unknown-unknown`,
// where `crossterm` (and its libc/mio/signal-hook dependencies) cannot compile.
// Everything above is pure computation and builds everywhere.
#[cfg(feature = "tui")]
pub mod editor;
#[cfg(feature = "tui")]
pub mod menubar;
#[cfg(feature = "tui")]
pub mod player;
#[cfg(feature = "tui")]
pub mod term;

/// Test helper: a path in the system temp dir unique to this test process
/// (`<pid>_<name>`), so concurrent `cargo test` runs on one host never share
/// (or race on) a test file. A leftover *file* there is removed; a directory
/// is left to the caller.
#[cfg(test)]
pub(crate) fn test_tmp(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("{}_{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}
