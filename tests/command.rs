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
fn animated_command_regions_follow_the_drawn_box() {
    // The box slides x = 2, 4, 6, 8 over frames 0..=3 and rests at 8 on frame 4:
    // one region per distinct position, each matching the box drawn there.
    let json = r#"{
        "width": 40, "height": 10, "frame_count": 5,
        "objects": [
            { "type": "command",
              "position": { "x": { "animated": { "from": 2, "to": 8, "anim": 1 } }, "y": { "fixed": 1 } },
              "width": 10, "height": 4, "command": "date",
              "frames": { "start": 0, "end": 5 } },
            { "type": "animation", "id": 1, "frames": { "start": 0, "end": 4 } }
        ]
    }"#;
    let source: SourcePresentation = serde_json::from_str(json).unwrap();
    let regions = source.command_regions();
    let p = render_json(json);
    for frame in 0..5 {
        let r = regions.iter().find(|r| r.start_frame <= frame && frame < r.end_frame).unwrap();
        // The interior starts one cell right of the drawn top-left corner.
        let bx = (0..40).find(|&x| char_at(&p, frame, x, 1) == '┌').unwrap();
        assert_eq!(r.x as usize, bx + 1, "frame {frame}");
        assert_eq!(r.status_x as usize, bx + 10 - 2, "frame {frame}");
    }
    // Contiguous, non-overlapping runs covering the range; the box rests on
    // frames 3..5, so those share one region.
    assert_eq!(regions.first().unwrap().start_frame, 0);
    assert_eq!(regions.last().unwrap().end_frame, 5);
    assert!(regions.windows(2).all(|w| w[0].end_frame == w[1].start_frame));
    assert_eq!(regions.len(), 4);
}

#[test]
fn command_starting_at_usize_max_yields_one_region_and_no_split() {
    // `start + 1` used to overflow (debug panic; release wrapped to 0 and split
    // regions onto frames 1 and 2, where nothing is drawn).
    let json = r#"{
        "width": 20, "height": 5, "frame_count": 3,
        "objects": [
            { "type": "animation", "id": 1, "frames": { "start": 0, "end": 3 } },
            { "type": "command", "command": "echo",
              "position": { "x": { "animated": { "from": 0, "to": 4, "anim": 1 } }, "y": { "fixed": 0 } },
              "width": 10, "height": 3,
              "frames": { "start": 18446744073709551615, "end": 3 } }
        ]
    }"#;
    let source: SourcePresentation = serde_json::from_str(json).unwrap();
    let regions = source.command_regions();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].start_frame, usize::MAX, "no region starts on a playable frame");
}

#[test]
fn command_draws_a_clean_placeholder_box_into_the_static_frame() {
    // Compiling/rendering must never run the binary — it just draws the box, so
    // editing a deck is always safe. The frame is clean: corners only, no label.
    let p = render_json(&source_json());

    assert_eq!(char_at(&p, 0, 5, 3), '┌'); // top-left
    assert_eq!(char_at(&p, 0, 44, 3), '┐'); // top-right (x=5+40-1)
    assert_eq!(char_at(&p, 0, 5, 12), '└'); // bottom-left (y=3+10-1)
    assert_eq!(char_at(&p, 0, 44, 12), '┘'); // bottom-right
    // The top edge carries no command name / args / "$" — just the border rule.
    assert_eq!(char_at(&p, 0, 7, 3), '─');
    assert_eq!(char_at(&p, 0, 9, 3), '─');
}

#[test]
fn box_height_follows_the_height_field() {
    // The drawn box must track `height`, so a vertical resize (which mutates
    // `height`) is visible. Box top is at y=3; with height H the bottom row is
    // at y = 3 + H - 1.
    let with_height = |h: u16| {
        source_json().replace(r#""width": 40, "height": 10,"#, &format!(r#""width": 40, "height": {h},"#))
    };

    let p6 = render_json(&with_height(6));
    assert_eq!(char_at(&p6, 0, 5, 8), '└'); // 3 + 6 - 1 = 8
    assert_eq!(char_at(&p6, 0, 44, 8), '┘');

    let p12 = render_json(&with_height(12));
    assert_eq!(char_at(&p12, 0, 5, 14), '└'); // 3 + 12 - 1 = 14
    assert_eq!(char_at(&p12, 0, 44, 14), '┘');
}

#[test]
fn border_can_be_disabled_for_a_frameless_region() {
    let json = source_json().replace(
        r#""command": "echo","#,
        r#""border": false, "command": "echo","#,
    );
    let p = render_json(&json);
    // No border drawn anywhere the box corners would have been.
    assert_eq!(char_at(&p, 0, 5, 3), ' ');
    assert_eq!(char_at(&p, 0, 44, 3), ' ');
    assert_eq!(char_at(&p, 0, 5, 12), ' ');

    // The output region now spans the full box (no inset), with the status cell
    // in the top-right corner.
    let source: SourcePresentation = serde_json::from_str(&json).unwrap();
    let r = &source.command_regions()[0];
    assert_eq!((r.x, r.y), (5, 3));
    assert_eq!((r.w, r.h), (40, 10));
    assert_eq!((r.status_x, r.status_y), (5 + 40 - 1, 3));
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

#[test]
fn command_region_follows_an_explicit_group_range() {
    // An explicit group range overrides the member's own 0..1, so the command
    // runs on the same frames (2..4) its placeholder box is drawn on.
    let source: SourcePresentation = serde_json::from_str(
        r#"{
            "width": 20, "height": 4, "frame_count": 4,
            "objects": [
                { "type": "command", "command": "echo",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 20, "height": 4,
                  "frames": { "start": 0, "end": 1 } },
                { "type": "group", "members": [0],
                  "frames": { "start": 2, "end": 4 } }
            ]
        }"#,
    )
    .unwrap();
    let p = bs::compile::compile(&source).unwrap();
    assert_eq!((p.commands[0].start_frame, p.commands[0].end_frame), (2, 4));
}

#[test]
fn layout_output_drops_osc_and_other_escape_strings() {
    // An OSC 8 hyperlink (BEL- and ST-terminated), a window title, a DCS string,
    // a charset designation and a colour CSI leave only their visible text.
    let out = b"\x1b]8;;file://h/a\x07a\x1b]8;;\x07 \x1b]8;;http://x\x1b\\b\x1b]8;;\x1b\\ \
\x1b]0;title\x07\x1bPdcs\x1b\\\x1b(Bc\x1b[31md\x1b[0m";
    assert_eq!(layout_output(out, 8, 1), vec!["a b cd  ".to_string()]);
}

#[test]
fn layout_output_drops_c0_controls_but_keeps_lines_and_tabs() {
    // BEL, BS, SO, SI, VT, FF, DEL, CR and a stray ESC never reach a cell.
    let out = b"a\x07b\x08c\x0ed\x0fe\x0bf\x0cg\x7fh\r\x1b\ni\tj";
    assert_eq!(layout_output(out, 9, 2), vec!["abcdefgh ".to_string(), "i j      ".to_string()]);
}

#[test]
fn layout_output_honours_c1_introducers_and_unterminated_strings() {
    // UTF-8 C1 CSI and ST-terminated OSC/DCS/SOS/PM/APC are stripped whole; an
    // OSC cut short by the next ESC ends there, so the CSI after it is stripped
    // and later text survives; an nF escape never swallows a following newline.
    let out = "\u{9b}31mY\u{9d}0;t\u{9c}Z\u{90}d\u{9c}\u{98}s\u{9c}\u{9e}p\u{9c}\u{9f}a\u{9c} \x1b]0;title\x1b[31mX\x1b(\nW".as_bytes();
    assert_eq!(layout_output(out, 4, 2), vec!["YZ X".to_string(), "W   ".to_string()]);
}

#[test]
fn layout_output_never_lets_a_cut_sequence_swallow_a_line_break() {
    // A CSI or OSC cut short at a line end (e.g. by output truncation) ends there.
    let out = b"a\x1b[3\nb\x1b]0;t\nc";
    assert_eq!(layout_output(out, 1, 3), vec!["a", "b", "c"]);
}

#[test]
fn layout_output_keeps_wide_and_zero_width_chars_to_one_column_each() {
    // A CJK char and an emoji (two columns each) become `?`; a combining accent
    // and a variation selector (zero columns) are dropped; single-width
    // non-ASCII (accented Latin, box drawing, braille) passes through. Each
    // row's chars, one per cell, are then exactly the box width.
    let out = "a\u{5b8c}b\u{1f389}c\ne\u{301}\u{2728}\u{fe0f}\u{e9}\u{2500}\u{2801}";
    assert_eq!(layout_output(out.as_bytes(), 6, 2), vec!["a?b?c ", "e?\u{e9}\u{2500}\u{2801} "]);
}
