use anyhow::{Context, Result};
use std::io::Write;

use crate::art_library::ArtItem;
use crate::engine::source::{
    AnimId, AnimSpans, Animation, AutoAdvance, Coordinate, FrameRange, SceneObject,
    SourcePresentation,
};

use super::config::EditorConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DeleteFrame,
    /// Delete a multi-selected set of frames (0-based indices).
    DeleteFrames { frames: Vec<usize> },
    DeleteObject { object_index: usize },
    /// Delete a multi-selected set of objects (indices into `source.objects`).
    DeleteObjects { object_indices: Vec<usize> },
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
    /// General selection: Enter routes to the single-object menu (1 object) or
    /// the action sub-menu (2+ objects). The main `s` entry point. Copy, converge,
    /// and delete are then chosen from that action sub-menu (`SelectAction`).
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    AddObject {
        selected: usize,
    },
    SelectedObject {
        object_index: usize,
    },
    /// The action sub-menu shown after a multi-object selection (`s` → select →
    /// 2+ objects). Lists the operations available on the whole set.
    SelectAction {
        /// The selected objects (real `objects` indices) the action applies to.
        members: Vec<usize>,
        /// Highlighted action row (index into `SELECT_ACTIONS`).
        selected: usize,
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
    /// Bulk-edit the properties **common** to a multi-object selection (reached
    /// from the `SelectAction` "Edit Props" action). Only the shared, simple
    /// properties are listed; editing a value writes it to every member. Mirrors
    /// the editing sub-state of [`Mode::EditProperties`] but carries the whole
    /// `members` set instead of a single `object_index`.
    EditMultiProperties {
        members: Vec<usize>,
        selected_property: usize,
        editing_value: Option<String>,
        cursor: usize,
        scroll: usize,
        panel_scroll: usize,
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
    /// Configuring a *converge* animation: a set of objects (`members`) each
    /// animate from wherever they sit at the span start to one shared target
    /// point (`to`, `to_y`). Mirrors [`Mode::AnimateProperty`] but the `from`
    /// values are per-object (seeded at apply time), so only the shared target
    /// and span/auto-play config are edited here.
    ConvergeConfig {
        /// Objects that will converge (real `objects` indices).
        members: Vec<usize>,
        /// Highlighted field (index into `CONVERGE_ROLES`).
        selected_field: usize,
        /// Text being typed into the selected field, if actively editing.
        editing: Option<String>,
        cursor: usize,
        /// Shared target column / row every member animates toward.
        to: u16,
        to_y: u16,
        start_frame: usize,
        end_frame: usize,
        /// Insert the spanned frames (and share the current frame's elements)
        /// on apply. Default off for converge (animate over existing frames).
        add_frames: bool,
        /// Auto-advance across the span at play time. Default on.
        auto_play: bool,
        /// Auto-play delay between frames, in milliseconds. Default 500.
        delay_ms: u64,
        /// Stop-motion strobe: empty frames between appearances (0 = off).
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
    /// "Save as" — a centred popup for typing the filename to write to. Enter
    /// saves (and adopts the new path); Esc cancels.
    SaveAs {
        buf: String,
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
    /// Typing the auto-advance delay (in seconds) for the current frame. Enter
    /// sets it (`0`/empty turns auto-advance off); Esc returns to the frame menu.
    FrameAutoInput {
        buf: String,
        cursor: usize,
    },
    /// A set of frames has been selected (0-based indices); `d` deletes them,
    /// and (for a contiguous range) `m` moves or `c` copies them as a block.
    FrameSelected {
        frames: Vec<usize>,
    },
    /// Placing a moved/copied contiguous frame block. Left/Right scroll the deck
    /// to a target slide (tracked by `current_frame`); Enter drops the block
    /// *after* it, `b` *before* it. `copy` distinguishes duplicate from relocate.
    FrameRangePlace {
        /// The contiguous block being placed (0-based indices, ascending).
        frames: Vec<usize>,
        /// `true` = duplicate the block (copy); `false` = relocate it (move).
        copy: bool,
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
    /// The presentations switcher/hub: lists every open deck (Enter switches to
    /// the highlighted one) and offers open-file / save-as / settings /
    /// fullscreen. `selected` is the highlighted deck row (index into the active
    /// deck's `workspace.deck_names`).
    PresentationMenu {
        selected: usize,
    },
    /// Typing a path to open another presentation as a new deck (reached from the
    /// presentations menu). Enter opens it; Esc returns to the menu.
    OpenFile {
        buf: String,
        cursor: usize,
    },
    /// Placing the cross-deck frame clipboard into this deck. Left/Right scroll
    /// the deck to a target slide (tracked by `current_frame`); Enter drops the
    /// pasted block *after* it, `b` *before* it. The block itself lives on the
    /// workspace-level frame clipboard, so this carries no fields.
    FramePastePlace,
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

/// A read-only snapshot of workspace-level state (the other open decks and the
/// cross-deck frame clipboard) that the `Editor` mirrors into the active deck
/// before each redraw. The menu bar, the presentations switcher panel, and the
/// frame sub-menu read it; only the `Editor` writes it. Keeping it here lets the
/// renderers and handlers stay `&EditorState`-only while still showing
/// cross-deck information.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceView {
    /// Display name (file basename, with a trailing `*` when dirty) of every open
    /// deck, in deck order.
    pub deck_names: Vec<String>,
    /// Index of the currently active deck within `deck_names`.
    pub active: usize,
    /// Number of frames currently held in the cross-deck frame clipboard (0 when
    /// empty). Gates the "paste frames" frame-menu action.
    pub frame_clip_frames: usize,
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
    /// Mirror of workspace-level state (other open decks, the cross-deck frame
    /// clipboard), refreshed by the `Editor` before each redraw. See
    /// [`WorkspaceView`].
    pub workspace: WorkspaceView,
    /// JSON keys of the opened file the model does not keep ([`unknown_fields`]):
    /// warned about on open and again on the first save, which drops them.
    pub dropped_fields: Vec<String>,
}

/// The JSON keys in `raw` (a deck file) that the loaded `source` does not keep,
/// so a save would drop them — `_comment` notes, misspelt fields (`"styel"`),
/// fields of a newer version — as paths like `title` or `objects[2].styel`.
/// Checked at the top level and inside each object (nested maps and arrays
/// included). A key missing from the re-serialized model may also be a known
/// field left at its default (skipped on write); those are told apart by
/// probing: an unknown key's value never changes the model, a known one's does
/// (or fails to load). Probing re-parses the object, so each verdict is cached
/// per object type and key path (array indices ignored) — a deck repeating a
/// default like `z_order: 0` probes it once — and at most [`MAX_PROBED_KEYS`]
/// probes run; past that (only a file flooded with distinct junk keys) the
/// rest are listed unprobed, keeping open fast.
pub fn unknown_fields(raw: &serde_json::Value, source: &SourcePresentation) -> Vec<String> {
    use serde_json::Value;
    let Ok(model) = serde_json::to_value(source) else { return Vec::new() };
    let mut out = Vec::new();
    let mut prober = Prober { budget: MAX_PROBED_KEYS, known: std::collections::HashMap::new() };
    // The deck itself, with its objects left out (each is checked on its own).
    let without_objects = |v: &Value| {
        let mut v = v.clone();
        if let Some(m) = v.as_object_mut() {
            m.insert("objects".into(), Value::Array(Vec::new()));
        }
        v
    };
    for path in unknown_in::<SourcePresentation>(&without_objects(raw), &without_objects(&model), &mut prober) {
        out.push(path_name("", &path));
    }
    if let (Some(raw_objs), Some(objs)) = (raw["objects"].as_array(), model["objects"].as_array()) {
        for (i, (r, m)) in raw_objs.iter().zip(objs).enumerate() {
            for path in unknown_in::<SceneObject>(r, m, &mut prober) {
                out.push(path_name(&format!("objects[{i}]"), &path));
            }
        }
    }
    out
}

/// How many candidate keys [`unknown_fields`] probes in all (see there).
const MAX_PROBED_KEYS: usize = 256;

/// [`unknown_fields`]' probing state: the probes left, and each verdict so far
/// (`true` = a known field) by object type and index-free key path.
struct Prober {
    budget: usize,
    known: std::collections::HashMap<String, bool>,
}

/// One step of a JSON path: a map key or an array index.
#[derive(Clone)]
enum Step {
    Key(String),
    Index(usize),
}

fn path_name(prefix: &str, path: &[Step]) -> String {
    let mut s = prefix.to_string();
    for step in path {
        match step {
            Step::Key(k) if s.is_empty() => s.push_str(k),
            Step::Key(k) => {
                s.push('.');
                s.push_str(k);
            }
            Step::Index(i) => s.push_str(&format!("[{i}]")),
        }
    }
    s
}

/// The paths of keys in `raw` (a `T` as JSON) that `model` (the loaded `T`,
/// re-serialized) lacks and that are not known fields at their default. A
/// verdict cached in `prober` is reused; a new probe spends one of its budget,
/// and once that is spent the remaining candidates are returned unprobed.
fn unknown_in<T: serde::de::DeserializeOwned + serde::Serialize>(
    raw: &serde_json::Value,
    model: &serde_json::Value,
    prober: &mut Prober,
) -> Vec<Vec<Step>> {
    let mut candidates = Vec::new();
    missing_keys(raw, model, &mut Vec::new(), &mut candidates);
    if candidates.is_empty() {
        return candidates;
    }
    let reload = |v: serde_json::Value| serde_json::from_value::<T>(v).ok().and_then(|t| serde_json::to_value(t).ok());
    let Some(base) = reload(raw.clone()) else { return Vec::new() };
    let probes = [
        serde_json::json!("\u{1}probe"),
        serde_json::json!(7_654_321),
        serde_json::json!(true),
        serde_json::json!({ "\u{1}probe": 1 }),
        serde_json::json!([1]),
        serde_json::Value::Null,
    ];
    let kind = raw["type"].as_str().unwrap_or("deck");
    candidates
        .into_iter()
        .filter(|path| {
            let key = format!("{kind}:{}", path_name("", &path.iter().map(|s| match s {
                Step::Index(_) => Step::Index(0),
                k => k.clone(),
            }).collect::<Vec<_>>()));
            if let Some(&known) = prober.known.get(&key) {
                return !known;
            }
            if prober.budget == 0 {
                return true;
            }
            prober.budget -= 1;
            let unknown = probes.iter().all(|p| {
                let mut v = raw.clone();
                set_at(&mut v, path, p.clone());
                reload(v).as_ref() == Some(&base)
            });
            prober.known.insert(key, !unknown);
            unknown
        })
        .collect()
}

/// Collect the paths of map keys in `raw` absent from `model`, recursing into
/// maps present in both and into arrays of equal length.
fn missing_keys(raw: &serde_json::Value, model: &serde_json::Value, path: &mut Vec<Step>, out: &mut Vec<Vec<Step>>) {
    use serde_json::Value;
    match (raw, model) {
        (Value::Object(r), Value::Object(m)) => {
            for (k, rv) in r {
                path.push(Step::Key(k.clone()));
                match m.get(k) {
                    None => out.push(path.clone()),
                    Some(mv) => missing_keys(rv, mv, path, out),
                }
                path.pop();
            }
        }
        (Value::Array(r), Value::Array(m)) if r.len() == m.len() => {
            for (i, (rv, mv)) in r.iter().zip(m).enumerate() {
                path.push(Step::Index(i));
                missing_keys(rv, mv, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn set_at(v: &mut serde_json::Value, path: &[Step], value: serde_json::Value) {
    let mut cur = v;
    for step in path {
        cur = match step {
            Step::Key(k) => &mut cur[k.as_str()],
            Step::Index(i) => &mut cur[*i],
        };
    }
    *cur = value;
}

/// Replace the file at `path` atomically: `write` fills a new temp file beside
/// the (symlink-resolved) target, created with the old file's permissions,
/// which is synced and renamed over it. On any failure the temp file is removed
/// and the old file is left untouched, so a full disk or a crash can't truncate
/// the deck. Like any rename-replace it gives the file a new inode (hard links
/// and ownership aren't carried over). Only if the directory refuses a new file
/// does it fall back to rewriting the file in place.
fn replace_file(
    path: &str,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::path::{Path, PathBuf};
    // Follow symlinks (even a dangling one) so the link keeps pointing at the
    // file we write.
    let mut target = PathBuf::from(path);
    let mut hops = 0;
    while std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
        hops += 1;
        if hops > 40 {
            return Err(Error::other(format!("too many levels of symbolic links: {path}")));
        }
        let link = std::fs::read_link(&target)?;
        target = target.parent().unwrap_or(Path::new("")).join(link);
    }
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let perms = std::fs::metadata(&target).ok().map(|m| m.permissions());
    if perms.is_some() {
        // A write-protected deck stays protected: fail as an in-place write would.
        std::fs::OpenOptions::new().write(true).open(&target)?;
    }

    // A fresh temp file: `create_new` refuses an existing file or symlink, so a
    // planted name can't redirect the write.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(perms.as_ref().map_or(0o666, |p| p.mode() & 0o7777));
    }
    let mut n = 0;
    let (tmp, mut f) = loop {
        let tmp = dir.join(format!(".{name}.tmp-{}-{n}", std::process::id()));
        match opts.open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                // Directory not writable: rewrite in place, as before.
                let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(&target)?;
                write(&mut f)?;
                return f.sync_all();
            }
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        if let Some(p) = perms {
            f.set_permissions(p)?; // exact bits, before any content lands
        }
        write(&mut f)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    } else if let Ok(d) = std::fs::File::open(&dir) {
        let _ = d.sync_all(); // persist the rename itself (best effort)
    }
    result
}

/// The status note for [`unknown_fields`]: `None` when there are none; at most a
/// few named, so the line stays short.
fn dropped_fields_note(fields: &[String], what: &str) -> Option<String> {
    const SHOWN: usize = 3;
    if fields.is_empty() {
        return None;
    }
    let mut names = fields.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
    if fields.len() > SHOWN {
        names.push_str(&format!(" and {} more", fields.len() - SHOWN));
    }
    Some(format!("unknown field(s) {names} {what}"))
}

/// Why compilation would reject `source` (its size limits, then its loops and
/// animations), if it would.
fn deck_problem(source: &SourcePresentation) -> Option<String> {
    source.validate_contract().and_then(|()| source.validate_loops()).err()
}

impl EditorState {
    /// Objects moved (`moved[old] = new`, as `add_frames_and_share` returns
    /// it): keep `clipboard_sources` on the same objects.
    pub fn note_moved_objects(&mut self, moved: &[usize]) {
        for s in &mut self.clipboard_sources {
            *s = moved.get(*s).copied().unwrap_or(*s);
        }
    }

    /// Objects were removed at `removed` (in removal order, each relative to the
    /// list as it stood at that removal — as `clear_gap_clones` and
    /// `prune_orphan_animations` return them). Keep `clipboard_sources` on the
    /// same objects, or forget them if a copied original itself is gone, so a
    /// later linked paste never links an unrelated object.
    pub fn note_removed_objects(&mut self, removed: &[usize]) {
        for &r in removed {
            if self.clipboard_sources.contains(&r) {
                self.clipboard_sources.clear();
                return;
            }
            for s in &mut self.clipboard_sources {
                *s = index_after_removals(*s, &[r]);
            }
        }
    }

    pub fn open(path: &str) -> Result<Self> {
        let mut dropped_fields = Vec::new();
        let mut source: SourcePresentation = if std::path::Path::new(path).exists() {
            let json =
                std::fs::read_to_string(path).with_context(|| format!("Failed to read {path}"))?;
            let source: SourcePresentation =
                serde_json::from_str(&json).with_context(|| format!("Failed to parse {path}"))?;
            if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&json) {
                dropped_fields = unknown_fields(&raw, &source);
            }
            source
        } else {
            SourcePresentation {
                width: 80,
                height: 24,
                frame_count: 1,
                objects: Vec::new(),
                links: Vec::new(),
            }
        };

        // A hand-edited file can carry a deck the compiler will reject (e.g. a
        // duplicate animation id the editor itself never creates, or a size
        // over the limits): say so now.
        let dropped_links = source.sanitize_links();
        let links_note = dropped_links.then_some("dropped link entries naming no object");
        let members_note =
            source.sanitize_group_members().then_some("dropped group members naming no object");
        let fields_note = dropped_fields_note(&dropped_fields, "will be dropped on save");
        let notes: Vec<String> = [
            deck_problem(&source),
            links_note.map(String::from),
            members_note.map(String::from),
            fields_note,
        ]
            .into_iter()
            .flatten()
            .collect();
        let status_message = (!notes.is_empty()).then(|| format!("⚠ {}", notes.join("; ")));
        Ok(EditorState {
            source,
            file_path: path.to_string(),
            current_frame: 0,
            mode: Mode::Normal,
            config: EditorConfig::load(),
            dirty: false,
            status_message,
            blink_hidden: false,
            fullscreen: false,
            clipboard: Vec::new(),
            clipboard_sources: Vec::new(),
            workspace: WorkspaceView::default(),
            dropped_fields,
        })
    }

    /// `" — ⚠ <problem>"` when the deck would fail compilation's size limits
    /// (`validate_contract`) or loop/animation validation, else empty —
    /// appended to open/save status messages.
    pub fn deck_warning(&self) -> String {
        deck_problem(&self.source).map(|e| format!(" — ⚠ {e}")).unwrap_or_default()
    }

    pub fn save(&mut self) -> Result<()> {
        self.write_file()?;
        self.status_message =
            Some(format!("Saved{}{}", self.deck_warning(), self.take_dropped_note("were dropped")));
        Ok(())
    }

    /// `" — ⚠ unknown field(s) … <what>"` once, for the first save of a file
    /// that had fields the model does not keep; empty afterwards.
    fn take_dropped_note(&mut self, what: &str) -> String {
        dropped_fields_note(&std::mem::take(&mut self.dropped_fields), what)
            .map(|n| format!(" — ⚠ {n}"))
            .unwrap_or_default()
    }

    /// Write the deck to `file_path` and clear `dirty`.
    fn write_file(&mut self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.source)?;
        // serde_json writes a non-finite float as `null`, which would not load
        // back: refuse to write a deck that does not round-trip.
        serde_json::from_str::<SourcePresentation>(&json)
            .context("refusing to save: the deck would not load back")?;
        replace_file(&self.file_path, |f| f.write_all(json.as_bytes()))
            .with_context(|| format!("Failed to write {}", self.file_path))?;
        self.dirty = false;
        Ok(())
    }

    /// Save to `path`, adopting it as the deck's file so later saves go there too.
    /// A failed write keeps the old path, so Ctrl-s still targets the old file.
    pub fn save_as(&mut self, path: &str) -> Result<()> {
        let old = std::mem::replace(&mut self.file_path, path.to_string());
        if let Err(e) = self.write_file() {
            self.file_path = old;
            return Err(e);
        }
        self.status_message =
            Some(format!("Saved as {path}{}{}", self.deck_warning(), self.take_dropped_note("were not written")));
        Ok(())
    }

    /// Returns indices into `self.source.objects` for objects visible on `current_frame`.
    pub fn objects_on_current_frame(&self) -> Vec<usize> {
        // A member of an explicit-range group is drawn on the group's range
        // (the engine's override); auto groups derive their span from their
        // members, so everything else goes through the effective-range helper
        // rather than the raw (possibly absent) stored range.
        let overrides = self.source.member_overrides();
        (0..self.source.objects.len())
            .filter(|&i| {
                overrides[i]
                    .clone()
                    .unwrap_or_else(|| self.source.effective_frame_range(i))
                    .contains(self.current_frame)
            })
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
        SceneObject::AutoAdvance(a) => Some(&a.frames),
        SceneObject::Circle(c) => Some(&c.frames),
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
        SceneObject::AutoAdvance(a) => Some(&mut a.frames),
        SceneObject::Circle(c) => Some(&mut c.frames),
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
        // Auto-advance has no coordinates (it draws nothing); its frame range
        // still shifts via `scene_object_frame_range_mut`.
        SceneObject::AutoAdvance(_) => vec![],
        // A circle is positioned (x/y animate); its diameter is a plain number.
        SceneObject::Circle(c) => vec![&mut c.position.x, &mut c.position.y],
    }
}

/// The animation ids referenced by `obj`'s `Animated` coordinates (an object can
/// be driven by several animations — e.g. x and y over different spans).
pub fn referenced_anim_ids(obj: &SceneObject) -> Vec<AnimId> {
    let mut clone = obj.clone();
    scene_object_coordinates_mut(&mut clone)
        .into_iter()
        .filter_map(|c| match c {
            Coordinate::Animated { anim, .. } => Some(*anim),
            _ => None,
        })
        .collect()
}

/// The union of the spans of every animation driving `obj`, as an exclusive
/// `[start, end)` frame range. Returns `None` when the object has no animated
/// coordinate (or its animations are missing from `anims`).
///
/// Used to keep an object's visible range in lock-step with its animation(s):
/// applying or editing an animation recomputes the range from this span, so it
/// both grows to cover a longer animation and shrinks when one is shortened.
pub fn scene_object_animation_span(
    obj: &SceneObject,
    anims: &AnimSpans,
) -> Option<(usize, usize)> {
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for id in referenced_anim_ids(obj) {
        if let Some(fr) = anims.span(id) {
            lo = lo.min(fr.start);
            hi = hi.max(fr.end);
        }
    }
    (lo != usize::MAX).then_some((lo, hi))
}

/// The next free [`AnimId`] for `source`: one past the largest id held by an
/// `Animation` or referenced by any coordinate (or `1` when there is none).
/// Persisted ids make this stable across save/load; counting references too
/// means a stray reference to a missing animation is never re-bound to a new one.
pub fn next_anim_id(source: &SourcePresentation) -> AnimId {
    source
        .objects
        .iter()
        .flat_map(|o| match o {
            SceneObject::Animation(a) => vec![a.id],
            _ => referenced_anim_ids(o),
        })
        .max()
        .map_or(1, |m| m + 1)
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
        // (Animation spans shift here too — an `Animation` is an object with a
        // frame range, which is now the single source of truth for its span; the
        // driven coordinates carry no span to keep in sync.)
        if let Some(fr) = scene_object_frame_range_mut(obj) {
            // Saturating: a hand-edited range near usize::MAX must not wrap.
            if fr.end > end_threshold {
                fr.end = fr.end.saturating_add(1);
            }
            if fr.start > inserted_after {
                fr.start = fr.start.saturating_add(1);
            }
        }
    }
}

/// Why inserting a block that carries a `Loop` at `dest` would break
/// [`SourcePresentation::validate_loops`], if it would: the insert stretches a
/// loop or animation spanning `dest` around the block, so the carried loop would
/// nest in that loop or cut that animation in half.
fn loop_insert_refusal(source: &SourcePresentation, dest: usize, carries_loop: bool) -> Option<String> {
    if !carries_loop {
        return None;
    }
    // By the ranges `validate_loops` checks: an explicit group overrides its
    // members' ranges.
    source.effective_objects().iter().find_map(|o| {
        let (kind, fr) = match o.as_ref() {
            SceneObject::Loop(l) => ("loop", &l.frames),
            SceneObject::Animation(a) => ("animation", &a.frames),
            _ => return None,
        };
        (fr.start < dest && dest < fr.end).then(|| {
            format!(
                "The block holds a loop, which can't go inside the {kind} on slides {}-{} — pick another slide",
                fr.start.saturating_add(1),
                fr.end
            )
        })
    })
}

/// Why [`copy_frames`] refuses this copy (it then leaves the deck unchanged):
/// the block carries a whole `Loop` and the copy would land inside another loop
/// or an animation ([`loop_insert_refusal`]).
pub fn copy_frames_refusal(
    source: &SourcePresentation,
    lo: usize,
    hi: usize,
    target: usize,
    before: bool,
) -> Option<String> {
    let dest = if before { target } else { target + 1 };
    // `copy_frames` clones a loop wholly inside the block (by its shown range),
    // unless the insert stretches it (then it is the loop around `dest` itself).
    let carries = source.effective_objects().iter().any(|o| {
        matches!(o.as_ref(), SceneObject::Loop(l)
            if lo <= l.frames.start && l.frames.end <= hi + 1
                && !(l.frames.start < dest && dest < l.frames.end))
    });
    loop_insert_refusal(source, dest, carries)
}

/// Where [`paste_frame_block`] lands: the clamped target frame, and the index
/// the pasted block starts at.
fn paste_dest(target: &SourcePresentation, target_frame: usize, before: bool) -> (usize, usize) {
    let target_frame = target_frame.min(target.frame_count.saturating_sub(1));
    (target_frame, if before { target_frame } else { target_frame + 1 })
}

/// Why [`paste_frame_block`] refuses this paste (it then leaves the deck
/// unchanged): as [`copy_frames_refusal`], for a clipboard holding a `Loop`.
pub fn paste_frame_block_refusal(
    target: &SourcePresentation,
    clip: &FrameClipboard,
    target_frame: usize,
    before: bool,
) -> Option<String> {
    let (_, dest) = paste_dest(target, target_frame, before);
    let carries = clip.objects.iter().any(|o| matches!(o, SceneObject::Loop(_)));
    loop_insert_refusal(target, dest, carries)
}

/// Re-point each cloned group (`index_map`: original -> clone index) at its
/// cloned members. An auto group keeps an uncloned member only when `shared`
/// says it is shown on the frames the clone lands on (spanning onto them, so
/// it is part of the copy without a clone of its own); any other uncloned
/// member — one on another slide — is dropped, or the copy would own it and a
/// move of the copy would move it on the original slides. A cloned *explicit*
/// group drops every uncloned member — that member is shown on the copy through
/// another group already, and keeping it would let the clone's override (last
/// one wins in `member_overrides`) steal it from the original slide.
fn repoint_cloned_groups(
    source: &mut SourcePresentation,
    index_map: &std::collections::HashMap<usize, usize>,
    shared: impl Fn(usize) -> bool,
) {
    for &new_index in index_map.values() {
        let explicit = matches!(&source.objects[new_index], SceneObject::Group(g) if g.frames.is_some());
        let members = match &source.objects[new_index] {
            SceneObject::Group(g) => g.members.clone(),
            _ => continue,
        };
        let members = members
            .into_iter()
            .filter_map(|m| match index_map.get(&m) {
                Some(&mapped) => Some(mapped),
                None if explicit
                    && !matches!(source.objects.get(m), Some(SceneObject::Animation(_))) =>
                {
                    None
                }
                None if !explicit && !shared(m) => None,
                None => Some(m),
            })
            .collect();
        if let SceneObject::Group(g) = &mut source.objects[new_index] {
            g.members = members;
        }
    }
}

/// The range each object is actually shown on: a member of an explicit-range
/// group renders on the group's range ([`SourcePresentation::member_overrides`]),
/// everything else on its effective range. Frame copy/overlay decide what to
/// clone by this, so a member is cloned exactly when its explicit group is (the
/// group that wins its override; see [`repoint_cloned_groups`] for the rest).
fn shown_ranges(source: &SourcePresentation) -> Vec<FrameRange> {
    source
        .member_overrides()
        .into_iter()
        .enumerate()
        .map(|(i, ov)| ov.unwrap_or_else(|| source.effective_frame_range(i)))
        .collect()
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
    let before = shown_ranges(source);
    let visible: Vec<usize> = (0..source.objects.len())
        .filter(|&i| before[i].contains(current))
        .collect();

    // Make room for the new frame; spanning objects extend across it.
    insert_blank_frame(source, current);
    let anims = AnimSpans::of(source);
    let shown = shown_ranges(source);

    // Clone every visible object the blank insert did NOT carry onto the new
    // frame, so the new frame gets its own independent copies.
    let mut index_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &i in &visible {
        // An `Animation` sidecar is a deck-wide behavior, not frame content —
        // duplicating it onto one frame would just orphan an inert animation. A
        // `Loop` is a span behavior too: a clone would be a one-frame loop.
        if matches!(source.objects[i], SceneObject::Animation(_) | SceneObject::Loop(_)) {
            continue;
        }
        if shown[i].contains(new_frame) {
            continue; // already shared onto the new frame (kept continuous)
        }
        let mut clone = source.objects[i].clone();
        // A duplicated frame is a static snapshot: flatten any animated coordinate
        // to the value it shows on the source frame, so the copy doesn't ride an
        // animation it no longer belongs to.
        flatten_coordinates(&mut clone, current, &anims);
        keep_morph_blend(&mut clone, current, current + 1);
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

    repoint_cloned_groups(source, &index_map, |m| shown.get(m).is_some_and(|r| r.contains(new_frame)));
}

/// A cloned or split morph whose range will show the frames `[s, e)` of the
/// original keeps that part of its blend ([`Morph::keep_blend`]) instead of
/// replaying the whole blend over its new range. Other objects are untouched.
fn keep_morph_blend(obj: &mut SceneObject, s: usize, e: usize) {
    if let SceneObject::Morph(m) = obj {
        m.keep_blend(s, e);
    }
}

/// Insert `count` blank frames so they occupy indices `[dest, dest + count)`,
/// shifting every existing frame at or after `dest` up by `count`. Objects (and
/// animated-coordinate spans) that strictly *cross* `dest` are stretched to cover
/// the new frames — exactly as a single [`insert_blank_frame`] does for one frame
/// (this is its `count`-frame generalisation, used by [`copy_frames`]). `count ==
/// 0` is a no-op; `dest == 0` inserts at the very front.
fn insert_blank_frames_at(source: &mut SourcePresentation, dest: usize, count: usize) {
    if count == 0 {
        return;
    }
    source.frame_count += count;
    for obj in &mut source.objects {
        // Includes `Animation` objects, whose frame range *is* their span (the
        // single source of truth); driven coordinates carry no span to shift.
        if let Some(fr) = scene_object_frame_range_mut(obj) {
            if fr.end > dest {
                fr.end = fr.end.saturating_add(count);
            }
            if fr.start >= dest {
                fr.start = fr.start.saturating_add(count);
            }
        }
    }
}

/// Duplicate the contiguous frame block `[lo, hi]` (inclusive, 0-based) as a new
/// block placed immediately before (`before`) or after the `target` frame,
/// growing the deck by `count = hi - lo + 1` frames. Returns `(new_current,
/// count)`, where `new_current` is the first frame of the inserted copy.
///
/// The block's content is deep-cloned: an object local to one block frame lands
/// on the matching copy frame, an object spanning several block frames stays a
/// single spanning clone, and animated coordinates are re-timed to the block
/// ([`clip_coords_to_block`]) so the copied frames show what the originals did.
/// Objects that the insert stretches across the seam and that cover the whole
/// block (e.g. a deck-wide background) are *not* cloned, so they aren't
/// duplicated — such an object stays one object with its own timing, so if it
/// moves within the block the copied frames don't reproduce that motion (as
/// with [`insert_blank_frame`]). One straddling the seam without covering the
/// whole block (a bullet revealed mid-block) is split around the copy instead
/// ([`split_around_copy`], its pieces placed right after it in draw order), so
/// the copy shows it only where the block did; a
/// `Loop` stays stretched. A `Loop` the block cuts is not copied (it would be a
/// shorter loop); a wholly contained one is. An `Animation` (it draws nothing) is cloned
/// when wholly inside the block and off the seam, or when a clone drives it.
/// Cloned groups are re-pointed at their cloned members.
/// A no-op (returns `(target, 0)`) on an invalid block/target.
pub fn copy_frames(
    source: &mut SourcePresentation,
    lo: usize,
    hi: usize,
    target: usize,
    before: bool,
) -> (usize, usize) {
    let n = source.frame_count;
    if lo > hi || hi >= n || target >= n {
        return (target, 0);
    }
    if copy_frames_refusal(source, lo, hi, target, before).is_some() {
        return (target, 0);
    }
    let count = hi - lo + 1;
    // New block occupies `[dest, dest + count)` after the insert (same index in
    // old and new coords, since the insert shifts only frames at/after `dest`).
    let dest = if before { target } else { target + 1 };

    let anims = AnimSpans::of(source);
    // Capture clone specs from the *pre-insert* deck: (src_index, clone,
    // new_start, new_end, kept_only_if_driven).
    let mut specs: Vec<(usize, SceneObject, usize, usize, bool)> = Vec::new();
    // Objects the insert would stretch over copied frames they are not shown on.
    let mut splits: Vec<usize> = Vec::new();
    let overrides = source.member_overrides();
    for i in 0..source.objects.len() {
        // A member of an explicit group goes by the group's range, so it is
        // cloned exactly when its group is (see `shown_ranges`).
        let (a, b) = match (&overrides[i], scene_object_frame_range(&source.objects[i])) {
            (Some(fr), _) | (None, Some(fr)) => (fr.start, fr.end),
            (None, None) => continue, // auto group: its members carry the range
        };
        let is_anim = matches!(source.objects[i], SceneObject::Animation(_));
        // The insert stretches an object straddling the seam over every new
        // frame. Right when it covers the whole block (a background: then it is
        // not cloned, or it would be duplicated); otherwise it is split around
        // the copy below. A `Loop`/`Animation` is a span behaviour and stays
        // stretched — an `Animation` is cloned for now (dropped below unless a
        // clone uses it).
        let straddles = a < dest && b > dest;
        let span = is_anim || matches!(source.objects[i], SceneObject::Loop(_));
        let split = straddles && !span && !(a <= lo && b > hi);
        if split {
            splits.push(i);
        }
        let ov_start = a.max(lo);
        let ov_end = b.min(hi + 1);
        if ov_start >= ov_end {
            continue; // doesn't touch the block
        }
        if is_anim && cut_to_one_frame(a, b, lo, hi) {
            continue; // its drivers are flattened by `clip_coords_to_block`
        }
        if matches!(source.objects[i], SceneObject::Loop(_)) && (a < lo || b > hi + 1) {
            continue; // a loop the block cuts is not copied as a shorter loop
        }
        if straddles && !is_anim && !split {
            continue;
        }
        let new_start = ov_start - lo + dest;
        let new_end = ov_end - lo + dest;
        // An `Animation` clone's frame range becomes the copied block's span
        // (set below from `new_start/new_end`), and its id is freshly assigned
        // afterwards so the copy is an independent animation.
        let mut clone = source.objects[i].clone();
        clip_coords_to_block(&mut clone, lo, hi, &anims);
        keep_morph_blend(&mut clone, ov_start, ov_end);
        let cut = (ov_start, ov_end) != (a, b);
        specs.push((i, clone, new_start, new_end, straddles || cut));
    }
    // Drop a seam-straddling or cut `Animation` that no clone drives.
    let used: std::collections::HashSet<AnimId> = specs
        .iter_mut()
        .flat_map(|(_, c, ..)| scene_object_coordinates_mut(c))
        .filter_map(|c| match c {
            Coordinate::Animated { anim, .. } => Some(*anim),
            _ => None,
        })
        .collect();
    specs.retain(|(_, c, .., if_driven)| match c {
        SceneObject::Animation(an) => !if_driven || used.contains(&an.id),
        _ => true,
    });

    insert_blank_frames_at(source, dest, count);

    // Append the clones with their mapped ranges, then re-point cloned groups.
    let mut index_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let first_new = source.objects.len();
    for (src, mut clone, ns, ne, _) in specs {
        if let Some(fr) = scene_object_frame_range_mut(&mut clone) {
            fr.start = ns;
            fr.end = ne;
        }
        let new_index = source.objects.len();
        source.objects.push(clone);
        index_map.insert(src, new_index);
    }
    // An auto group has no range of its own to copy by: clone it when any of
    // its members was cloned (repeatedly, so an auto group holding a cloned
    // auto group follows), so the copied slides keep their grouping. It stays
    // auto; its clone keeps the cloned members and any shared one shown on the
    // copied frames (a background stretched over them), never one elsewhere.
    loop {
        let more: Vec<usize> = (0..first_new)
            .filter(|&i| {
                !index_map.contains_key(&i)
                    && matches!(&source.objects[i], SceneObject::Group(g)
                        if g.frames.is_none() && g.members.iter().any(|m| index_map.contains_key(m)))
            })
            .collect();
        if more.is_empty() {
            break;
        }
        for i in more {
            let new_index = source.objects.len();
            source.objects.push(source.objects[i].clone());
            index_map.insert(i, new_index);
        }
    }
    // A split object (cut back to end at `dest` below) is not shared onto the
    // copy even though the insert stretched it there for now.
    let shown = shown_ranges(source);
    repoint_cloned_groups(source, &index_map, |m| {
        !splits.contains(&m) && shown.get(m).is_some_and(|r| r.start < dest + count && r.end > dest)
    });

    // Give each cloned `Animation` a fresh id and repoint the cloned coordinates
    // that referenced the original, so the copied block animates independently
    // (the copy must not share — or collide on — the source's animation id).
    // `clip_coords_to_block` already flattened every coordinate whose animation
    // was not cloned.
    let mut id_map: std::collections::HashMap<AnimId, AnimId> = std::collections::HashMap::new();
    for idx in first_new..source.objects.len() {
        if matches!(source.objects[idx], SceneObject::Animation(_)) {
            // `next_anim_id` sees the ids assigned on previous iterations, so each
            // cloned animation gets a distinct fresh id.
            let new = next_anim_id(source);
            if let SceneObject::Animation(a) = &mut source.objects[idx] {
                id_map.insert(a.id, new);
                a.id = new;
            }
        }
    }
    if !id_map.is_empty() {
        for idx in first_new..source.objects.len() {
            for coord in scene_object_coordinates_mut(&mut source.objects[idx]) {
                if let Coordinate::Animated { anim, .. } = coord {
                    if let Some(&new) = id_map.get(anim) {
                        *anim = new;
                    }
                }
            }
        }
    }

    unstretch_unsplit_members(source, &overrides, &splits, dest, count);
    if splits.is_empty() {
        return (dest, count);
    }
    let cont_map = split_around_copy(source, &splits, &overrides, dest, count, first_new);
    // A split object's pieces were appended; move them right after it so it
    // keeps its draw position among equal-`z_order` objects on every frame.
    let mut followers: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for &k in &splits {
        followers.insert(k, index_map.get(&k).into_iter().chain(cont_map.get(&k)).copied().collect());
    }
    place_after(source, &followers);
    (dest, count)
}

/// Undo the insert's stretch of an explicit-group member's own range when its
/// group was not split: the group shows it on one side of the insert point
/// only (its range, `overrides[i]` before the insert, ends at or starts after
/// `dest`), so the member keeps just that side of its own range — a Morph's
/// own range is its blend timing, windowed with `keep_blend` so the slides the
/// group shows keep their blend stages. A group covering the insert point is
/// either split (see [`split_around_copy`]) or a background stretched over the
/// copy, and is left alone, as are spans (`Loop`/`Animation`/`AutoAdvance`).
fn unstretch_unsplit_members(
    source: &mut SourcePresentation,
    overrides: &[Option<FrameRange>],
    splits: &[usize],
    dest: usize,
    count: usize,
) {
    for (i, ov) in overrides.iter().enumerate() {
        let Some(group) = ov else { continue };
        let span = matches!(
            source.objects[i],
            SceneObject::Loop(_) | SceneObject::Animation(_) | SceneObject::AutoAdvance(_)
        );
        if span || splits.contains(&i) {
            continue;
        }
        let Some(own) = scene_object_frame_range(&source.objects[i]).cloned() else { continue };
        if !(own.start < dest && own.end > dest) {
            continue; // the insert only shifted it, or left it alone
        }
        let pre_end = own.end - count;
        let (keep, new) = if group.end <= dest {
            ((own.start, dest), FrameRange { start: own.start, end: dest })
        } else if group.start >= dest {
            ((dest, pre_end), FrameRange { start: dest + count, end: own.end })
        } else {
            continue;
        };
        if let SceneObject::Morph(m) = &mut source.objects[i] {
            m.frames.end = pre_end;
            m.keep_blend(keep.0, keep.1);
        }
        if let Some(fr) = scene_object_frame_range_mut(&mut source.objects[i]) {
            *fr = new;
        }
    }
}

/// Move each object listed in `followers[k]` to sit right after object `k`, in
/// one [`reorder_objects`] pass, so split pieces keep their original's draw
/// position among equal-`z_order` objects.
fn place_after(source: &mut SourcePresentation, followers: &std::collections::HashMap<usize, Vec<usize>>) {
    let moved: std::collections::HashSet<usize> = followers.values().flatten().copied().collect();
    let mut order = Vec::with_capacity(source.objects.len());
    for k in 0..source.objects.len() {
        if !moved.contains(&k) {
            order.push(k);
            order.extend(followers.get(&k).into_iter().flatten().copied());
        }
    }
    reorder_objects(source, &order);
}

/// Rearrange `source.objects` so position `j` holds the old object `order[j]`
/// (`order` is a permutation), re-pointing `Group.members` and `links`. A stale
/// out-of-range index (nothing validates them on load) is left as it is.
fn reorder_objects(source: &mut SourcePresentation, order: &[usize]) {
    let mut new_pos = vec![0; order.len()];
    for (j, &old) in order.iter().enumerate() {
        new_pos[old] = j;
    }
    let mut old: Vec<Option<SceneObject>> = std::mem::take(&mut source.objects).into_iter().map(Some).collect();
    source.objects = order.iter().map(|&o| old[o].take().expect("order is a permutation")).collect();
    for obj in &mut source.objects {
        if let SceneObject::Group(g) = obj {
            for m in &mut g.members {
                *m = new_pos.get(*m).copied().unwrap_or(*m);
            }
        }
    }
    for fam in &mut source.links {
        for m in fam.iter_mut() {
            *m = new_pos.get(*m).copied().unwrap_or(*m);
        }
    }
}

/// Split each object in `splits` — stretched by the insert over the copied
/// block `[dest, dest + count)` though the block's frames don't all show it —
/// so the copy shows only the clones [`copy_frames`] made: the original ends at
/// `dest` and an unchanged clone carries on after the copy. The continuation
/// joins the original's link family (or a new one with it) and every original
/// (index below `first_new`) auto group holding it; an explicit group's
/// continuation holds its members' continuations. An explicit-group member's
/// group decides where it shows, but its own range is split the same way when
/// the insert stretched it (it crossed `dest`): a member Morph's own range is
/// its blend timing, and a group reverted to auto shows each piece only on its
/// own slides. Returns the original -> continuation index map.
fn split_around_copy(
    source: &mut SourcePresentation,
    splits: &[usize],
    overrides: &[Option<FrameRange>],
    dest: usize,
    count: usize,
    first_new: usize,
) -> std::collections::HashMap<usize, usize> {
    let mut cont_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &i in splits {
        let mut cont = source.objects[i].clone();
        // Did the insert stretch this object's own range (it crossed `dest`)?
        // Always so for an ungrouped split; for an explicit-group member only
        // when its own range crossed the insert point too.
        let stretched = scene_object_frame_range(&source.objects[i])
            .is_some_and(|r| r.start < dest && r.end > dest);
        if overrides[i].is_none() || stretched {
            // A morph keeps its blend: the insert stretched it to end `count`
            // frames late, so window it in its pre-insert range — the head
            // shows its frames before `dest`, the continuation those from it.
            for (obj, head) in [(&mut cont, false), (&mut source.objects[i], true)] {
                if let SceneObject::Morph(m) = obj {
                    m.frames.end -= count;
                    let (a, b) = (m.frames.start, m.frames.end);
                    if head { m.keep_blend(a, dest) } else { m.keep_blend(dest, b) }
                    m.frames.end += count;
                }
            }
            if let Some(fr) = scene_object_frame_range_mut(&mut cont) {
                fr.start = dest + count;
            }
            if let Some(fr) = scene_object_frame_range_mut(&mut source.objects[i]) {
                fr.end = dest;
            }
        }
        let idx = source.objects.len();
        source.objects.push(cont);
        cont_map.insert(i, idx);
        match source.links.iter_mut().find(|fam| fam.contains(&i)) {
            Some(fam) => fam.push(idx),
            None => source.links.push(vec![i, idx]),
        }
    }
    repoint_cloned_groups(source, &cont_map, |_| true);
    for obj in &mut source.objects[..first_new] {
        if let SceneObject::Group(g) = obj {
            if g.frames.is_none() {
                let conts: Vec<usize> = g.members.iter().filter_map(|m| cont_map.get(m).copied()).collect();
                g.members.extend(conts);
            }
        }
    }
    cont_map
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
/// cloned members (see [`repoint_cloned_groups`] for a skipped, shared member).
///
/// Returns the number of objects pasted. A no-op (returns 0) when `from == onto`.
pub fn overlay_frame(source: &mut SourcePresentation, from: usize, onto: usize) -> usize {
    if from == onto {
        return 0;
    }
    // Objects visible on the source frame are the paste candidates.
    let shown = shown_ranges(source);
    let visible: Vec<usize> = (0..source.objects.len())
        .filter(|&i| shown[i].contains(from))
        .collect();
    let anims = AnimSpans::of(source);

    let mut index_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &i in &visible {
        // An `Animation` sidecar, a `Loop` or an `AutoAdvance` marker is a span
        // behavior, not frame content — don't overlay it onto an unrelated
        // frame (a cloned loop would be a one-frame loop, possibly nested in
        // another; a cloned marker would change the target slide's timing).
        if matches!(
            source.objects[i],
            SceneObject::Animation(_) | SceneObject::Loop(_) | SceneObject::AutoAdvance(_)
        ) {
            continue;
        }
        if shown[i].contains(onto) {
            continue; // already present on the target frame — don't duplicate it
        }
        let mut clone = source.objects[i].clone();
        // The overlay is a static snapshot of the source frame; flatten any
        // animated coordinate to the value it shows there.
        flatten_coordinates(&mut clone, from, &anims);
        keep_morph_blend(&mut clone, from, from + 1);
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

    repoint_cloned_groups(source, &index_map, |m| {
        shown.get(m).is_some_and(|r| r.contains(from) && r.contains(onto))
    });

    index_map.len()
}

/// A self-contained, deck-independent capture of a contiguous block of frames,
/// ready to paste into *any* open deck (see [`copy_frame_block`] /
/// [`paste_frame_block`]). The capture is normalised so it carries no references
/// into its source deck:
///
/// * object frame ranges are in the block's own `0..frame_count` coordinates;
/// * `Group.members` are block-local indices (members outside the block dropped);
/// * `Animation` ids are block-local (reassigned fresh on paste);
/// * an `Animation` cut by the block is clipped to it and its driven
///   coordinates re-timed, and any animated coordinate whose `Animation` does
///   not travel with the block is flattened to a `Fixed`
///   ([`clip_coords_to_block`]), so the clipboard never holds a dangling
///   animation reference.
///
/// `width`/`height` are the source canvas size, so a paste can warn on a
/// mismatch.
#[derive(Debug, Clone)]
pub struct FrameClipboard {
    pub width: u16,
    pub height: u16,
    pub frame_count: usize,
    pub objects: Vec<SceneObject>,
}

/// Whether an animation span `[a, b)` only partly overlaps the block `[lo, hi]`
/// and that overlap is a single frame — a clipped copy of it would have no
/// motion left, so its drivers are flattened and it isn't copied.
fn cut_to_one_frame(a: usize, b: usize, lo: usize, hi: usize) -> bool {
    let (s, e) = (a.max(lo), b.min(hi + 1));
    (s, e) != (a, b) && e <= s + 1
}

/// Re-time `obj`'s animated coordinates for a copy of the frame block `[lo, hi]`
/// whose animations are clipped to the block, so each copied frame shows what the
/// original frame did. A coordinate whose animation misses the block holds one
/// value there and is flattened to it, as is one whose clipped span is a single
/// frame ([`cut_to_one_frame`]). One whose animation is cut by the block gets
/// `from`/`to` = its values on the first/last overlapping frame, so frames in
/// between may round to a neighbouring cell. A wholly contained one is unchanged.
fn clip_coords_to_block(obj: &mut SceneObject, lo: usize, hi: usize, anims: &AnimSpans) {
    for coord in scene_object_coordinates_mut(obj) {
        let Coordinate::Animated { anim, .. } = *coord else {
            continue;
        };
        let (a, b) = anims.span(anim).map_or((0, 0), |r| (r.start, r.end));
        let (s, e) = (a.max(lo), b.min(hi + 1));
        if s >= e || cut_to_one_frame(a, b, lo, hi) {
            *coord = Coordinate::Fixed(coord.evaluate(s.min(hi), anims) as f64);
        } else if (s, e) != (a, b) {
            let (from, to) = (coord.evaluate(s, anims), coord.evaluate(e - 1, anims));
            *coord = Coordinate::Animated { from, to, anim };
        }
    }
}

/// Capture the contiguous frame block `[lo, hi]` (inclusive, 0-based) of `source`
/// into a self-contained [`FrameClipboard`] (see its docs for the normalisation).
/// Unlike [`copy_frames`] (which duplicates within one deck and relies on the
/// deck's own spanning objects being stretched across the seam), this captures
/// **every** object overlapping the block — the destination deck has nothing of
/// its own to reuse. A no-op-ish empty clipboard results from an invalid block.
pub fn copy_frame_block(source: &SourcePresentation, lo: usize, hi: usize) -> FrameClipboard {
    if lo > hi || hi >= source.frame_count {
        return FrameClipboard {
            width: source.width,
            height: source.height,
            frame_count: 0,
            objects: Vec::new(),
        };
    }
    let count = hi - lo + 1;
    let anims = AnimSpans::of(source);

    // Objects overlapping the block by the range they are shown on
    // (`shown_ranges`): an auto group by its members' union, a member of an
    // explicit group by the group's range. Bar an `Animation` cut to one frame,
    // whose drivers `clip_coords_to_block` flattens, and a `Loop` the block cuts
    // (it would paste as a shorter loop).
    let shown = shown_ranges(source);
    let captured: Vec<usize> = (0..source.objects.len())
        .filter(|&i| {
            let fr = &shown[i];
            let cut = match source.objects[i] {
                SceneObject::Animation(_) => cut_to_one_frame(fr.start, fr.end, lo, hi),
                SceneObject::Loop(_) => fr.start < lo || fr.end > hi + 1,
                _ => false,
            };
            fr.start.max(lo) < fr.end.min(hi + 1) && !cut
        })
        .collect();

    // Block-local index map (old object index -> position within the capture).
    let pos: std::collections::HashMap<usize, usize> = captured
        .iter()
        .enumerate()
        .map(|(local, &src)| (src, local))
        .collect();

    let mut objects: Vec<SceneObject> = Vec::with_capacity(captured.len());
    for &i in &captured {
        let mut clone = source.objects[i].clone();
        // Re-time animated coordinates to the clipped animations; flatten those
        // driven by an animation that doesn't travel with the block.
        clip_coords_to_block(&mut clone, lo, hi, &anims);
        keep_morph_blend(&mut clone, shown[i].start.max(lo), shown[i].end.min(hi + 1));
        // Normalise the stored range to block-local `0..count` (clipped to block);
        // an overridden member takes its group's range, which overlaps the block.
        if let Some(fr) = scene_object_frame_range_mut(&mut clone) {
            fr.start = shown[i].start.max(lo) - lo;
            fr.end = shown[i].end.min(hi + 1) - lo;
        }
        // Remap group members to block-local; drop members outside the block.
        if let SceneObject::Group(g) = &mut clone {
            g.members = g.members.iter().filter_map(|m| pos.get(m).copied()).collect();
        }
        objects.push(clone);
    }

    FrameClipboard {
        width: source.width,
        height: source.height,
        frame_count: count,
        objects,
    }
}

/// Paste a [`FrameClipboard`] into `target` as a new block of frames placed
/// immediately before (`before`) or after the `target_frame`, growing the deck by
/// `clip.frame_count` frames. Returns `(new_current, count)`, where `new_current`
/// is the first frame of the pasted block.
///
/// The clipboard is deck-independent, so this is the cross-deck counterpart of
/// the tail of [`copy_frames`]: it inserts blank frames, appends the clones with
/// their block-local ranges shifted into the destination, re-points cloned
/// `Group.members` from block-local to the new absolute indices, and — the key
/// correctness step — gives each cloned `Animation` a **fresh id** in the target
/// (remapping the driven coordinates) so a pasted animation can never collide
/// with or alias one already in the destination deck.
///
/// The pasted slides show only what their source slides showed: a target
/// object the insert would stretch over them (crossing the paste point, by its
/// shown range) is split around the block by [`split_around_copy`], its
/// continuation placed right after it (shifting later object indices). Only
/// `Loop`/`Animation` spans keep stretching; an `AutoAdvance` marker is split
/// too, so the pasted slides keep their own timing.
pub fn paste_frame_block(
    target: &mut SourcePresentation,
    clip: &FrameClipboard,
    target_frame: usize,
    before: bool,
) -> (usize, usize) {
    if clip.frame_count == 0 || paste_frame_block_refusal(target, clip, target_frame, before).is_some() {
        return (paste_dest(target, target_frame, before).0, 0);
    }
    let count = clip.frame_count;
    let (_, dest) = paste_dest(target, target_frame, before);

    // The clipboard brings everything its slides showed, so a target object the
    // insert would stretch over the pasted frames is split around them instead
    // (as `copy_frames` does): it never appears on the foreign slides — an
    // `AutoAdvance` marker included, so the pasted slides keep the timing they
    // came with. Span behaviours (`Loop`, `Animation`) keep stretching.
    let overrides = target.member_overrides();
    let splits: Vec<usize> = (0..target.objects.len())
        .filter(|&i| {
            let range = overrides[i].clone().or_else(|| scene_object_frame_range(&target.objects[i]).cloned());
            let span = matches!(target.objects[i], SceneObject::Loop(_) | SceneObject::Animation(_));
            !span && range.is_some_and(|r| r.start < dest && dest < r.end)
        })
        .collect();

    insert_blank_frames_at(target, dest, count);

    // Append the clones, shifting block-local ranges and group members into the
    // destination. `first_new` is where the clipboard's object 0 lands.
    let first_new = target.objects.len();
    let mut clones = clip.objects.clone();
    for clone in &mut clones {
        if let Some(fr) = scene_object_frame_range_mut(clone) {
            fr.start = fr.start.saturating_add(dest);
            fr.end = fr.end.saturating_add(dest);
        }
        if let SceneObject::Group(g) = clone {
            for m in &mut g.members {
                *m += first_new;
            }
        }
    }
    target.objects.extend(clones);

    // Fresh animation ids in the target; remap the cloned coordinates that
    // referenced them so the pasted block animates independently.
    let mut id_map: std::collections::HashMap<AnimId, AnimId> = std::collections::HashMap::new();
    for idx in first_new..target.objects.len() {
        if matches!(target.objects[idx], SceneObject::Animation(_)) {
            // `next_anim_id` sees ids assigned on previous iterations, so each
            // cloned animation gets a distinct fresh id.
            let new = next_anim_id(target);
            if let SceneObject::Animation(a) = &mut target.objects[idx] {
                id_map.insert(a.id, new);
                a.id = new;
            }
        }
    }
    if !id_map.is_empty() {
        for idx in first_new..target.objects.len() {
            for coord in scene_object_coordinates_mut(&mut target.objects[idx]) {
                if let Coordinate::Animated { anim, .. } = coord {
                    if let Some(&new) = id_map.get(anim) {
                        *anim = new;
                    }
                }
            }
        }
    }

    unstretch_unsplit_members(target, &overrides, &splits, dest, count);
    if !splits.is_empty() {
        let cont_map = split_around_copy(target, &splits, &overrides, dest, count, first_new);
        let followers = cont_map.into_iter().map(|(k, c)| (k, vec![c])).collect();
        place_after(target, &followers);
    }
    (dest, count)
}

/// Give a new animation spanning `[start, end_frame]` (inclusive last animated
/// frame, started from the `current` frame) its own dedicated frames: insert
/// `N - 1` **fresh** blank frames immediately after `current` (where `N` is the
/// span length), then extend every object visible on `current` to span the whole
/// range. The elements therefore become a single shared object across all the
/// animation's frames — editing one edits them all.
///
/// The span must start on `current` (`start == current`); the animate and
/// converge menus refuse an inserting span that starts elsewhere
/// (`input::check_anim_span`), since the new frames would not be the span's.
///
/// The new frames are always inserted, not reused — any existing content after
/// `current` shifts back to make room (`insert_blank_frame` already extends
/// deck-wide/spanning objects across them, keeping each a single object). The
/// caller is responsible for *not* calling this twice for the same animation
/// (re-applying X+Y over one span, or re-saving), which would insert again — see
/// the span guard in `input::apply_animation`.
///
/// An object an *earlier* animation drives keeps showing on the new frames,
/// holding its end position there: its range reaches past its animations'
/// spans, which [`restrobe`] preserves as a hold. A gap-strobed element is not
/// stretched (it shows on its samples only; a stretched sample would be a copy
/// no re-strobe clears) unless it is in `animating`, the objects the new
/// animation is about to drive, which re-lock over the new span themselves.
///
/// A morph ending on `current` gets a frozen continuation placed right after
/// it in the object list (so it keeps the morph's draw order among equal
/// `z_order`s). Returns where each object that existed before the call now
/// sits (`moved[old] = new`), for callers holding object indices.
pub fn add_frames_and_share(
    source: &mut SourcePresentation,
    current: usize,
    start: usize,
    end_frame: usize,
    animating: &[usize],
) -> Vec<usize> {
    let before = source.objects.len();
    let mut continuations: Vec<(usize, usize)> = Vec::new();
    let end_excl = end_frame + 1;
    // Elements to share are those visible on the current frame *before* growth.
    // An earlier `Animation` ending here is a span, not frame content:
    // stretching it would re-time every coordinate it drives on its
    // already-authored slides. Its driven objects still extend, holding their
    // `to` value past the span (a hold, see above; not a gap strobe). An
    // `AutoAdvance` marker stays on its own frames too. A `Loop` covering this frame does grow over the new frames: it must
    // contain the new animation wholly (`validate_loops`), and it already held
    // the earlier one.
    let visible: Vec<usize> = (0..source.objects.len())
        .filter(|&i| {
            !matches!(source.objects[i], SceneObject::Animation(_) | SceneObject::AutoAdvance(_))
                && source.effective_frame_range(i).contains(current)
        })
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
        let obj = &source.objects[i];
        if !animating.contains(&i) && is_animated(obj) && has_gap_strobe(source, obj) {
            continue;
        }
        // A morph's range is also its blend timing: stretching one that ends
        // here would re-time what every one of its slides shows. Keep it and
        // hold its finished state on the new frames with a frozen continuation,
        // also frozen in place (its coordinates flattened), so no animation
        // drives it and no later re-lock stretches it back over the morph.
        // (A morph the new animation drives is re-locked to the new span
        // like any animated object.)
        if let SceneObject::Morph(m) = obj {
            if !animating.contains(&i) && m.frames.end == current + 1 && end_excl > current + 1 {
                let mut cont = m.frozen_at(current);
                cont.frames = FrameRange { start: current + 1, end: end_excl };
                let mut cont = SceneObject::Morph(cont);
                flatten_coordinates(&mut cont, current, &AnimSpans::of(source));
                let SceneObject::Morph(cont) = cont else { unreachable!() };
                let idx = source.objects.len();
                source.objects.push(SceneObject::Morph(cont));
                for obj in &mut source.objects[..idx] {
                    if let SceneObject::Group(g) = obj {
                        if g.frames.is_none() && g.members.contains(&i) {
                            g.members.push(idx);
                        }
                    }
                }
                continuations.push((i, idx));
                continue;
            }
        }
        if let Some(fr) = scene_object_frame_range_mut(&mut source.objects[i]) {
            if fr.start > lo {
                fr.start = lo;
            }
            if fr.end < end_excl {
                fr.end = end_excl;
            }
        }
    }
    // Move each continuation right after its morph (draw order).
    let mut order: Vec<usize> = Vec::with_capacity(source.objects.len());
    for k in 0..before {
        order.push(k);
        order.extend(continuations.iter().filter(|&&(o, _)| o == k).map(|&(_, c)| c));
    }
    let mut moved = vec![0; before];
    for (j, &k) in order.iter().enumerate() {
        if k < before {
            moved[k] = j;
        }
    }
    if !continuations.is_empty() {
        reorder_objects(source, &order);
    }
    moved
}

/// Turn the just-animated element at `object_index` into a stop-motion strobe:
/// `gap` is the number of *empty* frames left between appearances, so the element
/// shows every `gap + 1` frames of its span — at `start`, `start + gap + 1`,
/// `start + 2·(gap + 1)`, … up to `end_frame` (e.g. `gap = 3` ⇒ frames `start`,
/// `start + 4`, `start + 8`, with three blank frames between each). The original
/// is kept on the first sample frame; single-frame clones are added on the later
/// sample frames. Every copy keeps the animated coordinate, so each evaluates to
/// the element's interpolated position for its own frame — the motion is sampled,
/// not held.
///
/// `gap == 0` is a no-op (no empty frames ⇒ the element stays on every frame).
/// Call this only on a freshly created animation; the clones are independent
/// objects, so re-applying over the same span would stack duplicates. To strobe
/// an element by all its animations at once, use [`restrobe`].
pub fn apply_gap(
    source: &mut SourcePresentation,
    object_index: usize,
    start: usize,
    end_frame: usize,
    gap: usize,
) {
    if gap == 0 {
        return;
    }
    // `gap` is the number of *empty* frames between appearances, so the element
    // appears every `gap + 1` frames: at `start`, `start + gap + 1`, … (saturating,
    // so a huge gap just leaves the first sample). Clipped to the deck, so an
    // oversized span can't strobe copies onto frames that don't exist.
    let end = end_frame.saturating_add(1).min(source.frame_count);
    let frames: std::collections::BTreeSet<usize> =
        (start..end).step_by(gap.saturating_add(1)).collect();
    place_on_frames(source, object_index, &frames);
}

/// Show the element at `object_index` on exactly `frames`: the original takes
/// the first contiguous run, and a clone (identical but for its range) is
/// appended for each further run. A no-op for an empty set.
fn place_on_frames(
    source: &mut SourcePresentation,
    object_index: usize,
    frames: &std::collections::BTreeSet<usize>,
) {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &f in frames {
        match runs.last_mut() {
            Some(r) if r.1 == f => r.1 = f + 1,
            _ => runs.push((f, f + 1)),
        }
    }
    let set = |obj: &mut SceneObject, (start, end): (usize, usize)| {
        if let Some(fr) = scene_object_frame_range_mut(obj) {
            fr.start = start;
            fr.end = end;
        }
    };
    let Some((&first, rest)) = runs.split_first() else {
        return;
    };
    set(&mut source.objects[object_index], first);
    for &run in rest {
        let mut clone = source.objects[object_index].clone();
        set(&mut clone, run);
        source.objects.push(clone);
    }
}

/// The `gap_frames` of the animation with id `id` (0 when missing).
fn anim_gap(source: &SourcePresentation, id: AnimId) -> usize {
    source
        .objects
        .iter()
        .find_map(|o| match o {
            SceneObject::Animation(a) if a.id == id => Some(a.gap_frames),
            _ => None,
        })
        .unwrap_or(0)
}

/// Whether any animation driving `obj` is gap-strobed.
fn has_gap_strobe(source: &SourcePresentation, obj: &SceneObject) -> bool {
    referenced_anim_ids(obj).into_iter().any(|id| anim_gap(source, id) > 0)
}

/// Re-derive where the element at `object_index` shows from the animations
/// driving it, each on its own span: a gapped animation contributes its sample
/// frames (every `gap + 1`), any other its whole span. Without a gapped one this
/// just locks the range to the union of the spans; with one, the element is
/// strobed over that frame set ([`place_on_frames`]). Call it on an original
/// whose old strobe copies were cleared; it only appends, so indices stay valid.
///
/// A *hold* is kept: when the range reached past the spans' end in `old` (the
/// spans before the edit that prompted this call), the element was holding its
/// end position on later frames (added frames, see [`add_frames_and_share`]),
/// so the range still ends there unless the new spans reach further.
pub fn restrobe(source: &mut SourcePresentation, object_index: usize, old: &AnimSpans) {
    let anims = AnimSpans::of(source);
    let obj = &source.objects[object_index];
    let Some((lo, hi)) = scene_object_animation_span(obj, &anims) else {
        return;
    };
    let (lo_b, hi_b) = split_bounds(source, object_index);
    if !has_gap_strobe(source, obj) {
        let hold = scene_object_frame_range(obj)
            .zip(scene_object_animation_span(obj, old))
            .and_then(|(fr, (_, old_hi))| (fr.end > old_hi).then_some(fr.end));
        let (start, end) = (lo.max(lo_b), hi.max(hold.unwrap_or(0)).min(hi_b));
        if let Some(fr) = scene_object_frame_range_mut(&mut source.objects[object_index]) {
            if start < end {
                (fr.start, fr.end) = (start, end);
            }
        }
        return;
    }
    let mut frames = strobe_frames(source, obj, &anims);
    frames.retain(|&f| lo_b <= f && f < hi_b);
    place_on_frames(source, object_index, &frames);
}

/// Why the objects in `members` cannot be gap-strobed, if one of them is a
/// member of an explicit-range group: such a member is shown on the group's
/// range instead of its own (`member_overrides`), while a strobe works by
/// narrowing the element's own range to its samples — it would silently play
/// continuous motion. The Animate/Converge `[s]`, a gapped animation's span
/// edit and making a group's range explicit refuse with this message.
pub fn strobe_group_error(source: &SourcePresentation, members: &[usize]) -> Option<String> {
    let overrides = source.member_overrides();
    members
        .iter()
        .any(|&i| overrides.get(i).is_some_and(Option::is_some))
        .then(|| STROBE_GROUP_ERROR.to_string())
}

/// The message for every refusal [`strobe_group_error`] describes.
pub const STROBE_GROUP_ERROR: &str = "gap frames can't strobe a member of a group with its own \
    frame range (keep the group's first/last frame blank, or set gap frames to 0)";

/// Whether the object at `i` is gap-strobed (driven by an animation with
/// `gap_frames > 0`).
pub fn is_gap_strobed(source: &SourcePresentation, i: usize) -> bool {
    let obj = &source.objects[i];
    is_animated(obj) && has_gap_strobe(source, obj)
}

/// Whether `j` is another piece of the element at `i` that a frame copy split
/// around the inserted block ([`split_around_copy`]): a linked sibling with the
/// same content but for its range ([`gap_clone_key`]), driven by one of the
/// same animations. Such a piece is not a strobe copy. (A linked paste is not
/// a piece: a stamp is placed elsewhere, so its content differs.)
fn is_split_piece(source: &SourcePresentation, i: usize, j: usize) -> bool {
    let ids = referenced_anim_ids(&source.objects[i]);
    source.link_siblings(i).contains(&j)
        && referenced_anim_ids(&source.objects[j]).iter().any(|id| ids.contains(id))
        && gap_clone_key(&source.objects[i]) == gap_clone_key(&source.objects[j])
}

/// The frames a re-lock may give the object at `i` when it is one piece of a
/// split element: an edge facing another piece stays where it is (the copied
/// block between them must not be re-covered), only an outer edge follows the
/// spans. `(0, usize::MAX)` for an object that was never split.
fn split_bounds(source: &SourcePresentation, i: usize) -> (usize, usize) {
    let Some(own) = scene_object_frame_range(&source.objects[i]).cloned() else {
        return (0, usize::MAX);
    };
    let mut bounds = (0, usize::MAX);
    for j in source.link_siblings(i) {
        if j >= source.objects.len() || !is_split_piece(source, i, j) {
            continue;
        }
        match scene_object_frame_range(&source.objects[j]) {
            Some(r) if r.start < own.start => bounds.0 = own.start,
            Some(r) if r.start > own.start => bounds.1 = bounds.1.min(own.end),
            _ => {}
        }
    }
    bounds
}

/// The frames `obj` shows on by its animations: each gapped one's samples
/// (every `gap + 1` frames of its span), every other one's whole span. Spans are
/// clipped to the deck, so a hand-edited oversized span stays bounded.
fn strobe_frames(
    source: &SourcePresentation,
    obj: &SceneObject,
    anims: &AnimSpans,
) -> std::collections::BTreeSet<usize> {
    let mut frames = std::collections::BTreeSet::new();
    for id in referenced_anim_ids(obj) {
        if let Some(fr) = anims.span(id) {
            let end = fr.end.min(source.frame_count);
            frames.extend((fr.start..end).step_by(anim_gap(source, id).saturating_add(1)));
        }
    }
    frames
}

/// After animation `id`'s span changed (`old` = the spans before), re-strobe
/// every element it drives as a unit: clear each gapped original's copies over
/// its old span, then [`restrobe`] each original over the new spans. Returns the
/// removed indices in removal order (see [`index_after_removals`]).
pub fn restrobe_driven(source: &mut SourcePresentation, id: AnimId, old: &AnimSpans) -> Vec<usize> {
    let removed = clear_driven_gap_clones(source, id, old);
    restrobe_all_driven(source, id, None, old);
    removed
}

/// The first half of [`restrobe_driven`]: clear the strobe copies of every
/// gapped element animation `id` drives, over its spans in `old`. Run it while
/// the animations still carry their old gaps (before a gap change), so an
/// element whose gap becomes 0 still loses its copies. Returns the removed
/// indices in removal order (see [`index_after_removals`]).
pub fn clear_driven_gap_clones(source: &mut SourcePresentation, id: AnimId, old: &AnimSpans) -> Vec<usize> {
    let mut removed = Vec::new();
    // Copies sit above their original, so clearing from low to high only
    // removes indices above `i`.
    let mut i = 0;
    while i < source.objects.len() {
        let obj = &source.objects[i];
        if is_driven_by(obj, id) && has_gap_strobe(source, obj) {
            if let Some((lo, hi)) = scene_object_animation_span(obj, old) {
                removed.extend(clear_gap_clones(source, i, lo, hi.saturating_sub(1)));
            }
        }
        i += 1;
    }
    removed
}

/// The second half of [`restrobe_driven`]: [`restrobe`] every element animation
/// `id` drives over the current spans, except `except`, which the caller
/// re-strobes itself *after* this call. Every driven element must be copy-free
/// when this runs (its strobe copies cleared, e.g. by
/// [`clear_driven_gap_clones`]): the driven list is taken up front, so copies
/// appended here are never re-strobed, but an existing copy would be. It only
/// appends, so indices stay valid. `old` as for [`restrobe`].
pub fn restrobe_all_driven(
    source: &mut SourcePresentation,
    id: AnimId,
    except: Option<usize>,
    old: &AnimSpans,
) {
    let driven: Vec<usize> = (0..source.objects.len())
        .filter(|&i| Some(i) != except && is_driven_by(&source.objects[i], id))
        .collect();
    for i in driven {
        restrobe(source, i, old);
    }
}

/// True if `obj` has at least one `Animated` coordinate — only animated elements
/// are gap-strobed, so a non-animated object can have no strobe copies.
fn is_animated(obj: &SceneObject) -> bool {
    let mut clone = obj.clone();
    scene_object_coordinates_mut(&mut clone)
        .into_iter()
        .any(|c| matches!(c, Coordinate::Animated { .. }))
}

/// A content key that ignores only the frame range: serialize `obj` with its
/// range normalized to `[0, 1)`. Two objects with this same key are identical in
/// every field *except* which single frame they occupy — exactly the relationship
/// between a gap-strobe clone and its source (clones are deep copies of the
/// source object with only the frame moved). Matching on the whole object — not
/// just its animated coordinates — keeps strobe copies of *different* elements
/// distinct even when their motion (from/to/span) coincides, so editing one
/// overlapping animation never deletes another's gap frames.
pub fn gap_clone_key(obj: &SceneObject) -> String {
    let mut clone = obj.clone();
    if let Some(fr) = scene_object_frame_range_mut(&mut clone) {
        fr.start = 0;
        fr.end = 1;
    }
    serde_json::to_string(&clone).unwrap_or_default()
}

/// The gap-strobe set the object at `index` belongs to, ascending: every object
/// with its exact content but for the frame range ([`gap_clone_key`]) when it is
/// driven by a gapped animation, else just `index`. The first entry is the
/// original (copies are always appended after it). Editing treats the set as
/// one element: see [`sync_strobe_set`].
pub fn strobe_set(source: &SourcePresentation, index: usize) -> Vec<usize> {
    let obj = &source.objects[index];
    if !is_animated(obj) || !has_gap_strobe(source, obj) {
        return vec![index];
    }
    let key = gap_clone_key(obj);
    (0..source.objects.len())
        .filter(|&i| {
            i == index
                || (gap_clone_key(&source.objects[i]) == key && !is_split_piece(source, index, i))
        })
        .collect()
}

/// Copy the content of the object at `from` onto every other index in `set`,
/// keeping each one's own frame range — so an edit made to any copy of a
/// strobe set (on whatever sample frame it was selected) lands on the whole set
/// and the copies stay recognisable to [`clear_gap_clones`] / [`restrobe_driven`].
pub fn sync_strobe_set(source: &mut SourcePresentation, from: usize, set: &[usize]) {
    for &i in set {
        if i == from {
            continue;
        }
        let range = scene_object_frame_range(&source.objects[i]).cloned();
        let mut copy = source.objects[from].clone();
        if let (Some(fr), Some(range)) = (scene_object_frame_range_mut(&mut copy), range) {
            *fr = range;
        }
        source.objects[i] = copy;
    }
}

/// Remove the gap-strobe copies of the element at `keep` within `[start,
/// end_frame]`: objects inside that span (appended *after* `keep`; one per
/// strobe run, normally a single frame) that are exact content-copies of it
/// (same key per [`gap_clone_key`], i.e. identical but for their frames).
/// Makes re-applying or clearing a gapped animation idempotent — no orphan
/// copies left showing on their old frames — without touching the strobe copies
/// of a *different* overlapping animation that happens to share the same
/// motion. Returns the removed indices (all above `keep`, highest first) so a
/// caller holding other indices can remap them.
pub fn clear_gap_clones(
    source: &mut SourcePresentation,
    keep: usize,
    start: usize,
    end_frame: usize,
) -> Vec<usize> {
    let mut removed = Vec::new();
    if !is_animated(&source.objects[keep]) {
        return removed; // not animated → nothing could be a gap copy of it
    }
    let key = gap_clone_key(&source.objects[keep]);
    // Clones are always pushed after `keep`, so scanning indices above it keeps
    // `keep` (and lower indices) stable as matches are removed.
    let mut i = source.objects.len();
    while i > keep + 1 {
        i -= 1;
        let in_span = scene_object_frame_range(&source.objects[i])
            .is_some_and(|fr| fr.start < fr.end && fr.start >= start && fr.end <= end_frame + 1);
        if in_span && gap_clone_key(&source.objects[i]) == key && !is_split_piece(source, keep, i) {
            source.objects.remove(i);
            adjust_group_members_after_delete(source, i);
            removed.push(i);
        }
    }
    removed
}

/// Create or update the [`Animation`] with id `id`: set its span `[start,
/// end_excl)` and playback config. Appends it if no animation has that id yet.
/// The single place an animation's span is stored — driven coordinates link to
/// it by `id`, so x and y of the same object share one animation by *reference*.
pub fn ensure_animation(
    source: &mut SourcePresentation,
    id: AnimId,
    start: usize,
    end_excl: usize,
    auto_play: bool,
    delay_ms: u64,
    gap_frames: usize,
) {
    for obj in &mut source.objects {
        if let SceneObject::Animation(a) = obj {
            if a.id == id {
                a.frames = FrameRange { start, end: end_excl };
                a.auto_play = auto_play;
                a.delay_ms = delay_ms;
                a.gap_frames = gap_frames;
                return;
            }
        }
    }
    source.objects.push(SceneObject::Animation(Animation {
        id,
        frames: FrameRange { start, end: end_excl },
        auto_play,
        delay_ms,
        gap_frames,
    }));
}

/// Is `obj` driven by the animation with id `id`?
fn is_driven_by(obj: &SceneObject, id: AnimId) -> bool {
    referenced_anim_ids(obj).contains(&id)
}

/// The span `[start, end_excl)` of the animation with id `id`, if present.
fn animation_span_by_id(source: &SourcePresentation, id: AnimId) -> Option<(usize, usize)> {
    source.objects.iter().find_map(|o| match o {
        SceneObject::Animation(a) if a.id == id => Some((a.frames.start, a.frames.end)),
        _ => None,
    })
}

/// Flatten every coordinate on `obj` driven by animation `id` back to a static
/// `Fixed` at its `from` value (where the motion began), leaving coordinates on
/// *other* animations alone. Then widen the object's frame range to cover
/// `[start, end_excl)` (never shrinking) so a reverted — possibly gap-strobed —
/// element shows statically across those frames instead of vanishing.
fn flatten_animation(obj: &mut SceneObject, id: AnimId, start: usize, end_excl: usize) {
    for coord in scene_object_coordinates_mut(obj) {
        if let Coordinate::Animated { from, anim, .. } = coord {
            if *anim == id {
                let v = *from;
                *coord = Coordinate::Fixed(v as f64);
            }
        }
    }
    if let Some(fr) = scene_object_frame_range_mut(obj) {
        if fr.start > start {
            fr.start = start;
        }
        if fr.end < end_excl {
            fr.end = end_excl;
        }
    }
}

/// Remove the animation with id `id` — the *whole* animation, both halves: the
/// motion (`Coordinate::Animated` on every coordinate it drives) and the
/// `Animation` sidecar. Driven coordinates revert to a static `Fixed`, their
/// gap-strobe copies are dropped, and the sidecar object is deleted. This makes
/// the selectable `Animation` object a real handle: deleting it removes the
/// motion too, instead of leaving elements moving with no sidecar.
pub fn remove_animation(source: &mut SourcePresentation, id: AnimId) {
    let Some((start, end_excl)) = animation_span_by_id(source, id) else {
        return;
    };
    let anims = AnimSpans::of(source);
    // Clear strobe copies (over all of the element's animations) + flatten
    // originals. Scanning low→high keeps the current index stable: an original's
    // gap clones sit at higher indices, so removing them (and the higher-index
    // sidecar) never shifts what's behind us; a re-strobe only appends copies,
    // which no longer reference `id`.
    let mut i = 0;
    while i < source.objects.len() {
        if is_driven_by(&source.objects[i], id) {
            if let Some((lo, hi)) = scene_object_animation_span(&source.objects[i], &anims) {
                clear_gap_clones(source, i, lo, hi.saturating_sub(1));
            }
            flatten_animation(&mut source.objects[i], id, start, end_excl);
            // Another animation still drives it: its range stays locked to (and
            // strobed by) that animation, so every copy stays inside a span a
            // later edit clears over — the element remains one unit.
            restrobe(source, i, &anims);
        }
        i += 1;
    }
    if let Some(idx) = source
        .objects
        .iter()
        .position(|o| matches!(o, SceneObject::Animation(a) if a.id == id))
    {
        source.objects.remove(idx);
        adjust_group_members_after_delete(source, idx);
    }
}

/// Remove every `Animation` sidecar that no coordinate references. A self-healing
/// pass run after animation edits, so a span change or a zeroed-out motion can
/// never leave an orphaned, selectable-but-inert `Animation` behind (the bug the
/// single-source-of-truth model is meant to make impossible).
///
/// Returns the removed indices in removal order, each relative to the list as it
/// stood at that removal — feed them to [`index_after_removals`] to keep a held
/// object index on the same object.
pub fn prune_orphan_animations(source: &mut SourcePresentation) -> Vec<usize> {
    let mut removed = Vec::new();
    let ids: Vec<AnimId> = source
        .objects
        .iter()
        .filter_map(|o| match o {
            SceneObject::Animation(a) => Some(a.id),
            _ => None,
        })
        .collect();
    for id in ids {
        if source.objects.iter().any(|o| is_driven_by(o, id)) {
            continue;
        }
        if let Some(idx) = source
            .objects
            .iter()
            .position(|o| matches!(o, SceneObject::Animation(a) if a.id == id))
        {
            source.objects.remove(idx);
            adjust_group_members_after_delete(source, idx);
            removed.push(idx);
        }
    }
    removed
}

/// Where the object at `index` sits after the removals `removed` (in removal
/// order, as [`prune_orphan_animations`] returns them). `index` itself must not
/// be among them.
pub fn index_after_removals(index: usize, removed: &[usize]) -> usize {
    removed.iter().fold(index, |i, &r| if r < i { i - 1 } else { i })
}

/// Replace every `Animated` coordinate on `obj` with a `Fixed` value sampled at
/// `frame` (looking the span up in `anims`). Used when pasting/snapshotting: a
/// clone re-anchored to a single frame can't ride an animation, and an animated
/// coordinate is un-nudgeable (the arrow-key move only adjusts `Fixed`).
/// Flattening makes the copy a static, movable object showing its position at
/// `frame` — and drops its animation reference.
pub fn flatten_coordinates(obj: &mut SceneObject, frame: usize, anims: &AnimSpans) {
    for coord in scene_object_coordinates_mut(obj) {
        if matches!(coord, Coordinate::Animated { .. }) {
            *coord = Coordinate::Fixed(coord.evaluate(frame, anims) as f64);
        }
    }
}

/// Expand a selection so every selected `Group` also pulls in its members —
/// through nested groups, so an inner group keeps its own members (cycle-safe:
/// each index is visited once). A copied group is meaningless without the
/// objects it contains. Returns the selection plus those members,
/// de-duplicated, in ascending index order.
pub fn expand_selection(source: &SourcePresentation, indices: &[usize]) -> Vec<usize> {
    let mut set: Vec<usize> = indices.to_vec();
    let mut stack: Vec<usize> = indices.to_vec();
    while let Some(i) = stack.pop() {
        if let Some(SceneObject::Group(g)) = source.objects.get(i) {
            for &m in &g.members {
                // A member naming no object (unchecked file data) is skipped.
                if m < source.objects.len() && !set.contains(&m) {
                    set.push(m);
                    stack.push(m);
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
/// frame indices. Every object keeps showing on exactly the slides it did: a
/// range the move tears is split into one object per contiguous run (see
/// [`remap_ranges_through_pos`]; the UI first refuses a tear the split can't
/// express, [`move_frames_refusal`]).
pub fn move_frame(
    source: &mut SourcePresentation,
    from: usize,
    target: usize,
    before: bool,
) -> usize {
    move_frames(source, &[from], target, before)
}

/// Reorder the deck so the frames in `frames` (a set of 0-based indices) sit, as
/// one contiguous block in ascending order, immediately before (`before`) or
/// after the `target` frame, and return the new index of the block's first frame.
/// A generalisation of [`move_frame`]: the selected frames are pulled out and
/// re-inserted together next to `target`. No-op (returns the first selected
/// frame's index unchanged) when the deck has ≤1 frame, the selection is empty or
/// out of range, or `target` lies *within* the moved block.
pub fn move_frames(
    source: &mut SourcePresentation,
    frames: &[usize],
    target: usize,
    before: bool,
) -> usize {
    let first = frames.iter().copied().min().unwrap_or(0);
    let Some(pos) = move_permutation(source.frame_count, frames, target, before) else {
        return first;
    };
    remap_ranges_through_pos(source, &pos, source.frame_count);
    pos[first]
}

/// The frame permutation of [`move_frames`] (`pos[old_index] = new_index`), or
/// `None` for a no-op move.
fn move_permutation(n: usize, frames: &[usize], target: usize, before: bool) -> Option<Vec<usize>> {
    let mut sel: Vec<usize> = frames.to_vec();
    sel.sort_unstable();
    sel.dedup();
    if n <= 1 || sel.is_empty() || sel.iter().any(|&f| f >= n) || target >= n || sel.contains(&target) {
        return None;
    }
    // Build the new ordering: order[new_index] = old_index — every frame not in
    // the selection, with the selected block spliced in next to `target`.
    let mut order: Vec<usize> = (0..n).filter(|f| !sel.contains(f)).collect();
    let target_pos = order.iter().position(|&f| f == target).unwrap_or(0);
    let insert_at = if before { target_pos } else { target_pos + 1 };
    for (k, &f) in sel.iter().enumerate() {
        order.insert(insert_at + k, f);
    }
    // Inverse map: pos[old_index] = new_index.
    let mut pos = vec![0usize; n];
    for (new_idx, &old) in order.iter().enumerate() {
        pos[old] = new_idx;
    }
    Some(pos)
}

/// Why [`move_frames`] (or [`move_frame`]) refuses this move (the deck is then
/// left unchanged): it would tear (leave not contiguous) an object the remap
/// cannot split faithfully into one piece per run ([`splits_on_move`]) — a
/// play-time span (`Loop`, `Animation`, `AutoAdvance`), a `Morph` (its blend is
/// timed by its range), an explicit-range `Group`, or an object with animated
/// coordinates. Stretched instead, it would show on slides it never covered
/// (and a span could then overlap or bisect another). Moving such an object's
/// frames together, or reordering frames inside it, is fine. Stored ranges are
/// checked, including those a group overrides (never stretched either); the
/// overriding range is a group's own, refused when torn.
pub fn move_frames_refusal(
    source: &SourcePresentation,
    frames: &[usize],
    target: usize,
    before: bool,
) -> Option<String> {
    let n = source.frame_count;
    let pos = move_permutation(n, frames, target, before)?;
    // Stored ranges suffice: a member's played range under an explicit group is
    // the group's, and a torn explicit group is refused itself.
    for o in &source.objects {
        let kind = match o {
            SceneObject::Loop(_) => "loop",
            SceneObject::Animation(_) => "animation",
            SceneObject::AutoAdvance(_) => "auto-advance",
            SceneObject::Morph(_) => "morph",
            SceneObject::Group(g) if g.frames.is_some() => "group",
            o if is_animated(o) => "animated object",
            _ => continue,
        };
        let Some(fr) = scene_object_frame_range(o) else { continue };
        let (start, end) = (fr.start, fr.end.min(n));
        if start >= end {
            continue;
        }
        let (lo, hi) = (start..end)
            .map(|f| pos[f])
            .fold((usize::MAX, 0), |(lo, hi), p| (lo.min(p), hi.max(p)));
        if hi + 1 - lo != end - start {
            return Some(format!(
                "That move would split the {kind} on slides {}-{end} (a slide moved into or out of it) — move all of its slides or none",
                start.saturating_add(1)
            ));
        }
    }
    // With every span kept whole (the same frames, still together), no overlap,
    // nesting or bisection between spans can appear or vanish — a valid deck
    // stays valid, so `validate_loops` need not be re-run.
    None
}

/// Whether the move remap may split `obj` (at index `i`) into one piece per
/// contiguous run when a move tears its range: plain content can be, since each
/// piece just shows on its own slides. Not a play-time span, a `Morph`, an
/// explicit group or an animated object (refused by [`move_frames_refusal`]),
/// nor an explicit group's member, whose own range is overridden (never shown).
fn splits_on_move(obj: &SceneObject, i: usize, explicit_members: &std::collections::HashSet<usize>) -> bool {
    !matches!(
        obj,
        SceneObject::Loop(_) | SceneObject::Animation(_) | SceneObject::AutoAdvance(_) | SceneObject::Morph(_)
    ) && !matches!(obj, SceneObject::Group(g) if g.frames.is_some())
        && !is_animated(obj)
        && !explicit_members.contains(&i)
}

/// Remap every object frame range through the frame permutation `pos`
/// (`pos[old_index] = new_index`) by membership: an object shows on exactly the
/// slides it showed before. A range whose frames stay together moves as one;
/// one the move tears (a slide moved out of or into it) is split into one
/// object per contiguous run — the original keeps the first, a clone each other
/// run, joining the original's link family (or a new one with it) and every
/// auto group holding it, as [`split_around_copy`]'s continuation does, and
/// placed right after it (object indices shift, so callers drop index caches
/// like `clipboard_sources`). An
/// object [`splits_on_move`] rules out keeps the contiguous hull (the UI
/// refuses such a tear first). Shared by [`move_frame`]/[`move_frames`].
///
/// `Animation` spans ride along here too: an `Animation`'s frame range *is* its
/// span (the single source of truth), and animated coordinates carry no span of
/// their own, so remapping object ranges is all that's needed.
fn remap_ranges_through_pos(source: &mut SourcePresentation, pos: &[usize], n: usize) {
    let explicit_members: std::collections::HashSet<usize> = source
        .objects
        .iter()
        .filter_map(|o| match o {
            SceneObject::Group(g) if g.frames.is_some() => Some(g.members.iter().copied()),
            _ => None,
        })
        .flatten()
        .collect();
    let first_new = source.objects.len();
    let mut pieces: Vec<(usize, SceneObject)> = Vec::new();
    for i in 0..first_new {
        let splits = splits_on_move(&source.objects[i], i, &explicit_members);
        let Some(fr) = scene_object_frame_range_mut(&mut source.objects[i]) else { continue };
        if fr.start >= fr.end {
            continue;
        }
        // Frames past the deck all map like its last one; walk only the
        // clamped frames, so a hand-edited huge range costs little.
        let mut ps: Vec<usize> = (fr.start.min(n - 1)..=(fr.end - 1).min(n - 1)).map(|f| pos[f]).collect();
        ps.sort_unstable();
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for p in ps {
            match runs.last_mut() {
                Some(r) if p == r.1 => r.1 += 1,
                _ => runs.push((p, p + 1)),
            }
        }
        if splits {
            (fr.start, fr.end) = runs[0];
        } else {
            (fr.start, fr.end) = (runs[0].0, runs[runs.len() - 1].1);
            continue;
        }
        for &(a, b) in &runs[1..] {
            let mut piece = source.objects[i].clone();
            if let Some(pr) = scene_object_frame_range_mut(&mut piece) {
                (pr.start, pr.end) = (a, b);
            }
            pieces.push((i, piece));
        }
    }
    let mut piece_map: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for (i, piece) in pieces {
        let idx = source.objects.len();
        source.objects.push(piece);
        piece_map.entry(i).or_default().push(idx);
        match source.links.iter_mut().find(|fam| fam.contains(&i)) {
            Some(fam) => fam.push(idx),
            None => source.links.push(vec![i, idx]),
        }
    }
    for obj in &mut source.objects[..first_new] {
        if let SceneObject::Group(g) = obj {
            if g.frames.is_none() {
                let extra: Vec<usize> =
                    g.members.iter().filter_map(|m| piece_map.get(m)).flatten().copied().collect();
                g.members.extend(extra);
            }
        }
    }
    if !piece_map.is_empty() {
        // The pieces were appended; move them right after their original so it
        // keeps its draw position among equal-`z_order` objects (as copy_frames).
        place_after(source, &piece_map);
    }
}

/// Why frame operations are refused on a deck of `frame_count` frames: `Some`
/// when it is over [`MAX_FRAME_COUNT`](crate::engine::source::MAX_FRAME_COUNT)
/// (only a hand-edited file gets there), since selecting or moving frames walks
/// every frame. Settings cannot change the frame count, so it says to edit it.
pub fn frame_limit_error(frame_count: usize) -> Option<String> {
    use crate::engine::source::MAX_FRAME_COUNT;
    (frame_count > MAX_FRAME_COUNT).then(|| {
        format!(
            "{frame_count} frames is over the limit of {MAX_FRAME_COUNT} — fix frame_count in the file"
        )
    })
}

/// Why inserting `added` frames into `source` is refused: `Some` when the grown
/// deck would fail [`SourcePresentation::validate_contract`] (frame count or
/// total cells), so no editor op can grow a deck past what compiles. Callers
/// check this before changing anything.
pub fn frame_growth_error(source: &SourcePresentation, added: usize) -> Option<String> {
    let grown = SourcePresentation {
        width: source.width,
        height: source.height,
        frame_count: source.frame_count.saturating_add(added),
        objects: Vec::new(),
        links: Vec::new(),
    };
    grown.validate_contract().err()
}

/// Parse a multi-frame selection string into 0-based, sorted, de-duplicated
/// frame indices clamped to the deck. Accepts comma-separated tokens, each a
/// single 1-based number (`3`) or an inclusive 1-based range (`5-12`), mixable
/// (`1, 3, 5-8`). Returns an error message on a malformed token or empty result.
pub fn parse_frame_selection(input: &str, frame_count: usize) -> Result<Vec<usize>, String> {
    // A hand-edited deck past the limit could otherwise collect billions.
    if let Some(e) = frame_limit_error(frame_count) {
        return Err(e);
    }
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
            // Only the part inside the deck: the work stays bounded by
            // `frame_count` however large `b` is typed (this runs per redraw).
            out.extend((a - 1)..b.min(frame_count));
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
    flatten_collapsed_animations(source, deleted);
    // Remove objects whose *shown* range collapsed: a member of an explicit
    // group goes by the group's range (`member_overrides`), so it goes with the
    // group even if its own range is wider (it must not resurface on frames the
    // group hid), and survives with the group by adopting the group's range as
    // its own (for good — it keeps that span if later ungrouped). A nested group
    // draws nothing and gates its members by its own range, so an override on it
    // is ignored. Auto groups (no stored range) are kept — their visibility
    // follows their members. Each removal fixes up group/link indices (highest
    // first, so the indices still to remove stay valid).
    let overrides = source.member_overrides();
    let mut doomed = Vec::new();
    for (i, ov) in overrides.into_iter().enumerate() {
        let ov = ov.filter(|_| !matches!(source.objects[i], SceneObject::Group(_)));
        let Some(own) = scene_object_frame_range_mut(&mut source.objects[i]) else {
            continue;
        };
        match ov {
            Some(shown) if shown.start < shown.end => {
                if own.start >= own.end {
                    *own = shown;
                }
            }
            Some(_) => doomed.push(i),
            None if own.start >= own.end => doomed.push(i),
            None => {}
        }
    }
    for &i in doomed.iter().rev() {
        source.objects.remove(i);
        adjust_group_members_after_delete(source, i);
    }
    // A pruned object may have been the last one an animation drove; an
    // undriven animation would still auto-play its span, so drop it too.
    prune_orphan_animations(source);
    // A surviving `Animation`'s span needs no separate fix-up: its frame range
    // *is* its span (shifted above), and animated coordinates carry no span.
}

/// Before `adjust_frames_after_delete` prunes an `Animation` whose span
/// collapsed (it covered only the deleted frame), freeze every coordinate it
/// drives to the value its surviving frames showed, so none is left pointing
/// at a missing id. A one-frame span shows `from` up to and on its frame and
/// `to` after it, so the coordinate becomes `to` when the object (its explicit
/// group range, if any) still has frames after the deleted one, else `from`.
/// An object on both sides keeps `to` for all of them (one value cannot hold
/// both); ranges are never widened, unlike [`remove_animation`].
fn flatten_collapsed_animations(source: &mut SourcePresentation, deleted: usize) {
    let gone: Vec<AnimId> = source
        .objects
        .iter()
        .filter_map(|o| match o {
            SceneObject::Animation(a) if a.frames.start >= a.frames.end => Some(a.id),
            _ => None,
        })
        .collect();
    if gone.is_empty() {
        return;
    }
    let overrides = source.member_overrides();
    for (obj, ov) in source.objects.iter_mut().zip(overrides) {
        let after = ov
            .as_ref()
            .or(scene_object_frame_range(obj))
            .is_some_and(|fr| fr.end > deleted);
        for coord in scene_object_coordinates_mut(obj) {
            if let Coordinate::Animated { from, to, anim } = *coord {
                if gone.contains(&anim) {
                    *coord = Coordinate::Fixed(if after { to } else { from } as f64);
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

/// Delete a set of objects (indices into `source.objects`), handling each the
/// same way single-object delete does: an `Animation` is removed through
/// [`remove_animation`] (reverting the motion it drives, not just dropping the
/// sidecar), every other object is removed in place with group/link index
/// fix-up ([`adjust_group_members_after_delete`]).
///
/// Plain objects are removed **highest index first** so the lower indices stay
/// valid as the array shrinks; animations are then removed **by id**, which is
/// stable across those shifts (and lets `remove_animation` clean up the motion
/// and any strobe clones). Finally any `Animation` no remaining object drives is
/// pruned ([`prune_orphan_animations`]) — deleting an animation's last driver
/// must not leave it auto-playing a motionless span. A selected sample of a
/// gap strobe takes its whole [`strobe_set`] with it. Returns the number of
/// *selected* objects removed.
pub fn delete_objects(source: &mut SourcePresentation, indices: &[usize]) -> usize {
    let mut targets: Vec<usize> = indices.to_vec();
    targets.sort_unstable();
    targets.dedup();
    targets.retain(|&i| i < source.objects.len());
    let selected = targets.len();
    // A gap-strobed element is one strobe set (original + copies): deleting any
    // sample deletes them all, or the survivors still drive the animation and
    // the next re-strobe would bring the deleted sample back.
    let sets: Vec<usize> = targets
        .iter()
        .filter(|&&i| !matches!(source.objects[i], SceneObject::Animation(_)))
        .flat_map(|&i| strobe_set(source, i))
        .collect();
    targets.extend(sets);
    targets.sort_unstable();
    targets.dedup();

    // Capture animation ids up front: plain removals shift indices, but ids are
    // stable and `remove_animation` locates its object by id.
    let anim_ids: Vec<AnimId> = targets
        .iter()
        .filter_map(|&i| match &source.objects[i] {
            SceneObject::Animation(a) => Some(a.id),
            _ => None,
        })
        .collect();

    // Remove the plain (non-animation) objects, highest index first.
    for &i in targets.iter().rev() {
        if matches!(source.objects[i], SceneObject::Animation(_)) {
            continue;
        }
        source.objects.remove(i);
        adjust_group_members_after_delete(source, i);
    }

    // Then the animations, by id (also reverts the coordinates they drove).
    for id in anim_ids {
        remove_animation(source, id);
    }
    prune_orphan_animations(source);

    selected
}

/// Format a millisecond delay as a compact seconds string (e.g. `5000` → `"5s"`,
/// `1500` → `"1.5s"`). Used for auto-advance summaries and status messages.
pub fn format_secs(ms: u64) -> String {
    let s = ms as f64 / 1000.0;
    format!("{s}s")
}

/// The auto-advance delay (ms) configured on `frame`, if any single-frame
/// `AutoAdvance` marker covers it. Used to seed the frame sub-action's input.
pub fn frame_auto_advance_delay(source: &SourcePresentation, frame: usize) -> Option<u64> {
    source.objects.iter().find_map(|o| match o {
        SceneObject::AutoAdvance(a) if a.frames.start <= frame && frame < a.frames.end => {
            Some(a.delay_ms)
        }
        _ => None,
    })
}

/// Set `frame`'s auto-advance to `delay_ms` (`0` turns it off). Every marker
/// covering the frame gives it up first — a wider one is split around it, so
/// the other frames keep their delay — then a single-frame marker is added
/// when `delay_ms > 0`. Afterwards [`frame_auto_advance_delay`] reports exactly
/// `delay_ms` (or `None`).
///
/// Returns the indices of removed markers, in removal order (for
/// `EditorState::note_removed_objects`). Removal goes through the same index
/// fix-up as object deletion so a group/link reference never points at the
/// wrong object.
pub fn set_frame_auto_advance(
    source: &mut SourcePresentation,
    frame: usize,
    delay_ms: u64,
) -> Vec<usize> {
    let mut removed = Vec::new();
    let mut i = 0;
    while i < source.objects.len() {
        let SceneObject::AutoAdvance(a) = &mut source.objects[i] else {
            i += 1;
            continue;
        };
        if !a.frames.contains(frame) {
            i += 1;
            continue;
        }
        let (start, end, delay) = (a.frames.start, a.frames.end, a.delay_ms);
        // Keep the part before `frame` in place; the part after it goes in
        // place too when nothing precedes, else into a new (appended) marker.
        if start < frame {
            a.frames.end = frame;
            if frame + 1 < end {
                source.objects.push(SceneObject::AutoAdvance(AutoAdvance {
                    frames: FrameRange { start: frame + 1, end },
                    delay_ms: delay,
                }));
            }
        } else if frame + 1 < end {
            a.frames.start = frame + 1;
        } else {
            source.objects.remove(i);
            adjust_group_members_after_delete(source, i);
            removed.push(i);
            continue;
        }
        i += 1;
    }
    if delay_ms > 0 {
        source.objects.push(SceneObject::AutoAdvance(AutoAdvance {
            frames: FrameRange { start: frame, end: frame + 1 },
            delay_ms,
        }));
    }
    removed
}

pub fn scene_object_summary(obj: &SceneObject) -> String {
    // Deck strings in it (label text, a command) are shown: never their controls.
    crate::types::printable_text(&match obj {
        SceneObject::Label(l) => {
            let first_line = l.text.split('\n').next().unwrap_or("");
            let text_preview: String = first_line.chars().take(15).collect();
            format!("Label: \"{}\"", text_preview)
        }
        SceneObject::HLine(h) => format!("HLine: y={} x={}..{}", h.y.start_value(), h.x_start.start_value(), h.x_end.start_value()),
        SceneObject::Rect(r) => format!("Rect: {}x{}", r.width.start_value(), r.height.start_value()),
        SceneObject::Header(h) => {
            let text_preview: String = h.text.chars().take(10).collect();
            format!("Header: \"{}\"", text_preview)
        }
        SceneObject::Group(g) => format!("Group: {} members", g.members.len()),
        SceneObject::Arrow(a) => format!("Arrow: ({},{})→({},{})", a.x1.start_value(), a.y1.start_value(), a.x2.start_value(), a.y2.start_value()),
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
            let lo = a.frames.start.saturating_add(1);
            let hi = a.frames.end; // exclusive end == 1-based inclusive last
            let play = if a.auto_play { format!("auto {}ms", a.delay_ms) } else { "manual".into() };
            format!("Animation: {lo}-{hi} ({play})")
        }
        SceneObject::AutoAdvance(a) => {
            let lo = a.frames.start.saturating_add(1);
            let hi = a.frames.end; // exclusive end == 1-based inclusive last
            format!("Auto-advance: {lo}-{hi} ({})", format_secs(a.delay_ms))
        }
        SceneObject::Circle(c) => format!("Circle: ⌀{} '{}'", c.diameter, c.ch),
    })
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
    fn copying_a_nested_group_keeps_the_inner_groups_members() {
        // objects: [label0, label1, group(0,1), label3, group(2,3), group(5)].
        // Selecting the outer group pulls in the inner group *and* its members;
        // the self-listing group 5 terminates.
        let p = pres(1, vec![
            label(0, 1), label(0, 1), group(vec![0, 1]), label(0, 1), group(vec![2, 3]), group(vec![5]),
        ]);
        let sel = expand_selection(&p, &[4]);
        assert_eq!(sel, vec![0, 1, 2, 3, 4]);
        assert_eq!(expand_selection(&p, &[5]), vec![5]);
        let clones = clone_selection(&p.objects, &sel);
        assert_eq!(members_of(&clones[2]), vec![0, 1], "the pasted inner group is not empty");
        assert_eq!(members_of(&clones[4]), vec![2, 3]);
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
        // A label animated over span [0,10). gap=3 means 3 *empty* frames between
        // appearances → the element shows every 4th frame: 0, 4, 8. The original
        // holds the first sample frame; clones occupy frames 4 and 8.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![obj]);
        apply_gap(&mut p, 0, 0, 9, 3);
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
    fn apply_gap_with_a_huge_gap_keeps_only_the_first_sample() {
        // gap = usize::MAX used to overflow `gap + 1` (debug panic, release hang).
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![obj]);
        apply_gap(&mut p, 0, 2, 9, usize::MAX);
        assert_eq!(p.objects.len(), 1, "no strobe copies");
        assert_eq!(range(&p.objects[0]), (2, 3), "the first sample only");
    }

    #[test]
    fn restrobe_clips_a_huge_gapped_span_to_the_deck() {
        // A hand-edited span far past a 10-frame deck strobes only the deck's
        // frames (0, 2, 4, 6, 8), not a sample per frame of the span.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
        }
        let anim = Animation {
            id: 1,
            frames: FrameRange { start: 0, end: 1_000_000_000 },
            auto_play: true,
            delay_ms: 500,
            gap_frames: 1,
        };
        let mut p = pres(10, vec![obj, SceneObject::Animation(anim)]);
        let old = AnimSpans::of(&p);
        restrobe(&mut p, 0, &old);
        assert_eq!(p.objects.len(), 2 + 4, "original + 4 copies");
    }

    #[test]
    fn apply_gap_of_zero_is_a_noop() {
        // gap 0 = no empty frames = off: the element stays spanning every frame.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![obj]);
        apply_gap(&mut p, 0, 0, 9, 0);
        assert_eq!(p.objects.len(), 1);
        assert_eq!(range(&p.objects[0]), (0, 10)); // untouched
    }

    #[test]
    fn clear_gap_clones_removes_only_matching_copies() {
        // Build a gapped element by hand: a label animated [0,10), strobed so it
        // shows on 0, 4, 8 (original + 2 clones), plus an unrelated label.
        let mut anim = create_default(0, 0);
        if let SceneObject::Label(l) = &mut anim {
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![anim, label(0, 10)]); // index 0 = animated, 1 = other
        apply_gap(&mut p, 0, 0, 9, 3); // → index 0 ([0,1)), clones appended at 2,3
        assert_eq!(p.objects.len(), 4);

        clear_gap_clones(&mut p, 0, 0, 9);
        // The two clones are gone; the original (0) and the unrelated label (1) stay.
        assert_eq!(p.objects.len(), 2);
        assert_eq!(range(&p.objects[0]), (0, 1)); // original untouched
        assert_eq!(range(&p.objects[1]), (0, 10)); // unrelated label untouched
    }

    #[test]
    fn clear_gap_clones_spares_a_different_animation_with_the_same_motion() {
        // Two distinct elements (different text) that animate *identically* — same
        // from/to/span — and are both strobed over the same frames. Clearing one
        // must not delete the other's gap copies (the bug: matching strobe copies
        // by motion alone wiped an overlapping animation that shared it).
        let mut a = create_default(0, 0);
        let mut b = create_default(0, 0);
        if let SceneObject::Label(l) = &mut a {
            l.text = "A".into();
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        if let SceneObject::Label(l) = &mut b {
            l.text = "B".into();
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![a, b]); // index 0 = A, 1 = B
        apply_gap(&mut p, 0, 0, 9, 3); // strobe A → A clones appended (indices 2,3)
        // B's index shifts only if A clones were inserted before it; they are
        // pushed to the tail, so B is still findable by its text.
        let b_idx = p.objects.iter().position(|o| matches!(o, SceneObject::Label(l) if l.text == "B")).unwrap();
        apply_gap(&mut p, b_idx, 0, 9, 3); // strobe B → B clones appended
        // 2 originals + 2 A-clones + 2 B-clones.
        assert_eq!(p.objects.len(), 6);

        // Clear A's clones (e.g. re-editing animation A). B's strobe must survive.
        clear_gap_clones(&mut p, 0, 0, 9);
        let count = |t: &str| p.objects.iter().filter(|o| matches!(o, SceneObject::Label(l) if l.text == t)).count();
        assert_eq!(count("A"), 1, "A's clones should be cleared (just the original left)");
        assert_eq!(count("B"), 3, "B's three strobe samples must be untouched");
    }

    #[test]
    fn remove_animation_reverts_motion_and_drops_the_sidecar() {
        // A label whose x animates 2→12 over frames 0..=4, plus its Animation
        // sidecar over [0,5). Removing the animation should flatten x to its
        // `from` (2), keep the label spanning the span, and delete the sidecar.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 2, to: 12, anim: 1 };
            l.frames = FrameRange { start: 0, end: 5 };
        }
        let mut p = pres(5, vec![obj]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        assert_eq!(p.objects.len(), 2); // label + Animation sidecar

        remove_animation(&mut p, 1);
        assert_eq!(p.objects.len(), 1, "sidecar removed");
        match &p.objects[0] {
            SceneObject::Label(l) => {
                assert!(matches!(l.position.x, Coordinate::Fixed(v) if v == 2.0), "x reverted to `from`");
                assert_eq!((l.frames.start, l.frames.end), (0, 5), "still spans the whole span statically");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn remove_animation_clears_gap_strobe_copies() {
        // A gapped (strobed) animation: original on a single frame + clones.
        // Removing it must delete the clones and restore the original to a static
        // element across the span — no leftover scattered samples.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![obj]);
        ensure_animation(&mut p, 1, 0, 10, true, 500, 3);
        apply_gap(&mut p, 0, 0, 9, 3); // original + 2 clones + sidecar
        assert_eq!(p.objects.iter().filter(|o| matches!(o, SceneObject::Label(_))).count(), 3);

        remove_animation(&mut p, 1);
        let labels: Vec<_> = p.objects.iter().filter(|o| matches!(o, SceneObject::Label(_))).collect();
        assert_eq!(labels.len(), 1, "strobe clones removed, one static label left");
        assert!(!p.objects.iter().any(|o| matches!(o, SceneObject::Animation(_))), "sidecar removed");
        assert_eq!(range(p.objects.iter().find(|o| matches!(o, SceneObject::Label(_))).unwrap()), (0, 10));
    }

    #[test]
    fn remove_animation_spares_an_overlapping_animation_on_another_span() {
        // Two labels: A animated over [0,5), B over [0,10). Removing A's animation
        // leaves B's motion and B's sidecar untouched.
        let mut a = create_default(0, 0);
        let mut b = create_default(0, 0);
        if let SceneObject::Label(l) = &mut a {
            l.text = "A".into();
            l.position.x = Coordinate::Animated { from: 1, to: 5, anim: 1 };
            l.frames = FrameRange { start: 0, end: 5 };
        }
        if let SceneObject::Label(l) = &mut b {
            l.text = "B".into();
            l.position.x = Coordinate::Animated { from: 1, to: 9, anim: 2 };
            l.frames = FrameRange { start: 0, end: 10 };
        }
        let mut p = pres(10, vec![a, b]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        ensure_animation(&mut p, 2, 0, 10, true, 500, 0);

        remove_animation(&mut p, 1);
        // A reverted to Fixed; B still animated; only B's sidecar remains.
        let a = p.objects.iter().find(|o| matches!(o, SceneObject::Label(l) if l.text == "A")).unwrap();
        let b = p.objects.iter().find(|o| matches!(o, SceneObject::Label(l) if l.text == "B")).unwrap();
        match a { SceneObject::Label(l) => assert!(matches!(l.position.x, Coordinate::Fixed(_))), _ => panic!() }
        match b { SceneObject::Label(l) => assert!(matches!(l.position.x, Coordinate::Animated { .. })), _ => panic!() }
        let anim_spans: Vec<_> = p.objects.iter().filter_map(|o| match o {
            SceneObject::Animation(an) => Some((an.frames.start, an.frames.end)), _ => None,
        }).collect();
        assert_eq!(anim_spans, vec![(0, 10)], "only B's sidecar should remain");
    }

    #[test]
    fn prune_orphan_animations_keeps_a_still_used_sidecar() {
        // Sidecar id 1 with a label still referencing it: not orphaned, so it
        // stays. Only once the coordinate is reverted does it become removable.
        let mut obj = create_default(0, 0);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 2, to: 12, anim: 1 };
            l.frames = FrameRange { start: 0, end: 5 };
        }
        let mut p = pres(5, vec![obj]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);

        prune_orphan_animations(&mut p);
        assert_eq!(p.objects.len(), 2, "sidecar kept while a coordinate references it");

        // Revert the motion, then the sidecar is an orphan and gets removed.
        if let SceneObject::Label(l) = &mut p.objects[0] {
            l.position.x = Coordinate::Fixed(2.0);
        }
        prune_orphan_animations(&mut p);
        assert_eq!(p.objects.len(), 1, "orphaned sidecar removed");
    }

    #[test]
    fn flatten_coordinates_converts_animated_to_fixed_at_frame() {
        // A label whose x animates 2→12 over frames 0..=4. Flattening at the end
        // frame yields a Fixed x of 12 — static and now nudgeable by move_by
        // (which only adjusts Fixed coords), so a paste can be moved horizontally.
        let mut obj = create_default(0, 0); // Label, all Fixed
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 2, to: 12, anim: 1 };
        }
        // Animation 1 spans frames 0..=4, so the end-frame value is `to` (12).
        let anims = AnimSpans::from_pairs([(1, FrameRange { start: 0, end: 5 })]);
        flatten_coordinates(&mut obj, 4, &anims);
        match &obj {
            SceneObject::Label(l) => match l.position.x {
                Coordinate::Fixed(v) => assert_eq!(v, 12.0),
                _ => panic!("x should be Fixed after flattening"),
            },
            _ => panic!(),
        }
        // No animated coordinate remains.
        assert_eq!(scene_object_animation_span(&obj, &anims), None);
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

    /// Deck from the R4-3 report: label M spans 0..10, but explicit group G
    /// (`5..6`) narrows it to slide 5 only.
    fn explicit_group_deck() -> SourcePresentation {
        let mut g = group(vec![0]);
        if let SceneObject::Group(grp) = &mut g {
            grp.frames = Some(FrameRange { start: 5, end: 6 });
        }
        pres(10, vec![label(0, 10), g])
    }

    /// Indices of the labels rendered on `frame` (group overrides applied).
    fn labels_on(p: &SourcePresentation, frame: usize) -> Vec<usize> {
        let shown = shown_ranges(p);
        (0..p.objects.len())
            .filter(|&i| matches!(p.objects[i], SceneObject::Label(_)) && shown[i].contains(frame))
            .collect()
    }

    /// Auto group G of label B (frame 0) and label A (frame 1, revealed after
    /// B): copying or overlaying frame 1 clones G (its range ends there).
    fn reveal_group_deck() -> SourcePresentation {
        serde_json::from_str(
            r#"{ "width": 10, "height": 3, "frame_count": 4, "objects": [
                { "type": "label", "text": "A", "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 1, "end": 2 } },
                { "type": "label", "text": "B", "position": { "x": { "fixed": 0 }, "y": { "fixed": 1 } },
                  "frames": { "start": 0, "end": 1 } },
                { "type": "group", "members": [0, 1] }
            ] }"#,
        )
        .unwrap()
    }

    /// Index of the last group and of the last label with `text` in `p`.
    fn last_group_and_label(p: &SourcePresentation, text: &str) -> (usize, usize) {
        let g = p.objects.iter().rposition(|o| matches!(o, SceneObject::Group(_))).unwrap();
        let l = p.objects.iter().rposition(|o| matches!(o, SceneObject::Label(l) if l.text == text)).unwrap();
        (g, l)
    }

    #[test]
    fn a_copied_auto_group_keeps_only_members_on_the_copied_slide() {
        // Copy frame 1: G's clone holds A's clone, not B (on another slide), so
        // moving the clone leaves every original slide as it was.
        let mut p = reveal_group_deck();
        copy_frame(&mut p, 1);
        let (clone_g, a_clone) = last_group_and_label(&p, "A");
        assert_ne!(clone_g, 2, "G was cloned");
        assert_eq!(members_of(&p.objects[clone_g]), vec![a_clone]);
        let before = frame_texts(&p);
        super::super::properties::move_group(&mut p.objects, clone_g, 3, 0);
        let after = frame_texts(&p);
        assert_eq!(after[..2], before[..2], "original slides unchanged");
        assert_eq!(after[3..], before[3..], "original slides unchanged");

        // Overlay frame 1 onto frame 3: likewise.
        let mut p = reveal_group_deck();
        overlay_frame(&mut p, 1, 3);
        let (clone_g, a_clone) = last_group_and_label(&p, "A");
        assert_ne!(clone_g, 2, "G was cloned");
        assert_eq!(members_of(&p.objects[clone_g]), vec![a_clone]);
        let before = frame_texts(&p);
        super::super::properties::move_group(&mut p.objects, clone_g, 3, 0);
        let after = frame_texts(&p);
        assert_eq!(after[..3], before[..3], "slides other than the overlay target unchanged");
    }

    #[test]
    fn copy_frame_clones_an_explicit_group_member_it_overrides() {
        // M's own range spans past slide 5, but G's override confines it there,
        // so the copy must clone M too, not let G's clone steal the original.
        let mut p = explicit_group_deck();
        copy_frame(&mut p, 5);
        assert_eq!(labels_on(&p, 5), vec![0], "original slide keeps M");
        assert_eq!(labels_on(&p, 6), vec![2], "the copy shows M's clone");
        assert_eq!(members_of(&p.objects[3]), vec![2]);
    }

    #[test]
    fn copy_frame_drops_a_member_another_explicit_group_shows() {
        // M is in G1 (5..6) and in G2 (0..10, later, so it wins M's override).
        // Copying slide 5 clones G1 but shares M; the clone must not keep M.
        let mut p = explicit_group_deck();
        let mut g2 = group(vec![0]);
        if let SceneObject::Group(grp) = &mut g2 {
            grp.frames = Some(FrameRange { start: 0, end: 10 });
        }
        p.objects.push(g2);
        copy_frame(&mut p, 5);
        assert_eq!(members_of(&p.objects[3]), Vec::<usize>::new());
        assert_eq!(labels_on(&p, 5), vec![0]);
        assert_eq!(labels_on(&p, 6), vec![0]);
    }

    fn loop_obj(start: usize, end: usize) -> SceneObject {
        let mut o = create_default(10, 0);
        assert!(matches!(o, SceneObject::Loop(_)));
        if let Some(fr) = scene_object_frame_range_mut(&mut o) {
            *fr = FrameRange { start, end };
        }
        o
    }

    fn loop_ranges(p: &SourcePresentation) -> Vec<(usize, usize)> {
        p.objects.iter().filter(|o| matches!(o, SceneObject::Loop(_))).map(range).collect()
    }

    #[test]
    fn frame_copy_and_overlay_never_clone_a_loop_onto_one_frame() {
        // Loop A on {0,1}, loop B on {4..6}; a label on frame 1.
        let deck = pres(8, vec![loop_obj(0, 2), loop_obj(4, 7), label(1, 2)]);
        let mut p = deck.clone();
        assert_eq!(overlay_frame(&mut p, 1, 5), 1, "only the label is overlaid");
        assert_eq!(loop_ranges(&p), vec![(0, 2), (4, 7)]);
        assert!(p.validate_loops().is_ok());
        // Copying A's last frame: the copy gets the label, not a one-frame loop.
        let mut p = deck.clone();
        copy_frame(&mut p, 1);
        assert_eq!(loop_ranges(&p), vec![(0, 2), (5, 8)]);
        assert!(p.validate_loops().is_ok());
    }

    #[test]
    fn block_copy_refuses_to_put_a_loop_inside_a_loop_or_animation() {
        let deck = pres(8, vec![loop_obj(0, 2), loop_obj(4, 7)]);
        // Copying the first loop into the second would nest it: refused, unchanged.
        let mut p = deck.clone();
        assert!(copy_frames_refusal(&p, 0, 1, 5, false).is_some());
        assert_eq!(copy_frames(&mut p, 0, 1, 5, false), (5, 0));
        assert_eq!((p.frame_count, loop_ranges(&p)), (8, vec![(0, 2), (4, 7)]));
        // Outside any loop it is copied.
        assert!(copy_frames_refusal(&p, 0, 1, 7, false).is_none());
        copy_frames(&mut p, 0, 1, 7, false);
        assert_eq!(loop_ranges(&p), vec![(0, 2), (4, 7), (8, 10)]);
        assert!(p.validate_loops().is_ok());
        // Nor inside an animation, which the loop would cut in half.
        let mut q = pres(8, vec![loop_obj(0, 2)]);
        ensure_animation(&mut q, 1, 4, 7, true, 500, 0);
        assert_eq!(copy_frames(&mut q, 0, 1, 5, false), (5, 0));
        assert_eq!(q.frame_count, 8);
        // A loop an explicit group moves into the block counts as carried.
        let mut g = group(vec![0]);
        if let SceneObject::Group(grp) = &mut g {
            grp.frames = Some(FrameRange { start: 0, end: 2 });
        }
        let mut r = pres(8, vec![loop_obj(6, 7), g, loop_obj(4, 7)]);
        assert_eq!(copy_frames(&mut r, 0, 1, 4, false), (4, 0));
        assert!(r.validate_loops().is_ok());
    }

    #[test]
    fn frame_block_paste_refuses_to_put_a_loop_inside_a_loop() {
        let deck = pres(8, vec![loop_obj(0, 2), loop_obj(4, 7)]);
        let clip = copy_frame_block(&deck, 0, 1);
        let mut p = deck.clone();
        assert!(paste_frame_block_refusal(&p, &clip, 5, false).is_some());
        assert_eq!(paste_frame_block(&mut p, &clip, 5, false), (5, 0));
        assert_eq!((p.frame_count, loop_ranges(&p)), (8, vec![(0, 2), (4, 7)]));
        paste_frame_block(&mut p, &clip, 7, false);
        assert_eq!(loop_ranges(&p), vec![(0, 2), (4, 7), (8, 10)]);
        assert!(p.validate_loops().is_ok());
        // A loop an explicit group moves onto [3,8) counts by that range.
        let mut g = group(vec![0]);
        if let SceneObject::Group(grp) = &mut g {
            grp.frames = Some(FrameRange { start: 3, end: 8 });
        }
        let mut q = pres(8, vec![loop_obj(0, 1), g]);
        assert_eq!(paste_frame_block(&mut q, &clip, 5, false), (5, 0));
        assert!(q.validate_loops().is_ok());
    }

    #[test]
    fn block_copies_take_a_contained_loop_but_not_a_cut_one() {
        let deck = pres(8, vec![loop_obj(0, 2), loop_obj(4, 7)]);
        // [3,5] cuts B: nothing to copy.
        let mut p = deck.clone();
        copy_frames(&mut p, 3, 5, 7, false);
        assert_eq!(loop_ranges(&p), vec![(0, 2), (4, 7)]);
        assert!(copy_frame_block(&deck, 3, 5).objects.is_empty());
        // [0,2] holds A whole: it is copied.
        let mut p = deck.clone();
        copy_frames(&mut p, 0, 2, 7, false);
        assert_eq!(loop_ranges(&p), vec![(0, 2), (4, 7), (8, 10)]);
        assert!(p.validate_loops().is_ok());
        assert_eq!(loop_ranges(&pres(3, copy_frame_block(&deck, 0, 2).objects)), vec![(0, 2)]);
    }

    #[test]
    fn overlay_frame_clones_an_explicit_group_member_it_overrides() {
        let mut p = explicit_group_deck();
        overlay_frame(&mut p, 5, 8);
        assert_eq!(labels_on(&p, 5), vec![0], "original slide keeps M");
        assert_eq!(labels_on(&p, 8), vec![2], "the target shows M's clone");
        assert_eq!(members_of(&p.objects[3]), vec![2]);
    }

    #[test]
    fn a_block_copy_clones_an_auto_group_with_its_cloned_members() {
        // Copying the block of frame 1 (A there, B on frame 0): the copied slide
        // gets a clone of G holding A's clone only; the original G is unchanged.
        let mut p = reveal_group_deck();
        copy_frames(&mut p, 1, 1, 1, false);
        let (clone_g, a_clone) = last_group_and_label(&p, "A");
        assert_ne!(clone_g, 2, "G was cloned");
        assert!(matches!(&p.objects[clone_g], SceneObject::Group(g) if g.frames.is_none()), "still auto");
        assert_eq!(members_of(&p.objects[clone_g]), vec![a_clone]);
        assert_eq!(members_of(&p.objects[2]), vec![0, 1], "the original G keeps its members");

        // Copying both frames: the clone holds both clones.
        let mut p = reveal_group_deck();
        copy_frames(&mut p, 0, 1, 1, false);
        let (clone_g, a_clone) = last_group_and_label(&p, "A");
        let (_, b_clone) = last_group_and_label(&p, "B");
        let mut members = members_of(&p.objects[clone_g]);
        members.sort();
        assert_eq!(members, vec![a_clone.min(b_clone), a_clone.max(b_clone)]);
    }

    #[test]
    fn a_block_copy_does_not_share_a_split_member_into_the_cloned_group() {
        // A on frame 1, B on frames 3..6, auto G = {A, B}. Copying frame 1 in
        // after frame 3 splits B around the copy (it crosses the insert point
        // but has no frame in the block): G's clone holds A's clone only, so
        // moving it leaves B on its original slides alone.
        let mut p: SourcePresentation = serde_json::from_str(
            r#"{ "width": 10, "height": 3, "frame_count": 7, "objects": [
                { "type": "label", "text": "A", "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 1, "end": 2 } },
                { "type": "label", "text": "B", "position": { "x": { "fixed": 0 }, "y": { "fixed": 1 } },
                  "frames": { "start": 3, "end": 6 } },
                { "type": "group", "members": [0, 1] }
            ] }"#,
        )
        .unwrap();
        copy_frames(&mut p, 1, 1, 3, false);
        let (clone_g, a_clone) = last_group_and_label(&p, "A");
        assert_eq!(members_of(&p.objects[clone_g]), vec![a_clone]);
        let before = frame_texts(&p);
        super::super::properties::move_group(&mut p.objects, clone_g, 3, 0);
        let after = frame_texts(&p);
        assert_eq!(after[..4], before[..4], "slide 3 (B's head) unchanged");
        assert_eq!(after[5..], before[5..], "B's continuation unchanged");
    }

    #[test]
    fn copy_frames_clones_an_explicit_group_member_it_overrides() {
        let mut p = explicit_group_deck();
        copy_frames(&mut p, 5, 5, 7, false);
        assert_eq!(labels_on(&p, 5), vec![0], "original slide keeps M");
        assert_eq!(labels_on(&p, 8), vec![2], "the copy shows M's clone");
        assert_eq!(members_of(&p.objects[3]), vec![2]);
    }

    #[test]
    fn copy_frame_block_captures_a_member_widened_by_its_group() {
        // M's own range (0..2) misses the block, but G's override shows it on
        // slide 5, so the capture keeps M (block-local, on G's range).
        let mut p = explicit_group_deck();
        if let Some(fr) = scene_object_frame_range_mut(&mut p.objects[0]) {
            fr.end = 2;
        }
        let clip = copy_frame_block(&p, 5, 5);
        assert_eq!(clip.objects.len(), 2);
        assert_eq!(range(&clip.objects[0]), (0, 1));
        assert_eq!(members_of(&clip.objects[1]), vec![0]);
    }

    // The exclusive `[start, end)` span of the animation driving `obj` (looked up
    // by the id its first animated coordinate references), if any.
    fn driving_span(p: &SourcePresentation, obj_idx: usize) -> Option<(usize, usize)> {
        let id = *referenced_anim_ids(&p.objects[obj_idx]).first()?;
        p.objects.iter().find_map(|o| match o {
            SceneObject::Animation(a) if a.id == id => Some((a.frames.start, a.frames.end)),
            _ => None,
        })
    }

    #[test]
    fn delete_animation_end_frame_keeps_span_and_range_in_lockstep() {
        // x animates 0→10 over animation 1's span [0,5) (object range [0,5)).
        // Deleting the animation's *last* frame (4) must shrink the sidecar span
        // and the object range together, so the motion still reaches `to` on its
        // new last frame instead of stopping short (the "jumpy" bug).
        let mut o = create_default(0, 0);
        if let SceneObject::Label(l) = &mut o {
            l.position.x = Coordinate::Animated { from: 0, to: 10, anim: 1 };
            l.frames = FrameRange { start: 0, end: 5 };
        }
        let mut p = pres(6, vec![o]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        adjust_frames_after_delete(&mut p, 4);

        let (_, re) = range(&p.objects[0]);
        let (s, e_excl) = driving_span(&p, 0).unwrap();
        assert_eq!((s, e_excl), (0, 4));
        assert_eq!(re, e_excl, "object range end stays == the animation's span end");
        // The motion reaches `to` on the last visible frame (no early stop).
        let anims = AnimSpans::of(&p);
        match &p.objects[0] {
            SceneObject::Label(l) => assert_eq!(l.position.x.evaluate(re - 1, &anims), 10),
            _ => panic!(),
        }
    }

    #[test]
    fn delete_frame_range_keeps_multiple_animations_consistent() {
        // Two animated objects with their sidecars: A on animation 1 (span [0,4)),
        // B on animation 2 (span [4,8)). Delete the contiguous range [2,5]
        // (frames 2,3,4,5) — straddling the end of A and the start of B.
        // Afterwards every driven object's range must still equal its animation's
        // (also-shifted) span.
        let mk = |id: AnimId, from: u16, to: u16, s: usize, e: usize| {
            let mut o = create_default(0, 0);
            if let SceneObject::Label(l) = &mut o {
                l.position.x = Coordinate::Animated { from, to, anim: id };
                l.frames = FrameRange { start: s, end: e + 1 };
            }
            o
        };
        let mut p = pres(
            8,
            vec![
                mk(1, 0, 9, 0, 3),
                SceneObject::Animation(Animation { id: 1, frames: FrameRange { start: 0, end: 4 }, auto_play: true, delay_ms: 500, gap_frames: 0 }),
                mk(2, 0, 9, 4, 7),
                SceneObject::Animation(Animation { id: 2, frames: FrameRange { start: 4, end: 8 }, auto_play: true, delay_ms: 500, gap_frames: 0 }),
            ],
        );
        delete_frames(&mut p, &[2, 3, 4, 5]);
        assert_eq!(p.frame_count, 4);

        // For every surviving driven object, its range still equals its
        // animation's span (the single source of truth, shifted in lock-step).
        for idx in 0..p.objects.len() {
            if let Some((s, e_excl)) = driving_span(&p, idx) {
                let (rs, re) = range(&p.objects[idx]);
                assert_eq!(rs, s, "range start tracks the span start");
                assert_eq!(re, e_excl, "range end tracks the span end");
                assert!(s < e_excl, "span stays well-formed");
            }
        }
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
    fn moving_a_frame_out_of_a_loop_or_animation_is_refused() {
        // Loop A on slides 3-5 (2..5), loop B on 7-8 (6..8). Moving frame 3 after
        // frame 7 would stretch A to [2,8) over B; it is refused, deck unchanged.
        let p = pres(10, vec![loop_obj(2, 5), loop_obj(6, 8)]);
        let why = move_frames_refusal(&p, &[3], 7, false).expect("tears loop A");
        assert!(why.contains("split the loop on slides 3-5"), "{why}");
        // Moving a slide in from outside would stretch it too.
        assert!(move_frames_refusal(&p, &[8], 3, true).is_some());
        // An animation the same way.
        let anim = SceneObject::Animation(Animation {
            id: 1, frames: FrameRange { start: 2, end: 5 }, auto_play: true, delay_ms: 500, gap_frames: 0,
        });
        let pa = pres(10, vec![anim]);
        assert!(move_frames_refusal(&pa, &[3], 7, false).unwrap().contains("split the animation"));
        // A multi-frame auto-advance marker.
        let aa: SceneObject =
            serde_json::from_str(r#"{"type":"auto_advance","frames":{"start":2,"end":5}}"#).unwrap();
        let pt = pres(10, vec![aa]);
        assert!(move_frames_refusal(&pt, &[3], 7, false).unwrap().contains("split the auto-advance"));
        // A loop whose played range comes from its group's explicit range: its
        // own stored range (frame 0) is untouched, the group's 2..5 is split.
        let member = loop_obj(0, 1);
        let group: SceneObject =
            serde_json::from_str(r#"{"type":"group","members":[0],"frames":{"start":2,"end":5}}"#).unwrap();
        let pg = pres(10, vec![member, group]);
        assert!(move_frames_refusal(&pg, &[3], 7, false).unwrap().contains("split the group on slides 3-5"));
    }

    /// The frames object `i` shows on.
    fn shows_on(p: &SourcePresentation, i: usize) -> Vec<usize> {
        let r = range(&p.objects[i]);
        (r.0..r.1).collect()
    }

    #[test]
    fn moving_a_slide_out_of_a_multi_slide_label_splits_it_by_membership() {
        // Title on slides 1-3 (0..3); slide 2 moves after slide 10. Old frames
        // 0,1,2 land on 0,9,1: the title stays on 0-1 and on the moved slide 9,
        // never on 2-8.
        let mut p = pres(10, vec![label(0, 3), label(3, 4)]);
        assert_eq!(move_frames_refusal(&p, &[1], 9, false), None);
        move_frame(&mut p, 1, 9, false);
        assert_eq!(p.objects.len(), 3, "one extra piece");
        assert_eq!(shows_on(&p, 0), vec![0, 1]);
        assert_eq!(shows_on(&p, 1), vec![9], "the piece, right after the original");
        assert_eq!(p.links, vec![vec![0, 1]], "the piece is linked to the original");
        assert_eq!(shows_on(&p, 2), vec![2], "the single-slide label just moves");
    }

    #[test]
    fn moving_a_foreign_slide_into_a_multi_slide_label_does_not_show_it_there() {
        // Slide 9 (frame 8) moves before slide 2: the title must skip it.
        let group: SceneObject = serde_json::from_str(r#"{"type":"group","members":[0]}"#).unwrap();
        let mut p = pres(10, vec![label(0, 3), group]);
        move_frame(&mut p, 8, 1, true);
        let mut shown: Vec<usize> = shows_on(&p, 0);
        shown.extend(shows_on(&p, 1));
        shown.sort_unstable();
        assert_eq!(shown, vec![0, 2, 3], "not on the moved-in frame 1");
        assert_eq!(members_of(&p.objects[2]), vec![0, 1], "the auto group holds the new piece");
    }

    #[test]
    fn a_torn_explicit_group_member_is_not_split() {
        // The group's 0..6 stays whole when frame 2 moves after frame 5, but its
        // member's own 0..3 is torn. The member is drawn on the group's range,
        // so it must not get a piece that would show on its own range.
        let group: SceneObject =
            serde_json::from_str(r#"{"type":"group","members":[0],"frames":{"start":0,"end":6}}"#).unwrap();
        let mut p = pres(10, vec![label(0, 3), group]);
        assert_eq!(move_frames_refusal(&p, &[2], 5, false), None);
        move_frame(&mut p, 2, 5, false);
        assert_eq!(p.objects.len(), 2, "no piece for an explicit group's member");
        assert_eq!(range(&p.objects[1]), (0, 6));
    }

    #[test]
    fn a_split_piece_joins_the_originals_existing_link_family() {
        let mut p = pres(10, vec![label(0, 3), label(5, 6)]);
        p.links = vec![vec![0, 1]];
        move_frame(&mut p, 1, 9, false);
        // [title, its piece, the linked label]: one family holding all three.
        assert_eq!(p.objects.len(), 3);
        assert_eq!(p.links.len(), 1, "no second family");
        let mut fam = p.links[0].clone();
        fam.sort_unstable();
        assert_eq!(fam, vec![0, 1, 2]);
    }

    #[test]
    fn tearing_a_morph_explicit_group_or_animated_object_is_refused() {
        let morph: SceneObject = serde_json::from_str(
            r#"{"type":"morph","position":{"x":{"fixed":0},"y":{"fixed":0}},"from":"AB","to":"XY",
                "frames":{"start":0,"end":3}}"#,
        )
        .unwrap();
        let why = move_frames_refusal(&pres(10, vec![morph]), &[1], 9, false).unwrap();
        assert!(why.contains("split the morph on slides 1-3"), "{why}");
        let group: SceneObject =
            serde_json::from_str(r#"{"type":"group","members":[0],"frames":{"start":0,"end":3}}"#).unwrap();
        assert!(move_frames_refusal(&pres(10, vec![label(5, 6), group]), &[1], 9, false)
            .unwrap()
            .contains("split the group"));
        let mut animated = label(0, 3);
        crate::editor::properties::set_coordinate(
            &mut animated,
            "x",
            Coordinate::Animated { from: 0, to: 9, anim: 1 },
        )
        .unwrap();
        assert!(move_frames_refusal(&pres(10, vec![animated]), &[1], 9, false)
            .unwrap()
            .contains("split the animated object"));
    }

    #[test]
    fn moving_frames_that_keep_every_span_whole_is_allowed() {
        let p = pres(10, vec![loop_obj(2, 5), loop_obj(6, 8), label(9, 10)]);
        // A slide outside any span, and a loop moved whole, both pass.
        assert_eq!(move_frames_refusal(&p, &[9], 0, true), None);
        assert_eq!(move_frames_refusal(&p, &[2, 3, 4], 9, false), None);
        // Reordering inside a loop keeps it whole too.
        assert_eq!(move_frames_refusal(&p, &[2], 4, false), None);
        let mut moved = p.clone();
        move_frames(&mut moved, &[9], 0, true);
        assert_eq!(loop_ranges(&moved), vec![(3, 6), (7, 9)]);
        assert!(moved.validate_loops().is_ok());
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

    // Build a single-frame label carrying `text`, used to tell originals from
    // clones in the range copy tests.
    fn tlabel(text: &str, start: usize, end: usize) -> SceneObject {
        let mut o = create_default(0, 0);
        if let SceneObject::Label(l) = &mut o {
            l.text = text.into();
            l.frames = FrameRange { start, end };
        }
        o
    }

    #[test]
    fn move_frames_relocates_a_contiguous_block_after_target() {
        // Deck 0..5; move block [1,2] to after frame 4 → order 0,3,4,1,2,5.
        let mut p = pres(6, (0..6).map(|f| label(f, f + 1)).collect());
        let new_index = move_frames(&mut p, &[1, 2], 4, false);
        assert_eq!(new_index, 3); // old frame 1 → index 3
        assert_eq!(range(&p.objects[1]), (3, 4)); // old frame 1
        assert_eq!(range(&p.objects[2]), (4, 5)); // old frame 2
        assert_eq!(range(&p.objects[3]), (1, 2)); // old frame 3 shifts left
        assert_eq!(range(&p.objects[4]), (2, 3)); // old frame 4 shifts left
        assert_eq!(p.frame_count, 6); // move never changes the count
    }

    #[test]
    fn move_frames_block_before_target() {
        // Deck 0..5; move block [3,4] before frame 1 → order 0,3,4,1,2,5.
        let mut p = pres(6, (0..6).map(|f| label(f, f + 1)).collect());
        let new_index = move_frames(&mut p, &[3, 4], 1, true);
        assert_eq!(new_index, 1);
        assert_eq!(range(&p.objects[3]), (1, 2));
        assert_eq!(range(&p.objects[4]), (2, 3));
        assert_eq!(range(&p.objects[1]), (3, 4)); // old frame 1 pushed right
    }

    #[test]
    fn move_frames_target_inside_block_is_a_noop() {
        let mut p = pres(6, (0..6).map(|f| label(f, f + 1)).collect());
        // Target 3 is within the moved block [2..4] → no move.
        let r = move_frames(&mut p, &[2, 3, 4], 3, false);
        assert_eq!(r, 2);
        for f in 0..6 {
            assert_eq!(range(&p.objects[f]), (f, f + 1)); // untouched
        }
    }

    #[test]
    fn move_frames_keeps_a_deck_wide_background_spanning() {
        let mut p = pres(5, vec![label(0, 5), label(1, 2), label(2, 3)]);
        move_frames(&mut p, &[1, 2], 4, false);
        assert_eq!(range(&p.objects[0]), (0, 5)); // background still spans the deck
    }

    #[test]
    fn inserting_frames_saturates_a_hand_edited_huge_range() {
        let mut p = pres(3, vec![label(1, usize::MAX)]);
        insert_blank_frame(&mut p, 0);
        assert_eq!(range(&p.objects[0]), (2, usize::MAX), "neither panics nor wraps");
        let mut p = pres(3, vec![label(usize::MAX - 1, usize::MAX)]);
        insert_blank_frames_at(&mut p, 0, 5);
        assert_eq!(range(&p.objects[0]), (usize::MAX, usize::MAX));
        // Moving a frame walks only the deck's frames of it.
        let mut q = pres(3, vec![label(1, usize::MAX)]);
        move_frame(&mut q, 0, 2, false);
        assert_eq!(range(&q.objects[0]), (0, 2)); // as before, without walking MAX frames
        // The 1-based display saturates too.
        let props = crate::editor::properties::get_properties(&p.objects, 0);
        let first = props.iter().find(|q| q.name == "first_frame").unwrap();
        assert_eq!(first.value, usize::MAX.to_string());
    }

    #[test]
    fn copy_frames_duplicates_a_block_after_target() {
        // Deck A,B,C,D; copy block [1,2] (B,C) after frame 3 → +2 frames at [4,6).
        let mut p = pres(4, vec![tlabel("A", 0, 1), tlabel("B", 1, 2), tlabel("C", 2, 3), tlabel("D", 3, 4)]);
        let (new_current, count) = copy_frames(&mut p, 1, 2, 3, false);
        assert_eq!((new_current, count), (4, 2));
        assert_eq!(p.frame_count, 6);
        // Originals unchanged.
        assert_eq!(range(&p.objects[1]), (1, 2)); // B
        assert_eq!(range(&p.objects[2]), (2, 3)); // C
        // Two new clones at the tail, on the new frames.
        let clones: Vec<_> = p.objects[4..].iter().filter_map(|o| match o {
            SceneObject::Label(l) => Some((l.text.clone(), range(o))),
            _ => None,
        }).collect();
        assert_eq!(clones, vec![("B".to_string(), (4, 5)), ("C".to_string(), (5, 6))]);
    }

    #[test]
    fn copy_frames_before_front_inserts_at_the_start() {
        let mut p = pres(4, vec![tlabel("A", 0, 1), tlabel("B", 1, 2), tlabel("C", 2, 3), tlabel("D", 3, 4)]);
        let (new_current, count) = copy_frames(&mut p, 1, 2, 0, true);
        assert_eq!((new_current, count), (0, 2));
        assert_eq!(p.frame_count, 6);
        // Originals all shifted right by 2.
        assert_eq!(range(&p.objects[0]), (2, 3)); // A
        assert_eq!(range(&p.objects[1]), (3, 4)); // B
        // Clones land on the new front frames 0 and 1.
        let clones: Vec<_> = p.objects[4..].iter().filter_map(|o| match o {
            SceneObject::Label(l) => Some((l.text.clone(), range(o))),
            _ => None,
        }).collect();
        assert_eq!(clones, vec![("B".to_string(), (0, 1)), ("C".to_string(), (1, 2))]);
    }

    #[test]
    fn copy_frames_keeps_an_interior_spanning_background_shared() {
        // Deck-wide bg + per-frame B,C. Copy [1,2] after frame 0 (dest strictly
        // inside the bg span) → the bg stretches over the new frames as ONE object,
        // not a duplicate; B and C are cloned.
        let mut p = pres(4, vec![tlabel("bg", 0, 4), tlabel("B", 1, 2), tlabel("C", 2, 3)]);
        let (new_current, count) = copy_frames(&mut p, 1, 2, 0, false);
        assert_eq!((new_current, count), (1, 2));
        assert_eq!(p.frame_count, 6);
        // The background is still a single object, now spanning the grown deck.
        let bgs: Vec<_> = p.objects.iter().filter(|o| matches!(o, SceneObject::Label(l) if l.text == "bg")).collect();
        assert_eq!(bgs.len(), 1, "background not duplicated");
        assert_eq!(range(bgs[0]), (0, 6));
        // B and C each have an original + a clone.
        let count_text = |t: &str| p.objects.iter().filter(|o| matches!(o, SceneObject::Label(l) if l.text == t)).count();
        assert_eq!(count_text("B"), 2);
        assert_eq!(count_text("C"), 2);
    }

    #[test]
    fn animation_span_unions_the_referenced_animations() {
        // x references animation 1 (span [5, 11)).
        let mut obj = create_default(0, 0); // Label, all Fixed
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
        }
        let anims = AnimSpans::from_pairs([(1, FrameRange { start: 5, end: 11 })]);
        assert_eq!(scene_object_animation_span(&obj, &anims), Some((5, 11)));

        // A second animation (2) starting earlier widens the union's start only.
        if let SceneObject::Label(l) = &mut obj {
            l.position.y = Coordinate::Animated { from: 0, to: 3, anim: 2 };
        }
        let anims = AnimSpans::from_pairs([
            (1, FrameRange { start: 5, end: 11 }),
            (2, FrameRange { start: 2, end: 9 }),
        ]);
        assert_eq!(scene_object_animation_span(&obj, &anims), Some((2, 11)));

        // No animated coordinate → no span (range is left untouched on apply).
        let plain = create_default(0, 0);
        assert_eq!(scene_object_animation_span(&plain, &anims), None);
    }

    #[test]
    fn add_frames_and_share_grows_the_deck_and_shares_elements() {
        // One frame, two local elements. Animate over 10 frames from frame 0:
        // insert 9 frames and extend both elements to span [0, 10).
        let mut p = pres(1, vec![label(0, 1), label(0, 1)]);
        add_frames_and_share(&mut p, 0, 0, 9, &[]); // current=0, start=0, end_frame=9
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
        add_frames_and_share(&mut p, 0, 0, 5, &[]); // span [0,6) ⇒ 5 new frames
        assert_eq!(p.frame_count, 9); // 4 existing + 5 fresh
        assert_eq!(range(&p.objects[0]), (0, 6)); // shared across the span
    }

    /// Label X animated x 0 → 30 by animation 1 over frames 0..3, plus a loop and
    /// an auto-advance marker on those frames.
    fn earlier_animation_deck() -> SourcePresentation {
        serde_json::from_str(
            r#"{ "width": 80, "height": 24, "frame_count": 4, "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "animated": { "from": 0, "to": 30, "anim": 1 } },
                                "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 4 } },
                { "type": "animation", "id": 1, "frames": { "start": 0, "end": 4 } },
                { "type": "loop", "frames": { "start": 0, "end": 4 } },
                { "type": "auto_advance", "frames": { "start": 3, "end": 4 } }
            ] }"#,
        )
        .unwrap()
    }

    /// X's x on each of `frames`.
    fn x_positions(p: &SourcePresentation, frames: std::ops::Range<usize>) -> Vec<u16> {
        let anims = AnimSpans::of(p);
        let SceneObject::Label(l) = &p.objects[0] else { panic!("object 0 is not a label") };
        frames.map(|f| l.position.x.evaluate(f, &anims)).collect()
    }

    /// The visible text of every frame of `p`, one string per frame.
    fn frame_texts(p: &SourcePresentation) -> Vec<String> {
        let contract = crate::types::TerminalContract { width: p.width, height: p.height };
        let played = crate::renderer::Renderer::render(&crate::engine::Engine::compile(p), contract);
        (0..p.frame_count)
            .map(|f| {
                played.grid_at(f).iter().map(|row| row.iter().map(|c| c.ch).collect::<String>() + "\n").collect()
            })
            .collect()
    }

    /// A ball → square wipe over frames 0..3, plus a label on frame 3.
    fn morph_ending_on_frame_3() -> SourcePresentation {
        serde_json::from_str(
            r#"{ "width": 12, "height": 4, "frame_count": 4, "objects": [
                { "type": "morph", "from": "oooo\noooo", "to": "XXXX\nXXXX", "mode": "wipe-right",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 4 } },
                { "type": "label", "text": "L",
                  "position": { "x": { "fixed": 8 }, "y": { "fixed": 3 } },
                  "frames": { "start": 3, "end": 4 } }
            ] }"#,
        )
        .unwrap()
    }

    #[test]
    fn add_frames_on_a_morphs_last_frame_keeps_its_blend() {
        // Animating the label from frame 3 (the morph's last) with add frames:
        // every morph slide renders as before, and the new frames hold its
        // finished state instead of re-timing the blend over them.
        let mut p = morph_ending_on_frame_3();
        let before = frame_texts(&p);
        add_frames_and_share(&mut p, 3, 3, 6, &[1]);
        let after = frame_texts(&p);
        assert_eq!(after[..4], before[..]);
        for f in 4..7 {
            assert_eq!(after[f], before[3], "frame {f} holds the finished morph");
        }
        assert_eq!(range(&p.objects[0]), (0, 4), "the morph keeps its timing");
    }

    #[test]
    fn a_morphs_frozen_continuation_is_not_driven_by_its_motion() {
        // The morph also moves (animation 1). Its frozen continuation must hold
        // still and not be driven by animation 1, so removing (or re-locking)
        // that animation never stretches it back over the morph's own slides.
        let mut p = morph_ending_on_frame_3();
        if let SceneObject::Morph(m) = &mut p.objects[0] {
            m.position.x = Coordinate::Animated { from: 0, to: 3, anim: 1 };
        }
        ensure_animation(&mut p, 1, 0, 4, true, 500, 0);
        let moved = add_frames_and_share(&mut p, 3, 3, 6, &[1]);
        assert_eq!(moved, vec![0, 2, 3], "the continuation sits right after the morph");
        let SceneObject::Morph(c) = &p.objects[1] else { panic!("no continuation") };
        assert!(matches!(c.position.x, Coordinate::Fixed(x) if x == 3.0), "held at x 3");
        remove_animation(&mut p, 1); // drops the Animation object after it
        assert!(matches!(&p.objects[1], SceneObject::Morph(m) if m.from == m.to));
        assert_eq!(range(&p.objects[1]), (4, 7), "continuation stays on the new frames");
    }

    #[test]
    fn a_morphs_continuation_keeps_its_draw_order() {
        // An equal-z label drawn over the morph (later in the list) must stay
        // over it on the held frames: the continuation goes right after the
        // morph, not to the end of the list.
        let mut p = morph_ending_on_frame_3();
        let over: SceneObject = serde_json::from_str(
            r#"{ "type": "label", "text": "LL",
                 "position": { "x": { "fixed": 1 }, "y": { "fixed": 0 } },
                 "frames": { "start": 0, "end": 4 } }"#,
        )
        .unwrap();
        p.objects.insert(1, over); // morph 0, LL 1, L 2
        let before = frame_texts(&p);
        let moved = add_frames_and_share(&mut p, 3, 3, 6, &[2]);
        assert_eq!(moved, vec![0, 2, 3]);
        let after = frame_texts(&p);
        assert_eq!(after[..4], before[..]);
        for f in 4..7 {
            assert_eq!(after[f], before[3], "frame {f} draws like the morph's last slide");
        }
    }

    #[test]
    fn add_frames_on_an_earlier_animations_last_frame_keeps_its_span() {
        // Animating the next thing from frame 3 (the earlier animation's last
        // frame) with add frames: the earlier Animation and the marker keep
        // their spans, X keeps its motion on frames 0..3 and holds its end
        // position on the three new frames. The loop grows so it still contains
        // the new animation wholly (the deck stays valid).
        let mut p = earlier_animation_deck();
        add_frames_and_share(&mut p, 3, 3, 6, &[]);
        ensure_animation(&mut p, 2, 3, 7, true, 500, 0); // what apply_animation records
        assert_eq!(p.frame_count, 7);
        assert_eq!(range(&p.objects[1]), (0, 4), "animation span unchanged");
        assert_eq!(range(&p.objects[2]), (0, 7), "loop grows over the new frames");
        assert_eq!(range(&p.objects[3]), (3, 4), "auto-advance marker unchanged");
        assert_eq!(p.validate_loops(), Ok(()));
        assert_eq!(range(&p.objects[0]), (0, 7), "X is shared over the new frames");
        assert_eq!(x_positions(&p, 0..7), vec![0, 10, 20, 30, 30, 30, 30]);
    }

    #[test]
    fn ensure_animation_updates_the_same_id_in_place() {
        // Animating X then Y of one object reuses the same id → one Animation,
        // updated in place (the second call can even change its span).
        let mut p = pres(10, vec![label(0, 10)]);
        ensure_animation(&mut p, 1, 0, 10, true, 500, 0);
        ensure_animation(&mut p, 1, 0, 8, false, 200, 0); // same id → update in place
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
        assert_eq!((anims[0].frames.start, anims[0].frames.end), (0, 8));
    }

    #[test]
    fn ensure_animation_appends_a_distinct_id() {
        let mut p = pres(10, vec![label(0, 10)]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        ensure_animation(&mut p, 2, 3, 9, true, 500, 0); // overlapping but distinct id
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
    fn parse_frame_selection_bounds_work_by_the_deck_size() {
        // A huge upper bound expands only to the deck's frames (no allocation of
        // the typed range, no overflow); a range starting past the deck is empty.
        assert_eq!(parse_frame_selection("1-18446744073709551615", 10), Ok((0..10).collect()));
        assert_eq!(parse_frame_selection("3-99999999999, 20-30", 5), Ok(vec![2, 3, 4]));
        assert!(parse_frame_selection("11-18446744073709551615", 10).is_err());
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
    fn unknown_fields_finds_dropped_keys_but_not_known_defaults() {
        let json = r#"{"width":10,"height":3,"frame_count":2,"title":"Q3","objects":[
            {"type":"label","text":"Hi","_comment":"note","styel":{"fg":"red"},
             "style":{"fg":"red","fgg":"blue"},"z_order":0,
             "position":{"x":{"fixed":0},"y":{"fixed":0}},"frames":{"start":0,"end":1}},
            {"type":"rect","position":{"x":{"fixed":0},"y":{"fixed":0}},"width":4,"height":2,
             "frames":{"start":0,"end":1}}]}"#;
        let raw: serde_json::Value = serde_json::from_str(json).unwrap();
        let source: SourcePresentation = serde_json::from_str(json).unwrap();
        let mut got = unknown_fields(&raw, &source);
        got.sort();
        // `z_order: 0` is a known field at its default (not written back), not unknown.
        assert_eq!(got, vec!["objects[0]._comment", "objects[0].styel", "objects[0].style.fgg", "title"]);
        // A deck the editor itself wrote has none.
        let saved = serde_json::to_value(&source).unwrap();
        assert!(unknown_fields(&saved, &source).is_empty());
        // A default repeated on many objects is probed once, never reported.
        let label = raw["objects"][0].clone();
        let mut repeated = raw.clone();
        let mut src = source.clone();
        for _ in 0..400 {
            repeated["objects"].as_array_mut().unwrap().push(label.clone());
            src.objects.push(source.objects[0].clone());
        }
        let got = unknown_fields(&repeated, &src);
        assert!(!got.iter().any(|p| p.ends_with("z_order")), "z_order: 0 is known");
        assert_eq!(got.len(), 4 + 400 * 3);
        // Many distinct junk keys on one object: all listed, probing capped.
        let mut many = raw.clone();
        for k in 0..5_000 {
            many["objects"][1][format!("junk{k}")] = serde_json::json!(k);
        }
        assert_eq!(unknown_fields(&many, &source).len(), 4 + 5_000);
    }

    #[test]
    fn opening_a_deck_with_unknown_fields_warns_on_open_and_first_save() {
        let path = crate::test_tmp("bs_unknown_fields_deck.json");
        std::fs::write(
            &path,
            r#"{"width":10,"height":3,"frame_count":1,"_comment":"keep me","objects":[]}"#,
        )
        .unwrap();
        let mut state = EditorState::open(&path).unwrap();
        let msg = state.status_message.clone().unwrap_or_default();
        assert!(msg.contains("unknown field(s) _comment will be dropped on save"), "got: {msg}");
        state.save().unwrap();
        assert!(state.status_message.as_deref().unwrap().contains("_comment were dropped"));
        state.save().unwrap();
        assert_eq!(state.status_message.as_deref(), Some("Saved"), "said once");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn opening_a_deck_with_a_duplicate_animation_id_warns() {
        let path = std::path::PathBuf::from(crate::test_tmp("bs_dup_anim_id.json"));
        let anim = r#"{"type":"animation","id":7,"frames":{"start":0,"end":1}}"#;
        std::fs::write(
            &path,
            format!(r#"{{"width":10,"height":3,"frame_count":2,"objects":[{anim},{anim}]}}"#),
        )
        .unwrap();
        let mut state = EditorState::open(path.to_str().unwrap()).unwrap();
        let msg = state.status_message.clone().unwrap_or_default();
        assert!(msg.contains("animation id 7 is used by more than one animation"), "got: {msg}");
        state.save().unwrap();
        assert!(state.status_message.as_deref().unwrap().contains("⚠"), "save must warn too");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_refuses_a_deck_that_would_not_load_back() {
        let path = crate::test_tmp("bs_save_nonfinite.json");
        let mut state = EditorState::open(&path).unwrap();
        let mut l = label(0, 1);
        crate::editor::properties::set_coordinate(&mut l, "x", Coordinate::Fixed(f64::INFINITY)).unwrap();
        state.source.objects.push(l);
        state.dirty = true;

        assert!(state.save().is_err(), "an infinite coordinate would save as null");
        assert!(state.dirty, "a failed save keeps the deck dirty");
        assert!(!std::path::Path::new(&path).exists(), "nothing written");
    }

    #[test]
    fn opening_an_over_limit_deck_warns_in_the_status_line() {
        let path = crate::test_tmp("bs_deck_warning_huge.json");
        let deck = r#"{ "width": 80, "height": 24, "frame_count": 100000000000, "objects": [] }"#;
        std::fs::write(&path, deck).unwrap();
        let state = EditorState::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let status = state.status_message.clone().unwrap_or_default();
        assert!(status.contains("frame_count"), "got: {status}");
        assert!(state.deck_warning().contains("frame_count"));
    }

    #[test]
    fn save_as_writes_the_file_and_adopts_the_path() {
        let target = crate::test_tmp("bs_save_as_unit_test.json");

        let mut state = EditorState::open(&crate::test_tmp("bs_save_as_orig_absent_99.json")).unwrap();
        state.save_as(&target).unwrap();

        assert_eq!(state.file_path, target); // adopted the new path
        assert!(std::path::Path::new(&target).exists());
        assert!(!state.dirty);
        // It's valid JSON for a SourcePresentation.
        let json = std::fs::read_to_string(&target).unwrap();
        let _: SourcePresentation = serde_json::from_str(&json).unwrap();
        let _ = std::fs::remove_file(&target);
    }

    #[test]
    fn failed_save_as_keeps_the_old_path() {
        let orig = crate::test_tmp("bs_failed_save_as_orig_absent.json");
        let mut state = EditorState::open(&orig).unwrap();
        state.dirty = true;
        // A regular file as the parent "directory" makes the write fail.
        let parent = std::path::PathBuf::from(crate::test_tmp("bs_save_as_parent"));
        std::fs::write(&parent, "").unwrap();
        let bad = parent.join("x.json");
        assert!(state.save_as(bad.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(&parent);
        assert_eq!(state.file_path, orig);
        assert!(state.dirty);
    }

    #[test]
    fn replace_file_failure_leaves_the_original_intact() {
        let dir = std::path::PathBuf::from(crate::test_tmp("bs_replace_fail"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("deck.json");
        std::fs::write(&path, "original").unwrap();
        // A write that fails part-way (e.g. a full disk).
        let r = replace_file(path.to_str().unwrap(), |f| {
            f.write_all(b"parti")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "temp file removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn replace_file_refuses_a_read_only_target() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::path::PathBuf::from(crate::test_tmp("bs_replace_ro"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("deck.json");
        std::fs::write(&path, "original").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        // Root may write a read-only file anyway; only check where the OS refuses.
        if std::fs::OpenOptions::new().write(true).open(&path).is_err() {
            assert!(replace_file(path.to_str().unwrap(), |f| f.write_all(b"new")).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temp file");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn save_replaces_content_through_a_symlink_keeping_permissions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = std::path::PathBuf::from(crate::test_tmp("bs_replace_ok"));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.json");
        let link = dir.join("link.json");
        std::fs::write(&real, "old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink("real.json", &link).unwrap();
        // A planted symlink on the first temp name must not redirect the write.
        let victim = dir.join("victim");
        std::fs::write(&victim, "keep").unwrap();
        let planted = dir.join(format!(".real.json.tmp-{}-0", std::process::id()));
        std::os::unix::fs::symlink("victim", &planted).unwrap();
        let ino = std::fs::metadata(&real).unwrap().ino();

        let mut state = EditorState::open(dir.join("absent.json").to_str().unwrap()).unwrap();
        state.file_path = link.to_str().unwrap().to_string();
        state.save().unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        let json = std::fs::read_to_string(&real).unwrap();
        let _: SourcePresentation = serde_json::from_str(&json).unwrap();
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        assert_ne!(std::fs::metadata(&real).unwrap().ino(), ino, "replaced, not rewritten");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 4, "no temp file left");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn deleting_an_animation_span_freezes_the_coordinates_it_drove() {
        // Label y 2→8 over anim 1 = [0,5), label wider (0..8); a second label on
        // 0..3 with x 4→9 over the one-frame anim 2 = [2,3).
        let y_of = |o: &SceneObject| match o {
            SceneObject::Label(l) => l.position.y.clone(),
            _ => panic!(),
        };
        let x_of = |o: &SceneObject| match o {
            SceneObject::Label(l) => l.position.x.clone(),
            _ => panic!(),
        };
        let mut a = label(0, 8);
        if let SceneObject::Label(l) = &mut a {
            l.position.y = Coordinate::Animated { from: 2, to: 8, anim: 1 };
        }
        let mut p = pres(8, vec![a]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        let before = y_of(&p.objects[0]).evaluate(5, &AnimSpans::of(&p));

        // Deleting the whole span: the frames that survive (after it) showed `to`.
        assert_eq!(delete_frames(&mut p, &[0, 1, 2, 3, 4]), 5);
        assert_eq!(p.objects.len(), 1, "collapsed animation pruned");
        assert!(matches!(y_of(&p.objects[0]), Coordinate::Fixed(v) if v == 8.0));
        assert_eq!(y_of(&p.objects[0]).evaluate(0, &AnimSpans::of(&p)), before);

        // Frames before a deleted one-frame span showed `from`.
        let mut b = label(0, 3);
        if let SceneObject::Label(l) = &mut b {
            l.position.x = Coordinate::Animated { from: 4, to: 9, anim: 2 };
        }
        let mut q = pres(3, vec![b]);
        ensure_animation(&mut q, 2, 2, 3, true, 500, 0);
        adjust_frames_after_delete(&mut q, 2);
        assert_eq!(q.objects.len(), 1);
        assert!(matches!(x_of(&q.objects[0]), Coordinate::Fixed(v) if v == 4.0));
    }

    #[test]
    fn next_anim_id_skips_an_id_still_referenced_by_a_coordinate() {
        // A stray reference to a missing animation 7 must not be re-bound.
        let mut a = label(0, 1);
        if let SceneObject::Label(l) = &mut a {
            l.position.x = Coordinate::Animated { from: 0, to: 5, anim: 7 };
        }
        assert_eq!(next_anim_id(&pres(1, vec![a])), 8);
    }

    #[test]
    fn set_frame_auto_advance_adds_replaces_and_removes() {
        let mut p = pres(3, vec![]);
        // Add on frame 1.
        set_frame_auto_advance(&mut p, 1, 5000);
        assert_eq!(frame_auto_advance_delay(&p, 1), Some(5000));
        assert_eq!(p.objects.iter().filter(|o| matches!(o, SceneObject::AutoAdvance(_))).count(), 1);

        // Re-setting the same frame replaces (no duplicate stacks up).
        set_frame_auto_advance(&mut p, 1, 2000);
        assert_eq!(frame_auto_advance_delay(&p, 1), Some(2000));
        assert_eq!(p.objects.iter().filter(|o| matches!(o, SceneObject::AutoAdvance(_))).count(), 1);

        // delay 0 turns it off (removes the marker).
        set_frame_auto_advance(&mut p, 1, 0);
        assert_eq!(frame_auto_advance_delay(&p, 1), None);
        assert!(!p.objects.iter().any(|o| matches!(o, SceneObject::AutoAdvance(_))));
    }

    #[test]
    fn set_frame_auto_advance_splits_a_wider_marker_around_the_frame() {
        let wide = || SceneObject::AutoAdvance(AutoAdvance { frames: FrameRange { start: 1, end: 4 }, delay_ms: 5000 });
        // Off in the middle: the frame stops, both neighbours keep 5 s.
        let mut p = pres(5, vec![wide()]);
        assert!(set_frame_auto_advance(&mut p, 2, 0).is_empty(), "split, nothing removed");
        assert_eq!(frame_auto_advance_delay(&p, 2), None);
        assert_eq!(frame_auto_advance_delay(&p, 1), Some(5000));
        assert_eq!(frame_auto_advance_delay(&p, 3), Some(5000));
        // A longer delay on an edge frame really is that delay now.
        let mut p = pres(5, vec![wide()]);
        set_frame_auto_advance(&mut p, 1, 9000);
        assert_eq!(frame_auto_advance_delay(&p, 1), Some(9000));
        assert_eq!(frame_auto_advance_delay(&p, 2), Some(5000));
        assert_eq!(frame_auto_advance_delay(&p, 0), None);
        // A one-frame marker under the frame is removed and reported.
        let mut p = pres(5, vec![label(0, 5), SceneObject::AutoAdvance(AutoAdvance {
            frames: FrameRange { start: 2, end: 3 }, delay_ms: 5000 })]);
        assert_eq!(set_frame_auto_advance(&mut p, 2, 0), vec![1]);
        assert_eq!(frame_auto_advance_delay(&p, 2), None);
    }

    #[test]
    fn deleting_one_sample_of_a_gap_strobe_deletes_the_element() {
        // Labels A (y 0) and B (y 1) both strobed by gapped animation 1 on
        // frames 0, 4, 8. Deleting a copy of A, or A's original, removes every
        // sample of A; B keeps its own, and a span edit brings none of A back.
        let strobed = |y: f64| {
            let mut l = create_default(0, 0);
            if let SceneObject::Label(l) = &mut l {
                l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
                l.position.y = Coordinate::Fixed(y);
            }
            l
        };
        let is_a = |o: &SceneObject| matches!(o, SceneObject::Label(l)
            if matches!(l.position.y, Coordinate::Fixed(y) if y == 0.0));
        let anim = Animation { id: 1, frames: FrameRange { start: 0, end: 9 },
            auto_play: true, delay_ms: 500, gap_frames: 3 };
        for pick_copy in [true, false] {
            let mut p = pres(10, vec![strobed(0.0), SceneObject::Animation(anim.clone()), strobed(1.0)]);
            let old = AnimSpans::of(&p);
            restrobe(&mut p, 0, &old);
            restrobe(&mut p, 2, &old);
            let a_samples: Vec<usize> = (0..p.objects.len()).filter(|&i| is_a(&p.objects[i])).collect();
            assert_eq!(a_samples.len(), 3);
            let target = if pick_copy { *a_samples.last().unwrap() } else { a_samples[0] };
            assert_eq!(delete_objects(&mut p, &[target]), 1);
            assert!(!p.objects.iter().any(is_a), "pick_copy={pick_copy}: no sample of A remains");

            // Extend the span: re-strobing B must not resurrect A.
            let old = AnimSpans::of(&p);
            for o in &mut p.objects {
                if let SceneObject::Animation(a) = o {
                    a.frames.end = 10;
                }
            }
            restrobe_driven(&mut p, 1, &old);
            assert!(!p.objects.iter().any(is_a), "pick_copy={pick_copy}: the span edit brings none back");
            assert!(p.objects.iter().any(|o| matches!(o, SceneObject::Animation(_))), "B still drives it");
        }
    }

    /// A 4-wide wipe-right morph "AAAA" -> "BBBB" over `frames`.
    fn wipe(start: usize, end: usize) -> SceneObject {
        serde_json::from_str(&format!(
            r#"{{"type":"morph","position":{{"x":{{"fixed":0}},"y":{{"fixed":0}}}},
                "from":"AAAA","to":"BBBB","mode":"wipe-right","frames":{{"start":{start},"end":{end}}}}}"#
        ))
        .unwrap()
    }

    /// The first row of every compiled frame, 4 cells wide.
    fn rows(p: &SourcePresentation) -> Vec<String> {
        let mut p = p.clone();
        p.width = 4;
        p.height = 1;
        let r = crate::compile::compile(&p).unwrap();
        (0..p.frame_count).map(|f| r.grid_at(f)[0].iter().map(|c| c.ch).collect()).collect()
    }

    #[test]
    fn copied_frames_show_a_morph_as_its_source_frames_did() {
        let old = rows(&pres(5, vec![wipe(0, 4)]));
        assert_eq!(old[..4], ["AAAA", "BBAA", "BBBA", "BBBB"]);

        // Duplicating the morph's last slide shows the finished `to`.
        let mut p = pres(5, vec![wipe(0, 4)]);
        copy_frame(&mut p, 3);
        assert_eq!(rows(&p)[4], "BBBB");

        // Overlaying a mid-blend slide onto a blank one matches it.
        let mut p = pres(5, vec![wipe(0, 4)]);
        overlay_frame(&mut p, 1, 4);
        assert_eq!(rows(&p)[4], old[1]);

        // A block cutting the morph (slides 3-4) copies those frames as shown...
        let mut p = pres(6, vec![wipe(0, 4)]);
        copy_frames(&mut p, 2, 3, 5, false);
        assert_eq!(rows(&p)[6..8], old[2..4]);

        // ...and so does a yank + paste of it into another deck.
        let clip = copy_frame_block(&pres(6, vec![wipe(0, 4)]), 2, 3);
        let mut other = pres(1, vec![]);
        paste_frame_block(&mut other, &clip, 0, false);
        assert_eq!(rows(&other)[1..3], old[2..4]);

        // Copying a block in front of the morph's middle splits it around the
        // copy; neither piece restarts or squeezes the blend.
        let mut p = pres(5, vec![wipe(1, 4)]);
        let before = rows(&p);
        copy_frames(&mut p, 0, 1, 2, false);
        let after = rows(&p);
        assert_eq!(after[..3], before[..3]);
        assert_eq!(after[3..5], before[..2]);
        assert_eq!(after[5], before[3]);
    }

    #[test]
    fn overlay_leaves_the_target_slides_auto_advance_alone() {
        let markers = |p: &SourcePresentation, f: usize| {
            p.objects.iter().filter(|o| matches!(o, SceneObject::AutoAdvance(a) if a.frames.contains(f))).count()
        };
        // Onto a slide without a marker: none is added.
        let mut p = pres(4, vec![label(1, 2)]);
        set_frame_auto_advance(&mut p, 1, 3000);
        overlay_frame(&mut p, 1, 3);
        assert_eq!(frame_auto_advance_delay(&p, 3), None);
        assert_eq!(markers(&p, 3), 0);
        // Onto a slide with its own 20 s marker: it keeps exactly that one.
        set_frame_auto_advance(&mut p, 2, 20000);
        overlay_frame(&mut p, 1, 2);
        assert_eq!(frame_auto_advance_delay(&p, 2), Some(20000));
        assert_eq!(markers(&p, 2), 1);
    }

    #[test]
    fn delete_objects_removes_a_set_and_fixes_group_member_indices() {
        // Objects: 0=A, 1=B, 2=C, 3=group(of A,C). Delete A and C (indices 0,2);
        // the group survives with its members re-pointed (B is now index 0).
        let mut a = label(0, 1); set_text(&mut a, "A");
        let mut b = label(0, 1); set_text(&mut b, "B");
        let mut c = label(0, 1); set_text(&mut c, "C");
        let mut p = pres(2, vec![a, b, c, group(vec![0, 2])]);

        let removed = delete_objects(&mut p, &[0, 2]);
        assert_eq!(removed, 2);
        // Survivors: B and the (now empty) group.
        let texts: Vec<String> = p.objects.iter().map(text).filter(|t| !t.is_empty()).collect();
        assert_eq!(texts, vec!["B"]);
        // The group dropped both deleted members (neither index points astray).
        let g = p.objects.iter().find(|o| matches!(o, SceneObject::Group(_))).unwrap();
        assert!(members_of(g).is_empty());
    }

    #[test]
    fn deleting_the_last_driver_of_an_animation_prunes_it() {
        // Two labels driven by animation 1: deleting one keeps the animation,
        // deleting the other (its last driver) removes it.
        let driven = |t: &str| {
            let mut o = label(0, 5);
            set_text(&mut o, t);
            if let SceneObject::Label(l) = &mut o {
                l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
            }
            o
        };
        let has_anim = |p: &SourcePresentation| p.objects.iter().any(|o| matches!(o, SceneObject::Animation(_)));
        let mut p = pres(5, vec![driven("a"), driven("b")]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        delete_objects(&mut p, &[0]);
        assert!(has_anim(&p), "still drives `b`");
        delete_objects(&mut p, &[0]);
        assert!(!has_anim(&p), "no driver left, so no auto-playing span either");

        // A frame delete that collapses the only driver prunes it the same way.
        let mut q = pres(5, vec![driven("c")]);
        if let Some(fr) = scene_object_frame_range_mut(&mut q.objects[0]) {
            *fr = FrameRange { start: 2, end: 3 };
        }
        ensure_animation(&mut q, 1, 0, 5, true, 500, 0);
        adjust_frames_after_delete(&mut q, 2);
        assert!(q.objects.is_empty(), "driver and its animation both gone: {:?}", q.objects);
    }

    #[test]
    fn delete_objects_special_cases_an_animation_in_the_set() {
        // A label whose x animates over [0,5) plus its Animation sidecar, and a
        // second plain label. Deleting both the animated label and the Animation
        // removes the sidecar via `remove_animation` (no orphan left behind).
        let mut anim_label = create_default(0, 0);
        if let SceneObject::Label(l) = &mut anim_label {
            l.text = "moving".into();
            l.position.x = Coordinate::Animated { from: 2, to: 12, anim: 1 };
            l.frames = FrameRange { start: 0, end: 5 };
        }
        let mut plain = label(0, 5); set_text(&mut plain, "plain");
        // objects: 0=moving (anim), 1=plain, 2=Animation sidecar
        let mut p = pres(5, vec![anim_label, plain]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        assert_eq!(p.objects.len(), 3);

        // Delete the animated label (0) and the Animation sidecar (2).
        let anim_idx = p.objects.iter().position(|o| matches!(o, SceneObject::Animation(_))).unwrap();
        let removed = delete_objects(&mut p, &[0, anim_idx]);
        assert_eq!(removed, 2);
        // Only the plain label survives; no Animation sidecar dangles.
        assert!(!p.objects.iter().any(|o| matches!(o, SceneObject::Animation(_))));
        let texts: Vec<String> = p.objects.iter().map(text).filter(|t| !t.is_empty()).collect();
        assert_eq!(texts, vec!["plain"]);
    }

    #[test]
    fn auto_advance_marker_shifts_with_frame_insert_and_delete() {
        let mut p = pres(3, vec![]);
        set_frame_auto_advance(&mut p, 1, 5000); // marker on frame 1 ([1,2))

        // Inserting a blank frame before it (after frame 0) shifts it to frame 2.
        insert_blank_frame(&mut p, 0);
        assert_eq!(frame_auto_advance_delay(&p, 2), Some(5000));
        assert_eq!(frame_auto_advance_delay(&p, 1), None);

        // Deleting the frame the marker sits on collapses its range and prunes it.
        adjust_frames_after_delete(&mut p, 2);
        assert!(!p.objects.iter().any(|o| matches!(o, SceneObject::AutoAdvance(_))));
    }

    // --- cross-deck frame clipboard (copy_frame_block / paste_frame_block) ----

    /// A Label on `[start, end)` whose x is animated by `anim` from `from`→`to`.
    fn animated_label(start: usize, end: usize, anim: AnimId, from: u16, to: u16) -> SceneObject {
        let mut obj = label(start, end);
        if let SceneObject::Label(l) = &mut obj {
            l.position.x = Coordinate::Animated { from, to, anim };
        }
        obj
    }

    #[test]
    fn copy_frame_block_normalises_ranges_to_block_local() {
        // frames 0..6: a deck-wide background (0..6) and a mid-deck label (2..5).
        let mut bg = label(0, 6);
        set_text(&mut bg, "bg");
        let mut mid = label(2, 5);
        set_text(&mut mid, "mid");
        let p = pres(6, vec![bg, mid]);

        // Copy block [2, 4] (inclusive) → 3 frames; ranges clip into 0..3.
        let clip = copy_frame_block(&p, 2, 4);
        assert_eq!(clip.frame_count, 3);
        assert_eq!(clip.objects.len(), 2);
        let got: Vec<(String, (usize, usize))> =
            clip.objects.iter().map(|o| (text(o), range(o))).collect();
        assert!(got.contains(&("bg".to_string(), (0, 3)))); // 0..6 ∩ 2..5 → local 0..3
        assert!(got.contains(&("mid".to_string(), (0, 3)))); // 2..5 ∩ 2..5 → local 0..3
    }

    #[test]
    fn copy_frame_block_skips_objects_outside_the_block() {
        // A label only on frame 5 is not captured by a [0,1] copy.
        let p = pres(6, vec![label(0, 1), label(5, 6)]);
        let clip = copy_frame_block(&p, 0, 1);
        assert_eq!(clip.objects.len(), 1);
    }

    #[test]
    fn paste_frame_block_inserts_frames_and_shifts_ranges() {
        let src = pres(2, vec![label(0, 2)]);
        let clip = copy_frame_block(&src, 0, 1); // whole deck, 2 frames

        let mut tgt = pres(3, vec![label(0, 3)]); // a 3-frame deck w/ a bg
        let (new_current, count) = paste_frame_block(&mut tgt, &clip, 0, false); // after frame 0
        assert_eq!(count, 2);
        assert_eq!(new_current, 1); // first frame of the pasted block
        assert_eq!(tgt.frame_count, 5);
        // The pasted label lands on the new block [1, 3). The target's bg is
        // split around it (the clipboard brings its own slides' content): it
        // keeps frame 0 and continues, linked, on 3..5 right after itself.
        let pasted = tgt.objects.last().unwrap();
        assert_eq!(range(pasted), (1, 3));
        assert_eq!(range(&tgt.objects[0]), (0, 1));
        assert_eq!(range(&tgt.objects[1]), (3, 5));
        assert_eq!(tgt.links, vec![vec![0, 1]]);
    }

    /// A wipe Morph on own frames 2..6, member of an explicit group on 0..`group_end`.
    fn grouped_morph_deck(group_end: usize) -> SourcePresentation {
        grouped_morph_deck_in(0, group_end)
    }

    /// A wipe Morph on own frames 2..6, member of an explicit group on
    /// `group_start..group_end`.
    fn grouped_morph_deck_in(group_start: usize, group_end: usize) -> SourcePresentation {
        serde_json::from_str(&format!(
            r#"{{ "width": 8, "height": 2, "frame_count": 10, "objects": [
                {{ "type": "morph", "from": "oooooo", "to": "XXXXXX", "mode": "wipe-right",
                   "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }},
                   "frames": {{ "start": 2, "end": 6 }} }},
                {{ "type": "group", "members": [0], "frames": {{ "start": {group_start}, "end": {group_end} }} }}
            ] }}"#
        ))
        .unwrap()
    }

    /// Revert every explicit group in `p` to auto.
    fn groups_to_auto(p: &mut SourcePresentation) {
        for obj in &mut p.objects {
            if let SceneObject::Group(g) = obj {
                g.frames = None;
            }
        }
    }

    #[test]
    fn splitting_an_explicit_group_keeps_its_member_morphs_blend() {
        // Paste a 3-frame block into the group (and the morph's own range) at 4:
        // every original slide renders as before, and with the group reverted
        // to auto the morph pieces show only on their original slides.
        let mut p = grouped_morph_deck(10);
        let before = frame_texts(&p);
        let mut auto_before = p.clone();
        groups_to_auto(&mut auto_before);
        let auto_before = frame_texts(&auto_before);
        let src = pres(3, vec![]);
        paste_frame_block(&mut p, &copy_frame_block(&src, 0, 2), 3, false);
        let after = frame_texts(&p);
        assert_eq!(after[..4], before[..4]);
        assert_eq!(after[7..], before[4..]);
        groups_to_auto(&mut p);
        let auto_after = frame_texts(&p);
        assert_eq!(auto_after[..4], auto_before[..4]);
        assert_eq!(auto_after[7..], auto_before[4..]);
        assert!(auto_after[4..7].iter().all(|t| t.trim().is_empty()), "nothing on the pasted slides");

        // The same for a block copy the group does not wholly cover; the
        // copied slides show what frames 3..5 showed.
        let mut p = grouped_morph_deck(5);
        let before = frame_texts(&p);
        copy_frames(&mut p, 3, 5, 3, false); // frames 3..5 copied in after 3
        let after = frame_texts(&p);
        assert_eq!(after[..4], before[..4]);
        assert_eq!(after[4..7], before[3..6]);
        assert_eq!(after[7..], before[4..]);
    }

    #[test]
    fn an_unsplit_explicit_group_keeps_its_member_morphs_blend() {
        // The morph's own range (2..6) crosses the insert point 4, but its group
        // lies wholly before it (0..4) or wholly after it (4..10), so the group
        // is not split: the member's blend must still keep its stages there.
        for (gs, ge) in [(0, 4), (4, 10)] {
            let mut p = grouped_morph_deck_in(gs, ge);
            let before = frame_texts(&p);
            paste_frame_block(&mut p, &copy_frame_block(&pres(3, vec![]), 0, 2), 3, false);
            let after = frame_texts(&p);
            assert_eq!(after[..4], before[..4], "group {gs}..{ge}: before the paste point");
            assert_eq!(after[7..], before[4..], "group {gs}..{ge}: after the pasted frames");

            let mut p = grouped_morph_deck_in(gs, ge);
            copy_frames(&mut p, 0, 1, 3, false); // frames 0..1 copied in after 3
            let after = frame_texts(&p);
            assert_eq!(after[..4], before[..4], "copy, group {gs}..{ge}");
            assert_eq!(after[6..], before[4..], "copy, group {gs}..{ge}");
        }
    }

    #[test]
    fn a_pasted_frame_block_is_not_covered_by_a_target_auto_advance_marker() {
        // A marker on frames 2..4; pasting two frames after frame 2 must not
        // make the pasted slides auto-advance: the marker is split around them
        // (as a block copy does), covering frames 2 and 5 only.
        let mut tgt: SourcePresentation = serde_json::from_str(
            r#"{ "width": 4, "height": 1, "frame_count": 6, "objects": [
                { "type": "auto_advance", "frames": { "start": 2, "end": 4 }, "delay_ms": 1000 }
            ] }"#,
        )
        .unwrap();
        let src = pres(2, vec![]);
        paste_frame_block(&mut tgt, &copy_frame_block(&src, 0, 1), 2, false);
        let covered: Vec<usize> =
            (0..tgt.frame_count).filter(|&f| frame_auto_advance_delay(&tgt, f).is_some()).collect();
        assert_eq!(covered, vec![2, 5]);
    }

    #[test]
    fn a_pasted_frame_block_does_not_show_target_objects_crossing_the_paste_point() {
        // Target: a label on frames 1..4 (a build). Pasting a one-frame block
        // after frame 2 (into the label's middle) must not draw the label on the
        // pasted slide; its own frames render as before.
        let mut tgt: SourcePresentation = serde_json::from_str(
            r#"{ "width": 10, "height": 2, "frame_count": 6, "objects": [
                { "type": "label", "text": "TGT",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 1, "end": 5 } }
            ] }"#,
        )
        .unwrap();
        let src: SourcePresentation = serde_json::from_str(
            r#"{ "width": 10, "height": 2, "frame_count": 1, "objects": [
                { "type": "label", "text": "src",
                  "position": { "x": { "fixed": 5 }, "y": { "fixed": 1 } },
                  "frames": { "start": 0, "end": 1 } }
            ] }"#,
        )
        .unwrap();
        let before = frame_texts(&tgt);
        let src_slide = frame_texts(&src)[0].clone();
        let clip = copy_frame_block(&src, 0, 0);
        assert_eq!(paste_frame_block(&mut tgt, &clip, 2, false), (3, 1));
        let after = frame_texts(&tgt);
        assert_eq!(after[3], src_slide, "the pasted slide looks like its source");
        assert_eq!(after[..3], before[..3]);
        assert_eq!(after[4..], before[3..]);
    }

    #[test]
    fn paste_frame_block_assigns_fresh_anim_ids_with_no_collision() {
        // Source: animation id=1 over [0,2) driving a label's x (3→9).
        let mut src = pres(2, vec![animated_label(0, 2, 1, 3, 9)]);
        ensure_animation(&mut src, 1, 0, 2, true, 500, 0);
        let clip = copy_frame_block(&src, 0, 1);

        // Target ALSO has an animation id=1 (same id) — the collision case.
        let mut tgt = pres(1, vec![animated_label(0, 1, 1, 0, 0)]);
        ensure_animation(&mut tgt, 1, 0, 1, true, 500, 0);

        paste_frame_block(&mut tgt, &clip, 0, false);

        // Two distinct animations now exist; the original keeps id 1.
        let ids: Vec<AnimId> = tgt
            .objects
            .iter()
            .filter_map(|o| match o {
                SceneObject::Animation(a) => Some(a.id),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&1));
        let fresh = *ids.iter().find(|&&id| id != 1).expect("a fresh id");
        // The pasted label (last Label) references the fresh id, not the old 1.
        let pasted_label = tgt
            .objects
            .iter()
            .rev()
            .find(|o| matches!(o, SceneObject::Label(_)))
            .unwrap();
        assert_eq!(referenced_anim_ids(pasted_label), vec![fresh]);
        // The original label still references id 1.
        assert_eq!(referenced_anim_ids(&tgt.objects[0]), vec![1]);
    }

    #[test]
    fn paste_frame_block_repoints_group_members_into_destination() {
        // Source: label0, label1, group(0,1) all on frame 0.
        let src = pres(1, vec![label(0, 1), label(0, 1), group(vec![0, 1])]);
        let clip = copy_frame_block(&src, 0, 0);
        // In the clipboard the group's members are block-local.
        let g = clip.objects.iter().find(|o| matches!(o, SceneObject::Group(_))).unwrap();
        assert_eq!(members_of(g), vec![0, 1]);

        // Paste into a deck that already has one object, so the block lands at
        // indices 1,2,3 and the group must point at 1,2 (not 0,1).
        let mut tgt = pres(1, vec![label(0, 1)]);
        paste_frame_block(&mut tgt, &clip, 0, false);
        let g2 = tgt.objects.iter().find(|o| matches!(o, SceneObject::Group(_))).unwrap();
        assert_eq!(members_of(g2), vec![1, 2]);
    }

    #[test]
    fn copy_frame_block_flattens_animation_outside_the_block() {
        // A label is visible on [0,4) but its driving animation spans only [0,2).
        // Copying frame 3 alone captures the label but NOT the animation object,
        // so the dangling coordinate must be flattened to a Fixed.
        let mut src = pres(4, vec![animated_label(0, 4, 1, 0, 30)]);
        ensure_animation(&mut src, 1, 0, 2, true, 500, 0);
        let clip = copy_frame_block(&src, 3, 3);
        // Only the label travels (the animation's span is outside the block)...
        assert_eq!(clip.objects.len(), 1);
        // ...and it keeps no animated coordinate (it was flattened).
        assert!(referenced_anim_ids(&clip.objects[0]).is_empty());
    }

    // --- block copies reproduce the original frames (R4-2) --------------------

    fn grid(p: &SourcePresentation, frame: usize) -> Vec<Vec<crate::types::Cell>> {
        let contract = crate::types::TerminalContract { width: p.width, height: p.height };
        let scenes = crate::engine::Engine::compile(p);
        crate::renderer::Renderer::render(&scenes, contract).grid_at(frame)
    }

    /// 10 frames: label "a" moves x 0→45 under animation 1 over every frame;
    /// label "b" (visible throughout) moves x 0→40 under animation 2 on [7,9).
    /// Each step of "a" is a whole 5 cells, so a clipped copy reproduces it
    /// exactly (in general the frames between a cut's ends may round to a
    /// neighbouring cell).
    fn cut_deck() -> SourcePresentation {
        let mut a = animated_label(0, 10, 1, 0, 45);
        set_text(&mut a, "a");
        let mut b = animated_label(0, 10, 2, 0, 40);
        set_text(&mut b, "b");
        if let SceneObject::Label(l) = &mut b {
            l.position.y = Coordinate::Fixed(5.0);
        }
        let mut p = pres(10, vec![a, b]);
        ensure_animation(&mut p, 1, 0, 10, true, 500, 0);
        ensure_animation(&mut p, 2, 7, 9, true, 500, 0);
        p
    }

    #[test]
    fn copy_frames_through_a_cut_animation_reproduces_the_frames() {
        let orig = cut_deck();
        let mut p = orig.clone();
        // Block [3,5] cuts animation 1; animation 2 lies after it, so the copy
        // placed after frame 9 must not pick up its later motion.
        assert_eq!(copy_frames(&mut p, 3, 5, 9, false), (10, 3));
        for f in 0..3 {
            assert_eq!(grid(&p, 10 + f), grid(&orig, 3 + f), "copy of frame {}", 3 + f);
        }
        // A one-frame cut copies no Animation; the coordinate is flattened.
        let mut q = orig.clone();
        copy_frames(&mut q, 9, 9, 9, false);
        assert_eq!(grid(&q, 10), grid(&orig, 9));
        let anims = q.objects.iter().filter(|o| matches!(o, SceneObject::Animation(_))).count();
        assert_eq!(anims, 2);
    }

    #[test]
    fn copy_frames_keeps_a_seam_straddling_animated_object_shared() {
        // A label on [0,10) moving under animation 1 on [0,5); copying [1,2]
        // after frame 7 inserts inside the label's range. The label stays one
        // stretched object (its own timing, so the copied frames show its end
        // position — the documented limitation), and the cut animation that no
        // clone drives is not copied as an orphan.
        let mut p = pres(10, vec![animated_label(0, 10, 1, 0, 20)]);
        ensure_animation(&mut p, 1, 0, 5, true, 500, 0);
        copy_frames(&mut p, 1, 2, 7, false);
        assert_eq!(p.objects.len(), 2);
        assert_eq!(range(&p.objects[0]), (0, 12));
    }

    /// Labels `(text, y, start, end)` on a 6-frame deck.
    fn reveal_deck(items: &[(&str, u16, usize, usize)]) -> SourcePresentation {
        pres(6, items.iter().map(|&(t, y, a, b)| {
            let mut o = tlabel(t, a, b);
            if let SceneObject::Label(l) = &mut o {
                l.position.y = Coordinate::Fixed(f64::from(y));
            }
            o
        }).collect())
    }

    /// `p` is `orig` with `orig`'s frames `[lo, lo + count)` copied to `dest`.
    fn assert_copied(p: &SourcePresentation, orig: &SourcePresentation, lo: usize, count: usize, dest: usize) {
        assert_eq!(p.frame_count, orig.frame_count + count);
        for f in 0..p.frame_count {
            let from = match f {
                f if f < dest => f,
                f if f < dest + count => lo + f - dest,
                f => f - count,
            };
            assert_eq!(grid(p, f), grid(orig, from), "frame {f} should show original frame {from}");
        }
    }

    #[test]
    fn copy_frames_splits_a_reveal_straddling_the_seam() {
        // Title on every slide, bullets revealed on slides 1 and 2 (in an auto
        // group). Copying [0,2] right after itself: the bullets straddle the
        // seam but don't cover the block, so the copy of slide 0 shows neither.
        // "cv", drawn after b1 at the same cell, must still cover its pieces.
        let mut orig = reveal_deck(&[("b1", 2, 1, 6), ("cv", 2, 0, 6), ("title", 0, 0, 6), ("b2", 4, 2, 6)]);
        orig.objects.push(group(vec![0, 3]));
        let mut p = orig.clone();
        assert_eq!(copy_frames(&mut p, 0, 2, 2, false), (3, 3));
        assert_copied(&p, &orig, 0, 3, 3);
        // b1's continuation is linked to it and joins its group.
        let cont = p.link_siblings(0)[0];
        assert_eq!(range(&p.objects[cont]), (6, 9));
        let g = p.objects.iter().find(|o| matches!(o, SceneObject::Group(_))).unwrap();
        assert!(members_of(g).contains(&cont));
    }

    #[test]
    fn copy_frames_split_tolerates_a_stale_group_member() {
        let mut p = reveal_deck(&[("b1", 2, 1, 6)]);
        p.objects.push(group(vec![0, 99]));
        copy_frames(&mut p, 0, 2, 2, false);
        let g = p.objects.iter().find(|o| matches!(o, SceneObject::Group(_))).unwrap();
        assert!(members_of(g).contains(&99));
    }

    #[test]
    fn copy_frames_splits_a_straddler_before_the_target() {
        // "x" ends inside the block [2,4]; "y" misses it but straddles the seam.
        let orig = reveal_deck(&[("title", 0, 0, 6), ("x", 2, 0, 3), ("y", 4, 0, 2), ("z", 6, 3, 6)]);
        let mut p = orig.clone();
        assert_eq!(copy_frames(&mut p, 2, 4, 1, true), (1, 3));
        assert_copied(&p, &orig, 2, 3, 1);
    }

    #[test]
    fn frame_block_paste_through_a_cut_animation_reproduces_the_frames() {
        let src = cut_deck();
        let mut tgt = pres(1, vec![]);
        paste_frame_block(&mut tgt, &copy_frame_block(&src, 3, 5), 0, false);
        for f in 0..3 {
            assert_eq!(grid(&tgt, 1 + f), grid(&src, 3 + f), "paste of frame {}", 3 + f);
        }
        let clip = copy_frame_block(&src, 9, 9);
        assert!(!clip.objects.iter().any(|o| matches!(o, SceneObject::Animation(_))));
        let mut tgt = pres(1, vec![]);
        paste_frame_block(&mut tgt, &clip, 0, false);
        assert_eq!(grid(&tgt, 1), grid(&src, 9));
    }

    // --- frame delete goes by the shown range of explicit-group members (R4-4)

    /// Labels "A" (y 1) and "B" (y 3) on their own range `own`, in a group whose
    /// explicit range is `explicit`, on an 8-frame deck.
    fn two_member_group_deck(own: (usize, usize), explicit: (usize, usize)) -> SourcePresentation {
        let mut a = tlabel("A", own.0, own.1);
        let mut b = tlabel("B", own.0, own.1);
        for (o, y) in [(&mut a, 1.0), (&mut b, 3.0)] {
            if let SceneObject::Label(l) = o {
                l.position.y = Coordinate::Fixed(y);
            }
        }
        let mut g = group(vec![0, 1]);
        if let SceneObject::Group(grp) = &mut g {
            grp.frames = Some(FrameRange { start: explicit.0, end: explicit.1 });
        }
        pres(8, vec![a, b, g])
    }

    /// Delete `deleted` and check every surviving slide renders as before.
    fn assert_delete_keeps_other_slides(before: &SourcePresentation, deleted: usize) {
        let mut after = before.clone();
        adjust_frames_after_delete(&mut after, deleted);
        let old: Vec<usize> = (0..before.frame_count).filter(|&f| f != deleted).collect();
        for (new, &f) in old.iter().enumerate() {
            assert_eq!(grid(&after, new), grid(before, f), "old slide {f}");
        }
    }

    #[test]
    fn frame_delete_keeps_members_an_explicit_group_widens() {
        // A, B on frame 5 only, widened by their group to 3..8: deleting slide 5
        // keeps them (and the group) on the other slides.
        let p = two_member_group_deck((5, 6), (3, 8));
        assert_delete_keeps_other_slides(&p, 5);
        let mut q = p.clone();
        adjust_frames_after_delete(&mut q, 5);
        assert_eq!(q.objects.len(), 3);
        assert_eq!(members_of(&q.objects[2]), vec![0, 1]);
    }

    #[test]
    fn frame_delete_drops_members_an_explicit_group_narrowed() {
        // A, B on 0..8, narrowed by their group to slide 5: deleting slide 5
        // drops them with the group instead of showing them everywhere.
        let p = two_member_group_deck((0, 8), (5, 6));
        assert_delete_keeps_other_slides(&p, 5);
    }

    #[test]
    fn frame_delete_ignores_an_outer_groups_range_on_a_nested_group() {
        // A (own 2..3) widened to the whole deck by inner group 0..8, which sits
        // in an outer group narrowed to slide 5. Deleting slide 5 prunes only the
        // outer group; the inner one still shows A everywhere.
        let mut p = two_member_group_deck((2, 3), (0, 8));
        let mut outer = group(vec![2]);
        if let SceneObject::Group(grp) = &mut outer {
            grp.frames = Some(FrameRange { start: 5, end: 6 });
        }
        p.objects.push(outer);
        assert_delete_keeps_other_slides(&p, 5);
    }
}
