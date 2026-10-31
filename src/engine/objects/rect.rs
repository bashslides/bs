use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::{Resolve, ResolveCtx};

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
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        let frame = ctx.frame;
        if !self.frames.contains(frame) {
            return;
        }

        let x = usize::from(self.position.x.evaluate(frame, ctx.anims));
        let y = usize::from(self.position.y.evaluate(frame, ctx.anims));
        let w = usize::from(self.width.evaluate(frame, ctx.anims));
        let h = usize::from(self.height.evaluate(frame, ctx.anims));
        let s = &self.style;
        let z = self.z_order;
        // Positions in usize; cells past u16::MAX are dropped by `DrawOp::at`.
        let mut put =
            |x: usize, y: usize, ch: char, z: i32| ops.extend(DrawOp::at(x, y, ch, s.clone(), z));

        // Top edge
        put(x, y, '┌', z);
        for i in 1..w.saturating_sub(1) {
            put(x + i, y, '─', z);
        }
        if w > 1 {
            put(x + w - 1, y, '┐', z);
        }

        // Side edges
        for j in 1..h.saturating_sub(1) {
            put(x, y + j, '│', z);
            if w > 1 {
                put(x + w - 1, y + j, '│', z);
            }
        }

        // Bottom edge
        if h > 1 {
            put(x, y + h - 1, '└', z);
            for i in 1..w.saturating_sub(1) {
                put(x + i, y + h - 1, '─', z);
            }
            if w > 1 {
                put(x + w - 1, y + h - 1, '┘', z);
            }
        }

        // Title (rendered on top edge, one z-level above the rect; at i32::MAX it
        // ties and wins by being drawn after the border)
        // from x + 2, clipped before the right corner: at most w - 3 chars (none
        // for a rect narrower than 4, without underflowing at width 0).
        if let Some(title) = &self.title {
            for (i, ch) in title.chars().take(w.saturating_sub(3)).enumerate() {
                put(x + 2 + i, y, ch, z.saturating_add(1));
            }
        }
    }
}
