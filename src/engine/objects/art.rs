use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{FrameRange, Position};
use super::Resolve;

/// A pre-made ASCII-art object. The art text is stored inline (copied from the
/// art library when added), so a presentation never depends on the library.
///
/// Rendered verbatim: each character is placed at its row/column offset from
/// the object position. Spaces are transparent (not drawn) unless a background
/// color is set, so art can overlap other objects cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Art {
    pub position: Position,
    /// Raw multi-line art content.
    pub art: String,
    /// Name of the library piece this came from (or "custom"); display only.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Resolve for Art {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);
        let has_bg = self.style.bg.is_some();

        for (row, line) in self.art.split('\n').enumerate() {
            for (col, ch) in line.chars().enumerate() {
                if ch == ' ' && !has_bg {
                    continue; // transparent
                }
                ops.push(DrawOp {
                    x: base_x + col as u16,
                    y: base_y + row as u16,
                    ch,
                    style: self.style.clone(),
                    z_order: self.z_order,
                });
            }
        }
    }
}
