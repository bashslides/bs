use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::Resolve;

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
/// Wrapping only happens at space characters; the space at the break point is
/// consumed so the next row never starts with an accidental leading space.
/// When no space exists within the available width the line is hard-broken.
/// List-item continuation rows (see `list_continuation_indent`) receive an
/// indent prefix on every row after the first.
///
/// Returns one `Vec<char>` per visual row, each of length `w` (padded with
/// spaces).  Empty lines return a single empty `Vec`.
fn wrap_text_line(line: &str, w: usize) -> Vec<Vec<char>> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return vec![Vec::new()];
    }
    let indent = list_continuation_indent(line);
    let mut rows: Vec<Vec<char>> = Vec::new();
    let mut pos = 0usize;
    let mut first = true;

    while pos < chars.len() {
        let col0 = if first { 0 } else { indent.min(w.saturating_sub(1)) };
        first = false;
        let avail = w - col0;

        let remaining = &chars[pos..];
        if remaining.len() <= avail {
            // Everything fits on this row.
            let mut row = vec![' '; w];
            for (i, &ch) in remaining.iter().enumerate() {
                row[col0 + i] = ch;
            }
            rows.push(row);
            break;
        }

        // Find the last space within the available width for a soft break.
        let chunk = &remaining[..avail];
        let (row_len, advance) = match chunk.iter().rposition(|&c| c == ' ') {
            Some(sp) => (sp, sp + 1), // break before space, skip the space
            None     => (avail, avail), // hard break
        };

        let mut row = vec![' '; w];
        for (i, &ch) in remaining[..row_len].iter().enumerate() {
            row[col0 + i] = ch;
        }
        rows.push(row);
        pos += advance;

        // Skip any additional leading spaces so the next row starts on a word.
        while pos < chars.len() && chars[pos] == ' ' {
            pos += 1;
        }
    }

    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
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
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Resolve for Label {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);
        let w = self.width.evaluate(frame) as usize;
        let h = self.height.evaluate(frame) as usize;

        let has_bg = self.style.bg.is_some();

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
                    rows.push(wrapped_row);
                    row += 1;
                }
            }
            // If height is set, pad remaining rows
            if h > 0 {
                while rows.len() < h {
                    rows.push(Vec::new());
                }
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
                        x: base_x + col as u16,
                        y: base_y + r as u16,
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
                    base_x.saturating_sub(1),
                    base_y.saturating_sub(1),
                    w + 2,
                    rows.len() + 2,
                    border_style,
                    self.z_order,
                );
            }
        } else {
            // No wrapping — emit chars directly, no fill
            let mut row: usize = 0;
            let mut max_len: usize = 0;
            for line in self.text.split('\n') {
                if h > 0 && row >= h {
                    break;
                }
                let line_len = line.chars().count();
                if line_len > max_len {
                    max_len = line_len;
                }
                for (col, ch) in line.chars().enumerate() {
                    ops.push(DrawOp {
                        x: base_x + col as u16,
                        y: base_y + row as u16,
                        ch,
                        style: self.style.clone(),
                        z_order: self.z_order,
                    });
                }
                row += 1;
            }
            if self.framed {
                let border_style = self.frame_style.as_ref().unwrap_or(&self.style);
                draw_frame(
                    ops,
                    base_x.saturating_sub(1),
                    base_y.saturating_sub(1),
                    max_len + 2,
                    (if h > 0 { h } else { row }) + 2,
                    border_style,
                    self.z_order,
                );
            }
        }
    }
}
