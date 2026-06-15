//! `AutoAdvance` object: the compiled `AutoAdvanceRegion` sidecar (frame range +
//! delay) and the fact that it draws nothing into the static frames. (The
//! play-time auto-advance run-loop is TUI; the pure `frame_auto_advance_delay` /
//! `effective_auto_delay` step fns are unit-tested inline in `src/player/mod.rs`.)

mod common;

use bs::engine::source::SourcePresentation;

fn source(json: &str) -> SourcePresentation {
    serde_json::from_str(json).expect("source JSON should parse")
}

#[test]
fn auto_advance_regions_collects_specs_with_default_delay() {
    // delay_ms omitted → serde default (5000 = 5s).
    let s = source(
        r#"{"width":40,"height":10,"frame_count":4,
            "objects":[{"type":"auto_advance","frames":{"start":1,"end":2}}]}"#,
    );
    let regions = s.auto_advance_regions();
    assert_eq!(regions.len(), 1);
    let r = &regions[0];
    assert_eq!((r.start_frame, r.end_frame), (1, 2));
    assert_eq!(r.delay_ms, 5000);
}

#[test]
fn auto_advance_regions_carries_an_explicit_delay_and_range() {
    let s = source(
        r#"{"width":40,"height":10,"frame_count":8,
            "objects":[{"type":"auto_advance","frames":{"start":0,"end":4},"delay_ms":2500}]}"#,
    );
    let r = &s.auto_advance_regions()[0];
    assert_eq!((r.start_frame, r.end_frame), (0, 4));
    assert_eq!(r.delay_ms, 2500);
}

#[test]
fn auto_advance_draws_nothing_into_the_frames() {
    // A lone auto-advance marker plus a label: the rendered grid shows only the
    // label, never anything from the marker.
    let json = r#"{"width":10,"height":3,"frame_count":2,
        "objects":[
            {"type":"label","text":"hi","position":{"x":{"fixed":0},"y":{"fixed":0}},
             "frames":{"start":0,"end":2}},
            {"type":"auto_advance","frames":{"start":0,"end":1},"delay_ms":1000}
        ]}"#;
    let pres = common::render_json(json);
    let grid = common::frame_lines(&pres, 0);
    assert_eq!(grid[0].trim_end(), "hi");
    // Nothing else painted anywhere on the canvas.
    assert!(grid[1..].iter().all(|line| line.trim().is_empty()));
}
