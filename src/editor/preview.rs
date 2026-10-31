use std::io;

use crossterm::{cursor, queue, style};

use crate::engine::objects::{Resolve, ResolveCtx};
use crate::engine::Engine;
use crate::engine::source::{AnimSpans, SceneObject};
use crate::player::to_content_style;
use crate::types::{Color, NamedColor, ResolvedScene, Style, TerminalContract, MAX_FRAME_OPS};

use super::state::{EditorState, Mode, TableCellSubState};
use super::ui::Layout;

/// Returns the set of "focused" object indices for the current mode.
/// Non-focused objects are dimmed; focused objects keep their style (or get white while selecting).
/// Returns None when all objects render normally.
fn focus_indices(state: &EditorState) -> Option<Vec<usize>> {
    match &state.mode {
        Mode::SelectedObject { object_index }
        | Mode::ResizeObject { object_index }
        | Mode::EditProperties { object_index, .. } => {
            // When a Group is selected, highlight its members instead.
            group_focus(&state.source.objects, *object_index)
        }
        Mode::AnimateProperty { object_index, .. } => Some(vec![*object_index]),
        Mode::ConvergeConfig { members, .. } => Some(members.clone()),
        Mode::MultiSelect { selected, .. } => {
            // Highlight the object under the cursor (a Group expands to its
            // members), the way the old single-pick select did.
            let visible = state.objects_on_current_frame();
            visible.get(*selected).copied().and_then(|i| group_focus(&state.source.objects, i))
        }
        // The action sub-menu keeps the whole selected set highlighted.
        Mode::SelectAction { members, .. } => Some(members.clone()),
        // Placing pasted clones: highlight the whole ghost set so it stands out
        // while the user moves it.
        Mode::PastePlacing { pending, .. } => {
            (!pending.is_empty()).then(|| pending.clone())
        }
        // Table modes: focus the table object (rendering is overridden separately)
        Mode::TableEditCellProps { object_index, .. }
        | Mode::TableAddColumn { object_index, .. }
        | Mode::TableRemoveColumn { object_index, .. } => Some(vec![*object_index]),
        _ => None,
    }
}

/// The focus set for object `i`: a group stands for its leaf members (through
/// nested groups — exactly what `move_group`/`resize_group` act on), anything
/// else (or a group with no placed members) for itself.
fn group_focus(objects: &[SceneObject], i: usize) -> Option<Vec<usize>> {
    match objects.get(i)? {
        SceneObject::Group(g) => {
            let leaves = super::properties::group_leaves(objects, &g.members);
            Some(if leaves.is_empty() { vec![i] } else { leaves })
        }
        _ => Some(vec![i]),
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

/// Style applied to the focused object while selecting: white, no dim, no background.
const fn selected_style() -> Style {
    Style {
        fg: Some(Color::Named(NamedColor::White)),
        bg: None,
        bold: false,
        dim: false,
    }
}

/// Whether a one-cell outline around a `pres_w`×`pres_h` deck fits in the
/// canvas. Widened to u32 so a deck near `u16::MAX` cannot overflow; when it
/// fits, the border coordinates stay inside the canvas and so fit in u16.
fn border_fits(pres_w: u16, pres_h: u16, canvas_w: u16, canvas_h: u16) -> bool {
    u32::from(pres_w) + 2 <= u32::from(canvas_w) && u32::from(pres_h) + 2 <= u32::from(canvas_h)
}

/// Say why the deck cannot be previewed, on the canvas's first row.
fn print_preview_error(stdout: &mut io::Stdout, layout: &Layout, err: &str) -> anyhow::Result<()> {
    let msg: String =
        format!("Cannot preview: {err}").chars().take(layout.canvas_width as usize).collect();
    queue!(stdout, cursor::MoveTo(layout.canvas_x, layout.canvas_y), style::Print(crate::types::printable_text(&msg)))?;
    Ok(())
}

/// The per-frame scenes the preview rasterizes, for frames `0..=last` only (all
/// that `grid_at(last)` replays), produced lazily: `Engine::scenes` when nothing
/// is focused, otherwise the same objects with non-focused ones dimmed, the
/// single focused object raised above the rest, and table editing overlays.
/// Each frame is held to `MAX_FRAME_OPS` like a compile.
fn preview_scenes(
    state: &EditorState,
    last: usize,
) -> Box<dyn Iterator<Item = Result<ResolvedScene, String>> + '_> {
    let frames = state.source.frame_count.min(last.saturating_add(1));
    // Determine table-specific overlay parameters.
    let table_cell_overlay = match &state.mode {
        Mode::TableEditCellProps { object_index, cursor_row, cursor_col, selected_cells, sub_state } => {
            // During navigation (Selecting) the cursor borders blink; in all other
            // sub-states (editing content, editing style) they are shown steadily.
            let cursor = if state.blink_hidden && matches!(sub_state, TableCellSubState::Selecting) {
                None
            } else {
                Some((*cursor_row, *cursor_col))
            };
            Some((*object_index, None::<usize>, selected_cells.clone(), cursor))
        }
        Mode::TableRemoveColumn { object_index, col_num, .. } => {
            let col_idx = col_num.saturating_sub(1);
            Some((*object_index, Some(col_idx), vec![], None))
        }
        _ => None,
    };

    // Compile and rasterize — dim non-focused objects when a focus set is active.
    let is_select_mode = matches!(state.mode, Mode::MultiSelect { .. });
    if let Some(focused) = focus_indices(state) {
        // For a single focused object (non-group) we boost its z_order above others.
        let single_focus = if focused.len() == 1 { Some(focused[0]) } else { None };
        // Single source of truth for animation timing, threaded into resolve.
        let anims = AnimSpans::of(&state.source);
        // The engine's view: explicit group ranges applied to their members,
        // so the focus path gates objects exactly like Normal mode and play.
        let objects = state.source.effective_objects();

        Box::new((0..frames).map(move |frame| {
            let mut ops = Vec::new();
            let ctx = ResolveCtx {
                frame,
                canvas_width: state.source.width,
                canvas_height: state.source.height,
                anims: &anims,
            };
            let mut single_start = 0;
            let mut single_end = 0;
            for (i, obj) in objects.iter().map(|o| o.as_ref()).enumerate() {
                let before = ops.len();
                // For table objects with editor overlay, use the specialized resolve.
                if let Some((tbl_idx, highlighted_col, ref sel_cells, cursor_cell)) = table_cell_overlay {
                    if i == tbl_idx {
                        if let SceneObject::Table(t) = obj {
                            // When actively editing a cell, apply the current edit buffer
                            // so the typed text is visible live in the canvas (WYSIWYG),
                            // and pass the caret so the cell renders a block cursor.
                            let editing_buf = match &state.mode {
                                Mode::TableEditCellProps {
                                    sub_state: TableCellSubState::EditingContent { row, col, buf, cursor },
                                    ..
                                } => Some((*row, *col, buf.clone(), *cursor)),
                                _ => None,
                            };
                            if let Some((er, ec, ref buf, caret)) = editing_buf {
                                let mut t_clone = t.clone();
                                t_clone.normalize_cells();
                                if let Some(row_vec) = t_clone.cells.get_mut(er) {
                                    if let Some(cell) = row_vec.get_mut(ec) {
                                        cell.content = buf.clone();
                                    }
                                }
                                t_clone.resolve_with_editor_overlay(
                                    &ctx,
                                    highlighted_col,
                                    sel_cells,
                                    cursor_cell,
                                    state.blink_hidden,
                                    Some((er, ec, caret)),
                                    &mut ops,
                                );
                            } else {
                                t.resolve_with_editor_overlay(
                                    &ctx,
                                    highlighted_col,
                                    sel_cells,
                                    cursor_cell,
                                    state.blink_hidden,
                                    None,
                                    &mut ops,
                                );
                            }
                        } else {
                            obj.resolve(&ctx, &mut ops);
                        }
                    } else {
                        obj.resolve(&ctx, &mut ops);
                    }
                } else {
                    obj.resolve(&ctx, &mut ops);
                }
                Engine::check_frame_ops(frame, ops.len(), MAX_FRAME_OPS)?;

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
                    // For table overlay modes: do NOT override styles (already set by resolve_with_editor_overlay)
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
                // In i64: z_order is any i32, so `max + 1 - min` can
                // overflow. Ops clamped to i32::MAX tie with the others
                // there, so the focused ops then move last — the renderer's
                // stable sort lets the later of equal z win. Focused ops
                // whose own z differed may also tie there; they keep their
                // emission order (a rect's title after its border, a
                // table's cursor after its cells), so they still layer right.
                if min_focused_z <= max_other_z {
                    let boost = max_other_z as i64 + 1 - min_focused_z as i64;
                    let mut clamped = false;
                    for op in &mut ops[single_start..single_end] {
                        let z = op.z_order as i64 + boost;
                        clamped |= z > i32::MAX as i64;
                        op.z_order = z.min(i32::MAX as i64) as i32;
                    }
                    if clamped {
                        let focused: Vec<_> = ops.drain(single_start..single_end).collect();
                        ops.extend(focused);
                    }
                }
            }
            Ok(ResolvedScene {
                width: state.source.width,
                height: state.source.height,
                ops,
            })
        }))
    } else {
        Box::new(Engine::scenes(&state.source).take(frames))
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

    // A deck outside the contract limits (e.g. hand-edited to 65535×65535)
    // would allocate far too much to compile; show why instead of rendering.
    if let Err(e) = state.source.validate_contract() {
        return print_preview_error(stdout, layout, &e);
    }

    // Draw a dim outline *around* the active (presentation) area. The frame sits
    // in the ring just outside the content, so the presentation is inset by one
    // cell — otherwise the painted cells below would cover the border. When the
    // outline wouldn't fit in the canvas, skip it and paint at the origin.
    let pres_w = state.source.width;
    let pres_h = state.source.height;
    let cx = layout.canvas_x;
    let cy = layout.canvas_y;

    let draw_border = border_fits(pres_w, pres_h, layout.canvas_width, layout.canvas_height);
    // Origin where presentation content is painted (inset when framed).
    let (ox, oy) = if draw_border { (cx + 1, cy + 1) } else { (cx, cy) };

    if draw_border {
        let left = cx;
        let right = cx + pres_w + 1;
        let top = cy;
        let bottom = cy + pres_h + 1;

        queue!(stdout, style::SetAttribute(style::Attribute::Dim))?;
        // Top/bottom edges
        for x in left..=right {
            queue!(stdout, cursor::MoveTo(x, top), style::Print("\u{2500}"))?;
            queue!(stdout, cursor::MoveTo(x, bottom), style::Print("\u{2500}"))?;
        }
        // Left/right edges
        for y in top..=bottom {
            queue!(stdout, cursor::MoveTo(left, y), style::Print("\u{2502}"))?;
            queue!(stdout, cursor::MoveTo(right, y), style::Print("\u{2502}"))?;
        }
        // Corners
        queue!(
            stdout,
            cursor::MoveTo(left, top),
            style::Print("\u{250c}"),
            cursor::MoveTo(right, top),
            style::Print("\u{2510}"),
            cursor::MoveTo(left, bottom),
            style::Print("\u{2514}"),
            cursor::MoveTo(right, bottom),
            style::Print("\u{2518}"),
        )?;
        queue!(stdout, style::SetAttribute(style::Attribute::Reset))?;
    }

    let contract = TerminalContract {
        width: state.source.width,
        height: state.source.height,
    };
    let scenes = preview_scenes(state, state.current_frame);
    let presentation = match crate::compile::render_scenes(scenes, contract) {
        Ok(presentation) => presentation,
        Err(e) => return print_preview_error(stdout, layout, &e.to_string()),
    };

    // Build the cell grid by replaying frames 0..=current. Shares one
    // implementation with the player and the test harness so the WYSIWYG
    // preview can never disagree with playback (see `grid_at`).
    let grid = presentation.grid_at(state.current_frame);

    // Paint cells within the canvas at the (possibly inset) content origin.
    for (y, row) in grid.iter().enumerate() {
        let sy = oy + y as u16;
        if sy >= cy + layout.canvas_height {
            break;
        }
        for (x, cell) in row.iter().enumerate() {
            let sx = ox + x as u16;
            if sx >= cx + layout.canvas_width {
                break;
            }
            let cs = to_content_style(&cell.style);
            queue!(
                stdout,
                cursor::MoveTo(sx, sy),
                style::PrintStyledContent(style::StyledContent::new(cs, cell.printable())),
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{border_fits, preview_scenes};
    use crate::editor::state::{EditorState, Mode};
    use crate::engine::Engine;
    use crate::types::ResolvedScene;

    /// Every frame's preview scene (small test decks, so all within budget).
    fn all_scenes(state: &EditorState) -> Vec<ResolvedScene> {
        preview_scenes(state, usize::MAX).map(Result::unwrap).collect()
    }

    #[test]
    fn focused_preview_bounds_frame_ops_and_frames() {
        // 300 overlapping full-canvas labels, one selected: the focus path
        // resolves frames itself, so it must stop at the per-frame op budget
        // (not allocate 300M ops), and only resolve frames up to `last`.
        let label = r#"{ "type": "label", "text": "", "width": 1000, "height": 1000,
            "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
            "style": { "bg": "red" }, "frames": { "start": 0, "end": 3 } }"#;
        let mut state = EditorState::open("/nonexistent/bs_preview_budget.json").unwrap();
        state.source = serde_json::from_str(&format!(
            r#"{{ "width": 1000, "height": 1000, "frame_count": 3, "objects": [{}] }}"#,
            vec![label; 300].join(",")
        ))
        .unwrap();
        state.mode = Mode::SelectedObject { object_index: 0 };
        let err = preview_scenes(&state, 0).next().unwrap().unwrap_err();
        assert!(err.contains("frame 1 draws over 8000000 cells"), "got: {err}");

        state.source.objects.truncate(1);
        assert_eq!(preview_scenes(&state, 1).count(), 2);
        state.mode = Mode::Normal;
        assert_eq!(preview_scenes(&state, 1).count(), 2);
    }

    /// Label 0 on frames 0..2 in an explicit-range group (object 1), plus an
    /// unrelated label 2 on every frame; the group selected, so its member is
    /// the focus set.
    fn focused_group_member(group_range: (usize, usize)) -> EditorState {
        let mut state = EditorState::open("/nonexistent/bs_preview_group_range.json").unwrap();
        state.source = serde_json::from_str(&format!(
            r#"{{ "width": 10, "height": 2, "frame_count": 4, "objects": [
                {{ "type": "label", "text": "Hi", "frames": {{ "start": 0, "end": 2 }},
                   "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }} }},
                {{ "type": "group", "members": [0],
                   "frames": {{ "start": {}, "end": {} }} }},
                {{ "type": "label", "text": "bg", "frames": {{ "start": 0, "end": 4 }},
                   "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 1 }} }} }}
            ] }}"#,
            group_range.0, group_range.1
        ))
        .unwrap();
        state.mode = Mode::SelectedObject { object_index: 1 };
        state
    }

    /// The cells drawn on each frame, ignoring the focus styling/z-boost.
    fn drawn(scenes: &[crate::types::ResolvedScene]) -> Vec<Vec<(u16, u16, char)>> {
        scenes.iter().map(|s| s.ops.iter().map(|o| (o.x, o.y, o.ch)).collect()).collect()
    }

    #[test]
    fn selecting_an_outer_group_focuses_the_nested_groups_members() {
        // Group 1 = [label 0]; new group 3 = [group 1, label 2]: selecting 3
        // must highlight both labels, not the (draw-nothing) inner group.
        let mut state = focused_group_member((0, 4));
        state.source.objects.push(serde_json::from_str(r#"{ "type": "group", "members": [1, 2] }"#).unwrap());
        state.mode = Mode::SelectedObject { object_index: 3 };
        assert_eq!(super::focus_indices(&state), Some(vec![0, 2]));
    }

    #[test]
    fn focused_preview_applies_explicit_group_ranges_like_compile() {
        // Widening (0..2 → 0..4) and narrowing (0..2 → 1..2) the member.
        for range in [(0, 4), (1, 2)] {
            let mut state = focused_group_member(range);
            assert_eq!(
                drawn(&all_scenes(&state)),
                drawn(&Engine::compile(&state.source)),
                "group range {range:?}"
            );
            // Select lists the member exactly on the frames it is drawn on.
            for frame in 0..4 {
                state.current_frame = frame;
                let listed = state.objects_on_current_frame().contains(&0);
                assert_eq!(listed, (range.0..range.1).contains(&frame), "frame {frame}");
            }
        }
    }

    #[test]
    fn focused_object_is_drawn_on_top_even_at_extreme_z_orders() {
        use crate::renderer::Renderer;
        use crate::types::TerminalContract;
        // Two labels on one cell; the selected one must show. Selecting A (drawn
        // first) against B at i32::MAX needs the tie-break, not just the boost.
        for (a_z, b_z, sel) in [
            (i32::MAX, 0, 1),
            (0, i32::MIN, 1),
            (i32::MAX, i32::MIN, 1),
            (0, 5, 1),
            (0, i32::MAX, 0),
            (i32::MIN, i32::MAX, 0),
        ] {
            let mut state = EditorState::open("/nonexistent/bs_preview_z.json").unwrap();
            state.source = serde_json::from_str(&format!(
                r#"{{ "width": 2, "height": 1, "frame_count": 1, "objects": [
                    {{ "type": "label", "text": "A", "z_order": {a_z}, "frames": {{ "start": 0, "end": 1 }},
                       "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }} }},
                    {{ "type": "label", "text": "B", "z_order": {b_z}, "frames": {{ "start": 0, "end": 1 }},
                       "position": {{ "x": {{ "fixed": 0 }}, "y": {{ "fixed": 0 }} }} }}
                ] }}"#
            ))
            .unwrap();
            state.mode = Mode::SelectedObject { object_index: sel };
            let p = Renderer::render(&all_scenes(&state), TerminalContract { width: 2, height: 1 });
            assert_eq!(p.grid_at(0)[0][0].ch, ['A', 'B'][sel], "z {a_z} vs {b_z}, selected {sel}");
        }
    }

    #[test]
    fn border_fits_needs_a_spare_ring_and_never_overflows() {
        assert!(border_fits(78, 22, 80, 24)); // exactly one ring of room
        assert!(!border_fits(79, 22, 80, 24));
        assert!(!border_fits(78, 23, 80, 24));
        assert!(!border_fits(u16::MAX, 10, 80, 24)); // would overflow in u16
        assert!(!border_fits(10, u16::MAX - 1, 80, 24));
    }
}
