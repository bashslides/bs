//! Engine compile-level behavior: one resolved scene per frame, empty
//! presentations, and objects whose frame range never intersects the deck.

mod common;
use bs::engine::{source::SourcePresentation, Engine};
use common::{frame_lines, render_json};

#[test]
fn compile_produces_one_scene_per_frame() {
    let source: SourcePresentation = serde_json::from_str(
        r#"{ "width": 4, "height": 2, "frame_count": 5, "objects": [] }"#,
    )
    .unwrap();
    let scenes = Engine::compile(&source);
    assert_eq!(scenes.len(), 5);
}

#[test]
fn empty_presentation_renders_blank_frames() {
    let p = render_json(
        r#"{ "width": 4, "height": 2, "frame_count": 3, "objects": [] }"#,
    );
    assert_eq!(p.frames.len(), 3);
    for f in 0..3 {
        assert!(
            frame_lines(&p, f).iter().all(|l| l.trim().is_empty()),
            "frame {f} should be blank",
        );
    }
}

#[test]
fn object_with_frame_range_outside_the_deck_is_never_drawn() {
    // The label lives on frames 5..6, but the deck only has frames 0..2.
    let p = render_json(
        r#"{
            "width": 4, "height": 2, "frame_count": 2,
            "objects": [
                { "type": "label", "text": "x",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 5, "end": 6 } }
            ]
        }"#,
    );
    for f in 0..2 {
        assert!(
            frame_lines(&p, f).iter().all(|l| l.trim().is_empty()),
            "frame {f} should be blank",
        );
    }
}
