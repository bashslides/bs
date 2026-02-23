use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::Resolve;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub position: Position,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub width: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub height: Coordinate,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Resolve for Rect {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }

        let x = self.position.x.evaluate(frame);
        let y = self.position.y.evaluate(frame);
        let w = self.width.evaluate(frame);
        let h = self.height.evaluate(frame);
        let s = &self.style;
        let z = self.z_order;

        // Top edge
        ops.push(DrawOp { x, y, ch: '┌', style: s.clone(), z_order: z });
        for i in 1..w.saturating_sub(1) {
            ops.push(DrawOp { x: x + i, y, ch: '─', style: s.clone(), z_order: z });
        }
        if w > 1 {
            ops.push(DrawOp { x: x + w - 1, y, ch: '┐', style: s.clone(), z_order: z });
        }

        // Side edges
        for j in 1..h.saturating_sub(1) {
            ops.push(DrawOp { x, y: y + j, ch: '│', style: s.clone(), z_order: z });
            if w > 1 {
                ops.push(DrawOp { x: x + w - 1, y: y + j, ch: '│', style: s.clone(), z_order: z });
            }
        }

        // Bottom edge
        if h > 1 {
            ops.push(DrawOp { x, y: y + h - 1, ch: '└', style: s.clone(), z_order: z });
            for i in 1..w.saturating_sub(1) {
                ops.push(DrawOp { x: x + i, y: y + h - 1, ch: '─', style: s.clone(), z_order: z });
            }
            if w > 1 {
                ops.push(DrawOp { x: x + w - 1, y: y + h - 1, ch: '┘', style: s.clone(), z_order: z });
            }
        }

        // Title (rendered on top edge, one z-level above the rect)
        if let Some(title) = &self.title {
            for (i, ch) in title.chars().enumerate() {
                let tx = x + 2 + i as u16;
                if tx < x + w - 1 {
                    ops.push(DrawOp {
                        x: tx,
                        y,
                        ch,
                        style: s.clone(),
                        z_order: z + 1,
                    });
                }
            }
        }
    }
}
