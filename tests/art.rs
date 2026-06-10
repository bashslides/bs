//! Tests for the `art` object — pre-made ASCII art rendered verbatim.

mod common;

use common::{char_at, render_json};

#[test]
fn art_renders_each_line_at_its_offset() {
    let p = render_json(
        r#"{
            "width": 4, "height": 3, "frame_count": 1,
            "objects": [
                { "type": "art",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "art": "ab\ncd",
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), 'a');
    assert_eq!(char_at(&p, 0, 1, 0), 'b');
    assert_eq!(char_at(&p, 0, 0, 1), 'c');
    assert_eq!(char_at(&p, 0, 1, 1), 'd');
}

#[test]
fn art_is_placed_at_the_object_position() {
    let p = render_json(
        r#"{
            "width": 6, "height": 4, "frame_count": 1,
            "objects": [
                { "type": "art",
                  "position": { "x": { "fixed": 2 }, "y": { "fixed": 1 } },
                  "art": "Z",
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 2, 1), 'Z');
    assert_eq!(char_at(&p, 0, 0, 0), ' ');
}

#[test]
fn art_spaces_are_transparent() {
    // A label 'X' sits under the art's leading space; since spaces are not drawn
    // (no background), the X must show through.
    let p = render_json(
        r#"{
            "width": 4, "height": 2, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 }, "z_order": 0 },
                { "type": "art",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "art": " Y",
                  "frames": { "start": 0, "end": 1 }, "z_order": 1 }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), 'X', "transparent space lets the label show through");
    assert_eq!(char_at(&p, 0, 1, 0), 'Y');
}
