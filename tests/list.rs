//! `List` object: ordered/unordered markers, inter-item spacing (default and
//! custom), and indentation of wrapped continuation rows under the item text.

mod common;
use bs::types::{Color, Frame, NamedColor, PlayablePresentation};
use common::{char_at, frame_lines, render_json};

fn cell_bg(p: &PlayablePresentation, x: usize, y: usize) -> Option<Color> {
    match &p.frames[0] {
        Frame::Full { cells } => cells[y][x].style.bg.clone(),
        _ => panic!("frame 0 must be Full"),
    }
}

/// One frame holding a single list object.
fn list_json(extra_fields: &str, w: u16, h: u16) -> String {
    format!(
        r#"{{
            "width": {w}, "height": {h}, "frame_count": 1,
            "objects": [
                {{
                    "type": "list",
                    "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }},
                    "frames": {{ "start": 0, "end": 1 }}
                    {extra_fields}
                }}
            ]
        }}"#
    )
}

#[test]
fn unordered_list_uses_default_one_blank_line_between_items() {
    // `spacing` omitted ⇒ defaults to 1 blank row between items.
    let json = list_json(r#", "text": "Apple\nBanana""#, 40, 6);
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("- Apple"), "row 0 = {:?}", lines[0]);
    assert!(lines[1].trim().is_empty(), "row 1 should be the blank spacer: {:?}", lines[1]);
    assert!(lines[2].starts_with("- Banana"), "row 2 = {:?}", lines[2]);
}

#[test]
fn spacing_zero_packs_items_on_consecutive_rows() {
    let json = list_json(r#", "text": "First\nSecond\nThird", "spacing": 0"#, 40, 6);
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("- First"), "{:?}", lines[0]);
    assert!(lines[1].starts_with("- Second"), "{:?}", lines[1]);
    assert!(lines[2].starts_with("- Third"), "{:?}", lines[2]);
}

#[test]
fn ordered_list_numbers_each_item() {
    let json = list_json(r#", "text": "First\nSecond\nThird", "ordered": true, "spacing": 0"#, 40, 6);
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("1. First"), "{:?}", lines[0]);
    assert!(lines[1].starts_with("2. Second"), "{:?}", lines[1]);
    assert!(lines[2].starts_with("3. Third"), "{:?}", lines[2]);
}

#[test]
fn custom_bullet_is_used_for_unordered_items() {
    let json = list_json(r#", "text": "One\nTwo", "bullet": "*", "spacing": 0"#, 40, 6);
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("* One"), "{:?}", lines[0]);
    assert!(lines[1].starts_with("* Two"), "{:?}", lines[1]);
}

#[test]
fn wrapped_continuation_rows_align_under_the_item_text() {
    // Width 10 forces "1. alpha beta gamma" to wrap. Continuation rows must be
    // indented by the marker width (3) so they line up under "alpha".
    let json = list_json(
        r#", "text": "alpha beta gamma", "ordered": true, "width": { "fixed": 10 }"#,
        10,
        6,
    );
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("1. alpha"), "row 0 = {:?}", lines[0]);
    // The first text glyph sits at column 3 on the first row ("1. " is 3 wide).
    assert_eq!(char_at(&p, 0, 3, 0), 'a', "alpha starts at col 3");
    // Continuation rows are blank in columns 0..3 and resume the text at col 3.
    assert_eq!(&lines[1][0..3], "   ", "row 1 indent = {:?}", lines[1]);
    assert_eq!(char_at(&p, 0, 3, 1), 'b', "beta aligns under alpha");
    assert_eq!(char_at(&p, 0, 3, 2), 'g', "gamma aligns under alpha");
}

#[test]
fn trailing_blank_line_does_not_render_a_dangling_bullet() {
    // A trailing newline must not produce an empty "- " marker row.
    let json = list_json(r#", "text": "Only\n", "spacing": 0"#, 40, 4);
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("- Only"), "{:?}", lines[0]);
    assert!(lines[1].trim().is_empty(), "row 1 should be empty, got {:?}", lines[1]);
}

#[test]
fn ordered_multi_digit_markers_align_continuation_rows() {
    // 10 items: the 10th's marker is "10. " (4 wide), and a wrapped row 11
    // continues indented by that width — verifying multi-digit alignment.
    let json = list_json(
        r#", "text": "1\n2\n3\n4\n5\n6\n7\n8\n9\nlong wrapped item", "ordered": true, "spacing": 0, "width": { "fixed": 10 }"#,
        10,
        14,
    );
    let p = render_json(&json);

    // First nine items are single rows numbered 1..9.
    assert_eq!(char_at(&p, 0, 0, 0), '1');
    assert_eq!(char_at(&p, 0, 0, 8), '9');
    // The 10th item's marker "10. " occupies rows 9.. and its text starts at col 4.
    assert_eq!(char_at(&p, 0, 0, 9), '1');
    assert_eq!(char_at(&p, 0, 1, 9), '0');
    assert_eq!(char_at(&p, 0, 2, 9), '.');
    assert_eq!(char_at(&p, 0, 4, 9), 'l', "item text starts after '10. '");
    // Continuation rows are indented to column 4 (under the text, not the marker).
    assert_eq!(char_at(&p, 0, 3, 10), ' ');
    assert_eq!(char_at(&p, 0, 4, 10), 'w', "wrapped continuation aligns under the text");
}

#[test]
fn explicit_height_clips_extra_items() {
    let json = list_json(r#", "text": "a\nb\nc", "spacing": 0, "height": { "fixed": 2 }"#, 40, 4);
    let p = render_json(&json);
    let lines = frame_lines(&p, 0);

    assert!(lines[0].starts_with("- a"), "{:?}", lines[0]);
    assert!(lines[1].starts_with("- b"), "{:?}", lines[1]);
    assert!(lines[2].trim().is_empty(), "third item clipped by height: {:?}", lines[2]);
}

#[test]
fn background_fills_the_wrap_width() {
    // With a width and a background, the row is filled across the full width.
    let json = list_json(
        r#", "text": "a", "width": { "fixed": 4 }, "style": { "bg": "blue" }"#,
        4,
        1,
    );
    let p = render_json(&json);

    assert_eq!(char_at(&p, 0, 2, 0), 'a', "'- a' across the row");
    let blue = Some(Color::Named(NamedColor::Blue));
    assert_eq!(cell_bg(&p, 3, 0), blue, "trailing cell is background-filled");
}
