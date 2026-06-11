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
