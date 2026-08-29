//! The single compile path: source JSON → playable JSON.
//!
//! Both entry points go through here — the `bs compile` CLI subcommand and the
//! WebAssembly module in `wasm/` that powers the browser compiler. Keeping the
//! pipeline in one function is what makes the two agree by construction: there
//! is no second implementation to drift.
//!
//! This module is deliberately free of I/O (the caller supplies and disposes of
//! the JSON) and of anything the `wasm32-unknown-unknown` target cannot provide
//! — no filesystem, clock, environment, threads or terminal.

use anyhow::{bail, Result};

use crate::engine::{source::SourcePresentation, Engine};
use crate::renderer::Renderer;
use crate::types::{PlayablePresentation, TerminalContract};

/// Compile a parsed source deck into its playable form.
///
/// Validates loop ranges first (a hard gate — the same one the editor surfaces
/// as a live warning), then resolves every object into per-frame draw ops,
/// rasterizes them into full/diff frames, and attaches the play-time sidecars
/// (`Command`, `Loop`, `Animation` and `AutoAdvance` regions) that cannot be
/// baked into static frames.
pub fn compile(source: &SourcePresentation) -> Result<PlayablePresentation> {
    if let Err(e) = source.validate_loops() {
        bail!("invalid loops: {e}");
    }

    let scenes = Engine::compile(source);
    let contract = TerminalContract {
        width: source.width,
        height: source.height,
    };
    let mut presentation = Renderer::render(&scenes, contract);
    presentation.commands = source.command_regions();
    presentation.loops = source.loop_regions();
    presentation.animations = source.animation_regions();
    presentation.auto_advances = source.auto_advance_regions();
    Ok(presentation)
}

/// Compile source JSON text into playable JSON text (pretty-printed).
///
/// The string-in/string-out shape the wasm module exports, and what the CLI
/// wraps in file I/O.
pub fn compile_json(source_json: &str) -> Result<String> {
    let source: SourcePresentation = serde_json::from_str(source_json)?;
    let presentation = compile(&source)?;
    Ok(serde_json::to_string_pretty(&presentation)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECK: &str = r#"{
        "width": 10, "height": 3, "frame_count": 2,
        "objects": [
            { "type": "label", "text": "hi",
              "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
              "frames": { "start": 0, "end": 2 } }
        ]
    }"#;

    #[test]
    fn compile_json_round_trips_a_deck_into_playable_json() {
        let out = compile_json(DECK).expect("deck compiles");
        let playable: PlayablePresentation =
            serde_json::from_str(&out).expect("output parses as a playable presentation");
        assert_eq!(playable.contract.width, 10);
        assert_eq!(playable.contract.height, 3);
        assert_eq!(playable.frames.len(), 2);
    }

    #[test]
    fn compile_json_reports_a_parse_error_rather_than_panicking() {
        let err = compile_json("{ not json").unwrap_err().to_string();
        assert!(!err.is_empty(), "parse failure must surface a message");
    }

    #[test]
    fn compile_rejects_overlapping_loops() {
        let deck = r#"{
            "width": 4, "height": 2, "frame_count": 8,
            "objects": [
                { "type": "loop", "frames": { "start": 0, "end": 4 } },
                { "type": "loop", "frames": { "start": 2, "end": 6 } }
            ]
        }"#;
        let err = compile_json(deck).unwrap_err().to_string();
        assert!(err.contains("invalid loops"), "got: {err}");
    }
}
