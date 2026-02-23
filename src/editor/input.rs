use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;

use crate::engine::objects::Group;
use crate::engine::source::{Coordinate, FrameRange, SceneObject};
use super::config::matches_binding;
use super::object_defaults;
use super::properties;
use super::state::{adjust_frames_after_delete, adjust_frames_after_insert, adjust_group_members_after_delete, ConfirmAction, EditorState, Mode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Continue,
    Redraw,
    BlinkSelection,
    Quit,
    ToggleFullscreen,
}

pub fn handle_event(state: &mut EditorState, event: Event) -> Action {
    match event {
        Event::Key(key) => handle_key(state, key),
        Event::Resize(_, _) => Action::Redraw,
        _ => Action::Continue,
    }
}

fn handle_key(state: &mut EditorState, key: KeyEvent) -> Action {
    // Global shortcut: works from any mode
    if matches_binding(&state.config.key_bindings.fullscreen, &key) {
        return Action::ToggleFullscreen;
    }

    match &state.mode {
        Mode::Normal => handle_normal(state, key),
        Mode::AddObject { .. } => handle_add_object(state, key),
        Mode::SelectObject { .. } => handle_select_object(state, key),
        Mode::SelectedObject { .. } => handle_selected_object(state, key),
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
        Mode::SelectGroupMembers { .. } => handle_select_group_members(state, key),
    }
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
    if matches_binding(&bindings.add_frame, &key) {
        adjust_frames_after_insert(&mut state.source, state.current_frame);
        state.current_frame += 1;
        state.dirty = true;
        state.status_message = Some(format!("Duplicated → frame {}", state.current_frame + 1));
        return Action::Redraw;
    }
    if matches_binding(&bindings.remove_frame, &key) {
        if state.source.frame_count > 1 {
            state.mode = Mode::Confirm {
                message: format!("Delete frame {}?", state.current_frame + 1),
                selected: 0,
                action: ConfirmAction::DeleteFrame,
                return_mode: Box::new(Mode::Normal),
            };
        }
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
        if object_defaults::OBJECT_TYPES[selected] == "Group" {
            // Group needs member selection before it can be created
            if state.source.objects.is_empty() {
                state.status_message = Some("No objects to group".into());
                state.mode = Mode::Normal;
            } else {
                state.mode = Mode::SelectGroupMembers { selected: 0, members: Vec::new() };
            }
        } else {
            let obj = object_defaults::create_default(
                selected,
                state.current_frame,
                state.source.frame_count,
            );
            let type_name = object_defaults::OBJECT_TYPES[selected];
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
            state.status_message = Some(format!("Added {type_name}"));
        }
        return Action::Redraw;
    }

    Action::Continue
}

fn handle_select_group_members(state: &mut EditorState, key: KeyEvent) -> Action {
    let bindings = state.config.key_bindings.clone();
    let (selected, members) = match &state.mode {
        Mode::SelectGroupMembers { selected, members } => (*selected, members.clone()),
        _ => return Action::Continue,
    };

    let total = state.source.objects.len();
    if total == 0 {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }

    if matches_binding(&bindings.cancel, &key) {
        state.mode = Mode::Normal;
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_up, &key) {
        let new_sel = if selected == 0 { total - 1 } else { selected - 1 };
        state.mode = Mode::SelectGroupMembers { selected: new_sel, members };
        return Action::BlinkSelection;
    }
    if matches_binding(&bindings.move_down, &key) {
        let new_sel = (selected + 1) % total;
        state.mode = Mode::SelectGroupMembers { selected: new_sel, members };
        return Action::BlinkSelection;
    }
    // Space: toggle membership of the highlighted object
    if key.code == KeyCode::Char(' ') {
        let mut new_members = members;
        if let Some(pos) = new_members.iter().position(|&m| m == selected) {
            new_members.remove(pos);
        } else {
            new_members.push(selected);
        }
        state.mode = Mode::SelectGroupMembers { selected, members: new_members };
        return Action::Redraw;
    }
    // Enter: create the group
    if matches_binding(&bindings.confirm, &key) {
        let group = SceneObject::Group(Group {
            members,
            frames: FrameRange {
                start: state.current_frame,
                end: state.source.frame_count,
            },
            z_order: 0,
        });
        state.source.objects.push(group);
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
        state.status_message = Some("Added Group".into());
        return Action::Redraw;
    }

    Action::Continue
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

    let is_group = matches!(state.source.objects[object_index], SceneObject::Group(_));

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

    let prop_count = properties::get_properties(&state.source.objects, object_index).len();

    // GroupMember property: 'd' opens a removal confirmation; editing is blocked.
    // Navigation (Up/Down) is allowed to fall through to the code below.
    {
        let props = properties::get_properties(&state.source.objects, object_index);
        if props[selected_property].kind == properties::PropertyKind::GroupMember {
            if matches_binding(&bindings.delete_object, &key) || key.code == KeyCode::Delete {
                let member_idx: usize = props[selected_property].value.parse().unwrap_or(usize::MAX);
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
                    return_mode: Box::new(Mode::EditProperties {
                        object_index,
                        selected_property,
                        editing_value: None,
                        cursor: 0,
                        scroll: 0,
                        panel_scroll,
                        dropdown: None,
                    }),
                };
                return Action::Redraw;
            }
            // Block editing-start keys; navigation (Up/Down/Tab) falls through below.
            if matches_binding(&bindings.confirm, &key) || matches_binding(&bindings.animate, &key) {
                return Action::Continue;
            }
        }
    }

    // ReadOnly property: block editing keys; allow navigation to fall through.
    {
        let props = properties::get_properties(&state.source.objects, object_index);
        if props[selected_property].kind == properties::PropertyKind::ReadOnly {
            if matches_binding(&bindings.confirm, &key)
                || matches_binding(&bindings.animate, &key)
                || matches_binding(&bindings.delete_object, &key)
                || key.code == KeyCode::Delete
            {
                return Action::Continue;
            }
        }
    }

    // Up/Down, Tab/BackTab: navigate the property list
    if matches_binding(&bindings.move_up, &key)
        || (key.code == KeyCode::BackTab)
    {
        let new_sel = if selected_property == 0 { prop_count - 1 } else { selected_property - 1 };
        state.mode = Mode::EditProperties {
            object_index,
            selected_property: new_sel,
            editing_value: None,
            cursor: 0,
            scroll: 0,
            panel_scroll: 0,
            dropdown: None,
        };
        return Action::Redraw;
    }
    if matches_binding(&bindings.move_down, &key)
        || (key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE)
    {
        let new_sel = (selected_property + 1) % prop_count;
        state.mode = Mode::EditProperties {
            object_index,
            selected_property: new_sel,
            editing_value: None,
            cursor: 0,
            scroll: 0,
            panel_scroll: 0,
            dropdown: None,
        };
        return Action::Redraw;
    }

    // Space: toggle boolean properties in-place (no edit mode needed)
    if key.code == KeyCode::Char(' ') && key.modifiers == KeyModifiers::NONE {
        let props = properties::get_properties(&state.source.objects, object_index);
        let prop = &props[selected_property];
        let toggled = match prop.value.as_str() {
            "true"  => Some("false"),
            "false" => Some("true"),
            _ => None,
        };
        if let Some(new_val) = toggled {
            match properties::set_property(&mut state.source.objects[object_index], prop.name, new_val) {
                Ok(()) => {
                    state.dirty = true;
                    state.status_message = Some(format!("Set {} = {}", prop.name, new_val));
                }
                Err(e) => state.status_message = Some(format!("Error: {e}")),
            }
            return Action::Redraw;
        }
    }

    if matches_binding(&bindings.confirm, &key) {
        let props = properties::get_properties(&state.source.objects, object_index);
        let prop = &props[selected_property];
        if let Some(opts) = properties::dropdown_options_for(&prop.kind) {
            // Open dropdown; pre-select the matching option if recognised
            let dd_sel = opts.iter().position(|&o| o == prop.value).unwrap_or(0);
            state.mode = Mode::EditProperties {
                object_index,
                selected_property,
                editing_value: None,
                cursor: 0,
                scroll: 0,
                panel_scroll: 0,
                dropdown: Some(dd_sel),
            };
        } else if prop.kind == properties::PropertyKind::Coordinate {
            if let Some(coord) = properties::get_coord(&state.source.objects[object_index], prop.name) {
                if let Coordinate::Animated { from, to, start_frame, end_frame } = &coord {
                    state.mode = Mode::AnimateProperty {
                        object_index,
                        return_property: selected_property,
                        property_name: prop.name,
                        selected_field: 0,
                        editing: None,
                        cursor: 0,
                        from: *from,
                        to: *to,
                        start_frame: *start_frame,
                        end_frame: *end_frame,
                    };
                } else {
                    state.mode = Mode::EditProperties {
                        object_index,
                        selected_property,
                        editing_value: Some(prop.value.clone()),
                        cursor: 0,
                        scroll: 0,
                        panel_scroll: 0,
                        dropdown: None,
                    };
                }
            }
        } else {
            state.mode = Mode::EditProperties {
                object_index,
                selected_property,
                editing_value: Some(prop.value.clone()),
                cursor: 0,
                scroll: 0,
                panel_scroll: 0,
                dropdown: None,
            };
        }
        return Action::Redraw;
    }

    // [a]nimate: open AnimateProperty panel for Coordinate properties
    if matches_binding(&bindings.animate, &key) {
        let props = properties::get_properties(&state.source.objects, object_index);
        let prop = &props[selected_property];
        if prop.kind == properties::PropertyKind::Coordinate {
            if let Some(coord) = properties::get_coord(&state.source.objects[object_index], prop.name) {
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
                state.mode = Mode::AnimateProperty {
                    object_index,
                    return_property: selected_property,
                    property_name: prop.name,
                    selected_field: 0,
                    editing: None,
                    cursor: 0,
                    from,
                    to,
                    start_frame,
                    end_frame,
                };
                return Action::Redraw;
            }
        }
    }

    Action::Continue
}

fn handle_edit_value(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, selected_property, editing_value, mut cursor, mut scroll, mut panel_scroll) =
        match &state.mode {
            Mode::EditProperties {
                object_index, selected_property, editing_value, cursor, scroll, panel_scroll, ..
            } => (*object_index, *selected_property, editing_value.clone(), *cursor, *scroll, *panel_scroll),
            _ => return Action::Continue,
        };

    let mut buf = editing_value.unwrap_or_default();

    let max_width = (super::ui::RIGHT_PANEL_WIDTH - 3) as usize;
    // prefix_len for line 0 ("propname: "); continuation lines have indent "  " (2 chars).
    let prefix0 = {
        let props = properties::get_properties(&state.source.objects, object_index);
        props[selected_property].name.chars().count() + 2
    };

    // Returns (line_index, col_within_that_line) for a char-index cursor.
    fn cursor_line_col(buf: &str, cursor: usize) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for (i, ch) in buf.chars().enumerate() {
            if i == cursor { break; }
            if ch == '\n' { line += 1; col = 0; } else { col += 1; }
        }
        (line, col)
    }

    fn line_col_to_cursor(buf: &str, target_line: usize, target_col: usize) -> usize {
        let mut line = 0usize;
        let mut col = 0usize;
        for (i, ch) in buf.chars().enumerate() {
            if line == target_line && col == target_col { return i; }
            if ch == '\n' {
                if line == target_line { return i; } // clamp: target_col beyond line length
                line += 1; col = 0;
            } else {
                col += 1;
            }
        }
        // cursor at end of buffer (target_col may be beyond last line's length)
        buf.chars().count()
    }

    // Update horizontal scroll for the cursor's current line.
    // Resets to 0 if the cursor crossed to a different line (old_line != new_line).
    let update_h_scroll = |new_cursor: usize, old_line: usize, old_scroll: usize, b: &str| -> usize {
        let (new_line, new_col) = cursor_line_col(b, new_cursor);
        let plen = if new_line == 0 { prefix0 } else { 2 };
        let horiz_w = max_width.saturating_sub(plen);
        let base = if new_line != old_line { 0 } else { old_scroll };
        if new_col < base { new_col }
        else if horiz_w > 0 && new_col >= base + horiz_w { new_col + 1 - horiz_w }
        else { base }
    };

    // Update panel scroll so the cursor's visual row stays visible.
    let update_panel_scroll = |new_cursor: usize, old_ps: usize, b: &str| -> usize {
        let (new_line, _) = cursor_line_col(b, new_cursor);
        let vis_row = selected_property + new_line;
        let term_h = terminal::size().map(|(_, h)| h).unwrap_or(24) as usize;
        let avail = term_h.saturating_sub(5); // menu:1 + timeline:2 + title:1 + sep:1
        if vis_row < old_ps { vis_row }
        else if avail > 0 && vis_row >= old_ps + avail { vis_row + 1 - avail }
        else { old_ps }
    };

    // Shorthand: build the editing Mode after a cursor/buf change.
    macro_rules! editing_mode {
        ($buf:expr, $cursor:expr, $scroll:expr, $ps:expr) => {
            Mode::EditProperties {
                object_index, selected_property,
                editing_value: Some($buf),
                cursor: $cursor, scroll: $scroll, panel_scroll: $ps,
                dropdown: None,
            }
        };
    }

    // insert_newline binding: insert a newline at the cursor position
    if matches_binding(&state.config.key_bindings.insert_newline, &key) {
        let (old_line, _) = cursor_line_col(&buf, cursor);
        let byte_idx = char_to_byte_idx(&buf, cursor);
        buf.insert(byte_idx, '\n');
        cursor += 1;
        scroll = update_h_scroll(cursor, old_line, scroll, &buf);
        panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
        state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
        return Action::Redraw;
    }

    match key.code {
        KeyCode::Enter => {
            // Apply the value
            let props = properties::get_properties(&state.source.objects, object_index);
            let prop_name = props[selected_property].name;
            match properties::set_property(
                &mut state.source.objects[object_index],
                prop_name,
                &buf,
            ) {
                Ok(()) => {
                    state.dirty = true;
                    state.status_message = Some(format!("Set {prop_name}"));
                }
                Err(e) => {
                    state.status_message = Some(format!("Error: {e}"));
                }
            }
            state.mode = Mode::EditProperties {
                object_index, selected_property,
                editing_value: None, cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
            };
            return Action::Redraw;
        }
        KeyCode::Esc => {
            // Cancel editing
            state.mode = Mode::EditProperties {
                object_index, selected_property,
                editing_value: None, cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
            };
            return Action::Redraw;
        }
        KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
            let (old_line, _) = cursor_line_col(&buf, cursor);
            cursor = cursor.saturating_sub(1);
            scroll = update_h_scroll(cursor, old_line, scroll, &buf);
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
            let (old_line, _) = cursor_line_col(&buf, cursor);
            cursor = (cursor + 1).min(buf.chars().count());
            scroll = update_h_scroll(cursor, old_line, scroll, &buf);
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
            let (line, col) = cursor_line_col(&buf, cursor);
            if line > 0 {
                cursor = line_col_to_cursor(&buf, line - 1, col);
                scroll = update_h_scroll(cursor, line, scroll, &buf);
                panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
                state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            }
            return Action::Redraw;
        }
        KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            let (line, col) = cursor_line_col(&buf, cursor);
            let line_count = buf.chars().filter(|&c| c == '\n').count() + 1;
            if line + 1 < line_count {
                cursor = line_col_to_cursor(&buf, line + 1, col);
                scroll = update_h_scroll(cursor, line, scroll, &buf);
                panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
                state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            }
            return Action::Redraw;
        }
        KeyCode::Home if key.modifiers == KeyModifiers::NONE => {
            let (line, _) = cursor_line_col(&buf, cursor);
            cursor = line_col_to_cursor(&buf, line, 0);
            scroll = 0;
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        KeyCode::End if key.modifiers == KeyModifiers::NONE => {
            let (line, _) = cursor_line_col(&buf, cursor);
            // Find length of the current line
            let line_len = buf.split('\n').nth(line).map(|s| s.chars().count()).unwrap_or(0);
            cursor = line_col_to_cursor(&buf, line, line_len);
            let (end_line, end_col) = cursor_line_col(&buf, cursor);
            let plen = if end_line == 0 { prefix0 } else { 2 };
            let horiz_w = max_width.saturating_sub(plen);
            scroll = end_col.saturating_sub(horiz_w.saturating_sub(1));
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        KeyCode::Delete if key.modifiers == KeyModifiers::NONE => {
            let char_count = buf.chars().count();
            if cursor < char_count {
                let start = char_to_byte_idx(&buf, cursor);
                let end = char_to_byte_idx(&buf, cursor + 1);
                buf.drain(start..end);
            }
            let (old_line, _) = cursor_line_col(&buf, cursor);
            scroll = update_h_scroll(cursor, old_line, scroll, &buf);
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        KeyCode::Backspace => {
            let (old_line, _) = cursor_line_col(&buf, cursor);
            if cursor > 0 {
                let start = char_to_byte_idx(&buf, cursor - 1);
                let end = char_to_byte_idx(&buf, cursor);
                buf.drain(start..end);
                cursor -= 1;
            }
            scroll = update_h_scroll(cursor, old_line, scroll, &buf);
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let (old_line, _) = cursor_line_col(&buf, cursor);
            let byte_idx = char_to_byte_idx(&buf, cursor);
            buf.insert(byte_idx, c);
            cursor += 1;
            scroll = update_h_scroll(cursor, old_line, scroll, &buf);
            panel_scroll = update_panel_scroll(cursor, panel_scroll, &buf);
            state.mode = editing_mode!(buf, cursor, scroll, panel_scroll);
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

fn handle_dropdown(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, selected_property, dd_sel) = match &state.mode {
        Mode::EditProperties { object_index, selected_property, dropdown: Some(sel), .. } => {
            (*object_index, *selected_property, *sel)
        }
        _ => return Action::Continue,
    };

    let props = properties::get_properties(&state.source.objects, object_index);
    let prop_kind = props[selected_property].kind.clone();
    let options = properties::dropdown_options_for(&prop_kind)
        .unwrap_or(properties::COLOR_OPTIONS);
    let opt_count = options.len();
    let sentinel = properties::dropdown_custom_sentinel(&prop_kind);

    // Navigate up/down through options
    if key.code == KeyCode::Up && key.modifiers == KeyModifiers::NONE {
        let new_sel = if dd_sel == 0 { opt_count - 1 } else { dd_sel - 1 };
        state.mode = Mode::EditProperties {
            object_index, selected_property,
            editing_value: None, cursor: 0,
            scroll: 0, panel_scroll: 0,
            dropdown: Some(new_sel),
        };
        return Action::Redraw;
    }
    if key.code == KeyCode::Down && key.modifiers == KeyModifiers::NONE {
        let new_sel = (dd_sel + 1) % opt_count;
        state.mode = Mode::EditProperties {
            object_index, selected_property,
            editing_value: None, cursor: 0,
            scroll: 0, panel_scroll: 0,
            dropdown: Some(new_sel),
        };
        return Action::Redraw;
    }

    let bindings = state.config.key_bindings.clone();

    if matches_binding(&bindings.confirm, &key) {
        let chosen = options[dd_sel];
        if chosen == sentinel {
            // Switch to text input; seed with current value if useful
            let current = &props[selected_property].value;
            let initial = if prop_kind == properties::PropertyKind::Color {
                if current.starts_with('#') { current.clone() } else { "#".to_string() }
            } else {
                // For char options: seed with current char, or empty if "auto"
                if current != "auto" { current.clone() } else { String::new() }
            };
            let cursor = initial.chars().count();
            state.mode = Mode::EditProperties {
                object_index, selected_property,
                editing_value: Some(initial), cursor,
                scroll: 0, panel_scroll: 0,
                dropdown: None,
            };
        } else {
            let prop_name = props[selected_property].name;
            match properties::set_property(&mut state.source.objects[object_index], prop_name, chosen) {
                Ok(()) => {
                    state.dirty = true;
                    state.status_message = Some(format!("Set {prop_name} = {chosen}"));
                }
                Err(e) => {
                    state.status_message = Some(format!("Error: {e}"));
                }
            }
            state.mode = Mode::EditProperties {
                object_index, selected_property,
                editing_value: None, cursor: 0,
                scroll: 0, panel_scroll: 0,
                dropdown: None,
            };
        }
        return Action::Redraw;
    }

    if key.code == KeyCode::Esc {
        state.mode = Mode::EditProperties {
            object_index, selected_property,
            editing_value: None, cursor: 0,
            scroll: 0, panel_scroll: 0,
            dropdown: None,
        };
        return Action::Redraw;
    }

    Action::Continue
}

fn handle_animate_property(state: &mut EditorState, key: KeyEvent) -> Action {
    let (object_index, return_property, property_name, selected_field, editing, cursor, from, to, start_frame, end_frame) =
        match &state.mode {
            Mode::AnimateProperty {
                object_index, return_property, property_name, selected_field,
                editing, cursor, from, to, start_frame, end_frame,
            } => (
                *object_index, *return_property, *property_name, *selected_field,
                editing.clone(), *cursor, *from, *to, *start_frame, *end_frame,
            ),
            _ => return Action::Continue,
        };

    // -- Editing a field value -------------------------------------------------
    if let Some(mut buf) = editing {
        match key.code {
            KeyCode::Enter => {
                let mut new_from = from;
                let mut new_to = to;
                let mut new_start = start_frame;
                let mut new_end = end_frame;
                let err: Option<String> = match selected_field {
                    0 => match buf.parse::<u16>() {
                        Ok(v) => { new_from = v; None }
                        Err(e) => Some(format!("Invalid number: {e}")),
                    },
                    1 => match buf.parse::<u16>() {
                        Ok(v) => { new_to = v; None }
                        Err(e) => Some(format!("Invalid number: {e}")),
                    },
                    2 => match buf.parse::<usize>() {
                        Ok(v) => { new_start = v; None }
                        Err(e) => Some(format!("Invalid number: {e}")),
                    },
                    3 => match buf.parse::<usize>() {
                        Ok(v) => { new_end = v; None }
                        Err(e) => Some(format!("Invalid number: {e}")),
                    },
                    _ => None,
                };
                if let Some(msg) = err {
                    state.status_message = Some(msg);
                }
                state.mode = Mode::AnimateProperty {
                    object_index, return_property, property_name, selected_field,
                    editing: None, cursor: 0,
                    from: new_from, to: new_to, start_frame: new_start, end_frame: new_end,
                };
                return Action::Redraw;
            }
            KeyCode::Esc => {
                state.mode = Mode::AnimateProperty {
                    object_index, return_property, property_name, selected_field,
                    editing: None, cursor: 0, from, to, start_frame, end_frame,
                };
                return Action::Redraw;
            }
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
                let new_cursor = cursor.saturating_sub(1);
                state.mode = Mode::AnimateProperty {
                    object_index, return_property, property_name, selected_field,
                    editing: Some(buf), cursor: new_cursor, from, to, start_frame, end_frame,
                };
                return Action::Redraw;
            }
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                let new_cursor = (cursor + 1).min(buf.chars().count());
                state.mode = Mode::AnimateProperty {
                    object_index, return_property, property_name, selected_field,
                    editing: Some(buf), cursor: new_cursor, from, to, start_frame, end_frame,
                };
                return Action::Redraw;
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    let start = char_to_byte_idx(&buf, cursor - 1);
                    let end = char_to_byte_idx(&buf, cursor);
                    buf.drain(start..end);
                    let new_cursor = cursor - 1;
                    state.mode = Mode::AnimateProperty {
                        object_index, return_property, property_name, selected_field,
                        editing: Some(buf), cursor: new_cursor, from, to, start_frame, end_frame,
                    };
                    return Action::Redraw;
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let byte_idx = char_to_byte_idx(&buf, cursor);
                buf.insert(byte_idx, c);
                let new_cursor = cursor + 1;
                state.mode = Mode::AnimateProperty {
                    object_index, return_property, property_name, selected_field,
                    editing: Some(buf), cursor: new_cursor, from, to, start_frame, end_frame,
                };
                return Action::Redraw;
            }
            _ => {}
        }
        return Action::Continue;
    }

    // -- Browsing fields -------------------------------------------------------
    match key.code {
        KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
            let new_sel = if selected_field == 0 { 3 } else { selected_field - 1 };
            state.mode = Mode::AnimateProperty {
                object_index, return_property, property_name,
                selected_field: new_sel, editing: None, cursor: 0,
                from, to, start_frame, end_frame,
            };
            return Action::Redraw;
        }
        KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
            let new_sel = (selected_field + 1) % 4;
            state.mode = Mode::AnimateProperty {
                object_index, return_property, property_name,
                selected_field: new_sel, editing: None, cursor: 0,
                from, to, start_frame, end_frame,
            };
            return Action::Redraw;
        }
        KeyCode::Enter => {
            // Start editing the selected field
            let init = match selected_field {
                0 => from.to_string(),
                1 => to.to_string(),
                2 => start_frame.to_string(),
                _ => end_frame.to_string(),
            };
            let new_cursor = init.chars().count();
            state.mode = Mode::AnimateProperty {
                object_index, return_property, property_name, selected_field,
                editing: Some(init), cursor: new_cursor,
                from, to, start_frame, end_frame,
            };
            return Action::Redraw;
        }
        // [s] apply → Animated coordinate
        KeyCode::Char('s') if key.modifiers == KeyModifiers::NONE => {
            let coord = if start_frame < end_frame {
                Coordinate::Animated { from, to, start_frame, end_frame }
            } else {
                Coordinate::Fixed(from as f64)
            };
            match properties::set_coordinate(&mut state.source.objects[object_index], property_name, coord) {
                Ok(()) => {
                    state.dirty = true;
                    // Extend the object's visibility range to cover the animation.
                    if start_frame < end_frame {
                        let fr = super::state::scene_object_frame_range_mut(
                            &mut state.source.objects[object_index],
                        );
                        if start_frame < fr.start {
                            fr.start = start_frame;
                        }
                        // frames.end is exclusive, so the object must be visible
                        // through end_frame inclusive.
                        if end_frame + 1 > fr.end {
                            fr.end = end_frame + 1;
                        }
                    }
                    state.status_message = Some(format!("Animated {property_name}"));
                }
                Err(e) => state.status_message = Some(format!("Error: {e}")),
            }
            state.mode = Mode::EditProperties {
                object_index, selected_property: return_property,
                editing_value: None, cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
            };
            return Action::Redraw;
        }
        // [x] clear → Fixed coordinate
        KeyCode::Char('x') if key.modifiers == KeyModifiers::NONE => {
            let coord = Coordinate::Fixed(from as f64);
            match properties::set_coordinate(&mut state.source.objects[object_index], property_name, coord) {
                Ok(()) => {
                    state.dirty = true;
                    state.status_message = Some(format!("Fixed {property_name} = {from}"));
                }
                Err(e) => state.status_message = Some(format!("Error: {e}")),
            }
            state.mode = Mode::EditProperties {
                object_index, selected_property: return_property,
                editing_value: None, cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
            };
            return Action::Redraw;
        }
        KeyCode::Esc => {
            state.mode = Mode::EditProperties {
                object_index, selected_property: return_property,
                editing_value: None, cursor: 0, scroll: 0, panel_scroll: 0, dropdown: None,
            };
            return Action::Redraw;
        }
        _ => {}
    }

    Action::Continue
}

/// Convert a char index into a byte index for string operations.
fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(s.len())
}
