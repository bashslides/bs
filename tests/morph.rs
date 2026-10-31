//! Tests for the `morph` object — a content morph between two ASCII-art grids
//! over a frame range. Verified end-to-end through Engine::compile +
//! Renderer::render (the same pipeline the editor previews with), so the per-
//! frame blend is pinned exactly as it will play.

mod common;

use common::{char_at, render_json};

/// A morph endpoints test: the first frame shows `from`, the last shows `to`.
/// `mode` is omitted, so it defaults to `dissolve` — but endpoints are
/// mode-independent (t=0 is all `from`, t=1 is all `to`).
#[test]
fn morph_shows_from_on_first_frame_and_to_on_last() {
    let p = render_json(
        r#"{
            "width": 4, "height": 2, "frame_count": 3,
            "objects": [
                { "type": "morph",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "from": "AB",
                  "to": "XY",
                  "frames": { "start": 0, "end": 3 } }
            ]
        }"#,
    );

    // Frame 0 → progress 0 → fully `from`.
    assert_eq!(char_at(&p, 0, 0, 0), 'A');
    assert_eq!(char_at(&p, 0, 1, 0), 'B');
    // Frame 2 (last) → progress 1 → fully `to`.
    assert_eq!(char_at(&p, 2, 0, 0), 'X');
    assert_eq!(char_at(&p, 2, 1, 0), 'Y');
}

/// `wipe-right` flips cells left→right, so a mid-range frame is half `to`
/// (left) and half `from` (right) — deterministic, unlike the dissolve order.
#[test]
fn morph_wipe_right_is_half_done_at_the_midpoint() {
    let p = render_json(
        r#"{
            "width": 4, "height": 1, "frame_count": 3,
            "objects": [
                { "type": "morph",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "from": "AAAA",
                  "to": "BBBB",
                  "mode": "wipe-right",
                  "frames": { "start": 0, "end": 3 } }
            ]
        }"#,
    );

    // Frame 1 → t = 0.5. Thresholds col/4 = 0, .25, .5, .75; t>thr for cols 0,1.
    assert_eq!(char_at(&p, 1, 0, 0), 'B');
    assert_eq!(char_at(&p, 1, 1, 0), 'B');
    assert_eq!(char_at(&p, 1, 2, 0), 'A');
    assert_eq!(char_at(&p, 1, 3, 0), 'A');
}

/// A smaller shape grows into a larger one: cells beyond the `to` grid are
/// spaces (transparent), so the underlying canvas shows through there.
#[test]
fn morph_pads_the_smaller_grid_with_transparent_space() {
    let p = render_json(
        r#"{
            "width": 3, "height": 2, "frame_count": 2,
            "objects": [
                { "type": "morph",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "from": "@@\n@@",
                  "to": "@",
                  "frames": { "start": 0, "end": 2 } }
            ]
        }"#,
    );

    // Last frame → fully `to` ("@"): only (0,0) is inked; the rest is space.
    assert_eq!(char_at(&p, 1, 0, 0), '@');
    assert_eq!(char_at(&p, 1, 1, 0), ' ');
    assert_eq!(char_at(&p, 1, 0, 1), ' ');
}

/// The morph is invisible outside its frame range.
#[test]
fn morph_is_hidden_outside_its_range() {
    let p = render_json(
        r#"{
            "width": 2, "height": 1, "frame_count": 3,
            "objects": [
                { "type": "morph",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "from": "A",
                  "to": "B",
                  "frames": { "start": 1, "end": 2 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), ' '); // before the range
    assert_eq!(char_at(&p, 1, 0, 0), 'A'); // only frame in range → progress 0
    assert_eq!(char_at(&p, 2, 0, 0), ' '); // after the range
}

/// The horizontal wipes move one straight front across ragged art: a column
/// flips on the same frame in every row, short rows included (thresholds use
/// the grid's width, not each row's own).
#[test]
fn morph_horizontal_wipes_flip_a_column_together_in_ragged_rows() {
    for mode in ["wipe-right", "wipe-left"] {
        let p = render_json(&format!(
            r#"{{
                "width": 10, "height": 2, "frame_count": 11,
                "objects": [
                    {{ "type": "morph",
                       "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }},
                       "from": "AAAAAAAAAA\nAA",
                       "to": "BBBBBBBBBB\nBB",
                       "mode": "{mode}",
                       "frames": {{ "start": 0, "end": 11 }} }}
                ]
            }}"#
        ));
        for frame in 0..11 {
            for col in 0..2 {
                assert_eq!(
                    char_at(&p, frame, col, 1) == 'B',
                    char_at(&p, frame, col, 0) == 'B',
                    "{mode}: column {col} on frame {frame}"
                );
            }
        }
    }
}

/// A 10-wide wipe-right morph over `own`, alone or in an explicit group `group`.
fn morph_deck(own: (usize, usize), group: Option<(usize, usize)>) -> String {
    let group = group
        .map(|(s, e)| format!(r#", {{ "type": "group", "members": [0], "frames": {{ "start": {s}, "end": {e} }} }}"#))
        .unwrap_or_default();
    format!(
        r#"{{ "width": 10, "height": 1, "frame_count": 10, "objects": [
            {{ "type": "morph", "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }},
               "from": "AAAAAAAAAA", "to": "BBBBBBBBBB", "mode": "wipe-right",
               "frames": {{ "start": {}, "end": {} }} }}{group} ] }}"#,
        own.0, own.1
    )
}

#[test]
fn an_explicit_group_range_gates_a_morph_without_retiming_it() {
    let row = |p: &bs::types::PlayablePresentation, f: usize| -> String {
        (0..10).map(|x| char_at(p, f, x, 0)).collect()
    };
    // Narrowed to frames 2..5: those frames show the morph's own progress over
    // 0..10 (not a full blend squeezed into 3 frames); the rest show nothing.
    let alone = render_json(&morph_deck((0, 10), None));
    let grouped = render_json(&morph_deck((0, 10), Some((2, 5))));
    for f in 0..10 {
        let want = if (2..5).contains(&f) { row(&alone, f) } else { " ".repeat(10) };
        assert_eq!(row(&grouped, f), want, "narrowed, frame {f}");
    }
    // Widened from 5..8 to 0..10: `from` before the morph's range, its own blend
    // inside it, `to` after it.
    let alone = render_json(&morph_deck((5, 8), None));
    let grouped = render_json(&morph_deck((5, 8), Some((0, 10))));
    for f in 0..10 {
        let want = match f {
            0..=4 => "A".repeat(10),
            5..=7 => row(&alone, f),
            _ => "B".repeat(10),
        };
        assert_eq!(row(&grouped, f), want, "widened, frame {f}");
    }
}
