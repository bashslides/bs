use std::io;

use crossterm::{cursor, queue, style};

use crate::engine::source::SceneObject;
use super::object_defaults;
use super::properties::{self, PropertyKind};
use super::state::{scene_object_summary, scene_object_type_name, ArtPick, EditorState, Mode, MultiSelectPurpose, TableCellSubState};
use super::ui::Layout;

/// If `value` names a concrete colour (named or `#rrggbb`), paint a two-cell
/// swatch in that colour at `(x, y)`. No-op for `none`/`auto`/sentinels.
fn draw_color_swatch(stdout: &mut io::Stdout, x: u16, y: u16, value: &str) -> anyhow::Result<()> {
    if let Ok(Some(color)) = properties::parse_opt_color_pub(value) {
        let ct = crate::player::to_ct_color(&color);
        let cs = style::ContentStyle { background_color: Some(ct), ..Default::default() };
        queue!(
            stdout,
            cursor::MoveTo(x, y),
            style::PrintStyledContent(style::StyledContent::new(cs, ' ')),
            style::PrintStyledContent(style::StyledContent::new(cs, ' ')),
        )?;
    }
    Ok(())
}

/// Draw one already-laid-out text line at `(x, y)`, rendering exactly `width`
/// cells (padding short lines with spaces, ignoring overflow). This is the
/// single place every editor text field paints its caret, so the convention
/// can't drift between widgets:
///
/// * `reverse` paints the whole line in reverse video — the "this field/row is
///   active" affordance, the same meaning reverse carries for selected list
///   rows.
/// * `caret` is the column of the **insertion point**: the gap *before* the
///   character at that column. It is marked with an underline, never a block or
///   an inserted glyph, so it reads as "the next character lands here, pushing
///   the rest right" rather than as a selected or overwritten character. Pass
///   the text's char count for the append slot at the very end; pass `None` for
///   no caret.
///
/// The two attributes are orthogonal and compose, so an active field still
/// shows its caret. `display` is expected to be pre-composed by the caller
/// (prefixes, horizontal scroll already applied); this helper only rasterizes.
fn draw_caret_line(
    stdout: &mut io::Stdout,
    x: u16,
    y: u16,
    display: &str,
    caret: Option<usize>,
    reverse: bool,
    width: usize,
) -> anyhow::Result<()> {
    let chars: Vec<char> = display.chars().collect();
    queue!(stdout, cursor::MoveTo(x, y))?;
    for col in 0..width {
        let ch = chars.get(col).copied().unwrap_or(' ');
        let is_caret = caret == Some(col);
        if reverse {
            queue!(stdout, style::SetAttribute(style::Attribute::Reverse))?;
        }
        if is_caret {
            queue!(stdout, style::SetAttribute(style::Attribute::Underlined))?;
        }
        queue!(stdout, style::Print(ch))?;
        if reverse || is_caret {
            queue!(stdout, style::SetAttribute(style::Attribute::Reset))?;
        }
    }
    Ok(())
}

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
            // Prefix each type with its quick-add shortcut, e.g. "[l] Label".
            let key = object_defaults::OBJECT_TYPE_KEYS.get(i).copied().unwrap_or(' ');
            let label = format!("[{key}] {name}");
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if i == selected {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("> {:<width$}", label, width = max_width.saturating_sub(2))),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(
                    stdout,
                    style::Print(format!("  {:<width$}", label, width = max_width.saturating_sub(2))),
                )?;
            }
        }
        return Ok(());
    }

    // === AddArt ===
    if let Mode::AddArt { selected, items, purpose } = &state.mode {
        let selected = *selected;
        let title = match purpose {
            ArtPick::MorphFrom => "Morph: pick start art",
            ArtPick::MorphTo { .. } => "Morph: pick target art",
            ArtPick::Art => "Add Art",
        };
        draw_header(stdout, title)?;
        // One row per library piece, then a final "Load from file…" action.
        let mut labels: Vec<String> = items.iter().map(|it| it.name.clone()).collect();
        labels.push("Load from file…".to_string());
        for (i, name) in labels.iter().enumerate() {
            let y = cy + (i + 2) as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            let text: String = name.chars().take(max_width.saturating_sub(2)).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if i == selected {
                queue!(
                    stdout,
                    style::SetAttribute(style::Attribute::Reverse),
                    style::Print(format!("> {:<width$}", text, width = max_width.saturating_sub(2))),
                    style::SetAttribute(style::Attribute::Reset),
                )?;
            } else {
                queue!(
                    stdout,
                    style::Print(format!("  {:<width$}", text, width = max_width.saturating_sub(2))),
                )?;
            }
        }
        return Ok(());
    }

    // === LoadArtFile ===
    if let Mode::LoadArtFile { buf, cursor, .. } = &state.mode {
        let cursor = *cursor;
        draw_header(stdout, "Load Art File")?;
        if cy + 2 < cy + layout.canvas_height {
            let instr: String = "Path to art file:".chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 2),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(instr),
                style::SetAttribute(style::Attribute::Reset))?;
        }
        if cy + 3 < cy + layout.canvas_height {
            let caret = cursor.min(buf.chars().count());
            draw_caret_line(stdout, panel_x + 2, cy + 3, buf, Some(caret), true, max_width)?;
        }
        if cy + 5 < cy + layout.canvas_height {
            let hint: String = "Enter = load   Esc = back".chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 5),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(hint),
                style::SetAttribute(style::Attribute::Reset))?;
        }
        return Ok(());
    }

    // === Settings (frame size) ===
    if let Mode::Settings { selected_field, width_buf, height_buf, cursor } = &state.mode {
        draw_header(stdout, "Frame Size")?;
        if cy + 2 < cy + layout.canvas_height {
            let instr: String = "Output size (cells):".chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 2),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(instr),
                style::SetAttribute(style::Attribute::Reset))?;
        }

        let fields = [("width", width_buf), ("height", height_buf)];
        for (i, (name, buf)) in fields.iter().enumerate() {
            let y = cy + 4 + i as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            let selected = *selected_field == i;
            let marker = if selected { "\u{203a} " } else { "  " };
            let prefix = format!("{marker}{name:>6}: ");
            queue!(stdout, cursor::MoveTo(panel_x + 2, y), style::Print(&prefix))?;

            let vx = panel_x + 2 + prefix.chars().count() as u16;
            if selected {
                // Active field: underline caret marks where the next digit lands.
                let cur = (*cursor).min(buf.chars().count());
                let field_w = max_width.saturating_sub(prefix.chars().count()).max(1);
                draw_caret_line(stdout, vx, y, buf, Some(cur), false, field_w)?;
            } else {
                let val: String = buf.chars().take(max_width).collect();
                queue!(stdout, cursor::MoveTo(vx, y), style::Print(val))?;
            }
        }

        if cy + 7 < cy + layout.canvas_height {
            let hint: String = "Enter = apply   Esc = cancel".chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 7),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(hint),
                style::SetAttribute(style::Attribute::Reset))?;
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

    // === MultiSelect (group / copy) ===
    if let Mode::MultiSelect { purpose, selected, members } = &state.mode {
        let selected = *selected;
        draw_header(stdout, match purpose {
            MultiSelectPurpose::Group => "Add Group",
            MultiSelectPurpose::Copy => "Copy Objects",
        })?;
        // Only the current slide's objects are groupable; `selected` and the
        // [+]/[ ] markers are keyed off the real object index in `visible`.
        let visible = state.objects_on_current_frame();
        for (vi, &obj_idx) in visible.iter().enumerate() {
            let y = cy + (vi + 2) as u16;
            if y >= cy + layout.canvas_height {
                break;
            }
            let obj = &state.source.objects[obj_idx];
            let is_member = members.contains(&obj_idx);
            let check = if is_member { "[+]" } else { "[ ]" };
            let summary = scene_object_summary(obj);
            let text: String = format!("{} {}", check, summary)
                .chars()
                .take(max_width)
                .collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
            if vi == selected {
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
        let (property_name, selected_field, editing, cursor, from, to, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames) =
            match &state.mode {
                Mode::AnimateProperty {
                    property_name, selected_field, editing, cursor,
                    from, to, start_frame, end_frame, add_frames, auto_play, delay_ms, gap_frames, ..
                } => (*property_name, *selected_field, editing, *cursor, *from, *to, *start_frame, *end_frame, *add_frames, *auto_play, *delay_ms, *gap_frames),
                _ => unreachable!(),
            };

        let title = format!("Animate: {property_name}");
        let title: String = title.chars().take((pw - 2) as usize).collect();
        draw_header(stdout, &title)?;

        let checkbox = |b: bool| if b { "[x]" } else { "[ ]" };
        let field_names = ["from", "to", "start", "end", "add frames", "auto play", "delay ms", "gap frames"];
        // `start`/`end` are shown 1-based to match the property panel's
        // first_frame/last_frame (an animation through 0-based `end_frame` reads
        // as slide `end_frame + 1`). `from`/`to` are coordinate values, shown raw.
        // `add frames`/`auto play` render as checkboxes (toggled, never typed).
        let field_values = [
            from.to_string(),
            to.to_string(),
            (start_frame + 1).to_string(),
            (end_frame + 1).to_string(),
            checkbox(add_frames).to_string(),
            checkbox(auto_play).to_string(),
            delay_ms.to_string(),
            gap_frames.to_string(),
        ];

        for i in 0..field_names.len() {
            let y = cy + (i as u16 + 2);
            if y >= cy + layout.canvas_height {
                break;
            }
            let (display, caret): (String, Option<usize>) = if i == selected_field {
                if let Some(buf) = editing {
                    let cur = cursor.min(buf.chars().count());
                    let prefix = format!("{}: ", field_names[i]);
                    let plen = prefix.chars().count();
                    let display: String =
                        format!("{prefix}{buf}").chars().take(max_width).collect();
                    (display, Some(plen + cur))
                } else {
                    let display: String = format!("{}: {}", field_names[i], field_values[i])
                        .chars().take(max_width).collect();
                    (display, None)
                }
            } else {
                let display: String = format!("{}: {}", field_names[i], field_values[i])
                    .chars().take(max_width).collect();
                (display, None)
            };

            if i == selected_field {
                draw_caret_line(stdout, panel_x + 2, y, &display, caret, true, max_width)?;
            } else {
                queue!(stdout, cursor::MoveTo(panel_x + 2, y), style::Print(display))?;
            }
        }

        // Hint row, just below the field list.
        let hint_y = cy + 2 + field_names.len() as u16;
        if hint_y < cy + layout.canvas_height {
            queue!(stdout, cursor::MoveTo(panel_x + 2, hint_y))?;
            let hint: String = "[Space]toggle [s]save [x]\u{2192}fixed"
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

    // === TableAddColumn ===
    if let Mode::TableAddColumn { object_index, after, col_num, buf, cursor } = &state.mode {
        let (object_index, after, col_num, buf, cursor) = (*object_index, *after, *col_num, buf, *cursor);
        let direction = if after { "after" } else { "before" };
        let title = format!("Add Col {}", if after { "After" } else { "Before" });
        draw_header(stdout, &title)?;

        let ncols = match state.source.objects.get(object_index) {
            Some(SceneObject::Table(t)) => t.col_widths.len(),
            _ => 0,
        };

        // Instruction
        if cy + 2 < cy + layout.canvas_height {
            let instr = format!("Col {} (1–{}):", direction, ncols);
            let instr: String = instr.chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 2),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(instr),
                style::SetAttribute(style::Attribute::Reset))?;
        }
        // Value field
        if cy + 3 < cy + layout.canvas_height {
            let caret = cursor.min(buf.chars().count());
            draw_caret_line(stdout, panel_x + 2, cy + 3, buf, Some(caret), true, max_width)?;
        }
        // Column list
        if let Some(SceneObject::Table(t)) = state.source.objects.get(object_index) {
            for (i, w) in t.col_widths.iter().enumerate() {
                let y = cy + (i + 4) as u16;
                if y >= cy + layout.canvas_height { break; }
                let marker = if i + 1 == col_num { ">" } else { " " };
                let line = format!("{} col {}: {:.1}%", marker, i + 1, w * 100.0);
                let line: String = line.chars().take(max_width).collect();
                queue!(stdout, cursor::MoveTo(panel_x + 2, y), style::Print(line))?;
            }
        }
        return Ok(());
    }

    // === TableRemoveColumn ===
    if let Mode::TableRemoveColumn { object_index, col_num, buf, cursor } = &state.mode {
        let (object_index, col_num, buf, cursor) = (*object_index, *col_num, buf, *cursor);
        draw_header(stdout, "Remove Column")?;

        let ncols = match state.source.objects.get(object_index) {
            Some(SceneObject::Table(t)) => t.col_widths.len(),
            _ => 0,
        };

        if cy + 2 < cy + layout.canvas_height {
            let instr = format!("Column (1–{}):", ncols);
            let instr: String = instr.chars().take(max_width).collect();
            queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 2),
                style::SetAttribute(style::Attribute::Dim),
                style::Print(instr),
                style::SetAttribute(style::Attribute::Reset))?;
        }
        if cy + 3 < cy + layout.canvas_height {
            let caret = cursor.min(buf.chars().count());
            draw_caret_line(stdout, panel_x + 2, cy + 3, buf, Some(caret), true, max_width)?;
        }
        if let Some(SceneObject::Table(t)) = state.source.objects.get(object_index) {
            for (i, w) in t.col_widths.iter().enumerate() {
                let y = cy + (i + 4) as u16;
                if y >= cy + layout.canvas_height { break; }
                let highlighted = i + 1 == col_num;
                let marker = if highlighted { ">" } else { " " };
                let line = format!("{} col {}: {:.1}%", marker, i + 1, w * 100.0);
                let line: String = line.chars().take(max_width).collect();
                queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
                if highlighted {
                    queue!(stdout,
                        style::SetAttribute(style::Attribute::Reverse),
                        style::Print(format!("{:<width$}", line, width = max_width)),
                        style::SetAttribute(style::Attribute::Reset))?;
                } else {
                    queue!(stdout, style::Print(line))?;
                }
            }
        }
        return Ok(());
    }

    // === TableEditCellProps ===
    if let Mode::TableEditCellProps { object_index, cursor_row, cursor_col, selected_cells, sub_state } = &state.mode {
        let (object_index, cursor_row, cursor_col) = (*object_index, *cursor_row, *cursor_col);

        match sub_state {
            TableCellSubState::Selecting => {
                draw_header(stdout, "Edit Cells")?;
                if cy + 2 < cy + layout.canvas_height {
                    let hint = format!("Cur: ({},{})", cursor_row + 1, cursor_col + 1);
                    queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 2),
                        style::Print(hint.chars().take(max_width).collect::<String>()))?;
                }
                if cy + 3 < cy + layout.canvas_height {
                    let sel_str = if selected_cells.is_empty() {
                        "None selected".to_string()
                    } else {
                        format!("{} selected", selected_cells.len())
                    };
                    queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 3),
                        style::SetAttribute(style::Attribute::Dim),
                        style::Print(sel_str.chars().take(max_width).collect::<String>()),
                        style::SetAttribute(style::Attribute::Reset))?;
                }
                // Hints
                let hints = [
                    "↑↓←→ navigate",
                    "Space: toggle sel",
                    "Enter: edit cell",
                    "l: add list",
                    "s: cell style",
                    "Esc: back",
                ];
                for (i, hint) in hints.iter().enumerate() {
                    let y = cy + (i + 4) as u16;
                    if y >= cy + layout.canvas_height { break; }
                    queue!(stdout, cursor::MoveTo(panel_x + 2, y),
                        style::SetAttribute(style::Attribute::Dim),
                        style::Print(hint.chars().take(max_width).collect::<String>()),
                        style::SetAttribute(style::Attribute::Reset))?;
                }
            }
            TableCellSubState::EditingContent { row, col, buf, cursor } => {
                let title = format!("Cell ({},{})", row + 1, col + 1);
                draw_header(stdout, &title)?;
                if cy + 2 < cy + layout.canvas_height {
                    let hint: String = "Shift+Enter=newline, Enter=save"
                        .chars().take(max_width).collect();
                    queue!(stdout, cursor::MoveTo(panel_x + 2, cy + 2),
                        style::SetAttribute(style::Attribute::Dim),
                        style::Print(hint),
                        style::SetAttribute(style::Attribute::Reset))?;
                }
                // Show content with the underline insertion caret on the line
                // that holds it (`base..=base+line_len` covers that line's text
                // plus the append slot just past its end / the newline boundary).
                let cursor_pos = (*cursor).min(buf.chars().count());
                let mut base = 0usize;
                let mut screen_y = cy + 3u16;
                for line in buf.split('\n') {
                    if screen_y >= cy + layout.canvas_height { break; }
                    let line_len = line.chars().count();
                    let caret = if cursor_pos >= base && cursor_pos <= base + line_len {
                        Some(cursor_pos - base)
                    } else {
                        None
                    };
                    draw_caret_line(stdout, panel_x + 2, screen_y, line, caret, false, max_width)?;
                    base += line_len + 1; // +1 for the newline
                    screen_y += 1;
                }
            }
            TableCellSubState::EditingStyle { selected_prop, editing_value, cursor, dropdown } => {
                let (selected_prop, cursor) = (*selected_prop, *cursor);
                // Show style props: fg_color, bg_color, bold, dimmed
                let target = if selected_cells.is_empty() {
                    (cursor_row, cursor_col)
                } else {
                    *selected_cells.first().unwrap_or(&(cursor_row, cursor_col))
                };
                let base_style = match state.source.objects.get(object_index) {
                    Some(SceneObject::Table(t)) => t.cells.get(target.0)
                        .and_then(|r| r.get(target.1))
                        .and_then(|c| c.style.as_ref())
                        .cloned()
                        .unwrap_or_default(),
                    _ => crate::types::Style::default(),
                };

                let title = if selected_cells.is_empty() {
                    format!("Cell Style ({},{})", target.0 + 1, target.1 + 1)
                } else {
                    format!("Cell Style ({} sel)", selected_cells.len())
                };
                draw_header(stdout, &title)?;

                let prop_values = [
                    properties::format_opt_color_pub(&base_style.fg),
                    properties::format_opt_color_pub(&base_style.bg),
                    base_style.bold.to_string(),
                    base_style.dim.to_string(),
                ];
                // bold/dimmed render as a checkbox; colours as "name: value".
                let label = |name: &str, value: &str| -> String {
                    if name == "bold" || name == "dimmed" {
                        let mark = if value.trim() == "true" { "x" } else { " " };
                        format!("[{}] {}", mark, name)
                    } else {
                        format!("{}: {}", name, value)
                    }
                };
                let mut sel_screen_y = None;
                for (i, &name) in properties::CELL_STYLE_PROPS.iter().enumerate() {
                    let y = cy + (i + 2) as u16;
                    if y >= cy + layout.canvas_height { break; }
                    queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;

                    let (val, caret): (String, Option<usize>) = if i == selected_prop {
                        if let Some(ev) = editing_value {
                            let cur = cursor.min(ev.chars().count());
                            let prefix = format!("{name}: ");
                            let plen = prefix.chars().count();
                            (format!("{prefix}{ev}"), Some(plen + cur))
                        } else if dropdown.is_some() {
                            (format!("{}: \u{25bc} {}", name, prop_values[i]), None)
                        } else {
                            (label(name, &prop_values[i]), None)
                        }
                    } else {
                        (label(name, &prop_values[i]), None)
                    };
                    let display: String = val.chars().take(max_width).collect();
                    if i == selected_prop {
                        draw_caret_line(stdout, panel_x + 2, y, &display, caret, true, max_width)?;
                        sel_screen_y = Some(y);
                    } else {
                        queue!(stdout, style::Print(display))?;
                    }
                    // Swatch for the fg/bg colour rows (not while editing the hex).
                    if (name == "fg_color" || name == "bg_color")
                        && !(i == selected_prop && editing_value.is_some())
                    {
                        let sx = panel_x + 2 + (max_width as u16).saturating_sub(2);
                        draw_color_swatch(stdout, sx, y, &prop_values[i])?;
                    }
                }

                // Dropdown overlay
                if let Some(dd_sel) = dropdown {
                    let opts = properties::COLOR_OPTIONS;
                    let dd_start = sel_screen_y.map(|y| y + 1).unwrap_or(cy + 6);
                    for (i, opt) in opts.iter().enumerate() {
                        let y = dd_start + i as u16;
                        if y >= cy + layout.canvas_height { break; }
                        queue!(stdout, cursor::MoveTo(panel_x + 2, y))?;
                        let marker = if i == *dd_sel { ">" } else { " " };
                        let line: String = format!("{} {}", marker, opt).chars()
                            .chain(std::iter::repeat(' ')).take(max_width).collect();
                        if i == *dd_sel {
                            queue!(stdout,
                                style::SetAttribute(style::Attribute::Reverse),
                                style::Print(line),
                                style::SetAttribute(style::Attribute::Reset))?;
                        } else {
                            queue!(stdout, style::Print(line))?;
                        }
                        draw_color_swatch(stdout, panel_x + 2 + (max_width as u16).saturating_sub(2), y, opt)?;
                    }
                }
            }
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
            // `Text` values are edited in the centred overlay (drawn separately),
            // so the panel just shows a preview row for them; other editable
            // kinds (number/coordinate/colour/char) edit inline here.
            if let Some(buf) = editing.as_ref().filter(|_| prop.kind != PropertyKind::Text) {
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

                        let display = format!("{prefix}{display_line}");
                        let caret = (line_idx == cursor_line_idx)
                            .then(|| prefix_len + cursor_col_in_line.saturating_sub(line_scroll));
                        draw_caret_line(stdout, panel_x + 2, screen_y, &display, caret, true, max_width)?;

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
                } else if prop.kind == PropertyKind::Bool {
                    let mark = if prop.value.trim() == "true" { "x" } else { " " };
                    format!("[{}] {}", mark, prop.name).chars().take(max_width).collect()
                } else if prop.kind == PropertyKind::Note {
                    // Free-form note: the value is the whole line (no `name:`).
                    prop.value.chars().take(max_width).collect()
                } else {
                    format!("{}: {}", prop.name, fmt_val(&prop.value))
                        .chars().take(max_width).collect()
                }
            };

            if i == selected_prop {
                if matches!(prop.kind, PropertyKind::ReadOnly | PropertyKind::Note) {
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
            } else if matches!(prop.kind, PropertyKind::ReadOnly | PropertyKind::Note) {
                // Non-selected ReadOnly / Note: always dimmed
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

            // Colour rows get a swatch at the right edge (skip while its dropdown
            // is open — the option list below shows swatches of its own).
            if prop.kind == PropertyKind::Color && !(i == selected_prop && dropdown.is_some()) {
                let sx = panel_x + 2 + (max_width as u16).saturating_sub(2);
                draw_color_swatch(stdout, sx, screen_y, &prop.value)?;
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
            draw_color_swatch(stdout, panel_x + 2 + (max_width as u16).saturating_sub(2), y, opt)?;
        }
    }

    Ok(())
}

/// Draw the centred multi-line text-editing overlay over the canvas. Active only
/// while editing a `Text` property's value; a no-op otherwise. The interior
/// shows `\n`-delimited lines with the underline insertion caret, scrolling
/// vertically and (per cursor line) horizontally so the caret is always visible.
pub fn render_text_overlay(
    stdout: &mut io::Stdout,
    layout: &Layout,
    state: &EditorState,
) -> anyhow::Result<()> {
    let (object_index, selected_property, buf, cursor) = match &state.mode {
        Mode::EditProperties { object_index, selected_property, editing_value: Some(b), cursor, .. } =>
            (*object_index, *selected_property, b.clone(), *cursor),
        _ => return Ok(()),
    };

    let props = properties::get_properties(&state.source.objects, object_index);
    match props.get(selected_property) {
        Some(p) if p.kind == PropertyKind::Text => {}
        _ => return Ok(()),
    }
    let name = props[selected_property].name;

    let (bx, by, bw, bh) = super::ui::text_overlay(layout);
    if bw < 4 || bh < 3 {
        return Ok(());
    }
    let inner_w = (bw - 2) as usize;
    let inner_h = (bh - 2) as usize;

    // Cursor position as (line, col) over the logical buffer.
    let (cur_line, cur_col) = {
        let (mut line, mut col) = (0usize, 0usize);
        for (i, ch) in buf.chars().enumerate() {
            if i == cursor { break; }
            if ch == '\n' { line += 1; col = 0; } else { col += 1; }
        }
        (line, col)
    };
    let lines: Vec<&str> = buf.split('\n').collect();
    let v_off = if cur_line >= inner_h { cur_line - inner_h + 1 } else { 0 };
    let h_off = if cur_col >= inner_w { cur_col - inner_w + 1 } else { 0 };

    // Border with a title on the top edge and a hint on the bottom edge.
    let title = {
        let t = format!(" edit {name} ");
        t.chars().take(inner_w).collect::<String>()
    };
    let hint = " Alt+Enter: newline · Enter: save · Esc: cancel ";
    let hint: String = hint.chars().take(inner_w).collect();

    let top: String = std::iter::once('\u{250c}')
        .chain(title.chars())
        .chain(std::iter::repeat('\u{2500}').take(inner_w.saturating_sub(title.chars().count())))
        .chain(std::iter::once('\u{2510}'))
        .collect();
    let bottom: String = std::iter::once('\u{2514}')
        .chain(hint.chars())
        .chain(std::iter::repeat('\u{2500}').take(inner_w.saturating_sub(hint.chars().count())))
        .chain(std::iter::once('\u{2518}'))
        .collect();

    queue!(stdout, cursor::MoveTo(bx, by), style::Print(top))?;
    queue!(stdout, cursor::MoveTo(bx, by + bh - 1), style::Print(bottom))?;

    for row in 0..inner_h {
        let y = by + 1 + row as u16;
        let li = v_off + row;
        let chars: Vec<char> = lines.get(li).copied().unwrap_or("").chars().collect();
        let line_h_off = if li == cur_line { h_off } else { 0 };
        let window: String =
            (0..inner_w).map(|col| chars.get(line_h_off + col).copied().unwrap_or(' ')).collect();
        let caret = (li == cur_line).then(|| cur_col - line_h_off);
        queue!(stdout, cursor::MoveTo(bx, y), style::Print("\u{2502}"))?;
        draw_caret_line(stdout, bx + 1, y, &window, caret, false, inner_w)?;
        queue!(stdout, cursor::MoveTo(bx + 1 + inner_w as u16, y), style::Print("\u{2502}"))?;
    }

    Ok(())
}

