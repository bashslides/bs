use crate::engine::source::*;
use crate::types::Style;

pub const OBJECT_TYPES: &[&str] = &["Label", "HLine", "Rect", "Header", "Group", "Arrow"];

pub fn create_default(type_index: usize, current_frame: usize, frame_count: usize) -> SceneObject {
    let frames = FrameRange {
        start: current_frame,
        end: frame_count,
    };

    match type_index {
        0 => SceneObject::Label(Label {
            text: "New Label".into(),
            position: Position {
                x: Coordinate::Fixed(0.0),
                y: Coordinate::Fixed(0.0),
            },
            width: Coordinate::Fixed(0.0),
            height: Coordinate::Fixed(0.0),
            framed: false,
            frame_style: None,
            style: Style::default(),
            frames,
            z_order: 0,
        }),
        1 => SceneObject::HLine(HLine {
            y: Coordinate::Fixed(0.0),
            x_start: Coordinate::Fixed(0.0),
            x_end: Coordinate::Fixed(20.0),
            ch: '─',
            style: Style::default(),
            frames,
            z_order: 0,
        }),
        2 => SceneObject::Rect(Rect {
            position: Position {
                x: Coordinate::Fixed(0.0),
                y: Coordinate::Fixed(0.0),
            },
            width: Coordinate::Fixed(10.0),
            height: Coordinate::Fixed(5.0),
            style: Style::default(),
            frames,
            z_order: 0,
            title: None,
        }),
        3 => SceneObject::Header(Header {
            text: "TITLE".into(),
            position: Position {
                x: Coordinate::Fixed(0.0),
                y: Coordinate::Fixed(0.0),
            },
            style: Style::default(),
            frames,
            z_order: 0,
            ch: '█',
        }),
        4 => SceneObject::Group(Group {
            members: vec![],
            frames,
            z_order: 0,
        }),
        5 => SceneObject::Arrow(Arrow {
            x1: Coordinate::Fixed(5.0),
            y1: Coordinate::Fixed(5.0),
            x2: Coordinate::Fixed(20.0),
            y2: Coordinate::Fixed(5.0),
            head: true,
            head_ch: None,
            body_ch: None,
            style: Style::default(),
            frames,
            z_order: 0,
        }),
        _ => unreachable!(),
    }
}
