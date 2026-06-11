//! Shared helpers for the integration test suite.
//!
//! Tests author presentations in the documented JSON source format, run them
//! through the full `Engine::compile` -> `Renderer::render` pipeline, and then
//! reconstruct a plain-character grid so assertions read like the terminal
//! output a viewer would see.

use bs::engine::{source::SourcePresentation, Engine};
use bs::renderer::Renderer;
use bs::types::{PlayablePresentation, TerminalContract};

/// Parse a source presentation from JSON and run it through the real pipeline.
///
/// Panics on malformed JSON — that is a test bug, not a runtime condition.
pub fn render_json(json: &str) -> PlayablePresentation {
    let source: SourcePresentation =
        serde_json::from_str(json).expect("test source JSON should parse");
    let scenes = Engine::compile(&source);
    let contract = TerminalContract {
        width: source.width,
        height: source.height,
    };
    Renderer::render(&scenes, contract)
}

/// Reconstruct the visible character grid at `frame_index` by replaying the
/// initial full frame plus every diff up to and including that frame.
///
/// Returns one `String` per row (style is ignored — assert on the `Frame`
/// directly when you need to check colors/attributes).
pub fn frame_lines(p: &PlayablePresentation, frame_index: usize) -> Vec<String> {
    p.grid_at(frame_index)
        .into_iter()
        .map(|row| row.into_iter().map(|cell| cell.ch).collect::<String>())
        .collect()
}

/// Convenience: the visible character at `(x, y)` on the given frame.
pub fn char_at(p: &PlayablePresentation, frame_index: usize, x: usize, y: usize) -> char {
    frame_lines(p, frame_index)[y].chars().nth(x).unwrap()
}
