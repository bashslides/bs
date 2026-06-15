//! `Circle` object: a parametric filled circle drawn with a single character.
//! Geometry is hand-derived from the fill rule (a cell is drawn when it lies
//! within the bounding-box ellipse, widened ~2× so the shape looks round on the
//! terminal cell grid). The per-row span/aspect helpers are unit-tested inline
//! in `engine/objects/circle.rs`.

mod common;

use common::{char_at, frame_lines, render_json};

/// A `width`×`height` deck with one circle at `(x, y)`, diameter `d`, char `ch`.
fn circle_deck(w: u16, h: u16, x: i32, y: i32, d: u16, ch: char) -> String {
    format!(
        r#"{{"width":{w},"height":{h},"frame_count":1,
            "objects":[{{"type":"circle",
                "position":{{"x":{{"fixed":{x}}},"y":{{"fixed":{y}}}}},
                "diameter":{d},"ch":"{ch}","frames":{{"start":0,"end":1}}}}]}}"#
    )
}

/// Count the filled (`ch`) cells on row `y`.
fn row_fill(lines: &[String], y: usize, ch: char) -> usize {
    lines[y].chars().filter(|&c| c == ch).count()
}

#[test]
fn diameter_10_circle_is_round_and_filled() {
    // 10 rows tall ⇒ 20 columns wide. Placed at (1,1) so it occupies rows 1..=10.
    let pres = render_json(&circle_deck(24, 12, 1, 1, 10, '@'));

    // The four central rows (y = 4..=7) are the full 20 columns wide: x = 1..=20.
    for y in 4..=7 {
        assert_eq!(char_at(&pres, 0, 0, y), ' ', "left of circle empty (y={y})");
        for x in 1..=20 {
            assert_eq!(char_at(&pres, 0, x, y), '@', "interior filled at ({x},{y})");
        }
        assert_eq!(char_at(&pres, 0, 21, y), ' ', "right of circle empty (y={y})");
    }

    // The top and bottom rows are narrower than the middle (a round cap, not a
    // rectangle), and symmetric about the centre.
    let lines = frame_lines(&pres, 0);
    let mid = row_fill(&lines, 5, '@');
    let top = row_fill(&lines, 1, '@');
    let bottom = row_fill(&lines, 10, '@');
    assert!(top < mid, "top row ({top}) narrower than middle ({mid})");
    assert_eq!(top, bottom, "top and bottom rows match (vertical symmetry)");

    // The corner of the bounding box is clipped away; the centre is filled.
    assert_eq!(char_at(&pres, 0, 1, 1), ' ', "bounding-box corner is empty");
    assert_eq!(char_at(&pres, 0, 10, 5), '@', "centre is filled");
}

#[test]
fn fill_char_is_customizable() {
    let pres = render_json(&circle_deck(20, 8, 0, 0, 6, '#'));
    let lines = frame_lines(&pres, 0);
    // Something was drawn, and only with the chosen character (never '@').
    assert!(lines.iter().any(|l| l.contains('#')), "circle drawn with '#'");
    assert!(lines.iter().all(|l| !l.contains('@')), "no stray default char");
}

#[test]
fn circle_is_horizontally_symmetric() {
    // Diameter 7 ⇒ 14 columns. Each filled row is a contiguous span centred on
    // the circle's middle column, so the gaps on the left and right match.
    let pres = render_json(&circle_deck(20, 9, 0, 0, 7, '@'));
    let cols = 14usize; // Circle::columns(7) = round(7*2) = 14
    for line in frame_lines(&pres, 0) {
        let left_gap = line.chars().take_while(|&c| c == ' ').count();
        let filled = line.chars().filter(|&c| c == '@').count();
        if filled == 0 {
            continue;
        }
        let right_gap = cols - left_gap - filled;
        assert_eq!(left_gap, right_gap, "row not symmetric: {line:?}");
    }
}

#[test]
fn circle_is_hidden_outside_its_frame_range() {
    // A two-frame deck with the circle only on frame 0.
    let json = r#"{"width":12,"height":8,"frame_count":2,
        "objects":[{"type":"circle","position":{"x":{"fixed":0},"y":{"fixed":0}},
                    "diameter":6,"frames":{"start":0,"end":1}}]}"#;
    let pres = render_json(json);
    assert!(frame_lines(&pres, 0).iter().any(|l| l.contains('@')), "shown on frame 0");
    assert!(frame_lines(&pres, 1).iter().all(|l| !l.contains('@')), "gone on frame 1");
}
