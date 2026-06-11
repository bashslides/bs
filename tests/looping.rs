//! `Loop` object: the compiled `LoopRegion` sidecar and the disjoint-range
//! validation enforced in the editor and at compile time. (The play-time
//! auto-advance / bounce / break-out run-loop is TUI and stays manual; the pure
//! `loop_next` step function is unit-tested inline in `src/player/mod.rs`.)

use bs::engine::source::SourcePresentation;

fn source(json: &str) -> SourcePresentation {
    serde_json::from_str(json).expect("source JSON should parse")
}

/// A deck of `frame_count` frames carrying the given `loop` objects (each
/// `[start, end)`), plus a single plain label so the deck is never empty.
fn deck_with_loops(frame_count: usize, loops: &[(usize, usize)]) -> SourcePresentation {
    let mut objs = vec![format!(
        r#"{{"type":"label","text":"x","position":{{"x":{{"fixed":0}},"y":{{"fixed":0}}}},"frames":{{"start":0,"end":{frame_count}}}}}"#
    )];
    for (s, e) in loops {
        objs.push(format!(r#"{{"type":"loop","frames":{{"start":{s},"end":{e}}}}}"#));
    }
    source(&format!(
        r#"{{"width":40,"height":10,"frame_count":{frame_count},"objects":[{}]}}"#,
        objs.join(",")
    ))
}

#[test]
fn loop_regions_collects_specs_with_defaults() {
    // delay_ms/count/bounce omitted → serde defaults (500 / 0 / true).
    let s = source(
        r#"{"width":40,"height":10,"frame_count":8,
            "objects":[{"type":"loop","frames":{"start":2,"end":5}}]}"#,
    );
    let regions = s.loop_regions();
    assert_eq!(regions.len(), 1);
    let r = &regions[0];
    assert_eq!((r.start_frame, r.end_frame), (2, 5));
    assert_eq!(r.delay_ms, 500);
    assert_eq!(r.count, 0);
    assert!(r.bounce);
}

#[test]
fn loop_regions_carries_explicit_fields() {
    let s = source(
        r#"{"width":40,"height":10,"frame_count":8,
            "objects":[{"type":"loop","frames":{"start":0,"end":4},
                        "delay_ms":120,"count":2,"bounce":false}]}"#,
    );
    let r = &s.loop_regions()[0];
    assert_eq!(r.delay_ms, 120);
    assert_eq!(r.count, 2);
    assert!(!r.bounce);
}

#[test]
fn disjoint_loops_validate() {
    let s = deck_with_loops(20, &[(0, 5), (5, 10), (12, 20)]);
    assert!(s.validate_loops().is_ok());
}

#[test]
fn overlapping_loops_are_rejected() {
    // The classic crossing case: 10..20 and 15..25.
    let s = deck_with_loops(30, &[(10, 20), (15, 25)]);
    assert!(s.validate_loops().is_err());
}

#[test]
fn nested_loops_are_rejected() {
    // Containment counts as overlap (no nesting allowed).
    let s = deck_with_loops(30, &[(10, 25), (15, 20)]);
    assert!(s.validate_loops().is_err());
}

#[test]
fn loop_past_end_of_deck_is_rejected() {
    let s = deck_with_loops(8, &[(5, 12)]);
    assert!(s.validate_loops().is_err());
}

#[test]
fn empty_loop_range_is_rejected() {
    let s = deck_with_loops(8, &[(4, 4)]);
    assert!(s.validate_loops().is_err());
}

#[test]
fn a_deck_with_no_loops_validates_and_emits_nothing() {
    let s = deck_with_loops(5, &[]);
    assert!(s.validate_loops().is_ok());
    assert!(s.loop_regions().is_empty());
}
