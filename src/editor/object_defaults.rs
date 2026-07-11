use crate::engine::source::*;
use crate::types::Style;

/// An object available in the editor's Add Object menu.
///
/// Name and shortcut intentionally share one descriptor: parallel arrays made
/// it possible to silently show one object while adding another after a future
/// insertion or reorder. The runtime-only types created through dedicated
/// sub-menu flows (`Animation` and `AutoAdvance`) are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddableObjectType {
    pub name: &'static str,
    pub shortcut: char,
}

/// The Add-Object menu shows each shortcut (`[l] Label`); pressing it adds that
/// type directly. Shortcuts avoid the global fullscreen key (`f`). They are the
/// type's initial where free, else another distinctive letter (Header→`e`,
/// Arrow→`w`, Art→`a`, List→`i`, Loop→`p`, Morph→`m`, Circle→`o`).
pub const OBJECT_TYPES: &[AddableObjectType] = &[
    AddableObjectType { name: "Label", shortcut: 'l' },
    AddableObjectType { name: "HLine", shortcut: 'h' },
    AddableObjectType { name: "Rect", shortcut: 'r' },
    AddableObjectType { name: "Header", shortcut: 'e' },
    AddableObjectType { name: "Group", shortcut: 'g' },
    AddableObjectType { name: "Arrow", shortcut: 'w' },
    AddableObjectType { name: "Table", shortcut: 't' },
    AddableObjectType { name: "Art", shortcut: 'a' },
    AddableObjectType { name: "Command", shortcut: 'c' },
    AddableObjectType { name: "List", shortcut: 'i' },
    AddableObjectType { name: "Loop", shortcut: 'p' },
    AddableObjectType { name: "Morph", shortcut: 'm' },
    AddableObjectType { name: "Circle", shortcut: 'o' },
];

/// Map a pressed character (case-insensitive) to an object-type index, if it is
/// a quick-add shortcut.
pub fn object_type_for_key(c: char) -> Option<usize> {
    let c = c.to_ascii_lowercase();
    OBJECT_TYPES.iter().position(|kind| kind.shortcut == c)
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
            align: TextAlign::default(),
            valign: VerticalAlign::default(),
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
            head_start: false,
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
        12 => SceneObject::Circle(Circle {
            position: Position {
                x: Coordinate::Fixed(0.0),
                y: Coordinate::Fixed(0.0),
            },
            diameter: 10,
            ch: '@',
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

    /// One value per `SceneObject` variant, for registry-completeness checks.
    ///
    /// The `match` in the compile-time guard below is exhaustive, so adding a new
    /// `SceneObject` variant makes this function fail to compile until the new
    /// variant is constructed here too. That is the whole point: it forces the
    /// author of a new object type through [`object_type_registry_is_complete`],
    /// which then checks the one part of the "add an object type" checklist the
    /// type checker cannot see on its own — the lookup *tables* (`OBJECT_TYPES`
    /// and the documented sub-menu-only set), not the `match` arms the compiler
    /// already guards. See the checklist in `src/engine/objects/mod.rs`.
    fn every_variant() -> Vec<SceneObject> {
        // Compile-time exhaustiveness guard: a newly-added variant breaks this
        // `match`, pointing whoever added it straight at this file.
        fn _exhaustiveness_guard(o: &SceneObject) {
            match o {
                SceneObject::Label(_)
                | SceneObject::HLine(_)
                | SceneObject::Rect(_)
                | SceneObject::Header(_)
                | SceneObject::Group(_)
                | SceneObject::Arrow(_)
                | SceneObject::Table(_)
                | SceneObject::Art(_)
                | SceneObject::Command(_)
                | SceneObject::List(_)
                | SceneObject::Loop(_)
                | SceneObject::Morph(_)
                | SceneObject::Circle(_)
                | SceneObject::Animation(_)
                | SceneObject::AutoAdvance(_) => {}
            }
        }

        use crate::engine::objects::table::TableCell;
        let fr = FrameRange { start: 0, end: 2 };
        let pos = || Position { x: Coordinate::Fixed(0.0), y: Coordinate::Fixed(0.0) };

        vec![
            SceneObject::Label(Label {
                text: "t".into(), position: pos(), width: Coordinate::Fixed(0.0),
                height: Coordinate::Fixed(0.0), framed: false, frame_style: None,
                align: TextAlign::default(), valign: VerticalAlign::default(),
                style: Style::default(), frames: fr.clone(), z_order: 0,
            }),
            SceneObject::HLine(HLine {
                y: Coordinate::Fixed(0.0), x_start: Coordinate::Fixed(0.0),
                x_end: Coordinate::Fixed(5.0), ch: '─', style: Style::default(),
                frames: fr.clone(), z_order: 0,
            }),
            SceneObject::Rect(Rect {
                position: pos(), width: Coordinate::Fixed(4.0), height: Coordinate::Fixed(3.0),
                style: Style::default(), frames: fr.clone(), z_order: 0, title: None,
            }),
            SceneObject::Header(Header {
                text: "H".into(), position: pos(), style: Style::default(),
                frames: fr.clone(), z_order: 0, ch: '█',
            }),
            SceneObject::Group(Group { members: vec![], frames: None, z_order: 0 }),
            SceneObject::Arrow(Arrow {
                x1: Coordinate::Fixed(0.0), y1: Coordinate::Fixed(0.0),
                x2: Coordinate::Fixed(5.0), y2: Coordinate::Fixed(0.0),
                head: true, head_start: false, head_ch: None, body_ch: None,
                style: Style::default(), frames: fr.clone(), z_order: 0,
            }),
            SceneObject::Table(Table {
                position: pos(), width: Coordinate::Fixed(9.0), height: Coordinate::Fixed(0.0),
                col_widths: vec![0.5, 0.5], rows: 2,
                cells: vec![vec![TableCell::default(); 2], vec![TableCell::default(); 2]],
                header_bold: true, borders: true, style: Style::default(),
                frames: fr.clone(), z_order: 0,
            }),
            SceneObject::Art(Art {
                position: pos(), art: "AB".into(), name: "a".into(),
                style: Style::default(), frames: fr.clone(), z_order: 0,
            }),
            SceneObject::Command(Command {
                position: pos(), width: Coordinate::Fixed(10.0), height: Coordinate::Fixed(4.0),
                command: "echo".into(), args: vec![], cwd: None, timeout_secs: None,
                border: true, style: Style::default(), frames: fr.clone(), z_order: 0,
            }),
            SceneObject::List(List {
                text: "a".into(), position: pos(), width: Coordinate::Fixed(0.0),
                height: Coordinate::Fixed(0.0), ordered: false, bullet: "-".into(),
                spacing: 1, style: Style::default(), frames: fr.clone(), z_order: 0,
            }),
            SceneObject::Loop(Loop {
                frames: fr.clone(), delay_ms: 500, count: 0, bounce: true,
            }),
            SceneObject::Morph(Morph {
                position: pos(), from: "A".into(), to: "B".into(), name: "m".into(),
                mode: MorphMode::default(), style: Style::default(),
                frames: fr.clone(), z_order: 0,
            }),
            SceneObject::Circle(Circle {
                position: pos(), diameter: 6, ch: '@', style: Style::default(),
                frames: fr.clone(), z_order: 0,
            }),
            // The two sub-menu-only types (created via editor sub-menus, not the
            // Add-Object menu), so deliberately absent from `OBJECT_TYPES`.
            SceneObject::Animation(Animation {
                id: 1, frames: fr.clone(), auto_play: true, delay_ms: 500, gap_frames: 0,
            }),
            SceneObject::AutoAdvance(AutoAdvance { frames: fr.clone(), delay_ms: 5000 }),
        ]
    }

    /// The `SceneObject` enum, the Add-Object menu (`OBJECT_TYPES`), and the
    /// documented sub-menu-only types must name exactly the same set — no
    /// unregistered variant (which would be invisible in the UI) and no stale
    /// table row. This enforces the lookup-table steps of the "add an object
    /// type" checklist that the compiler's exhaustive `match`es can't.
    #[test]
    fn object_type_registry_is_complete() {
        // Types created through editor sub-menus, not the Add-Object menu. Their
        // absence from `OBJECT_TYPES` is intentional (see CLAUDE.md's runtime
        // exceptions); listing them here documents *why* they're excluded.
        const SUBMENU_ONLY: &[&str] = &["Animation", "AutoAdvance"];

        let variant_names: Vec<&'static str> =
            every_variant().iter().map(|o| o.type_name()).collect();

        // Names are distinct.
        let mut sorted = variant_names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), variant_names.len(), "duplicate type names");

        // Every variant is registered in exactly one of the two tables.
        for n in &variant_names {
            let in_menu = OBJECT_TYPES.iter().any(|t| t.name == *n);
            let in_submenu = SUBMENU_ONLY.contains(n);
            assert!(
                in_menu ^ in_submenu,
                "{n}: must appear in exactly one of OBJECT_TYPES / SUBMENU_ONLY \
                 (in_menu={in_menu}, in_submenu={in_submenu}) — the Add-Object \
                 menu is out of sync with the SceneObject enum",
            );
        }

        // No stale entries: every listed name maps to a real variant.
        for n in OBJECT_TYPES.iter().map(|t| t.name).chain(SUBMENU_ONLY.iter().copied()) {
            assert!(variant_names.contains(&n), "{n} is listed but names no SceneObject variant");
        }

        // Count: the enum is exactly the menu types plus the sub-menu-only types.
        assert_eq!(variant_names.len(), OBJECT_TYPES.len() + SUBMENU_ONLY.len());
    }

    #[test]
    fn create_default_covers_every_object_type() {
        for (i, kind) in OBJECT_TYPES.iter().enumerate() {
            let obj = create_default(i, 0);
            assert_eq!(obj.type_name(), kind.name, "index {i} ({})", kind.name);
        }
    }

    #[test]
    fn every_type_has_a_unique_shortcut_key() {
        for (i, kind) in OBJECT_TYPES.iter().enumerate() {
            let k = kind.shortcut;
            // The fullscreen toggle ('f') is a global key and must stay free.
            assert_ne!(k, 'f', "shortcut for {} collides with fullscreen", kind.name);
            assert_eq!(object_type_for_key(k), Some(i));
            // Uppercase resolves to the same type (case-insensitive lookup).
            assert_eq!(object_type_for_key(k.to_ascii_uppercase()), Some(i));
        }
        // Keys are unique.
        let mut seen: Vec<_> = OBJECT_TYPES.iter().map(|kind| kind.shortcut).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), OBJECT_TYPES.len(), "shortcut keys must be unique");
    }
}
