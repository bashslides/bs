use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, deserialize_coord_compat};
use super::{Resolve, ResolveCtx};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HLine {
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub y: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub x_start: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub x_end: Coordinate,
    #[serde(default = "default_hline_char")]
    pub ch: char,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

fn default_hline_char() -> char {
    '─'
}

impl Resolve for HLine {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        let frame = ctx.frame;
        if !self.frames.contains(frame) {
            return;
        }
        let y = self.y.evaluate(frame, ctx.anims);
        let x_start = self.x_start.evaluate(frame, ctx.anims);
        let x_end = self.x_end.evaluate(frame, ctx.anims);
        for x in x_start..x_end {
            ops.push(DrawOp {
                x,
                y,
                ch: self.ch,
                style: self.style.clone(),
                z_order: self.z_order,
            });
        }
    }
}
