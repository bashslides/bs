//! Unit tests for the pure value types the whole engine is built on:
//! coordinate evaluation (the animation core), frame ranges, and the
//! number-or-object coordinate deserializer.

use bs::engine::source::{AnimId, AnimSpans, Coordinate, FrameRange, Label};

/// A one-entry span table: animation `id` covers `[start, end_excl)`.
fn span(id: AnimId, start: usize, end_excl: usize) -> AnimSpans {
    AnimSpans::from_pairs([(id, FrameRange { start, end: end_excl })])
}

#[test]
fn fixed_coordinate_floors_to_a_cell() {
    let none = AnimSpans::default();
    assert_eq!(Coordinate::Fixed(5.0).evaluate(0, &none), 5);
    assert_eq!(Coordinate::Fixed(5.9).evaluate(0, &none), 5);
    // Negative fractional positions clamp to the grid origin.
    assert_eq!(Coordinate::Fixed(-3.0).evaluate(0, &none), 0);
}

#[test]
fn animated_coordinate_interpolates_linearly() {
    // The span lives on the referenced animation, not the coordinate.
    let c = Coordinate::Animated { from: 0, to: 10, anim: 1 };
    let a = span(1, 0, 11); // animated frames 0..=10
    assert_eq!(c.evaluate(0, &a), 0);
    assert_eq!(c.evaluate(5, &a), 5);
    assert_eq!(c.evaluate(10, &a), 10);
}

#[test]
fn animated_coordinate_supports_a_decreasing_ramp() {
    // from > to: the value ramps downward across the window.
    let c = Coordinate::Animated { from: 10, to: 0, anim: 1 };
    let a = span(1, 0, 11);
    assert_eq!(c.evaluate(0, &a), 10);
    assert_eq!(c.evaluate(2, &a), 8);
    assert_eq!(c.evaluate(5, &a), 5);
    assert_eq!(c.evaluate(10, &a), 0);
}

#[test]
fn animated_coordinate_clamps_outside_its_window() {
    let c = Coordinate::Animated { from: 2, to: 8, anim: 1 };
    let a = span(1, 3, 7); // animated frames 3..=6
    // Before the window: held at `from`.
    assert_eq!(c.evaluate(0, &a), 2);
    assert_eq!(c.evaluate(3, &a), 2);
    // After the window: held at `to`.
    assert_eq!(c.evaluate(6, &a), 8);
    assert_eq!(c.evaluate(100, &a), 8);
}

#[test]
fn animated_coordinate_with_missing_animation_holds_at_from() {
    // A dangling reference (no such animation) renders as if static at `from`.
    let c = Coordinate::Animated { from: 4, to: 9, anim: 99 };
    let a = AnimSpans::default();
    assert_eq!(c.evaluate(0, &a), 4);
    assert_eq!(c.evaluate(50, &a), 4);
}

#[test]
fn start_value_ignores_animation_timing() {
    assert_eq!(Coordinate::Fixed(7.0).start_value(), 7);
    assert_eq!(Coordinate::Animated { from: 4, to: 9, anim: 1 }.start_value(), 4);
}

#[test]
fn frame_range_end_is_exclusive() {
    let r = FrameRange { start: 1, end: 3 };
    assert!(!r.contains(0));
    assert!(r.contains(1));
    assert!(r.contains(2));
    assert!(!r.contains(3));
}

#[test]
fn coordinate_field_accepts_bare_number_or_object() {
    // The compat deserializer is used on Label::width/height: both a plain
    // number and a `{ "fixed": N }` object must parse to the same value.
    let none = AnimSpans::default();
    let bare: Label = serde_json::from_str(
        r#"{ "text": "a",
             "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 5,
             "frames": { "start": 0, "end": 1 } }"#,
    )
    .expect("bare-number width should parse");
    assert_eq!(bare.width.evaluate(0, &none), 5);

    let object: Label = serde_json::from_str(
        r#"{ "text": "a",
             "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": { "fixed": 7 },
             "frames": { "start": 0, "end": 1 } }"#,
    )
    .expect("object-form width should parse");
    assert_eq!(object.width.evaluate(0, &none), 7);
}

#[test]
fn omitted_optional_width_defaults_to_zero() {
    // width/height are optional; an omitted width means "no fixed box".
    let l: Label = serde_json::from_str(
        r#"{ "text": "a",
             "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "frames": { "start": 0, "end": 1 } }"#,
    )
    .expect("label without width should parse");
    assert_eq!(l.width.evaluate(0, &AnimSpans::default()), 0);
}
