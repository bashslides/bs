use std::io;

use crossterm::{cursor, queue, style, terminal};

use super::state::{EditorState, Mode};
use super::ui::Layout;

pub fn render_timeline(
    stdout: &mut io::Stdout,
    layout: &Layout,
    state: &EditorState,
) -> anyhow::Result<()> {
    let y = layout.timeline_y;
    let width = layout.term_width as usize;
    let frame_count = state.source.frame_count;
    let current = state.current_frame;

    // Row 1: Frame numbers
    queue!(
        stdout,
        cursor::MoveTo(0, y),
        terminal::Clear(terminal::ClearType::CurrentLine),
    )?;

    if frame_count == 0 {
        queue!(stdout, style::Print(" (no frames)"))?;
    } else {
        render_frame_bar(stdout, width, frame_count, current)?;
    }

    // Row 2: Mode + status
    queue!(
        stdout,
        cursor::MoveTo(0, y + 1),
        terminal::Clear(terminal::ClearType::CurrentLine),
    )?;

    let mode_str = match &state.mode {
        Mode::Normal => "NORMAL",
        Mode::AddObject { .. } => "ADD OBJECT",
        Mode::SelectObject { .. } => "SELECT OBJECT",
        Mode::SelectedObject { .. } => "SELECTED",
        Mode::EditProperties { .. } => "EDIT PROPERTIES",
        Mode::AnimateProperty { .. } => "ANIMATE",
        Mode::Confirm { .. } => "CONFIRM",
        Mode::SelectGroupMembers { .. } => "ADD GROUP",
        Mode::TableAddColumn { .. } => "ADD COL",
        Mode::TableRemoveColumn { .. } => "REMOVE COL",
        Mode::TableEditCellProps { .. } => "EDIT CELLS",
    };
    let dirty_str = if state.dirty { " [modified]" } else { "" };
    // Replace newlines so a multi-line label value doesn't scroll the terminal.
    let status: String = state.status_message.as_deref().unwrap_or("")
        .chars().map(|c| if c == '\n' { '↵' } else { c }).collect();

    queue!(
        stdout,
        style::SetAttribute(style::Attribute::Dim),
        style::Print(format!(
            " {mode_str} | Frame {}/{frame_count}{dirty_str} {status}",
            current + 1,
        )),
        style::SetAttribute(style::Attribute::Reset),
    )?;

    Ok(())
}

fn render_frame_bar(
    stdout: &mut io::Stdout,
    width: usize,
    frame_count: usize,
    current: usize,
) -> anyhow::Result<()> {
    // Each frame label takes roughly 5 chars: "[XX] "
    // For wider numbers it takes more, but 5 is a good estimate.
    let label_width = 5usize;
    let max_visible = width / label_width;

    if frame_count <= max_visible {
        // Show all frames
        queue!(stdout, style::Print(" "))?;
        for f in 0..frame_count {
            render_frame_number(stdout, f, current)?;
        }
    } else {
        // Abbreviated view
        // Always show: frame 0, ..., vicinity of current, ..., last frame
        let mut to_show: Vec<usize> = Vec::new();

        // First frame
        to_show.push(0);

        // Vicinity of current (current-1, current, current+1)
        if current > 1 {
            to_show.push(current - 1);
        }
        to_show.push(current);
        if current + 1 < frame_count {
            to_show.push(current + 1);
        }

        // Last frame
        if frame_count > 1 {
            to_show.push(frame_count - 1);
        }

        // Deduplicate and sort
        to_show.sort();
        to_show.dedup();

        queue!(stdout, style::Print(" "))?;

        let mut prev: Option<usize> = None;
        for &f in &to_show {
            if let Some(p) = prev {
                if f > p + 1 {
                    queue!(stdout, style::Print("... "))?;
                }
            }
            render_frame_number(stdout, f, current)?;
            prev = Some(f);
        }
    }

    Ok(())
}

fn render_frame_number(
    stdout: &mut io::Stdout,
    frame: usize,
    current: usize,
) -> anyhow::Result<()> {
    let display_num = frame + 1; // 1-based display
    if frame == current {
        queue!(
            stdout,
            style::SetAttribute(style::Attribute::Reverse),
            style::Print(format!("[{display_num:>2}]")),
            style::SetAttribute(style::Attribute::Reset),
            style::Print(" "),
        )?;
    } else {
        queue!(stdout, style::Print(format!("[{display_num:>2}] ")))?;
    }
    Ok(())
}
