use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;

use crate::engine::objects::table::{table_add_column, table_remove_column};
use crate::engine::objects::Group;
use crate::engine::source::{AnimId, AnimSpans, Coordinate, SceneObject, SourcePresentation};
use crate::types::Style;
use super::config::matches_binding;
use super::object_defaults;
use super::properties;
use super::textedit::{TextAction, TextEdit};
use super::state::{
    adjust_frames_after_delete, copy_frame,
    frame_auto_advance_delay, insert_blank_frame, move_frame, overlay_frame, ArtPick,
    ConfirmAction, EditorState, Mode, MultiSelectPurpose, TableCellSubState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Continue,
    Redraw,
    BlinkSelection,
    Quit,
    ToggleFullscreen,
    /// Switch the active presentation to the deck at this index (from the
    /// presentations switcher). Interpreted by the `Editor`, which owns the decks.
    SwitchDeck(usize),
    /// Open the file at this path as a new deck (or focus it if already open).
    OpenDeck(String),
    /// Save the active deck to this path. The `Editor` checks it against the
    /// other open decks and existing files first.
    SaveAs(String),
    /// Capture the contiguous frame block `[lo, hi]` of the active deck into the
    /// cross-deck frame clipboard (owned by the `Editor`).
    CopyFrameBlock { lo: usize, hi: usize },
    /// Paste the cross-deck frame clipboard into the active deck, dropping the
    /// block before/after `target`.
    PasteFrameBlock { target: usize, before: bool },
}

/// Whether the current mode is actively capturing typed characters into a text
/// buffer. When true, a plain-letter global shortcut (like fullscreen on "f")
/// must yield to text input so the letter can be typed literally.
fn mode_accepts_text(mode: &Mode) -> bool {
    match mode {
        Mode::EditProperties { editing_value, .. } => editing_value.is_some(),
        Mode::EditMultiProperties { editing_value, .. } => editing_value.is_some(),
        Mode::AnimateProperty { editing, .. } | Mode::ConvergeConfig { editing, .. } => editing.is_some(),
        Mode::Settings { .. }
        | Mode::TableAddColumn { .. }
        | Mode::TableRemoveColumn { .. }
        | Mode::LoadArtFile { .. }
        | Mode::SaveAs { .. }
        | Mode::OpenFile { .. }
        | Mode::FrameJump { .. }
        | Mode::FrameSelectInput { .. }
        | Mode::FrameAutoInput { .. } => true,
        Mode::TableEditCellProps { sub_state, .. } => match sub_state {
            TableCellSubState::EditingContent { .. } => true,
            TableCellSubState::EditingStyle { editing_value, .. } => editing_value.is_some(),
            TableCellSubState::Selecting => false,
        },
        _ => false,
    }
}

pub fn handle_event(state: &mut EditorState, event: Event) -> Action {
    match event {
        // Act on presses only (`term::key_press`), so each press fires once.
        Event::Key(_) => match crate::term::key_press(&event) {
            Some(key) => handle_key(state, key),
            None => Action::Continue,
        },
        Event::Resize(_, _) => Action::Redraw,
        Event::Paste(text) => handle_paste(state, &text),
        _ => Action::Continue,
    }
}

/// A bracketed paste: insert it at the active text field's cursor
/// ([`TextEdit::insert_str`]) — line breaks kept only in the multi-line fields
/// (a `Text` property, table cell content), dropped elsewhere. Outside a text
/// field it is ignored, so its characters are never read as commands.
fn handle_paste(state: &mut EditorState, text: &str) -> Action {
    if !mode_accepts_text(&state.mode) {
        return Action::Continue;
    }
    // Multi-line, and how to re-derive an in-panel field's scrolls afterwards.
    let (multiline, panel_field) = match &state.mode {
        Mode::EditProperties { object_index, selected_property, .. } => {
            let props = properties::get_properties(&state.source.objects, *object_index);
            match props.get(*selected_property) {
                Some(p) if p.kind == properties::PropertyKind::Text => (true, None),
                Some(p) => (false, Some(p.name.chars().count() + 2)),
                None => return Action::Continue,
            }
        }
        Mode::EditMultiProperties { members, selected_property, .. } => {
            let props = properties::common_properties(&state.source.objects, members);
            match props.get(*selected_property) {
                Some(p) => (false, Some(p.name.chars().count() + 2)),
                None => return Action::Continue,
            }
        }
        Mode::TableEditCellProps { sub_state: TableCellSubState::EditingContent { .. }, .. } => (true, None),
        _ => (false, None),
    };
    // The digit-only fields keep only the digits, as typing does (Settings
    // also its 4-digit cap).
    let digit_cap = match &state.mode {
        Mode::Settings { .. } => Some(4),
        Mode::TableAddColumn { .. } | Mode::TableRemoveColumn { .. } => Some(usize::MAX),
        _ => None,
    };
    let field: Option<(&mut String, &mut usize)> = match &mut state.mode {
        Mode::EditProperties { editing_value: Some(b), cursor, .. }
        | Mode::EditMultiProperties { editing_value: Some(b), cursor, .. }
        | Mode::AnimateProperty { editing: Some(b), cursor, .. }
        | Mode::ConvergeConfig { editing: Some(b), cursor, .. }
        | Mode::TableAddColumn { buf: b, cursor, .. }
        | Mode::TableRemoveColumn { buf: b, cursor, .. }
        | Mode::LoadArtFile { buf: b, cursor, .. }
        | Mode::SaveAs { buf: b, cursor }
        | Mode::OpenFile { buf: b, cursor }
        | Mode::FrameJump { buf: b, cursor }
        | Mode::FrameSelectInput { buf: b, cursor }
        | Mode::FrameAutoInput { buf: b, cursor } => Some((b, cursor)),
        Mode::Settings { selected_field, width_buf, height_buf, cursor } => {
            Some((if *selected_field == 0 { width_buf } else { height_buf }, cursor))
        }
        Mode::TableEditCellProps { sub_state, .. } => match sub_state {
            TableCellSubState::EditingContent { buf: b, cursor, .. }
            | TableCellSubState::EditingStyle { editing_value: Some(b), cursor, .. } => Some((b, cursor)),
            _ => None,
        },
        _ => None,
    };
    let Some((buf, cursor)) = field else { return Action::Continue };
    let text: String = match digit_cap {
        Some(cap) => text
            .chars()
            .filter(char::is_ascii_digit)
            .take(cap.saturating_sub(buf.chars().count()))
            .collect(),
        None => text.to_string(),
    };
    let mut te = TextEdit::new(std::mem::take(buf), *cursor);
    te.insert_str(&text, multiline);
    *cursor = te.cursor;
    *buf = te.buf.clone();
    // A table column field acts on `col_num`, kept in step with the digits.
    if let Mode::TableAddColumn { col_num, buf, .. } | Mode::TableRemoveColumn { col_num, buf, .. } =
        &mut state.mode
    {
        if let Ok(n) = buf.parse::<usize>() {
            *col_num = n;
        }
    }
    if let Some(prefix0) = panel_field {
        if let Mode::EditProperties { selected_property, scroll, panel_scroll, .. }
        | Mode::EditMultiProperties { selected_property, scroll, panel_scroll, .. } = &mut state.mode
        {
            (*scroll, *panel_scroll) = panel_field_scrolls(&te, *selected_property, prefix0, *scroll, *panel_scroll);
        }
    }
    Action::Redraw
}

/// Handle one key press, keeping gap-strobe sets whole: when the key edits an
/// object that belongs to a strobe set (typically a copy, the only member
/// selectable on a later sample frame), the edit is mirrored onto the rest of
/// the set. Structural edits (that add/remove objects, like animate `[s]`) are
/// left to their own handlers, which act on the set's original.
fn handle_key(state: &mut EditorState, key: KeyEvent) -> Action {
    let len = state.source.objects.len();
    let linked_table = linked_table_edit(&state.mode, &state.source);
    let status_before = state.status_message.clone();
    let targets = mode_targets(&state.mode, &state.source);
    let sets: Vec<(usize, String, Vec<usize>)> = targets
        .iter()
        .copied()
        .filter(|&t| t < len)
        .map(|t| (t, super::state::strobe_set(&state.source, t)))
        .filter(|(_, set)| set.len() > 1)
        .map(|(t, set)| (t, super::state::gap_clone_key(&state.source.objects[t]), set))
        .collect();
    // The sync is only sound for a pure in-place edit: every object outside the
    // targets must be untouched. Anything else (a span edit re-strobing,
    // removing and re-adding objects) restructured the deck, so the recorded
    // indices can't be trusted and the restructuring code kept the sets whole.
    if sets.is_empty() {
        let action = dispatch_key(state, key);
        sync_linked_tables(state, linked_table, len, &status_before);
        return action;
    }
    let snapshot: Vec<String> = state.source.objects.iter().map(super::state::gap_clone_key).collect();
    let action = dispatch_key(state, key);
    // Before the strobe sync, so a linked table that is itself strobed gets its
    // whole set synced too (it is among the targets).
    sync_linked_tables(state, linked_table, len, &status_before);
    let in_place = state.source.objects.len() == len
        && (0..len).all(|i| {
            targets.contains(&i) || super::state::gap_clone_key(&state.source.objects[i]) == snapshot[i]
        });
    if in_place {
        for (t, before, set) in sets {
            if super::state::gap_clone_key(&state.source.objects[t]) != before {
                // Only members still holding the pre-edit content: if the edit
                // shuffled objects (same count, other indices), a stale slot that
                // now holds something else is left alone.
                let set: Vec<usize> = set
                    .into_iter()
                    .filter(|&m| super::state::gap_clone_key(&state.source.objects[m]) == before)
                    .collect();
                super::state::sync_strobe_set(&mut state.source, t, &set);
            }
        }
    }
    action
}

/// A table's content: what its own editing modes change (cells, column
/// layout, rows), as opposed to the props-panel fields `apply_property` sets.
fn table_content(obj: &SceneObject) -> Option<String> {
    match obj {
        SceneObject::Table(t) => serde_json::to_string(&(&t.cells, &t.col_widths, t.rows)).ok(),
        _ => None,
    }
}

/// A planned linked-table sync: the edited table, its content before the key,
/// the linked tables that held that same content (in sync), and how many linked
/// tables held something else.
type LinkedTableEdit = (usize, String, Vec<usize>, usize);

/// For a table-content mode (cell text/styles, add/remove column): the table,
/// its content before the key and its link family (`link_family_of`), split
/// into the tables [`sync_linked_tables`] copies the content onto and the ones
/// that had already diverged.
fn linked_table_edit(mode: &Mode, source: &SourcePresentation) -> Option<LinkedTableEdit> {
    let t = match mode {
        Mode::TableAddColumn { object_index, .. }
        | Mode::TableEditCellProps { object_index, .. }
        | Mode::Confirm { action: ConfirmAction::RemoveTableColumn { object_index, .. }, .. } => *object_index,
        _ => return None,
    };
    let before = table_content(source.objects.get(t)?)?;
    let tables: Vec<usize> = link_family_of(source, t)
        .into_iter()
        .filter(|&j| matches!(source.objects.get(j), Some(SceneObject::Table(_))))
        .collect();
    if tables.is_empty() {
        return None;
    }
    let (same, diverged): (Vec<usize>, Vec<usize>) = tables
        .into_iter()
        .partition(|&j| table_content(&source.objects[j]).as_ref() == Some(&before));
    Some((t, before, same, diverged.len()))
}

/// Linked tables share their content like any other non-placement property:
/// after a key changed the edited table's cells or columns in place, copy them
/// onto every linked table that held the same content before, so a later
/// `col_N_width` edit lands on the same column everywhere. A linked table whose
/// content already differed (linked before tables synced) is left alone, and
/// the status line says so, rather than overwriting its cells.
/// `status_before` is the status line before the key: the note is appended only
/// to a message this key set, so it never piles up across edits.
fn sync_linked_tables(
    state: &mut EditorState,
    plan: Option<LinkedTableEdit>,
    len: usize,
    status_before: &Option<String>,
) {
    let Some((t, before, same, diverged)) = plan else { return };
    if state.source.objects.len() != len {
        return;
    }
    let (cells, col_widths, rows) = match &state.source.objects[t] {
        SceneObject::Table(tb) if table_content(&state.source.objects[t]).as_ref() != Some(&before) => {
            (tb.cells.clone(), tb.col_widths.clone(), tb.rows)
        }
        _ => return,
    };
    for j in same {
        if let Some(SceneObject::Table(s)) = state.source.objects.get_mut(j) {
            s.cells = cells.clone();
            s.col_widths = col_widths.clone();
            s.rows = rows;
        }
    }
    if diverged > 0 {
        let note = format!("{diverged} linked table(s) differ and were not synced");
        let fresh = state.status_message != *status_before;
        state.status_message = Some(match state.status_message.take() {
            Some(m) if fresh => format!("{m} — {note}"),
            _ => note,
        });
    }
}

/// The objects a mode edits in place (for [`handle_key`]'s strobe sync),
/// including the leaves of a targeted `Group` — moving or resizing a group
/// edits its members — and the link siblings of every one of them (and of a
/// strobe copy's original), which a non-placement edit propagates to
/// (`apply_property`).
fn mode_targets(mode: &Mode, source: &SourcePresentation) -> Vec<usize> {
    let mut targets = match mode {
        Mode::SelectedObject { object_index }
        | Mode::ResizeObject { object_index }
        | Mode::EditProperties { object_index, .. }
        | Mode::TableAddColumn { object_index, .. }
        | Mode::TableRemoveColumn { object_index, .. }
        | Mode::TableEditCellProps { object_index, .. }
        | Mode::Confirm { action: ConfirmAction::RemoveTableColumn { object_index, .. }, .. } => {
            vec![*object_index]
        }
        Mode::EditMultiProperties { members, .. } => members.clone(),
        _ => Vec::new(),
    };
    let leaves = properties::group_leaves(&source.objects, &targets);
    targets.extend(leaves);
    let siblings: Vec<usize> = targets
        .iter()
        .filter(|&&t| t < source.objects.len())
        .flat_map(|&t| link_family_of(source, t))
        .collect();
    targets.extend(siblings);
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn dispatch_key(state: &mut EditorState, key: KeyEvent) -> Action {
    // Global shortcut: works from any mode *except* while a text field is being
    // typed into — otherwise a plain-letter binding (e.g. "f") would be swallowed
    // by the fullscreen toggle instead of inserting the character.
    if !mode_accepts_text(&state.mode) {
        if matches_binding(&state.config.key_bindings.fullscreen, &key) {
            return Action::ToggleFullscreen;
        }
        // While in fullscreen, Esc leaves "no bars" mode (instead of its usual
        // per-mode cancel) so the user can always get the bars back.
        if state.fullscreen && key.code == KeyCode::Esc {
            return Action::ToggleFullscreen;
        }
    }

    match &state.mode {
        Mode::Normal => handle_normal(state, key),
        Mode::SaveAs { .. } => handle_save_as(state, key),
        Mode::FrameMenu => handle_frame_menu(state, key),
        Mode::FrameJump { .. } => handle_frame_jump(state, key),
        Mode::FrameSelectInput { .. } => handle_frame_select_input(state, key),
        Mode::FrameAutoInput { .. } => handle_frame_auto_input(state, key),
        Mode::FrameSelected { .. } => handle_frame_selected(state, key),
        Mode::FrameRangePlace { .. } => handle_frame_range_place(state, key),
        Mode::FrameMove { .. } => handle_frame_move(state, key),
        Mode::FrameMovePlace { .. } => handle_frame_move_place(state, key),
        Mode::FrameOverlay { .. } => handle_frame_overlay(state, key),
        Mode::FramePastePlace => handle_frame_paste_place(state, key),
        Mode::PresentationMenu { .. } => handle_presentation_menu(state, key),
        Mode::OpenFile { .. } => handle_open_file(state, key),
        Mode::AddObject { .. } => handle_add_object(state, key),
        Mode::SelectAction { .. } => handle_select_action(state, key),
        Mode::SelectedObject { .. } => handle_selected_object(state, key),
        Mode::ResizeObject { .. } => handle_resize_object(state, key),
        Mode::EditProperties { editing_value, dropdown, .. } => {
            let has_dropdown = dropdown.is_some();
            let is_editing = editing_value.is_some();
            if has_dropdown {
                handle_dropdown(state, key)
            } else if is_editing {
                handle_edit_value(state, key)
            } else {
                handle_edit_properties(state, key)
            }
        }
        Mode::EditMultiProperties { editing_value, dropdown, .. } => {
            let has_dropdown = dropdown.is_some();
            let is_editing = editing_value.is_some();
            if has_dropdown {
                handle_edit_multi_dropdown(state, key)
            } else if is_editing {
                handle_edit_multi_value(state, key)
            } else {
                handle_edit_multi_properties(state, key)
            }
        }
        Mode::AnimateProperty { .. } => handle_animate_property(state, key),
        Mode::ConvergeConfig { .. } => handle_converge_config(state, key),
        Mode::Confirm { .. } => handle_confirm(state, key),
        Mode::MultiSelect { .. } => handle_multi_select(state, key),
        Mode::PastePlacing { .. } => handle_paste_placing(state, key),
        Mode::TableAddColumn { .. } => handle_table_add_column(state, key),
        Mode::TableRemoveColumn { .. } => handle_table_remove_column(state, key),
        Mode::TableEditCellProps { sub_state, .. } => {
            match sub_state {
                TableCellSubState::Selecting => handle_table_cell_selecting(state, key),
                TableCellSubState::EditingContent { .. } => handle_table_cell_edit_content(state, key),
                TableCellSubState::EditingStyle { editing_value, dropdown, .. } => {
                    let has_dd = dropdown.is_some();
                    let is_ed = editing_value.is_some();
                    if has_dd {
                        handle_table_cell_style_dropdown(state, key)
                    } else if is_ed {
                        handle_table_cell_style_edit_value(state, key)
                    } else {
                        handle_table_cell_style_props(state, key)
                    }
                }
            }
        }
        Mode::AddArt { .. } => handle_add_art(state, key),
        Mode::LoadArtFile { .. } => handle_load_art_file(state, key),
        Mode::Settings { .. } => handle_settings(state, key),
    }
}

// ---------------------------------------------------------------------------
// Mode::EditProperties transition helpers
//
// The variant has seven fields but transitions only ever vary a couple of them,
// so building the literal by hand (as this file used to, ~15 times) is noisy and
// made it easy to reset a field — e.g. `panel_scroll` — that should have been
// preserved. These constructors centralise the defaults.
// ---------------------------------------------------------------------------

/// Browsing the property list (no value being edited, no dropdown open).
fn ep_browse(object_index: usize, selected_property: usize, panel_scroll: usize) -> Mode {
    Mode::EditProperties {
        object_index,
        selected_property,
        editing_value: None,
        cursor: 0,
        scroll: 0,
        panel_scroll,
        dropdown: None,
    }
}

/// Editing a property's value as text.
fn ep_editing(
    object_index: usize,
    selected_property: usize,
    buf: String,
    cursor: usize,
    scroll: usize,
    panel_scroll: usize,
) -> Mode {
    Mode::EditProperties {
        object_index,
        selected_property,
        editing_value: Some(buf),
        cursor,
        scroll,
        panel_scroll,
        dropdown: None,
    }
}

/// Showing an open dropdown for the selected property.
fn ep_dropdown(
    object_index: usize,
    selected_property: usize,
    dd_sel: usize,
    panel_scroll: usize,
) -> Mode {
    Mode::EditProperties {
        object_index,
        selected_property,
        editing_value: None,
        cursor: 0,
        scroll: 0,
        panel_scroll,
        dropdown: Some(dd_sel),
    }
}

// ---------------------------------------------------------------------------
// Mode::EditMultiProperties transition helpers — the multi-object counterparts
// of the `ep_*` constructors above (same sub-state, but a `members` set instead
// of a single object index).
// ---------------------------------------------------------------------------

fn emp_browse(members: Vec<usize>, selected_property: usize, panel_scroll: usize) -> Mode {
    Mode::EditMultiProperties {
        members,
        selected_property,
        editing_value: None,
        cursor: 0,
        scroll: 0,
        panel_scroll,
        dropdown: None,
    }
}

fn emp_editing(
    members: Vec<usize>,
    selected_property: usize,
    buf: String,
    cursor: usize,
    scroll: usize,
    panel_scroll: usize,
) -> Mode {
    Mode::EditMultiProperties {
        members,
        selected_property,
        editing_value: Some(buf),
        cursor,
        scroll,
        panel_scroll,
        dropdown: None,
    }
}

fn emp_dropdown(
    members: Vec<usize>,
    selected_property: usize,
    dd_sel: usize,
    panel_scroll: usize,
) -> Mode {
    Mode::EditMultiProperties {
        members,
        selected_property,
        editing_value: None,
        cursor: 0,
        scroll: 0,
        panel_scroll,
        dropdown: Some(dd_sel),
    }
}

/// The `SelectAction` row index of the "Edit Props" action — used to restore the
/// action sub-menu's highlight when Esc leaves the multi-edit panel.
fn edit_props_action_index() -> usize {
    SELECT_ACTIONS
        .iter()
        .position(|(k, _)| *k == SelectActionKind::EditProps)
        .unwrap_or(0)
}

/// Vertical scroll so the property row at `selected_row` stays on screen. Mirrors
/// the panel layout: menu(1) + timeline(2) + title(1) + separator(1) = 5 reserved.
fn follow_panel_scroll(selected_row: usize, panel_scroll: usize, term_h: usize) -> usize {
    let avail = term_h.saturating_sub(5);
    if selected_row < panel_scroll {
        selected_row
    } else if avail > 0 && selected_row >= panel_scroll + avail {
        selected_row + 1 - avail
    } else {
        panel_scroll
    }
}

/// Apply a property edit and report it on the status line. Shared by the toggle,
/// dropdown, and text-entry paths so they format success/errors identically.
/// Returns the indices of objects it removed (gap-strobe copies re-made after an
/// `Animation` span edit), in removal order — see `index_after_removals`.
#[must_use]
fn apply_property(state: &mut EditorState, object_index: usize, name: &str, value: &str) -> Vec<usize> {
    // A group's frame range is optional ("auto", derived from members). Editing
    // first_frame/last_frame transitions it:
    //   * blank value  -> revert to auto (no override of member ranges)
    //   * any value    -> materialise an explicit range (seeded from the derived
    //                     union) that then overrides member ranges
    let is_group = matches!(state.source.objects.get(object_index), Some(SceneObject::Group(_)));
    let frame_field = name == "first_frame" || name == "last_frame";
    // (An `Animation`'s span has its own, stricter check below.)
    let is_anim = matches!(state.source.objects.get(object_index), Some(SceneObject::Animation(_)));
    if frame_field && !is_anim && !(is_group && value.trim().is_empty()) {
        if let Some(e) = frame_edit_error(&state.source, object_index, name, value) {
            state.status_message = Some(format!("Error: {e}"));
            return Vec::new();
        }
    }
    if is_group && frame_field {
        if value.trim().is_empty() {
            if let SceneObject::Group(g) = &mut state.source.objects[object_index] {
                g.frames = None;
            }
            state.dirty = true;
            state.status_message = Some("Group range: auto (from members)".into());
            return Vec::new();
        }
        let derived = state.source.effective_frame_range(object_index);
        // An explicit range would override a gap-strobed direct member's samples
        // (the override is one level deep, like `member_overrides`).
        if let SceneObject::Group(g) = &state.source.objects[object_index] {
            if g.frames.is_none()
                && g.members.iter().any(|&m| {
                    m < state.source.objects.len()
                        && !matches!(state.source.objects[m], SceneObject::Animation(_))
                        && super::state::is_gap_strobed(&state.source, m)
                })
            {
                state.status_message = Some(format!(
                    "Error: {}",
                    super::state::STROBE_GROUP_ERROR
                ));
                return Vec::new();
            }
        }
        if let SceneObject::Group(g) = &mut state.source.objects[object_index] {
            if g.frames.is_none() {
                // Check the value on a probe first: a rejected one must leave
                // the group auto, not materialise the members' union range.
                let mut probe = g.clone();
                probe.frames = Some(derived.clone());
                if let Err(e) = properties::set_property(&mut SceneObject::Group(probe), name, value) {
                    state.status_message = Some(format!("Error: {e}"));
                    return Vec::new();
                }
                g.frames = Some(derived);
            }
        }
    }
    // Editing an `Animation`'s span (its first/last frame — the single source of
    // truth) re-locks the visibility range of every object it drives, re-making
    // a gap strobe over the new span; the old spans locate the old strobe copies.
    let span_edit = match state.source.objects.get(object_index) {
        Some(SceneObject::Animation(a)) if name == "first_frame" || name == "last_frame" => {
            // Check the new span on a copy first, under the Animate menu's rules
            // (`check_anim_span`): a span past the deck would strobe a gapped
            // element once per sample frame of it, unbounded.
            let mut probe = SceneObject::Animation(a.clone());
            if properties::set_property(&mut probe, name, value).is_ok() {
                if let SceneObject::Animation(p) = &probe {
                    let fr = &p.frames;
                    let check = if fr.start >= fr.end {
                        Err("an animation must span at least one frame".to_string())
                    } else if fr.end > state.source.frame_count {
                        // (`check_anim_span` skips a one-frame span.)
                        Err(format!(
                            "last frame {} is past the deck's last frame {}",
                            fr.end, state.source.frame_count
                        ))
                    } else if p.gap_frames > 0 {
                        // Re-strobing a member of an explicit group would only
                        // re-make copies the group's range overrides.
                        let driven: Vec<usize> = (0..state.source.objects.len())
                            .filter(|&i| super::state::referenced_anim_ids(&state.source.objects[i]).contains(&p.id))
                            .collect();
                        match super::state::strobe_group_error(&state.source, &driven) {
                            Some(e) => Err(e),
                            None => check_anim_span(&state.source, state.current_frame, fr.start, fr.end - 1, false),
                        }
                    } else {
                        check_anim_span(&state.source, state.current_frame, fr.start, fr.end - 1, false)
                    };
                    if let Err(e) = check {
                        state.status_message = Some(format!("Error: {e}"));
                        return Vec::new();
                    }
                }
            }
            Some((a.id, AnimSpans::of(&state.source)))
        }
        _ => None,
    };
    // Before the edit changes its content: a strobe copy's link family is its
    // original's.
    let siblings = link_family_of(&state.source, object_index);
    let mut object_index = object_index;
    let mut removed = Vec::new();
    match properties::set_property(&mut state.source.objects[object_index], name, value) {
        Ok(()) => {
            state.dirty = true;
            if let Some((id, old)) = span_edit {
                removed = super::state::restrobe_driven(&mut state.source, id, &old);
                state.note_removed_objects(&removed);
                object_index = super::state::index_after_removals(object_index, &removed);
            }
            // Linked objects share appearance: a non-placement edit propagates to
            // the family (placement/layering stays per-object). Siblings that lack
            // the property (a mixed-type family) simply ignore it.
            let mut propagated = 0usize;
            if !is_placement_prop(&state.source.objects, object_index, name) {
                // (A span edit, the only one that removes objects, is a
                // placement edit, so `siblings` never went stale here.)
                for j in siblings {
                    if properties::set_property(&mut state.source.objects[j], name, value).is_ok() {
                        propagated += 1;
                    }
                }
            }
            // Surface a loop overlap/range error live (the compile step enforces
            // it hard); otherwise confirm the edit.
            state.status_message = Some(match state.source.validate_loops() {
                Ok(()) => {
                    if propagated > 0 {
                        format!("Set {name} = {value} (+{propagated} linked)")
                    } else {
                        format!("Set {name} = {value}")
                    }
                }
                Err(e) => format!("⚠ {e}"),
            });
        }
        Err(e) => state.status_message = Some(format!("Error: {e}")),
    }
    removed
}

/// Why a typed `first_frame`/`last_frame` of the object at `index` is refused:
/// the typed slide must lie on the deck (1..=frame_count) and the range must not
/// come out empty. Only the edited end is checked, so a hand-edited range past
/// the deck can still have its other end edited. `None` for a value
/// `set_property` itself rejects (it reports that error).
fn frame_edit_error(source: &SourcePresentation, index: usize, name: &str, value: &str) -> Option<String> {
    let mut probe = source.objects.get(index)?.clone();
    if let SceneObject::Group(g) = &mut probe {
        g.frames.get_or_insert_with(|| source.effective_frame_range(index));
    }
    properties::set_property(&mut probe, name, value).ok()?;
    let fr = super::state::scene_object_frame_range(&probe)?;
    let n = source.frame_count;
    let typed = if name == "first_frame" { fr.start.saturating_add(1) } else { fr.end };
    if typed == 0 || typed > n {
        return Some(format!("{name} must be a slide from 1 to {n}"));
    }
    (fr.start >= fr.end).then(|| "first_frame must not be after last_frame".to_string())
}

/// The link siblings a non-placement edit of `index` propagates to. A gap-strobe
/// copy belongs to no family itself — only its set's original was linked — so
/// it uses the original's (the set's first entry: copies are always appended
/// after it, see `state::strobe_set`), keeping a family whole when the edit is
/// made on a later sample frame (where only a copy is selectable).
fn link_family_of(source: &SourcePresentation, index: usize) -> Vec<usize> {
    let original = super::state::strobe_set(source, index)[0];
    let mut family = source.link_siblings(index);
    if original != index {
        family.extend(source.link_siblings(original).into_iter().filter(|&j| j != index));
    }
    family.sort_unstable();
    family.dedup();
    family
}

/// Apply one property edit to **every** member of a multi-object selection,
/// reusing the single-object `apply_property` per member (so group auto-range,
/// animation re-locking, link propagation, and loop validation all behave
/// exactly as they do for a single edit). Members lacking the property simply
/// report an error and are skipped; a final summary replaces the per-member
/// status. Returns the members' indices afterwards (an edit can remove objects,
/// see `apply_property`); a removed member is dropped.
fn apply_multi_property(state: &mut EditorState, members: &[usize], name: &str, value: &str) -> Vec<usize> {
    let mut ok = 0usize;
    let mut tried = 0usize;
    let mut last_err: Option<String> = None;
    let mut members: Vec<Option<usize>> = members.iter().map(|&m| Some(m)).collect();
    for k in 0..members.len() {
        let Some(m) = members[k] else { continue };
        tried += 1;
        let removed = apply_property(state, m, name, value);
        for &r in &removed {
            for slot in &mut members {
                *slot = slot.filter(|&i| i != r).map(|i| super::state::index_after_removals(i, &[r]));
            }
        }
        // `apply_property` reports through the status line; sniff its result so
        // the summary can distinguish a fully-applied edit from a partial one.
        match state.status_message.as_deref() {
            Some(s) if s.starts_with('⚠') || s.starts_with("Error") => last_err = Some(s.to_string()),
            _ => ok += 1,
        }
    }
    state.status_message = Some(match last_err {
        Some(e) if ok == 0 => e,
        Some(_) => format!("Set {name} = {value} on {ok}/{tried} objects"),
        None => format!("Set {name} = {value} on {ok} objects"),
    });
    members.into_iter().flatten().collect()
}

/// Build an Art object from `item`, append it, and jump to editing it.
fn add_art_item(state: &mut EditorState, art: String, name: String) {
    let obj = object_defaults::create_art(art, name.clone(), state.current_frame);
    state.source.objects.push(obj);
    state.dirty = true;
    let new_index = state.source.objects.len() - 1;
    state.mode = Mode::EditProperties {
        object_index: new_index,
        selected_property: 0,
        editing_value: None,
        cursor: 0,
        scroll: 0,
        panel_scroll: 0,
        dropdown: None,
    };
    state.status_message = Some(format!("Added art: {name}"));
}

/// Route a chosen art piece according to the picker's `purpose`: add it as a
/// standalone `Art`, capture it as a morph's *from* (and re-open the picker for
/// the *to* piece), or finish a morph with the chosen *to* piece.
fn route_picked_art(state: &mut EditorState, art: String, name: String, purpose: ArtPick) {
    match purpose {
        ArtPick::Art => add_art_item(state, art, name),
        ArtPick::MorphFrom => {
            // Re-open the library to pick the target piece for the morph.
            state.mode = Mode::AddArt {
                selected: 0,
                items: crate::art_library::all_items(),
                purpose: ArtPick::MorphTo { from_art: art, from_name: name },
            };
            state.status_message = Some("Morph: now pick the target art".into());
        }
        ArtPick::MorphTo { from_art, from_name } => {
            let obj = object_defaults::create_morph(
                from_art,
                from_name.clone(),
                art,
                name.clone(),
                state.current_frame,
            );
            state.source.objects.push(obj);
            state.dirty = true;
            let new_index = state.source.objects.len() - 1;
            state.mode = ep_browse(new_index, 0, 0);
            state.status_message = Some(format!("Added morph: {from_name}→{name}"));
        }
    }
}

fn handle_add_art(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (selected, items, purpose) = match &state.mode {
        Mode::AddArt { selected, items, purpose } => (*selected, items.clone(), purpose.clone()),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::AddObject { selected: 0 };
        return Action::Redraw;
    }

    // Entries: one per library item, plus a final "Load from file…" action.
    let entry_count = items.len() + 1;

    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { entry_count - 1 } else { selected - 1 };
        state.mode = Mode::AddArt { selected: new_sel, items, purpose };
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % entry_count;
        state.mode = Mode::AddArt { selected: new_sel, items, purpose };
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        if selected < items.len() {
            let item = items[selected].clone();
            route_picked_art(state, item.art, item.name, purpose);
        } else {
            // "Load from file…" entry — carry the purpose through.
            state.mode = Mode::LoadArtFile { buf: String::new(), cursor: 0, purpose };
        }
        return Action::Redraw;
    }

    Action::Continue
}

fn handle_load_art_file(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (mut buf, mut cursor, purpose) = match &state.mode {
        Mode::LoadArtFile { buf, cursor, purpose } => (buf.clone(), *cursor, purpose.clone()),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::AddArt {
            selected: 0,
            items: crate::art_library::all_items(),
            purpose,
        };
        return Action::Redraw;
    }

    if matches_binding(&bindings.confirm, &key) {
        let path = buf.trim();
        if path.is_empty() {
            state.status_message = Some("Enter a file path".into());
            return Action::Redraw;
        }
        match crate::art_library::load_file(std::path::Path::new(path)) {
            Ok(item) => route_picked_art(state, item.art, item.name, purpose),
            Err(e) => {
                // Stay in the input so the path can be corrected.
                state.status_message = Some(format!("Load failed: {e}"));
                state.mode = Mode::LoadArtFile { buf, cursor, purpose };
            }
        }
        return Action::Redraw;
    }

    match key.code {
        KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
            let byte_idx = char_to_byte_idx(&buf, cursor);
            buf.insert(byte_idx, c);
            cursor += 1;
            state.mode = Mode::LoadArtFile { buf, cursor, purpose };
            return Action::Redraw;
        }
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
            if cursor > 0 {
                let byte_idx = char_to_byte_idx(&buf, cursor - 1);
                buf.remove(byte_idx);
                cursor -= 1;
            }
            state.mode = Mode::LoadArtFile { buf, cursor, purpose };
            return Action::Redraw;
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            if cursor > 0 {
                cursor -= 1;
            }
            state.mode = Mode::LoadArtFile { buf, cursor, purpose };
            return Action::Redraw;
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            if cursor < buf.chars().count() {
                cursor += 1;
            }
            state.mode = Mode::LoadArtFile { buf, cursor, purpose };
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

fn handle_settings(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (mut selected_field, mut width_buf, mut height_buf, mut cursor) = match &state.mode {
        Mode::Settings { selected_field, width_buf, height_buf, cursor } => {
            (*selected_field, width_buf.clone(), height_buf.clone(), *cursor)
        }
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    if matches_binding(&bindings.confirm, &key) {
        match (width_buf.trim().parse::<u16>(), height_buf.trim().parse::<u16>()) {
            (Ok(w), Ok(h)) if w >= 1 && h >= 1 => {
                let old = (state.source.width, state.source.height);
                (state.source.width, state.source.height) = (w, h);
                if let Err(e) = state.source.validate_contract() {
                    (state.source.width, state.source.height) = old;
                    state.status_message = Some(format!("Frame size refused: {e}"));
                    return Action::Redraw;
                }
                state.dirty = true;
                state.status_message = Some(format!("Frame size set to {w}×{h}"));
                state.mode = Mode::Normal;
            }
            _ => {
                state.status_message =
                    Some("Width and height must be whole numbers ≥ 1".into());
            }
        }
        return Action::Redraw;
    }

    // Switch between the width and height fields; park the cursor at the end of
    // the newly-selected field.
    if matches_binding(&bindings.move_up, &key)
        || matches_binding(&bindings.move_down, &key)
        || (key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE)
    {
        selected_field = if selected_field == 0 { 1 } else { 0 };
        cursor = if selected_field == 0 {
            width_buf.chars().count()
        } else {
            height_buf.chars().count()
        };
        state.mode = Mode::Settings { selected_field, width_buf, height_buf, cursor };
        return Action::Redraw;
    }

    // Edit the selected field (digits only).
    let buf = if selected_field == 0 { &mut width_buf } else { &mut height_buf };
    match key.code {
        KeyCode::Char(c)
            if c.is_ascii_digit()
                && (key.modifiers == KeyModifiers::NONE
                    || key.modifiers == KeyModifiers::SHIFT) =>
        {
            if buf.chars().count() < 4 {
                let bi = char_to_byte_idx(buf, cursor);
                buf.insert(bi, c);
                cursor += 1;
            }
        }
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
            if cursor > 0 {
                let bi = char_to_byte_idx(buf, cursor - 1);
                buf.remove(bi);
                cursor -= 1;
            }
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            cursor = cursor.saturating_sub(1);
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            cursor = (cursor + 1).min(buf.chars().count());
        }
        _ => return Action::Continue,
    }
    state.mode = Mode::Settings { selected_field, width_buf, height_buf, cursor };
    Action::Redraw
}

/// Frames moved per Shift+arrow jump in top-level (Normal) frame navigation.
const FRAMES_PER_JUMP: usize = 10;

/// The merged animation span covering `frame`, as `(lo, hi)` — `lo` the earliest
/// start and `hi` the (exclusive) latest end of every `Animation` whose span
/// transitively overlaps one covering `frame`. `None` when no animation covers it.
///
/// Mirrors `Player::animation_cluster` so the editor's Shift+←/→ skips an
/// animation the same way the player's arrows do. Unlike the player it considers
/// **all** animations, not just `auto_play` ones: the editor has no auto-advance,
/// so while authoring you want to jump across any animation's span.
fn animation_cluster(objects: &[SceneObject], frame: usize) -> Option<(usize, usize)> {
    let spans: Vec<(usize, usize)> = objects
        .iter()
        .filter_map(|o| match o {
            SceneObject::Animation(a) => Some((a.frames.start, a.frames.end)),
            _ => None,
        })
        .collect();
    // Seed the cluster with every span covering `frame`.
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for &(s, e) in &spans {
        if s <= frame && frame < e {
            lo = lo.min(s);
            hi = hi.max(e);
        }
    }
    if lo == usize::MAX {
        return None;
    }
    // Grow across overlaps (`s < hi && lo < e`) until the cluster is closed.
    loop {
        let mut grew = false;
        for &(s, e) in &spans {
            if s < hi && lo < e {
                if s < lo {
                    lo = s;
                    grew = true;
                }
                if e > hi {
                    hi = e;
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    Some((lo, hi))
}

/// Target frame for a "big jump" from `frame` in a deck whose last frame is
/// `last`. When `frame` sits inside an animation, the jump lands just past the
/// merged span (`forward`) or just before it (`!forward`) — the player-style
/// animation skip. Otherwise it is a coarse `FRAMES_PER_JUMP` scrub. Shared by the
/// Shift+←/→ keys and their always-deliverable `[` / `]` aliases.
fn jump_target(objects: &[SceneObject], frame: usize, last: usize, forward: bool) -> usize {
    match animation_cluster(objects, frame) {
        Some((lo, hi)) => {
            if forward {
                hi.min(last)
            } else {
                lo.saturating_sub(1)
            }
        }
        None => {
            if forward {
                (frame + FRAMES_PER_JUMP).min(last)
            } else {
                frame.saturating_sub(FRAMES_PER_JUMP)
            }
        }
    }
}

fn handle_normal(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = &state.config.key_bindings;

    if matches_binding(&bindings.quit, &key) {
        return Action::Quit;
    }
    // The "big jump" keys: when the cursor is on an animation, skip to just
    // before/after its span (like the player's arrows); otherwise scrub
    // FRAMES_PER_JUMP frames. Two equivalent triggers — Shift+←/→, and the
    // always-deliverable `[` / `]` aliases (Shift+arrow is sent as a bare arrow by
    // macOS Terminal.app and many tmux setups, so it can't be relied on alone).
    // Checked before the plain 1-frame nav, which a shifted arrow also matches
    // (`matches_binding` ignores Shift on bare keys).
    let last = state.source.frame_count.saturating_sub(1);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let jump_fwd = matches_binding(&bindings.anim_skip_next, &key)
        || (shift && matches_binding(&bindings.next_frame, &key));
    let jump_back = matches_binding(&bindings.anim_skip_prev, &key)
        || (shift && matches_binding(&bindings.prev_frame, &key));
    if jump_fwd {
        state.current_frame = jump_target(&state.source.objects, state.current_frame, last, true);
        state.status_message = None;
        return Action::Redraw;
    }
    if jump_back {
        state.current_frame = jump_target(&state.source.objects, state.current_frame, last, false);
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.next_frame, &key) {
        let last = state.source.frame_count.saturating_sub(1);
        if state.current_frame < last {
            state.current_frame += 1;
            state.status_message = None;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.prev_frame, &key) {
        if state.current_frame > 0 {
            state.current_frame -= 1;
            state.status_message = None;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.add_object, &key) {
        state.mode = Mode::AddObject { selected: 0 };
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.select_object, &key) {
        let visible = state.objects_on_current_frame();
        if !visible.is_empty() {
            // Select is a multi-select: Space toggles members, Enter acts on the
            // set (1 object → its menu, 2+ → the action sub-menu).
            state.mode = Mode::MultiSelect {
                purpose: MultiSelectPurpose::Select,
                selected: 0,
                members: Vec::new(),
            };
            state.status_message =
                Some("Select: [Space] toggle, [Enter] act, [d] delete".into());
        } else {
            state.status_message = Some("No objects on this frame".into());
        }
        return Action::Redraw;
    }
    // Save-as (default `S`) is checked before plain save. Order also keeps a
    // custom `Ctrl-Shift-s` binding working on enhanced terminals, where it would
    // otherwise also satisfy the plain `Ctrl-s` check.
    if matches_binding(&bindings.save_as, &key) {
        let buf = state.file_path.clone();
        let cursor = buf.chars().count();
        state.mode = Mode::SaveAs { buf, cursor };
        state.status_message = Some("Save as — type a filename".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.save, &key) {
        match state.save() {
            Ok(()) => {}
            Err(e) => state.status_message = Some(format!("Save failed: {e}")),
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.open_settings, &key) {
        let width_buf = state.source.width.to_string();
        let height_buf = state.source.height.to_string();
        let cursor = width_buf.chars().count();
        state.mode = Mode::Settings { selected_field: 0, width_buf, height_buf, cursor };
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_menu, &key) {
        // Frame ops walk every frame; refuse them on an over-limit deck.
        if let Some(e) = super::state::frame_limit_error(state.source.frame_count) {
            state.status_message = Some(format!("⚠ {e}"));
            return Action::Redraw;
        }
        state.mode = Mode::FrameMenu;
        state.status_message = None;
        return Action::Redraw;
    }
    // Presentations hub: switch between open decks, open another file, and the
    // save-as / settings / fullscreen actions (all also reachable elsewhere).
    if matches_binding(&bindings.presentations_menu, &key) {
        let selected = state.workspace.active;
        state.mode = Mode::PresentationMenu { selected };
        state.status_message = None;
        return Action::Redraw;
    }
    // Paste: place the clipboard's clones as a movable ghost on this frame.
    // (Copy and Converge are now reached via `s` select → action sub-menu.)
    if matches_binding(&bindings.paste, &key) {
        return start_paste(state);
    }

    Action::Continue
}

/// Frame operations sub-menu: add a blank frame, copy/delete the current
/// frame, or start moving it.
fn handle_frame_menu(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    // Paste the cross-deck frame clipboard — offered only when it holds frames.
    if state.workspace.frame_clip_frames > 0 && matches_binding(&bindings.frame_clip_paste, &key) {
        state.current_frame = state.current_frame.min(state.source.frame_count.saturating_sub(1));
        state.mode = Mode::FramePastePlace;
        state.status_message = Some(format!(
            "Paste {} frame(s) — ←/→ pick target, [Enter] after / [b] before",
            state.workspace.frame_clip_frames
        ));
        return Action::Redraw;
    }
    let grows = matches_binding(&bindings.frame_add, &key) || matches_binding(&bindings.frame_copy, &key);
    if let Some(e) = super::state::frame_growth_error(&state.source, 1).filter(|_| grows) {
        state.status_message = Some(format!("⚠ {e}"));
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_add, &key) {
        insert_blank_frame(&mut state.source, state.current_frame);
        state.current_frame += 1;
        state.dirty = true;
        state.status_message = Some(format!("Added blank frame {}", state.current_frame + 1));
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_copy, &key) {
        copy_frame(&mut state.source, state.current_frame);
        state.current_frame += 1;
        state.dirty = true;
        state.status_message = Some(format!("Copied → frame {}", state.current_frame + 1));
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_delete, &key) {
        if state.source.frame_count > 1 {
            state.mode = Mode::Confirm {
                message: format!("Delete frame {}?", state.current_frame + 1),
                selected: 0,
                action: ConfirmAction::DeleteFrame,
                return_mode: Box::new(Mode::FrameMenu),
            };
        } else {
            state.status_message = Some("Can't delete the only frame".into());
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_move, &key) {
        if state.source.frame_count > 1 {
            let from = state.current_frame;
            state.mode = Mode::FrameMove { from };
            state.status_message = Some(format!(
                "Moving frame {} — ←/→ pick a slide, Enter to place",
                from + 1
            ));
        } else {
            state.status_message = Some("Only one frame to move".into());
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_overlay, &key) {
        if state.source.frame_count > 1 {
            let from = state.current_frame;
            state.mode = Mode::FrameOverlay { from };
            state.status_message = Some(format!(
                "Overlay frame {} — ←/→ pick a target, Enter to paste",
                from + 1
            ));
        } else {
            state.status_message = Some("Only one frame — nowhere to overlay".into());
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_jump, &key) {
        state.mode = Mode::FrameJump { buf: String::new(), cursor: 0 };
        state.status_message = Some(format!("Jump to frame (1-{})", state.source.frame_count));
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_select, &key) {
        state.mode = Mode::FrameSelectInput { buf: String::new(), cursor: 0 };
        state.status_message = Some("Select frames: e.g. 1, 2, 3 or 5-12".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.frame_auto, &key) {
        // Seed the input with the existing delay (in seconds) if this frame
        // already auto-advances, else the 5s default.
        let buf = frame_auto_advance_delay(&state.source, state.current_frame)
            .map(secs_string)
            .unwrap_or_else(|| "5".into());
        let cursor = buf.chars().count();
        state.mode = Mode::FrameAutoInput { buf, cursor };
        state.status_message = None;
        return Action::Redraw;
    }

    Action::Continue
}

/// "Save as" filename input: type a path, Enter hands it to the `Editor`, which
/// writes the deck there (adopting the new path), Esc cancels.
fn handle_save_as(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (mut buf, mut cursor) = match &state.mode {
        Mode::SaveAs { buf, cursor } => (buf.clone(), *cursor),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        state.status_message = Some("Save as cancelled".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        let path = buf.trim();
        if path.is_empty() {
            state.status_message = Some("Enter a filename".into());
            return Action::Redraw;
        }
        // The mode stays SaveAs; the Editor closes it once the save succeeds.
        return Action::SaveAs(path.to_string());
    }
    if frame_text_key(&key, &mut buf, &mut cursor) {
        state.mode = Mode::SaveAs { buf, cursor };
        return Action::Redraw;
    }
    Action::Continue
}

/// Typing a 1-based frame number to jump to. Enter jumps (clamped to the deck);
/// Esc returns to the frame menu.
fn handle_frame_jump(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (mut buf, mut cursor) = match &state.mode {
        Mode::FrameJump { buf, cursor } => (buf.clone(), *cursor),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::FrameMenu;
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        match buf.trim().parse::<usize>() {
            Ok(n) if n >= 1 => {
                let target = (n - 1).min(state.source.frame_count.saturating_sub(1));
                state.current_frame = target;
                state.mode = Mode::Normal;
                state.status_message = Some(format!("Jumped to frame {}", target + 1));
            }
            _ => state.status_message = Some("Enter a frame number (1-based)".into()),
        }
        return Action::Redraw;
    }
    if frame_text_key(&key, &mut buf, &mut cursor) {
        // Editing clears any stale prompt/error so the row-2 hint (and the live
        // frame-bar preview) reflect the current input.
        state.status_message = None;
        state.mode = Mode::FrameJump { buf, cursor };
        return Action::Redraw;
    }
    Action::Continue
}

/// Typing a multi-frame selection (`1, 2, 3` or `5-12`). Enter parses it into
/// `FrameSelected`; Esc returns to the frame menu.
fn handle_frame_select_input(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (mut buf, mut cursor) = match &state.mode {
        Mode::FrameSelectInput { buf, cursor } => (buf.clone(), *cursor),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::FrameMenu;
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        match super::state::parse_frame_selection(&buf, state.source.frame_count) {
            Ok(frames) => {
                state.status_message = Some(format!(
                    "Selected {} frame(s) — [d] delete, [m] move, [c] copy, [Esc] cancel",
                    frames.len()
                ));
                state.mode = Mode::FrameSelected { frames };
            }
            Err(e) => state.status_message = Some(format!("⚠ {e}")),
        }
        return Action::Redraw;
    }
    if frame_text_key(&key, &mut buf, &mut cursor) {
        // Editing clears any stale ⚠ error so the row-2 hint (and the live
        // frame-bar preview) reflect the current input.
        state.status_message = None;
        state.mode = Mode::FrameSelectInput { buf, cursor };
        return Action::Redraw;
    }
    Action::Continue
}

/// Format a millisecond delay as the seconds value to seed the auto-advance
/// input with (`5000` → `"5"`, `1500` → `"1.5"`) — no trailing `s` unit here so
/// it edits cleanly as a number.
fn secs_string(ms: u64) -> String {
    format!("{}", ms as f64 / 1000.0)
}

/// Typing the auto-advance delay (in seconds) for the current frame. Enter sets
/// it (a value of `0` or an empty field turns auto-advance off); Esc returns to
/// the frame menu.
fn handle_frame_auto_input(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (mut buf, mut cursor) = match &state.mode {
        Mode::FrameAutoInput { buf, cursor } => (buf.clone(), *cursor),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::FrameMenu;
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        let trimmed = buf.trim();
        let secs: f64 = if trimmed.is_empty() {
            0.0
        } else {
            match trimmed.parse::<f64>() {
                Ok(v) if v >= 0.0 => v,
                _ => {
                    state.status_message = Some("⚠ enter a delay in seconds (0 = off)".into());
                    return Action::Redraw;
                }
            }
        };
        let ms = (secs * 1000.0).round() as u64;
        let frame = state.current_frame;
        let removed = super::state::set_frame_auto_advance(&mut state.source, frame, ms);
        state.note_removed_objects(&removed);
        let on = ms > 0;
        state.dirty = true;
        state.status_message = Some(if on {
            format!(
                "Frame {} auto-advances after {}",
                frame + 1,
                super::state::format_secs(ms)
            )
        } else {
            format!("Frame {} auto-advance off", frame + 1)
        });
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if frame_text_key(&key, &mut buf, &mut cursor) {
        state.status_message = None;
        state.mode = Mode::FrameAutoInput { buf, cursor };
        return Action::Redraw;
    }
    Action::Continue
}

/// A multi-frame selection is active: `d` deletes the set (with confirm); Esc
/// returns to the frame menu.
fn handle_frame_selected(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let frames = match &state.mode {
        Mode::FrameSelected { frames } => frames.clone(),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::FrameMenu;
        state.status_message = None;
        return Action::Redraw;
    }
    // [y] yank the (contiguous) block to the cross-deck frame clipboard, ready to
    // paste into another open presentation.
    if matches_binding(&bindings.frame_clip_copy, &key) {
        if !is_contiguous_range(&frames) {
            state.status_message =
                Some("Select a contiguous range (e.g. 5-12) to copy across decks".into());
            return Action::Redraw;
        }
        return Action::CopyFrameBlock {
            lo: frames[0],
            hi: frames[frames.len() - 1],
        };
    }
    if matches_binding(&bindings.frame_delete, &key) {
        if frames.len() >= state.source.frame_count {
            state.status_message = Some("Can't delete every frame".into());
            return Action::Redraw;
        }
        state.mode = Mode::Confirm {
            message: format!("Delete {} selected frame(s)?", frames.len()),
            selected: 0,
            action: ConfirmAction::DeleteFrames { frames: frames.clone() },
            return_mode: Box::new(Mode::FrameSelected { frames }),
        };
        return Action::Redraw;
    }
    // [m]ove / [c]opy the selected block. Both need a *contiguous* range — moving
    // or duplicating a scattered set as a block has no clear meaning.
    let copy = matches_binding(&bindings.frame_copy, &key);
    if copy || matches_binding(&bindings.frame_move, &key) {
        if !is_contiguous_range(&frames) {
            state.status_message =
                Some("Select a contiguous range (e.g. 5-12) to move/copy".into());
            return Action::Redraw;
        }
        if !copy && frames.len() >= state.source.frame_count {
            state.status_message = Some("Nowhere to move every frame".into());
            return Action::Redraw;
        }
        // Start the scroll cursor at the block so the target is easy to pick.
        state.current_frame = frames[0];
        let verb = if copy { "Copy" } else { "Move" };
        state.status_message = Some(format!(
            "{verb} {} frame(s) — ←/→ pick target, [Enter] after / [b] before",
            frames.len()
        ));
        state.mode = Mode::FrameRangePlace { frames, copy };
        return Action::Redraw;
    }
    Action::Continue
}

/// A sorted, de-duplicated frame selection is contiguous iff it spans exactly its
/// own length (no gaps) — `parse_frame_selection` already sorts and dedups.
fn is_contiguous_range(frames: &[usize]) -> bool {
    match (frames.first(), frames.last()) {
        (Some(&lo), Some(&hi)) => hi - lo + 1 == frames.len(),
        _ => false,
    }
}

/// Scrolling the deck to place a moved/copied contiguous frame block: ←/→ pick a
/// target slide, Enter drops the block after it, `b` before it.
fn handle_frame_range_place(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (frames, copy) = match &state.mode {
        Mode::FrameRangePlace { frames, copy } => (frames.clone(), *copy),
        _ => return Action::Continue,
    };
    let (lo, hi) = (frames[0], frames[frames.len() - 1]);

    if matches_binding(&bindings.cancel, &key) {
        state.status_message = Some("Cancelled".into());
        state.mode = Mode::FrameSelected { frames };
        return Action::Redraw;
    }
    if matches_binding(&bindings.next_frame, &key) {
        let last = state.source.frame_count.saturating_sub(1);
        if state.current_frame < last {
            state.current_frame += 1;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.prev_frame, &key) {
        if state.current_frame > 0 {
            state.current_frame -= 1;
        }
        return Action::Redraw;
    }

    let before = if matches_binding(&bindings.confirm, &key) {
        false // Enter = after
    } else if matches_binding(&bindings.frame_move_before, &key) {
        true // b = before
    } else {
        return Action::Continue;
    };

    let target = state.current_frame;
    if copy {
        if let Some(e) = super::state::frame_growth_error(&state.source, hi - lo + 1) {
            state.status_message = Some(format!("⚠ {e}"));
            return Action::Redraw;
        }
        if let Some(why) = super::state::copy_frames_refusal(&state.source, lo, hi, target, before) {
            state.status_message = Some(why);
            return Action::Redraw;
        }
        let (new_current, count) =
            super::state::copy_frames(&mut state.source, lo, hi, target, before);
        state.current_frame = new_current;
        state.clipboard_sources.clear();
        state.dirty = true;
        state.status_message = Some(format!(
            "Copied {count} frame(s) {} frame {}",
            if before { "before" } else { "after" },
            target + 1
        ));
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    // Move: the target can't sit inside the block being moved.
    if target >= lo && target <= hi {
        state.status_message = Some("Pick a slide outside the moved range (←/→)".into());
        return Action::Redraw;
    }
    if let Some(why) = super::state::move_frames_refusal(&state.source, &frames, target, before) {
        state.status_message = Some(why);
        return Action::Redraw;
    }
    let new_index = super::state::move_frames(&mut state.source, &frames, target, before);
    state.current_frame = new_index;
    state.clipboard_sources.clear();
    state.dirty = true;
    state.status_message = Some(format!(
        "Moved {} frame(s) {} frame {}",
        frames.len(),
        if before { "before" } else { "after" },
        target + 1
    ));
    state.mode = Mode::Normal;
    Action::Redraw
}

/// Shared text-buffer key handling for the short single-line frame inputs
/// (jump / select). Returns `true` if the key edited the buffer or cursor.
fn frame_text_key(key: &KeyEvent, buf: &mut String, cursor: &mut usize) -> bool {
    match key.code {
        KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
            let byte_idx = char_to_byte_idx(buf, *cursor);
            buf.insert(byte_idx, c);
            *cursor += 1;
            true
        }
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
            if *cursor > 0 {
                let byte_idx = char_to_byte_idx(buf, *cursor - 1);
                buf.remove(byte_idx);
                *cursor -= 1;
            }
            true
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            *cursor = cursor.saturating_sub(1);
            true
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            *cursor = (*cursor + 1).min(buf.chars().count());
            true
        }
        _ => false,
    }
}

/// Scrolling the deck to choose where the moved slide should land.
fn handle_frame_move(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let from = match &state.mode {
        Mode::FrameMove { from } => *from,
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        // Cancel: drop back to the frame menu on the slide being moved.
        state.current_frame = from;
        state.mode = Mode::FrameMenu;
        state.status_message = Some("Move cancelled".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.next_frame, &key) {
        let last = state.source.frame_count.saturating_sub(1);
        if state.current_frame < last {
            state.current_frame += 1;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.prev_frame, &key) {
        if state.current_frame > 0 {
            state.current_frame -= 1;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        let target = state.current_frame;
        if target == from {
            state.status_message = Some("Pick a different slide (←/→)".into());
            return Action::Redraw;
        }
        state.mode = Mode::FrameMovePlace { from, target };
        state.status_message = Some(format!(
            "Place moved slide [b]efore or [Enter] after slide {}?",
            target + 1
        ));
        return Action::Redraw;
    }

    Action::Continue
}

/// Choosing whether the moved slide lands before or after the shown slide.
fn handle_frame_move_place(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (from, target) = match &state.mode {
        Mode::FrameMovePlace { from, target } => (*from, *target),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        // Back to scrolling; keep the cursor on the target slide.
        state.mode = Mode::FrameMove { from };
        state.status_message = Some(format!(
            "Moving frame {} — ←/→ pick a slide, Enter to place",
            from + 1
        ));
        return Action::Redraw;
    }

    let place = if matches_binding(&bindings.confirm, &key) {
        Some(false) // after
    } else if matches_binding(&bindings.frame_move_before, &key) {
        Some(true) // before
    } else {
        None
    };

    if let Some(before) = place {
        if let Some(why) = super::state::move_frames_refusal(&state.source, &[from], target, before) {
            state.status_message = Some(why);
            return Action::Redraw;
        }
        let new_index = move_frame(&mut state.source, from, target, before);
        state.current_frame = new_index;
        state.clipboard_sources.clear(); // a split piece shifts object indices
        state.dirty = true;
        state.status_message = Some(format!(
            "Moved frame to position {} ({} slide {})",
            new_index + 1,
            if before { "before" } else { "after" },
            target + 1
        ));
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    Action::Continue
}

/// Scrolling the deck to choose which existing frame to paste the source
/// frame's objects on top of, then pasting them (no new frame is inserted).
fn handle_frame_overlay(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let from = match &state.mode {
        Mode::FrameOverlay { from } => *from,
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        // Cancel: drop back to the frame menu on the source slide.
        state.current_frame = from;
        state.mode = Mode::FrameMenu;
        state.status_message = Some("Overlay cancelled".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.next_frame, &key) {
        let last = state.source.frame_count.saturating_sub(1);
        if state.current_frame < last {
            state.current_frame += 1;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.prev_frame, &key) {
        if state.current_frame > 0 {
            state.current_frame -= 1;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        let onto = state.current_frame;
        if onto == from {
            state.status_message = Some("Pick a different frame (←/→)".into());
            return Action::Redraw;
        }
        let pasted = overlay_frame(&mut state.source, from, onto);
        state.dirty = true;
        state.status_message = Some(format!(
            "Pasted {} object{} from frame {} onto frame {}",
            pasted,
            if pasted == 1 { "" } else { "s" },
            from + 1,
            onto + 1
        ));
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    Action::Continue
}

/// Scrolling the deck to choose where to drop the cross-deck frame clipboard:
/// ←/→ pick a target slide, Enter drops the pasted block after it, `b` before it.
/// The actual paste is performed by the `Editor` (it owns the clipboard).
fn handle_frame_paste_place(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::FrameMenu;
        state.status_message = Some("Paste cancelled".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.next_frame, &key) {
        let last = state.source.frame_count.saturating_sub(1);
        if state.current_frame < last {
            state.current_frame += 1;
        }
        return Action::Redraw;
    }
    if matches_binding(&bindings.prev_frame, &key) {
        if state.current_frame > 0 {
            state.current_frame -= 1;
        }
        return Action::Redraw;
    }
    let before = if matches_binding(&bindings.confirm, &key) {
        false // Enter = after
    } else if matches_binding(&bindings.frame_move_before, &key) {
        true // b = before
    } else {
        return Action::Continue;
    };
    Action::PasteFrameBlock {
        target: state.current_frame,
        before,
    }
}

/// The presentations hub: a list of the open decks (Enter switches to the
/// highlighted one) plus open-file / save-as / settings / fullscreen actions.
/// Switching decks is an `Editor`-level action (a deck can't reach its siblings),
/// so Enter returns `Action::SwitchDeck`; the deck list itself is mirrored into
/// `state.workspace` by the `Editor`.
fn handle_presentation_menu(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let selected = match &state.mode {
        Mode::PresentationMenu { selected } => *selected,
        _ => return Action::Continue,
    };
    let count = state.workspace.deck_names.len().max(1);

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { count - 1 } else { selected - 1 };
        state.mode = Mode::PresentationMenu { selected: new_sel };
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % count;
        state.mode = Mode::PresentationMenu { selected: new_sel };
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        return Action::SwitchDeck(selected);
    }
    if matches_binding(&bindings.presentation_open, &key) {
        state.mode = Mode::OpenFile { buf: String::new(), cursor: 0 };
        state.status_message = Some("Open file — type a path".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.presentation_save_as, &key) {
        let buf = state.file_path.clone();
        let cursor = buf.chars().count();
        state.mode = Mode::SaveAs { buf, cursor };
        state.status_message = Some("Save as — type a filename".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.open_settings, &key) {
        let width_buf = state.source.width.to_string();
        let height_buf = state.source.height.to_string();
        let cursor = width_buf.chars().count();
        state.mode = Mode::Settings { selected_field: 0, width_buf, height_buf, cursor };
        state.status_message = None;
        return Action::Redraw;
    }
    if matches_binding(&bindings.presentation_fullscreen, &key) {
        return Action::ToggleFullscreen;
    }

    Action::Continue
}

/// Typing a path to open another presentation as a new deck. Enter opens it (the
/// `Editor` adds/focuses the deck); Esc returns to the presentations menu.
fn handle_open_file(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (mut buf, mut cursor) = match &state.mode {
        Mode::OpenFile { buf, cursor } => (buf.clone(), *cursor),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::PresentationMenu { selected: state.workspace.active };
        state.status_message = Some("Open cancelled".into());
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        let path = buf.trim();
        if path.is_empty() {
            state.status_message = Some("Enter a file path".into());
            return Action::Redraw;
        }
        return Action::OpenDeck(path.to_string());
    }
    if frame_text_key(&key, &mut buf, &mut cursor) {
        state.mode = Mode::OpenFile { buf, cursor };
        return Action::Redraw;
    }
    Action::Continue
}

fn handle_add_object(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    let selected = match &state.mode {
        Mode::AddObject { selected } => *selected,
        _ => return Action::Continue,
    };

    let type_count = object_defaults::OBJECT_TYPES.len();

    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { type_count - 1 } else { selected - 1 };
        state.mode = Mode::AddObject { selected: new_sel };
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % type_count;
        state.mode = Mode::AddObject { selected: new_sel };
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        return commit_add_object(state, selected);
    }
    // Quick-add: a single letter selects and adds its type directly. Checked
    // after the configured nav/confirm bindings so a custom letter binding for
    // those still wins. (The global fullscreen key is handled earlier.)
    if let KeyCode::Char(c) = key.code {
        if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
            if let Some(idx) = object_defaults::object_type_for_key(c) {
                return commit_add_object(state, idx);
            }
        }
    }

    Action::Continue
}

/// Commit the addition of the object type at `index` (from the Add-Object menu,
/// via Enter or a quick-add shortcut). Most types land in EditProperties; Group
/// and Art first enter their own member/library pickers.
fn commit_add_object(state: &mut EditorState, index: usize) -> Action {
    let type_name = object_defaults::OBJECT_TYPES[index].name;
    if type_name == "Group" {
        // Group members are chosen from the objects on the current slide.
        if state.objects_on_current_frame().is_empty() {
            state.status_message = Some("No objects on this slide to group".into());
            state.mode = Mode::Normal;
        } else {
            state.mode = Mode::MultiSelect {
                purpose: MultiSelectPurpose::Group,
                selected: 0,
                members: Vec::new(),
            };
        }
    } else if type_name == "Art" {
        // Art needs a library piece chosen first.
        state.mode = Mode::AddArt {
            selected: 0,
            items: crate::art_library::all_items(),
            purpose: ArtPick::Art,
        };
    } else if type_name == "Morph" {
        // A morph needs two pieces: pick the `from` first, then the `to`.
        state.mode = Mode::AddArt {
            selected: 0,
            items: crate::art_library::all_items(),
            purpose: ArtPick::MorphFrom,
        };
        state.status_message = Some("Morph: pick the starting art".into());
    } else {
        let obj = object_defaults::create_default(index, state.current_frame);
        state.source.objects.push(obj);
        state.dirty = true;
        let new_index = state.source.objects.len() - 1;
        // Text-first objects (Label, List) jump straight into the centred
        // multi-line text editor with an empty buffer, so the user can type
        // content immediately instead of browsing properties first.
        // Cancelling (Esc) leaves the object's default text intact.
        state.mode = if type_name == "Label" || type_name == "List" {
            ep_editing(new_index, 0, String::new(), 0, 0, 0)
        } else {
            ep_browse(new_index, 0, 0)
        };
        // A freshly added loop can already collide with another loop on the
        // same slide — flag it immediately rather than only at compile time.
        state.status_message = Some(match state.source.validate_loops() {
            Ok(()) => format!("Added {type_name}"),
            Err(e) => format!("Added {type_name} — ⚠ {e}"),
        });
    }
    Action::Redraw
}

fn handle_multi_select(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (purpose, selected, members) = match &state.mode {
        Mode::MultiSelect { purpose, selected, members } => (*purpose, *selected, members.clone()),
        _ => return Action::Continue,
    };

    // Only the current slide's objects are selectable. `selected` indexes into
    // this visible list; `members` stores the real `source.objects` indices.
    let visible = state.objects_on_current_frame();
    let total = visible.len();
    if total == 0 {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    let selected = selected.min(total - 1);

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { total - 1 } else { selected - 1 };
        state.mode = Mode::MultiSelect { purpose, selected: new_sel, members };
        return Action::BlinkSelection;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % total;
        state.mode = Mode::MultiSelect { purpose, selected: new_sel, members };
        return Action::BlinkSelection;
    }
    // Space: toggle membership of the highlighted object
    if key.code == KeyCode::Char(' ') {
        let obj_idx = visible[selected];
        let mut new_members = members;
        if let Some(pos) = new_members.iter().position(|&m| m == obj_idx) {
            new_members.remove(pos);
        } else {
            new_members.push(obj_idx);
        }
        state.mode = Mode::MultiSelect { purpose, selected, members: new_members };
        return Action::Redraw;
    }
    // [d]elete the highlighted object (general Select only) — preserves the
    // quick browse-and-delete that the old single-pick select had. Returns to a
    // fresh selection (members cleared) so toggled indices can't go stale.
    if purpose == MultiSelectPurpose::Select && matches_binding(&bindings.delete_object, &key) {
        let obj_index = visible[selected];
        let message = delete_confirm_message(&state.source, obj_index);
        state.mode = Mode::Confirm {
            message,
            selected: 0,
            action: ConfirmAction::DeleteObject { object_index: obj_index },
            return_mode: Box::new(Mode::MultiSelect {
                purpose: MultiSelectPurpose::Select, selected: 0, members: Vec::new(),
            }),
        };
        return Action::Redraw;
    }
    // Enter: commit. Group builds a group from the toggled set directly; the
    // general Select routes to the single-object menu or the action sub-menu
    // (where copy/converge/delete live). With nothing explicitly toggled, fall
    // back to the highlighted object.
    if matches_binding(&bindings.confirm, &key) {
        let mut chosen = members;
        if chosen.is_empty() {
            chosen.push(visible[selected]);
        }
        match purpose {
            MultiSelectPurpose::Group => {
                let group = SceneObject::Group(Group {
                    members: chosen,
                    // Auto by default: the group's span follows its members'
                    // ranges until an explicit range is set in the props panel.
                    frames: None,
                    z_order: 0,
                });
                state.source.objects.push(group);
                state.dirty = true;
                let new_index = state.source.objects.len() - 1;
                state.mode = ep_browse(new_index, 0, 0);
                state.status_message = Some("Added Group".into());
            }
            MultiSelectPurpose::Select => {
                // One object → its menu; many → the action sub-menu.
                state.mode = if chosen.len() == 1 {
                    Mode::SelectedObject { object_index: chosen[0] }
                } else {
                    Mode::SelectAction { members: chosen, selected: 0 }
                };
            }
        }
        return Action::Redraw;
    }

    Action::Continue
}

/// Actions offered by the multi-object select sub-menu ([`Mode::SelectAction`]),
/// in display order.
#[derive(Clone, Copy, PartialEq)]
enum SelectActionKind {
    Copy,
    Converge,
    Delete,
    EditProps,
}

const SELECT_ACTIONS: &[(SelectActionKind, &str)] = &[
    (SelectActionKind::Copy, "Copy"),
    (SelectActionKind::Converge, "Converge"),
    (SelectActionKind::Delete, "Delete"),
    (SelectActionKind::EditProps, "Edit Props"),
];

/// The action sub-menu's row labels, in display order — for the panel renderer.
pub(crate) fn select_action_labels() -> Vec<&'static str> {
    SELECT_ACTIONS.iter().map(|(_, l)| *l).collect()
}

/// The action sub-menu shown after selecting 2+ objects: pick what to do with
/// the whole set (copy to clipboard, converge onto a shared point, or delete).
fn handle_select_action(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (members, selected) = match &state.mode {
        Mode::SelectAction { members, selected } => (members.clone(), *selected),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { SELECT_ACTIONS.len() - 1 } else { selected - 1 };
        state.mode = Mode::SelectAction { members, selected: new_sel };
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % SELECT_ACTIONS.len();
        state.mode = Mode::SelectAction { members, selected: new_sel };
        return Action::Redraw;
    }
    if matches_binding(&bindings.confirm, &key) {
        match SELECT_ACTIONS[selected.min(SELECT_ACTIONS.len() - 1)].0 {
            SelectActionKind::Copy => {
                copy_to_clipboard(state, &members);
                state.mode = Mode::Normal;
            }
            SelectActionKind::Converge => {
                let expanded = super::state::expand_selection(&state.source, &members);
                state.mode = enter_converge(state, expanded);
            }
            SelectActionKind::Delete => {
                // Confirm first (multi-delete is destructive). "No" returns to
                // the action sub-menu so the still-valid selection isn't lost.
                let n = members.len();
                state.mode = Mode::Confirm {
                    message: format!("Delete {n} objects?"),
                    selected: 0,
                    action: ConfirmAction::DeleteObjects { object_indices: members.clone() },
                    return_mode: Box::new(Mode::SelectAction { members, selected }),
                };
            }
            SelectActionKind::EditProps => {
                // Bulk-edit the shared properties. Refuse if the selection has no
                // common editable property (e.g. a Label + a Loop).
                let common = properties::common_properties(&state.source.objects, &members);
                if common.is_empty() {
                    state.status_message =
                        Some("These objects share no editable properties".into());
                    state.mode = Mode::SelectAction { members, selected };
                } else {
                    state.status_message = Some(format!(
                        "Editing {} shared propert{} across {} objects",
                        common.len(),
                        if common.len() == 1 { "y" } else { "ies" },
                        members.len(),
                    ));
                    state.mode = emp_browse(members, 0, 0);
                }
            }
        }
        return Action::Redraw;
    }

    Action::Continue
}

/// The confirm-dialog message for deleting object `idx`. A `Group` ungroups
/// (members kept); an `Animation` spells out that the objects it drives will be
/// frozen (deleting it removes the motion, not just the auto-play sidecar);
/// everything else is a plain "Delete <summary>?".
fn delete_confirm_message(source: &SourcePresentation, idx: usize) -> String {
    match &source.objects[idx] {
        SceneObject::Group(_) => "Ungroup? (members are kept)".to_string(),
        SceneObject::Animation(a) => {
            let count = source
                .objects
                .iter()
                .filter(|o| super::state::referenced_anim_ids(o).contains(&a.id))
                .count();
            // Span shown 1-based inclusive, matching the props panel.
            let (lo, hi) = (a.frames.start.saturating_add(1), a.frames.end);
            format!(
                "Delete animation {lo}-{hi}? The {count} object{} it moves will be frozen.",
                if count == 1 { "" } else { "s" }
            )
        }
        other => format!("Delete {}?", super::state::scene_object_summary(other)),
    }
}

/// Capture the objects at `indices` (expanding any group to include its members)
/// into the clipboard as self-contained clones, recording their source indices
/// for a later *linked* paste. Shared by the copy-select flow and the quick
/// single-object copy from `SelectedObject`. Span behaviours are left out, as
/// frame overlay does: a copied `Animation` would duplicate its id and retime
/// the original's motion, and a `Loop` or `AutoAdvance` marker pasted onto one
/// slide would be a one-slide loop (nested in any loop there, so the deck stops
/// compiling) or make the paste slide auto-advance.
fn copy_to_clipboard(state: &mut EditorState, indices: &[usize]) {
    let mut expanded = super::state::expand_selection(&state.source, indices);
    let before = expanded.len();
    expanded.retain(|&i| {
        !matches!(
            state.source.objects[i],
            SceneObject::Animation(_) | SceneObject::Loop(_) | SceneObject::AutoAdvance(_)
        )
    });
    let skipped = if expanded.len() < before {
        " (animations, loops and auto-advance markers are not copied)"
    } else {
        ""
    };
    if expanded.is_empty() {
        state.status_message = Some(
            "Animations, loops and auto-advance markers can't be copied — copy the objects instead".into(),
        );
        return;
    }
    state.clipboard = super::state::clone_selection(&state.source.objects, &expanded);
    // A paste is a static snapshot of this (the copy) frame: animated
    // coordinates are flattened to what they show here — so the clipboard holds
    // no animation ids an edit could make stale — and a morph keeps the blend it
    // shows here instead of restarting at `from` (as R4-15's frame copy).
    let frame = state.current_frame;
    let anims = AnimSpans::of(&state.source);
    for obj in &mut state.clipboard {
        super::state::flatten_coordinates(obj, frame, &anims);
        if let SceneObject::Morph(m) = obj {
            m.keep_blend(frame, frame + 1);
        }
    }
    // A linked paste's family is anchored on a gap-strobe set's original (its
    // content is the copy's), the member `link_family_of` and re-strobing keep:
    // a copy picked on a later sample frame would be cleared, taking the link.
    state.clipboard_sources =
        expanded.iter().map(|&i| super::state::strobe_set(&state.source, i)[0]).collect();
    let n = state.clipboard.len();
    state.status_message = Some(format!(
        "Copied {n} object{}{skipped} — press paste to place",
        if n == 1 { "" } else { "s" }
    ));
}

/// Begin a paste session: spawn the first ghost clone-set and enter
/// `PastePlacing`. No-op (with a hint) when the clipboard is empty.
fn start_paste(state: &mut EditorState) -> Action {
    if state.clipboard.is_empty() {
        state.status_message = Some("Clipboard is empty — copy something first".into());
        return Action::Redraw;
    }
    let pending = spawn_paste(state);
    state.dirty = true;
    state.mode = Mode::PastePlacing { pending, linked: false, families: Vec::new() };
    state.status_message =
        Some("Paste: arrows move, [Enter] stamp, [l] link, [Esc] done".into());
    Action::Redraw
}

/// Append a fresh set of clipboard clones onto the current frame — re-anchored
/// to it (single frame), group members re-pointed to their new absolute
/// indices, and nudged by (1,1) so a same-frame paste is visibly a new copy.
/// Returns the new objects' (tail) indices.
fn spawn_paste(state: &mut EditorState) -> Vec<usize> {
    let base = state.source.objects.len();
    let current = state.current_frame;
    let anims = AnimSpans::of(&state.source);
    let mut clones = state.clipboard.clone();
    for obj in &mut clones {
        // Re-anchor to the current frame (auto groups keep their derived range).
        if let Some(fr) = super::state::scene_object_frame_range_mut(obj) {
            fr.start = current;
            fr.end = current + 1;
        }
        // A clone lands on a single frame, so flatten any animated coordinate to
        // a static value (otherwise it's degenerate — and can't be moved with the
        // arrow keys, which only nudge `Fixed` coordinates).
        super::state::flatten_coordinates(obj, current, &anims);
        // Clipboard-local member index → new absolute index.
        if let SceneObject::Group(g) = obj {
            for m in &mut g.members {
                *m += base;
            }
        }
    }
    let pending: Vec<usize> = (base..base + clones.len()).collect();
    state.source.objects.extend(clones);
    // Nudge off the source (groups are no-ops; their members carry the motion).
    for &i in &pending {
        properties::move_object(&mut state.source.objects[i], 1, 1);
    }
    pending
}

fn handle_paste_placing(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (pending, mut linked, mut families) = match &state.mode {
        Mode::PastePlacing { pending, linked, families } => {
            (pending.clone(), *linked, families.clone())
        }
        _ => return Action::Continue,
    };

    // Esc: discard the un-dropped ghost (always the tail) and finish. Commit each
    // accumulated per-object link family — whatever [l] shows now: the families
    // only ever hold copies stamped while linked (and their sources).
    if matches_binding(&bindings.cancel, &key) {
        let keep = state.source.objects.len().saturating_sub(pending.len());
        state.source.objects.truncate(keep);
        for mut fam in families {
            fam.sort_unstable();
            fam.dedup();
            fam.retain(|&i| i < state.source.objects.len());
            if fam.len() >= 2 {
                state.source.links.push(fam);
            }
        }
        state.mode = Mode::Normal;
        state.status_message = Some("Paste finished".into());
        return Action::Redraw;
    }

    // [l]: toggle linked vs independent for the rest of this session.
    if key.code == KeyCode::Char('l') && key.modifiers == KeyModifiers::NONE {
        linked = !linked;
        state.mode = Mode::PastePlacing { pending, linked, families };
        state.status_message = Some(if linked {
            "Linked: each copy syncs edits with its original".into()
        } else {
            "Independent: detached copies".into()
        });
        return Action::Redraw;
    }

    // Arrows: move the whole ghost set together.
    if key.modifiers == KeyModifiers::NONE {
        let (dx, dy) = match key.code {
            KeyCode::Left => (-1, 0),
            KeyCode::Right => (1, 0),
            KeyCode::Up => (0, -1),
            KeyCode::Down => (0, 1),
            _ => (0, 0),
        };
        if dx != 0 || dy != 0 {
            // One clamp for the whole ghost set, so it stops at the edge intact.
            properties::move_objects(&mut state.source.objects, &pending, dx, dy);
            state.dirty = true;
            return Action::Redraw;
        }
    }

    // Enter: drop the current set and re-arm a fresh ghost (rubber-stamp loop).
    if matches_binding(&bindings.confirm, &key) {
        if linked {
            // One family per clipboard object: clone `pending[p]` joins object
            // `p`'s family (seeded with its still-valid source on the first
            // stamp), so an object links only to its own copies.
            if families.is_empty() {
                families = pending
                    .iter()
                    .enumerate()
                    .map(|(p, _)| {
                        match state.clipboard_sources.get(p) {
                            Some(&src) if src < state.source.objects.len() => vec![src],
                            _ => vec![],
                        }
                    })
                    .collect();
            }
            for (p, &clone_idx) in pending.iter().enumerate() {
                if let Some(fam) = families.get_mut(p) {
                    fam.push(clone_idx);
                }
            }
        }
        let next = spawn_paste(state);
        state.dirty = true;
        state.mode = Mode::PastePlacing { pending: next, linked, families };
        state.status_message = Some("Stamped — place the next, or [Esc] to finish".into());
        return Action::Redraw;
    }

    Action::Continue
}

/// Properties that stay **per-object** on linked copies (placement/layering):
/// frames, z-order and every coordinate-kind geometry field (so `HLine`/`Arrow`
/// endpoints too, not just `x/y/width/height`). Every other property
/// propagates to a linked object's siblings.
fn is_placement_prop(objects: &[SceneObject], index: usize, name: &str) -> bool {
    matches!(
        name,
        "x" | "y" | "width" | "height" | "first_frame" | "last_frame" | "z_order"
    ) || properties::get_properties(objects, index)
        .iter()
        .any(|p| p.name == name && matches!(p.kind, properties::PropertyKind::Coordinate))
}

fn handle_selected_object(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    let object_index = match &state.mode {
        Mode::SelectedObject { object_index } => *object_index,
        _ => return Action::Continue,
    };

    // [e]dit: open properties panel
    if matches_binding(&bindings.edit_object, &key) {
        state.mode = Mode::EditProperties {
            object_index,
            selected_property: 0,
            editing_value: None,
            cursor: 0,
            scroll: 0,
            panel_scroll: 0,
            dropdown: None,
        };
        return Action::Redraw;
    }

    // [c]opy this object to the clipboard (quick single-object path).
    if matches_binding(&bindings.copy, &key) {
        copy_to_clipboard(state, &[object_index]);
        return Action::Redraw;
    }
    // [v] paste the clipboard right away.
    if matches_binding(&bindings.paste, &key) {
        return start_paste(state);
    }

    // [d]elete
    if matches_binding(&bindings.delete_object, &key) {
        let message = delete_confirm_message(&state.source, object_index);
        state.mode = Mode::Confirm {
            message,
            selected: 0,
            action: ConfirmAction::DeleteObject { object_index },
            return_mode: Box::new(Mode::SelectedObject { object_index }),
        };
        return Action::Redraw;
    }

    // [r]esize: enter arrow-key resize mode (works on every terminal, unlike
    // Shift+arrows which some terminals capture for scrollback).
    if matches_binding(&bindings.resize_object, &key) {
        state.mode = Mode::ResizeObject { object_index };
        state.status_message = None;
        return Action::Redraw;
    }

    let is_group = matches!(state.source.objects[object_index], SceneObject::Group(_));
    let is_table = matches!(state.source.objects[object_index], SceneObject::Table(_));

    // Ctrl+Shift+Arrow: shrink from that edge
    if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) {
        let (dw, dh) = match key.code {
            KeyCode::Right => (1, 0),
            KeyCode::Left => (-1, 0),
            KeyCode::Down => (0, 1),
            KeyCode::Up => (0, -1),
            _ => (0, 0),
        };
        if dw != 0 || dh != 0 {
            if is_group {
                // Shrink from the pressed edge; anchor the opposite edge.
                // delta is always -1 (shrink); anchor is opposite to key direction.
                let (gdw, gdh, anchor_left, anchor_top) = match key.code {
                    KeyCode::Right => (-1,  0, true,  true),  // shrink from right, anchor left
                    KeyCode::Left  => (-1,  0, false, true),  // shrink from left,  anchor right
                    KeyCode::Down  => ( 0, -1, true,  true),  // shrink from bottom, anchor top
                    KeyCode::Up    => ( 0, -1, true,  false), // shrink from top,    anchor bottom
                    _ => (0, 0, true, true),
                };
                properties::resize_group(&mut state.source.objects, object_index, gdw, gdh, anchor_left, anchor_top);
            } else {
                properties::shrink_object(&mut state.source.objects[object_index], dw, dh);
            }
            state.dirty = true;
            return Action::Redraw;
        }
    }

    // Shift+Arrow: grow in the direction of the arrow
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        let (dw, dh): (i32, i32) = match key.code {
            KeyCode::Right => (1, 0),
            KeyCode::Left => (-1, 0),
            KeyCode::Down => (0, 1),
            KeyCode::Up => (0, -1),
            _ => (0, 0),
        };
        if dw != 0 || dh != 0 {
            if is_group {
                // grow toward the pressed edge: anchor the opposite edge.
                let anchor_left = key.code != KeyCode::Left;
                let anchor_top  = key.code != KeyCode::Up;
                properties::resize_group(&mut state.source.objects, object_index, dw.abs(), dh.abs(), anchor_left, anchor_top);
            } else if is_table {
                // For tables: Right/Down grows from the right/bottom edge,
                // Left/Up shrinks from the right/bottom edge. A table's height
                // auto-fits its content, so vertical resizes are seeded from the
                // natural height — otherwise small changes sit below the content
                // height and look like a no-op.
                let frame = state.current_frame;
                let anims = AnimSpans::of(&state.source);
                match key.code {
                    KeyCode::Right => properties::resize_object(&mut state.source.objects[object_index], 1, 0),
                    KeyCode::Left  => properties::shrink_object(&mut state.source.objects[object_index], 1, 0),
                    KeyCode::Down  => grow_table_height(&mut state.source.objects[object_index], frame, &anims, 1),
                    KeyCode::Up    => grow_table_height(&mut state.source.objects[object_index], frame, &anims, -1),
                    _ => {}
                }
            } else {
                properties::resize_object(&mut state.source.objects[object_index], dw, dh);
            }
            state.dirty = true;
            return Action::Redraw;
        }
    }

    // Plain Arrow keys: move
    if key.modifiers == KeyModifiers::NONE {
        let (dx, dy) = match key.code {
            KeyCode::Left => (-1, 0),
            KeyCode::Right => (1, 0),
            KeyCode::Up => (0, -1),
            KeyCode::Down => (0, 1),
            _ => (0, 0),
        };
        if dx != 0 || dy != 0 {
            if is_group {
                properties::move_group(&mut state.source.objects, object_index, dx, dy);
            } else {
                properties::move_object(&mut state.source.objects[object_index], dx, dy);
            }
            state.dirty = true;
            return Action::Redraw;
        }
    }

    Action::Continue
}

/// Resize mode: plain arrow keys adjust the selected object's far edge —
/// Left/Right change width, Up/Down change height (Down/Right grow, Up/Left
/// shrink). Plain arrows are delivered by every terminal, so this works where
/// Shift+Up/Down (captured for scrollback by many terminals) does not.
fn handle_resize_object(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let object_index = match &state.mode {
        Mode::ResizeObject { object_index } => *object_index,
        _ => return Action::Continue,
    };

    // Esc or Enter return to the selected-object menu.
    if matches_binding(&bindings.cancel, &key) || matches_binding(&bindings.confirm, &key) {
        state.mode = Mode::SelectedObject { object_index };
        return Action::Redraw;
    }

    if key.modifiers != KeyModifiers::NONE
        || !matches!(key.code, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down)
    {
        return Action::Continue;
    }

    let is_group = matches!(state.source.objects[object_index], SceneObject::Group(_));
    let is_table = matches!(state.source.objects[object_index], SceneObject::Table(_));
    let frame = state.current_frame;
    let anims = AnimSpans::of(&state.source);
    let objects = &mut state.source.objects;

    if is_group {
        // Grow/shrink the group's bounding box, anchored at its top-left.
        match key.code {
            KeyCode::Right => properties::resize_group(objects, object_index, 1, 0, true, true),
            KeyCode::Left  => properties::resize_group(objects, object_index, -1, 0, true, true),
            KeyCode::Down  => properties::resize_group(objects, object_index, 0, 1, true, true),
            KeyCode::Up    => properties::resize_group(objects, object_index, 0, -1, true, true),
            _ => {}
        }
    } else if is_table {
        // Table height auto-fits content, so seed vertical resizes from natural.
        match key.code {
            KeyCode::Right => properties::resize_object(&mut objects[object_index], 1, 0),
            KeyCode::Left  => properties::shrink_object(&mut objects[object_index], 1, 0),
            KeyCode::Down  => grow_table_height(&mut objects[object_index], frame, &anims, 1),
            KeyCode::Up    => grow_table_height(&mut objects[object_index], frame, &anims, -1),
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Right => properties::resize_object(&mut objects[object_index], 1, 0),
            KeyCode::Left  => properties::shrink_object(&mut objects[object_index], 1, 0),
            KeyCode::Down  => properties::resize_object(&mut objects[object_index], 0, 1),
            KeyCode::Up    => properties::shrink_object(&mut objects[object_index], 0, 1),
            _ => {}
        }
    }
    state.dirty = true;
    Action::Redraw
}

/// Grow (`delta > 0`) or shrink (`delta < 0`) a table's height by one row,
/// seeded from its natural content-fit height so the change is always visible.
/// Dropping back to (or below) the natural height stores 0, i.e. auto-fit, since
/// a table is never clipped below its content.
fn grow_table_height(obj: &mut SceneObject, frame: usize, anims: &AnimSpans, delta: i32) {
    if let SceneObject::Table(t) = obj {
        // Only adjust a fixed height; leave an animated height untouched.
        if !matches!(t.height, Coordinate::Fixed(_)) {
            return;
        }
        let natural = t.natural_height(frame, anims);
        let current = t.height.evaluate(frame, anims).max(natural);
        let new = (current as i32 + delta).max(0) as u16;
        t.height = if new <= natural {
            Coordinate::Fixed(0.0)
        } else {
            Coordinate::Fixed(new as f64)
        };
    }
}

fn handle_confirm(state: &mut EditorState, key: KeyEvent) -> Action {
    let (selected, action, return_mode) = match &state.mode {
        Mode::Confirm { selected, action, return_mode, .. } => {
            (*selected, action.clone(), (**return_mode).clone())
        }
        _ => return Action::Continue,
    };

    match key.code {
        KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
            let new_sel = if selected == 0 { 1 } else { 0 };
            if let Mode::Confirm { selected: sel, .. } = &mut state.mode {
                *sel = new_sel;
            }
            return Action::Redraw;
        }
        KeyCode::Enter => {
            if selected == 0 {
                // Yes — execute the action; compute the next mode
                let next_mode = match action {
                    ConfirmAction::DeleteFrame => {
                        let deleted = state.current_frame;
                        adjust_frames_after_delete(&mut state.source, deleted);
                        // Object indices may have shifted; the copy clipboard's
                        // source indices (for a linked paste) can no longer be
                        // trusted, so a later linked paste links copies only.
                        state.clipboard_sources.clear();
                        if state.current_frame >= state.source.frame_count {
                            state.current_frame = state.source.frame_count.saturating_sub(1);
                        }
                        state.dirty = true;
                        state.status_message = Some(format!(
                            "Deleted frame {} (now {})",
                            deleted + 1,
                            state.source.frame_count
                        ));
                        Mode::Normal
                    }
                    ConfirmAction::DeleteFrames { frames } => {
                        let removed = super::state::delete_frames(&mut state.source, &frames);
                        state.clipboard_sources.clear();
                        if state.current_frame >= state.source.frame_count {
                            state.current_frame = state.source.frame_count.saturating_sub(1);
                        }
                        state.dirty = true;
                        state.status_message = Some(format!(
                            "Deleted {removed} frame(s) (now {})",
                            state.source.frame_count
                        ));
                        Mode::Normal
                    }
                    ConfirmAction::DeleteObject { object_index } => {
                        if object_index < state.source.objects.len() {
                            // Same path as a multi-delete: an `Animation` goes as a
                            // whole (sidecar + the motion it drives), and an
                            // animation left with no driver is pruned.
                            let is_anim =
                                matches!(state.source.objects[object_index], SceneObject::Animation(_));
                            super::state::delete_objects(&mut state.source, &[object_index]);
                            state.status_message = Some(if is_anim {
                                "Animation removed (objects frozen at their start)".into()
                            } else {
                                "Object deleted".into()
                            });
                            // Source indices for a linked paste are now stale.
                            state.clipboard_sources.clear();
                            state.dirty = true;
                        }
                        Mode::Normal
                    }
                    ConfirmAction::DeleteObjects { object_indices } => {
                        let removed =
                            super::state::delete_objects(&mut state.source, &object_indices);
                        // Object indices shifted; a later linked paste can't trust
                        // the copied source indices any more.
                        state.clipboard_sources.clear();
                        state.dirty = true;
                        state.status_message =
                            Some(format!("Deleted {removed} object(s)"));
                        Mode::Normal
                    }
                    ConfirmAction::RemoveGroupMember {
                        group_index,
                        member_idx,
                        return_selected_property,
                        return_panel_scroll,
                    } => {
                        if let SceneObject::Group(g) = &mut state.source.objects[group_index] {
                            g.members.retain(|&m| m != member_idx);
                        }
                        state.dirty = true;
                        state.status_message = Some("Removed from group".into());
                        let new_count =
                            properties::get_properties(&state.source.objects, group_index).len();
                        Mode::EditProperties {
                            object_index: group_index,
                            selected_property: return_selected_property
                                .min(new_count.saturating_sub(1)),
                            editing_value: None,
                            cursor: 0,
                            scroll: 0,
                            panel_scroll: return_panel_scroll,
                            dropdown: None,
                        }
                    }
                    ConfirmAction::RemoveTableColumn { object_index, col_index } => {
                        if let SceneObject::Table(t) = &mut state.source.objects[object_index] {
                            table_remove_column(t, col_index);
                            state.dirty = true;
                            state.status_message = Some(format!("Removed column {}", col_index + 1));
                        }
                        let new_count =
                            properties::get_properties(&state.source.objects, object_index).len();
                        Mode::EditProperties {
                            object_index,
                            selected_property: 0_usize.min(new_count.saturating_sub(1)),
                            editing_value: None,
                            cursor: 0,
                            scroll: 0,
                            panel_scroll: 0,
                            dropdown: None,
                        }
                    }
                };
                state.mode = next_mode;
            } else {
                // No — return to the originating mode
                state.mode = return_mode;
            }
            return Action::Redraw;
        }
        KeyCode::Esc => {
            state.mode = return_mode;
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

fn handle_edit_properties(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.cancel, &key) {
        let object_index = match &state.mode {
            Mode::EditProperties { object_index, .. } => *object_index,
            _ => return Action::Continue,
        };
        state.mode = Mode::SelectedObject { object_index };
        return Action::Redraw;
    }

    let (object_index, selected_property, panel_scroll) = match &state.mode {
        Mode::EditProperties { object_index, selected_property, panel_scroll, .. } =>
            (*object_index, *selected_property, *panel_scroll),
        _ => return Action::Continue,
    };

    // Fetched once: `Property` owns its strings, so this does not borrow `state`
    // and the value/kind/name read below stay valid across the mutating calls.
    let props = properties::get_properties(&state.source.objects, object_index);
    let prop_count = props.len();
    let prop_kind = props[selected_property].kind.clone();
    let prop_name = props[selected_property].name;
    let prop_value = props[selected_property].value.clone();
    let term_h = terminal::size().map(|(_, h)| h).unwrap_or(24) as usize;

    use properties::PropertyKind;

    // GroupMember property: 'd' opens a removal confirmation; editing is blocked.
    // Navigation (Up/Down) is allowed to fall through to the code below.
    if prop_kind == PropertyKind::GroupMember {
        if matches_binding(&bindings.delete_object, &key) || key.code == KeyCode::Delete {
            let member_idx: usize = prop_value.parse().unwrap_or(usize::MAX);
            let member_summary = if member_idx < state.source.objects.len() {
                super::state::scene_object_summary(&state.source.objects[member_idx])
            } else {
                "?".to_string()
            };
            state.mode = Mode::Confirm {
                message: format!("Remove {} from group?", member_summary),
                selected: 0,
                action: ConfirmAction::RemoveGroupMember {
                    group_index: object_index,
                    member_idx,
                    return_selected_property: selected_property,
                    return_panel_scroll: panel_scroll,
                },
                return_mode: Box::new(ep_browse(object_index, selected_property, panel_scroll)),
            };
            return Action::Redraw;
        }
        // Block editing-start keys; navigation (Up/Down/Tab) falls through below.
        if matches_binding(&bindings.confirm, &key) || matches_binding(&bindings.animate, &key) {
            return Action::Continue;
        }
    }

    // ReadOnly / Note properties: block editing keys; allow navigation through.
    if matches!(prop_kind, PropertyKind::ReadOnly | PropertyKind::Note) {
        if matches_binding(&bindings.confirm, &key)
            || matches_binding(&bindings.animate, &key)
            || matches_binding(&bindings.delete_object, &key)
            || key.code == KeyCode::Delete
        {
            return Action::Continue;
        }
    }

    // Up/Down, Tab/BackTab: navigate the property list (scroll follows selection).
    if matches_binding(&bindings.move_up, &key) || key.code == KeyCode::BackTab {
        let new_sel = if selected_property == 0 { prop_count - 1 } else { selected_property - 1 };
        let ps = follow_panel_scroll(new_sel, panel_scroll, term_h);
        state.mode = ep_browse(object_index, new_sel, ps);
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key)
        || (key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE)
    {
        let new_sel = (selected_property + 1) % prop_count;
        let ps = follow_panel_scroll(new_sel, panel_scroll, term_h);
        state.mode = ep_browse(object_index, new_sel, ps);
        return Action::Redraw;
    }

    // Booleans flip in place on Space or Enter — no text-entry detour.
    let toggle_requested = matches_binding(&bindings.confirm, &key)
        || (key.code == KeyCode::Char(' ') && key.modifiers == KeyModifiers::NONE);
    if prop_kind == PropertyKind::Bool && toggle_requested {
        let removed = apply_property(state, object_index, prop_name, properties::toggled_bool_value(&prop_value));
        let object_index = super::state::index_after_removals(object_index, &removed);
        state.mode = ep_browse(object_index, selected_property, panel_scroll);
        return Action::Redraw;
    }
    // Space on a non-bool does nothing special; let it fall through (and be ignored).

    if matches_binding(&bindings.confirm, &key) {
        if let Some(opts) = properties::dropdown_options_for(&prop_kind) {
            // Open dropdown; pre-select the matching option if recognised.
            let dd_sel = opts.iter().position(|&o| o == prop_value).unwrap_or(0);
            state.mode = ep_dropdown(object_index, selected_property, dd_sel, panel_scroll);
        } else if prop_kind == PropertyKind::Coordinate {
            if let Some(coord) = properties::get_coord(&state.source.objects[object_index], prop_name) {
                if matches!(coord, Coordinate::Animated { .. }) {
                    state.mode = enter_animate(state, object_index, selected_property, prop_name);
                } else {
                    let cursor = prop_value.chars().count();
                    state.mode = ep_editing(object_index, selected_property, prop_value, cursor, 0, panel_scroll);
                }
            }
        } else {
            let cursor = prop_value.chars().count();
            state.mode = ep_editing(object_index, selected_property, prop_value, cursor, 0, panel_scroll);
        }
        return Action::Redraw;
    }

    // Table-specific key bindings (only active when editing a Table object)
    if matches!(state.source.objects.get(object_index), Some(SceneObject::Table(_))) {
        if matches_binding(&bindings.table_add_col_after, &key) {
            if let SceneObject::Table(t) = &state.source.objects[object_index] {
                let ncols = t.col_widths.len();
                state.mode = Mode::TableAddColumn {
                    object_index,
                    after: true,
                    col_num: ncols,
                    buf: ncols.to_string(),
                    cursor: ncols.to_string().len(),
                };
            }
            return Action::Redraw;
        }
        if matches_binding(&bindings.table_add_col_before, &key) {
            if let SceneObject::Table(t) = &state.source.objects[object_index] {
                let ncols = t.col_widths.len();
                state.mode = Mode::TableAddColumn {
                    object_index,
                    after: false,
                    col_num: 1,
                    buf: "1".to_string(),
                    cursor: 1,
                };
                let _ = ncols; // suppress unused warning
            }
            return Action::Redraw;
        }
        if matches_binding(&bindings.table_remove_col, &key) {
            if let SceneObject::Table(t) = &state.source.objects[object_index] {
                if t.col_widths.len() > 1 {
                    state.mode = Mode::TableRemoveColumn {
                        object_index,
                        col_num: 1,
                        buf: "1".to_string(),
                        cursor: 1,
                    };
                } else {
                    state.status_message = Some("Cannot remove the only column".into());
                }
            }
            return Action::Redraw;
        }
        if matches_binding(&bindings.table_edit_cells, &key) {
            state.mode = Mode::TableEditCellProps {
                object_index,
                cursor_row: 0,
                cursor_col: 0,
                selected_cells: Vec::new(),
                sub_state: TableCellSubState::Selecting,
            };
            return Action::Redraw;
        }
    }

    // [a]nimate: open AnimateProperty panel for Coordinate properties
    if matches_binding(&bindings.animate, &key) {
        if prop_kind == PropertyKind::Coordinate {
            state.mode = enter_animate(state, object_index, selected_property, prop_name);
            return Action::Redraw;
        }
    }

    Action::Continue
}

fn handle_edit_value(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, selected_property, editing_value, cursor, scroll, panel_scroll) =
        match &state.mode {
            Mode::EditProperties {
                object_index, selected_property, editing_value, cursor, scroll, panel_scroll, ..
            } => (*object_index, *selected_property, editing_value.clone(), *cursor, *scroll, *panel_scroll),
            _ => return Action::Continue,
        };

    let props = properties::get_properties(&state.source.objects, object_index);
    let prop_name = props[selected_property].name;
    // `Text` values are edited in the wide overlay (which derives its own scroll
    // from the cursor); everything else (numbers, coordinates, …) is a short
    // field edited in place in the narrow panel.
    let is_text = props[selected_property].kind == properties::PropertyKind::Text;
    let prefix0 = prop_name.chars().count() + 2;

    let newline = matches_binding(&state.config.key_bindings.insert_newline, &key);
    let mut te = TextEdit::new(editing_value.unwrap_or_default(), cursor);

    match te.handle_key(&key, newline) {
        TextAction::Ignored => Action::Continue,
        TextAction::Cancel => {
            state.mode = ep_browse(object_index, selected_property, panel_scroll);
            Action::Redraw
        }
        TextAction::Commit => {
            let removed = apply_property(state, object_index, prop_name, &te.buf);
            let object_index = super::state::index_after_removals(object_index, &removed);
            state.mode = ep_browse(object_index, selected_property, panel_scroll);
            Action::Redraw
        }
        TextAction::Edited => {
            let (new_scroll, new_ps) = if is_text {
                (0, panel_scroll)
            } else {
                panel_field_scrolls(&te, selected_property, prefix0, scroll, panel_scroll)
            };
            state.mode = ep_editing(object_index, selected_property, te.buf, te.cursor, new_scroll, new_ps);
            Action::Redraw
        }
    }
}

/// Horizontal + vertical scroll for editing a short value inside the narrow
/// right panel (coordinates, colors, char pickers). Keeps the cursor visible
/// without jumping the window while it is already in view.
fn panel_field_scrolls(
    te: &TextEdit,
    selected_property: usize,
    prefix0: usize,
    old_scroll: usize,
    old_ps: usize,
) -> (usize, usize) {
    let max_width = (super::ui::RIGHT_PANEL_WIDTH - 3) as usize;
    let (line, col) = te.line_col();
    let plen = if line == 0 { prefix0 } else { 2 };
    let horiz_w = max_width.saturating_sub(plen);
    let scroll = if col < old_scroll {
        col
    } else if horiz_w > 0 && col >= old_scroll + horiz_w {
        col + 1 - horiz_w
    } else {
        old_scroll
    };
    let term_h = terminal::size().map(|(_, h)| h).unwrap_or(24) as usize;
    let ps = follow_panel_scroll(selected_property + line, old_ps, term_h);
    (scroll, ps)
}

/// Outcome of a key in a dropdown list. Shared by the object-property dropdown
/// and the table cell-style colour dropdown so both navigate identically.
enum DropdownKey {
    Move(usize),
    Choose(usize),
    Cancel,
    Ignored,
}

fn dropdown_key(
    key: &KeyEvent,
    bindings: &super::config::KeyBindings,
    current: usize,
    count: usize,
) -> DropdownKey {
    if matches_binding(&bindings.cancel, key) {
        DropdownKey::Cancel
    } else if matches_binding(&bindings.move_up, key) {
        DropdownKey::Move(if current == 0 { count - 1 } else { current - 1 })
    } else if matches_binding(&bindings.move_down, key) {
        DropdownKey::Move((current + 1) % count)
    } else if matches_binding(&bindings.confirm, key) {
        DropdownKey::Choose(current)
    } else {
        DropdownKey::Ignored
    }
}

fn handle_dropdown(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, selected_property, dd_sel, panel_scroll) = match &state.mode {
        Mode::EditProperties { object_index, selected_property, panel_scroll, dropdown: Some(sel), .. } =>
            (*object_index, *selected_property, *sel, *panel_scroll),
        _ => return Action::Continue,
    };

    let props = properties::get_properties(&state.source.objects, object_index);
    let prop_kind = props[selected_property].kind.clone();
    let prop_name = props[selected_property].name;
    let prop_value = props[selected_property].value.clone();
    let options = properties::dropdown_options_for(&prop_kind).unwrap_or(properties::COLOR_OPTIONS);
    let sentinel = properties::dropdown_custom_sentinel(&prop_kind);
    let bindings = state.config.key_bindings.clone();

    match dropdown_key(&key, &bindings, dd_sel, options.len()) {
        DropdownKey::Ignored => Action::Continue,
        DropdownKey::Cancel => {
            state.mode = ep_browse(object_index, selected_property, panel_scroll);
            Action::Redraw
        }
        DropdownKey::Move(n) => {
            state.mode = ep_dropdown(object_index, selected_property, n, panel_scroll);
            Action::Redraw
        }
        DropdownKey::Choose(n) => {
            let chosen = options[n];
            if chosen == sentinel {
                // Switch to text input, seeding with the current value if useful.
                let initial = if prop_kind == properties::PropertyKind::Color {
                    if prop_value.starts_with('#') { prop_value.clone() } else { "#".to_string() }
                } else if prop_value != "auto" {
                    prop_value.clone()
                } else {
                    String::new()
                };
                let cursor = initial.chars().count();
                state.mode = ep_editing(object_index, selected_property, initial, cursor, 0, panel_scroll);
            } else {
                let removed = apply_property(state, object_index, prop_name, chosen);
                let object_index = super::state::index_after_removals(object_index, &removed);
                state.mode = ep_browse(object_index, selected_property, panel_scroll);
            }
            Action::Redraw
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-object property editing (Mode::EditMultiProperties)
//
// Slimmer cousins of the single-object handlers above: only the bulk-editable
// property kinds appear (no animate / table / group-member / multi-line text),
// and every edit fans out to the whole selection via `apply_multi_property`.
// ---------------------------------------------------------------------------

/// Browsing the common-property list of a multi-object selection.
fn handle_edit_multi_properties(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (members, selected_property, panel_scroll) = match &state.mode {
        Mode::EditMultiProperties { members, selected_property, panel_scroll, .. } =>
            (members.clone(), *selected_property, *panel_scroll),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        // Back to the action sub-menu with the selection intact.
        state.mode = Mode::SelectAction { members, selected: edit_props_action_index() };
        return Action::Redraw;
    }

    let props = properties::common_properties(&state.source.objects, &members);
    if props.is_empty() {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    let prop_count = props.len();
    let selected_property = selected_property.min(prop_count - 1);
    let prop_kind = props[selected_property].kind.clone();
    let prop_name = props[selected_property].name;
    let prop_value = props[selected_property].value.clone();
    let term_h = terminal::size().map(|(_, h)| h).unwrap_or(24) as usize;

    use properties::PropertyKind;

    // Up/Down, Tab/BackTab: navigate the property list (scroll follows selection).
    if matches_binding(&bindings.move_up, &key) || key.code == KeyCode::BackTab {
        let new_sel = if selected_property == 0 { prop_count - 1 } else { selected_property - 1 };
        let ps = follow_panel_scroll(new_sel, panel_scroll, term_h);
        state.mode = emp_browse(members, new_sel, ps);
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key)
        || (key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE)
    {
        let new_sel = (selected_property + 1) % prop_count;
        let ps = follow_panel_scroll(new_sel, panel_scroll, term_h);
        state.mode = emp_browse(members, new_sel, ps);
        return Action::Redraw;
    }

    // Booleans flip in place on Space or Enter.
    let toggle_requested = matches_binding(&bindings.confirm, &key)
        || (key.code == KeyCode::Char(' ') && key.modifiers == KeyModifiers::NONE);
    if prop_kind == PropertyKind::Bool && toggle_requested {
        let members = apply_multi_property(state, &members, prop_name, properties::toggled_bool_value(&prop_value));
        state.mode = emp_browse(members, selected_property, panel_scroll);
        return Action::Redraw;
    }

    // Enter: open a dropdown, or start an inline text edit. A `Coordinate` is
    // edited as a plain value here — animation is a single-object affair.
    if matches_binding(&bindings.confirm, &key) {
        if let Some(opts) = properties::dropdown_options_for(&prop_kind) {
            let dd_sel = opts.iter().position(|&o| o == prop_value).unwrap_or(0);
            state.mode = emp_dropdown(members, selected_property, dd_sel, panel_scroll);
        } else {
            let cursor = prop_value.chars().count();
            state.mode = emp_editing(members, selected_property, prop_value, cursor, 0, panel_scroll);
        }
        return Action::Redraw;
    }

    Action::Continue
}

/// Editing a common property's value as text; commit fans the value out to all.
fn handle_edit_multi_value(state: &mut EditorState, key: KeyEvent) -> Action {
    let (members, selected_property, editing_value, cursor, scroll, panel_scroll) =
        match &state.mode {
            Mode::EditMultiProperties {
                members, selected_property, editing_value, cursor, scroll, panel_scroll, ..
            } => (members.clone(), *selected_property, editing_value.clone(), *cursor, *scroll, *panel_scroll),
            _ => return Action::Continue,
        };

    let props = properties::common_properties(&state.source.objects, &members);
    if selected_property >= props.len() {
        state.mode = emp_browse(members, 0, panel_scroll);
        return Action::Redraw;
    }
    let prop_name = props[selected_property].name;
    let prefix0 = prop_name.chars().count() + 2;

    let newline = matches_binding(&state.config.key_bindings.insert_newline, &key);
    let mut te = TextEdit::new(editing_value.unwrap_or_default(), cursor);

    match te.handle_key(&key, newline) {
        TextAction::Ignored => Action::Continue,
        TextAction::Cancel => {
            state.mode = emp_browse(members, selected_property, panel_scroll);
            Action::Redraw
        }
        TextAction::Commit => {
            let members = apply_multi_property(state, &members, prop_name, &te.buf);
            state.mode = emp_browse(members, selected_property, panel_scroll);
            Action::Redraw
        }
        TextAction::Edited => {
            let (new_scroll, new_ps) =
                panel_field_scrolls(&te, selected_property, prefix0, scroll, panel_scroll);
            state.mode = emp_editing(members, selected_property, te.buf, te.cursor, new_scroll, new_ps);
            Action::Redraw
        }
    }
}

/// Navigating the dropdown of a common property (colour / alignment / …); a
/// choice fans out to the whole selection (or opens custom text entry).
fn handle_edit_multi_dropdown(state: &mut EditorState, key: KeyEvent) -> Action {
    let (members, selected_property, dd_sel, panel_scroll) = match &state.mode {
        Mode::EditMultiProperties { members, selected_property, panel_scroll, dropdown: Some(sel), .. } =>
            (members.clone(), *selected_property, *sel, *panel_scroll),
        _ => return Action::Continue,
    };

    let props = properties::common_properties(&state.source.objects, &members);
    if selected_property >= props.len() {
        state.mode = emp_browse(members, 0, panel_scroll);
        return Action::Redraw;
    }
    let prop_kind = props[selected_property].kind.clone();
    let prop_name = props[selected_property].name;
    let prop_value = props[selected_property].value.clone();
    let options = properties::dropdown_options_for(&prop_kind).unwrap_or(properties::COLOR_OPTIONS);
    let sentinel = properties::dropdown_custom_sentinel(&prop_kind);
    let bindings = state.config.key_bindings.clone();

    match dropdown_key(&key, &bindings, dd_sel, options.len()) {
        DropdownKey::Ignored => Action::Continue,
        DropdownKey::Cancel => {
            state.mode = emp_browse(members, selected_property, panel_scroll);
            Action::Redraw
        }
        DropdownKey::Move(n) => {
            state.mode = emp_dropdown(members, selected_property, n, panel_scroll);
            Action::Redraw
        }
        DropdownKey::Choose(n) => {
            let chosen = options[n];
            if chosen == sentinel {
                let initial = if prop_kind == properties::PropertyKind::Color {
                    if prop_value.starts_with('#') { prop_value.clone() } else { "#".to_string() }
                } else if prop_value != "auto" {
                    prop_value.clone()
                } else {
                    String::new()
                };
                let cursor = initial.chars().count();
                state.mode = emp_editing(members, selected_property, initial, cursor, 0, panel_scroll);
            } else {
                let members = apply_multi_property(state, &members, prop_name, chosen);
                state.mode = emp_browse(members, selected_property, panel_scroll);
            }
            Action::Redraw
        }
    }
}

/// One field in the Animate sub-menu. `XFrom`/`XTo` are the primary axis (x, or
/// width/height for a 1-D coordinate); `YFrom`/`YTo` appear only for a two-axis
/// (position) animation, so x and y can be set in one go.
#[derive(Clone, Copy, PartialEq)]
enum AnimRole {
    XFrom,
    XTo,
    YFrom,
    YTo,
    Start,
    End,
    AddFrames,
    AutoPlay,
    Delay,
    Gap,
}

const ANIM_ROLES_2D: &[AnimRole] = &[
    AnimRole::XFrom, AnimRole::XTo, AnimRole::YFrom, AnimRole::YTo, AnimRole::Start,
    AnimRole::End, AnimRole::AddFrames, AnimRole::AutoPlay, AnimRole::Delay, AnimRole::Gap,
];
const ANIM_ROLES_1D: &[AnimRole] = &[
    AnimRole::XFrom, AnimRole::XTo, AnimRole::Start, AnimRole::End,
    AnimRole::AddFrames, AnimRole::AutoPlay, AnimRole::Delay, AnimRole::Gap,
];

fn anim_roles(two_axis: bool) -> &'static [AnimRole] {
    if two_axis { ANIM_ROLES_2D } else { ANIM_ROLES_1D }
}

impl AnimRole {
    fn is_toggle(self) -> bool {
        matches!(self, AnimRole::AddFrames | AnimRole::AutoPlay)
    }
    fn label(self, two_axis: bool) -> &'static str {
        match self {
            AnimRole::XFrom => if two_axis { "x from" } else { "from" },
            AnimRole::XTo => if two_axis { "x to" } else { "to" },
            AnimRole::YFrom => "y from",
            AnimRole::YTo => "y to",
            AnimRole::Start => "start",
            AnimRole::End => "end",
            AnimRole::AddFrames => "add frames",
            AnimRole::AutoPlay => "auto play",
            AnimRole::Delay => "delay ms",
            AnimRole::Gap => "gap frames",
        }
    }
}

/// The display value of a role (a checkbox for the toggles, the number otherwise).
#[allow(clippy::too_many_arguments)]
fn anim_role_value(
    role: AnimRole, from: u16, to: u16, from_y: u16, to_y: u16,
    start_frame: usize, end_frame: usize, add_frames: bool, auto_play: bool,
    delay_ms: u64, gap_frames: usize,
) -> String {
    let cb = |b| if b { "[x]".to_string() } else { "[ ]".to_string() };
    match role {
        AnimRole::XFrom => from.to_string(),
        AnimRole::XTo => to.to_string(),
        AnimRole::YFrom => from_y.to_string(),
        AnimRole::YTo => to_y.to_string(),
        // start/end shown 1-based (matching first/last_frame).
        AnimRole::Start => (start_frame + 1).to_string(),
        AnimRole::End => (end_frame + 1).to_string(),
        AnimRole::AddFrames => cb(add_frames),
        AnimRole::AutoPlay => cb(auto_play),
        AnimRole::Delay => delay_ms.to_string(),
        AnimRole::Gap => gap_frames.to_string(),
    }
}

/// The Animate sub-menu's `(label, value)` rows, in display order — used by the
/// panel to render the fields without duplicating the role layout.
#[allow(clippy::too_many_arguments)]
pub(crate) fn anim_field_rows(
    two_axis: bool, from: u16, to: u16, from_y: u16, to_y: u16,
    start_frame: usize, end_frame: usize, add_frames: bool, auto_play: bool,
    delay_ms: u64, gap_frames: usize,
) -> Vec<(&'static str, String)> {
    anim_roles(two_axis)
        .iter()
        .map(|&r| {
            (
                r.label(two_axis),
                anim_role_value(r, from, to, from_y, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames),
            )
        })
        .collect()
}

/// Apply an edited numeric value for `role` into the matching local. Returns an
/// error message string on a parse failure (toggles are never edited here).
#[allow(clippy::too_many_arguments)]
fn anim_apply_edit(
    role: AnimRole, buf: &str, from: &mut u16, to: &mut u16, from_y: &mut u16, to_y: &mut u16,
    start_frame: &mut usize, end_frame: &mut usize, delay_ms: &mut u64, gap_frames: &mut usize,
) -> Option<String> {
    let bad = |e: std::num::ParseIntError| format!("Invalid number: {e}");
    match role {
        AnimRole::XFrom => buf.trim().parse::<u16>().map(|v| *from = v).err().map(bad),
        AnimRole::XTo => buf.trim().parse::<u16>().map(|v| *to = v).err().map(bad),
        AnimRole::YFrom => buf.trim().parse::<u16>().map(|v| *from_y = v).err().map(bad),
        AnimRole::YTo => buf.trim().parse::<u16>().map(|v| *to_y = v).err().map(bad),
        AnimRole::Start => buf.trim().parse::<usize>().map(|v| *start_frame = v.saturating_sub(1)).err().map(bad),
        AnimRole::End => buf.trim().parse::<usize>().map(|v| *end_frame = v.saturating_sub(1)).err().map(bad),
        AnimRole::Delay => buf.trim().parse::<u64>().map(|v| *delay_ms = v).err().map(bad),
        // gap of 0 is meaningless; clamp to 1 (every frame).
        // `gap_frames` is the count of empty frames between appearances (0 = off).
        AnimRole::Gap => buf.trim().parse::<usize>().map(|v| *gap_frames = v).err().map(bad),
        AnimRole::AddFrames | AnimRole::AutoPlay => None,
    }
}

/// Build the `AnimateProperty` mode in one place (the variant has many fields).
#[allow(clippy::too_many_arguments)]
fn anim_mode(
    object_index: usize, return_property: usize, property_name: &'static str,
    selected_field: usize, editing: Option<String>, cursor: usize,
    from: u16, to: u16, from_y: u16, to_y: u16, two_axis: bool,
    start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64, gap_frames: usize,
) -> Mode {
    Mode::AnimateProperty {
        object_index, return_property, property_name, selected_field, editing, cursor,
        from, to, from_y, to_y, two_axis, start_frame, end_frame,
        add_frames, auto_play, delay_ms, gap_frames,
    }
}

/// Open the Animate sub-menu for `property_name`, reading the object's current
/// coordinate(s) to seed the fields. Animating `x` or `y` on an object that has
/// both becomes a **two-axis** session (x and y set together); every other
/// coordinate (width/height) stays single-axis. The span and auto-play config
/// are seeded from an existing animation on the coordinate / matching span.
fn enter_animate(
    state: &EditorState, object_index: usize, return_property: usize, property_name: &'static str,
) -> Mode {
    let obj = &state.source.objects[object_index];
    let anims = AnimSpans::of(&state.source);
    // Read a coordinate's (from, to, referenced-animation-id).
    let read = |name: &str| -> (u16, u16, Option<AnimId>) {
        match properties::get_coord(obj, name) {
            Some(Coordinate::Fixed(v)) => {
                let n = v.max(0.0).floor() as u16;
                (n, n, None)
            }
            Some(Coordinate::Animated { from, to, anim }) => (from, to, Some(anim)),
            None => (0, 0, None),
        }
    };
    let two_axis = (property_name == "x" || property_name == "y")
        && properties::get_coord(obj, "x").is_some()
        && properties::get_coord(obj, "y").is_some();
    let (from, to, from_y, to_y) = if two_axis {
        let (xf, xt, _) = read("x");
        let (yf, yt, _) = read("y");
        (xf, xt, yf, yt)
    } else {
        let (f, t, _) = read(property_name);
        (f, t, 0, 0)
    };
    let anim_id = edited_anim_id(obj, property_name, two_axis);
    // Span comes from the referenced animation (the single source of truth); the
    // `end_frame` field is shown 1-based-inclusive, so it's `frames.end - 1`.
    let (start_frame, end_frame) = anim_id
        .and_then(|id| anims.span(id))
        .map(|fr| (fr.start, fr.end.saturating_sub(1)))
        .unwrap_or((state.current_frame, state.source.frame_count.saturating_sub(1)));
    let (auto_play, delay_ms, gap_frames) = anim_id
        .and_then(|id| state.source.objects.iter().find_map(|o| match o {
            SceneObject::Animation(a) if a.id == id => Some((a.auto_play, a.delay_ms, a.gap_frames)),
            _ => None,
        }))
        .unwrap_or((true, 500, 0));
    anim_mode(
        object_index, return_property, property_name, 0, None, 0,
        from, to, from_y, to_y, two_axis, start_frame, end_frame, true, auto_play, delay_ms, gap_frames,
    )
}

/// The animation driving the coordinate(s) an animate session edits — x's (else
/// y's) for a two-axis session, otherwise `property_name`'s — or `None` if they
/// are static. Other coordinates' animations never count: animating a new
/// coordinate must not retime an unrelated motion on the same object.
fn edited_anim_id(obj: &SceneObject, property_name: &str, two_axis: bool) -> Option<AnimId> {
    let anim = |name: &str| match properties::get_coord(obj, name) {
        Some(Coordinate::Animated { anim, .. }) => Some(anim),
        _ => None,
    };
    if two_axis {
        anim("x").or_else(|| anim("y"))
    } else {
        anim(property_name)
    }
}

fn handle_animate_property(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, return_property, property_name, selected_field, editing, cursor,
         mut from, mut to, mut from_y, mut to_y, two_axis,
         mut start_frame, mut end_frame, mut add_frames, mut auto_play, mut delay_ms, mut gap_frames) =
        match &state.mode {
            Mode::AnimateProperty {
                object_index, return_property, property_name, selected_field, editing, cursor,
                from, to, from_y, to_y, two_axis, start_frame, end_frame,
                add_frames, auto_play, delay_ms, gap_frames,
            } => (
                *object_index, *return_property, *property_name, *selected_field,
                editing.clone(), *cursor, *from, *to, *from_y, *to_y, *two_axis,
                *start_frame, *end_frame, *add_frames, *auto_play, *delay_ms, *gap_frames,
            ),
            _ => return Action::Continue,
        };

    let roles = anim_roles(two_axis);
    let role = roles[selected_field.min(roles.len() - 1)];

    // Rebuild the mode from the (possibly mutated) locals.
    macro_rules! rebuild {
        ($editing:expr, $cursor:expr, $field:expr) => {
            anim_mode(object_index, return_property, property_name, $field, $editing, $cursor,
                from, to, from_y, to_y, two_axis, start_frame, end_frame,
                add_frames, auto_play, delay_ms, gap_frames)
        };
    }

    // -- Editing a numeric field value -----------------------------------------
    if let Some(mut buf) = editing {
        let (new_editing, new_cursor): (Option<String>, usize) = match key.code {
            KeyCode::Enter => {
                if let Some(msg) = anim_apply_edit(
                    role, &buf, &mut from, &mut to, &mut from_y, &mut to_y,
                    &mut start_frame, &mut end_frame, &mut delay_ms, &mut gap_frames,
                ) {
                    state.status_message = Some(msg);
                }
                (None, 0)
            }
            KeyCode::Esc => (None, 0),
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => (Some(buf), cursor.saturating_sub(1)),
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                let c = (cursor + 1).min(buf.chars().count());
                (Some(buf), c)
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    let s = char_to_byte_idx(&buf, cursor - 1);
                    let e = char_to_byte_idx(&buf, cursor);
                    buf.drain(s..e);
                    (Some(buf), cursor - 1)
                } else {
                    (Some(buf), cursor)
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let byte_idx = char_to_byte_idx(&buf, cursor);
                buf.insert(byte_idx, c);
                (Some(buf), cursor + 1)
            }
            _ => return Action::Continue,
        };
        state.mode = rebuild!(new_editing, new_cursor, selected_field);
        return Action::Redraw;
    }

    // -- Browsing fields -------------------------------------------------------
    match key.code {
        KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
            let new_sel = if selected_field == 0 { roles.len() - 1 } else { selected_field - 1 };
            state.mode = rebuild!(None, 0, new_sel);
            return Action::Redraw;
        }
        KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            let new_sel = (selected_field + 1) % roles.len();
            state.mode = rebuild!(None, 0, new_sel);
            return Action::Redraw;
        }
        // Space / Enter on a boolean field toggles it in place (no text detour).
        KeyCode::Char(' ') | KeyCode::Enter if role.is_toggle() => {
            match role {
                AnimRole::AddFrames => add_frames = !add_frames,
                AnimRole::AutoPlay => auto_play = !auto_play,
                _ => {}
            }
            state.mode = rebuild!(None, 0, selected_field);
            return Action::Redraw;
        }
        KeyCode::Enter => {
            // Start editing the selected numeric field (seed with its value).
            let init = anim_role_value(role, from, to, from_y, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames);
            let new_cursor = init.chars().count();
            state.mode = rebuild!(Some(init), new_cursor, selected_field);
            return Action::Redraw;
        }
        // [s] apply → animate the coordinate(s) (+ optional add-frames/auto-play).
        KeyCode::Char('s') if key.modifiers == KeyModifiers::NONE => {
            // A strobe copy stands for its whole set: animate the original, which
            // clears every copy above it and re-strobes once.
            let object_index = super::state::strobe_set(&state.source, object_index)[0];
            // Refuse an out-of-range span up front (the menu stays open).
            let inserts = add_frames
                && edited_anim_id(&state.source.objects[object_index], property_name, two_axis).is_none();
            if let Err(e) = check_anim_span(&state.source, state.current_frame, start_frame, end_frame, inserts) {
                state.status_message = Some(format!("Error: {e}"));
                return Action::Redraw;
            }
            if gap_frames > 0 {
                if let Some(e) = super::state::strobe_group_error(&state.source, &[object_index]) {
                    state.status_message = Some(format!("Error: {e}"));
                    return Action::Redraw;
                }
            }
            let object_index = apply_animation(state, object_index, property_name, from, to, from_y, to_y, two_axis,
                start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames);
            state.mode = ep_browse(object_index, return_property, 0);
            return Action::Redraw;
        }
        // [x] clear → Fixed coordinate(s). Also removes any gap-strobe copies so
        // clearing doesn't leave the element scattered on its old sample frames.
        KeyCode::Char('x') if key.modifiers == KeyModifiers::NONE => {
            // Revert the whole strobe set through its original (see [s]).
            let mut object_index = super::state::strobe_set(&state.source, object_index)[0];
            let anims = AnimSpans::of(&state.source);
            if let Some((lo, hi)) = super::state::scene_object_animation_span(&state.source.objects[object_index], &anims) {
                let removed = super::state::clear_gap_clones(&mut state.source, object_index, lo, hi.saturating_sub(1));
                state.note_removed_objects(&removed);
            }
            let obj = &mut state.source.objects[object_index];
            let res = if two_axis {
                properties::set_coordinate(obj, "x", Coordinate::Fixed(from as f64))
                    .and_then(|_| properties::set_coordinate(obj, "y", Coordinate::Fixed(from_y as f64)))
            } else {
                properties::set_coordinate(obj, property_name, Coordinate::Fixed(from as f64))
            };
            match res {
                Ok(()) => {
                    // Drop any animation no coordinate references any more (the
                    // one we just cleared, if nothing else still drives it). One
                    // stored below the object shifts it down.
                    let removed = super::state::prune_orphan_animations(&mut state.source);
                    state.note_removed_objects(&removed);
                    object_index = super::state::index_after_removals(object_index, &removed);
                    // Its other animations (if any) still show and strobe it.
                    super::state::restrobe(&mut state.source, object_index, &anims);
                    state.dirty = true;
                    state.status_message = Some(if two_axis {
                        format!("Fixed position = ({from}, {from_y})")
                    } else {
                        format!("Fixed {property_name} = {from}")
                    });
                }
                Err(e) => state.status_message = Some(format!("Error: {e}")),
            }
            state.mode = ep_browse(object_index, return_property, 0);
            return Action::Redraw;
        }
        KeyCode::Esc => {
            state.mode = ep_browse(object_index, return_property, 0);
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

/// Check an animate/converge span before anything is mutated: the deck after
/// inserting the span's frames (`inserts`, i.e. *add frames* on a new animation)
/// must pass `validate_contract` (the deck-size limits), and the span must end
/// inside that deck. Inserted frames go right after `current` and share its
/// elements, so an inserting span must start on `current` — any other start
/// would leave the new frames and the animation's span disagreeing. A static
/// span (`start >= end`) inserts nothing and is fine.
fn check_anim_span(
    source: &SourcePresentation,
    current: usize,
    start: usize,
    end_frame: usize,
    inserts: bool,
) -> Result<(), String> {
    if start >= end_frame {
        return Ok(());
    }
    if inserts && start != current {
        return Err(format!(
            "with add frames on, start must be the current frame ({}); \
             turn add frames off to animate over existing frames",
            current + 1
        ));
    }
    let added = if inserts { end_frame - start } else { 0 };
    let frame_count = source.frame_count.saturating_add(added);
    if let Some(e) = super::state::frame_growth_error(source, added) {
        return Err(e);
    }
    if end_frame >= frame_count {
        return Err(format!("end frame {} is past the deck's last frame {frame_count}", end_frame + 1));
    }
    Ok(())
}

/// Apply the animation configured in the Animate sub-menu: optionally insert the
/// spanned frames and share the current frame's elements across them, set the
/// animated coordinate, keep the object's own range in lock-step with its
/// animation, and create/update the Animation span entity (auto-play config).
/// Returns the object's index afterwards (pruning an orphan animation stored
/// below it shifts it down).
#[allow(clippy::too_many_arguments)]
fn apply_animation(
    state: &mut EditorState, object_index: usize, property_name: &'static str,
    from: u16, to: u16, from_y: u16, to_y: u16, two_axis: bool,
    start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64, gap_frames: usize,
) -> usize {
    let animate = start_frame < end_frame;
    let end_excl = end_frame + 1;

    // Reuse the animation the edited coordinate(s) already reference, so editing
    // a span updates the *same* animation (the single source of truth) instead of
    // spawning a second one. Allocate a fresh id only for a brand-new animation.
    let existing_id = edited_anim_id(&state.source.objects[object_index], property_name, two_axis);
    let id = existing_id.unwrap_or_else(|| super::state::next_anim_id(&state.source));

    // Remove any prior gap-strobe copies of this element (matched by its *current*
    // animated coords, before we change them), so re-applying is idempotent.
    let anims_now = AnimSpans::of(&state.source);
    if let Some((lo, hi)) = super::state::scene_object_animation_span(&state.source.objects[object_index], &anims_now) {
        let removed = super::state::clear_gap_clones(&mut state.source, object_index, lo, hi.saturating_sub(1));
        state.note_removed_objects(&removed);
    }
    // Re-editing an animation other objects share (a Converge) re-times them
    // all: clear their strobe copies too, while the old gap still applies.
    let mut object_index = object_index;
    if let Some(id) = existing_id {
        let removed = super::state::clear_driven_gap_clones(&mut state.source, id, &anims_now);
        state.note_removed_objects(&removed);
        object_index = super::state::index_after_removals(object_index, &removed);
    }

    // "Add frames" inserts the span's frames (and shares the current frame's
    // elements). Only for a *brand-new* animation — re-editing an existing one
    // must never insert again.
    if animate && add_frames && existing_id.is_none() {
        let moved = super::state::add_frames_and_share(
            &mut state.source, state.current_frame, start_frame, end_frame, &[object_index]);
        object_index = moved[object_index];
        state.note_moved_objects(&moved);
    }

    // Record/Update the animation (its span + playback config) — the single home
    // of the span. `gap_frames` is editor metadata, recovered when re-animating.
    if animate {
        super::state::ensure_animation(&mut state.source, id, start_frame, end_excl, auto_play, delay_ms, gap_frames);
    }

    // Set the coordinate(s) to reference the animation (or stay Fixed if static).
    if let Err(e) = set_object_animation(
        &mut state.source, object_index, property_name,
        from, to, from_y, to_y, two_axis, animate, id,
    ) {
        state.status_message = Some(format!("Error: {e}"));
        return object_index;
    }
    state.dirty = true;

    // Lock the object's range to its animations, strobing it over each gapped
    // one's span (`gap_frames` was stored on the animation above). Every old copy
    // was cleared above, so this re-makes another animation's strobe too. A
    // re-edited animation's other driven objects get the same, so none keeps its
    // old visibility range (vanishing mid-motion or frozen past the span); they
    // go first, listed while copy-free, before this object's restrobe appends
    // copies driven by the same animation.
    if let Some(id) = existing_id {
        super::state::restrobe_all_driven(&mut state.source, id, Some(object_index), &anims_now);
    }
    super::state::restrobe(&mut state.source, object_index, &anims_now);

    // If no axis actually moved (from == to on all), the coordinates stayed Fixed
    // and the animation we just ensured drives nothing — drop it so a no-op apply
    // never leaves an inert animation behind.
    let removed = super::state::prune_orphan_animations(&mut state.source);
    state.note_removed_objects(&removed);

    // A new animation can collide with a loop (a loop may not bisect an
    // animation); surface that live, the way property edits do.
    let what = if two_axis { "position" } else { property_name };
    state.status_message = Some(match state.source.validate_loops() {
        Ok(()) => format!("Animated {what}"),
        Err(e) => format!("Animated {what} — ⚠ {e}"),
    });
    super::state::index_after_removals(object_index, &removed)
}

/// Set the animated coordinate(s) for one object to reference animation `anim`.
/// A coordinate becomes `Animated` only when `animate` is set *and* the value
/// moves (`from != to`); an unchanged axis stays `Fixed`, so a two-axis session
/// can animate just one axis. The span lives on the animation, not here.
#[allow(clippy::too_many_arguments)]
fn set_object_animation(
    source: &mut SourcePresentation, object_index: usize, property_name: &str,
    from: u16, to: u16, from_y: u16, to_y: u16, two_axis: bool,
    animate: bool, anim: AnimId,
) -> anyhow::Result<()> {
    let coord = |f: u16, t: u16| {
        if animate && f != t {
            Coordinate::Animated { from: f, to: t, anim }
        } else {
            Coordinate::Fixed(f as f64)
        }
    };
    let obj = &mut source.objects[object_index];
    if two_axis {
        properties::set_coordinate(obj, "x", coord(from, to))?;
        properties::set_coordinate(obj, "y", coord(from_y, to_y))?;
    } else {
        properties::set_coordinate(obj, property_name, coord(from, to))?;
    }
    Ok(())
}

/// The Converge config fields, in display order. Reuses the Animate roles but
/// drops `XFrom`/`YFrom` — each object's *from* is its own current position,
/// seeded per-object at apply time, not edited here.
const CONVERGE_ROLES: &[AnimRole] = &[
    AnimRole::XTo, AnimRole::YTo, AnimRole::Start, AnimRole::End,
    AnimRole::AddFrames, AnimRole::AutoPlay, AnimRole::Delay, AnimRole::Gap,
];

/// The Converge config's `(label, value)` rows, in display order — used by the
/// panel to render the fields without duplicating the role layout.
pub(crate) fn converge_field_rows(
    to: u16, to_y: u16, start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64, gap_frames: usize,
) -> Vec<(&'static str, String)> {
    CONVERGE_ROLES
        .iter()
        .map(|&r| {
            (
                // Two-axis labels ("x to" / "y to"); `from` args are unused here.
                r.label(true),
                anim_role_value(r, 0, to, 0, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames),
            )
        })
        .collect()
}

/// Open the Converge config menu for the chosen `members`. The shared target is
/// seeded to the centroid of the members' current positions at the current frame
/// (falling back to the canvas centre), so the default is a sensible "meet in the
/// middle"; the span defaults to current..last frame.
fn enter_converge(state: &EditorState, members: Vec<usize>) -> Mode {
    let start_frame = state.current_frame;
    let end_frame = state.source.frame_count.saturating_sub(1);
    let anims = AnimSpans::of(&state.source);
    let (mut sx, mut sy, mut nx, mut ny) = (0u32, 0u32, 0u32, 0u32);
    for &idx in &members {
        let obj = &state.source.objects[idx];
        if let Some(c) = properties::get_coord(obj, "x") {
            sx += c.evaluate(start_frame, &anims) as u32;
            nx += 1;
        }
        if let Some(c) = properties::get_coord(obj, "y") {
            sy += c.evaluate(start_frame, &anims) as u32;
            ny += 1;
        }
    }
    let to = if nx > 0 { (sx / nx) as u16 } else { state.source.width / 2 };
    let to_y = if ny > 0 { (sy / ny) as u16 } else { state.source.height / 2 };
    Mode::ConvergeConfig {
        members, selected_field: 0, editing: None, cursor: 0,
        to, to_y, start_frame, end_frame,
        add_frames: false, auto_play: true, delay_ms: 500, gap_frames: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn converge_mode(
    members: Vec<usize>, selected_field: usize, editing: Option<String>, cursor: usize,
    to: u16, to_y: u16, start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64, gap_frames: usize,
) -> Mode {
    Mode::ConvergeConfig {
        members, selected_field, editing, cursor,
        to, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames,
    }
}

fn handle_converge_config(state: &mut EditorState, key: KeyEvent) -> Action {
    let (members, selected_field, editing, cursor,
         mut to, mut to_y, mut start_frame, mut end_frame,
         mut add_frames, mut auto_play, mut delay_ms, mut gap_frames) = match &state.mode {
        Mode::ConvergeConfig {
            members, selected_field, editing, cursor,
            to, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames,
        } => (
            members.clone(), *selected_field, editing.clone(), *cursor,
            *to, *to_y, *start_frame, *end_frame, *add_frames, *auto_play, *delay_ms, *gap_frames,
        ),
        _ => return Action::Continue,
    };

    let role = CONVERGE_ROLES[selected_field.min(CONVERGE_ROLES.len() - 1)];

    macro_rules! rebuild {
        ($editing:expr, $cursor:expr, $field:expr) => {
            converge_mode(members.clone(), $field, $editing, $cursor,
                to, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames)
        };
    }

    // -- Editing a numeric field value -----------------------------------------
    if let Some(mut buf) = editing {
        let (new_editing, new_cursor): (Option<String>, usize) = match key.code {
            KeyCode::Enter => {
                // `from`/`from_y` are per-object; pass throwaway locals.
                let (mut f, mut fy) = (0u16, 0u16);
                if let Some(msg) = anim_apply_edit(
                    role, &buf, &mut f, &mut to, &mut fy, &mut to_y,
                    &mut start_frame, &mut end_frame, &mut delay_ms, &mut gap_frames,
                ) {
                    state.status_message = Some(msg);
                }
                (None, 0)
            }
            KeyCode::Esc => (None, 0),
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => (Some(buf), cursor.saturating_sub(1)),
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                let c = (cursor + 1).min(buf.chars().count());
                (Some(buf), c)
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    let s = char_to_byte_idx(&buf, cursor - 1);
                    let e = char_to_byte_idx(&buf, cursor);
                    buf.drain(s..e);
                    (Some(buf), cursor - 1)
                } else {
                    (Some(buf), cursor)
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let byte_idx = char_to_byte_idx(&buf, cursor);
                buf.insert(byte_idx, c);
                (Some(buf), cursor + 1)
            }
            _ => return Action::Continue,
        };
        state.mode = rebuild!(new_editing, new_cursor, selected_field);
        return Action::Redraw;
    }

    // -- Browsing fields -------------------------------------------------------
    match key.code {
        KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
            let new_sel = if selected_field == 0 { CONVERGE_ROLES.len() - 1 } else { selected_field - 1 };
            state.mode = rebuild!(None, 0, new_sel);
            return Action::Redraw;
        }
        KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            let new_sel = (selected_field + 1) % CONVERGE_ROLES.len();
            state.mode = rebuild!(None, 0, new_sel);
            return Action::Redraw;
        }
        KeyCode::Char(' ') | KeyCode::Enter if role.is_toggle() => {
            match role {
                AnimRole::AddFrames => add_frames = !add_frames,
                AnimRole::AutoPlay => auto_play = !auto_play,
                _ => {}
            }
            state.mode = rebuild!(None, 0, selected_field);
            return Action::Redraw;
        }
        KeyCode::Enter => {
            let init = anim_role_value(role, 0, to, 0, to_y, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames);
            let new_cursor = init.chars().count();
            state.mode = rebuild!(Some(init), new_cursor, selected_field);
            return Action::Redraw;
        }
        // [s] apply → converge every member onto the shared point.
        KeyCode::Char('s') if key.modifiers == KeyModifiers::NONE => {
            // Refuse an out-of-range span up front (the menu stays open).
            if let Err(e) = check_anim_span(&state.source, state.current_frame, start_frame, end_frame, add_frames) {
                state.status_message = Some(format!("Error: {e}"));
                return Action::Redraw;
            }
            if gap_frames > 0 {
                if let Some(e) = super::state::strobe_group_error(&state.source, &members) {
                    state.status_message = Some(format!("Error: {e}"));
                    return Action::Redraw;
                }
            }
            apply_converge(state, &members, to, to_y, start_frame, end_frame,
                add_frames, auto_play, delay_ms, gap_frames);
            state.mode = Mode::Normal;
            return Action::Redraw;
        }
        KeyCode::Esc => {
            state.mode = Mode::Normal;
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

/// Apply a converge animation: every member animates from wherever it sits at
/// `start_frame` to the shared `(to, to_y)` point over the span. Inserts the
/// spanned frames once (if `add_frames`), animates each member's axes (whichever
/// it has), records **one** shared `Animation` sidecar, and strobes each member
/// when `gap_frames > 0`.
#[allow(clippy::too_many_arguments)]
fn apply_converge(
    state: &mut EditorState, members: &[usize], to: u16, to_y: u16,
    start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64, gap_frames: usize,
) {
    if start_frame >= end_frame {
        state.status_message = Some("Converge needs end after start".into());
        return;
    }
    let end_excl = end_frame + 1;
    // Evaluate every member's *current* displayed position from the pre-edit
    // animation table (before we touch anything).
    let anims = AnimSpans::of(&state.source);

    // Clear each member's prior gap-strobe copies (by its current span) so
    // re-applying is idempotent. Removing copies shifts every index above them,
    // so remap the remaining members after each pass (a member that was itself a
    // copy is dropped). Sorted, so every removed copy lies after position `k`.
    // A strobe copy stands for its whole set: converge its original.
    let mut members: Vec<usize> =
        members.iter().map(|&m| super::state::strobe_set(&state.source, m)[0]).collect();
    members.sort_unstable();
    members.dedup();
    let mut k = 0;
    while k < members.len() {
        let idx = members[k];
        if let Some((lo, hi)) = super::state::scene_object_animation_span(&state.source.objects[idx], &anims) {
            let removed = super::state::clear_gap_clones(&mut state.source, idx, lo, hi.saturating_sub(1));
            state.note_removed_objects(&removed);
            members.retain(|m| !removed.contains(m));
            for m in &mut members {
                *m -= removed.iter().filter(|&&r| r < *m).count();
            }
        }
        k += 1;
    }

    // Insert + share the spanned frames once (if requested).
    if add_frames {
        let moved = super::state::add_frames_and_share(
            &mut state.source, state.current_frame, start_frame, end_frame, &members);
        for m in &mut members {
            *m = moved[*m];
        }
        state.note_moved_objects(&moved);
    }

    // One shared animation drives the whole convergence.
    let id = super::state::next_anim_id(&state.source);
    super::state::ensure_animation(&mut state.source, id, start_frame, end_excl, auto_play, delay_ms, gap_frames);

    // Each member animates whichever of x/y it has, from where it sits now toward
    // the shared target, referencing the one shared animation.
    let mut animated: Vec<usize> = Vec::new();
    let mut errors = 0;
    for &idx in &members {
        let obj = &state.source.objects[idx];
        let fx = properties::get_coord(obj, "x").map(|c| c.evaluate(start_frame, &anims));
        let fy = properties::get_coord(obj, "y").map(|c| c.evaluate(start_frame, &anims));
        let res = match (fx, fy) {
            (Some(fx), Some(fy)) =>
                set_object_animation(&mut state.source, idx, "x", fx, to, fy, to_y, true, true, id),
            (Some(fx), None) =>
                set_object_animation(&mut state.source, idx, "x", fx, to, 0, 0, false, true, id),
            (None, Some(fy)) =>
                set_object_animation(&mut state.source, idx, "y", fy, to_y, 0, 0, false, true, id),
            (None, None) => continue, // no coordinate to converge (Group/Loop/etc.)
        };
        match res {
            Ok(()) => animated.push(idx),
            Err(_) => errors += 1,
        }
    }

    // Lock each member's range to its animations, strobing it over every gapped
    // one (the shared animation stores `gap_frames`).
    for &idx in &animated {
        super::state::restrobe(&mut state.source, idx, &anims);
    }

    // Drop the shared animation if nothing actually moved, and any of the members'
    // previous animations left without a referrer by the convergence.
    let removed = super::state::prune_orphan_animations(&mut state.source);
    state.note_removed_objects(&removed);

    if animated.is_empty() {
        state.status_message = Some("Nothing to converge (selected objects have no position)".into());
        return;
    }
    state.dirty = true;

    let n = animated.len();
    let mut msg = format!("Converged {n} object{}", if n == 1 { "" } else { "s" });
    if errors > 0 {
        msg.push_str(&format!(" ({errors} failed)"));
    }
    state.status_message = Some(match state.source.validate_loops() {
        Ok(()) => msg,
        Err(e) => format!("{msg} — ⚠ {e}"),
    });
}

/// Convert a char index into a byte index for string operations.
fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(s.len())
}

// ---------------------------------------------------------------------------
// Table: add column
// ---------------------------------------------------------------------------

fn handle_table_add_column(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (object_index, after, mut col_num, mut buf, mut cursor) = match &state.mode {
        Mode::TableAddColumn { object_index, after, col_num, buf, cursor } => {
            (*object_index, *after, *col_num, buf.clone(), *cursor)
        }
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::EditProperties {
            object_index, selected_property: 0, editing_value: None,
            cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
        };
        return Action::Redraw;
    }

    if matches_binding(&bindings.confirm, &key) {
        let ncols = match &state.source.objects[object_index] {
            SceneObject::Table(t) => t.col_widths.len(),
            _ => return Action::Redraw,
        };
        let clamped = col_num.max(1).min(ncols);
        let insert_idx = if after { clamped } else { clamped.saturating_sub(1) };
        if let SceneObject::Table(t) = &mut state.source.objects[object_index] {
            table_add_column(t, insert_idx);
            state.dirty = true;
            state.status_message = Some(format!(
                "Added column {} {}", if after { "after" } else { "before" }, clamped
            ));
        }
        state.mode = Mode::EditProperties {
            object_index, selected_property: 0, editing_value: None,
            cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
        };
        return Action::Redraw;
    }

    // Digit input
    match key.code {
        KeyCode::Char(c) if c.is_ascii_digit() && key.modifiers == KeyModifiers::NONE => {
            let byte_idx = char_to_byte_idx(&buf, cursor);
            buf.insert(byte_idx, c);
            cursor += 1;
            if let Ok(n) = buf.parse::<usize>() {
                col_num = n;
            }
            state.mode = Mode::TableAddColumn { object_index, after, col_num, buf, cursor };
            return Action::Redraw;
        }
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
            if cursor > 0 {
                let byte_idx = char_to_byte_idx(&buf, cursor - 1);
                buf.remove(byte_idx);
                cursor -= 1;
                col_num = buf.parse().unwrap_or(1);
            }
            state.mode = Mode::TableAddColumn { object_index, after, col_num, buf, cursor };
            return Action::Redraw;
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            cursor = cursor.saturating_sub(1);
            state.mode = Mode::TableAddColumn { object_index, after, col_num, buf, cursor };
            return Action::Redraw;
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            cursor = (cursor + 1).min(buf.chars().count());
            state.mode = Mode::TableAddColumn { object_index, after, col_num, buf, cursor };
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

// ---------------------------------------------------------------------------
// Table: remove column
// ---------------------------------------------------------------------------

fn handle_table_remove_column(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (object_index, mut col_num, mut buf, mut cursor) = match &state.mode {
        Mode::TableRemoveColumn { object_index, col_num, buf, cursor } => {
            (*object_index, *col_num, buf.clone(), *cursor)
        }
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::EditProperties {
            object_index, selected_property: 0, editing_value: None,
            cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
        };
        return Action::Redraw;
    }

    if matches_binding(&bindings.confirm, &key) {
        let ncols = match &state.source.objects[object_index] {
            SceneObject::Table(t) => t.col_widths.len(),
            _ => return Action::Redraw,
        };
        let clamped = col_num.max(1).min(ncols);
        let col_idx = clamped.saturating_sub(1);
        // Transition to confirmation
        state.mode = Mode::Confirm {
            message: format!("Remove column {}?", clamped),
            selected: 0,
            action: ConfirmAction::RemoveTableColumn { object_index, col_index: col_idx },
            return_mode: Box::new(Mode::TableRemoveColumn {
                object_index, col_num: clamped, buf: clamped.to_string(),
                cursor: clamped.to_string().len(),
            }),
        };
        return Action::Redraw;
    }

    match key.code {
        KeyCode::Char(c) if c.is_ascii_digit() && key.modifiers == KeyModifiers::NONE => {
            let byte_idx = char_to_byte_idx(&buf, cursor);
            buf.insert(byte_idx, c);
            cursor += 1;
            if let Ok(n) = buf.parse::<usize>() {
                col_num = n;
            }
            state.mode = Mode::TableRemoveColumn { object_index, col_num, buf, cursor };
            return Action::Redraw;
        }
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
            if cursor > 0 {
                let byte_idx = char_to_byte_idx(&buf, cursor - 1);
                buf.remove(byte_idx);
                cursor -= 1;
                col_num = buf.parse().unwrap_or(1);
            }
            state.mode = Mode::TableRemoveColumn { object_index, col_num, buf, cursor };
            return Action::Redraw;
        }
        KeyCode::Up | KeyCode::Down => {
            let ncols = match &state.source.objects[object_index] {
                SceneObject::Table(t) => t.col_widths.len(),
                _ => 1,
            };
            if key.code == KeyCode::Up {
                col_num = if col_num <= 1 { ncols } else { col_num - 1 };
            } else {
                col_num = if col_num >= ncols { 1 } else { col_num + 1 };
            }
            buf = col_num.to_string();
            cursor = buf.len();
            state.mode = Mode::TableRemoveColumn { object_index, col_num, buf, cursor };
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

// ---------------------------------------------------------------------------
// Table: cell props — selecting sub-state
// ---------------------------------------------------------------------------

fn handle_table_cell_selecting(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (object_index, mut cursor_row, mut cursor_col, mut selected_cells) = match &state.mode {
        Mode::TableEditCellProps { object_index, cursor_row, cursor_col, selected_cells, .. } => {
            (*object_index, *cursor_row, *cursor_col, selected_cells.clone())
        }
        _ => return Action::Continue,
    };

    let (nrows, ncols) = match &state.source.objects[object_index] {
        SceneObject::Table(t) => (t.rows.min(crate::engine::objects::table::MAX_ROWS), t.col_widths.len()),
        _ => return Action::Continue,
    };

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::EditProperties {
            object_index, selected_property: 0, editing_value: None,
            cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
        };
        return Action::Redraw;
    }

    // Arrow navigation
    match key.code {
        KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
            cursor_row = if cursor_row == 0 { nrows.saturating_sub(1) } else { cursor_row - 1 };
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::BlinkSelection;
        }
        KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            cursor_row = (cursor_row + 1) % nrows.max(1);
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::BlinkSelection;
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            cursor_col = if cursor_col == 0 { ncols.saturating_sub(1) } else { cursor_col - 1 };
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::BlinkSelection;
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            cursor_col = (cursor_col + 1) % ncols.max(1);
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::BlinkSelection;
        }
        KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE => {
            // Toggle selection of cursor cell
            let cell = (cursor_row, cursor_col);
            if let Some(pos) = selected_cells.iter().position(|&c| c == cell) {
                selected_cells.remove(pos);
            } else {
                selected_cells.push(cell);
            }
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::Redraw;
        }
        _ => {}
    }

    // Table-specific: add list ("l") → edit content with "- " prefix
    if matches_binding(&bindings.table_add_list, &key) {
        let existing = table_cell_content(state, object_index, cursor_row, cursor_col);
        let buf = if existing.is_empty() {
            "- ".to_string()
        } else {
            format!("{}\n- ", existing)
        };
        let cursor = buf.chars().count();
        state.mode = Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingContent { row: cursor_row, col: cursor_col, buf, cursor },
        };
        return Action::Redraw;
    }

    // Table-specific: edit cell style ("s")
    if matches_binding(&bindings.table_edit_cell_style, &key) {
        state.mode = Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingStyle {
                selected_prop: 0,
                editing_value: None,
                cursor: 0,
                dropdown: None,
            },
        };
        return Action::Redraw;
    }

    // Enter → edit content of cursor cell
    if matches_binding(&bindings.confirm, &key) {
        let existing = table_cell_content(state, object_index, cursor_row, cursor_col);
        let cursor_pos = existing.chars().count();
        state.mode = Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingContent {
                row: cursor_row, col: cursor_col,
                buf: existing, cursor: cursor_pos,
            },
        };
        return Action::Redraw;
    }

    Action::Continue
}

fn table_cell_content(state: &EditorState, object_index: usize, row: usize, col: usize) -> String {
    match &state.source.objects[object_index] {
        SceneObject::Table(t) => t.cells.get(row)
            .and_then(|r| r.get(col))
            .map(|c| c.content.clone())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Table: cell props — content editing sub-state
// ---------------------------------------------------------------------------

fn handle_table_cell_edit_content(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (object_index, cursor_row, cursor_col, selected_cells, edit_row, edit_col, buf, cursor) =
        match &state.mode {
            Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row, col, buf, cursor },
            } => (*object_index, *cursor_row, *cursor_col, selected_cells.clone(), *row, *col, buf.clone(), *cursor),
            _ => return Action::Continue,
        };

    // The shared insert-before model (the underline caret the panel draws marks
    // the gap the next keystroke lands in): Shift-Enter / the newline binding
    // insert a line break, Enter saves, Esc cancels.
    let newline = matches_binding(&bindings.insert_newline, &key);
    let mut te = TextEdit::new(buf, cursor);
    match te.handle_key(&key, newline) {
        TextAction::Commit => {
            if let SceneObject::Table(t) = &mut state.source.objects[object_index] {
                t.normalize_cells();
                if let Some(row_vec) = t.cells.get_mut(edit_row) {
                    if let Some(cell) = row_vec.get_mut(edit_col) {
                        cell.content = te.buf;
                    }
                }
                state.dirty = true;
            }
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            Action::Redraw
        }
        TextAction::Cancel => {
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            Action::Redraw
        }
        TextAction::Edited => {
            let (buf, cursor) = (te.buf, te.cursor);
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row: edit_row, col: edit_col, buf, cursor },
            };
            Action::Redraw
        }
        TextAction::Ignored => Action::Continue,
    }
}

// ---------------------------------------------------------------------------
// Table: cell style editing (3 sub-handlers mirroring EditProperties logic)
// ---------------------------------------------------------------------------

/// Get the current style of the cell(s) being edited.
/// If multiple cells are selected, returns a merged style (first selected cell's style).
fn cell_style_for_editing(
    state: &EditorState,
    object_index: usize,
    selected_cells: &[(usize, usize)],
    cursor_row: usize,
    cursor_col: usize,
) -> Style {
    let target = if selected_cells.is_empty() {
        (cursor_row, cursor_col)
    } else {
        selected_cells[0]
    };
    match &state.source.objects[object_index] {
        SceneObject::Table(t) => t.cells.get(target.0)
            .and_then(|r| r.get(target.1))
            .and_then(|c| c.style.as_ref())
            .cloned()
            .unwrap_or_default(),
        _ => Style::default(),
    }
}

fn cell_style_prop_value(style: &Style, prop: &str) -> String {
    use crate::editor::properties::format_opt_color_pub;
    match prop {
        "fg_color" => format_opt_color_pub(&style.fg),
        "bg_color" => format_opt_color_pub(&style.bg),
        "bold"     => style.bold.to_string(),
        "dimmed"   => style.dim.to_string(),
        _ => String::new(),
    }
}

/// Build the `EditingStyle` sub-mode of `TableEditCellProps`.
fn cell_style_mode(
    object_index: usize,
    cursor_row: usize,
    cursor_col: usize,
    selected_cells: Vec<(usize, usize)>,
    selected_prop: usize,
    editing_value: Option<String>,
    cursor: usize,
    dropdown: Option<usize>,
) -> Mode {
    Mode::TableEditCellProps {
        object_index,
        cursor_row,
        cursor_col,
        selected_cells,
        sub_state: TableCellSubState::EditingStyle { selected_prop, editing_value, cursor, dropdown },
    }
}

/// Back to plain cell selection.
fn cell_selecting_mode(
    object_index: usize,
    cursor_row: usize,
    cursor_col: usize,
    selected_cells: Vec<(usize, usize)>,
) -> Mode {
    Mode::TableEditCellProps {
        object_index,
        cursor_row,
        cursor_col,
        selected_cells,
        sub_state: TableCellSubState::Selecting,
    }
}

fn handle_table_cell_style_props(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (object_index, cursor_row, cursor_col, selected_cells, selected_prop) = match &state.mode {
        Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingStyle { selected_prop, .. },
        } => (*object_index, *cursor_row, *cursor_col, selected_cells.clone(), *selected_prop),
        _ => return Action::Continue,
    };

    let prop_count = properties::CELL_STYLE_PROPS.len();
    let prop_name = properties::CELL_STYLE_PROPS[selected_prop];
    // `bold`/`dimmed` are booleans (toggled); `fg_color`/`bg_color` open a dropdown.
    let is_bool = prop_name == "bold" || prop_name == "dimmed";

    if matches_binding(&bindings.cancel, &key) {
        state.mode = cell_selecting_mode(object_index, cursor_row, cursor_col, selected_cells);
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_up, &key) {
        let sp = if selected_prop == 0 { prop_count - 1 } else { selected_prop - 1 };
        state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, sp, None, 0, None);
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key) || (key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE) {
        let sp = (selected_prop + 1) % prop_count;
        state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, sp, None, 0, None);
        return Action::Redraw;
    }

    // Booleans flip on Space or Enter — same affordance as object properties.
    let toggle = matches_binding(&bindings.confirm, &key)
        || (key.code == KeyCode::Char(' ') && key.modifiers == KeyModifiers::NONE);
    if is_bool && toggle {
        apply_cell_style(state, object_index, selected_cells.clone(), cursor_row, cursor_col, prop_name, None);
        state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, None);
        return Action::Redraw;
    }

    // Colours open the shared dropdown on Enter.
    if matches_binding(&bindings.confirm, &key) {
        let style = cell_style_for_editing(state, object_index, &selected_cells, cursor_row, cursor_col);
        let current_val = cell_style_prop_value(&style, prop_name);
        let dd_sel = properties::COLOR_OPTIONS.iter().position(|&o| o == current_val).unwrap_or(0);
        state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, Some(dd_sel));
        return Action::Redraw;
    }

    Action::Continue
}

fn handle_table_cell_style_dropdown(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    let (object_index, cursor_row, cursor_col, selected_cells, selected_prop, dd_sel) = match &state.mode {
        Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingStyle { selected_prop, dropdown: Some(dd), .. },
        } => (*object_index, *cursor_row, *cursor_col, selected_cells.clone(), *selected_prop, *dd),
        _ => return Action::Continue,
    };

    let opts = properties::COLOR_OPTIONS;
    let prop_name = properties::CELL_STYLE_PROPS[selected_prop];
    let sentinel = properties::dropdown_custom_sentinel(&properties::PropertyKind::Color);

    match dropdown_key(&key, &bindings, dd_sel, opts.len()) {
        DropdownKey::Ignored => Action::Continue,
        DropdownKey::Cancel => {
            state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, None);
            Action::Redraw
        }
        DropdownKey::Move(n) => {
            state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, Some(n));
            Action::Redraw
        }
        DropdownKey::Choose(n) => {
            let chosen = opts[n];
            if chosen == sentinel {
                // Switch to text entry for a custom hex colour.
                let style = cell_style_for_editing(state, object_index, &selected_cells, cursor_row, cursor_col);
                let cur_val = cell_style_prop_value(&style, prop_name);
                let cur = cur_val.chars().count();
                state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, Some(cur_val), cur, None);
            } else {
                apply_cell_style(state, object_index, selected_cells.clone(), cursor_row, cursor_col, prop_name, Some(chosen));
                state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, None);
            }
            Action::Redraw
        }
    }
}

fn handle_table_cell_style_edit_value(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, cursor_row, cursor_col, selected_cells, selected_prop, ev, cursor) = match &state.mode {
        Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingStyle { selected_prop, editing_value: Some(ev), cursor, .. },
        } => (*object_index, *cursor_row, *cursor_col, selected_cells.clone(), *selected_prop, ev.clone(), *cursor),
        _ => return Action::Continue,
    };

    let prop_name = properties::CELL_STYLE_PROPS[selected_prop];
    let newline = matches_binding(&state.config.key_bindings.insert_newline, &key);
    let mut te = TextEdit::new(ev, cursor);

    match te.handle_key(&key, newline) {
        TextAction::Ignored => Action::Continue,
        TextAction::Cancel => {
            state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, None);
            Action::Redraw
        }
        TextAction::Commit => {
            apply_cell_style(state, object_index, selected_cells.clone(), cursor_row, cursor_col, prop_name, Some(&te.buf));
            state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, None, 0, None);
            Action::Redraw
        }
        TextAction::Edited => {
            state.mode = cell_style_mode(object_index, cursor_row, cursor_col, selected_cells, selected_prop, Some(te.buf), te.cursor, None);
            Action::Redraw
        }
    }
}

/// Apply a style property to all selected cells (or the cursor cell if none selected).
fn apply_cell_style(
    state: &mut EditorState,
    object_index: usize,
    selected_cells: Vec<(usize, usize)>,
    cursor_row: usize,
    cursor_col: usize,
    prop: &str,
    value: Option<&str>,
) {
    // A bold/dimmed toggle flips the value the panel shows (the first target's)
    // and writes that one result to every target, so a mixed selection becomes
    // uniform instead of each cell inverting.
    let shown = cell_style_for_editing(state, object_index, &selected_cells, cursor_row, cursor_col);
    let targets: Vec<(usize, usize)> = if selected_cells.is_empty() {
        vec![(cursor_row, cursor_col)]
    } else {
        selected_cells
    };

    if let SceneObject::Table(t) = &mut state.source.objects[object_index] {
        t.normalize_cells();
        for (row, col) in targets {
            if let Some(cell_row) = t.cells.get_mut(row) {
                if let Some(cell) = cell_row.get_mut(col) {
                    let st = cell.style.get_or_insert_with(Style::default);
                    match prop {
                        "fg_color" => {
                            use crate::editor::properties::parse_opt_color_pub;
                            if let Ok(c) = parse_opt_color_pub(value.unwrap_or("none")) {
                                st.fg = c;
                            }
                        }
                        "bg_color" => {
                            use crate::editor::properties::parse_opt_color_pub;
                            if let Ok(c) = parse_opt_color_pub(value.unwrap_or("none")) {
                                st.bg = c;
                            }
                        }
                        "bold" => {
                            st.bold = !shown.bold;
                        }
                        "dimmed" => {
                            st.dim = !shown.dim;
                        }
                        _ => {}
                    }
                    // If style is now default, remove it
                    if st.fg.is_none() && st.bg.is_none() && !st.bold && !st.dim {
                        cell.style = None;
                    }
                }
            }
        }
        state.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(id: AnimId, start: usize, end: usize, auto_play: bool) -> SceneObject {
        use crate::engine::objects::Animation;
        use crate::engine::source::FrameRange;
        SceneObject::Animation(Animation {
            id,
            frames: FrameRange { start, end },
            auto_play,
            delay_ms: 500,
            gap_frames: 0,
        })
    }

    #[test]
    fn frame_inserts_are_refused_at_the_deck_limit() {
        use crate::engine::source::MAX_FRAME_COUNT;
        let mut state =
            EditorState::open(&crate::test_tmp("bs_at_limit_frames_absent_4.json")).unwrap();
        state.source.width = 10;
        state.source.height = 10;
        state.source.frame_count = MAX_FRAME_COUNT;
        let keys = state.config.key_bindings.clone();
        for binding in [&keys.frame_add, &keys.frame_copy] {
            state.mode = Mode::FrameMenu;
            let c = binding.chars().next().unwrap();
            handle_frame_menu(&mut state, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            assert_eq!(state.source.frame_count, MAX_FRAME_COUNT, "{binding} refused");
            assert!(state.status_message.as_deref().unwrap().contains("exceeds the limit"));
        }
        state.mode = Mode::FrameRangePlace { frames: vec![0, 1], copy: true };
        handle_frame_range_place(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(state.source.frame_count, MAX_FRAME_COUNT, "block copy refused");
        assert!(state.status_message.as_deref().unwrap().contains("exceeds the limit"));
        assert!(!state.dirty);
    }

    #[test]
    fn frame_ops_are_refused_on_an_over_limit_deck() {
        // A hand-edited deck past MAX_FRAME_COUNT: the frame menu does not open,
        // and a huge selection is refused at once instead of collecting frames.
        let mut state =
            EditorState::open(&crate::test_tmp("bs_over_limit_frames_absent_3.json")).unwrap();
        state.source.frame_count = 100_000_000_000;
        state.mode = Mode::Normal;
        let f = state.config.key_bindings.frame_menu.chars().next().unwrap();
        handle_key(&mut state, KeyEvent::new(KeyCode::Char(f), KeyModifiers::NONE));
        assert!(matches!(state.mode, Mode::Normal), "frame menu refused");
        assert!(state.status_message.as_deref().unwrap().contains("over the limit"));

        state.mode = Mode::FrameSelectInput { buf: "1-4000000000".into(), cursor: 12 };
        handle_frame_select_input(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(state.mode, Mode::FrameSelectInput { .. }), "nothing selected");
        assert!(state.status_message.as_deref().unwrap().contains("over the limit"));
    }

    #[test]
    fn shift_skip_cluster_spans_a_single_animation() {
        // Inside the span → its (start, end); outside → None. End is exclusive,
        // so the start frame counts and the end frame does not.
        let objs = vec![anim(1, 2, 5, true)];
        assert_eq!(animation_cluster(&objs, 2), Some((2, 5)));
        assert_eq!(animation_cluster(&objs, 4), Some((2, 5)));
        assert_eq!(animation_cluster(&objs, 1), None);
        assert_eq!(animation_cluster(&objs, 5), None);
    }

    #[test]
    fn shift_skip_cluster_merges_overlapping_animations() {
        // Two overlapping spans merge to their union from any covered frame, so a
        // Shift+←/→ jumps clear of the whole overlapping group.
        let objs = vec![anim(1, 0, 6, true), anim(2, 3, 8, true)];
        assert_eq!(animation_cluster(&objs, 1), Some((0, 8)));
        assert_eq!(animation_cluster(&objs, 7), Some((0, 8)));
    }

    #[test]
    fn shift_skip_cluster_keeps_disjoint_animations_separate() {
        // Touching-but-disjoint spans (end == next start) stay separate.
        let objs = vec![anim(1, 1, 3, true), anim(2, 3, 5, true)];
        assert_eq!(animation_cluster(&objs, 2), Some((1, 3)));
        assert_eq!(animation_cluster(&objs, 3), Some((3, 5)));
    }

    #[test]
    fn shift_arrow_through_handle_key_jumps_across_an_animation() {
        // End-to-end through the real dispatch: a Shift+Right on a frame inside an
        // animation span must land on the first frame past the span, and Shift+Left
        // on the first frame before it.
        use crate::editor::object_defaults::create_default;
        use crate::editor::state::scene_object_frame_range_mut;

        let mut state =
            EditorState::open(&crate::test_tmp("bs_shift_arrow_regression_does_not_exist_77.json")).unwrap();
        state.source.frame_count = 12;
        let mut label = create_default(0, 0);
        if let Some(fr) = scene_object_frame_range_mut(&mut label) {
            fr.start = 0;
            fr.end = 12;
        }
        state.source.objects = vec![label, anim(1, 3, 7, true)];
        state.mode = Mode::Normal;

        let shift_right = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);
        let shift_left = KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT);

        // Inside the span (frame 4) → Shift+Right lands past the span (frame 7).
        state.current_frame = 4;
        handle_key(&mut state, shift_right);
        assert_eq!(state.current_frame, 7, "Shift+Right should skip to past the animation");

        // Inside the span (frame 4) → Shift+Left lands just before it (frame 2).
        state.current_frame = 4;
        handle_key(&mut state, shift_left);
        assert_eq!(state.current_frame, 2, "Shift+Left should skip to before the animation");

        // Off the span (frame 11) → Shift+Left falls back to the ±10 coarse scrub.
        state.current_frame = 11;
        handle_key(&mut state, shift_left);
        assert_eq!(state.current_frame, 1, "off-animation Shift+Left keeps the ±10 jump");

        // The `[` / `]` aliases do the same jump on terminals that swallow
        // Shift+arrow — `]` skips past the span, `[` skips before it.
        let bracket_right = KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE);
        let bracket_left = KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE);
        state.current_frame = 4;
        handle_key(&mut state, bracket_right);
        assert_eq!(state.current_frame, 7, "] should skip to past the animation");
        state.current_frame = 4;
        handle_key(&mut state, bracket_left);
        assert_eq!(state.current_frame, 2, "[ should skip to before the animation");
    }

    #[test]
    fn shift_skip_cluster_includes_non_auto_play_animations() {
        // Unlike the player, the editor jump considers every animation, since
        // there is no auto-advance distinction while authoring.
        let objs = vec![anim(1, 2, 5, false)];
        assert_eq!(animation_cluster(&objs, 3), Some((2, 5)));
    }

    #[test]
    fn animate_refuses_an_out_of_range_end_without_inserting() {
        // A huge `end` with add frames on used to insert that many frames one by
        // one; it must be refused up front, leaving the deck and the menu
        // untouched. (Just past the limit, so a regression fails fast.)
        use crate::editor::object_defaults::create_default;

        let mut state = EditorState::open(&crate::test_tmp("bs_anim_huge_end_absent_9.json")).unwrap();
        state.source.frame_count = 10;
        state.current_frame = 0;
        state.source.objects = vec![create_default(0, 0)]; // Label
        let end = crate::engine::source::MAX_FRAME_COUNT;
        state.mode = anim_mode(0, 0, "x", 0, None, 0, 0, 9, 0, 0, false, 0, end, true, true, 500, 0);
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(state.source.frame_count, 10, "no frames inserted");
        assert_eq!(state.source.objects.len(), 1, "no animation created");
        assert!(matches!(state.mode, Mode::AnimateProperty { .. }), "the menu stays open");
        assert!(state.status_message.as_deref().unwrap().starts_with("Error"));

        // Without add frames, a span past the deck is refused too; one inside is fine.
        assert!(check_anim_span(&state.source, 0, 0, 10, false).is_err());
        assert!(check_anim_span(&state.source, 0, 0, 9, false).is_ok());
        assert!(check_anim_span(&state.source, 0, 0, 9, true).is_ok());

        // Converge's [s] runs the same check.
        state.mode = converge_mode(vec![0], 0, None, 0, 20, 10, 0, end, true, true, 500, 0);
        state.status_message = None;
        handle_converge_config(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!((state.source.frame_count, state.source.objects.len()), (10, 1));
        assert!(matches!(state.mode, Mode::ConvergeConfig { .. }), "the converge menu stays open");
        assert!(state.status_message.as_deref().unwrap().starts_with("Error"));
    }

    /// A deck whose only object is a table, editing cell (0,0) holding `text`
    /// with the caret at `cursor`.
    fn table_cell_editing(text: &str, cursor: usize) -> EditorState {
        use crate::editor::object_defaults::{create_default, OBJECT_TYPES};
        let mut state = EditorState::open(&crate::test_tmp("bs_table_cell_edit_absent.json")).unwrap();
        let ti = OBJECT_TYPES.iter().position(|t| t.name == "Table").unwrap();
        let mut table = create_default(ti, 0);
        if let SceneObject::Table(t) = &mut table {
            t.normalize_cells();
            t.cells[0][0].content = text.into();
        }
        state.source.objects = vec![table];
        state.mode = Mode::TableEditCellProps {
            object_index: 0, cursor_row: 0, cursor_col: 0, selected_cells: Vec::new(),
            sub_state: TableCellSubState::EditingContent { row: 0, col: 0, buf: text.into(), cursor },
        };
        state
    }

    fn cell_buf(state: &EditorState) -> (String, usize) {
        match &state.mode {
            Mode::TableEditCellProps { sub_state: TableCellSubState::EditingContent { buf, cursor, .. }, .. } => {
                (buf.clone(), *cursor)
            }
            _ => panic!("not editing a cell"),
        }
    }

    #[test]
    fn table_cell_editing_inserts_before_the_caret() {
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        // Left then type: lands before the shown caret ('c'), not after it.
        let mut state = table_cell_editing("abc", 3);
        handle_table_cell_edit_content(&mut state, key(KeyCode::Left));
        handle_table_cell_edit_content(&mut state, key(KeyCode::Char('X')));
        assert_eq!(cell_buf(&state), ("abXc".into(), 3));
        // Backspace deletes the char before the caret.
        handle_table_cell_edit_content(&mut state, key(KeyCode::Backspace));
        assert_eq!(cell_buf(&state), ("abc".into(), 2));
        handle_table_cell_edit_content(&mut state, key(KeyCode::Backspace));
        assert_eq!(cell_buf(&state), ("ac".into(), 1));
        // At the start of the cell, typing prepends.
        handle_table_cell_edit_content(&mut state, key(KeyCode::Home));
        handle_table_cell_edit_content(&mut state, key(KeyCode::Char('1')));
        assert_eq!(cell_buf(&state), ("1ac".into(), 1));
        // Enter saves the text into the cell.
        handle_table_cell_edit_content(&mut state, key(KeyCode::Enter));
        let SceneObject::Table(t) = &state.source.objects[0] else { panic!() };
        assert_eq!(t.cells[0][0].content, "1ac");
    }

    #[test]
    fn animate_add_frames_after_a_morph_animates_the_same_object() {
        // The label sits after a morph ending on frame 3. Animating it with add
        // frames inserts the morph's continuation right after the morph, so the
        // label moves down one: the animation, the menu and a copied-from index
        // must all follow it, not land on the continuation.
        let mut state = EditorState::open(&crate::test_tmp("bs_anim_after_morph_absent.json")).unwrap();
        state.source = serde_json::from_str(
            r#"{ "width": 12, "height": 4, "frame_count": 4, "objects": [
                { "type": "morph", "from": "oooo", "to": "XXXX", "mode": "wipe-right",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 4 } },
                { "type": "label", "text": "L",
                  "position": { "x": { "fixed": 8 }, "y": { "fixed": 3 } },
                  "frames": { "start": 3, "end": 4 } }
            ] }"#,
        )
        .unwrap();
        state.current_frame = 3;
        state.clipboard_sources = vec![1];
        state.mode = anim_mode(1, 0, "x", 0, None, 0, 8, 2, 0, 0, false, 3, 6, true, true, 500, 0);
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let SceneObject::Label(l) = &state.source.objects[2] else { panic!("label not at 2") };
        assert!(matches!(l.position.x, Coordinate::Animated { from: 8, to: 2, .. }), "the label is animated");
        let SceneObject::Morph(c) = &state.source.objects[1] else { panic!("continuation not at 1") };
        assert!(matches!(c.position.x, Coordinate::Fixed(_)), "the continuation is not");
        assert!(matches!(state.mode, Mode::EditProperties { object_index: 2, .. }), "the menu follows the label");
        assert_eq!(state.clipboard_sources, vec![2], "a copied-from index follows the label");
    }

    #[test]
    fn converge_add_frames_on_a_morphs_last_frame_keeps_its_blend() {
        // A ball → square wipe ends on frame 3; converging a label there with
        // add frames must not re-time the wipe: its slides render as before and
        // the new frames hold the finished square.
        let texts = |p: &SourcePresentation| -> Vec<String> {
            let contract = crate::types::TerminalContract { width: p.width, height: p.height };
            let played = crate::renderer::Renderer::render(&crate::engine::Engine::compile(p), contract);
            (0..p.frame_count)
                .map(|f| played.grid_at(f).iter().map(|r| r.iter().map(|c| c.ch).collect::<String>() + "\n").collect())
                .collect()
        };
        let mut state = EditorState::open(&crate::test_tmp("bs_converge_morph_absent.json")).unwrap();
        state.source = serde_json::from_str(
            r#"{ "width": 12, "height": 4, "frame_count": 4, "objects": [
                { "type": "morph", "from": "oooo\noooo", "to": "XXXX\nXXXX", "mode": "wipe-right",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 4 } },
                { "type": "label", "text": "L",
                  "position": { "x": { "fixed": 8 }, "y": { "fixed": 3 } },
                  "frames": { "start": 3, "end": 4 } }
            ] }"#,
        )
        .unwrap();
        let before = texts(&state.source);
        state.current_frame = 3;
        state.mode = converge_mode(vec![1], 0, None, 0, 8, 3, 3, 6, true, true, 500, 0);
        handle_converge_config(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(state.source.frame_count, 7);
        let after = texts(&state.source);
        assert_eq!(after[..4], before[..]);
        for f in 4..7 {
            assert_eq!(after[f], before[3], "frame {f} holds the finished morph");
        }
    }

    #[test]
    fn converge_add_frames_keeps_an_earlier_animations_span() {
        // X moves x 0 → 30 over frames 0..3 (animation 1). From frame 3, converge
        // Y with add frames: animation 1 keeps its span, so X's motion on frames
        // 0..3 is unchanged and X holds 30 on the inserted frames.
        let mut state = EditorState::open(&crate::test_tmp("bs_converge_earlier_absent.json")).unwrap();
        state.source = serde_json::from_str(
            r#"{ "width": 80, "height": 24, "frame_count": 4, "objects": [
                { "type": "label", "text": "X",
                  "position": { "x": { "animated": { "from": 0, "to": 30, "anim": 1 } },
                                "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 4 } },
                { "type": "animation", "id": 1, "frames": { "start": 0, "end": 4 } },
                { "type": "label", "text": "Y",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 5 } },
                  "frames": { "start": 3, "end": 4 } }
            ] }"#,
        )
        .unwrap();
        state.current_frame = 3;
        state.mode = converge_mode(vec![2], 0, None, 0, 40, 5, 3, 6, true, true, 500, 0);
        handle_converge_config(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(state.source.frame_count, 7);
        let SceneObject::Animation(a) = &state.source.objects[1] else { panic!("animation moved") };
        assert_eq!((a.frames.start, a.frames.end), (0, 4), "earlier animation span unchanged");
        let anims = AnimSpans::of(&state.source);
        let SceneObject::Label(x) = &state.source.objects[0] else { panic!() };
        let xs: Vec<u16> = (0..7).map(|f| x.position.x.evaluate(f, &anims)).collect();
        assert_eq!(xs, vec![0, 10, 20, 30, 30, 30, 30]);
    }

    #[test]
    fn add_frames_refuses_a_span_that_does_not_start_on_the_current_frame() {
        // Frames are inserted after the current frame, so a span starting
        // elsewhere would smear the element over old slides and leave blank
        // ones behind: refused up front, deck and menu untouched.
        use crate::editor::object_defaults::create_default;

        let mut state = EditorState::open(&crate::test_tmp("bs_anim_start_absent.json")).unwrap();
        state.source.frame_count = 7;
        state.current_frame = 5;
        state.source.objects = vec![create_default(0, 5)]; // a Label on frame 5
        let before = state.source.clone();
        state.mode = anim_mode(0, 0, "x", 0, None, 0, 0, 9, 0, 0, false, 3, 6, true, true, 500, 0);
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(serde_json::to_string(&state.source).unwrap(), serde_json::to_string(&before).unwrap());
        assert!(matches!(state.mode, Mode::AnimateProperty { .. }), "the menu stays open");
        assert!(state.status_message.as_deref().unwrap().contains("current frame"));

        // Without add frames the same span animates over the existing slides.
        assert!(check_anim_span(&state.source, 5, 3, 6, false).is_ok());
        assert!(check_anim_span(&state.source, 5, 5, 6, true).is_ok());

        // Converge's add-frames path is held to the same rule.
        state.mode = converge_mode(vec![0], 0, None, 0, 20, 10, 3, 6, true, true, 500, 0);
        state.status_message = None;
        handle_converge_config(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(state.source.frame_count, 7);
        assert!(matches!(state.mode, Mode::ConvergeConfig { .. }), "the converge menu stays open");
    }

    #[test]
    fn single_object_delete_prunes_an_animation_it_alone_drove() {
        // [d] on the only object an animation drives must take the animation too.
        use crate::editor::object_defaults::create_default;

        let mut state = EditorState::open(&crate::test_tmp("bs_delete_driver_absent_4.json")).unwrap();
        state.source.frame_count = 5;
        let mut o = create_default(0, 0); // Label
        if let SceneObject::Label(l) = &mut o {
            l.position.x = Coordinate::Animated { from: 0, to: 9, anim: 1 };
        }
        state.source.objects = vec![o, anim(1, 0, 5, true)];
        state.mode = Mode::Confirm {
            message: String::new(),
            selected: 0,
            action: ConfirmAction::DeleteObject { object_index: 0 },
            return_mode: Box::new(Mode::Normal),
        };
        handle_confirm(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.source.objects.is_empty(), "left: {:?}", state.source.objects);
    }

    /// A deck with a Label "Hi" at (10, 5) whose x animates 10→40 over frames
    /// 2..=10 with gap 3 — shown on frames 2, 6, 10 (original + two copies) —
    /// and the index of the copy on frame 6.
    fn strobed_label() -> (EditorState, usize) {
        use crate::editor::object_defaults::create_default;
        let mut state = EditorState::open(&crate::test_tmp("bs_strobe_set_absent.json")).unwrap();
        state.source.frame_count = 15;
        let mut o = create_default(0, 2); // a Label on frame 2
        if let SceneObject::Label(l) = &mut o {
            l.text = "Hi".into();
            l.position.x = Coordinate::Fixed(10.0);
            l.position.y = Coordinate::Fixed(5.0);
        }
        state.source.objects = vec![o];
        state.current_frame = 2;
        apply_animation(&mut state, 0, "x", 10, 40, 0, 0, false, 2, 10, false, true, 500, 3);
        state.current_frame = 6;
        let copy = labels_on(&state, 6)[0];
        assert_ne!(copy, 0, "frame 6 shows a copy");
        (state, copy)
    }

    /// Indices of the labels visible on `frame`.
    fn labels_on(state: &EditorState, frame: usize) -> Vec<usize> {
        (0..state.source.objects.len())
            .filter(|&i| matches!(state.source.objects[i], SceneObject::Label(_)))
            .filter(|&i| state.source.effective_frame_range(i).contains(frame))
            .collect()
    }

    /// Edit the Animation's last frame (1-based) like the props panel does.
    fn set_anim_last_frame(state: &mut EditorState, last: &str) {
        let a = state.source.objects.iter().position(|o| matches!(o, SceneObject::Animation(_))).unwrap();
        let _ = apply_property(state, a, "last_frame", last);
    }

    #[test]
    fn editing_a_strobe_copy_edits_the_whole_set() {
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        // Move the copy on frame 6 down one row, then widen the span to frame 15:
        // every sample frame shows the element exactly once, at the new y.
        let (mut state, copy) = strobed_label();
        state.mode = Mode::SelectedObject { object_index: copy };
        handle_key(&mut state, key(KeyCode::Down));
        set_anim_last_frame(&mut state, "15");
        for f in [2, 6, 10, 14] {
            let on = labels_on(&state, f);
            assert_eq!(on.len(), 1, "frame {f}: {on:?}");
            let y = properties::get_coord(&state.source.objects[on[0]], "y").unwrap();
            assert!(matches!(y, Coordinate::Fixed(v) if v == 6.0), "frame {f}: {y:?}");
        }

        // Retyping the copy's text likewise reaches every sample.
        let (mut state, copy) = strobed_label();
        let text = properties::get_properties(&state.source.objects, copy)
            .iter()
            .position(|p| p.name == "text")
            .unwrap();
        state.mode = ep_editing(copy, text, "Yo".into(), 2, 0, 0);
        handle_key(&mut state, key(KeyCode::Enter));
        set_anim_last_frame(&mut state, "15");
        for f in [2, 6, 10, 14] {
            let on = labels_on(&state, f);
            assert_eq!(on.len(), 1, "frame {f}: {on:?}");
            assert!(matches!(&state.source.objects[on[0]], SceneObject::Label(l) if l.text == "Yo"));
        }
    }

    #[test]
    fn a_span_edit_beside_a_strobe_copy_does_not_overwrite_other_objects() {
        // Bulk-editing [Animation, copy]'s last_frame re-strobes (removing and
        // re-adding copies, shifting a later object into the copy's slot); the
        // in-place sync must not then clone that object over the set.
        use crate::editor::object_defaults::create_default;
        let (mut state, copy) = strobed_label();
        let mut x = create_default(0, 0); // an unrelated Label "X"
        if let SceneObject::Label(l) = &mut x {
            l.text = "X".into();
        }
        state.source.objects.push(x);
        let anim = state.source.objects.iter().position(|o| matches!(o, SceneObject::Animation(_))).unwrap();
        let members = vec![anim, copy];
        let last = properties::common_properties(&state.source.objects, &members)
            .iter()
            .position(|p| p.name == "last_frame")
            .unwrap();
        state.mode = emp_editing(members, last, "11".into(), 2, 0, 0);
        handle_key(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let texts: Vec<&str> = state.source.objects.iter().filter_map(|o| match o {
            SceneObject::Label(l) => Some(l.text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(texts.iter().filter(|t| **t == "X").count(), 1, "{texts:?}");
        assert_eq!(texts.iter().filter(|t| **t == "Hi").count(), 3, "{texts:?}");
    }

    #[test]
    fn moving_or_resizing_a_group_edits_its_members_whole_strobe_set() {
        // A group holding the strobed label's original: moving (or resizing) the
        // group must reach every copy too, so a later span edit shows the
        // element exactly once per sample frame.
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        for (mode_of, code) in [
            (|g| Mode::SelectedObject { object_index: g }, KeyCode::Down),
            (|g| Mode::ResizeObject { object_index: g }, KeyCode::Right),
        ] as [(fn(usize) -> Mode, KeyCode); 2]
        {
            let (mut state, _) = strobed_label();
            state.current_frame = 2;
            // Give the whole set a width, so a group resize has something to scale.
            for o in &mut state.source.objects {
                if let SceneObject::Label(l) = o {
                    l.width = Coordinate::Fixed(10.0);
                }
            }
            state.source.objects.push(SceneObject::Group(Group { members: vec![0], frames: None, z_order: 0 }));
            let g = state.source.objects.len() - 1;
            state.mode = mode_of(g);
            handle_key(&mut state, key(code));
            set_anim_last_frame(&mut state, "15");
            let mut seen = Vec::new();
            for f in [2, 6, 10, 14] {
                let on = labels_on(&state, f);
                assert_eq!(on.len(), 1, "{code:?}, frame {f}: {on:?}");
                seen.push(super::super::state::gap_clone_key(&state.source.objects[on[0]]));
            }
            assert!(seen.windows(2).all(|w| w[0] == w[1]), "{code:?}: every sample is the same element");
            let y = properties::get_coord(&state.source.objects[labels_on(&state, 14)[0]], "y").unwrap();
            if code == KeyCode::Down {
                assert!(matches!(y, Coordinate::Fixed(v) if v == 6.0), "moved: {y:?}");
            }
        }
    }

    #[test]
    fn a_linked_edit_of_a_strobed_element_reaches_its_whole_set() {
        // The strobed label's original is linked to a pasted (static) clone. A
        // text edit propagates to the clone; the strobe copies must follow too,
        // so a later span edit shows one label per sample frame.
        let (mut state, _) = strobed_label();
        let mut clone = state.source.objects[0].clone();
        super::super::state::flatten_coordinates(&mut clone, 2, &AnimSpans::of(&state.source));
        if let Some(fr) = super::super::state::scene_object_frame_range_mut(&mut clone) {
            *fr = crate::engine::source::FrameRange { start: 12, end: 13 };
        }
        state.source.objects.push(clone);
        let clone = state.source.objects.len() - 1;
        state.source.links = vec![vec![0, clone]];

        let text = properties::get_properties(&state.source.objects, 0)
            .iter()
            .position(|p| p.name == "text")
            .unwrap();
        state.mode = ep_editing(0, text, "Bye".into(), 3, 0, 0);
        handle_key(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let texts: Vec<&str> = state.source.objects.iter().filter_map(|o| match o {
            SceneObject::Label(l) => Some(l.text.as_str()),
            _ => None,
        }).collect();
        assert!(texts.iter().all(|t| *t == "Bye"), "{texts:?}");

        set_anim_last_frame(&mut state, "15");
        for f in [2, 6, 10, 14] {
            assert_eq!(labels_on(&state, f).len(), 1, "frame {f}");
        }
    }

    #[test]
    fn editing_a_strobe_copy_reaches_the_originals_linked_clone() {
        // As above, but the edit is made on a later sample frame, where only a
        // copy (in no link family) is selectable: the original's linked clone
        // must get it too.
        let (mut state, copy) = strobed_label();
        let mut clone = state.source.objects[0].clone();
        super::super::state::flatten_coordinates(&mut clone, 2, &AnimSpans::of(&state.source));
        if let Some(fr) = super::super::state::scene_object_frame_range_mut(&mut clone) {
            *fr = crate::engine::source::FrameRange { start: 12, end: 13 };
        }
        state.source.objects.push(clone);
        let clone = state.source.objects.len() - 1;
        state.source.links = vec![vec![0, clone]];

        let text = properties::get_properties(&state.source.objects, copy)
            .iter()
            .position(|p| p.name == "text")
            .unwrap();
        state.mode = ep_editing(copy, text, "Bye".into(), 3, 0, 0);
        handle_key(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(&state.source.objects[clone], SceneObject::Label(l) if l.text == "Bye"));
        let texts: Vec<&str> = state.source.objects.iter().filter_map(|o| match o {
            SceneObject::Label(l) => Some(l.text.as_str()),
            _ => None,
        }).collect();
        assert!(texts.iter().all(|t| *t == "Bye"), "{texts:?}");
        assert!(state.status_message.as_deref().unwrap().contains("+1 linked"));
    }

    #[test]
    fn add_frames_hold_survives_later_edits_of_the_earlier_animation() {
        // X animates over frames 1-5; Y is then animated with add frames from
        // X's last frame. Without a gap X holds its end on the three new frames;
        // a strobe (gap 1) shows on its samples only. Re-applying X's animation
        // and editing its span longer or shorter must keep exactly one X on
        // every frame it shows on — no vanished hold, no stale duplicate.
        use crate::editor::object_defaults::create_default;
        for gap in [0, 1] {
            let mut state = EditorState::open(&crate::test_tmp("bs_hold_absent.json")).unwrap();
            state.source.frame_count = 5;
            let label = |t: &str, start, end| {
                let mut o = create_default(0, start);
                if let SceneObject::Label(l) = &mut o {
                    l.text = t.into();
                    l.frames = crate::engine::source::FrameRange { start, end };
                }
                o
            };
            state.source.objects = vec![label("X", 0, 5), label("Y", 4, 5)];
            state.current_frame = 0;
            apply_animation(&mut state, 0, "x", 0, 30, 0, 0, false, 0, 4, false, true, 500, gap);
            state.current_frame = 4;
            apply_animation(&mut state, 1, "x", 0, 20, 0, 0, false, 4, 7, true, true, 500, 0);
            assert_eq!(state.source.frame_count, 8);
            let shown = |state: &EditorState| -> Vec<usize> {
                (0..8).map(|f| labels_on(state, f)
                    .into_iter()
                    .filter(|&i| matches!(&state.source.objects[i], SceneObject::Label(l) if l.text == "X"))
                    .count()).collect()
            };
            // Frames X shows on with its span ending at exclusive `end`.
            let expect = |end: usize| -> Vec<usize> {
                (0..8).map(|f| usize::from(if gap == 0 { true } else { f < end && f % 2 == 0 })).collect()
            };
            assert_eq!(shown(&state), expect(5), "gap {gap}: after add frames");

            // Re-apply X's animation with a new `to`: the hold shows the new end.
            apply_animation(&mut state, 0, "x", 0, 35, 0, 0, false, 0, 4, false, true, 500, gap);
            assert_eq!(shown(&state), expect(5), "gap {gap}: after re-applying X");
            if gap == 0 {
                let x = properties::get_coord(&state.source.objects[0], "x").unwrap();
                assert_eq!(x.evaluate(6, &AnimSpans::of(&state.source)), 35);
            }

            // Edit X's span longer (onto the held frames), then shorter.
            let a = state.source.objects.iter().position(|o| matches!(o, SceneObject::Animation(a) if a.frames.start == 0)).unwrap();
            for (last, end) in [("7", 7), ("3", 3)] {
                let _ = apply_property(&mut state, a, "last_frame", last);
                assert_eq!(shown(&state), expect(end), "gap {gap}: span to {last}");
            }
        }
    }

    #[test]
    fn a_split_element_keeps_its_pieces_through_a_re_apply() {
        // E on frames 4..10, driven by x (no gap) and y (gap 2). Copying block
        // 2-5 after frame 8 splits E around the copy: head 4..9, linked
        // continuation 13..14, with the copied part of E on 11..13. Re-applying
        // the gapped animation must not take the continuation for a strobe
        // copy, nor re-cover the copied block with the head; a longer span then
        // extends only the continuation's outer edge.
        use crate::editor::object_defaults::create_default;
        for gap in [2, 0] {
            let mut state = EditorState::open(&crate::test_tmp("bs_split_strobe_absent.json")).unwrap();
            state.source.frame_count = 12;
            let mut o = create_default(0, 4);
            if let SceneObject::Label(l) = &mut o {
                l.frames = crate::engine::source::FrameRange { start: 4, end: 10 };
            }
            state.source.objects = vec![o];
            state.current_frame = 4;
            apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 4, 9, false, true, 500, 0);
            apply_animation(&mut state, 0, "y", 0, 9, 0, 0, false, 4, 9, false, true, 500, gap);
            super::super::state::copy_frames(&mut state.source, 2, 5, 8, false);
            let shown = |s: &EditorState| (0..16).map(|f| labels_on(s, f).len()).collect::<Vec<_>>();
            let after_copy = shown(&state);
            assert_eq!(after_copy, vec![0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0], "gap {gap}");

            apply_animation(&mut state, 0, "y", 0, 9, 0, 0, false, 4, 13, false, true, 500, gap);
            assert_eq!(shown(&state), after_copy, "gap {gap}: after re-applying y");

            let a = state.source.objects.iter().position(|o| matches!(o, SceneObject::Animation(a) if a.frames.start == 4)).unwrap();
            let _ = apply_property(&mut state, a, "last_frame", "15");
            let mut longer = after_copy.clone();
            longer[14] = 1;
            assert_eq!(shown(&state), longer, "gap {gap}: after a longer span");
        }
    }

    #[test]
    fn converging_a_linked_paste_locks_both_to_the_span() {
        // A linked stamp is not a split piece: converging the source (0..3) and
        // its stamp (5..6, placed one cell off) gives both the whole span 2..8.
        use crate::editor::object_defaults::create_default;
        let mut state = EditorState::open(&crate::test_tmp("bs_converge_linked_absent.json")).unwrap();
        state.source.frame_count = 10;
        let at = |x: f64, start, end| {
            let mut o = create_default(0, start);
            if let SceneObject::Label(l) = &mut o {
                l.position.x = Coordinate::Fixed(x);
                l.frames = crate::engine::source::FrameRange { start, end };
            }
            o
        };
        state.source.objects = vec![at(0.0, 0, 3), at(1.0, 5, 6)];
        state.source.links = vec![vec![0, 1]];
        apply_converge(&mut state, &[0, 1], 20, 10, 2, 8, false, true, 500, 0);
        for i in [0, 1] {
            let fr = super::super::state::scene_object_frame_range(&state.source.objects[i]).unwrap();
            assert_eq!((fr.start, fr.end), (2, 9), "object {i}");
        }
    }

    #[test]
    fn a_gap_strobe_is_refused_for_a_member_of_an_explicit_group() {
        // An explicit group's range overrides each member's own range, which a
        // strobe narrows to its samples: animating, converging with a gap, or
        // making a strobed member's group explicit is refused, deck unchanged.
        use crate::editor::object_defaults::create_default;
        let key_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        let mut state = EditorState::open(&crate::test_tmp("bs_strobe_group_absent.json")).unwrap();
        state.source.frame_count = 10;
        let mut l = create_default(0, 0);
        if let SceneObject::Label(l) = &mut l {
            l.frames = crate::engine::source::FrameRange { start: 0, end: 10 };
        }
        let group = |frames| SceneObject::Group(Group { members: vec![0], frames, z_order: 0 });
        state.source.objects = vec![l, group(Some(crate::engine::source::FrameRange { start: 0, end: 10 }))];
        let before = serde_json::to_string(&state.source).unwrap();

        state.mode = anim_mode(0, 0, "x", 0, None, 0, 0, 40, 0, 0, false, 1, 9, false, true, 500, 3);
        handle_animate_property(&mut state, key_s);
        assert_eq!(serde_json::to_string(&state.source).unwrap(), before, "animate refused");
        assert!(matches!(state.mode, Mode::AnimateProperty { .. }));
        assert!(state.status_message.as_deref().unwrap().starts_with("Error"));

        state.mode = converge_mode(vec![0], 0, None, 0, 20, 10, 1, 9, false, true, 500, 3);
        handle_converge_config(&mut state, key_s);
        assert_eq!(serde_json::to_string(&state.source).unwrap(), before, "converge refused");

        // Without a gap the same animation applies.
        state.mode = anim_mode(0, 0, "x", 0, None, 0, 0, 40, 0, 0, false, 1, 9, false, true, 500, 0);
        handle_animate_property(&mut state, key_s);
        assert!(state.source.objects.iter().any(|o| matches!(o, SceneObject::Animation(_))));

        // A strobed member in an auto group: its range can't be made explicit.
        let mut state = EditorState::open(&crate::test_tmp("bs_strobe_group_auto_absent.json")).unwrap();
        state.source.frame_count = 10;
        let mut l = create_default(0, 1);
        if let SceneObject::Label(l) = &mut l {
            l.frames = crate::engine::source::FrameRange { start: 1, end: 2 };
        }
        state.source.objects = vec![l];
        state.current_frame = 1;
        apply_animation(&mut state, 0, "x", 0, 40, 0, 0, false, 1, 9, false, true, 500, 3);
        state.source.objects.push(group(None));
        let g = state.source.objects.len() - 1;
        let _ = apply_property(&mut state, g, "first_frame", "1");
        assert!(matches!(&state.source.objects[g], SceneObject::Group(g) if g.frames.is_none()));
        assert!(state.status_message.as_deref().unwrap().starts_with("Error"));
        // One level deep, like the override: an outer group around that group
        // may take a range (it gates the inner group, not the strobed label).
        state.source.objects.push(SceneObject::Group(Group { members: vec![g], frames: None, z_order: 0 }));
        let outer = state.source.objects.len() - 1;
        let _ = apply_property(&mut state, outer, "last_frame", "5");
        assert!(matches!(&state.source.objects[outer], SceneObject::Group(g) if g.frames.is_some()), "{:?}", state.status_message);

        // A hand-edited deck with the strobe already inside an explicit group:
        // a span edit of its gapped animation is refused too.
        let mut state = EditorState::open(&crate::test_tmp("bs_strobe_group_span_absent.json")).unwrap();
        state.source = serde_json::from_str(r#"{ "width": 80, "height": 24, "frame_count": 10, "objects": [
            { "type": "label", "text": "L",
              "position": { "x": { "animated": { "from": 0, "to": 9, "anim": 1 } }, "y": { "fixed": 0 } },
              "frames": { "start": 1, "end": 2 } },
            { "type": "animation", "id": 1, "frames": { "start": 1, "end": 10 }, "gap_frames": 3 },
            { "type": "group", "members": [0], "frames": { "start": 0, "end": 10 } }
        ] }"#).unwrap();
        let before = serde_json::to_string(&state.source).unwrap();
        let _ = apply_property(&mut state, 1, "last_frame", "8");
        assert_eq!(serde_json::to_string(&state.source).unwrap(), before, "span edit refused");
        assert!(state.status_message.as_deref().unwrap().starts_with("Error"));
    }

    #[test]
    fn re_animating_from_a_strobe_copy_makes_one_set() {
        // [s] from the copy on frame 6 with a new `to`: one set (3 labels) with
        // the new motion, not a second set beside the old one.
        let (mut state, copy) = strobed_label();
        state.mode = anim_mode(copy, 0, "x", 0, None, 0, 10, 60, 0, 0, false, 2, 10, false, true, 500, 3);
        handle_key(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let labels: Vec<usize> = (0..state.source.objects.len())
            .filter(|&i| matches!(state.source.objects[i], SceneObject::Label(_)))
            .collect();
        assert_eq!(labels.len(), 3, "one strobe set");
        for i in labels {
            let x = properties::get_coord(&state.source.objects[i], "x").unwrap();
            assert!(matches!(x, Coordinate::Animated { to: 60, .. }), "{x:?}");
        }
    }

    #[test]
    fn animate_two_axis_layout_exposes_x_and_y_fields() {
        // A two-axis (position) session shows x from/to AND y from/to, then the
        // span/toggles/delay/gap — 10 fields.
        let rows = anim_field_rows(true, 1, 9, 2, 8, 0, 4, true, true, 500, 1);
        let labels: Vec<&str> = rows.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            labels,
            vec!["x from", "x to", "y from", "y to", "start", "end", "add frames", "auto play", "delay ms", "gap frames"]
        );
        // Values are taken from the right axis.
        assert_eq!(rows[0].1, "1"); // x from
        assert_eq!(rows[1].1, "9"); // x to
        assert_eq!(rows[2].1, "2"); // y from
        assert_eq!(rows[3].1, "8"); // y to
        assert_eq!(rows[6].1, "[x]"); // add frames checkbox
    }

    #[test]
    fn animate_single_axis_layout_has_one_from_to_pair() {
        // A 1-D coordinate (e.g. width) shows just from/to — 8 fields.
        let rows = anim_field_rows(false, 3, 7, 0, 0, 0, 4, true, true, 500, 1);
        let labels: Vec<&str> = rows.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            labels,
            vec!["from", "to", "start", "end", "add frames", "auto play", "delay ms", "gap frames"]
        );
    }

    #[test]
    fn gap_strobes_even_without_add_frames() {
        use crate::editor::object_defaults::create_default;
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::SceneObject;

        // A 10-frame deck with a label already on every frame (no insertion).
        let mut state = EditorState::open(&crate::test_tmp("bs_gap_regression_does_not_exist_42.json")).unwrap();
        state.source.frame_count = 10;
        let mut label = create_default(0, 0);
        if let Some(fr) = scene_object_frame_range_mut(&mut label) {
            fr.start = 0;
            fr.end = 10;
        }
        state.source.objects = vec![label];
        state.current_frame = 0;

        // Animate x 0→9 over the span with add_frames OFF and gap 3 (3 empty
        // frames between appearances → shows on 0, 4, 8).
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 3);

        // The label is strobed into samples (original + clones on frames 4 and 8),
        // not left spanning every frame — so three labels, not one.
        let labels = state.source.objects.iter().filter(|o| matches!(o, SceneObject::Label(_))).count();
        assert_eq!(labels, 3, "gap must strobe the element even without add_frames");
        // The strobed original holds only its first sample frame.
        match scene_object_frame_range_mut(&mut state.source.objects[0]) {
            Some(fr) => assert_eq!((fr.start, fr.end), (0, 1)),
            None => panic!(),
        }
    }

    #[test]
    fn re_applying_a_gapped_animation_does_not_stack_orphan_copies() {
        use crate::editor::object_defaults::create_default;
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::SceneObject;

        let mut state = EditorState::open(&crate::test_tmp("bs_gap_reapply_absent_77.json")).unwrap();
        state.source.frame_count = 10;
        let mut label = create_default(0, 0);
        if let Some(fr) = scene_object_frame_range_mut(&mut label) {
            fr.start = 0;
            fr.end = 10;
        }
        state.source.objects = vec![label];

        let count_labels = |s: &EditorState| {
            s.source.objects.iter().filter(|o| matches!(o, SceneObject::Label(_))).count()
        };

        // First apply: gap 3 → 3 strobe samples (frames 0, 4, 8).
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 3);
        assert_eq!(count_labels(&state), 3);

        // Re-applying the same animation must not leave orphan copies behind —
        // the old strobe copies are cleared first, so the count stays 3.
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 3);
        assert_eq!(count_labels(&state), 3, "re-apply must not stack orphan copies");

        // Re-applying with gap 0 (off) clears the strobe entirely: one label
        // spanning the whole span again, no orphans.
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 0);
        assert_eq!(count_labels(&state), 1, "gap 0 should remove the strobe copies");
    }

    #[test]
    fn select_action_submenu_offers_copy_converge_delete_and_edit_props() {
        // The post-multi-select action sub-menu lists Copy, Converge, Delete,
        // then Edit Props (bulk-edit the shared properties).
        assert_eq!(
            select_action_labels(),
            vec!["Copy", "Converge", "Delete", "Edit Props"]
        );
    }

    #[test]
    fn converge_field_rows_omits_the_per_object_from_fields() {
        // Converge only edits the shared target + span/toggles — no x/y "from",
        // since each object's from is its own current position.
        let rows = converge_field_rows(20, 10, 0, 9, false, true, 500, 0);
        let labels: Vec<&str> = rows.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            labels,
            vec!["x to", "y to", "start", "end", "add frames", "auto play", "delay ms", "gap frames"]
        );
        assert_eq!(rows[0].1, "20"); // shared x target
        assert_eq!(rows[1].1, "10"); // shared y target
        assert_eq!(rows[4].1, "[ ]"); // add frames off by default
    }

    #[test]
    fn converge_animates_each_object_from_its_own_spot_to_the_shared_point() {
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::{get_coord, set_coordinate};
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::{AnimId, Coordinate, SceneObject};

        let mut state = EditorState::open(&crate::test_tmp("bs_converge_absent_91.json")).unwrap();
        state.source.frame_count = 10;
        state.current_frame = 0;

        // Two labels at distinct spots, both spanning the whole deck.
        let mut a = create_default(0, 0);
        let mut b = create_default(0, 0);
        for (obj, x, y) in [(&mut a, 2.0, 3.0), (&mut b, 40.0, 18.0)] {
            set_coordinate(obj, "x", Coordinate::Fixed(x)).unwrap();
            set_coordinate(obj, "y", Coordinate::Fixed(y)).unwrap();
            if let Some(fr) = scene_object_frame_range_mut(obj) {
                fr.start = 0;
                fr.end = 10;
            }
        }
        state.source.objects = vec![a, b];

        // Converge both onto (20, 10) over frames 0..9, no inserted frames.
        apply_converge(&mut state, &[0, 1], 20, 10, 0, 9, false, true, 500, 0);

        // Each object keeps its own `from` but shares the `to` target, and both
        // its axes reference the *same* animation (returned for cross-check).
        let check = |idx: usize, fx: u16, fy: u16| -> AnimId {
            match (get_coord(&state.source.objects[idx], "x"), get_coord(&state.source.objects[idx], "y")) {
                (
                    Some(Coordinate::Animated { from: xf, to: xt, anim: xa }),
                    Some(Coordinate::Animated { from: yf, to: yt, anim: ya }),
                ) => {
                    assert_eq!((xf, xt), (fx, 20), "object {idx} x");
                    assert_eq!((yf, yt), (fy, 10), "object {idx} y");
                    assert_eq!(xa, ya, "object {idx} x and y share one animation");
                    xa
                }
                other => panic!("object {idx} not animated on both axes: {other:?}"),
            }
        };
        let id0 = check(0, 2, 3);
        let id1 = check(1, 40, 18);
        assert_eq!(id0, id1, "all members share one converge animation");

        // Exactly one shared Animation sidecar, with that id, covers the span.
        let anims: Vec<_> = state.source.objects.iter()
            .filter_map(|o| match o { SceneObject::Animation(an) => Some((an.id, an.frames.start, an.frames.end)), _ => None })
            .collect();
        assert_eq!(anims, vec![(id0, 0, 10)], "one shared animation over the span");
    }

    /// Two labels A (x 0) and B (x 40) on frames 0..12, converged onto x 20 over
    /// frames 0..=4 with `gap`; returns the state.
    fn converged_pair(gap: usize, name: &str) -> EditorState {
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::set_coordinate;
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::Coordinate;

        let mut state = EditorState::open(&crate::test_tmp(name)).unwrap();
        state.source.frame_count = 12;
        state.current_frame = 0;
        let label = |x: f64| {
            let mut o = create_default(0, 0);
            set_coordinate(&mut o, "x", Coordinate::Fixed(x)).unwrap();
            set_coordinate(&mut o, "y", Coordinate::Fixed(5.0)).unwrap();
            if let Some(fr) = scene_object_frame_range_mut(&mut o) {
                fr.start = 0;
                fr.end = 12;
            }
            o
        };
        state.source.objects = vec![label(0.0), label(40.0)];
        apply_converge(&mut state, &[0, 1], 20, 5, 0, 4, false, true, 500, gap);
        state
    }

    /// Per frame `0..12`: the x of every visible label, sorted.
    fn visible_label_xs(state: &EditorState) -> Vec<Vec<u16>> {
        use crate::editor::properties::get_coord;
        use crate::editor::state::scene_object_frame_range;
        use crate::engine::source::SceneObject;
        let anims = AnimSpans::of(&state.source);
        (0..12)
            .map(|f| {
                let mut xs: Vec<u16> = state.source.objects.iter()
                    .filter(|o| matches!(o, SceneObject::Label(_)))
                    .filter(|o| scene_object_frame_range(o).is_some_and(|r| r.start <= f && f < r.end))
                    .map(|o| get_coord(o, "x").unwrap().evaluate(f, &anims) as u16)
                    .collect();
                xs.sort_unstable();
                xs
            })
            .collect()
    }

    #[test]
    fn re_timing_a_converge_from_one_member_re_locks_every_member() {
        let mut state = converged_pair(0, "bs_retime_converge_absent_31.json");
        // Re-animate A's x (shared animation) with end 9 instead of 4.
        apply_animation(&mut state, 0, "x", 0, 20, 5, 5, true, 0, 9, false, true, 500, 0);
        let xs = visible_label_xs(&state);
        for (f, v) in xs.iter().enumerate().take(10) {
            assert_eq!(v.len(), 2, "both members visible on frame {f}: {v:?}");
        }
        assert_eq!(xs[9], vec![20, 20], "both reach the target on frame 9");
        assert!(xs[10].is_empty() && xs[11].is_empty(), "neither past the span");
    }

    #[test]
    fn re_strobing_a_converge_from_one_member_leaves_no_stale_copies() {
        // Gap 1 over 0..=4 (samples 0, 2, 4), then re-animate A with gap 2 over
        // 0..=9 (samples 0, 3, 6, 9): one sample per member on exactly those.
        let mut state = converged_pair(1, "bs_restrobe_converge_absent_32.json");
        apply_animation(&mut state, 0, "x", 0, 20, 5, 5, true, 0, 9, false, true, 500, 2);
        let xs = visible_label_xs(&state);
        for (f, v) in xs.iter().enumerate() {
            let want = if [0, 3, 6, 9].contains(&f) { 2 } else { 0 };
            assert_eq!(v.len(), want, "frame {f}: {v:?}");
        }
        assert_eq!(xs[9], vec![20, 20]);
        // And back to no gap: every old copy of both members goes.
        apply_animation(&mut state, 0, "x", 0, 20, 5, 5, true, 0, 9, false, true, 500, 0);
        let xs = visible_label_xs(&state);
        for (f, v) in xs.iter().enumerate().take(10) {
            assert_eq!(v.len(), 2, "frame {f}: {v:?}");
        }
    }

    #[test]
    fn converge_remaps_a_member_above_another_members_gap_copies() {
        // A converged alone with gap 2 leaves strobe copies of A above it; B is
        // appended after them. Re-converging A and B clears A's copies first,
        // which shifts B down — B must still be the object that converges.
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::{get_coord, set_coordinate};
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::Coordinate;

        let mut state = EditorState::open(&crate::test_tmp("bs_converge_gap_absent_17.json")).unwrap();
        state.source.frame_count = 10;
        state.current_frame = 0;
        let label = |x: f64, y: f64| {
            let mut o = create_default(0, 0);
            set_coordinate(&mut o, "x", Coordinate::Fixed(x)).unwrap();
            set_coordinate(&mut o, "y", Coordinate::Fixed(y)).unwrap();
            if let Some(fr) = scene_object_frame_range_mut(&mut o) {
                fr.start = 0;
                fr.end = 10;
            }
            o
        };
        state.source.objects = vec![label(2.0, 3.0)];
        apply_converge(&mut state, &[0], 20, 10, 0, 9, false, true, 500, 2);
        state.source.objects.push(label(40.0, 18.0));
        let b = state.source.objects.len() - 1;

        apply_converge(&mut state, &[0, b], 20, 10, 0, 9, false, true, 500, 0);

        let froms: Vec<(u16, u16, AnimId)> = state.source.objects.iter()
            .filter_map(|o| match (get_coord(o, "x"), get_coord(o, "y")) {
                (Some(Coordinate::Animated { from: fx, anim, .. }), Some(Coordinate::Animated { from: fy, .. })) =>
                    Some((fx, fy, anim)),
                _ => None,
            })
            .collect();
        assert_eq!(froms.len(), 2, "exactly A and B are animated, no copies left: {froms:?}");
        assert_eq!((froms[0].0, froms[0].1), (2, 3), "A converges from its spot");
        assert_eq!((froms[1].0, froms[1].1), (40, 18), "B converges from its spot");
        assert_eq!(froms[0].2, froms[1].2, "one shared animation");
    }

    #[test]
    fn bulk_setting_x_skips_an_animated_member_and_leaves_no_orphan() {
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::{get_coord, set_coordinate};
        use crate::engine::source::Coordinate;

        let mut state = EditorState::open(&crate::test_tmp("bs_bulk_anim_absent_23.json")).unwrap();
        state.source.frame_count = 10;
        let mut a = create_default(0, 0);
        set_coordinate(&mut a, "x", Coordinate::Animated { from: 2, to: 40, anim: 1 }).unwrap();
        let b = create_default(0, 0);
        state.source.objects = vec![a, anim(1, 0, 10, true), b];

        let _ = apply_multi_property(&mut state, &[0, 2], "x", "10");

        assert!(matches!(get_coord(&state.source.objects[0], "x"),
            Some(Coordinate::Animated { from: 2, to: 40, anim: 1 })), "A keeps its motion");
        assert!(matches!(state.source.objects[1], SceneObject::Animation(_)), "animation still driven, indices unchanged");
        assert!(matches!(get_coord(&state.source.objects[2], "x"), Some(Coordinate::Fixed(v)) if v == 10.0));
        assert_eq!(state.status_message.as_deref(), Some("Set x = 10 on 1/2 objects"));
    }

    /// A label at the end of the deck whose x (and, with `y_anim`, y) are driven by
    /// animations stored *before* it, opened in the animate menu.
    fn animate_menu_on_last_label(y_anim: Option<AnimId>) -> EditorState {
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::set_coordinate;
        use crate::engine::source::Coordinate;

        let mut state = EditorState::open(&crate::test_tmp("bs_anim_menu_absent_29.json")).unwrap();
        state.source.frame_count = 10;
        let mut l = create_default(0, 0);
        set_coordinate(&mut l, "x", Coordinate::Animated { from: 2, to: 40, anim: 1 }).unwrap();
        if let Some(id) = y_anim {
            set_coordinate(&mut l, "y", Coordinate::Animated { from: 3, to: 9, anim: id }).unwrap();
        }
        state.source.objects = vec![anim(1, 0, 10, true), anim(2, 0, 10, true), l];
        state.mode = enter_animate(&state, 2, 0, "x");
        state
    }

    fn mode_object_index(state: &EditorState) -> usize {
        match state.mode {
            Mode::EditProperties { object_index, .. } => object_index,
            ref m => panic!("expected EditProperties, got {m:?}"),
        }
    }

    #[test]
    fn animate_menu_revert_keeps_the_index_on_the_label_after_pruning_below_it() {
        // [x] orphans animation 1 (and 2 was never used); both sit below the
        // label, so it moves from 2 to 0 and the menu must follow it.
        let mut state = animate_menu_on_last_label(None);
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(state.source.objects.len(), 1);
        assert_eq!(mode_object_index(&state), 0);
        assert!(matches!(state.source.objects[0], SceneObject::Label(_)));
    }

    #[test]
    fn animate_menu_apply_keeps_the_index_on_the_label_after_pruning_below_it() {
        // [s] unifies x and y onto one animation, orphaning the other (stored
        // below the label), so the label moves from 2 to 1.
        let mut state = animate_menu_on_last_label(Some(2));
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(state.source.objects.len(), 2);
        assert_eq!(mode_object_index(&state), 1);
        assert!(matches!(state.source.objects[1], SceneObject::Label(_)));
    }

    #[test]
    fn toggling_back_to_independent_keeps_the_linked_stamps_links() {
        let mut state = EditorState::open(&crate::test_tmp("bs_link_toggle_absent.json")).unwrap();
        state.source.objects = vec![crate::editor::object_defaults::create_default(0, 0)];
        copy_to_clipboard(&mut state, &[0]);
        start_paste(&mut state);
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        handle_paste_placing(&mut state, key(KeyCode::Char('l')));
        handle_paste_placing(&mut state, key(KeyCode::Enter)); // A (index 1), linked
        handle_paste_placing(&mut state, key(KeyCode::Char('l')));
        handle_paste_placing(&mut state, key(KeyCode::Enter)); // B (index 2), independent
        handle_paste_placing(&mut state, key(KeyCode::Esc));
        assert_eq!(state.source.objects.len(), 3);
        assert_eq!(state.source.links, vec![vec![0, 1]], "A stays linked, B does not");
    }

    #[test]
    fn linked_paste_after_a_prune_below_the_copied_object_links_that_object() {
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::set_coordinate;
        use crate::engine::source::Coordinate;

        let mut state = EditorState::open(&crate::test_tmp("bs_link_prune_absent_31.json")).unwrap();
        state.source.frame_count = 10;
        let mut a = create_default(0, 0);
        set_coordinate(&mut a, "x", Coordinate::Animated { from: 2, to: 40, anim: 1 }).unwrap();
        let mut b = create_default(0, 0);
        if let SceneObject::Label(l) = &mut b {
            l.text = "B".into();
        }
        state.source.objects = vec![anim(1, 0, 10, true), a, b];

        // Copy B (index 2), then [x]-revert A's animation: the orphaned
        // Animation at index 0 is pruned, so B moves to index 1.
        copy_to_clipboard(&mut state, &[2]);
        state.mode = enter_animate(&state, 1, 0, "x");
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(state.clipboard_sources, vec![1]);

        // Linked paste: stamp once, then finish.
        start_paste(&mut state);
        handle_paste_placing(&mut state, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        handle_paste_placing(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        handle_paste_placing(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(state.source.links, vec![vec![1, 2]], "B links to its own copy");
        assert!(matches!(&state.source.objects[1], SceneObject::Label(l) if l.text == "B"));
    }

    /// A fresh 10-frame editor deck holding `objects`.
    fn deck_state(objects: Vec<SceneObject>) -> EditorState {
        let mut state = EditorState::open(&crate::test_tmp("bs_clip_src_absent_37.json")).unwrap();
        state.source.frame_count = 10;
        state.current_frame = 0;
        state.source.objects = objects;
        state
    }

    /// A label spanning the whole 10-frame deck, its x driven by `x_anim` if set.
    fn deck_label(x_anim: Option<AnimId>) -> SceneObject {
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::set_coordinate;
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::Coordinate;
        let mut l = create_default(0, 0);
        if let Some(fr) = scene_object_frame_range_mut(&mut l) {
            fr.start = 0;
            fr.end = 10;
        }
        if let Some(id) = x_anim {
            set_coordinate(&mut l, "x", Coordinate::Animated { from: 2, to: 40, anim: id }).unwrap();
        }
        l
    }

    #[test]
    fn note_removed_objects_shifts_sources_and_forgets_a_removed_one() {
        let mut state = deck_state(vec![]);
        state.clipboard_sources = vec![1, 4];
        state.note_removed_objects(&[3, 0]);
        assert_eq!(state.clipboard_sources, vec![0, 2]);
        state.note_removed_objects(&[2]);
        assert!(state.clipboard_sources.is_empty(), "a copied original was removed");
    }

    #[test]
    fn apply_animation_keeps_clipboard_sources_valid() {
        use crate::editor::properties::set_coordinate;
        use crate::engine::source::Coordinate;
        // Prune: unifying x/y orphans an animation stored below the copied B.
        let mut l = deck_label(Some(1));
        set_coordinate(&mut l, "y", Coordinate::Animated { from: 3, to: 9, anim: 2 }).unwrap();
        let mut state = deck_state(vec![anim(1, 0, 10, true), anim(2, 0, 10, true), l, deck_label(None)]);
        copy_to_clipboard(&mut state, &[3]);
        apply_animation(&mut state, 2, "x", 2, 40, 3, 9, true, 0, 9, false, true, 500, 0);
        assert_eq!(state.clipboard_sources, vec![2]);

        // Gap clear: re-applying removes the strobe copy that was copied; the
        // source is anchored on the set's original (R5-26), which stays.
        let mut state = deck_state(vec![deck_label(None)]);
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 3);
        let clone = state.source.objects.len() - 1;
        copy_to_clipboard(&mut state, &[clone]);
        assert_eq!(state.clipboard_sources, vec![0]);
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 3);
        assert_eq!(state.clipboard_sources, vec![0]);
    }

    #[test]
    fn animate_menu_revert_keeps_a_copied_strobe_copys_source() {
        // [x] clears the strobe copies, including the one copied; its source is
        // the set's original (R5-26), which survives and stays the source.
        let mut state = deck_state(vec![deck_label(None)]);
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 9, false, true, 500, 3);
        let clone = state.source.objects.len() - 1;
        copy_to_clipboard(&mut state, &[clone]);
        state.mode = enter_animate(&state, 0, 0, "x");
        handle_animate_property(&mut state, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(state.clipboard_sources, vec![0]);
        assert!(matches!(state.source.objects[0], SceneObject::Label(_)));
    }

    #[test]
    fn converge_keeps_clipboard_sources_valid() {
        // Prune: A's old animation (stored first) is orphaned, so B moves 2 -> 1.
        let mut state = deck_state(vec![anim(1, 0, 10, true), deck_label(Some(1)), deck_label(None)]);
        copy_to_clipboard(&mut state, &[2]);
        apply_converge(&mut state, &[1], 20, 10, 0, 9, false, true, 500, 0);
        assert_eq!(state.clipboard_sources, vec![1]);

        // Gap clear: re-converging removes the strobe copy that was copied; the
        // source is anchored on the set's original (R5-26), which stays.
        let mut state = deck_state(vec![deck_label(None)]);
        apply_converge(&mut state, &[0], 20, 10, 0, 9, false, true, 500, 2);
        let clone = state.source.objects.len() - 1;
        copy_to_clipboard(&mut state, &[clone]);
        apply_converge(&mut state, &[0], 20, 10, 0, 9, false, true, 500, 0);
        assert_eq!(state.clipboard_sources, vec![0]);
    }

    #[test]
    fn replacing_an_auto_advance_marker_keeps_clipboard_sources_valid() {
        // The marker on frame 0 is stored before B; replacing it re-appends it,
        // so B moves 1 -> 0.
        let mut state = deck_state(vec![]);
        super::super::state::set_frame_auto_advance(&mut state.source, 0, 5000);
        state.source.objects.push(deck_label(None));
        copy_to_clipboard(&mut state, &[1]);
        state.mode = Mode::FrameAutoInput { buf: "2".into(), cursor: 1 };
        handle_frame_auto_input(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(state.clipboard_sources, vec![0]);
        assert!(matches!(state.source.objects[0], SceneObject::Label(_)));
    }

    #[test]
    fn animating_a_second_coordinate_does_not_retime_the_first() {
        use crate::editor::properties::get_coord;
        use crate::engine::source::Coordinate;
        let rect: SceneObject = serde_json::from_str(
            r#"{"type":"rect","position":{"x":{"fixed":0},"y":{"fixed":0}},
                "width":10,"height":4,"frames":{"start":0,"end":10}}"#,
        )
        .unwrap();
        let mut state = deck_state(vec![rect]);
        apply_animation(&mut state, 0, "x", 0, 20, 0, 0, true, 0, 2, false, true, 500, 0);
        apply_animation(&mut state, 0, "width", 10, 30, 0, 0, false, 5, 9, false, true, 500, 0);

        let spans: Vec<(AnimId, usize, usize)> = state.source.objects.iter()
            .filter_map(|o| match o { SceneObject::Animation(a) => Some((a.id, a.frames.start, a.frames.end)), _ => None })
            .collect();
        let anim_of = |name| match get_coord(&state.source.objects[0], name) {
            Some(Coordinate::Animated { anim, .. }) => anim,
            other => panic!("{name} not animated: {other:?}"),
        };
        let (x, w) = (anim_of("x"), anim_of("width"));
        assert_ne!(x, w, "width gets its own animation");
        assert!(spans.contains(&(x, 0, 3)), "x keeps its span: {spans:?}");
        assert!(spans.contains(&(w, 5, 10)), "width runs on its own span: {spans:?}");
    }

    #[test]
    fn toggling_bold_on_mixed_table_cells_sets_one_value() {
        let table: SceneObject = serde_json::from_str(
            r#"{"type":"table","position":{"x":{"fixed":0},"y":{"fixed":0}},"width":20,
                "col_widths":[0.34,0.33,0.33],"rows":1,"frames":{"start":0,"end":1},
                "cells":[[{"content":"a"},{"content":"b","style":{"bold":true}},{"content":"c"}]]}"#,
        )
        .unwrap();
        let mut state = deck_state(vec![table]);
        let sel = vec![(0, 0), (0, 1), (0, 2)];
        let bold = |s: &EditorState| match &s.source.objects[0] {
            SceneObject::Table(t) => t.cells[0].iter()
                .map(|c| c.style.as_ref().is_some_and(|st| st.bold)).collect::<Vec<_>>(),
            _ => unreachable!(),
        };
        // The panel shows cell (0,0): not bold, so one press makes all bold...
        apply_cell_style(&mut state, 0, sel.clone(), 0, 0, "bold", None);
        assert_eq!(bold(&state), vec![true, true, true]);
        // ...and the next makes all plain (style dropped back to None).
        apply_cell_style(&mut state, 0, sel, 0, 0, "bold", None);
        assert_eq!(bold(&state), vec![false, false, false]);
        if let SceneObject::Table(t) = &state.source.objects[0] {
            assert!(t.cells[0].iter().all(|c| c.style.is_none()));
        }
    }

    #[test]
    fn a_rejected_frame_value_leaves_an_auto_group_auto() {
        let group = |s: &EditorState| match &s.source.objects[2] {
            SceneObject::Group(g) => g.frames.clone().map(|f| (f.start, f.end)),
            _ => unreachable!(),
        };
        let mut state = deck_state(vec![deck_label(None), deck_label(None),
            serde_json::from_str(r#"{"type":"group","members":[0,1]}"#).unwrap()]);
        for bad in ["x", "-1", "1.5"] {
            let _ = apply_property(&mut state, 2, "first_frame", bad);
            assert!(state.status_message.as_deref().unwrap_or("").starts_with("Error"), "{bad}");
            assert_eq!(group(&state), None, "{bad}: still auto");
            assert!(!state.dirty);
        }
        // A valid value still materialises the explicit range (1-based input).
        let _ = apply_property(&mut state, 2, "first_frame", "3");
        assert_eq!(group(&state), Some((2, 10)));
        assert!(state.dirty);
    }

    #[test]
    fn a_pasted_morph_shows_what_it_showed_where_it_was_copied() {
        // "AAAA" -> "BBBB" wipe-right over frames 0..4: AAAA, BBAA, BBBA, BBBB.
        let morph: SceneObject = serde_json::from_str(
            r#"{"type":"morph","position":{"x":{"fixed":0},"y":{"fixed":0}},"from":"AAAA",
                "to":"BBBB","mode":"wipe-right","frames":{"start":0,"end":4}}"#,
        )
        .unwrap();
        for (copy_frame, want) in [(3, "BBBB"), (1, "BBAA")] {
            let mut state = deck_state(vec![morph.clone()]);
            state.source.width = 6;
            state.source.height = 2;
            state.current_frame = copy_frame;
            copy_to_clipboard(&mut state, &[0]);
            state.current_frame = 5;
            spawn_paste(&mut state);
            let shown = crate::compile::compile(&state.source).unwrap().grid_at(5);
            // The paste is nudged one cell right and down.
            let row: String = shown[1][1..5].iter().map(|c| c.ch).collect();
            assert_eq!(row, want, "copied on frame {copy_frame}");
        }
    }

    #[test]
    fn a_pasted_animated_label_lands_where_it_was_copied() {
        use crate::editor::properties::get_coord;
        use crate::engine::source::Coordinate;
        // x moves 0 -> 40 over frames 0..10.
        let mut label = deck_label(Some(1));
        if let SceneObject::Label(l) = &mut label {
            l.position.x = Coordinate::Animated { from: 0, to: 40, anim: 1 };
        }
        for delete_anim in [false, true] {
            let mut state = deck_state(vec![label.clone(), anim(1, 0, 10, true)]);
            state.source.frame_count = 20;
            state.current_frame = 3;
            let seen = get_coord(&state.source.objects[0], "x").unwrap().evaluate(3, &AnimSpans::of(&state.source));
            assert!(seen > 0 && seen < 40);
            copy_to_clipboard(&mut state, &[0]);
            if delete_anim {
                crate::editor::state::remove_animation(&mut state.source, 1);
            }
            state.current_frame = 15;
            let pasted = spawn_paste(&mut state)[0];
            let x = get_coord(&state.source.objects[pasted], "x");
            // The copy-frame position plus the paste nudge.
            assert!(matches!(x, Some(Coordinate::Fixed(v)) if v == f64::from(seen) + 1.0), "delete_anim={delete_anim}: {x:?}");
        }
    }

    #[test]
    fn copying_a_loop_or_marker_with_a_label_pastes_only_the_label() {
        let lp: SceneObject =
            serde_json::from_str(r#"{"type":"loop","frames":{"start":4,"end":8}}"#).unwrap();
        let aa: SceneObject = serde_json::from_str(
            r#"{"type":"auto_advance","frames":{"start":4,"end":8},"delay_ms":3000}"#,
        )
        .unwrap();
        let mut state = deck_state(vec![deck_label(None), lp, aa]);
        state.current_frame = 4;
        copy_to_clipboard(&mut state, &[0, 1, 2]);
        assert_eq!(state.clipboard.len(), 1, "only the label is copied");
        assert!(state.status_message.as_deref().unwrap().contains("not copied"));
        state.current_frame = 5;
        spawn_paste(&mut state);
        assert!(state.source.validate_loops().is_ok());
        let count = |f: fn(&SceneObject) -> bool| state.source.objects.iter().filter(|o| f(o)).count();
        assert_eq!(count(|o| matches!(o, SceneObject::Loop(_))), 1);
        assert_eq!(count(|o| matches!(o, SceneObject::AutoAdvance(_))), 1);

        // Nothing but span behaviours selected: refused, clipboard untouched.
        let mut state = deck_state(vec![
            serde_json::from_str(r#"{"type":"loop","frames":{"start":4,"end":8}}"#).unwrap(),
        ]);
        copy_to_clipboard(&mut state, &[0]);
        assert!(state.clipboard.is_empty());
        assert!(state.status_message.as_deref().unwrap().contains("can't be copied"));
    }

    #[test]
    fn editing_an_animation_span_updates_one_animation_not_two() {
        // The reported bug: changing an animation's time range left two selectable
        // Animation objects (a new one + the orphaned old one). With the span owned
        // solely by the Animation (referenced by id), re-applying must update the
        // *same* animation in place — never spawn a second.
        use crate::editor::object_defaults::create_default;
        use crate::editor::properties::get_coord;
        use crate::editor::state::scene_object_frame_range_mut;
        use crate::engine::source::{Coordinate, SceneObject};

        let mut state = EditorState::open(&crate::test_tmp("bs_anim_no_dup_absent_3.json")).unwrap();
        state.source.frame_count = 10;
        state.current_frame = 0;
        let mut o = create_default(0, 0); // Label
        if let Some(fr) = scene_object_frame_range_mut(&mut o) {
            fr.start = 0;
            fr.end = 10;
        }
        state.source.objects = vec![o];

        let anim_count = |s: &EditorState| {
            s.source.objects.iter().filter(|o| matches!(o, SceneObject::Animation(_))).count()
        };
        let anim = |s: &EditorState| {
            s.source.objects.iter().find_map(|o| match o {
                SceneObject::Animation(a) => Some(a.clone()),
                _ => None,
            }).unwrap()
        };

        // Animate x 0→9 over frames 0..=4 (single-axis), no inserted frames.
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 4, false, true, 500, 0);
        assert_eq!(anim_count(&state), 1, "one animation created");
        let id = anim(&state).id;
        assert_eq!((anim(&state).frames.start, anim(&state).frames.end), (0, 5));

        // Edit the span end to frame 8 (end_frame 8 → exclusive 9). Re-apply.
        apply_animation(&mut state, 0, "x", 0, 9, 0, 0, false, 0, 8, false, true, 500, 0);
        assert_eq!(anim_count(&state), 1, "editing the span must not spawn a second animation");
        let a = anim(&state);
        assert_eq!(a.id, id, "the same animation is updated in place");
        assert_eq!((a.frames.start, a.frames.end), (0, 9), "its span widened");

        // The coordinate still references that one animation, and the object's
        // visible range tracked the new span.
        match get_coord(&state.source.objects[0], "x") {
            Some(Coordinate::Animated { anim, .. }) => assert_eq!(anim, id),
            other => panic!("x should still be animated: {other:?}"),
        }
        assert_eq!(
            scene_object_frame_range_mut(&mut state.source.objects[0]).map(|fr| (fr.start, fr.end)),
            Some((0, 9)),
            "object range re-locked to the new span",
        );
    }

    #[test]
    fn copy_paste_never_duplicates_an_animation() {
        // A label driven by animation 1, both selected and copied: the paste must
        // leave exactly one Animation with that id (the copy skips it).
        use crate::editor::object_defaults::create_default;
        use crate::engine::source::{Coordinate, SceneObject};

        let mut state = EditorState::open(&crate::test_tmp("bs_copy_anim_absent_5.json")).unwrap();
        state.source.frame_count = 10;
        let mut o = create_default(0, 0); // Label
        if let SceneObject::Label(l) = &mut o {
            l.position.x = Coordinate::Animated { from: 2, to: 40, anim: 1 };
            l.frames.start = 1;
            l.frames.end = 10;
        }
        state.source.objects = vec![o, anim(1, 1, 10, true)];
        let anim_count = |s: &EditorState| {
            s.source.objects.iter().filter(|o| matches!(o, SceneObject::Animation(_))).count()
        };

        copy_to_clipboard(&mut state, &[0, 1]);
        assert_eq!(state.clipboard.len(), 1, "the Animation is left out of the clipboard");
        state.current_frame = 5;
        start_paste(&mut state);
        assert_eq!(anim_count(&state), 1, "still exactly one Animation after paste");

        // Copying only the Animation keeps the clipboard and says why.
        copy_to_clipboard(&mut state, &[1]);
        assert_eq!(state.clipboard.len(), 1);
        assert!(state.status_message.as_deref().unwrap().contains("can't be copied"));
    }

    #[test]
    fn linked_edit_keeps_endpoint_geometry_per_copy() {
        // Arrow endpoints are placement: editing one linked copy's `y2` must not
        // move its sibling's, while an appearance prop still propagates.
        let arrow = |y: u16| -> SceneObject {
            serde_json::from_str(&format!(
                r#"{{"type":"arrow","x1":{{"fixed":2}},"y1":{{"fixed":{y}}},
                    "x2":{{"fixed":20}},"y2":{{"fixed":{y}}},"frames":{{"start":0,"end":1}}}}"#
            ))
            .unwrap()
        };
        let mut state = EditorState::open(&crate::test_tmp("bs_linked_endpoint_absent_8.json")).unwrap();
        state.source.objects = vec![arrow(2), arrow(12)];
        state.source.links = vec![vec![0, 1]];

        let _ = apply_property(&mut state, 0, "y2", "5");
        let _ = apply_property(&mut state, 0, "fg_color", "red");
        let props = properties::get_properties(&state.source.objects, 1);
        let val = |n: &str| props.iter().find(|p| p.name == n).unwrap().value.clone();
        assert_eq!(val("y2"), "12", "sibling endpoint untouched");
        assert_eq!(val("fg_color"), "red", "appearance still propagates");
    }

    #[test]
    fn a_paste_lands_whole_in_a_text_field_and_is_ignored_elsewhere() {
        let mut state = EditorState::open(&crate::test_tmp("bs_paste_absent.json")).unwrap();
        state.source.objects = vec![crate::editor::object_defaults::create_default(0, 0)];
        let paste = |s: &str| Event::Paste(s.to_string());
        // Normal mode: nothing happens (an `F` would otherwise toggle fullscreen).
        assert!(matches!(handle_event(&mut state, paste("Fq\rx")), Action::Continue));
        assert!(matches!(state.mode, Mode::Normal) && !state.fullscreen);
        // The multi-line text overlay keeps every line.
        let text = properties::get_properties(&state.source.objects, 0)
            .iter()
            .position(|p| p.name == "text")
            .unwrap();
        state.mode = ep_editing(0, text, "<>".into(), 1, 0, 0);
        handle_event(&mut state, paste("Intro\r\nFirst point\rSecond point"));
        match &state.mode {
            Mode::EditProperties { editing_value: Some(v), cursor, .. } => {
                assert_eq!(v, "<Intro\nFirst point\nSecond point>");
                assert_eq!(*cursor, 31);
            }
            m => panic!("left the text field: {m:?}"),
        }
        // A single-line field drops the line breaks.
        state.mode = Mode::FrameJump { buf: String::new(), cursor: 0 };
        handle_event(&mut state, paste("1\r\n2\n"));
        assert!(matches!(&state.mode, Mode::FrameJump { buf, cursor: 2 } if buf == "12"));
        // A table column field keeps only digits and acts on what it shows.
        state.mode = Mode::TableAddColumn { object_index: 0, after: true, col_num: 1, buf: String::new(), cursor: 0 };
        handle_event(&mut state, paste("x2"));
        assert!(matches!(&state.mode, Mode::TableAddColumn { col_num: 2, buf, .. } if buf == "2"));
        // Settings keeps its 4-digit cap.
        state.mode = Mode::Settings { selected_field: 0, width_buf: "8".into(), height_buf: "24".into(), cursor: 1 };
        handle_event(&mut state, paste("0a12345"));
        assert!(matches!(&state.mode, Mode::Settings { width_buf, cursor: 4, .. } if width_buf == "8012"));
    }

    #[test]
    fn a_stale_group_member_is_dropped_on_open_and_never_used() {
        let path = crate::test_tmp("bs_stale_member.json");
        std::fs::write(&path, r#"{"width":40,"height":10,"frame_count":1,"objects":[{"type":"label",
            "text":"a","position":{"x":{"fixed":1},"y":{"fixed":1}},"frames":{"start":0,"end":1}},
            {"type":"group","members":[0,5,0]}]}"#).unwrap();
        let mut state = EditorState::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let members = |s: &EditorState| match &s.source.objects[1] {
            SceneObject::Group(g) => g.members.clone(),
            _ => unreachable!(),
        };
        assert_eq!(members(&state), vec![0], "nothing stale to save back");
        assert!(state.status_message.as_deref().unwrap_or("").contains("group members"));
        // Even set after open, a stale member is never indexed: copy and converge.
        if let SceneObject::Group(g) = &mut state.source.objects[1] {
            g.members = vec![0, 5];
        }
        assert_eq!(crate::editor::state::expand_selection(&state.source, &[1]), vec![0, 1]);
        copy_to_clipboard(&mut state, &[1]);
        assert_eq!(state.clipboard.len(), 2);
        let expanded = crate::editor::state::expand_selection(&state.source, &[1]);
        let _ = enter_converge(&state, expanded);
    }

    #[test]
    fn a_stale_link_index_is_dropped_on_open_and_never_used() {
        let path = crate::test_tmp("bs_stale_link.json");
        std::fs::write(&path, r#"{"width":20,"height":4,"frame_count":1,"objects":[{"type":"label",
            "text":"a","position":{"x":{"fixed":0},"y":{"fixed":0}},"frames":{"start":0,"end":1}}],
            "links":[[0,5]]}"#).unwrap();
        let mut state = EditorState::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(state.source.links.is_empty(), "nothing stale to save back");
        assert!(state.status_message.is_some());
        let _ = apply_property(&mut state, 0, "text", "b");
        // Even set after open, a stale entry is never indexed.
        state.source.links = vec![vec![0, 5, 0]];
        assert!(state.source.link_siblings(0).is_empty());
        let _ = apply_property(&mut state, 0, "text", "c");
        // A repeated entry counts once.
        state.source.objects.push(state.source.objects[0].clone());
        state.source.links = vec![vec![0, 1, 1, 0]];
        assert_eq!(state.source.link_siblings(0), vec![1]);
        assert!(state.source.sanitize_links());
        assert_eq!(state.source.links, vec![vec![0, 1]]);
    }

    // --- a gap strobe is re-made as a unit (R5-11) ---------------------------

    /// The frame ranges the labels occupy, sorted (one entry per object).
    fn label_ranges(state: &EditorState) -> Vec<(usize, usize)> {
        let mut r: Vec<_> = state.source.objects.iter().filter_map(|o| match o {
            SceneObject::Label(l) => Some((l.frames.start, l.frames.end)),
            _ => None,
        }).collect();
        r.sort_unstable();
        r
    }

    #[test]
    fn editing_a_gapped_animations_span_restrobes_it() {
        let mut state = deck_state(vec![deck_label(None)]);
        state.source.frame_count = 14;
        // x over frames 1..=9 with 3 empty frames between samples: 1, 5, 9.
        apply_animation(&mut state, 0, "x", 0, 40, 0, 0, false, 1, 9, false, true, 500, 3);
        assert_eq!(label_ranges(&state), vec![(1, 2), (5, 6), (9, 10)]);
        let a = state.source.objects.iter().position(|o| matches!(o, SceneObject::Animation(_))).unwrap();
        let _ = apply_property(&mut state, a, "last_frame", "14");
        assert_eq!(label_ranges(&state), vec![(1, 2), (5, 6), (9, 10), (13, 14)]);
        let SceneObject::Animation(an) = &state.source.objects[a] else { panic!("index kept") };
        let id = an.id;
        super::super::state::remove_animation(&mut state.source, id);
        assert_eq!(label_ranges(&state).len(), 1, "one static label left");
    }

    #[test]
    fn animating_another_coordinate_keeps_a_gap_strobe() {
        let mut state = deck_state(vec![deck_label(None)]);
        state.source.frame_count = 14;
        apply_animation(&mut state, 0, "x", 0, 40, 0, 0, false, 1, 9, false, true, 500, 3);
        // Width on its own animation over 11..=13, no gap.
        apply_animation(&mut state, 0, "width", 10, 20, 0, 0, false, 11, 13, false, true, 500, 0);
        assert_eq!(label_ranges(&state), vec![(1, 2), (5, 6), (9, 10), (11, 14)]);
    }

    #[test]
    fn removing_one_of_two_animations_keeps_the_other_ones_strobe() {
        use crate::editor::properties::get_coord;
        use crate::engine::source::Coordinate;
        let setup = || {
            let mut state = deck_state(vec![deck_label(None)]);
            state.source.frame_count = 16;
            apply_animation(&mut state, 0, "x", 0, 40, 0, 0, false, 1, 9, false, true, 500, 3);
            apply_animation(&mut state, 0, "width", 10, 20, 0, 0, false, 11, 13, false, true, 500, 0);
            let id = |name| match get_coord(&state.source.objects[0], name) {
                Some(Coordinate::Animated { anim, .. }) => anim,
                _ => panic!("{name} animated"),
            };
            let (x, w) = (id("x"), id("width"));
            (state, x, w)
        };
        let anim_index = |state: &EditorState, id| state.source.objects.iter()
            .position(|o| matches!(o, SceneObject::Animation(a) if a.id == id)).unwrap();
        // Width removed: the element stays locked to x's strobe, one unit, so a
        // later span edit of x re-strobes it without duplicates.
        let (mut state, x, w) = setup();
        super::super::state::remove_animation(&mut state.source, w);
        assert_eq!(label_ranges(&state), vec![(1, 2), (5, 6), (9, 10)]);
        let a = anim_index(&state, x);
        let _ = apply_property(&mut state, a, "last_frame", "14");
        assert_eq!(label_ranges(&state), vec![(1, 2), (5, 6), (9, 10), (13, 14)]);
        // x removed: locked to width's span; editing it moves the one object.
        let (mut state, x, w) = setup();
        super::super::state::remove_animation(&mut state.source, x);
        assert_eq!(label_ranges(&state), vec![(11, 14)]);
        let a = anim_index(&state, w);
        let _ = apply_property(&mut state, a, "last_frame", "15");
        assert_eq!(label_ranges(&state), vec![(11, 15)]);
        // Re-applying width after that keeps one object too.
        apply_animation(&mut state, 0, "width", 10, 30, 0, 0, false, 11, 14, false, true, 500, 0);
        assert_eq!(label_ranges(&state), vec![(11, 15)]);
    }

    #[test]
    fn editing_an_animation_span_past_the_deck_is_rejected() {
        // A gapped animation's last_frame typed far past the 10-frame deck must
        // be refused before any re-strobe (which would clone the element once per
        // sample frame), leaving the deck untouched; a span inside it applies.
        use crate::editor::object_defaults::create_default;
        let mut state = EditorState::open(&crate::test_tmp("bs_span_edit_absent_16.json")).unwrap();
        state.source.frame_count = 10;
        let mut o = create_default(0, 0); // Label
        if let SceneObject::Label(l) = &mut o {
            l.position.x = Coordinate::Animated { from: 2, to: 40, anim: 1 };
        }
        let mut a = anim(1, 1, 9, true);
        if let SceneObject::Animation(an) = &mut a {
            an.gap_frames = 1;
        }
        state.source.objects = vec![o, a];
        let old = AnimSpans::of(&state.source);
        crate::editor::state::restrobe(&mut state.source, 0, &old);
        let before = state.source.objects.len();

        for bad in ["1000000000", "11"] {
            let _ = apply_property(&mut state, 1, "last_frame", bad);
            assert_eq!(state.source.objects.len(), before, "last_frame {bad}");
            assert!(state.status_message.as_deref().unwrap().starts_with("Error:"));
        }
        let _ = apply_property(&mut state, 1, "first_frame", "10"); // 1-based: start 9
        assert!(state.status_message.as_deref().unwrap().contains("at least one frame"));

        // A hand-edited span already past the deck: narrowing it to one frame
        // still past the end is refused too.
        if let SceneObject::Animation(an) = &mut state.source.objects[1] {
            an.frames = crate::engine::source::FrameRange { start: 1, end: 20 };
        }
        let _ = apply_property(&mut state, 1, "first_frame", "20"); // start 19
        assert!(state.status_message.as_deref().unwrap().contains("past the deck"));

        let _ = apply_property(&mut state, 1, "last_frame", "10");
        assert!(!state.status_message.as_deref().unwrap().starts_with("Error:"));
    }

    #[test]
    fn a_frame_value_off_the_deck_is_rejected() {
        let mut state = deck_state(vec![deck_label(None)]); // 10 frames, label 0..10
        let range = |s: &EditorState| {
            let fr = crate::editor::state::scene_object_frame_range(&s.source.objects[0]).unwrap();
            (fr.start, fr.end)
        };
        for (name, bad) in [("last_frame", "18446744073709551615"), ("last_frame", "11"),
                            ("first_frame", "11"), ("last_frame", "0")] {
            let _ = apply_property(&mut state, 0, name, bad);
            assert!(state.status_message.as_deref().unwrap().starts_with("Error:"), "{name}={bad}");
            assert_eq!(range(&state), (0, 10), "{name}={bad}: unchanged");
        }
        let _ = apply_property(&mut state, 0, "first_frame", "4");
        let _ = apply_property(&mut state, 0, "last_frame", "3");
        assert!(state.status_message.as_deref().unwrap().contains("after last_frame"));
        let _ = apply_property(&mut state, 0, "last_frame", "10");
        assert_eq!(range(&state), (3, 10));
    }

    #[test]
    fn linked_tables_share_cell_and_column_edits() {
        let table = || -> SceneObject {
            serde_json::from_str(r#"{ "type": "table",
                "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                "col_widths": [0.5, 0.5], "rows": 2,
                "frames": { "start": 0, "end": 10 } }"#).unwrap()
        };
        let mut state = deck_state(vec![table(), table()]);
        state.source.links = vec![vec![0, 1]];
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let content = |s: &EditorState, i: usize| table_content(&s.source.objects[i]).unwrap();

        // Cell text typed into the copy reaches the original.
        state.mode = Mode::TableEditCellProps {
            object_index: 1, cursor_row: 0, cursor_col: 0, selected_cells: vec![],
            sub_state: TableCellSubState::EditingContent { row: 0, col: 0, buf: "Q3".into(), cursor: 2 },
        };
        handle_key(&mut state, enter);
        assert!(content(&state, 0).contains("Q3"));
        assert_eq!(content(&state, 0), content(&state, 1));

        // So does a column added to it, keeping the column layouts aligned.
        state.mode = Mode::TableAddColumn { object_index: 1, after: true, col_num: 1, buf: "1".into(), cursor: 1 };
        handle_key(&mut state, enter);
        let cols = |s: &EditorState, i: usize| match &s.source.objects[i] {
            SceneObject::Table(t) => t.col_widths.len(),
            _ => 0,
        };
        assert_eq!((cols(&state, 0), cols(&state, 1)), (3, 3));
        assert_eq!(content(&state, 0), content(&state, 1));
    }

    #[test]
    fn linked_table_edits_skip_a_diverged_table() {
        let table = |cell: &str| -> SceneObject {
            serde_json::from_str(&format!(r#"{{ "type": "table",
                "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }},
                "col_widths": [0.5, 0.5], "rows": 1,
                "cells": [[{{ "content": "{cell}" }}, {{ "content": "b" }}]],
                "frames": {{ "start": 0, "end": 10 }} }}"#)).unwrap()
        };
        let cols = |s: &EditorState, i: usize| match &s.source.objects[i] {
            SceneObject::Table(t) => t.col_widths.len(),
            _ => 0,
        };
        // Tables 0 and 1 are in sync; 2 (linked before tables synced) differs.
        let mut state = deck_state(vec![table("a"), table("a"), table("mine")]);
        state.source.links = vec![vec![0, 1, 2]];

        // Removing column 2 from table 0 (via its confirm) reaches table 1.
        state.mode = Mode::Confirm {
            message: String::new(),
            selected: 0,
            action: ConfirmAction::RemoveTableColumn { object_index: 0, col_index: 1 },
            return_mode: Box::new(Mode::Normal),
        };
        handle_key(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!((cols(&state, 0), cols(&state, 1)), (1, 1));
        assert_eq!(table_content(&state.source.objects[0]), table_content(&state.source.objects[1]));

        // Table 2 keeps its own cells and columns; the status says why.
        assert_eq!(cols(&state, 2), 2);
        assert!(table_content(&state.source.objects[2]).unwrap().contains("mine"));
        assert!(state.status_message.as_deref().unwrap().contains("1 linked table(s) differ"));

        // A cell edit likewise leaves the diverged table's cells alone.
        state.mode = Mode::TableEditCellProps {
            object_index: 0, cursor_row: 0, cursor_col: 0, selected_cells: vec![],
            sub_state: TableCellSubState::EditingContent { row: 0, col: 0, buf: "new".into(), cursor: 3 },
        };
        handle_key(&mut state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(table_content(&state.source.objects[1]).unwrap().contains("new"));
        let kept = table_content(&state.source.objects[2]).unwrap();
        assert!(kept.contains("mine") && kept.contains("\"b\"") && !kept.contains("new"), "{kept}");
        // The note replaces the previous key's status instead of piling up.
        assert_eq!(state.status_message.as_deref(), Some("1 linked table(s) differ and were not synced"));
    }

    #[test]
    fn a_linked_paste_of_a_strobe_copy_links_to_the_original() {
        let (mut state, copy) = strobed_label();
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        copy_to_clipboard(&mut state, &[copy]);
        state.current_frame = 12;
        start_paste(&mut state);
        handle_key(&mut state, key(KeyCode::Char('l')));
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Esc));
        let clone = state.source.links[0].iter().copied().find(|&i| i != 0).unwrap();
        assert_eq!(state.source.links, vec![vec![0, clone]], "anchored on the original");

        // An edit through the original reaches the pasted clone.
        let text = properties::get_properties(&state.source.objects, 0)
            .iter()
            .position(|p| p.name == "text")
            .unwrap();
        state.mode = ep_editing(0, text, "Bye".into(), 3, 0, 0);
        handle_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(&state.source.objects[clone], SceneObject::Label(l) if l.text == "Bye"));

        // Re-applying the animation re-strobes without dropping the link.
        apply_animation(&mut state, 0, "x", 10, 40, 0, 0, false, 2, 10, false, true, 500, 3);
        assert_eq!(state.source.links.len(), 1);
        assert!(state.source.links[0].contains(&0) && state.source.links[0].len() == 2);
        let clone = state.source.links[0][1];
        assert!(matches!(&state.source.objects[clone], SceneObject::Label(l) if l.text == "Bye"));
    }
}
