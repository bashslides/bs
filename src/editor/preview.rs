use std::io;

use crossterm::{cursor, queue, style};

use crate::engine::objects::Resolve;
use crate::engine::Engine;
use crate::player::to_content_style;
use crate::renderer::Renderer;
use crate::types::{Color, Frame, NamedColor, ResolvedScene, Style, TerminalContract};

use super::state::{EditorState, Mode};
use super::ui::Layout;

/// Returns the set of "focused" object indices for the current mode.
/// Non-focused objects are dimmed; focused objects keep their style (or get white in SelectObject).
/// Returns None when all objects render normally.
fn focus_indices(state: &EditorState) -> Option<Vec<usize>> {
    match &state.mode {
        Mode::SelectedObject { object_index } | Mode::EditProperties { object_index, .. } => {
            // When a Group is selected, highlight its members instead.
            match state.source.objects.get(*object_index) {
                Some(crate::engine::source::SceneObject::Group(g)) if !g.members.is_empty() => {
                    Some(g.members.clone())
                }
                Some(_) => Some(vec![*object_index]),
                None => None,
            }
        }
        Mode::AnimateProperty { object_index, .. } => Some(vec![*object_index]),
        Mode::SelectObject { selected } => {
            let visible = state.objects_on_current_frame();
            visible.get(*selected).copied().and_then(|i| {
                // When the selected object is a Group, highlight its members instead
                // (the Group itself has no visual representation).
                match state.source.objects.get(i) {
                    Some(crate::engine::source::SceneObject::Group(g)) if !g.members.is_empty() => {
                        Some(g.members.clone())
                    }
                    Some(_) => Some(vec![i]),
                    None => None,
                }
            })
        }
        Mode::SelectGroupMembers { selected, .. } => {
            // Only the currently navigated object is highlighted; toggled members
            // are dimmed the same as everything else (the panel shows [+]/[ ] markers).
            if *selected < state.source.objects.len() {
                Some(vec![*selected])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Dim style applied to non-focused objects: white foreground, dimmed, no background.
const fn dim_style() -> Style {
    Style {
        fg: Some(Color::Named(NamedColor::White)),
        bg: None,
        bold: false,
        dim: true,
    }
}

/// Style applied to the focused object in SelectObject mode: white, no dim, no background.
const fn selected_style() -> Style {
    Style {
        fg: Some(Color::Named(NamedColor::White)),
        bg: None,
        bold: false,
        dim: false,
    }
}

/// Render the current frame using the production Engine + Renderer pipeline,
/// positioned within the canvas area of the editor layout.
pub fn render_canvas_production(
    stdout: &mut io::Stdout,
    layout: &Layout,
    state: &EditorState,
) -> anyhow::Result<()> {
    // Clear canvas area
    for y in layout.canvas_y..layout.canvas_y + layout.canvas_height {
        queue!(stdout, cursor::MoveTo(layout.canvas_x, y))?;
        for _ in 0..layout.canvas_width {
            queue!(stdout, style::Print(" "))?;
        }
    }

    // Draw a dim border showing the presentation area boundary
    let pres_w = state.source.width;
    let pres_h = state.source.height;
    let cx = layout.canvas_x;
    let cy = layout.canvas_y;

    if pres_w <= layout.canvas_width && pres_h <= layout.canvas_height {
        queue!(stdout, style::SetAttribute(style::Attribute::Dim))?;

        // Top/bottom edges
        for x in 0..pres_w {
            queue!(
                stdout,
                cursor::MoveTo(cx + x, cy),
                style::Print("\u{2500}"),
            )?;
            if pres_h > 1 {
                queue!(
                    stdout,
                    cursor::MoveTo(cx + x, cy + pres_h - 1),
                    style::Print("\u{2500}"),
                )?;
            }
        }
        // Left/right edges
        for y in 0..pres_h {
            queue!(
                stdout,
                cursor::MoveTo(cx, cy + y),
                style::Print("\u{2502}"),
            )?;
            if pres_w > 1 {
                queue!(
                    stdout,
                    cursor::MoveTo(cx + pres_w - 1, cy + y),
                    style::Print("\u{2502}"),
                )?;
            }
        }
        // Corners
        queue!(
            stdout,
            cursor::MoveTo(cx, cy),
            style::Print("\u{250c}"),
            cursor::MoveTo(cx + pres_w - 1, cy),
            style::Print("\u{2510}"),
            cursor::MoveTo(cx, cy + pres_h - 1),
            style::Print("\u{2514}"),
            cursor::MoveTo(cx + pres_w - 1, cy + pres_h - 1),
            style::Print("\u{2518}"),
        )?;

        queue!(stdout, style::SetAttribute(style::Attribute::Reset))?;
    }

    // Compile and rasterize — dim non-focused objects when a focus set is active.
    let is_select_mode = matches!(state.mode, Mode::SelectObject { .. } | Mode::SelectGroupMembers { .. });
    let scenes = if let Some(focused) = focus_indices(state) {
        // For a single focused object (non-group) we boost its z_order above others.
        let single_focus = if focused.len() == 1 { Some(focused[0]) } else { None };

        (0..state.source.frame_count)
            .map(|frame| {
                let mut ops = Vec::new();
                let mut single_start = 0;
                let mut single_end = 0;
                for (i, obj) in state.source.objects.iter().enumerate() {
                    let before = ops.len();
                    obj.resolve(frame, &mut ops);
                    if focused.contains(&i) {
                        if Some(i) == single_focus {
                            single_start = before;
                            single_end = ops.len();
                        }
                        if is_select_mode {
                            let s = if state.blink_hidden { dim_style() } else { selected_style() };
                            for op in &mut ops[before..] {
                                op.style = s.clone();
                            }
                        }
                        // else: keep original style for focused objects
                    } else {
                        let ds = dim_style();
                        for op in &mut ops[before..] {
                            op.style = ds.clone();
                        }
                    }
                }
                // Boost single focused object's z_order above all others
                if single_start < single_end {
                    let max_other_z = ops[..single_start]
                        .iter()
                        .chain(ops[single_end..].iter())
                        .map(|op| op.z_order)
                        .max()
                        .unwrap_or(0);
                    let min_focused_z = ops[single_start..single_end]
                        .iter()
                        .map(|op| op.z_order)
                        .min()
                        .unwrap_or(0);
                    if min_focused_z <= max_other_z {
                        let boost = max_other_z + 1 - min_focused_z;
                        for op in &mut ops[single_start..single_end] {
                            op.z_order += boost;
                        }
                    }
                }
                ResolvedScene {
                    width: state.source.width,
                    height: state.source.height,
                    ops,
                }
            })
            .collect()
    } else {
        Engine::compile(&state.source)
    };
    let contract = TerminalContract {
        width: state.source.width,
        height: state.source.height,
    };
    let presentation = Renderer::render(&scenes, contract);

    // Build the cell grid by replaying frames 0..=current
    let w = state.source.width as usize;
    let h = state.source.height as usize;
    let mut grid = vec![vec![crate::types::Cell::default(); w]; h];
    let last_frame = state
        .current_frame
        .min(presentation.frames.len().saturating_sub(1));
    for i in 0..=last_frame {
        match &presentation.frames[i] {
            Frame::Full { cells } => grid = cells.clone(),
            Frame::Diff { changes } => {
                for c in changes {
                    let x = c.x as usize;
                    let y = c.y as usize;
                    if y < grid.len() && x < grid[0].len() {
                        grid[y][x] = c.cell.clone();
                    }
                }
            }
        }
    }

    // Paint cells within the canvas at the layout offset
    for (y, row) in grid.iter().enumerate() {
        let sy = cy + y as u16;
        if sy >= cy + layout.canvas_height {
            break;
        }
        for (x, cell) in row.iter().enumerate() {
            let sx = cx + x as u16;
            if sx >= cx + layout.canvas_width {
                break;
            }
            let cs = to_content_style(&cell.style);
            queue!(
                stdout,
                cursor::MoveTo(sx, sy),
                style::PrintStyledContent(style::StyledContent::new(cs, cell.ch)),
            )?;
        }
    }

    Ok(())
}
