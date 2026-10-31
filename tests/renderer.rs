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

#[test]
fn grid_at_normalises_a_ragged_full_frame() {
    // Row 1 of the full frame is short; a diff into its missing cell must not
    // panic, and the grid keeps the contract's 2×2 shape.
    let a = Cell { ch: 'a', style: Style::default() };
    let z = Cell { ch: 'Z', style: Style::default() };
    let mut p = two_frame_presentation(vec![CellChange { x: 1, y: 1, cell: z }]);
    p.frames[0] = Frame::Full { cells: vec![vec![a.clone(), a.clone(), a], vec![]] };

    let full = p.grid_at(0);
    assert!(full.iter().all(|row| row.len() == 2), "rows match the contract width");
    assert_eq!((full[0][1].ch, full[1][0].ch), ('a', ' '));
    assert_eq!(p.grid_at(1)[1][1].ch, 'Z');
}

#[test]
fn control_characters_in_a_deck_print_as_spaces() {
    // A hand-edited art piece with a CR and a tab: the cells keep the raw chars,
    // but what the player/preview send to the terminal is a plain space.
    let p = render_json(
        r#"{
            "width": 4, "height": 1, "frame_count": 1,
            "objects": [
                { "type": "art", "art": "a\r\tb",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    let row = &p.grid_at(0)[0];
    let printed: String = row.iter().map(Cell::printable).collect();
    assert_eq!(printed, "a  b");
    assert_eq!(Cell { ch: 'x', style: Style::default() }.printable(), 'x');
}

#[test]
fn wide_and_zero_width_chars_print_as_one_column() {
    // Each cell is printed back to back, so it must be one terminal column: an
    // emoji and a CJK char become `?`, a combining accent a space; ASCII, box
    // drawing and the arrow-head glyphs pass through.
    let print = |ch| Cell { ch, style: Style::default() }.printable();
    assert_eq!(print('\u{1f680}'), '?');
    assert_eq!(print('\u{5b8c}'), '?');
    assert_eq!(print('\u{2630}'), '?'); // wide since Unicode 16
    assert_eq!(print('\u{301}'), ' ');
    for ch in ['a', '~', '\u{2500}', '\u{2502}', '\u{25b6}', '\u{2605}'] {
        assert_eq!(print(ch), ch);
    }
}
