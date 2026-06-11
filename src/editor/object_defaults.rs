use crate::engine::source::*;
use crate::types::Style;

pub const OBJECT_TYPES: &[&str] =
    &["Label", "HLine", "Rect", "Header", "Group", "Arrow", "Table", "Art", "Command", "List"];

/// Build an `Art` object embedding the given art text. Used by the editor's
/// art-library picker (the index-based `create_default` path is never hit for
/// Art, since adding one requires choosing a library piece first).
pub fn create_art(art: String, name: String, current_frame: usize) -> SceneObject {
    SceneObject::Art(Art {
        position: Position {
            x: Coordinate::Fixed(0.0),
            y: Coordinate::Fixed(0.0),
        },
        art,
        name,
        style: Style::default(),
        // New objects live on the current slide only (end is exclusive).
        frames: FrameRange { start: current_frame, end: current_frame + 1 },
        z_order: 0,
    })
}

pub fn create_default(type_index: usize, current_frame: usize) -> SceneObject {
    // New objects live on the current slide only (end is exclusive).
    let frames = FrameRange {
        start: current_frame,
        end: current_frame + 1,
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
        6 => {
            use crate::engine::objects::table::TableCell;
            let default_cells = vec![
                vec![TableCell::default(); 3],
                vec![TableCell::default(); 3],
                vec![TableCell::default(); 3],
            ];
            SceneObject::Table(Table {
                position: Position {
                    x: Coordinate::Fixed(0.0),
                    y: Coordinate::Fixed(0.0),
                },
                width: Coordinate::Fixed(36.0),
                height: Coordinate::Fixed(0.0),
                col_widths: vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
                rows: 3,
                cells: default_cells,
                header_bold: true,
                borders: true,
                style: Style::default(),
                frames,
                z_order: 0,
            })
        }
        7 => {
            // Fallback only — the editor adds Art via the library picker, which
            // calls `create_art`. Default to the first built-in piece.
            let item = crate::art_library::builtins().swap_remove(0);
            create_art(item.art, item.name, current_frame)
        }
        8 => SceneObject::Command(Command {
            position: Position {
                x: Coordinate::Fixed(0.0),
                y: Coordinate::Fixed(0.0),
            },
            width: Coordinate::Fixed(40.0),
            height: Coordinate::Fixed(10.0),
            command: "echo".into(),
            args: vec!["hello".into()],
            cwd: None,
            timeout_secs: None,
            border: true,
            style: Style::default(),
            frames,
            z_order: 0,
        }),
        9 => SceneObject::List(List {
            text: "Item one\nItem two\nItem three".into(),
            position: Position {
                x: Coordinate::Fixed(0.0),
                y: Coordinate::Fixed(0.0),
            },
            width: Coordinate::Fixed(0.0),
            height: Coordinate::Fixed(0.0),
            ordered: false,
            bullet: "-".into(),
            spacing: 1,
            style: Style::default(),
            frames,
            z_order: 0,
        }),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::state::scene_object_type_name;

    #[test]
    fn create_default_covers_every_object_type() {
        for (i, name) in OBJECT_TYPES.iter().enumerate() {
            let obj = create_default(i, 0);
            assert_eq!(scene_object_type_name(&obj), *name, "index {i} ({name})");
        }
    }
}
