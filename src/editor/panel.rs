use std::io;

use crossterm::{cursor, queue, style};

use super::object_defaults;
use super::properties::{self, PropertyKind};
use super::state::{scene_object_summary, scene_object_type_name, EditorState, Mode};
use super::ui::Layout;

pub fn render_right_panel(
    stdout: &mut io::Stdout,
    layout: &Layout,
    state: &EditorState,
) -> anyhow::Result<()> {
    if layout.right_panel_width == 0 {
        return Ok(());
    }

    let panel_x = layout.canvas_x + layout.canvas_width;
    let pw = layout.right_panel_width;
    let cy = layout.canvas_y;
    let max_width = (pw - 3) as usize;

    // Helper: draw the shared vertical border + title + separator
    let draw_header = |stdout: &mut io::Stdout, title: &str| -> anyhow::Result<()> {
        for y in 0..layout.canvas_height {
            queue!(stdout, cursor::MoveTo(panel_x, cy + y), style::Print("\u{2502}"))?;
        }
        queue!(
            stdout,
            cursor::MoveTo(panel_x + 2, cy),
            style::SetAttribute(style::Attribute::Bold),
            style::Print(title),
            style::SetAttribute(style::Attribute::Reset),
        )?;
        queue!(stdout, cursor::MoveTo(panel_x, cy + 1), style::Print("\u{253c}"))?;
        for _ in 1..pw {
            queue!(stdout, style::Print("\u{2500}"))?;
        }
        Ok(())
    };

    // === AddObject ===
    if let Mode::AddObject { selected } = &state.mode {
        let selected = *selected;
        draw_header(stdout, "Add Object")?;
        let types = object_defaults::OBJECT_TYPES;
        for (i, name) in types.iter().enumerate() {
            let y = cy + (i + 2) as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if i == selected {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("> {:<width$}", name, width = max_width.saturating_sub(2))),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(
                    stdout,
                    style::Print(format!("  {:<width$}", name, width = max_width.saturating_sub(2))),
                )?;
            }
        }
        return Ok(());
    }

    // === SelectObject ===
    if let Mode::SelectObject { selected } = &state.mode {
        let selected = *selected;
        draw_header(stdout, "Select Object")?;
        let visible = state.objects_on_current_frame();
        for (i, &obj_idx) in visible.iter().enumerate() {
            let y = cy + (i + 2) as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            let obj = &state.source.objects[obj_idx];
            let summary = scene_object_summary(obj);
            let summary: String = summary.chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if i == selected {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("{:<width$}", summary, width = max_width)),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(
                    stdout,
                    style::Print(format!("{:<width$}", summary, width = max_width)),
                )?;
            }
        }
        return Ok(());
    }

    // === Confirm ===
    if let Mode::Confirm { message, selected, .. } = &state.mode {
        let message = message.as_str();
        let selected = *selected;
        draw_header(stdout, "Confirm")?;
        // Message row (dimmed)
        if cy + 2 < cy + layout.canvas_height {
            let msg: String = message.chars().take(max_width).collect();
            queue!(
                stdout,
                cursor::MoveTo(panel_x + 2, cy + 2),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(msg),
                style::SetAttribute(style::Attribute::Reset),
            )?;
        }
        // Yes / No
        let labels = ["Yes", "No"];
        for (i, label) in labels.iter().enumerate() {
            let y = cy + (i + 3) as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if i == selected {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("{:<width$}", label, width = max_width)),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(
                    stdout,
                    style::Print(format!("{:<width$}", label, width = max_width)),
                )?;
            }
        }
        return Ok(());
    }

    // === SelectGroupMembers ===
    if let Mode::SelectGroupMembers { selected, members } = &state.mode {
        let selected = *selected;
        draw_header(stdout, "Add Group")?;
        for (i, obj) in state.source.objects.iter().enumerate() {
            let y = cy + (i + 2) as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            let is_member = members.contains(&i);
            let check = if is_member { "[+]" } else { "[ ]" };
            let summary = scene_object_summary(obj);
            let text: String = format!("{} {}", check, summary)
                .chars()
                .take(max_width)
                .collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if i == selected {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("{:<width$}", text, width = max_width)),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(stdout, style::Print(format!("{:<width$}", text, width = max_width)))?;
            }
        }
        return Ok(());
    }

    // AnimateProperty panel
    if matches!(state.mode, Mode::AnimateProperty { .. }) {
        let (property_name, selected_field, editing, cursor, from, to, start_frame, end_frame) =
            match &state.mode {
                Mode::AnimateProperty {
                    property_name, selected_field, editing, cursor,
                    from, to, start_frame, end_frame, ..
                } => (*property_name, *selected_field, editing, *cursor, *from, *to, *start_frame, *end_frame),
                _ => unreachable!(),
            };

        let title = format!("Animate: {property_name}");
        let title: String = title.chars().take((pw - 2) as usize).collect();
        draw_header(stdout, &title)?;

        let field_names = ["from", "to", "start", "end"];
        let field_values = [
            from.to_string(),
            to.to_string(),
            start_frame.to_string(),
            end_frame.to_string(),
        ];

        for i in 0..4usize {
            let y = cy + (i as u16 + 2);
            if y >= cy + layout.canvas_height {
                break;
            }
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;

            let display: String = if i == selected_field {
                if let Some(buf) = editing {
                    let cur = cursor.min(buf.chars().count());
                    let before: String = buf.chars().take(cur).collect();
                    let after: String = buf.chars().skip(cur).collect();
                    format!("{}: {}\u{2588}{}", field_names[i], before, after)
                        .chars()
                        .take(max_width)
                        .collect()
                } else {
                    format!("{}: {}", field_names[i], field_values[i])
                        .chars()
                        .take(max_width)
                        .collect()
                }
            } else {
                format!("{}: {}", field_names[i], field_values[i])
                    .chars()
                    .take(max_width)
                    .collect()
            };

            if i == selected_field {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("{:<width$}", display, width = max_width)),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(stdout, style::Print(display))?;
            }
        }

        // Hint row
        let hint_y = cy + 6;
        if hint_y < cy + layout.canvas_height {
            queue!(stdout, cursor::MoveTo(panel_x + 2, hint_y))?;
            let hint: String = format!("[s]save  [x]\u{2192}fixed")
                .chars()
                .take(max_width)
                .collect();
            queue!(
                stdout,
                style::SetAttribute(style::Attribute::Dim),
                style::Print(hint),
                style::SetAttribute(style::Attribute::Reset),
            )?;
        }

        return Ok(());
    }

    let (object_index, selected_prop, editing, cursor, scroll, panel_scroll, dropdown) = match &state.mode {
        Mode::EditProperties {
            object_index,
            selected_property,
            editing_value,
            cursor,
            scroll,
            panel_scroll,
            dropdown,
        } => (*object_index, *selected_property, editing_value, *cursor, *scroll, *panel_scroll, *dropdown),
        _ => return Ok(()),
    };

    let obj = &state.source.objects[object_index];
    draw_header(stdout, scene_object_type_name(obj))?;

    // Render \n as ↵ in any value for display purposes (non-editing rows).
    let fmt_val = |v: &str| -> String {
        v.chars().map(|c| if c == '\n' { '↵' } else { c }).collect()
    };

    // Returns (line_index, col_within_that_line) for a char-index cursor.
    let cursor_line_col = |buf: &str, cur: usize| -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for (i, ch) in buf.chars().enumerate() {
            if i == cur { break; }
            if ch == '\n' { line += 1; col = 0; } else { col += 1; }
        }
        (line, col)
    };

    // Properties
    let props = properties::get_properties(&state.source.objects, object_index);

    // screen_y: next terminal row to paint (starts after title row + separator).
    // visual_row: logical property row index (before panel_scroll is applied).
    let mut screen_y: u16 = cy + 2;
    let mut visual_row: usize = 0;
    // Track where the selected property was rendered (for dropdown placement).
    let mut selected_screen_y: Option<u16> = None;

    'props: for (i, prop) in props.iter().enumerate() {
        if screen_y >= cy + layout.canvas_height {
            break;
        }

        if i == selected_prop {
            if let Some(buf) = editing {
                // ── Multi-line editing: each \n-delimited segment on its own row ──
                let (cursor_line_idx, cursor_col_in_line) = cursor_line_col(buf, cursor);

                for (line_idx, line_text) in buf.split('\n').enumerate() {
                    if visual_row >= panel_scroll {
                        if screen_y >= cy + layout.canvas_height {
                            break 'props;
                        }
                        let prefix: String = if line_idx == 0 {
                            format!("{}: ", prop.name)
                        } else {
                            "  ".to_string()
                        };
                        let prefix_len = prefix.chars().count();
                        let horiz_w = max_width.saturating_sub(prefix_len);
                        // Horizontal scroll only on the cursor's line; other lines start at 0.
                        let line_scroll = if line_idx == cursor_line_idx { scroll } else { 0 };

                        let display_line: String = line_text.chars()
                            .chain(std::iter::repeat(' '))
                            .skip(line_scroll)
                            .take(horiz_w)
                            .collect();

                        queue!(stdout, cursor::MoveTo(panel_x + 2, screen_y))?;

                        if line_idx == cursor_line_idx {
                            let cursor_pos = cursor_col_in_line.saturating_sub(line_scroll);
                            let before: String = display_line.chars().take(cursor_pos).collect();
                            let cursor_ch = display_line.chars().nth(cursor_pos).unwrap_or(' ');
                            let after: String = display_line.chars().skip(cursor_pos + 1).collect();
                            queue!(
                                stdout,
                                style::SetAttribute(style::Attribute::Reverse),
                                style::Print(format!("{}{}", prefix, before)),
                                style::SetAttribute(style::Attribute::Reset),
                                style::SetAttribute(style::Attribute::Bold),
                                style::Print(cursor_ch),
                                style::SetAttribute(style::Attribute::Reset),
                                style::SetAttribute(style::Attribute::Reverse),
                                style::Print(&after),
                                style::SetAttribute(style::Attribute::Reset),
                            )?;
                        } else {
                            queue!(
                                stdout,
                                style::SetAttribute(style::Attribute::Reverse),
                                style::Print(format!("{}{}", prefix, display_line)),
                                style::SetAttribute(style::Attribute::Reset),
                            )?;
                        }

                        if line_idx == 0 { selected_screen_y = Some(screen_y); }
                        screen_y += 1;
                    }
                    visual_row += 1;
                }
                continue 'props; // visual_row already advanced in the inner loop
            }
        }

        // ── Single-row path: selected-not-editing, or any non-selected prop ──
        if visual_row >= panel_scroll && screen_y < cy + layout.canvas_height {
            queue!(stdout, cursor::MoveTo(panel_x + 2, screen_y))?;

            // GroupMember rows show the member summary rather than a raw index.
            let fmt_prop_display = |prop: &properties::Property| -> String {
                if prop.kind == PropertyKind::GroupMember {
                    let obj_idx: usize = prop.value.parse().unwrap_or(usize::MAX);
                    let summary = if obj_idx < state.source.objects.len() {
                        scene_object_summary(&state.source.objects[obj_idx])
                    } else {
                        "?".to_string()
                    };
                    format!("[Del] {}", summary).chars().take(max_width).collect()
                } else {
                    format!("{}: {}", prop.name, fmt_val(&prop.value))
                        .chars().take(max_width).collect()
                }
            };

            if i == selected_prop {
                if prop.kind == PropertyKind::ReadOnly {
                    // ReadOnly: show selected but dimmed (not editable)
                    let display: String = fmt_prop_display(prop);
                    queue!(
                        stdout,
                        style::SetAttribute(style::Attribute::Reverse),
                        style::SetAttribute(style::Attribute::Dim),
                        style::Print(format!("{:<width$}", display, width = max_width)),
                        style::SetAttribute(style::Attribute::Reset),
                    )?;
                } else {
                    let display: String = if dropdown.is_some() {
                        format!("{}: \u{25bc} {}", prop.name, fmt_val(&prop.value))
                            .chars().take(max_width).collect()
                    } else {
                        fmt_prop_display(prop)
                    };
                    queue!(
                        stdout,
                        style::SetAttribute(style::Attribute::Reverse),
                        style::Print(format!("{:<width$}", display, width = max_width)),
                        style::SetAttribute(style::Attribute::Reset),
                    )?;
                }
                selected_screen_y = Some(screen_y);
            } else if prop.kind == PropertyKind::ReadOnly {
                // Non-selected ReadOnly: always dimmed
                let display = fmt_prop_display(prop);
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Dim),
                    style::Print(display),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                let display = fmt_prop_display(prop);
                queue!(stdout, style::Print(display))?;
            }

            screen_y += 1;
        }
        visual_row += 1;
    }

    // Dropdown overlay
    if let Some(dd_sel) = dropdown {
        let prop_kind = &props[selected_prop].kind;
        let options = properties::dropdown_options_for(prop_kind)
            .unwrap_or(properties::COLOR_OPTIONS);
        let dd_start_y = selected_screen_y
            .map(|y| y + 1)
            .unwrap_or(cy + (selected_prop + 3) as u16);
        for (i, opt) in options.iter().enumerate() {
            let y = dd_start_y + i as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            let marker = if i == dd_sel { ">" } else { " " };
            let line: String = format!("{} {}", marker, opt)
                .chars()
                .chain(std::iter::repeat(' '))
                .take(max_width)
                .collect();
            if i == dd_sel {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(line),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(stdout, style::Print(line))?;
            }
        }
    }

    Ok(())
}

