use super::state::Mode;

/// Width (in columns) of the right-hand property panel in EditProperties mode.
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

impl Layout {
    pub fn compute(term_width: u16, term_height: u16, mode: &Mode) -> Self {
        let right = match mode {
            Mode::EditProperties { .. }
            | Mode::AnimateProperty { .. }
            | Mode::AddObject { .. }
            | Mode::SelectObject { .. }
            | Mode::Confirm { .. }
            | Mode::SelectGroupMembers { .. } => RIGHT_PANEL_WIDTH,
            _ => 0,
        };
        let timeline_h: u16 = 2;
        // SelectedObject has more key hints, so reserve 2 lines.
        let menu_h: u16 = match mode {
            Mode::SelectedObject { .. } => 2,
            _ => 1,
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
