//! `Group` object: a logical container only. It emits no draw operations of its
//! own; its members are ordinary objects that render independently.

mod common;
use common::{char_at, frame_lines, render_json};

#[test]
fn group_members_render_independently_and_the_group_adds_nothing() {
    // Two labels and a group wrapping them. The output is exactly the two
    // labels — the group itself contributes no cells.
    let p = render_json(
        r#"{
            "width": 5, "height": 1, "frame_count": 1,
            "objects": [
                { "type": "label", "text": "A",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } },
                { "type": "label", "text": "B",
                  "position": { "x": { "fixed": 2 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } },
                { "type": "group", "members": [0, 1],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), 'A');
    assert_eq!(char_at(&p, 0, 2, 0), 'B');
    // Only the two member glyphs exist; everything else is blank.
    let non_blank: usize = frame_lines(&p, 0)[0].chars().filter(|c| *c != ' ').count();
    assert_eq!(non_blank, 2, "group emits no extra cells");
}

#[test]
fn group_frame_range_does_not_gate_its_members() {
    // The group is only on frame 0, but its member spans frames 0..2 and must
    // still render on frame 1 (members resolve by their own frame range).
    let p = render_json(
        r#"{
            "width": 3, "height": 1, "frame_count": 2,
            "objects": [
                { "type": "label", "text": "M",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 2 } },
                { "type": "group", "members": [0],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 1, 0, 0), 'M');
}
