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
use crate::types::{
    PlayablePresentation, ResolvedScene, TerminalContract, MAX_DECK_OPS, MAX_STORED_CELLS,
};

/// Compile a parsed source deck into its playable form.
///
/// Rejects a deck outside the contract limits (no frames, or too many
/// frames/cells — `SourcePresentation::validate_contract`) and validates loop
/// ranges first (hard gates; the loop check is the one the editor surfaces as
/// a live warning), then resolves every object into per-frame draw ops and
/// rasterizes them into full/diff frames — failing once one frame draws more
/// than `MAX_FRAME_OPS` cells, the frames together draw more than
/// `MAX_DECK_OPS`, or they would store more than `MAX_STORED_CELLS` — and
/// attaches the play-time sidecars (`Command`, `Loop`, `Animation` and
/// `AutoAdvance` regions) that cannot be baked into static frames.
pub fn compile(source: &SourcePresentation) -> Result<PlayablePresentation> {
    if let Err(e) = source.validate_contract() {
        bail!("{e}");
    }
    if let Err(e) = source.validate_loops() {
        bail!("invalid loops or animations: {e}");
    }

    let contract = TerminalContract {
        width: source.width,
        height: source.height,
    };
    let mut presentation = render_scenes(Engine::scenes(source), contract)?;
    presentation.commands = source.command_regions();
    presentation.loops = source.loop_regions();
    presentation.animations = source.animation_regions();
    presentation.auto_advances = source.auto_advance_regions();
    Ok(presentation)
}

/// Rasterize fallible scenes (`Engine::scenes`) within `MAX_STORED_CELLS`,
/// stopping at the first scene error (a frame over `MAX_FRAME_OPS`), once the
/// frames' ops together pass `MAX_DECK_OPS`, or at the stored-cell limit,
/// whichever comes first. Shared with the editor preview.
pub fn render_scenes(
    scenes: impl IntoIterator<Item = Result<ResolvedScene, String>>,
    contract: TerminalContract,
) -> Result<PlayablePresentation> {
    render_scenes_within(scenes, contract, MAX_DECK_OPS)
}

fn render_scenes_within(
    scenes: impl IntoIterator<Item = Result<ResolvedScene, String>>,
    contract: TerminalContract,
    max_deck_ops: u64,
) -> Result<PlayablePresentation> {
    let mut scene_err = None;
    let mut total = 0u64;
    let ok_scenes = scenes.into_iter().enumerate().map_while(|(frame, s)| {
        let scene = s.map_err(|e| scene_err = Some(e)).ok()?;
        total += scene.ops.len() as u64;
        if total > max_deck_ops {
            scene_err = Some(format!(
                "frames 1-{} draw over {max_deck_ops} cells in total (counting overlapping \
                 objects); use fewer frames or fewer overlapping objects",
                frame + 1
            ));
            return None;
        }
        Some(scene)
    });
    let rendered = Renderer::render_bounded(ok_scenes, contract, MAX_STORED_CELLS);
    match (scene_err, rendered) {
        (Some(e), _) | (None, Err(e)) => bail!("{e}"),
        (None, Ok(p)) => Ok(p),
    }
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
        assert!(err.contains("invalid loops or animations"), "got: {err}");
    }

    #[test]
    fn compile_rejects_an_oversized_canvas() {
        let deck = r#"{ "width": 65535, "height": 65535, "frame_count": 1, "objects": [] }"#;
        let err = compile_json(deck).unwrap_err().to_string();
        assert!(err.contains("canvas 65535×65535"), "got: {err}");
    }

    #[test]
    fn compile_rejects_an_oversized_frame_count() {
        let deck = r#"{ "width": 10, "height": 3, "frame_count": 2000000000, "objects": [] }"#;
        let err = compile_json(deck).unwrap_err().to_string();
        assert!(err.contains("frame_count 2000000000"), "got: {err}");
    }

    #[test]
    fn compile_rejects_too_many_cells_in_total() {
        // Each limit alone is met (1000×1000 canvas, 51 frames); the product isn't.
        let deck = r#"{ "width": 1000, "height": 1000, "frame_count": 51, "objects": [] }"#;
        let err = compile_json(deck).unwrap_err().to_string();
        assert!(err.contains("51 frames of 1000×1000"), "got: {err}");
    }

    #[test]
    fn compile_rejects_a_deck_that_stores_too_many_cells() {
        // Within every contract limit, but a full-canvas background toggling on
        // frame 1 makes each of the 3 frames store 1M cells (3M > the 2M budget).
        let deck = r#"{
            "width": 1000, "height": 1000, "frame_count": 3,
            "objects": [
                { "type": "label", "text": "", "width": 1000, "height": 1000,
                  "style": { "bg": "red" },
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 1, "end": 2 } }
            ]
        }"#;
        let err = compile_json(deck).unwrap_err().to_string();
        assert!(err.contains("would store over 2000000 cells"), "got: {err}");
    }

    #[test]
    fn compile_rejects_a_frame_of_too_many_overlapping_objects() {
        // 300 full-canvas labels on one frame: every contract and stored-cell
        // limit is met, but the frame draws 300M cells. Resolution stops at the
        // ninth label (9M > MAX_FRAME_OPS), so this fails fast in bounded memory.
        let label = r#"{ "type": "label", "text": "", "width": 1000, "height": 1000,
            "style": { "bg": "red" },
            "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
            "frames": { "start": 0, "end": 1 } }"#;
        let deck = format!(
            r#"{{ "width": 1000, "height": 1000, "frame_count": 1, "objects": [{}] }}"#,
            vec![label; 300].join(",")
        );
        let err = compile_json(&deck).unwrap_err().to_string();
        assert!(err.contains("frame 1 draws over 8000000 cells"), "got: {err}");
    }

    #[test]
    fn rendering_stops_once_the_frames_draw_too_much_in_total() {
        // Every frame is within MAX_FRAME_OPS; the sum is what a small deck
        // budget (standing in for MAX_DECK_OPS) rejects, naming the frame.
        let src: SourcePresentation = serde_json::from_str(
            r#"{ "width": 10, "height": 2, "frame_count": 5, "objects": [
                { "type": "label", "text": "", "width": 10, "height": 2,
                  "style": { "bg": "red" },
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 5 } } ] }"#,
        )
        .unwrap();
        let contract = TerminalContract { width: 10, height: 2 };
        let err = render_scenes_within(Engine::scenes(&src), contract.clone(), 60).unwrap_err();
        assert!(err.to_string().contains("frames 1-4 draw over 60 cells"), "got: {err}");
        assert!(render_scenes_within(Engine::scenes(&src), contract, 100).is_ok());
        assert!(compile(&src).is_ok(), "a normal deck is far from MAX_DECK_OPS");
    }

    #[test]
    fn compile_rejects_a_deck_with_no_frames() {
        let deck = r#"{ "width": 10, "height": 3, "frame_count": 0, "objects": [] }"#;
        let err = compile_json(deck).unwrap_err().to_string();
        assert!(err.contains("no frames"), "got: {err}");
    }
}
