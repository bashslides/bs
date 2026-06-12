use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal;

use crate::engine::objects::table::{table_add_column, table_remove_column};
use crate::engine::objects::Group;
use crate::engine::source::{Coordinate, SceneObject};
use crate::types::Style;
use super::config::matches_binding;
use super::object_defaults;
use super::properties;
use super::textedit::{TextAction, TextEdit};
use super::state::{
    adjust_frames_after_delete, adjust_group_members_after_delete, copy_frame,
    insert_blank_frame, move_frame, overlay_frame, ArtPick, ConfirmAction, EditorState, Mode,
    MultiSelectPurpose, TableCellSubState,
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
        Mode::AnimateProperty { editing, .. } => editing.is_some(),
        Mode::Settings { .. }
        | Mode::TableAddColumn { .. }
        | Mode::TableRemoveColumn { .. }
        | Mode::LoadArtFile { .. } => true,
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
        Mode::FrameMenu => handle_frame_menu(state, key),
        Mode::FrameMove { .. } => handle_frame_move(state, key),
        Mode::FrameMovePlace { .. } => handle_frame_move_place(state, key),
        Mode::FrameOverlay { .. } => handle_frame_overlay(state, key),
        Mode::AddObject { .. } => handle_add_object(state, key),
        Mode::SelectObject { .. } => handle_select_object(state, key),
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
        Mode::AnimateProperty { .. } => handle_animate_property(state, key),
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

fn handle_normal(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = &state.config.key_bindings;

    if matches_binding(&bindings.quit, &key) {
        return Action::Quit;
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
            state.mode = Mode::SelectObject { selected: 0 };
            state.status_message = None;
        } else {
            state.status_message = Some("No objects on this frame".into());
        }
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
    // Copy: pick a set of objects on this frame to capture to the clipboard.
    if matches_binding(&bindings.copy, &key) {
        if state.objects_on_current_frame().is_empty() {
            state.status_message = Some("No objects on this frame to copy".into());
        } else {
            state.mode = Mode::MultiSelect {
                purpose: MultiSelectPurpose::Copy,
                selected: 0,
                members: Vec::new(),
            };
            state.status_message = Some("Copy: [Space] toggle objects, [Enter] copy".into());
        }
        return Action::Redraw;
    }
    // Paste: place the clipboard's clones as a movable ghost on this frame.
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

    Action::Continue
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
    // Enter: commit — create a group, or copy the set to the clipboard. With
    // nothing explicitly toggled, fall back to the highlighted object.
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
            MultiSelectPurpose::Copy => {
                copy_to_clipboard(state, &chosen);
                state.mode = Mode::Normal;
            }
        }
        return Action::Redraw;
    }

    Action::Continue
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
    let mut clones = state.clipboard.clone();
    for obj in &mut clones {
        // Re-anchor to the current frame (auto groups keep their derived range).
        if let Some(fr) = super::state::scene_object_frame_range_mut(obj) {
            fr.start = current;
            fr.end = current + 1;
        }
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

fn handle_select_object(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    let selected = match &state.mode {
        Mode::SelectObject { selected } => *selected,
        _ => return Action::Continue,
    };

    let visible = state.objects_on_current_frame();
    if visible.is_empty() {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { visible.len() - 1 } else { selected - 1 };
        state.mode = Mode::SelectObject { selected: new_sel };
        return Action::BlinkSelection;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % visible.len();
        state.mode = Mode::SelectObject { selected: new_sel };
        return Action::BlinkSelection;
    }
    if matches_binding(&bindings.confirm, &key) {
        let obj_index = visible[selected];
        state.mode = Mode::SelectedObject { object_index: obj_index };
        return Action::Redraw;
    }
    if matches_binding(&bindings.delete_object, &key) {
        let obj_index = visible[selected];
        let message = if matches!(state.source.objects[obj_index], SceneObject::Group(_)) {
            "Ungroup? (members are kept)".to_string()
        } else {
            format!("Delete {}?", super::state::scene_object_summary(&state.source.objects[obj_index]))
        };
        state.mode = Mode::Confirm {
            message,
            selected: 0,
            action: ConfirmAction::DeleteObject { object_index: obj_index },
            return_mode: Box::new(Mode::SelectObject { selected }),
        };
        return Action::Redraw;
    }

    Action::Continue
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
        let message = if matches!(state.source.objects[object_index], SceneObject::Group(_)) {
            "Ungroup? (members are kept)".to_string()
        } else {
            format!("Delete {}?", super::state::scene_object_summary(&state.source.objects[object_index]))
        };
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
                match key.code {
                    KeyCode::Right => properties::resize_object(&mut state.source.objects[object_index], 1, 0),
                    KeyCode::Left  => properties::shrink_object(&mut state.source.objects[object_index], 1, 0),
                    KeyCode::Down  => grow_table_height(&mut state.source.objects[object_index], frame, 1),
                    KeyCode::Up    => grow_table_height(&mut state.source.objects[object_index], frame, -1),
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
            KeyCode::Down  => grow_table_height(&mut objects[object_index], frame, 1),
            KeyCode::Up    => grow_table_height(&mut objects[object_index], frame, -1),
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
fn grow_table_height(obj: &mut SceneObject, frame: usize, delta: i32) {
    if let SceneObject::Table(t) = obj {
        // Only adjust a fixed height; leave an animated height untouched.
        if !matches!(t.height, Coordinate::Fixed(_)) {
            return;
        }
        let natural = t.natural_height(frame);
        let current = t.height.evaluate(frame).max(natural);
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
                    ConfirmAction::DeleteObject { object_index } => {
                        if object_index < state.source.objects.len() {
                            state.source.objects.remove(object_index);
                            adjust_group_members_after_delete(&mut state.source, object_index);
                            // Source indices for a linked paste are now stale.
                            state.clipboard_sources.clear();
                            state.dirty = true;
                            state.status_message = Some("Object deleted".into());
                        }
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
                if let Coordinate::Animated { from, to, start_frame, end_frame } = &coord {
                    state.mode = enter_animate(
                        state, object_index, selected_property, prop_name,
                        *from, *to, *start_frame, *end_frame,
                    );
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
            if let Some(coord) = properties::get_coord(&state.source.objects[object_index], prop_name) {
                let (from, to, start_frame, end_frame) = match &coord {
                    Coordinate::Fixed(v) => (
                        v.floor() as u16, v.floor() as u16,
                        state.current_frame,
                        state.source.frame_count.saturating_sub(1),
                    ),
                    Coordinate::Animated { from, to, start_frame, end_frame } => {
                        (*from, *to, *start_frame, *end_frame)
                    }
                };
                state.mode = enter_animate(
                    state, object_index, selected_property, prop_name,
                    from, to, start_frame, end_frame,
                );
                return Action::Redraw;
            }
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

/// Number of fields in the Animate sub-menu: from, to, start, end, add_frames,
/// auto_play, delay_ms.
const ANIM_FIELDS: usize = 7;

/// Build the `AnimateProperty` mode in one place (the variant has many fields).
#[allow(clippy::too_many_arguments)]
fn anim_mode(
    object_index: usize, return_property: usize, property_name: &'static str,
    selected_field: usize, editing: Option<String>, cursor: usize,
    from: u16, to: u16, start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64,
) -> Mode {
    Mode::AnimateProperty {
        object_index, return_property, property_name, selected_field, editing, cursor,
        from, to, start_frame, end_frame, add_frames, auto_play, delay_ms,
    }
}

/// Open the Animate sub-menu, seeding the add-frames / auto-play / delay config
/// from an Animation span that exactly matches `[start_frame, end_frame + 1)` —
/// so re-animating that span (or adding a second coordinate to it) keeps its
/// current auto-play settings — else the defaults (add frames on, auto-play on,
/// 500 ms).
#[allow(clippy::too_many_arguments)]
fn enter_animate(
    state: &EditorState, object_index: usize, return_property: usize,
    property_name: &'static str, from: u16, to: u16, start_frame: usize, end_frame: usize,
) -> Mode {
    let end_excl = end_frame + 1;
    let (auto_play, delay_ms) = state
        .source
        .objects
        .iter()
        .find_map(|o| match o {
            SceneObject::Animation(a)
                if a.frames.start == start_frame && a.frames.end == end_excl =>
            {
                Some((a.auto_play, a.delay_ms))
            }
            _ => None,
        })
        .unwrap_or((true, 500));
    anim_mode(
        object_index, return_property, property_name, 0, None, 0,
        from, to, start_frame, end_frame, true, auto_play, delay_ms,
    )
}

fn handle_animate_property(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, return_property, property_name, selected_field, editing, cursor,
         mut from, mut to, mut start_frame, mut end_frame, mut add_frames, mut auto_play, mut delay_ms) =
        match &state.mode {
            Mode::AnimateProperty {
                object_index, return_property, property_name, selected_field, editing, cursor,
                from, to, start_frame, end_frame, add_frames, auto_play, delay_ms,
            } => (
                *object_index, *return_property, *property_name, *selected_field,
                editing.clone(), *cursor, *from, *to, *start_frame, *end_frame,
                *add_frames, *auto_play, *delay_ms,
            ),
            _ => return Action::Continue,
        };

    // -- Editing a numeric field value -----------------------------------------
    if let Some(mut buf) = editing {
        match key.code {
            KeyCode::Enter => {
                // `start`/`end` are entered 1-based (matching first/last_frame).
                let err: Option<String> = match selected_field {
                    0 => buf.parse::<u16>().map(|v| from = v).err().map(|e| format!("Invalid number: {e}")),
                    1 => buf.parse::<u16>().map(|v| to = v).err().map(|e| format!("Invalid number: {e}")),
                    2 => buf.trim().parse::<usize>().map(|v| start_frame = v.saturating_sub(1)).err().map(|e| format!("Invalid number: {e}")),
                    3 => buf.trim().parse::<usize>().map(|v| end_frame = v.saturating_sub(1)).err().map(|e| format!("Invalid number: {e}")),
                    _ => buf.trim().parse::<u64>().map(|v| delay_ms = v).err().map(|e| format!("Invalid number: {e}")),
                };
                if let Some(msg) = err {
                    state.status_message = Some(msg);
                }
                state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                    None, 0, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
                return Action::Redraw;
            }
            KeyCode::Esc => {
                state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                    None, 0, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
                return Action::Redraw;
            }
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
                let new_cursor = cursor.saturating_sub(1);
                state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                    Some(buf), new_cursor, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
                return Action::Redraw;
            }
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                let new_cursor = (cursor + 1).min(buf.chars().count());
                state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                    Some(buf), new_cursor, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
                return Action::Redraw;
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    let s = char_to_byte_idx(&buf, cursor - 1);
                    let e = char_to_byte_idx(&buf, cursor);
                    buf.drain(s..e);
                    state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                        Some(buf), cursor - 1, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
                    return Action::Redraw;
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let byte_idx = char_to_byte_idx(&buf, cursor);
                buf.insert(byte_idx, c);
                state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                    Some(buf), cursor + 1, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
                return Action::Redraw;
            }
            _ => {}
        }
        return Action::Continue;
    }

    // -- Browsing fields -------------------------------------------------------
    let is_toggle = selected_field == 4 || selected_field == 5;
    match key.code {
        KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
            let new_sel = if selected_field == 0 { ANIM_FIELDS - 1 } else { selected_field - 1 };
            state.mode = anim_mode(object_index, return_property, property_name, new_sel,
                None, 0, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
            return Action::Redraw;
        }
        KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            let new_sel = (selected_field + 1) % ANIM_FIELDS;
            state.mode = anim_mode(object_index, return_property, property_name, new_sel,
                None, 0, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
            return Action::Redraw;
        }
        // Space / Enter on a boolean field toggles it in place (no text detour).
        KeyCode::Char(' ') | KeyCode::Enter if is_toggle => {
            if selected_field == 4 { add_frames = !add_frames; } else { auto_play = !auto_play; }
            state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                None, 0, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
            return Action::Redraw;
        }
        KeyCode::Enter => {
            // Start editing the selected numeric field. start/end shown 1-based.
            let init = match selected_field {
                0 => from.to_string(),
                1 => to.to_string(),
                2 => (start_frame + 1).to_string(),
                3 => (end_frame + 1).to_string(),
                _ => delay_ms.to_string(),
            };
            let new_cursor = init.chars().count();
            state.mode = anim_mode(object_index, return_property, property_name, selected_field,
                Some(init), new_cursor, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
            return Action::Redraw;
        }
        // [s] apply → animate the coordinate (+ optional add-frames + auto-play).
        KeyCode::Char('s') if key.modifiers == KeyModifiers::NONE => {
            apply_animation(state, object_index, property_name,
                from, to, start_frame, end_frame, add_frames, auto_play, delay_ms);
            state.mode = ep_browse(object_index, return_property, 0);
            return Action::Redraw;
        }
        // [x] clear → Fixed coordinate (does not touch any Animation span).
        KeyCode::Char('x') if key.modifiers == KeyModifiers::NONE => {
            let coord = Coordinate::Fixed(from as f64);
            match properties::set_coordinate(&mut state.source.objects[object_index], property_name, coord) {
                Ok(()) => {
                    state.dirty = true;
                    state.status_message = Some(format!("Fixed {property_name} = {from}"));
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
fn apply_animation(
    state: &mut EditorState, object_index: usize, property_name: &'static str,
    from: u16, to: u16, start_frame: usize, end_frame: usize,
    add_frames: bool, auto_play: bool, delay_ms: u64,
) {
    let animate = start_frame < end_frame;
    let end_excl = end_frame + 1;
    let coord = if animate {
        Coordinate::Animated { from, to, start_frame, end_frame }
    } else {
        Coordinate::Fixed(from as f64)
    };

    // "Add frames" gives the animation its own fresh frames (inserting N-1 after
    // the current one and sharing the current frame's elements across them, so
    // editing one edits all). Only for a *new* span, though: re-applying over a
    // span that already exists — animating Y after X, or re-saving an animation —
    // must not insert frames again, so guard on whether the span already exists.
    let span_exists = state.source.objects.iter().any(|o| {
        matches!(o, SceneObject::Animation(a) if a.frames.start == start_frame && a.frames.end == end_excl)
    });
    if animate && add_frames && !span_exists {
        super::state::add_frames_and_share(&mut state.source, state.current_frame, start_frame, end_frame);
    }

    if let Err(e) = properties::set_coordinate(&mut state.source.objects[object_index], property_name, coord) {
        state.status_message = Some(format!("Error: {e}"));
        return;
    }
    state.dirty = true;

    // Keep the object's visible range in lock-step with its animation window(s)
    // (grows for a longer animation, shrinks when shortened — no zombie frames).
    if let Some((lo, hi)) = super::state::scene_object_animation_span(&mut state.source.objects[object_index]) {
        if let Some(fr) = super::state::scene_object_frame_range_mut(&mut state.source.objects[object_index]) {
            fr.start = lo;
            fr.end = hi;
        }
    }

    // Record the animation span + its auto-play config (reusing one that already
    // covers exactly this span, so X and Y of an object stay one animation).
    if animate {
        super::state::upsert_animation(&mut state.source, start_frame, end_excl, auto_play, delay_ms);
    }

    // A new animation can collide with a loop (a loop may not bisect an
    // animation); surface that live, the way property edits do.
    state.status_message = Some(match state.source.validate_loops() {
        Ok(()) => format!("Animated {property_name}"),
        Err(e) => format!("Animated {property_name} — ⚠ {e}"),
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
