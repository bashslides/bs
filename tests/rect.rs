//! `Rect` object: a box-drawing border (interior left untouched) with an
//! optional title rendered on the top edge.

mod common;
use common::{char_at, render_json};

#[test]
fn border_draws_corners_edges_and_leaves_interior_blank() {
    // 5-wide × 3-tall rect at (1,1): corners at (1,1)/(5,1)/(1,3)/(5,3).
    let p = render_json(
        r#"{
            "width": 8, "height": 5, "frame_count": 1,
            "objects": [
                { "type": "rect",
                  "position": { "x": { "fixed": 1 }, "y": { "fixed": 1 } },
                  "width": 5, "height": 3,
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 1, 1), '┌');
    assert_eq!(char_at(&p, 0, 5, 1), '┐');
    assert_eq!(char_at(&p, 0, 1, 3), '└');
    assert_eq!(char_at(&p, 0, 5, 3), '┘');
    assert_eq!(char_at(&p, 0, 3, 1), '─', "top edge");
    assert_eq!(char_at(&p, 0, 3, 3), '─', "bottom edge");
    assert_eq!(char_at(&p, 0, 1, 2), '│', "left edge");
    assert_eq!(char_at(&p, 0, 5, 2), '│', "right edge");
    assert_eq!(char_at(&p, 0, 3, 2), ' ', "interior is not filled");
}

#[test]
fn title_is_drawn_on_the_top_edge() {
    // Title starts at x + 2; it paints over the top border at one z above.
    let p = render_json(
        r#"{
            "width": 10, "height": 4, "frame_count": 1,
            "objects": [
                { "type": "rect",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 8, "height": 3, "title": "Hi",
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), '┌');
    assert_eq!(char_at(&p, 0, 2, 0), 'H', "title char 0 at x+2");
    assert_eq!(char_at(&p, 0, 3, 0), 'i', "title char 1");
    assert_eq!(char_at(&p, 0, 1, 0), '─', "border still shows left of the title");
}

#[test]
fn title_on_a_rect_narrower_than_its_padding_is_skipped() {
    // Width 0 at x = 0 used to underflow the title clip (`x + w - 1`).
    for w in [0, 1] {
        let p = render_json(&format!(
            r#"{{
                "width": 8, "height": 3, "frame_count": 1,
                "objects": [
                    {{ "type": "rect",
                      "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }},
                      "width": {w}, "height": 3, "title": "Notes",
                      "frames": {{ "start": 0, "end": 1 }} }}
                ]
            }}"#
        ));
        for x in 1..8 {
            assert_eq!(char_at(&p, 0, x, 0), ' ', "width {w}: no title cell at x={x}");
        }
    }
}

#[test]
fn title_at_the_maximum_z_order_still_draws_over_the_border() {
    // `z + 1` used to overflow (panic in debug, wrap under the border in release).
    let p = render_json(
        r#"{
            "width": 10, "height": 3, "frame_count": 1,
            "objects": [
                { "type": "rect", "z_order": 2147483647,
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 8, "height": 3, "title": "Hi",
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 2, 0), 'H');
}
