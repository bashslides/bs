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
fn auto_group_does_not_gate_its_members() {
    // A group with no `frames` field is *auto*: members resolve on their own
    // ranges. The member spans frames 0..2, so it renders on frame 1.
    let p = render_json(
        r#"{
            "width": 3, "height": 1, "frame_count": 2,
            "objects": [
                { "type": "label", "text": "M",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 2 } },
                { "type": "group", "members": [0] }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), 'M');
    assert_eq!(char_at(&p, 1, 0, 0), 'M');
}

#[test]
fn explicit_group_range_narrows_member_frames() {
    // An explicit group range overrides its members: the member's own 0..2 range
    // is replaced by the group's 0..1, so it disappears on frame 1.
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
    assert_eq!(char_at(&p, 0, 0, 0), 'M');
    assert_eq!(char_at(&p, 1, 0, 0), ' ', "group range overrides member, gating it off");
}

#[test]
fn explicit_group_range_widens_member_frames() {
    // The override widens too: the member's own 0..1 range is replaced by the
    // group's 0..2, so it now renders on frame 1 as well.
    let p = render_json(
        r#"{
            "width": 3, "height": 1, "frame_count": 2,
            "objects": [
                { "type": "label", "text": "M",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } },
                { "type": "group", "members": [0],
                  "frames": { "start": 0, "end": 2 } }
            ]
        }"#,
    );
    assert_eq!(char_at(&p, 0, 0, 0), 'M');
    assert_eq!(char_at(&p, 1, 0, 0), 'M', "group range widens the member's visibility");
}
