use anyhow::{Context, Result};

use crate::art_library::ArtItem;
use crate::engine::source::{Animation, Coordinate, FrameRange, SceneObject, SourcePresentation};

use super::config::EditorConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DeleteFrame,
    /// Delete a multi-selected set of frames (0-based indices).
    DeleteFrames { frames: Vec<usize> },
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

/// What the ASCII-art picker (`Mode::AddArt` / `Mode::LoadArtFile`) is choosing
/// a piece *for*. Lets the one picker flow serve both a standalone `Art` object
/// and the two-stage `from`/`to` selection of a new `Morph`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtPick {
    /// Add a standalone `Art` object with the chosen piece.
    Art,
    /// Pick the *from* piece of a new `Morph`; the next pick is its *to* piece.
    MorphFrom,
    /// Pick the *to* piece of a new `Morph`, carrying the already-chosen *from*.
    MorphTo { from_art: String, from_name: String },
}

/// What a [`Mode::MultiSelect`] session is collecting objects for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiSelectPurpose {
    /// Build a new `Group` from the toggled objects.
    Group,
    /// Copy the toggled objects to the clipboard.
    Copy,
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
        /// Which field is highlighted: 0=from, 1=to, 2=start_frame, 3=end_frame,
        /// 4=add_frames (toggle), 5=auto_play (toggle), 6=delay_ms.
        selected_field: usize,
        /// Text being typed into the selected field, if actively editing.
        editing: Option<String>,
        cursor: usize,
        /// `from`/`to` are the animated values of the primary axis (x, or
        /// width/height); `from_y`/`to_y` the y axis when `two_axis` is set (the
        /// object has both an x and a y coordinate, so both animate together).
        from: u16,
        to: u16,
        from_y: u16,
        to_y: u16,
        /// Whether this session animates both x and y (a position coordinate on an
        /// object that has both). When false, only the named property animates.
        two_axis: bool,
        start_frame: usize,
        end_frame: usize,
        /// Insert the frames the animation spans (and share the current frame's
        /// elements across them) on apply. Default on.
        add_frames: bool,
        /// Auto-advance across the animation's span at play time. Default on.
        auto_play: bool,
        /// Auto-play delay between frames, in milliseconds. Default 500.
        delay_ms: u64,
        /// Show the animated element only every `gap_frames`-th frame of the span
        /// (a stop-motion strobe with empty gaps between). `1` = every frame (off).
        gap_frames: usize,
    },
    Confirm {
        message: String,
        /// 0 = Yes, 1 = No
        selected: usize,
        action: ConfirmAction,
        /// Mode to restore when the user picks No or presses Esc.
        return_mode: Box<Mode>,
    },
    /// Toggling a set of objects on the current frame, for either grouping or
    /// copying (`purpose`). Reuses one multi-select flow for both.
    MultiSelect {
        purpose: MultiSelectPurpose,
        /// Currently highlighted object's position in the visible list.
        selected: usize,
        /// Objects toggled into the set so far (real `objects` indices).
        members: Vec<usize>,
    },
    /// Placing freshly-pasted clones: a movable "ghost" that rides the arrow keys
    /// until Enter drops it (and re-arms the next stamp). Esc discards the
    /// un-dropped set. The clones live in `objects` (so the preview shows them);
    /// `pending` is their tail indices.
    PastePlacing {
        /// Indices of the clones currently being placed (the live ghost), in
        /// clipboard order.
        pending: Vec<usize>,
        /// Whether this paste session links its copies (synced edits).
        linked: bool,
        /// One accumulating link family **per clipboard object** (seeded with its
        /// source, then each stamp's clone of it), so an object links only to its
        /// own copies — distinct objects copied together never cross-sync.
        /// Written to `source.links` when the session ends. Empty until the first
        /// linked stamp.
        families: Vec<Vec<usize>>,
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
    /// `items.len()` is the "Load from file…" action. `purpose` says whether the
    /// pick becomes a standalone `Art` or one end of a new `Morph`.
    AddArt {
        selected: usize,
        items: Vec<ArtItem>,
        purpose: ArtPick,
    },
    /// Typing a path to load a custom art file at runtime. `purpose` is carried
    /// through so a file picked mid-morph routes the same as a library pick.
    LoadArtFile {
        buf: String,
        cursor: usize,
        purpose: ArtPick,
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
    /// blank frame, copy/delete the current frame, jump, select, or move it.
    FrameMenu,
    /// Typing a (1-based) frame number to jump the deck to.
    FrameJump {
        buf: String,
        cursor: usize,
    },
    /// Typing a multi-frame selection (`1, 2, 3` or a range `5-12`, mixable).
    FrameSelectInput {
        buf: String,
        cursor: usize,
    },
    /// A set of frames has been selected (0-based indices); `d` deletes them.
    FrameSelected {
        frames: Vec<usize>,
    },
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
    /// Overlaying (pasting) the source slide's objects onto another existing
    /// slide. Left/Right scroll the deck to the target slide (tracked by
    /// `current_frame`); Enter then pastes the source slide's objects on top of
    /// it. Unlike `FrameMove`/`FrameMovePlace` this inserts no new frame.
    FrameOverlay {
        /// Index of the source slide whose objects will be pasted.
        from: usize,
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
    /// Self-contained deep clones captured by **copy**, ready to **paste**. Group
    /// member indices inside are clipboard-local. Persists across frames/pastes.
    pub clipboard: Vec<SceneObject>,
    /// Source object indices the clipboard was copied from, for a *linked* paste
    /// (so the stamped copies sync with the original). Re-validated at paste.
    pub clipboard_sources: Vec<usize>,
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
                links: Vec::new(),
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
            clipboard: Vec::new(),
            clipboard_sources: Vec::new(),
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
        SceneObject::Loop(l) => Some(&l.frames),
        SceneObject::Morph(m) => Some(&m.frames),
        SceneObject::Animation(a) => Some(&a.frames),
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
        SceneObject::Loop(l) => Some(&mut l.frames),
        SceneObject::Morph(m) => Some(&mut m.frames),
        SceneObject::Animation(a) => Some(&mut a.frames),
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
        SceneObject::Loop(_) => "Loop",
        SceneObject::Morph(_) => "Morph",
        SceneObject::Animation(_) => "Animation",
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
        // A loop has no coordinates (it draws nothing); its frame range still
        // shifts via `scene_object_frame_range_mut` during frame insert/delete.
        SceneObject::Loop(_) => vec![],
        // A morph is sized by its art content (like Art); only its position can
        // animate, so width/height are absent here.
        SceneObject::Morph(m) => vec![&mut m.position.x, &mut m.position.y],
        // An animation span has no coordinates (it draws nothing); its frame
        // range still shifts via `scene_object_frame_range_mut`.
        SceneObject::Animation(_) => vec![],
    }
}

/// The union of every animated coordinate's window on `obj`, as an exclusive
/// `[start, end)` frame range — i.e. `[min start_frame, max end_frame + 1)`
/// (an animation reaches its destination *on* `end_frame`, so the exclusive end
/// is one past it). Returns `None` when the object has no animated coordinate.
///
/// Used to keep an object's visible range in lock-step with its animation(s):
/// applying or editing an animation recomputes the range from this span, so it
/// both grows to cover a longer animation and shrinks when one is shortened
/// (no frames left visible past the animation's new end).
pub fn scene_object_animation_span(obj: &mut SceneObject) -> Option<(usize, usize)> {
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for coord in scene_object_coordinates_mut(obj) {
        if let Coordinate::Animated { start_frame, end_frame, .. } = coord {
            lo = lo.min(*start_frame);
            hi = hi.max(*end_frame + 1);
        }
    }
    (lo != usize::MAX).then_some((lo, hi))
}

/// Insert a *blank* frame just after `inserted_after`: objects local to the
/// source frame do not extend into the new one, so it starts empty. This is the
/// "make room" primitive shared by the editor's *add blank frame* action and by
/// [`copy_frame`] (which then deep-clones the source frame's objects onto it).
///
/// Objects spanning *past* the source frame still cover the new frame — a
/// contiguous range can't skip a single interior frame — so deck-wide
/// backgrounds remain visible, matching the range-based frame model.
pub fn insert_blank_frame(source: &mut SourcePresentation, inserted_after: usize) {
    source.frame_count += 1;
    // A range ending exactly at the new frame position is left alone (the
    // source frame's object does not bleed into the blank one); only ranges
    // that genuinely span past it are stretched to stay contiguous.
    let end_threshold = inserted_after + 1;
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

/// Copy (duplicate) the frame at `current`, inserting an independent copy
/// immediately after it. Every object shown on `current` also appears on the
/// new frame, but as a **deep, independent clone** — editing the copy never
/// changes the original (and vice versa).
///
/// Objects that already span *past* `current` (e.g. a deck-wide background)
/// stay shared: they are extended across the new frame rather than cloned, so
/// they remain a single continuous object. Only per-slide objects — those that
/// the blank insert would not otherwise carry onto the new frame — are cloned.
pub fn copy_frame(source: &mut SourcePresentation, current: usize) {
    let new_frame = current + 1;
    // Objects visible on the source frame are the clone candidates.
    let visible: Vec<usize> = (0..source.objects.len())
        .filter(|&i| source.effective_frame_range(i).contains(current))
        .collect();

    // Make room for the new frame; spanning objects extend across it.
    insert_blank_frame(source, current);

    // Clone every visible object the blank insert did NOT carry onto the new
    // frame, so the new frame gets its own independent copies.
    let mut index_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &i in &visible {
        if source.effective_frame_range(i).contains(new_frame) {
            continue; // already shared onto the new frame (kept continuous)
        }
        let mut clone = source.objects[i].clone();
        // Plain objects (and explicit-range groups) land on the new frame only;
        // an auto group has no stored range and stays auto (derived from its
        // — also cloned — members).
        if let Some(fr) = scene_object_frame_range_mut(&mut clone) {
            fr.start = new_frame;
            fr.end = new_frame + 1;
        }
        let new_index = source.objects.len();
        source.objects.push(clone);
        index_map.insert(i, new_index);
    }

    // Re-point cloned groups at their cloned members. A member that was not
    // cloned (a shared spanning object) keeps referring to the original.
    for &new_index in index_map.values() {
        if let SceneObject::Group(g) = &mut source.objects[new_index] {
            for m in &mut g.members {
                if let Some(&mapped) = index_map.get(m) {
                    *m = mapped;
                }
            }
        }
    }
}

/// Copy every object shown on frame `from` and paste an independent, deep clone
/// of each **on top of** the existing frame `onto` — same positions, styles, and
/// z-order — without inserting a new frame (the deck's `frame_count` is
/// unchanged). The target frame keeps whatever it already had; the pasted
/// clones, appended after the existing objects, render over it.
///
/// Objects already visible on `onto` (e.g. a deck-wide background spanning both
/// frames) are skipped — they are already present, so re-cloning them would just
/// stack an identical duplicate on top. Cloned groups are re-pointed at their
/// cloned members (a skipped, shared member keeps referring to the original).
///
/// Returns the number of objects pasted. A no-op (returns 0) when `from == onto`.
pub fn overlay_frame(source: &mut SourcePresentation, from: usize, onto: usize) -> usize {
    if from == onto {
        return 0;
    }
    // Objects visible on the source frame are the paste candidates.
    let visible: Vec<usize> = (0..source.objects.len())
        .filter(|&i| source.effective_frame_range(i).contains(from))
        .collect();

    let mut index_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &i in &visible {
        if source.effective_frame_range(i).contains(onto) {
            continue; // already present on the target frame — don't duplicate it
        }
        let mut clone = source.objects[i].clone();
        // Plain objects (and explicit-range groups) land on the target frame
        // only; an auto group has no stored range and stays auto (derived from
        // its — also cloned — members).
        if let Some(fr) = scene_object_frame_range_mut(&mut clone) {
            fr.start = onto;
            fr.end = onto + 1;
        }
        let new_index = source.objects.len();
        source.objects.push(clone);
        index_map.insert(i, new_index);
    }

    // Re-point cloned groups at their cloned members. A member that was not
    // cloned (a shared spanning object) keeps referring to the original.
    for &new_index in index_map.values() {
        if let SceneObject::Group(g) = &mut source.objects[new_index] {
            for m in &mut g.members {
                if let Some(&mapped) = index_map.get(m) {
                    *m = mapped;
                }
            }
        }
    }

    index_map.len()
}

/// Give a new animation spanning `[start, end_frame]` (inclusive last animated
/// frame, started from the `current` frame) its own dedicated frames: insert
/// `N - 1` **fresh** blank frames immediately after `current` (where `N` is the
/// span length), then extend every object visible on `current` to span the whole
/// range. The elements therefore become a single shared object across all the
/// animation's frames — editing one edits them all.
///
/// The new frames are always inserted, not reused — any existing content after
/// `current` shifts back to make room (`insert_blank_frame` already extends
/// deck-wide/spanning objects across them, keeping each a single object). The
/// caller is responsible for *not* calling this twice for the same animation
/// (re-applying X+Y over one span, or re-saving), which would insert again — see
/// the span guard in `input::apply_animation`.
pub fn add_frames_and_share(
    source: &mut SourcePresentation,
    current: usize,
    start: usize,
    end_frame: usize,
) {
    let end_excl = end_frame + 1;
    // Elements to share are those visible on the current frame *before* growth.
    let visible: Vec<usize> = (0..source.objects.len())
        .filter(|&i| source.effective_frame_range(i).contains(current))
        .collect();
    // Insert N-1 fresh frames after the current one (N = span length). The
    // `current` frame is the span's first frame; the rest are brand new.
    let new_frames = end_frame.saturating_sub(start);
    for _ in 0..new_frames {
        insert_blank_frame(source, current);
    }
    // Extend each shared element across the span. Auto groups have no stored
    // range; their (also-visible) members extend instead.
    let lo = start.min(current);
    for &i in &visible {
        if let Some(fr) = scene_object_frame_range_mut(&mut source.objects[i]) {
            if fr.start > lo {
                fr.start = lo;
            }
            if fr.end < end_excl {
                fr.end = end_excl;
            }
        }
    }
}

/// Turn the just-animated element at `object_index` into a stop-motion strobe:
/// show it only on every `gap`-th frame of its span (frames `start`,
/// `start + gap`, `start + 2·gap`, … up to `end_frame`), leaving the frames
/// between as gaps. The original is kept on the first sample frame; single-frame
/// clones are added on the later sample frames. Every copy keeps the animated
/// coordinate, so each evaluates to the element's interpolated position for its
/// own frame — the motion is sampled, not held.
///
/// `gap <= 1` is a no-op (the element stays on every frame). Call this only on a
/// freshly created animation; the clones are independent objects, so re-applying
/// over the same span would stack duplicates.
pub fn apply_gap(
    source: &mut SourcePresentation,
    object_index: usize,
    start: usize,
    end_frame: usize,
    gap: usize,
) {
    if gap <= 1 {
        return;
    }
    // The original holds the first sample frame.
    if let Some(fr) = scene_object_frame_range_mut(&mut source.objects[object_index]) {
        fr.start = start;
        fr.end = start + 1;
    }
    let proto = source.objects[object_index].clone();
    let mut f = start + gap;
    while f <= end_frame {
        let mut clone = proto.clone();
        if let Some(fr) = scene_object_frame_range_mut(&mut clone) {
            fr.start = f;
            fr.end = f + 1;
        }
        source.objects.push(clone);
        f += gap;
    }
}

/// Create or update the [`Animation`] span `[start, end_excl)` with the given
/// auto-play config. If an animation with exactly this span already exists (e.g.
/// animating a second coordinate over the same frames), reuse it so X and Y of
/// the same object stay one animation; otherwise append a new one.
pub fn upsert_animation(
    source: &mut SourcePresentation,
    start: usize,
    end_excl: usize,
    auto_play: bool,
    delay_ms: u64,
) {
    for obj in &mut source.objects {
        if let SceneObject::Animation(a) = obj {
            if a.frames.start == start && a.frames.end == end_excl {
                a.auto_play = auto_play;
                a.delay_ms = delay_ms;
                return;
            }
        }
    }
    source.objects.push(SceneObject::Animation(Animation {
        frames: FrameRange { start, end: end_excl },
        auto_play,
        delay_ms,
    }));
}

/// Replace every `Animated` coordinate on `obj` with a `Fixed` value sampled at
/// `frame`. Used when pasting: a clone is re-anchored to a single frame, where
/// an animated coordinate is degenerate — it would also be un-nudgeable, since
/// the arrow-key move only adjusts `Fixed` coordinates. Flattening makes the
/// pasted copy a static, movable object showing the position it had at `frame`.
pub fn flatten_coordinates(obj: &mut SceneObject, frame: usize) {
    for coord in scene_object_coordinates_mut(obj) {
        if matches!(coord, Coordinate::Animated { .. }) {
            *coord = Coordinate::Fixed(coord.evaluate(frame) as f64);
        }
    }
}

/// Expand a selection so every selected `Group` also pulls in its members — a
/// copied group is meaningless without the objects it contains. Returns the
/// selection plus those members, de-duplicated, in ascending index order.
pub fn expand_selection(source: &SourcePresentation, indices: &[usize]) -> Vec<usize> {
    let mut set: Vec<usize> = indices.to_vec();
    for &i in indices {
        if let Some(SceneObject::Group(g)) = source.objects.get(i) {
            for &m in &g.members {
                if !set.contains(&m) {
                    set.push(m);
                }
            }
        }
    }
    set.sort_unstable();
    set.dedup();
    set
}

/// Deep-clone the objects at `indices` into a self-contained list whose internal
/// `Group.members` are remapped to be **selection-local** (an index into the
/// returned vec). Members that were not part of the selection are dropped, so a
/// copied group only references objects that travel with it.
pub fn clone_selection(objects: &[SceneObject], indices: &[usize]) -> Vec<SceneObject> {
    // Map each source index to its position within the cloned list.
    let pos: std::collections::HashMap<usize, usize> = indices
        .iter()
        .enumerate()
        .map(|(local, &src)| (src, local))
        .collect();
    let mut out: Vec<SceneObject> = indices.iter().map(|&i| objects[i].clone()).collect();
    for obj in &mut out {
        if let SceneObject::Group(g) = obj {
            g.members = g.members.iter().filter_map(|m| pos.get(m).copied()).collect();
        }
    }
    out
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

/// Parse a multi-frame selection string into 0-based, sorted, de-duplicated
/// frame indices clamped to the deck. Accepts comma-separated tokens, each a
/// single 1-based number (`3`) or an inclusive 1-based range (`5-12`), mixable
/// (`1, 3, 5-8`). Returns an error message on a malformed token or empty result.
pub fn parse_frame_selection(input: &str, frame_count: usize) -> Result<Vec<usize>, String> {
    let mut out: Vec<usize> = Vec::new();
    for token in input.split(',') {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        if let Some((a, b)) = t.split_once('-') {
            let a: usize = a.trim().parse().map_err(|_| format!("bad range '{t}'"))?;
            let b: usize = b.trim().parse().map_err(|_| format!("bad range '{t}'"))?;
            if a == 0 || b == 0 || a > b {
                return Err(format!("bad range '{t}'"));
            }
            out.extend((a..=b).map(|n| n - 1));
        } else {
            let n: usize = t.parse().map_err(|_| format!("bad number '{t}'"))?;
            if n == 0 {
                return Err("frames are numbered from 1".into());
            }
            out.push(n - 1);
        }
    }
    out.retain(|&f| f < frame_count);
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err("no valid frames in range".into());
    }
    Ok(out)
}

/// Delete a set of frames (0-based), highest index first so the lower indices
/// stay valid as the deck shrinks. Always keeps at least one frame — once the
/// deck is down to a single frame, further deletions are skipped. Returns the
/// number of frames actually removed.
pub fn delete_frames(source: &mut SourcePresentation, frames: &[usize]) -> usize {
    let mut fs: Vec<usize> = frames.to_vec();
    fs.sort_unstable();
    fs.dedup();
    let mut removed = 0;
    for &f in fs.iter().rev() {
        if source.frame_count > 1 && f < source.frame_count {
            adjust_frames_after_delete(source, f);
            removed += 1;
        }
    }
    removed
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
    // Each removal also fixes up group member indices (which reference positions
    // in `objects`), so a surviving group can't end up pointing at the wrong
    // object after the array shifts.
    let mut i = 0;
    while i < source.objects.len() {
        let collapsed = scene_object_frame_range(&source.objects[i])
            .is_some_and(|fr| fr.start >= fr.end);
        if collapsed {
            source.objects.remove(i);
            adjust_group_members_after_delete(source, i);
        } else {
            i += 1;
        }
    }
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

/// After an object at `removed_idx` is deleted, fix the index references that
/// point into `objects`: `Group.members` and the `links` families. Each drops
/// the removed index and shifts every higher index down by one. Link families
/// that fall below two members are pruned (a one-object "family" syncs nothing).
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
    for fam in &mut source.links {
        fam.retain(|&m| m != removed_idx);
        for m in fam.iter_mut() {
            if *m > removed_idx {
                *m -= 1;
            }
        }
    }
    source.links.retain(|fam| fam.len() >= 2);
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
        SceneObject::Loop(l) => {
            let n = l.frames.end.saturating_sub(l.frames.start);
            let times = if l.count == 0 { "∞".to_string() } else { format!("{}×", l.count) };
            let mode = if l.bounce { "bounce" } else { "loop" };
            format!("Loop: {n} frames {times} ({mode})")
        }
        SceneObject::Morph(m) => {
            let n = m.frames.end.saturating_sub(m.frames.start);
            let label = if m.name.is_empty() { m.mode.as_str().to_string() } else { m.name.clone() };
            format!("Morph: {label} ({n} frames)")
        }
        SceneObject::Animation(a) => {
            let lo = a.frames.start + 1;
            let hi = a.frames.end; // exclusive end == 1-based inclusive last
            let play = if a.auto_play { format!("auto {}ms", a.delay_ms) } else { "manual".into() };
            format!("Animation: {lo}-{hi} ({play})")
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
        SourcePresentation { width: 80, height: 24, frame_count, objects, links: Vec::new() }
    }

    fn range(obj: &SceneObject) -> (usize, usize) {
        let fr = scene_object_frame_range(obj).unwrap();
        (fr.start, fr.end)
    }

    fn set_text(obj: &mut SceneObject, t: &str) {
        if let SceneObject::Label(l) = obj {
            l.text = t.into();
        }
    }
    fn text(obj: &SceneObject) -> String {
        match obj {
            SceneObject::Label(l) => l.text.clone(),
            _ => String::new(),
        }
    }
    fn group(members: Vec<usize>) -> SceneObject {
        let mut g = create_default(4, 0); // Group, auto range
        if let SceneObject::Group(grp) = &mut g {
            grp.members = members;
        }
        g
    }

    fn members_of(obj: &SceneObject) -> Vec<usize> {
        match obj {
            SceneObject::Group(g) => g.members.clone(),
            _ => vec![],
        }
    }

    #[test]
    fn expand_selection_pulls_in_group_members() {
        // objects: [label0, label1, group(0,1)]. Selecting just the group must
        // expand to include its members.
        let p = pres(1, vec![label(0, 1), label(0, 1), group(vec![0, 1])]);
        assert_eq!(expand_selection(&p, &[2]), vec![0, 1, 2]);
        // A plain selection is unchanged (deduped/sorted).
        assert_eq!(expand_selection(&p, &[1, 0]), vec![0, 1]);
    }

    #[test]
    fn clone_selection_remaps_members_locally_and_is_independent() {
        // Select [label0, group(0)] → cloned group points at the cloned label
        // (local index 1), and editing a clone never touches the original.
        let mut p = pres(1, vec![label(0, 1), group(vec![0])]);
        let mut clones = clone_selection(&p.objects, &[0, 1]);
        assert_eq!(clones.len(), 2);
        assert_eq!(members_of(&clones[1]), vec![0]); // remapped to local pos of orig 0
        set_text(&mut clones[0], "changed");
        assert_eq!(text(&p.objects[0]), "New Label"); // original untouched
        assert_eq!(text(&clones[0]), "changed");
        // A member dropped from the selection is not referenced by the clone.
        set_text(&mut p.objects[0], "orig");
        assert_eq!(text(&clones[0]), "changed");
    }

    #[test]
    fn clone_selection_drops_members_outside_the_selection() {
        // group references member 0, but only the group is selected (not 0).
        let p = pres(1, vec![label(0, 1), group(vec![0])]);
        let clones = clone_selection(&p.objects, &[1]);
        assert_eq!(members_of(&clones[0]), Vec::<usize>::new());
    }

    #[test]
    fn apply_gap_strobes_element_onto_every_nth_frame() {
        // A label animated over span [0,10). gap=4 → keep the original on the
        // first sample frame, clone onto frames 4 and 8 (single-frame each); the
        // frames between are gaps.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 0, to: 9, start_frame: 0, end_frame: 9 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![obj]);
        apply_gap(&mut p, 0, 0, 9, 4);
        assert_eq!(p.objects.len(), 3); // original + 2 clones
        assert_eq!(range(&p.objects[0]), (0, 1));
        assert_eq!(range(&p.objects[1]), (4, 5));
        assert_eq!(range(&p.objects[2]), (8, 9));
        // Every copy keeps the animated coordinate, so each samples its own frame.
        for o in &p.objects {
            match o {
                SceneObject::Label(l) => {
                    assert!(matches!(l.position.x, Coordinate::Animated { .. }));
                }
                _ => panic!(),
            }
        }
    }

    #[test]
    fn apply_gap_of_one_is_a_noop() {
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![obj]);
        apply_gap(&mut p, 0, 0, 9, 1);
        assert_eq!(p.objects.len(), 1);
        assert_eq!(range(&p.objects[0]), (0, 10)); // untouched
    }

    #[test]
    fn flatten_coordinates_converts_animated_to_fixed_at_frame() {
        // A label whose x animates 2→12 over frames 0..=4. Flattening at the end
        // frame yields a Fixed x of 12 — static and now nudgeable by move_by
        // (which only adjusts Fixed coords), so a paste can be moved horizontally.
        let mut obj = create_default(0, 0); // Label, all Fixed
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 2, to: 12, start_frame: 0, end_frame: 4 };
        }
        flatten_coordinates(&mut obj, 4);
        match &obj {
            SceneObject::Label(l) => match l.position.x {
                Coordinate::Fixed(v) => assert_eq!(v, 12.0),
                _ => panic!("x should be Fixed after flattening"),
            },
            _ => panic!(),
        }
        // No animated coordinate remains.
        assert_eq!(scene_object_animation_span(&mut obj), None);
    }

    #[test]
    fn link_siblings_returns_family_minus_self() {
        let mut p = pres(1, vec![label(0, 1), label(0, 1), label(0, 1), label(0, 1)]);
        p.links = vec![vec![0, 2, 3]];
        assert_eq!(p.link_siblings(2), vec![0, 3]);
        assert_eq!(p.link_siblings(0), vec![2, 3]);
        assert_eq!(p.link_siblings(1), Vec::<usize>::new()); // not in any family
    }

    #[test]
    fn delete_shifts_and_prunes_link_families() {
        let mut p = pres(1, vec![label(0, 1), label(0, 1), label(0, 1), label(0, 1), label(0, 1)]);
        p.links = vec![vec![0, 2, 4]];
        // Remove index 2: drop it, shift 4 → 3.
        adjust_group_members_after_delete(&mut p, 2);
        assert_eq!(p.links, vec![vec![0, 3]]);
        // Remove index 0: family collapses to one member and is pruned.
        adjust_group_members_after_delete(&mut p, 0);
        assert!(p.links.is_empty());
    }

    #[test]
    fn copy_frame_clones_objects_independently() {
        let mut p = pres(1, vec![label(0, 1)]);
        copy_frame(&mut p, 0);
        assert_eq!(p.frame_count, 2);
        // A true copy — two distinct objects, not one shared span.
        assert_eq!(p.objects.len(), 2);
        assert_eq!(range(&p.objects[0]), (0, 1));
        assert_eq!(range(&p.objects[1]), (1, 2));
        // Editing the copy must not change the original.
        set_text(&mut p.objects[1], "changed");
        assert_eq!(text(&p.objects[0]), "New Label");
        assert_eq!(text(&p.objects[1]), "changed");
    }

    #[test]
    fn overlay_frame_pastes_clones_onto_existing_frame_without_growing_deck() {
        // Frame 0 has a label; frame 1 has its own. Overlay 0 onto 1 pastes an
        // independent clone of frame 0's object onto frame 1 — no new frame.
        let mut p = pres(2, vec![label(0, 1), label(1, 2)]);
        let pasted = overlay_frame(&mut p, 0, 1);
        assert_eq!(pasted, 1);
        assert_eq!(p.frame_count, 2); // overlay never changes the count
        assert_eq!(p.objects.len(), 3);
        assert_eq!(range(&p.objects[0]), (0, 1)); // source untouched
        assert_eq!(range(&p.objects[1]), (1, 2)); // target's own object kept
        assert_eq!(range(&p.objects[2]), (1, 2)); // clone lands on the target frame
        // The clone is independent of the source.
        set_text(&mut p.objects[2], "changed");
        assert_eq!(text(&p.objects[0]), "New Label");
        assert_eq!(text(&p.objects[2]), "changed");
    }

    #[test]
    fn overlay_frame_skips_objects_already_on_the_target() {
        // obj0 spans the whole deck (already on every frame); obj1 is local to
        // frame 0. Overlaying 0 onto 1 must paste only obj1 — the background is
        // already present on frame 1, so re-cloning it would just duplicate it.
        let mut p = pres(2, vec![label(0, 2), label(0, 1)]);
        let pasted = overlay_frame(&mut p, 0, 1);
        assert_eq!(pasted, 1);
        assert_eq!(p.objects.len(), 3);
        assert_eq!(range(&p.objects[0]), (0, 2)); // background unchanged, not duplicated
        assert_eq!(range(&p.objects[1]), (0, 1)); // source-local original
        assert_eq!(range(&p.objects[2]), (1, 2)); // clone pasted onto frame 1
    }

    #[test]
    fn overlay_frame_onto_itself_is_a_noop() {
        let mut p = pres(2, vec![label(0, 1)]);
        let pasted = overlay_frame(&mut p, 0, 0);
        assert_eq!(pasted, 0);
        assert_eq!(p.objects.len(), 1);
    }

    #[test]
    fn overlay_frame_repoints_cloned_group_members() {
        // A group over a per-slide member on frame 0, overlaid onto frame 1:
        // both the member and the group are cloned, and the cloned group points
        // at the cloned member.
        let mut p = pres(2, vec![label(0, 1), group(vec![0])]);
        let pasted = overlay_frame(&mut p, 0, 1);
        assert_eq!(pasted, 2); // member + group
        // Cloned member at index 2, cloned group at index 3 pointing to it.
        assert_eq!(range(&p.objects[2]), (1, 2));
        match &p.objects[3] {
            SceneObject::Group(g) => assert_eq!(g.members, vec![2]),
            _ => panic!("expected a cloned group at index 3"),
        }
    }

    #[test]
    fn copy_frame_keeps_a_spanning_background_shared() {
        // obj0 spans the whole 2-frame deck (a background); obj1 is local to
        // frame 0. Copying frame 0 extends the background (still one object) and
        // clones only the per-slide object.
        let mut p = pres(2, vec![label(0, 2), label(0, 1)]);
        copy_frame(&mut p, 0);
        assert_eq!(p.objects.len(), 3);
        assert_eq!(range(&p.objects[0]), (0, 3)); // background extended, not cloned
        assert_eq!(range(&p.objects[1]), (0, 1)); // original local object
        assert_eq!(range(&p.objects[2]), (1, 2)); // independent clone on new frame
    }

    #[test]
    fn delete_frame_fixes_group_member_indices() {
        // obj0 collapses on delete; the surviving group must follow the
        // background as the array shifts down.
        let mut p = pres(3, vec![label(2, 3), label(0, 3), group(vec![1])]);
        adjust_frames_after_delete(&mut p, 2);
        assert_eq!(p.objects.len(), 2);
        match &p.objects[1] {
            SceneObject::Group(g) => assert_eq!(g.members, vec![0]),
            _ => panic!("expected a group at index 1"),
        }
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
    fn animation_span_unions_animated_coordinates_and_makes_end_exclusive() {
        // x animated over frames 5..=10 → exclusive span [5, 11).
        let mut obj = create_default(0, 0); // Label, all Fixed
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 0, to: 9, start_frame: 5, end_frame: 10 };
        }
        assert_eq!(scene_object_animation_span(&mut obj), Some((5, 11)));

        // A second animation starting earlier widens the union's start only.
        if let SceneObject::Label(l) = &mut obj {
            l.position.y = Coordinate::Animated { from: 0, to: 3, start_frame: 2, end_frame: 8 };
        }
        assert_eq!(scene_object_animation_span(&mut obj), Some((2, 11)));

        // No animated coordinate → no span (range is left untouched on apply).
        let mut plain = create_default(0, 0);
        assert_eq!(scene_object_animation_span(&mut plain), None);
    }

    #[test]
    fn add_frames_and_share_grows_the_deck_and_shares_elements() {
        // One frame, two local elements. Animate over 10 frames from frame 0:
        // insert 9 frames and extend both elements to span [0, 10).
        let mut p = pres(1, vec![label(0, 1), label(0, 1)]);
        add_frames_and_share(&mut p, 0, 0, 9); // current=0, start=0, end_frame=9
        assert_eq!(p.frame_count, 10);
        assert_eq!(range(&p.objects[0]), (0, 10));
        assert_eq!(range(&p.objects[1]), (0, 10));
    }

    #[test]
    fn add_frames_and_share_inserts_n_minus_1_fresh_frames() {
        // Deck already has 4 frames; animating frame 0 over a 6-frame span
        // (start=0, end_frame=5) inserts 5 NEW frames after frame 0 — the old
        // frames 1-3 shift back — and the element is shared across the span.
        let mut p = pres(4, vec![label(0, 1)]);
        add_frames_and_share(&mut p, 0, 0, 5); // span [0,6) ⇒ 5 new frames
        assert_eq!(p.frame_count, 9); // 4 existing + 5 fresh
        assert_eq!(range(&p.objects[0]), (0, 6)); // shared across the span
    }

    #[test]
    fn upsert_animation_reuses_a_matching_span() {
        // Animating X then Y over the same span keeps a single Animation entity.
        let mut p = pres(10, vec![label(0, 10)]);
        upsert_animation(&mut p, 0, 10, true, 500);
        upsert_animation(&mut p, 0, 10, false, 200); // same span → update in place
        let anims: Vec<_> = p
            .objects
            .iter()
            .filter_map(|o| match o {
                SceneObject::Animation(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(anims.len(), 1);
        assert!(!anims[0].auto_play);
        assert_eq!(anims[0].delay_ms, 200);
    }

    #[test]
    fn upsert_animation_appends_a_distinct_span() {
        let mut p = pres(10, vec![label(0, 10)]);
        upsert_animation(&mut p, 0, 5, true, 500);
        upsert_animation(&mut p, 3, 9, true, 500); // overlapping but distinct span
        let count = p
            .objects
            .iter()
            .filter(|o| matches!(o, SceneObject::Animation(_)))
            .count();
        assert_eq!(count, 2);
    }

    #[test]
    fn move_frame_is_a_noop_onto_itself() {
        let mut p = pres(3, vec![label(0, 1)]);
        let new_index = move_frame(&mut p, 1, 1, false);
        assert_eq!(new_index, 1);
        assert_eq!(range(&p.objects[0]), (0, 1));
    }

    #[test]
    fn parse_frame_selection_handles_lists_ranges_and_mixes() {
        // 1-based input → 0-based, sorted, de-duplicated, clamped to the deck.
        assert_eq!(parse_frame_selection("1, 2, 3", 10).unwrap(), vec![0, 1, 2]);
        assert_eq!(parse_frame_selection("5-12", 10).unwrap(), vec![4, 5, 6, 7, 8, 9]);
        assert_eq!(parse_frame_selection("1, 3, 5-7", 10).unwrap(), vec![0, 2, 4, 5, 6]);
        assert_eq!(parse_frame_selection("3, 1, 1", 10).unwrap(), vec![0, 2]); // sort + dedup
    }

    #[test]
    fn parse_frame_selection_rejects_bad_input() {
        assert!(parse_frame_selection("0", 10).is_err()); // frames are 1-based
        assert!(parse_frame_selection("abc", 10).is_err());
        assert!(parse_frame_selection("7-3", 10).is_err()); // reversed range
        assert!(parse_frame_selection("", 10).is_err()); // nothing
        assert!(parse_frame_selection("20-30", 10).is_err()); // all out of range
    }

    #[test]
    fn delete_frames_removes_highest_first_and_keeps_one() {
        // 5 single-frame labels (one per frame). Delete frames 2 and 4 (0-based
        // 1 and 3): two removed, the survivors keep their relative order.
        let mut p = pres(5, vec![label(0, 1), label(1, 2), label(2, 3), label(3, 4), label(4, 5)]);
        let removed = delete_frames(&mut p, &[1, 3]);
        assert_eq!(removed, 2);
        assert_eq!(p.frame_count, 3);

        // Deleting every frame keeps the last one (never empties the deck).
        let mut q = pres(3, vec![label(0, 1), label(1, 2), label(2, 3)]);
        let removed = delete_frames(&mut q, &[0, 1, 2]);
        assert_eq!(removed, 2);
        assert_eq!(q.frame_count, 1);
    }
}
