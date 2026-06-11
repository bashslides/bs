use anyhow::{Context, Result};

use crate::art_library::ArtItem;
use crate::engine::source::{Coordinate, FrameRange, SceneObject, SourcePresentation};

use super::config::EditorConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DeleteFrame,
    DeleteObject { object_index: usize },
    /// Remove one member from a group (does not delete the underlying object).
    RemoveGroupMember {
        group_index: usize,
        member_idx: usize,
        /// Property row to restore in EditProperties after removal.
        return_selected_property: usize,
        return_panel_scroll: usize,
    },
    /// Remove a column from a table.
    RemoveTableColumn {
        object_index: usize,
        col_index: usize,
    },
}

// ---------------------------------------------------------------------------
// Table cell-properties sub-state
// ---------------------------------------------------------------------------

/// Sub-state used inside `Mode::TableEditCellProps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableCellSubState {
    /// Navigating cells; Space toggles selection.
    Selecting,
    /// Editing the style of selected (or cursor) cells.
    EditingStyle {
        selected_prop: usize,
        editing_value: Option<String>,
        cursor: usize,
        dropdown: Option<usize>,
    },
    /// Editing the text content of a single cell.
    EditingContent {
        row: usize,
        col: usize,
        buf: String,
        cursor: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    AddObject {
        selected: usize,
    },
    SelectObject {
        selected: usize,
    },
    SelectedObject {
        object_index: usize,
    },
    /// Resizing the selected object with plain arrow keys (Left/Right = width,
    /// Up/Down = height). A terminal-robust alternative to Shift+arrows, which
    /// many terminals capture for scrollback.
    ResizeObject {
        object_index: usize,
    },
    EditProperties {
        object_index: usize,
        selected_property: usize,
        editing_value: Option<String>,
        /// Char index of the text cursor; only meaningful when editing_value is Some.
        cursor: usize,
        /// Number of display characters scrolled off the left edge of the panel.
        scroll: usize,
        /// Number of visual property rows scrolled off the top of the panel.
        panel_scroll: usize,
        /// Index into COLOR_OPTIONS when a color dropdown is open; None otherwise.
        dropdown: Option<usize>,
    },
    AnimateProperty {
        object_index: usize,
        /// Which property index to restore in EditProperties when exiting.
        return_property: usize,
        /// The property name being animated (static because Property.name is &'static str).
        property_name: &'static str,
        /// Which field is highlighted: 0=from, 1=to, 2=start_frame, 3=end_frame.
        selected_field: usize,
        /// Text being typed into the selected field, if actively editing.
        editing: Option<String>,
        cursor: usize,
        from: u16,
        to: u16,
        start_frame: usize,
        end_frame: usize,
    },
    Confirm {
        message: String,
        /// 0 = Yes, 1 = No
        selected: usize,
        action: ConfirmAction,
        /// Mode to restore when the user picks No or presses Esc.
        return_mode: Box<Mode>,
    },
    /// Picking which existing objects belong to a new group being created.
    SelectGroupMembers {
        /// Currently highlighted object index in the full objects list.
        selected: usize,
        /// Objects toggled into the group so far.
        members: Vec<usize>,
    },
    /// Adding a column to a table (before or after a reference column).
    TableAddColumn {
        object_index: usize,
        /// true = add after `col_num`, false = add before.
        after: bool,
        /// 1-indexed reference column number currently being entered.
        col_num: usize,
        buf: String,
        cursor: usize,
    },
    /// Removing a column from a table: ask which column, then confirm.
    TableRemoveColumn {
        object_index: usize,
        /// 1-indexed column number currently selected.
        col_num: usize,
        buf: String,
        cursor: usize,
    },
    /// Choosing a piece from the ASCII-art library to add. The entry at index
    /// `items.len()` is the "Load from file…" action.
    AddArt {
        selected: usize,
        items: Vec<ArtItem>,
    },
    /// Typing a path to load a custom art file at runtime.
    LoadArtFile {
        buf: String,
        cursor: usize,
    },
    /// Presentation settings — currently the output frame size (width × height).
    Settings {
        /// 0 = width, 1 = height.
        selected_field: usize,
        width_buf: String,
        height_buf: String,
        /// Text cursor within the selected field's buffer.
        cursor: usize,
    },
    /// Frame operations sub-menu (opened with [f]rame from Normal): add a
    /// blank frame, copy/delete the current frame, or move it.
    FrameMenu,
    /// Relocating the current slide. Left/Right scroll the deck to a target
    /// slide (tracked by `current_frame`); Enter then opens `FrameMovePlace`.
    FrameMove {
        /// Original index of the slide being moved.
        from: usize,
    },
    /// Choosing whether the moved slide lands before or after the shown slide.
    FrameMovePlace {
        /// Original index of the slide being moved.
        from: usize,
        /// Target slide (currently shown) the moved slide will sit next to.
        target: usize,
    },
    /// Navigating / selecting cells in a table to edit their properties.
    TableEditCellProps {
        object_index: usize,
        cursor_row: usize,
        cursor_col: usize,
        /// Set of (row, col) cells toggled as selected.
        selected_cells: Vec<(usize, usize)>,
        sub_state: TableCellSubState,
    },
}

pub struct EditorState {
    pub source: SourcePresentation,
    pub file_path: String,
    pub current_frame: usize,
    pub mode: Mode,
    pub config: EditorConfig,
    pub dirty: bool,
    pub status_message: Option<String>,
    /// Set during a blink animation to suppress the highlight on the selected object.
    pub blink_hidden: bool,
    /// "No bars" mode: hide the menu bar and timeline so the canvas fills the screen.
    pub fullscreen: bool,
}

impl EditorState {
    pub fn open(path: &str) -> Result<Self> {
        let source = if std::path::Path::new(path).exists() {
            let json =
                std::fs::read_to_string(path).with_context(|| format!("Failed to read {path}"))?;
            serde_json::from_str(&json).with_context(|| format!("Failed to parse {path}"))?
        } else {
            SourcePresentation {
                width: 80,
                height: 24,
                frame_count: 1,
                objects: Vec::new(),
            }
        };

        Ok(EditorState {
            source,
            file_path: path.to_string(),
            current_frame: 0,
            mode: Mode::Normal,
            config: EditorConfig::load(),
            dirty: false,
            status_message: None,
            blink_hidden: false,
            fullscreen: false,
        })
    }

    pub fn save(&mut self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.source)?;
        std::fs::write(&self.file_path, &json)
            .with_context(|| format!("Failed to write {}", self.file_path))?;
        self.dirty = false;
        self.status_message = Some("Saved".into());
        Ok(())
    }

    /// Returns indices into `self.source.objects` for objects visible on `current_frame`.
    pub fn objects_on_current_frame(&self) -> Vec<usize> {
        self.source
            .objects
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                // Auto groups derive their span from their members, so go through
                // the presentation's effective-range helper rather than the raw
                // (possibly absent) stored range.
                self.source
                    .effective_frame_range(*i)
                    .contains(self.current_frame)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// The object's stored frame range. A `Group` with an *auto* range has none
/// (`None`); its effective span is derived via
/// `SourcePresentation::effective_frame_range`.
pub fn scene_object_frame_range(obj: &SceneObject) -> Option<&FrameRange> {
    match obj {
        SceneObject::Label(l) => Some(&l.frames),
        SceneObject::HLine(h) => Some(&h.frames),
        SceneObject::Rect(r) => Some(&r.frames),
        SceneObject::Header(h) => Some(&h.frames),
        SceneObject::Group(g) => g.frames.as_ref(),
        SceneObject::Arrow(a) => Some(&a.frames),
        SceneObject::Table(t) => Some(&t.frames),
        SceneObject::Art(a) => Some(&a.frames),
        SceneObject::Command(c) => Some(&c.frames),
        SceneObject::List(l) => Some(&l.frames),
    }
}

/// Mutable access to the object's stored frame range. Returns `None` for an
/// *auto* group (no stored range to mutate).
pub fn scene_object_frame_range_mut(obj: &mut SceneObject) -> Option<&mut FrameRange> {
    match obj {
        SceneObject::Label(l) => Some(&mut l.frames),
        SceneObject::HLine(h) => Some(&mut h.frames),
        SceneObject::Rect(r) => Some(&mut r.frames),
        SceneObject::Header(h) => Some(&mut h.frames),
        SceneObject::Group(g) => g.frames.as_mut(),
        SceneObject::Arrow(a) => Some(&mut a.frames),
        SceneObject::Table(t) => Some(&mut t.frames),
        SceneObject::Art(a) => Some(&mut a.frames),
        SceneObject::Command(c) => Some(&mut c.frames),
        SceneObject::List(l) => Some(&mut l.frames),
    }
}

pub fn scene_object_type_name(obj: &SceneObject) -> &'static str {
    match obj {
        SceneObject::Label(_) => "Label",
        SceneObject::HLine(_) => "HLine",
        SceneObject::Rect(_) => "Rect",
        SceneObject::Header(_) => "Header",
        SceneObject::Group(_) => "Group",
        SceneObject::Arrow(_) => "Arrow",
        SceneObject::Table(_) => "Table",
        SceneObject::Art(_) => "Art",
        SceneObject::Command(_) => "Command",
        SceneObject::List(_) => "List",
    }
}

/// Collect mutable references to all Coordinate fields of a SceneObject.
fn scene_object_coordinates_mut(obj: &mut SceneObject) -> Vec<&mut Coordinate> {
    match obj {
        SceneObject::Label(l) => vec![
            &mut l.position.x,
            &mut l.position.y,
            &mut l.width,
            &mut l.height,
        ],
        SceneObject::Rect(r) => vec![
            &mut r.position.x,
            &mut r.position.y,
            &mut r.width,
            &mut r.height,
        ],
        SceneObject::HLine(h) => vec![&mut h.y, &mut h.x_start, &mut h.x_end],
        SceneObject::Header(h) => vec![&mut h.position.x, &mut h.position.y],
        SceneObject::Arrow(a) => vec![&mut a.x1, &mut a.y1, &mut a.x2, &mut a.y2],
        SceneObject::Group(_) => vec![],
        SceneObject::Table(t) => vec![
            &mut t.position.x,
            &mut t.position.y,
            &mut t.width,
            &mut t.height,
        ],
        SceneObject::Art(a) => vec![&mut a.position.x, &mut a.position.y],
        SceneObject::Command(c) => vec![
            &mut c.position.x,
            &mut c.position.y,
            &mut c.width,
            &mut c.height,
        ],
        SceneObject::List(l) => vec![
            &mut l.position.x,
            &mut l.position.y,
            &mut l.width,
            &mut l.height,
        ],
    }
}

/// Insert a frame just after `inserted_after`, *duplicating* that frame's
/// content: an object whose range ends exactly at the boundary is extended so
/// the new frame looks identical to the source frame.
pub fn adjust_frames_after_insert(source: &mut SourcePresentation, inserted_after: usize) {
    insert_frame(source, inserted_after, false);
}

/// Insert a *blank* frame just after `inserted_after`: objects local to the
/// source frame do not extend into the new one, so it starts empty.
///
/// (Objects spanning *past* the source frame still cover the new frame — a
/// contiguous range can't skip a single interior frame — so deck-wide
/// backgrounds remain visible, matching the existing range-based model.)
pub fn insert_blank_frame(source: &mut SourcePresentation, inserted_after: usize) {
    insert_frame(source, inserted_after, true);
}

/// Shared frame-insertion logic. The only difference between duplicate and
/// blank is the end-shift threshold: a range ending exactly at the new frame
/// position is extended for a duplicate but left alone for a blank insert.
fn insert_frame(source: &mut SourcePresentation, inserted_after: usize, blank: bool) {
    source.frame_count += 1;
    let end_threshold = if blank { inserted_after + 1 } else { inserted_after };
    for obj in &mut source.objects {
        // Auto groups have no stored range to shift; their members shift instead.
        if let Some(fr) = scene_object_frame_range_mut(obj) {
            if fr.end > end_threshold {
                fr.end += 1;
            }
            if fr.start > inserted_after {
                fr.start += 1;
            }
        }
        for coord in scene_object_coordinates_mut(obj) {
            if let Coordinate::Animated {
                start_frame,
                end_frame,
                ..
            } = coord
            {
                if *start_frame > inserted_after {
                    *start_frame += 1;
                }
                if *end_frame > inserted_after {
                    *end_frame += 1;
                }
            }
        }
    }
}

/// Reorder the deck so frame `from` sits immediately before (`before == true`)
/// or after (`before == false`) frame `target`, and return the moved frame's
/// new index.
///
/// Frames are implicit (defined by object ranges), so this is a permutation of
/// frame indices. Each contiguous range is remapped to the contiguous hull of
/// its members' new positions: exact when the move doesn't reorder frames
/// *inside* the range (the common case — single-slide and whole-deck objects),
/// and an inclusive approximation when a partial multi-frame span is torn by
/// the move (the span then also covers the frames it was spread across).
pub fn move_frame(
    source: &mut SourcePresentation,
    from: usize,
    target: usize,
    before: bool,
) -> usize {
    let n = source.frame_count;
    if n <= 1 || from >= n || target >= n || from == target {
        return from;
    }
    // Build the new ordering: order[new_index] = old_index.
    let mut order: Vec<usize> = (0..n).filter(|&f| f != from).collect();
    let target_pos = order.iter().position(|&f| f == target).unwrap_or(0);
    let insert_at = if before { target_pos } else { target_pos + 1 };
    order.insert(insert_at, from);

    // Inverse map: pos[old_index] = new_index.
    let mut pos = vec![0usize; n];
    for (new_idx, &old) in order.iter().enumerate() {
        pos[old] = new_idx;
    }

    for obj in &mut source.objects {
        if let Some(fr) = scene_object_frame_range_mut(obj) {
            if fr.start < fr.end {
                let (mut lo, mut hi) = (usize::MAX, 0usize);
                for f in fr.start..fr.end {
                    let p = pos[f.min(n - 1)];
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                fr.start = lo;
                fr.end = hi + 1;
            }
        }
        for coord in scene_object_coordinates_mut(obj) {
            if let Coordinate::Animated {
                start_frame,
                end_frame,
                ..
            } = coord
            {
                let a = pos[(*start_frame).min(n - 1)];
                let b = pos[(*end_frame).min(n - 1)];
                *start_frame = a.min(b);
                *end_frame = a.max(b);
            }
        }
    }

    pos[from]
}

/// Adjust all frame indices after frame `deleted` has been removed.
pub fn adjust_frames_after_delete(source: &mut SourcePresentation, deleted: usize) {
    source.frame_count -= 1;
    for obj in &mut source.objects {
        if let Some(fr) = scene_object_frame_range_mut(obj) {
            if fr.start > deleted {
                fr.start -= 1;
            }
            if fr.end > deleted {
                fr.end -= 1;
            }
        }
    }
    // Remove objects whose frame range collapsed. Auto groups (no stored range)
    // are kept — their visibility follows their members, which are pruned here.
    source.objects.retain(|obj| {
        scene_object_frame_range(obj).map_or(true, |fr| fr.start < fr.end)
    });
    for obj in &mut source.objects {
        for coord in scene_object_coordinates_mut(obj) {
            if let Coordinate::Animated {
                start_frame,
                end_frame,
                ..
            } = coord
            {
                if *start_frame > deleted {
                    *start_frame -= 1;
                }
                if *end_frame > deleted {
                    *end_frame -= 1;
                }
            }
        }
    }
}

/// After an object at `removed_idx` is deleted, fix group member index references.
pub fn adjust_group_members_after_delete(source: &mut SourcePresentation, removed_idx: usize) {
    for obj in &mut source.objects {
        if let SceneObject::Group(g) = obj {
            g.members.retain(|&m| m != removed_idx);
            for m in &mut g.members {
                if *m > removed_idx {
                    *m -= 1;
                }
            }
        }
    }
}

pub fn scene_object_summary(obj: &SceneObject) -> String {
    match obj {
        SceneObject::Label(l) => {
            let first_line = l.text.split('\n').next().unwrap_or("");
            let text_preview: String = first_line.chars().take(15).collect();
            format!("Label: \"{}\"", text_preview)
        }
        SceneObject::HLine(h) => format!("HLine: y={} x={}..{}", h.y.evaluate(0), h.x_start.evaluate(0), h.x_end.evaluate(0)),
        SceneObject::Rect(r) => format!("Rect: {}x{}", r.width.evaluate(0), r.height.evaluate(0)),
        SceneObject::Header(h) => {
            let text_preview: String = h.text.chars().take(10).collect();
            format!("Header: \"{}\"", text_preview)
        }
        SceneObject::Group(g) => format!("Group: {} members", g.members.len()),
        SceneObject::Arrow(a) => format!("Arrow: ({},{})→({},{})", a.x1.evaluate(0), a.y1.evaluate(0), a.x2.evaluate(0), a.y2.evaluate(0)),
        SceneObject::Table(t) => format!("Table: {}r×{}c", t.rows, t.col_widths.len()),
        SceneObject::Art(a) => {
            let name = if a.name.is_empty() { "custom" } else { &a.name };
            format!("Art: {name}")
        }
        SceneObject::Command(c) => format!("Command: {}", c.command),
        SceneObject::List(l) => {
            let kind = if l.ordered { "ordered" } else { "unordered" };
            let count = l.text.split('\n').filter(|s| !s.is_empty()).count();
            format!("List: {count} items ({kind})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::object_defaults::create_default;

    /// A Label spanning frames `[start, end)`.
    fn label(start: usize, end: usize) -> SceneObject {
        let mut obj = create_default(0, 0);
        if let Some(fr) = scene_object_frame_range_mut(&mut obj) {
            fr.start = start;
            fr.end = end;
        }
        obj
    }

    fn pres(frame_count: usize, objects: Vec<SceneObject>) -> SourcePresentation {
        SourcePresentation { width: 80, height: 24, frame_count, objects }
    }

    fn range(obj: &SceneObject) -> (usize, usize) {
        let fr = scene_object_frame_range(obj).unwrap();
        (fr.start, fr.end)
    }

    #[test]
    fn copy_frame_extends_a_single_frame_object_onto_the_new_frame() {
        // Copy duplicates: an object local to the source frame appears on both.
        let mut p = pres(1, vec![label(0, 1)]);
        adjust_frames_after_insert(&mut p, 0);
        assert_eq!(p.frame_count, 2);
        assert_eq!(range(&p.objects[0]), (0, 2));
    }

    #[test]
    fn blank_frame_leaves_a_single_frame_object_behind() {
        // Blank insert does not extend the source frame's object into the new one.
        let mut p = pres(1, vec![label(0, 1)]);
        insert_blank_frame(&mut p, 0);
        assert_eq!(p.frame_count, 2);
        assert_eq!(range(&p.objects[0]), (0, 1));
    }

    #[test]
    fn blank_frame_still_shifts_later_objects() {
        // An object on a later frame slides forward to make room for the blank.
        let mut p = pres(2, vec![label(0, 1), label(1, 2)]);
        insert_blank_frame(&mut p, 0); // new blank frame at index 1
        assert_eq!(p.frame_count, 3);
        assert_eq!(range(&p.objects[0]), (0, 1)); // unchanged
        assert_eq!(range(&p.objects[1]), (2, 3)); // shifted past the blank
    }

    #[test]
    fn move_frame_relocates_single_frame_objects_after_target() {
        // Deck 0,1,2,3 → move frame 0 to after frame 2 → order 1,2,0,3.
        let mut p = pres(4, vec![label(0, 1), label(1, 2), label(2, 3), label(3, 4)]);
        let new_index = move_frame(&mut p, 0, 2, false);
        assert_eq!(new_index, 2);
        assert_eq!(range(&p.objects[0]), (2, 3)); // old frame 0 → index 2
        assert_eq!(range(&p.objects[1]), (0, 1)); // old frame 1 → index 0
        assert_eq!(range(&p.objects[2]), (1, 2)); // old frame 2 → index 1
        assert_eq!(range(&p.objects[3]), (3, 4)); // old frame 3 unchanged
        assert_eq!(p.frame_count, 4); // move never changes the count
    }

    #[test]
    fn move_frame_relocates_before_target() {
        // Deck 0,1,2,3 → move frame 3 to before frame 1 → order 0,3,1,2.
        let mut p = pres(4, vec![label(0, 1), label(1, 2), label(2, 3), label(3, 4)]);
        let new_index = move_frame(&mut p, 3, 1, true);
        assert_eq!(new_index, 1);
        assert_eq!(range(&p.objects[3]), (1, 2)); // old frame 3 → index 1
        assert_eq!(range(&p.objects[1]), (2, 3)); // old frame 1 → index 2
    }

    #[test]
    fn move_frame_keeps_a_whole_deck_object_spanning_the_whole_deck() {
        let mut p = pres(3, vec![label(0, 3)]);
        move_frame(&mut p, 0, 2, false);
        assert_eq!(range(&p.objects[0]), (0, 3));
    }

    #[test]
    fn move_frame_is_a_noop_onto_itself() {
        let mut p = pres(3, vec![label(0, 1)]);
        let new_index = move_frame(&mut p, 1, 1, false);
        assert_eq!(new_index, 1);
        assert_eq!(range(&p.objects[0]), (0, 1));
    }
}
