use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::Resolve;

/// Horizontal alignment of text within the label's `width`. Only meaningful when
/// `width > 0` (there is a box to align within); with auto width it is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical alignment of text within the label's `height`. Only meaningful when
/// `height > 0`; with auto height it is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

impl TextAlign {
    pub fn as_str(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim() {
            "left" => Some(TextAlign::Left),
            "center" => Some(TextAlign::Center),
            "right" => Some(TextAlign::Right),
            _ => None,
        }
    }
    fn is_default(&self) -> bool {
        matches!(self, TextAlign::Left)
    }
}

impl VerticalAlign {
    pub fn as_str(self) -> &'static str {
        match self {
            VerticalAlign::Top => "top",
            VerticalAlign::Center => "center",
            VerticalAlign::Bottom => "bottom",
        }
    }
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim() {
            "top" => Some(VerticalAlign::Top),
            "center" => Some(VerticalAlign::Center),
            "bottom" => Some(VerticalAlign::Bottom),
            _ => None,
        }
    }
    fn is_default(&self) -> bool {
        matches!(self, VerticalAlign::Top)
    }
    /// Number of empty rows above `n` content rows inside an `h`-row box.
    fn top_pad(self, h: usize, n: usize) -> usize {
        match self {
            VerticalAlign::Top => 0,
            VerticalAlign::Center => h.saturating_sub(n) / 2,
            VerticalAlign::Bottom => h.saturating_sub(n),
        }
    }
}

/// Re-place a wrapped, width-`w` row's content according to `align`. `Left`
/// returns the row untouched (preserving any list-continuation indent); `Center`
/// and `Right` trim the content to its non-space span and re-seat it within `w`.
fn align_row(row: Vec<char>, w: usize, align: TextAlign) -> Vec<char> {
    if align == TextAlign::Left {
        return row;
    }
    let Some(lo) = row.iter().position(|&c| c != ' ') else {
        return row; // blank row — nothing to align
    };
    let hi = row.iter().rposition(|&c| c != ' ').unwrap() + 1;
    let content_len = hi - lo;
    let start = match align {
        TextAlign::Center => w.saturating_sub(content_len) / 2,
        TextAlign::Right => w.saturating_sub(content_len),
        TextAlign::Left => 0,
    };
    let mut out = vec![' '; w];
    for (i, &c) in row[lo..hi].iter().enumerate() {
        if start + i < w {
            out[start + i] = c;
        }
    }
    out
}

fn draw_frame(ops: &mut Vec<DrawOp>, fx: u16, fy: u16, fw: usize, fh: usize, style: &Style, z_order: i32) {
    if fw < 2 || fh < 2 {
        return;
    }
    let fw = fw as u16;
    let fh = fh as u16;
    let z = z_order;

    ops.push(DrawOp { x: fx,          y: fy,          ch: '┌', style: style.clone(), z_order: z });
    ops.push(DrawOp { x: fx + fw - 1, y: fy,          ch: '┐', style: style.clone(), z_order: z });
    ops.push(DrawOp { x: fx,          y: fy + fh - 1, ch: '└', style: style.clone(), z_order: z });
    ops.push(DrawOp { x: fx + fw - 1, y: fy + fh - 1, ch: '┘', style: style.clone(), z_order: z });
    for i in 1..fw - 1 {
        ops.push(DrawOp { x: fx + i, y: fy,          ch: '─', style: style.clone(), z_order: z });
        ops.push(DrawOp { x: fx + i, y: fy + fh - 1, ch: '─', style: style.clone(), z_order: z });
    }
    for j in 1..fh - 1 {
        ops.push(DrawOp { x: fx,          y: fy + j, ch: '│', style: style.clone(), z_order: z });
        ops.push(DrawOp { x: fx + fw - 1, y: fy + j, ch: '│', style: style.clone(), z_order: z });
    }
}

/// Returns the number of spaces to insert at the start of wrapped continuation
/// lines for list items:
/// - `"- text"` → 2 (aligns with the text after the bullet)
/// - `"1. text"` → 3 (aligns with the text after the number and dot)
/// - anything else → 0
fn list_continuation_indent(line: &str) -> usize {
    if line.starts_with("- ") {
        return 2;
    }
    // Match "N. " where N is one or more ASCII digits.
    let after_digits = line.trim_start_matches(|c: char| c.is_ascii_digit());
    let digit_count = line.len() - after_digits.len(); // bytes == chars (ASCII digits)
    if digit_count > 0 && after_digits.starts_with(". ") {
        return 3;
    }
    0
}

/// Wrap a single logical text line to a grid width using word-breaking.
///
/// Delegates to the shared [`wrap`](super::wrap) helper, supplying the
/// list-item continuation indent. Returns one `Vec<char>` per visual row, each
/// of length `w` (padded with spaces); empty lines return a single empty `Vec`.
fn wrap_text_line(line: &str, w: usize) -> Vec<Vec<char>> {
    let indent = list_continuation_indent(line);
    let indexed = super::wrap::wrap_line_indexed(0, line, w, indent);
    super::wrap::indexed_to_chars(line, indexed)
}

fn default_label_width() -> Coordinate {
    Coordinate::Fixed(0.0)
}

fn default_label_height() -> Coordinate {
    Coordinate::Fixed(0.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub text: String,
    pub position: Position,
    #[serde(default = "default_label_width", deserialize_with = "deserialize_coord_compat")]
    pub width: Coordinate,
    #[serde(default = "default_label_height", deserialize_with = "deserialize_coord_compat")]
    pub height: Coordinate,
    /// When `true`, draw a single-cell border (box-drawing chars) around the label's
    /// bounding box.  The border is rendered one cell outside (x-1, y-1) so the text
    /// position is preserved.  Requires `width > 0` for reliable sizing; if `width`
    /// is 0 the frame is sized from the longest text line and line count.
    #[serde(default)]
    pub framed: bool,
    /// Optional separate style for the box border.  When `None`, the border uses
    /// the label's own `style`.  Only `fg` and `bg` are relevant for the border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_style: Option<Style>,
    /// Horizontal alignment of the text within `width` (no-op when `width == 0`).
    #[serde(default, skip_serializing_if = "TextAlign::is_default")]
    pub align: TextAlign,
    /// Vertical alignment of the text within `height` (no-op when `height == 0`).
    #[serde(default, skip_serializing_if = "VerticalAlign::is_default")]
    pub valign: VerticalAlign,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Resolve for Label {
    fn resolve(&self, frame: usize, _canvas_width: u16, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);
        let w = self.width.evaluate(frame) as usize;
        let h = self.height.evaluate(frame) as usize;

        let has_bg = self.style.bg.is_some();

        // The border sits one cell outside the text. Normally the text keeps its
        // declared position; but at the canvas edge (base 0) there is no room
        // outside, so the border would land *on* the text and hide it. There we
        // shift the text in by one instead, keeping it inside the border. Away
        // from the edge `draw_*` equals `base_*`, so positions are unchanged.
        let (frame_x, frame_y) = (base_x.saturating_sub(1), base_y.saturating_sub(1));
        let (draw_x, draw_y) = if self.framed {
            (frame_x + 1, frame_y + 1)
        } else {
            (base_x, base_y)
        };

        // Build a grid of characters when width > 0, so we can fill
        // remaining cells in the bounding box with bg-colored spaces.
        if w > 0 {
            let mut rows: Vec<Vec<char>> = Vec::new();
            let mut row: usize = 0;
            'lines: for line in self.text.split('\n') {
                if h > 0 && row >= h {
                    break;
                }
                for wrapped_row in wrap_text_line(line, w) {
                    if h > 0 && row >= h {
                        break 'lines;
                    }
                    rows.push(align_row(wrapped_row, w, self.align));
                    row += 1;
                }
            }
            // Vertical alignment within an explicit height: offset the content
            // rows by the top padding and fill the rest of the box with blanks.
            if h > 0 {
                let n = rows.len().min(h);
                let pad_top = self.valign.top_pad(h, n);
                let mut padded: Vec<Vec<char>> = vec![Vec::new(); h];
                for (i, r) in rows.into_iter().take(n).enumerate() {
                    padded[pad_top + i] = r;
                }
                rows = padded;
            }
            // Emit DrawOps for all cells
            for (r, row_chars) in rows.iter().enumerate() {
                let emit_w = if has_bg { w } else { row_chars.len() };
                for col in 0..emit_w {
                    let ch = row_chars.get(col).copied().unwrap_or(' ');
                    if !has_bg && ch == ' ' && col >= row_chars.len() {
                        continue;
                    }
                    ops.push(DrawOp {
                        x: draw_x + col as u16,
                        y: draw_y + r as u16,
                        ch,
                        style: self.style.clone(),
                        z_order: self.z_order,
                    });
                }
            }
            if self.framed {
                let border_style = self.frame_style.as_ref().unwrap_or(&self.style);
                draw_frame(
                    ops,
                    frame_x,
                    frame_y,
                    w + 2,
                    rows.len() + 2,
                    border_style,
                    self.z_order,
                );
            }
        } else {
            // No wrapping (auto width) — emit chars directly, no fill. Horizontal
            // alignment has no box to act in here, but vertical alignment still
            // does when a height is set: offset the rows by the top padding.
            let lines: Vec<&str> = self.text.split('\n').collect();
            let visible = if h > 0 { lines.len().min(h) } else { lines.len() };
            let pad_top = if h > 0 { self.valign.top_pad(h, visible) } else { 0 };
            let mut max_len: usize = 0;
            for (row, line) in lines.iter().take(visible).enumerate() {
                let line_len = line.chars().count();
                if line_len > max_len {
                    max_len = line_len;
                }
                for (col, ch) in line.chars().enumerate() {
                    ops.push(DrawOp {
                        x: draw_x + col as u16,
                        y: draw_y + (pad_top + row) as u16,
                        ch,
                        style: self.style.clone(),
                        z_order: self.z_order,
                    });
                }
            }
            if self.framed {
                let border_style = self.frame_style.as_ref().unwrap_or(&self.style);
                draw_frame(
                    ops,
                    frame_x,
                    frame_y,
                    max_len + 2,
                    (if h > 0 { h } else { visible }) + 2,
                    border_style,
                    self.z_order,
                );
            }
        }
    }
}
