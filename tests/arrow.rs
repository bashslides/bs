//! `Arrow` object: axis-aligned and diagonal (orthogonal L-shape) routing,
//! auto and custom head/body characters, head-disabled, and the single-point
//! degenerate case.

mod common;
use common::{char_at, render_json};

fn arrow_json(fields: &str, w: u16, h: u16) -> String {
    format!(
        r#"{{
            "width": {w}, "height": {h}, "frame_count": 1,
            "objects": [
                {{ "type": "arrow", {fields}, "frames": {{ "start": 0, "end": 1 }} }}
            ]
        }}"#
    )
}

#[test]
fn horizontal_arrow_uses_body_then_auto_right_head() {
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 4, "y2": 0"#, 6, 1));
    assert_eq!(char_at(&p, 0, 0, 0), '─');
    assert_eq!(char_at(&p, 0, 3, 0), '─');
    assert_eq!(char_at(&p, 0, 4, 0), '▶');
}

#[test]
fn vertical_arrow_uses_body_then_auto_down_head() {
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 0, "y2": 3"#, 1, 4));
    assert_eq!(char_at(&p, 0, 0, 0), '│');
    assert_eq!(char_at(&p, 0, 0, 2), '│');
    assert_eq!(char_at(&p, 0, 0, 3), '▼');
}

#[test]
fn leftward_arrow_with_custom_body_points_left() {
    // x2 < x1 ⇒ pointing left; auto head is ◀, custom body is '='.
    let p = render_json(&arrow_json(r#""x1": 4, "y1": 0, "x2": 0, "y2": 0, "body_ch": "=""#, 6, 1));
    assert_eq!(char_at(&p, 0, 0, 0), '◀');
    assert_eq!(char_at(&p, 0, 1, 0), '=');
    assert_eq!(char_at(&p, 0, 4, 0), '=');
}

#[test]
fn diagonal_h_first_routes_along_y1_then_bends_down() {
    // |dx| >= |dy| ⇒ horizontal-first: body along y1, corner, then vertical head.
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 3, "y2": 1"#, 5, 3));
    assert_eq!(char_at(&p, 0, 0, 0), '─');
    assert_eq!(char_at(&p, 0, 2, 0), '─');
    assert_eq!(char_at(&p, 0, 3, 0), '┐', "corner where the line bends");
    assert_eq!(char_at(&p, 0, 3, 1), '▼', "vertical head at the destination");
}

#[test]
fn head_disabled_draws_body_at_the_endpoint() {
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 3, "y2": 0, "head": false"#, 5, 1));
    assert_eq!(char_at(&p, 0, 3, 0), '─', "endpoint is body, not a head glyph");
}

#[test]
fn horizontal_double_head_points_outward_at_both_ends() {
    // head_start adds an outward head at (x1,y1): rightward arrow ⇒ ◀ at the
    // start, ▶ at the end, body between.
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 4, "y2": 0, "head_start": true"#, 6, 1));
    assert_eq!(char_at(&p, 0, 0, 0), '◀', "outward start head");
    assert_eq!(char_at(&p, 0, 2, 0), '─', "body in the middle");
    assert_eq!(char_at(&p, 0, 4, 0), '▶', "end head");
}

#[test]
fn vertical_double_head_points_outward_at_both_ends() {
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 0, "y2": 3, "head_start": true"#, 1, 4));
    assert_eq!(char_at(&p, 0, 0, 0), '▲', "outward start head (up)");
    assert_eq!(char_at(&p, 0, 0, 1), '│');
    assert_eq!(char_at(&p, 0, 0, 3), '▼', "end head (down)");
}

#[test]
fn diagonal_double_head_heads_the_horizontal_start() {
    // H-first routing: the start is the horizontal end, so its head points left.
    let p = render_json(&arrow_json(r#""x1": 0, "y1": 0, "x2": 3, "y2": 1, "head_start": true"#, 5, 3));
    assert_eq!(char_at(&p, 0, 0, 0), '◀', "outward start head along the horizontal leg");
    assert_eq!(char_at(&p, 0, 1, 0), '─');
    assert_eq!(char_at(&p, 0, 3, 0), '┐', "corner");
    assert_eq!(char_at(&p, 0, 3, 1), '▼', "end head");
}

#[test]
fn start_head_only_leaves_the_end_as_body() {
    // head:false + head_start:true ⇒ a head only at the start.
    let p = render_json(&arrow_json(
        r#""x1": 0, "y1": 0, "x2": 4, "y2": 0, "head": false, "head_start": true"#, 6, 1));
    assert_eq!(char_at(&p, 0, 0, 0), '◀', "start head present");
    assert_eq!(char_at(&p, 0, 4, 0), '─', "end is plain body");
}

#[test]
fn custom_head_char_applies_to_the_start_head_too() {
    // A custom head char from a directional family rotates for the outward start.
    let p = render_json(&arrow_json(
        r#""x1": 0, "y1": 0, "x2": 4, "y2": 0, "head_start": true, "head_ch": ">""#, 6, 1));
    assert_eq!(char_at(&p, 0, 0, 0), '<', "start head uses the left sibling of '>'");
    assert_eq!(char_at(&p, 0, 4, 0), '>', "end head");
}

#[test]
fn zero_length_arrow_draws_a_single_point() {
    let p = render_json(&arrow_json(r#""x1": 2, "y1": 1, "x2": 2, "y2": 1"#, 4, 3));
    assert_eq!(char_at(&p, 0, 2, 1), '*');
}
