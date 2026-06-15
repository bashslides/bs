//! Renderer rasterization details and `PlayablePresentation::grid_at` replay.

mod common;
use bs::types::{Cell, CellChange, Frame, PlayablePresentation, Style, TerminalContract};
use common::{char_at, render_json};

#[test]
fn equal_z_order_keeps_source_order() {
    // Two glyphs at the same cell with the same z_order: the one later in the
    // object list wins (stable sort by z_order preserves source order).
    let p = render_json(
        r#"{
            "width": 1, "height": 1, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "a",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 }, "z_order": 0 },
                { "type": "label", "text": "b",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 }, "z_order": 0 }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), 'b');
}

/// A presentation with one full frame and one diff frame, built by hand so the
/// diff can carry an out-of-bounds change.
fn two_frame_presentation(diff: Vec<CellChange>) -> PlayablePresentation {
    PlayablePresentation {
        contract: TerminalContract { width: 2, height: 2 },
        frames: vec![
            Frame::Full { cells: vec![vec![Cell::default(); 2]; 2] },
            Frame::Diff { changes: diff },
        ],
        markers: Vec::new(),
        commands: Vec::new(),
        loops: Vec::new(),
        animations: Vec::new(),
        auto_advances: Vec::new(),
    }
}

#[test]
fn grid_at_clamps_a_frame_index_past_the_end() {
    let q = Cell { ch: 'Q', style: Style::default() };
    let p = two_frame_presentation(vec![CellChange { x: 0, y: 0, cell: q }]);

    let last = p.grid_at(1);
    let clamped = p.grid_at(99);
    assert_eq!(clamped, last, "an out-of-range frame clamps to the last frame");
    assert_eq!(clamped[0][0].ch, 'Q');
}

#[test]
fn grid_at_skips_out_of_bounds_diff_changes() {
    // A diff change at x=99 (outside the 2×2 grid) must be ignored, not panic.
    let z = Cell { ch: 'Z', style: Style::default() };
    let p = two_frame_presentation(vec![CellChange { x: 99, y: 0, cell: z }]);

    let grid = p.grid_at(1);
    assert_eq!(grid.len(), 2);
    assert!(grid.iter().all(|row| row.iter().all(|c| c.ch == ' ')));
}
