use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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
    adjust_frames_after_delete, adjust_group_members_after_delete, copy_frame,
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
}

/// Whether the current mode is actively capturing typed characters into a text
/// buffer. When true, a plain-letter global shortcut (like fullscreen on "f")
/// must yield to text input so the letter can be typed literally.
fn mode_accepts_text(mode: &Mode) -> bool {
    match mode {
        Mode::EditProperties { editing_value, .. } => editing_value.is_some(),
        Mode::EditMultiProperties { editing_value, .. } => editing_value.is_some(),
        Mode::AnimateProperty { editing, .. } => editing.is_some(),
        Mode::Settings { .. }
        | Mode::TableAddColumn { .. }
        | Mode::TableRemoveColumn { .. }
        | Mode::LoadArtFile { .. }
        | Mode::SaveAs { .. }
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
        // Ignore key release/repeat events (only delivered when the terminal's
        // keyboard-enhancement protocol is active) so each press fires once.
        Event::Key(key) if key.kind != KeyEventKind::Press => Action::Continue,
        Event::Key(key) => handle_key(state, key),
        Event::Resize(_, _) => Action::Redraw,
        _ => Action::Continue,
    }
}

fn handle_key(state: &mut EditorState, key: KeyEvent) -> Action {
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
fn apply_property(state: &mut EditorState, object_index: usize, name: &str, value: &str) {
    // A group's frame range is optional ("auto", derived from members). Editing
    // first_frame/last_frame transitions it:
    //   * blank value  -> revert to auto (no override of member ranges)
    //   * any value    -> materialise an explicit range (seeded from the derived
    //                     union) that then overrides member ranges
    if matches!(state.source.objects.get(object_index), Some(SceneObject::Group(_)))
        && (name == "first_frame" || name == "last_frame")
    {
        if value.trim().is_empty() {
            if let SceneObject::Group(g) = &mut state.source.objects[object_index] {
                g.frames = None;
            }
            state.dirty = true;
            state.status_message = Some("Group range: auto (from members)".into());
            return;
        }
        let derived = state.source.effective_frame_range(object_index);
        if let SceneObject::Group(g) = &mut state.source.objects[object_index] {
            if g.frames.is_none() {
                g.frames = Some(derived);
            }
        }
    }
    match properties::set_property(&mut state.source.objects[object_index], name, value) {
        Ok(()) => {
            state.dirty = true;
            // Editing an `Animation`'s span (its first/last frame — the single
            // source of truth) re-locks the visibility range of every object it
            // drives, so they stay shown across the new span. No coordinate spans
            // to touch: they reference the animation by id.
            if name == "first_frame" || name == "last_frame" {
                let anim_id = match state.source.objects.get(object_index) {
                    Some(SceneObject::Animation(a)) => Some(a.id),
                    _ => None,
                };
                if let Some(id) = anim_id {
                    let driven: Vec<usize> = (0..state.source.objects.len())
                        .filter(|&j| super::state::referenced_anim_ids(&state.source.objects[j]).contains(&id))
                        .collect();
                    for j in driven {
                        lock_range_to_animation(&mut state.source, j);
                    }
                }
            }
            // Linked objects share appearance: a non-placement edit propagates to
            // the family (placement/layering stays per-object). Siblings that lack
            // the property (a mixed-type family) simply ignore it.
            let mut propagated = 0usize;
            if !is_placement_prop(name) {
                for j in state.source.link_siblings(object_index) {
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
}

/// Apply one property edit to **every** member of a multi-object selection,
/// reusing the single-object `apply_property` per member (so group auto-range,
/// animation re-locking, link propagation, and loop validation all behave
/// exactly as they do for a single edit). Members lacking the property simply
/// report an error and are skipped; a final summary replaces the per-member
/// status. Returns nothing — the status line carries the outcome.
fn apply_multi_property(state: &mut EditorState, members: &[usize], name: &str, value: &str) {
    let mut ok = 0usize;
    let mut last_err: Option<String> = None;
    for &m in members {
        apply_property(state, m, name, value);
        // `apply_property` reports through the status line; sniff its result so
        // the summary can distinguish a fully-applied edit from a partial one.
        match state.status_message.as_deref() {
            Some(s) if s.starts_with('⚠') || s.starts_with("Error") => last_err = Some(s.to_string()),
            _ => ok += 1,
        }
    }
    state.status_message = Some(match last_err {
        Some(e) if ok == 0 => e,
        Some(_) => format!("Set {name} = {value} on {ok}/{} objects", members.len()),
        None => format!("Set {name} = {value} on {ok} objects"),
    });
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
                state.source.width = w;
                state.source.height = h;
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

fn handle_normal(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = &state.config.key_bindings;

    if matches_binding(&bindings.quit, &key) {
        return Action::Quit;
    }
    // Shift + the frame-nav keys jump FRAMES_PER_JUMP frames at once (clamped).
    // Checked before the plain 1-frame nav, which also matches a shifted arrow
    // (`matches_binding` ignores Shift on bare keys).
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        let last = state.source.frame_count.saturating_sub(1);
        if matches_binding(&bindings.next_frame, &key) {
            state.current_frame = (state.current_frame + FRAMES_PER_JUMP).min(last);
            state.status_message = None;
            return Action::Redraw;
        }
        if matches_binding(&bindings.prev_frame, &key) {
            state.current_frame = state.current_frame.saturating_sub(FRAMES_PER_JUMP);
            state.status_message = None;
            return Action::Redraw;
        }
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
        state.mode = Mode::FrameMenu;
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

/// "Save as" filename input: type a path, Enter writes the deck there (adopting
/// the new path), Esc cancels.
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
        match state.save_as(path) {
            Ok(()) => state.mode = Mode::Normal,
            // Stay in the input so the path can be corrected.
            Err(e) => {
                state.status_message = Some(format!("Save failed: {e}"));
                state.mode = Mode::SaveAs { buf, cursor };
            }
        }
        return Action::Redraw;
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
        let on = super::state::set_frame_auto_advance(&mut state.source, frame, ms);
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
        let new_index = move_frame(&mut state.source, from, target, before);
        state.current_frame = new_index;
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
    let type_name = object_defaults::OBJECT_TYPES[index];
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
            let (lo, hi) = (a.frames.start + 1, a.frames.end);
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
/// single-object copy from `SelectedObject`.
fn copy_to_clipboard(state: &mut EditorState, indices: &[usize]) {
    let expanded = super::state::expand_selection(&state.source, indices);
    state.clipboard = super::state::clone_selection(&state.source.objects, &expanded);
    state.clipboard_sources = expanded;
    let n = state.clipboard.len();
    state.status_message = Some(format!(
        "Copied {n} object{} — press paste to place",
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
    // accumulated per-object link family.
    if matches_binding(&bindings.cancel, &key) {
        let keep = state.source.objects.len().saturating_sub(pending.len());
        state.source.objects.truncate(keep);
        if linked {
            for mut fam in families {
                fam.sort_unstable();
                fam.dedup();
                fam.retain(|&i| i < state.source.objects.len());
                if fam.len() >= 2 {
                    state.source.links.push(fam);
                }
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
            for &i in &pending {
                properties::move_object(&mut state.source.objects[i], dx, dy);
            }
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

/// Properties that stay **per-object** on linked copies (placement/layering);
/// every other property propagates to a linked object's siblings.
fn is_placement_prop(name: &str) -> bool {
    matches!(
        name,
        "x" | "y" | "width" | "height" | "first_frame" | "last_frame" | "z_order"
    )
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
                            // Deleting an `Animation` removes the *whole* animation:
                            // both the auto-play sidecar and the motion it drives
                            // (otherwise the element keeps moving with no sidecar).
                            if let SceneObject::Animation(a) = &state.source.objects[object_index] {
                                let id = a.id;
                                super::state::remove_animation(&mut state.source, id);
                                state.status_message =
                                    Some("Animation removed (objects frozen at their start)".into());
                            } else {
                                state.source.objects.remove(object_index);
                                adjust_group_members_after_delete(&mut state.source, object_index);
                                state.status_message = Some("Object deleted".into());
                            }
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
        apply_property(state, object_index, prop_name, properties::toggled_bool_value(&prop_value));
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
            apply_property(state, object_index, prop_name, &te.buf);
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
                apply_property(state, object_index, prop_name, chosen);
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
        apply_multi_property(state, &members, prop_name, properties::toggled_bool_value(&prop_value));
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
            apply_multi_property(state, &members, prop_name, &te.buf);
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
                apply_multi_property(state, &members, prop_name, chosen);
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
    let (from, to, from_y, to_y, anim_id) = if two_axis {
        let (xf, xt, xa) = read("x");
        let (yf, yt, ya) = read("y");
        (xf, xt, yf, yt, xa.or(ya))
    } else {
        let (f, t, a) = read(property_name);
        (f, t, 0, 0, a)
    };
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
            apply_animation(state, object_index, property_name, from, to, from_y, to_y, two_axis,
                start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames);
            state.mode = ep_browse(object_index, return_property, 0);
            return Action::Redraw;
        }
        // [x] clear → Fixed coordinate(s). Also removes any gap-strobe copies so
        // clearing doesn't leave the element scattered on its old sample frames.
        KeyCode::Char('x') if key.modifiers == KeyModifiers::NONE => {
            let anims = AnimSpans::of(&state.source);
            if let Some((lo, hi)) = super::state::scene_object_animation_span(&state.source.objects[object_index], &anims) {
                super::state::clear_gap_clones(&mut state.source, object_index, lo, hi.saturating_sub(1));
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
                    // one we just cleared, if nothing else still drives it).
                    super::state::prune_orphan_animations(&mut state.source);
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

/// Apply the animation configured in the Animate sub-menu: optionally insert the
/// spanned frames and share the current frame's elements across them, set the
/// animated coordinate, keep the object's own range in lock-step with its
/// animation, and create/update the Animation span entity (auto-play config).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn apply_animation(
    state: &mut EditorState, object_index: usize, property_name: &'static str,
    from: u16, to: u16, from_y: u16, to_y: u16, two_axis: bool,
    start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64, gap_frames: usize,
) {
    let animate = start_frame < end_frame;
    let end_excl = end_frame + 1;

    // Reuse the animation this object's coordinate already references, so editing
    // a span updates the *same* animation (the single source of truth) instead of
    // spawning a second one. Allocate a fresh id only for a brand-new animation.
    let existing_id = super::state::referenced_anim_ids(&state.source.objects[object_index])
        .first()
        .copied();
    let id = existing_id.unwrap_or_else(|| super::state::next_anim_id(&state.source));

    // Remove any prior gap-strobe copies of this element (matched by its *current*
    // animated coords, before we change them), so re-applying is idempotent.
    let anims_now = AnimSpans::of(&state.source);
    if let Some((lo, hi)) = super::state::scene_object_animation_span(&state.source.objects[object_index], &anims_now) {
        super::state::clear_gap_clones(&mut state.source, object_index, lo, hi.saturating_sub(1));
    }

    // "Add frames" inserts the span's frames (and shares the current frame's
    // elements). Only for a *brand-new* animation — re-editing an existing one
    // must never insert again.
    if animate && add_frames && existing_id.is_none() {
        super::state::add_frames_and_share(&mut state.source, state.current_frame, start_frame, end_frame);
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
        return;
    }
    state.dirty = true;

    // Lock the object's range to the union of the animations driving it.
    lock_range_to_animation(&mut state.source, object_index);

    // Gap-frames: strobe the element so it shows every `gap_frames + 1` frames of
    // the span. Old copies were cleared above, so this is idempotent on re-apply.
    if animate && gap_frames > 0 {
        super::state::apply_gap(&mut state.source, object_index, start_frame, end_frame, gap_frames);
    }

    // If no axis actually moved (from == to on all), the coordinates stayed Fixed
    // and the animation we just ensured drives nothing — drop it so a no-op apply
    // never leaves an inert animation behind.
    super::state::prune_orphan_animations(&mut state.source);

    // A new animation can collide with a loop (a loop may not bisect an
    // animation); surface that live, the way property edits do.
    let what = if two_axis { "position" } else { property_name };
    state.status_message = Some(match state.source.validate_loops() {
        Ok(()) => format!("Animated {what}"),
        Err(e) => format!("Animated {what} — ⚠ {e}"),
    });
}

/// Lock an object's visible frame range to the union of the spans of the
/// animations that drive it (grows for a longer animation, shrinks when one is
/// shortened — no zombie frames). A no-op if the object has no animated coord.
fn lock_range_to_animation(source: &mut SourcePresentation, object_index: usize) {
    let anims = AnimSpans::of(source);
    if let Some((lo, hi)) = super::state::scene_object_animation_span(&source.objects[object_index], &anims) {
        if let Some(fr) = super::state::scene_object_frame_range_mut(&mut source.objects[object_index]) {
            fr.start = lo;
            fr.end = hi;
        }
    }
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
    // re-applying is idempotent.
    for &idx in members {
        if let Some((lo, hi)) = super::state::scene_object_animation_span(&state.source.objects[idx], &anims) {
            super::state::clear_gap_clones(&mut state.source, idx, lo, hi.saturating_sub(1));
        }
    }

    // Insert + share the spanned frames once (if requested).
    if add_frames {
        super::state::add_frames_and_share(&mut state.source, state.current_frame, start_frame, end_frame);
    }

    // One shared animation drives the whole convergence.
    let id = super::state::next_anim_id(&state.source);
    super::state::ensure_animation(&mut state.source, id, start_frame, end_excl, auto_play, delay_ms, gap_frames);

    // Each member animates whichever of x/y it has, from where it sits now toward
    // the shared target, referencing the one shared animation.
    let mut animated: Vec<usize> = Vec::new();
    let mut errors = 0;
    for &idx in members {
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
            Ok(()) => {
                lock_range_to_animation(&mut state.source, idx);
                animated.push(idx);
            }
            Err(_) => errors += 1,
        }
    }

    if gap_frames > 0 {
        for &idx in &animated {
            super::state::apply_gap(&mut state.source, idx, start_frame, end_frame, gap_frames);
        }
    }

    // Drop the shared animation if nothing actually moved, and any of the members'
    // previous animations left without a referrer by the convergence.
    super::state::prune_orphan_animations(&mut state.source);

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
        SceneObject::Table(t) => (t.rows, t.col_widths.len()),
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

    let (object_index, cursor_row, cursor_col, selected_cells, edit_row, edit_col, mut buf, mut cursor) =
        match &state.mode {
            Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row, col, buf, cursor },
            } => (*object_index, *cursor_row, *cursor_col, selected_cells.clone(), *row, *col, buf.clone(), *cursor),
            _ => return Action::Continue,
        };

    // The cursor highlights a character (block cursor); inserting puts the new
    // character *after* the highlighted one and moves the highlight onto it.
    // `cursor` ranges over `0..=len` (== len is the trailing "append" slot).
    let insert_after = |buf: &mut String, cursor: &mut usize, ch: char| {
        let len = buf.chars().count();
        let at = if *cursor >= len { *cursor } else { *cursor + 1 };
        let bi = char_to_byte_idx(buf, at);
        buf.insert(bi, ch);
        *cursor = at;
    };

    // Shift-Enter (or Alt-Enter, where Shift isn't reported): insert a newline
    // instead of saving. The cursor lands on the newline, rendered as a caret at
    // the start of the new line, so typing continues there.
    if matches_binding(&bindings.insert_newline, &key)
        || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT))
    {
        insert_after(&mut buf, &mut cursor, '\n');
        state.mode = Mode::TableEditCellProps {
            object_index, cursor_row, cursor_col, selected_cells,
            sub_state: TableCellSubState::EditingContent { row: edit_row, col: edit_col, buf, cursor },
        };
        return Action::Redraw;
    }

    match key.code {
        KeyCode::Enter => {
            // Save content
            if let SceneObject::Table(t) = &mut state.source.objects[object_index] {
                t.normalize_cells();
                if let Some(row_vec) = t.cells.get_mut(edit_row) {
                    if let Some(cell) = row_vec.get_mut(edit_col) {
                        cell.content = buf;
                    }
                }
                state.dirty = true;
            }
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::Redraw;
        }
        KeyCode::Esc => {
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::Selecting,
            };
            return Action::Redraw;
        }
        KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
            insert_after(&mut buf, &mut cursor, c);
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row: edit_row, col: edit_col, buf, cursor },
            };
            return Action::Redraw;
        }
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
            // Delete the highlighted character (the inverse of insert-after) and
            // move the highlight to the preceding one.
            let len = buf.chars().count();
            if len > 0 {
                let del = if cursor >= len { len - 1 } else { cursor };
                let bi = char_to_byte_idx(&buf, del);
                buf.remove(bi);
                cursor = del.saturating_sub(1);
            }
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row: edit_row, col: edit_col, buf, cursor },
            };
            return Action::Redraw;
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            cursor = cursor.saturating_sub(1);
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row: edit_row, col: edit_col, buf, cursor },
            };
            return Action::Redraw;
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            cursor = (cursor + 1).min(buf.chars().count());
            state.mode = Mode::TableEditCellProps {
                object_index, cursor_row, cursor_col, selected_cells,
                sub_state: TableCellSubState::EditingContent { row: edit_row, col: edit_col, buf, cursor },
            };
            return Action::Redraw;
        }
        _ => {}
    }
    Action::Continue
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
                            st.bold = !st.bold;
                        }
                        "dimmed" => {
                            st.dim = !st.dim;
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
        let mut state = EditorState::open("/tmp/bs_gap_regression_does_not_exist_42.json").unwrap();
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

        let mut state = EditorState::open("/tmp/bs_gap_reapply_absent_77.json").unwrap();
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

        let mut state = EditorState::open("/tmp/bs_converge_absent_91.json").unwrap();
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

        let mut state = EditorState::open("/tmp/bs_anim_no_dup_absent_3.json").unwrap();
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
}
