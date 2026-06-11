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
fn zero_length_arrow_draws_a_single_point() {
    let p = render_json(&arrow_json(r#""x1": 2, "y1": 1, "x2": 2, "y2": 1"#, 4, 3));
    assert_eq!(char_at(&p, 0, 2, 1), '*');
}
