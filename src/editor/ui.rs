use super::state::Mode;

/// Width (in columns) of the right-hand property panel (EditProperties,
/// EditMultiProperties, and the other panel modes in `Layout::compute`).
/// Used by both the renderer and the input handler for scroll calculations.
pub const RIGHT_PANEL_WIDTH: u16 = 24;

pub struct Layout {
    pub right_panel_width: u16,
    pub canvas_x: u16,
    pub canvas_y: u16,
    pub canvas_width: u16,
    pub canvas_height: u16,
    pub timeline_y: u16,
    pub term_width: u16,
    pub menu_h: u16,
}

/// Outer rectangle `(x, y, w, h)` of the multi-line text-editing overlay,
/// centred in the canvas. The editable interior is this inset by one cell on
/// every side (so a border can be drawn around it). Used when editing a
/// `Text` property — far roomier than the ~21-column right-panel field.
pub fn text_overlay(layout: &Layout) -> (u16, u16, u16, u16) {
    let w = layout.canvas_width.min(64).max(12);
    let h = layout.canvas_height.min(16).max(5);
    let x = layout.canvas_x + layout.canvas_width.saturating_sub(w) / 2;
    let y = layout.canvas_y + layout.canvas_height.saturating_sub(h) / 2;
    (x, y, w, h)
}

/// Centred geometry for the single-line "Save As" popup: `(x, y, w, h)` with a
/// fixed 3-row height (top border + input line + bottom border).
pub fn save_as_overlay(layout: &Layout) -> (u16, u16, u16, u16) {
    let w = layout.canvas_width.min(60).max(16);
    let h = 3u16;
    let x = layout.canvas_x + layout.canvas_width.saturating_sub(w) / 2;
    let y = layout.canvas_y + layout.canvas_height.saturating_sub(h) / 2;
    (x, y, w, h)
}

impl Layout {
    pub fn compute(term_width: u16, term_height: u16, mode: &Mode, fullscreen: bool) -> Self {
        let right = match mode {
            Mode::EditProperties { .. }
            | Mode::EditMultiProperties { .. }
            | Mode::AnimateProperty { .. }
            | Mode::ConvergeConfig { .. }
            | Mode::AddObject { .. }
            | Mode::SelectAction { .. }
            | Mode::Confirm { .. }
            | Mode::MultiSelect { .. }
            | Mode::AddArt { .. }
            | Mode::LoadArtFile { .. }
            | Mode::Settings { .. } => RIGHT_PANEL_WIDTH,
            _ => 0,
        };
        // Fullscreen ("no bars") mode hides the menu bar and timeline, handing
        // their rows to the canvas.
        let timeline_h: u16 = if fullscreen { 0 } else { 2 };
        // SelectedObject has more key hints, so reserve 2 lines.
        let menu_h: u16 = if fullscreen {
            0
        } else {
            match mode {
                Mode::SelectedObject { .. } | Mode::ResizeObject { .. } => 2,
                _ => 1,
            }
        };
        Layout {
            right_panel_width: right,
            canvas_x: 0,
            canvas_y: menu_h,
            canvas_width: term_width.saturating_sub(right),
            canvas_height: term_height.saturating_sub(timeline_h + menu_h),
            timeline_y: term_height.saturating_sub(timeline_h),
            term_width,
            menu_h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel_width(mode: &Mode) -> u16 {
        Layout::compute(120, 40, mode, false).right_panel_width
    }

    #[test]
    fn property_panel_modes_get_a_right_panel() {
        // Regression: every mode whose right panel is drawn by `render_right_panel`
        // must also be allocated panel width here, or the panel silently vanishes
        // (the bug that left EditMultiProperties invisible while still editing).
        let edit_multi = Mode::EditMultiProperties {
            members: vec![0, 1],
            selected_property: 0,
            editing_value: None,
            cursor: 0,
            scroll: 0,
            panel_scroll: 0,
            dropdown: None,
        };
        assert_eq!(panel_width(&edit_multi), RIGHT_PANEL_WIDTH);
        assert_eq!(panel_width(&Mode::SelectAction { members: vec![0, 1], selected: 0 }), RIGHT_PANEL_WIDTH);
        // A plain navigation mode keeps the full canvas (no panel).
        assert_eq!(panel_width(&Mode::Normal), 0);
    }
}
