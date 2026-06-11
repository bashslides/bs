//! `HLine` object: a horizontal run of one character from `x_start` to `x_end`
//! (end exclusive), with a configurable draw character.

mod common;
use common::{char_at, render_json};

#[test]
fn default_hline_spans_x_start_to_x_end_exclusive() {
    let p = render_json(
        r#"{
            "width": 7, "height": 2, "frame_count": 1,
            "objects": [
                { "type": "h_line", "y": 1, "x_start": 2, "x_end": 5,
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 1, 1), ' ', "before the start");
    assert_eq!(char_at(&p, 0, 2, 1), '─');
    assert_eq!(char_at(&p, 0, 4, 1), '─');
    assert_eq!(char_at(&p, 0, 5, 1), ' ', "x_end is exclusive");
}

#[test]
fn custom_draw_character_is_used() {
    let p = render_json(
        r#"{
            "width": 5, "height": 1, "frame_count": 1,
            "objects": [
                { "type": "h_line", "y": 0, "x_start": 0, "x_end": 3, "ch": "=",
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), '=');
    assert_eq!(char_at(&p, 0, 2, 0), '=');
    assert_eq!(char_at(&p, 0, 3, 0), ' ');
}
