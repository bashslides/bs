//! `Header` object: 5-row bitmap-font glyphs filled with a configurable
//! character, laid out left to right with a one-column gap between glyphs.
//!
//! Expectations are derived from the `I` glyph in the font:
//! `["###", " # ", " # ", " # ", "###"]` (3 wide, 5 tall).

mod common;
use common::{char_at, render_json};

#[test]
fn glyph_is_filled_with_the_default_block_character() {
    let p = render_json(
        r#"{
            "width": 10, "height": 7, "frame_count": 1,
            "objects": [
                { "type": "header", "text": "I",
                  "position": { "x": { "fixed": 1 }, "y": { "fixed": 1 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    // Top bar (row 0 of the glyph): all three columns filled.
    assert_eq!(char_at(&p, 0, 1, 1), '█');
    assert_eq!(char_at(&p, 0, 2, 1), '█');
    assert_eq!(char_at(&p, 0, 3, 1), '█');
    // Middle stem (row 1): only the centre column.
    assert_eq!(char_at(&p, 0, 2, 2), '█');
    assert_eq!(char_at(&p, 0, 1, 2), ' ');
    // Bottom bar (row 4 → y = 1 + 4 = 5).
    assert_eq!(char_at(&p, 0, 1, 5), '█');
}

#[test]
fn custom_fill_character_is_used() {
    let p = render_json(
        r#"{
            "width": 10, "height": 7, "frame_count": 1,
            "objects": [
                { "type": "header", "text": "I", "ch": "*",
                  "position": { "x": { "fixed": 1 }, "y": { "fixed": 1 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 2, 2), '*');
}

#[test]
fn glyphs_are_spaced_one_column_apart() {
    // "II": first glyph at columns 0..3, a gap column at 3, second at 4..7.
    let p = render_json(
        r#"{
            "width": 10, "height": 6, "frame_count": 1,
            "objects": [
                { "type": "header", "text": "II",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), '█', "first glyph top-left");
    assert_eq!(char_at(&p, 0, 3, 0), ' ', "inter-glyph gap column");
    assert_eq!(char_at(&p, 0, 4, 0), '█', "second glyph top-left");
}

#[test]
fn text_word_wraps_when_too_wide_for_the_canvas() {
    // "A B": rendered on one line it spans 15 columns (A=5, gap, space=3,
    // gap, B=5). The canvas is only 11 wide, so "B" must wrap onto the next
    // glyph line (5 glyph rows + 1 gap row below the first). "A" is 5 wide
    // and fits, so it stays on the first line. The break is on the word
    // boundary — no glyph is split.
    let p = render_json(
        r#"{
            "width": 11, "height": 14, "frame_count": 1,
            "objects": [
                { "type": "header", "text": "A B",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    // Line 0: "A" — its top row is " ### " so columns 1..4 are filled.
    assert_eq!(char_at(&p, 0, 1, 0), '█', "A top bar on line 0");
    // Where "B" would have started unwrapped (x = 5 + 1 gap = 6) is empty.
    assert_eq!(char_at(&p, 0, 6, 0), ' ', "B did not stay on line 0");
    // Line 1: "B" starts at x=0, y = 5 + 1 = 6; its top row "#### " fills 0..4.
    assert_eq!(char_at(&p, 0, 0, 6), '█', "B top-left on wrapped line");
    assert_eq!(char_at(&p, 0, 1, 6), '█', "B top bar on wrapped line");
}
