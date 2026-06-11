use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::Resolve;

fn default_list_width() -> Coordinate {
    Coordinate::Fixed(0.0)
}

fn default_list_height() -> Coordinate {
    Coordinate::Fixed(0.0)
}

fn default_bullet() -> String {
    "-".to_string()
}

fn default_spacing() -> usize {
    1
}

/// An ordered (numbered) or unordered (bulleted) list.
///
/// Each `\n`-separated line of `text` is one item — editing is therefore
/// identical to a `Label`'s multi-line text editor. At render time every item
/// is prefixed with its marker (`"1. "`, `"2. "`, … when `ordered`, otherwise
/// `"{bullet} "`), and `spacing` blank rows are inserted between items.
/// Wrapped continuation rows are indented to line up under the item text (the
/// marker width), so multi-line items stay visually aligned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List {
    /// Newline-separated items; each line is one list entry.
    pub text: String,
    pub position: Position,
    /// Wrap width in cells. `0` disables wrapping (items render on one row each).
    #[serde(default = "default_list_width", deserialize_with = "deserialize_coord_compat")]
    pub width: Coordinate,
    /// Optional max height in cells; `0` means "as tall as the content".
    #[serde(default = "default_list_height", deserialize_with = "deserialize_coord_compat")]
    pub height: Coordinate,
    /// Ordered (numbered `1.`, `2.`, …) when `true`; bulleted when `false`.
    #[serde(default)]
    pub ordered: bool,
    /// Marker used for unordered items (ignored when `ordered`). Defaults to `-`.
    #[serde(default = "default_bullet")]
    pub bullet: String,
    /// Blank rows inserted between consecutive items. Defaults to `1`.
    #[serde(default = "default_spacing")]
    pub spacing: usize,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl List {
    /// The marker (including its trailing space) for the 0-based item `i`.
    fn marker(&self, i: usize) -> String {
        if self.ordered {
            format!("{}. ", i + 1)
        } else {
            format!("{} ", self.bullet)
        }
    }
}

impl Resolve for List {
    fn resolve(&self, frame: usize, _canvas_width: u16, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);
        let w = self.width.evaluate(frame) as usize;
        let h = self.height.evaluate(frame) as usize;
        let has_bg = self.style.bg.is_some();

        // Each non-empty line is one item; blank lines are dropped so a stray
        // trailing newline never renders a dangling empty bullet.
        let items: Vec<&str> = self.text.split('\n').filter(|l| !l.is_empty()).collect();

        // Build the full grid of rows (one Vec<char> per visual row), inserting
        // `spacing` blank rows between items.
        let mut rows: Vec<Vec<char>> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                for _ in 0..self.spacing {
                    rows.push(Vec::new());
                }
            }
            let marker = self.marker(i);
            let indent = marker.chars().count();
            let full = format!("{marker}{item}");
            if w > 0 {
                let indexed = super::wrap::wrap_line_indexed(0, &full, w, indent);
                rows.extend(super::wrap::indexed_to_chars(&full, indexed));
            } else {
                rows.push(full.chars().collect());
            }
        }

        // Clip / pad to an explicit height.
        if h > 0 {
            rows.truncate(h);
            while rows.len() < h {
                rows.push(Vec::new());
            }
        }

        for (r, row_chars) in rows.iter().enumerate() {
            // With a background, fill the whole wrap width so the block is solid;
            // otherwise emit exactly the produced cells.
            let emit_w = if has_bg && w > 0 { w } else { row_chars.len() };
            for col in 0..emit_w {
                let ch = row_chars.get(col).copied().unwrap_or(' ');
                ops.push(DrawOp {
                    x: base_x + col as u16,
                    y: base_y + r as u16,
                    ch,
                    style: self.style.clone(),
                    z_order: self.z_order,
                });
            }
        }
    }
}
