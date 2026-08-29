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
