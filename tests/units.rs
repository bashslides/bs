//! Unit tests for the pure value types the whole engine is built on:
//! coordinate evaluation (the animation core), frame ranges, and the
//! number-or-object coordinate deserializer.

use ascii_presenter::engine::source::{Coordinate, FrameRange, Label};

#[test]
fn fixed_coordinate_floors_to_a_cell() {
    assert_eq!(Coordinate::Fixed(5.0).evaluate(0), 5);
    assert_eq!(Coordinate::Fixed(5.9).evaluate(0), 5);
    // Negative fractional positions clamp to the grid origin.
    assert_eq!(Coordinate::Fixed(-3.0).evaluate(0), 0);
}

#[test]
fn animated_coordinate_interpolates_linearly() {
    let c = Coordinate::Animated {
        from: 0,
        to: 10,
        start_frame: 0,
        end_frame: 10,
    };
    assert_eq!(c.evaluate(0), 0);
    assert_eq!(c.evaluate(5), 5);
    assert_eq!(c.evaluate(10), 10);
}

#[test]
fn animated_coordinate_clamps_outside_its_window() {
    let c = Coordinate::Animated {
        from: 2,
        to: 8,
        start_frame: 3,
        end_frame: 6,
    };
    // Before the window: held at `from`.
    assert_eq!(c.evaluate(0), 2);
    assert_eq!(c.evaluate(3), 2);
    // After the window: held at `to`.
    assert_eq!(c.evaluate(6), 8);
    assert_eq!(c.evaluate(100), 8);
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
    let bare: Label = serde_json::from_str(
        r#"{ "text": "a",
             "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 5,
             "frames": { "start": 0, "end": 1 } }"#,
    )
    .expect("bare-number width should parse");
    assert_eq!(bare.width.evaluate(0), 5);

    let object: Label = serde_json::from_str(
        r#"{ "text": "a",
             "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": { "fixed": 7 },
             "frames": { "start": 0, "end": 1 } }"#,
    )
    .expect("object-form width should parse");
    assert_eq!(object.width.evaluate(0), 7);
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
    assert_eq!(l.width.evaluate(0), 0);
}
