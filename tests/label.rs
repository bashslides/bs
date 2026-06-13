//! `Label` object features beyond plain placement: the optional `framed`
//! border (and its separate `frame_style`), background fill across the bounding
//! box, height clipping/padding, and multi-line word wrapping.

mod common;
use bs::types::{Color, Frame, NamedColor};
use common::{char_at, render_json};

/// The style of cell (x, y) on the first (full) frame.
fn cell_fg(p: &bs::types::PlayablePresentation, x: usize, y: usize) -> Option<Color> {
    match &p.frames[0] {
        Frame::Full { cells } => cells[y][x].style.fg.clone(),
        _ => panic!("frame 0 must be Full"),
    }
}
fn cell_bg(p: &bs::types::PlayablePresentation, x: usize, y: usize) -> Option<Color> {
    match &p.frames[0] {
        Frame::Full { cells } => cells[y][x].style.bg.clone(),
        _ => panic!("frame 0 must be Full"),
    }
}

#[test]
fn framed_label_draws_a_border_one_cell_outside_the_text() {
    // Unsized label "Hi" at (2,2): the frame is drawn at (1,1)..(4,3).
    let p = render_json(
        r#"{
            "width": 6, "height": 5, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "Hi", "framed": true,
                  "position": { "x": { "fixed": 2 }, "y": { "fixed": 2 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 1, 1), '┌');
    assert_eq!(char_at(&p, 0, 4, 1), '┐');
    assert_eq!(char_at(&p, 0, 1, 3), '└');
    assert_eq!(char_at(&p, 0, 4, 3), '┘');
    assert_eq!(char_at(&p, 0, 2, 2), 'H');
    assert_eq!(char_at(&p, 0, 3, 2), 'i');
}

#[test]
fn framed_label_at_the_origin_keeps_its_text_visible_inside_the_border() {
    // A framed label at (0,0) (the default new-label position): there's no room
    // to draw the border one cell outside, so the text shifts in by one and stays
    // inside the box instead of vanishing under the top/left border.
    let p = render_json(
        r#"{
            "width": 6, "height": 4, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "Hi", "framed": true,
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    // Border anchored at the origin.
    assert_eq!(char_at(&p, 0, 0, 0), '┌');
    assert_eq!(char_at(&p, 0, 3, 0), '┐');
    assert_eq!(char_at(&p, 0, 0, 2), '└');
    assert_eq!(char_at(&p, 0, 3, 2), '┘');
    // Text sits inside the box (shifted in by one), not hidden under the border.
    assert_eq!(char_at(&p, 0, 1, 1), 'H');
    assert_eq!(char_at(&p, 0, 2, 1), 'i');
}

#[test]
fn frame_style_colours_the_border_independently_of_the_text() {
    let p = render_json(
        r#"{
            "width": 6, "height": 5, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "Hi", "framed": true,
                  "frame_style": { "fg": "red" },
                  "position": { "x": { "fixed": 2 }, "y": { "fixed": 2 } },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(cell_fg(&p, 1, 1), Some(Color::Named(NamedColor::Red)), "border uses frame_style");
    assert_eq!(cell_fg(&p, 2, 2), None, "text keeps the default style");
}

#[test]
fn background_fills_the_box_and_pads_to_height() {
    // width 4, height 2, bg blue: every cell of the 4×2 box is painted, even
    // the padding cells past the text and the empty second row.
    let p = render_json(
        r#"{
            "width": 4, "height": 2, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "Hi",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 4, "height": 2,
                  "style": { "bg": "blue" },
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), 'H');
    assert_eq!(char_at(&p, 0, 2, 0), ' ', "padding cell after the text");
    let blue = Some(Color::Named(NamedColor::Blue));
    assert_eq!(cell_bg(&p, 2, 0), blue, "padding cell is background-filled");
    assert_eq!(cell_bg(&p, 0, 1), blue, "padded second row is background-filled");
}

#[test]
fn height_clips_extra_lines() {
    // Three lines, height 2 ⇒ the third line is dropped.
    let p = render_json(
        r#"{
            "width": 3, "height": 3, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "a\nb\nc",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "height": 2,
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), 'a');
    assert_eq!(char_at(&p, 0, 0, 1), 'b');
    assert_eq!(char_at(&p, 0, 0, 2), ' ', "third line clipped by height");
}

#[test]
fn width_wraps_text_across_multiple_rows() {
    // "one two three" wraps at width 5 into "one", "two", "three".
    let p = render_json(
        r#"{
            "width": 5, "height": 3, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "one two three",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 5,
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 2, 0), 'e', "end of 'one'");
    assert_eq!(char_at(&p, 0, 2, 1), 'o', "end of 'two'");
    assert_eq!(char_at(&p, 0, 4, 2), 'e', "end of 'three'");
}
