//! End-to-end tests for the compile -> render pipeline.
//!
//! These exercise the full path a presentation takes: source JSON -> `Engine`
//! (resolve objects per frame) -> `Renderer` (rasterize + diff). They are the
//! safety net for refactors to object resolution, z-ordering, and framing.

mod common;

use bs::types::Frame;
use common::{char_at, frame_lines, render_json};

#[test]
fn label_renders_text_at_its_position() {
    let p = render_json(
        r#"{
            "width": 12, "height": 4, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "Hi",
                  "position": { "x": { "fixed": 3 }, "y": { "fixed": 1 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 3, 1), 'H');
    assert_eq!(char_at(&p, 0, 4, 1), 'i');

    let lines = frame_lines(&p, 0);
    assert_eq!(lines[1].trim(), "Hi");
    // Untouched rows stay blank.
    assert!(lines[0].trim().is_empty());
}

#[test]
fn first_frame_is_full_and_later_frames_are_diffs() {
    let p = render_json(
        r#"{
            "width": 8, "height": 3, "frame_count": 3,
            "objects": [
                { "type": "label", "text": "x",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 3 } }
            ]
        }"#,
    );

    assert!(matches!(p.frames[0], Frame::Full { .. }));
    assert!(matches!(p.frames[1], Frame::Diff { .. }));
    assert!(matches!(p.frames[2], Frame::Diff { .. }));

    // A static object produces no changes after the first frame.
    if let Frame::Diff { changes } = &p.frames[1] {
        assert!(changes.is_empty(), "static label should not emit diffs");
    }
}

#[test]
fn animated_position_moves_the_glyph_and_clears_the_old_cell() {
    // Label slides straight down column 0 from row 0 to row 3 over 4 frames.
    let p = render_json(
        r#"{
            "width": 4, "height": 4, "frame_count": 4,
            "objects": [
                { "type": "label", "text": "X",
                  "position": {
                      "x": { "fixed": 0 },
                      "y": { "animated": { "from": 0, "to": 3, "start_frame": 0, "end_frame": 3 } }
                  },
                  "frames": { "start": 0, "end": 4 } }
            ]
        }"#,
    );

    // Frame 0: glyph at the top.
    assert_eq!(char_at(&p, 0, 0, 0), 'X');
    // Frame 3: glyph at the bottom, and the original cell has been cleared.
    assert_eq!(char_at(&p, 3, 0, 3), 'X');
    assert_eq!(char_at(&p, 3, 0, 0), ' ');
}

#[test]
fn higher_z_order_paints_over_lower() {
    // Two glyphs at the same cell; the higher z_order must win.
    let p = render_json(
        r#"{
            "width": 3, "height": 1, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "a",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 }, "z_order": 0 },
                { "type": "label", "text": "b",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 }, "z_order": 5 }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), 'b');
}

#[test]
fn frames_range_end_is_exclusive() {
    // Object is present only on frames 1 and 2 (end = 3 is excluded).
    let p = render_json(
        r#"{
            "width": 3, "height": 1, "frame_count": 4,
            "objects": [
                { "type": "label", "text": "o",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 1, "end": 3 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), ' ', "absent before start");
    assert_eq!(char_at(&p, 1, 0, 0), 'o', "present at start");
    assert_eq!(char_at(&p, 2, 0, 0), 'o', "present within range");
    assert_eq!(char_at(&p, 3, 0, 0), ' ', "absent at exclusive end");
}

#[test]
fn off_grid_object_is_clipped_not_panicked() {
    // Position past the grid bounds must be silently clipped.
    let p = render_json(
        r#"{
            "width": 3, "height": 2, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "edge",
                  "position": { "x": { "fixed": 50 }, "y": { "fixed": 50 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    // Nothing rendered, grid is the right size, no panic.
    let lines = frame_lines(&p, 0);
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.trim().is_empty()));
}
