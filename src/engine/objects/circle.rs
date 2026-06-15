use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{FrameRange, Position};
use super::{Resolve, ResolveCtx};

fn default_diameter() -> u16 {
    10
}

fn default_fill_char() -> char {
    '@'
}

/// Terminal cells are roughly twice as tall as they are wide, so a circle that
/// is `diameter` rows tall needs about `2 × diameter` columns to *look* round
/// rather than squashed. The bounding box is therefore `diameter` rows by
/// [`Circle::columns`] columns.
pub const CIRCLE_ASPECT: f64 = 2.0;

/// A filled circle drawn with a single repeated character.
///
/// Unlike the static `Art` palette pieces, a `Circle` is **parametric**: its
/// `diameter` (height in rows) and fill character `ch` are ordinary editable
/// properties, so resizing or recolouring it re-renders the shape live. The
/// horizontal extent is derived from `diameter` (see [`Circle::columns`]) so the
/// shape stays circular on the ~2:1 terminal cell grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Circle {
    /// Top-left corner of the circle's bounding box.
    pub position: Position,
    /// Diameter in **rows** (the vertical extent). The column extent is derived
    /// from this to keep the shape round.
    #[serde(default = "default_diameter")]
    pub diameter: u16,
    /// Character the circle is filled with (default `@`).
    #[serde(default = "default_fill_char")]
    pub ch: char,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Circle {
    /// Columns in the bounding box of a `diameter`-row circle — widened by
    /// [`CIRCLE_ASPECT`] so the shape looks round, never squashed.
    pub fn columns(diameter: u16) -> u16 {
        ((diameter as f64) * CIRCLE_ASPECT).round().max(1.0) as u16
    }

    /// Inverse of [`Circle::columns`]: the row diameter implied by a bounding-box
    /// width of `cols`. Used when the editor resizes the circle by its width.
    pub fn rows_for_width(cols: f64) -> f64 {
        cols / CIRCLE_ASPECT
    }
}

impl Resolve for Circle {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        let frame = ctx.frame;
        if !self.frames.contains(frame) {
            return;
        }

        let x0 = self.position.x.evaluate(frame, ctx.anims);
        let y0 = self.position.y.evaluate(frame, ctx.anims);
        let rows = self.diameter.max(1);
        let cols = Circle::columns(rows);

        // Centre of the bounding box and the two radii. Because `cols ≈ 2 × rows`
        // and a cell is ~2× taller than wide, the cell-space ellipse (rx = cols/2,
        // ry = rows/2) renders as a visually round circle. A cell is filled when
        // it lies within that ellipse.
        let cx = (cols as f64 - 1.0) / 2.0;
        let cy = (rows as f64 - 1.0) / 2.0;
        let rx = cols as f64 / 2.0;
        let ry = rows as f64 / 2.0;

        for r in 0..rows {
            for c in 0..cols {
                let dx = (c as f64 - cx) / rx;
                let dy = (r as f64 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    ops.push(DrawOp {
                        x: x0 + c,
                        y: y0 + r,
                        ch: self.ch,
                        style: self.style.clone(),
                        z_order: self.z_order,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Circle;

    #[test]
    fn columns_widen_the_diameter_by_the_aspect_ratio() {
        // A circle N rows tall is ~2N columns wide so it reads as round.
        assert_eq!(Circle::columns(10), 20);
        assert_eq!(Circle::columns(7), 14);
        // Never collapses below one column.
        assert_eq!(Circle::columns(0), 1);
    }

    #[test]
    fn rows_for_width_inverts_columns() {
        // The editor maps a width-resize back to a row diameter.
        assert_eq!(Circle::rows_for_width(20.0).round() as u16, 10);
        assert_eq!(Circle::rows_for_width(Circle::columns(8) as f64).round() as u16, 8);
    }
}
