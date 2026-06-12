use crate::engine::source::*;
use crate::types::Style;

pub const OBJECT_TYPES: &[&str] = &[
    "Label", "HLine", "Rect", "Header", "Group", "Arrow", "Table", "Art", "Command", "List",
    "Loop", "Morph",
];

/// One quick-add shortcut per object type, aligned by index with `OBJECT_TYPES`.
/// The Add-Object menu shows each key (`[l] Label`); pressing it adds that type
/// directly. Keys are unique and avoid the global fullscreen key (`f`). They are
/// the type's initial where free, else another distinctive letter (Header→`e`,
/// Arrow→`w`, Art→`a`, List→`i`, Loop→`p`, Morph→`m`).
pub const OBJECT_TYPE_KEYS: &[char] =
    &['l', 'h', 'r', 'e', 'g', 'w', 't', 'a', 'c', 'i', 'p', 'm'];

/// Map a pressed character (case-insensitive) to an object-type index, if it is
/// a quick-add shortcut.
pub fn object_type_for_key(c: char) -> Option<usize> {
    let c = c.to_ascii_lowercase();
    OBJECT_TYPE_KEYS.iter().position(|&k| k == c)
}

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

/// Build a `Morph` object that morphs `from_art` into `to_art`. Used by the
/// editor's two-stage art picker (pick the *from* piece, then the *to* piece).
/// The morph spans only the current slide by default — widen its frame range in
/// the properties panel to give it room to animate.
pub fn create_morph(
    from_art: String,
    from_name: String,
    to_art: String,
    to_name: String,
    current_frame: usize,
) -> SceneObject {
    SceneObject::Morph(Morph {
        position: Position {
            x: Coordinate::Fixed(0.0),
            y: Coordinate::Fixed(0.0),
        },
        from: from_art,
        to: to_art,
        name: format!("{from_name}→{to_name}"),
        mode: MorphMode::default(),
        style: Style::default(),
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
            // Auto range by default (derived from members; none here).
            frames: None,
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
        10 => SceneObject::Loop(Loop {
            // A new loop spans only the current slide; widen its range (and tune
            // delay/count/bounce) in the properties panel.
            frames,
            delay_ms: 500,
            count: 0,
            bounce: true,
        }),
        11 => {
            // Fallback only — the editor adds Morph via the two-stage art picker
            // (`create_morph`). Default to the matched ball→square builtins.
            let by_name = |want: &str| {
                crate::art_library::builtins()
                    .into_iter()
                    .find(|it| it.name == want)
                    .map(|it| (it.art, it.name))
                    .unwrap_or_else(|| (String::new(), want.to_string()))
            };
            let (from_art, from_name) = by_name("ball");
            let (to_art, to_name) = by_name("square");
            create_morph(from_art, from_name, to_art, to_name, current_frame)
        }
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

    #[test]
    fn every_type_has_a_unique_shortcut_key() {
        assert_eq!(OBJECT_TYPE_KEYS.len(), OBJECT_TYPES.len());
        for (i, &k) in OBJECT_TYPE_KEYS.iter().enumerate() {
            // The fullscreen toggle ('f') is a global key and must stay free.
            assert_ne!(k, 'f', "shortcut for {} collides with fullscreen", OBJECT_TYPES[i]);
            assert_eq!(object_type_for_key(k), Some(i));
            // Uppercase resolves to the same type (case-insensitive lookup).
            assert_eq!(object_type_for_key(k.to_ascii_uppercase()), Some(i));
        }
        // Keys are unique.
        let mut seen = OBJECT_TYPE_KEYS.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), OBJECT_TYPE_KEYS.len(), "shortcut keys must be unique");
    }
}
