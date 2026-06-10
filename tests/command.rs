//! Tests for the `command` object — runs a binary and renders its output.
//!
//! The interactive run-loop (spawning, timeout, key handling) is part of the
//! TUI and stays manual; these tests pin the deterministic core: the compiled
//! `CommandRegion` spec, the placeholder box drawn into the static frame, and
//! the pure output-layout used to paint stdout into the region.

mod common;

use common::{char_at, render_json};

use bs::engine::source::SourcePresentation;
use bs::player::layout_output;
use std::process::Command as ProcCommand;

/// The command under test, per the request: `echo "<a fairly long line>"`.
const LONG: &str = "this is a reasonably but already fairly long text!!!";

fn source_json() -> String {
    format!(
        r#"{{
            "width": 80, "height": 24, "frame_count": 1,
            "objects": [
                {{ "type": "command",
                   "position": {{ "x": {{ "fixed": 5 }}, "y": {{ "fixed": 3 }} }},
                   "width": 40, "height": 10,
                   "command": "echo",
                   "args": ["{LONG}"],
                   "frames": {{ "start": 0, "end": 1 }} }}
            ]
        }}"#
    )
}

#[test]
fn command_compiles_to_region_spec() {
    let source: SourcePresentation = serde_json::from_str(&source_json()).unwrap();
    let regions = source.command_regions();
    assert_eq!(regions.len(), 1);

    let r = &regions[0];
    assert_eq!(r.command, "echo");
    assert_eq!(r.args, vec![LONG.to_string()]);
    // Interior is inset one cell of border on every side of the 40x10 box.
    assert_eq!((r.x, r.y), (6, 4));
    assert_eq!((r.w, r.h), (38, 8));
    // ✓/✗ indicator sits on the top edge near the right corner.
    assert_eq!((r.status_x, r.status_y), (5 + 40 - 2, 3));
    assert_eq!((r.start_frame, r.end_frame), (0, 1));
}

#[test]
fn command_draws_a_placeholder_box_into_the_static_frame() {
    // Compiling/rendering must never run the binary — it just draws the box, so
    // editing a deck is always safe. Check the corners and the default title.
    let p = render_json(&source_json());

    assert_eq!(char_at(&p, 0, 5, 3), '┌'); // top-left
    assert_eq!(char_at(&p, 0, 44, 3), '┐'); // top-right (x=5+40-1)
    assert_eq!(char_at(&p, 0, 5, 12), '└'); // bottom-left (y=3+10-1)
    assert_eq!(char_at(&p, 0, 44, 12), '┘'); // bottom-right
    // Default title "$ echo …" begins two cells in on the top edge.
    assert_eq!(char_at(&p, 0, 7, 3), '$');
    assert_eq!(char_at(&p, 0, 9, 3), 'e');
}

#[test]
fn command_output_renders_clipped_into_region() {
    let source: SourcePresentation = serde_json::from_str(&source_json()).unwrap();
    let r = &source.command_regions()[0];

    // Run the binary exactly as the player would and capture stdout.
    let out = ProcCommand::new(&r.command)
        .args(&r.args)
        .output()
        .expect("echo should run");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), LONG);

    // Lay it out into the box interior: tail to h lines, clip/pad each to w cols.
    let w = r.w as usize;
    let h = r.h as usize;
    let rows = layout_output(&out.stdout, w, h);

    assert_eq!(rows.len(), h);
    // Every row is exactly the interior width (clipped if long, padded if short).
    assert!(rows.iter().all(|row| row.chars().count() == w));
    // The single output line lands on the first row, clipped to the width.
    let clipped: String = LONG.chars().take(w).collect();
    assert!(rows[0].starts_with(&clipped));
    // The text is longer than the box, so the row really is truncated.
    assert!(LONG.chars().count() > w);
    assert_eq!(rows[0].chars().count(), w);
    // Remaining rows are blank padding.
    assert!(rows[1..].iter().all(|row| row.trim().is_empty()));
}
