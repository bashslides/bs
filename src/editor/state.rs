use anyhow::{Context, Result};

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
            .filter(|(_, obj)| {
                let range = scene_object_frame_range(obj);
                range.contains(self.current_frame)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

pub fn scene_object_frame_range(obj: &SceneObject) -> &FrameRange {
    match obj {
        SceneObject::Label(l) => &l.frames,
        SceneObject::HLine(h) => &h.frames,
        SceneObject::Rect(r) => &r.frames,
        SceneObject::Header(h) => &h.frames,
        SceneObject::Group(g) => &g.frames,
        SceneObject::Arrow(a) => &a.frames,
        SceneObject::Table(t) => &t.frames,
    }
}

pub fn scene_object_frame_range_mut(obj: &mut SceneObject) -> &mut FrameRange {
    match obj {
        SceneObject::Label(l) => &mut l.frames,
        SceneObject::HLine(h) => &mut h.frames,
        SceneObject::Rect(r) => &mut r.frames,
        SceneObject::Header(h) => &mut h.frames,
        SceneObject::Group(g) => &mut g.frames,
        SceneObject::Arrow(a) => &mut a.frames,
        SceneObject::Table(t) => &mut t.frames,
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
    }
}

/// Adjust all frame indices after a frame has been inserted after `inserted_after`.
pub fn adjust_frames_after_insert(source: &mut SourcePresentation, inserted_after: usize) {
    source.frame_count += 1;
    for obj in &mut source.objects {
        let fr = scene_object_frame_range_mut(obj);
        if fr.end > inserted_after {
            fr.end += 1;
        }
        if fr.start > inserted_after {
            fr.start += 1;
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

/// Adjust all frame indices after frame `deleted` has been removed.
pub fn adjust_frames_after_delete(source: &mut SourcePresentation, deleted: usize) {
    source.frame_count -= 1;
    for obj in &mut source.objects {
        let fr = scene_object_frame_range_mut(obj);
        if fr.start > deleted {
            fr.start -= 1;
        }
        if fr.end > deleted {
            fr.end -= 1;
        }
    }
    // Remove objects whose frame range collapsed
    source.objects.retain(|obj| {
        let fr = scene_object_frame_range(obj);
        fr.start < fr.end
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
    }
}
