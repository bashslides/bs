//! `Animation` object: the compiled `AnimationRegion` sidecar (span + auto-play
//! config) and the loop/animation validation — a loop may wrap whole animations
//! but must not bisect one, while animations themselves may overlap freely. (The
//! play-time auto-advance / min-delay run-loop is TUI; the pure
//! `auto_advance_delay` step is unit-tested inline in `src/player/mod.rs`.)

use bs::engine::source::SourcePresentation;

fn source(json: &str) -> SourcePresentation {
    serde_json::from_str(json).expect("source JSON should parse")
}

/// A deck of `frame_count` frames carrying the given `animation` spans (each
/// `[start, end)`) and `loop` spans, plus a label so the deck is never empty.
fn deck(frame_count: usize, animations: &[(usize, usize)], loops: &[(usize, usize)]) -> SourcePresentation {
    let mut objs = vec![format!(
        r#"{{"type":"label","text":"x","position":{{"x":{{"fixed":0}},"y":{{"fixed":0}}}},"frames":{{"start":0,"end":{frame_count}}}}}"#
    )];
    for (s, e) in animations {
        objs.push(format!(r#"{{"type":"animation","frames":{{"start":{s},"end":{e}}}}}"#));
    }
    for (s, e) in loops {
        objs.push(format!(r#"{{"type":"loop","frames":{{"start":{s},"end":{e}}}}}"#));
    }
    source(&format!(
        r#"{{"width":40,"height":10,"frame_count":{frame_count},"objects":[{}]}}"#,
        objs.join(",")
    ))
}

#[test]
fn animation_regions_collects_specs_with_defaults() {
    // auto_play/delay_ms omitted → serde defaults (true / 500).
    let s = source(
        r#"{"width":40,"height":10,"frame_count":8,
            "objects":[{"type":"animation","frames":{"start":2,"end":6}}]}"#,
    );
    let regions = s.animation_regions();
    assert_eq!(regions.len(), 1);
    let r = &regions[0];
    assert_eq!((r.start_frame, r.end_frame), (2, 6));
    assert!(r.auto_play);
    assert_eq!(r.delay_ms, 500);
}

#[test]
fn animation_regions_carries_explicit_fields() {
    let s = source(
        r#"{"width":40,"height":10,"frame_count":8,
            "objects":[{"type":"animation","frames":{"start":0,"end":4},
                        "auto_play":false,"delay_ms":120}]}"#,
    );
    let r = &s.animation_regions()[0];
    assert!(!r.auto_play);
    assert_eq!(r.delay_ms, 120);
}

#[test]
fn animations_may_overlap_each_other() {
    // Two overlapping animation spans — allowed (unlike loops). No loops here.
    let s = deck(20, &[(2, 10), (5, 15)], &[]);
    assert!(s.validate_loops().is_ok());
}

#[test]
fn a_loop_containing_a_whole_animation_validates() {
    // Loop [0,10) fully contains animation [2,8): fine.
    let s = deck(10, &[(2, 8)], &[(0, 10)]);
    assert!(s.validate_loops().is_ok());
}

#[test]
fn a_loop_whose_bounds_match_the_animation_validates() {
    // Containment is inclusive of equal bounds.
    let s = deck(10, &[(0, 10)], &[(0, 10)]);
    assert!(s.validate_loops().is_ok());
}

#[test]
fn an_animation_fully_outside_a_loop_validates() {
    let s = deck(20, &[(12, 18)], &[(0, 10)]);
    assert!(s.validate_loops().is_ok());
}

#[test]
fn a_loop_cutting_an_animation_in_half_is_rejected() {
    // Loop [0,6) overlaps animation [4,10) but does not contain it.
    let s = deck(10, &[(4, 10)], &[(0, 6)]);
    let err = s.validate_loops().unwrap_err();
    assert!(err.contains("cut an animation"), "got: {err}");
}

#[test]
fn a_loop_starting_inside_an_animation_is_rejected() {
    // Loop [5,12) starts inside animation [2,8): a partial overlap.
    let s = deck(12, &[(2, 8)], &[(5, 12)]);
    assert!(s.validate_loops().is_err());
}
