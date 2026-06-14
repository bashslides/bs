use serde::{Deserialize, Serialize};

use crate::types::{CommandRegion, DrawOp, Style};

use super::super::source::{deserialize_coord_compat, AnimSpans, Coordinate, FrameRange, Position};
use super::{Resolve, ResolveCtx};

fn default_true() -> bool {
    true
}

/// A "run a binary and show its output" object.
///
/// At compile time this draws only an optional clean border (a placeholder you
/// can frame and decorate with other objects) — its interior is left blank
/// because the binary's output is unknown until play time. The command spec
/// itself is emitted as a [`CommandRegion`] sidecar (see [`Command::region`]);
/// the player runs the binary, paints stdout/stderr into the interior, and marks
/// success with a ✓ or failure with a ✗ near the top-right. The editor and
/// compiler never execute the binary, so editing a deck is always safe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub position: Position,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub width: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub height: Coordinate,
    /// Program to run (looked up on `PATH`).
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Kill the binary after this many seconds. Omitted ⇒ no timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Draw a clean border around the output region (no label). On by default.
    #[serde(default = "default_true")]
    pub border: bool,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Command {
    /// Resolve this command into its runtime sidecar spec for the given frame.
    ///
    /// With a border, the output region is the inside of the box (one cell of
    /// border on each side) and the status indicator sits on the top edge near
    /// the right corner. Without a border, the region spans the full box and the
    /// status indicator sits in its top-right cell.
    pub fn region(&self, frame: usize, anims: &AnimSpans) -> CommandRegion {
        let bx = self.position.x.evaluate(frame, anims);
        let by = self.position.y.evaluate(frame, anims);
        let bw = self.width.evaluate(frame, anims);
        let bh = self.height.evaluate(frame, anims);

        let (x, y, w, h, status_x, status_y) = if self.border {
            (
                bx + 1,
                by + 1,
                bw.saturating_sub(2),
                bh.saturating_sub(2),
                bx + bw.saturating_sub(2),
                by,
            )
        } else {
            (bx, by, bw, bh, bx + bw.saturating_sub(1), by)
        };

        CommandRegion {
            start_frame: self.frames.start,
            end_frame: self.frames.end,
            x,
            y,
            w,
            h,
            status_x,
            status_y,
            command: self.command.clone(),
            args: self.args.clone(),
            cwd: self.cwd.clone(),
            timeout_secs: self.timeout_secs,
            style: self.style.clone(),
        }
    }
}

impl Resolve for Command {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        let frame = ctx.frame;
        if !self.frames.contains(frame) || !self.border {
            return;
        }

        let x = self.position.x.evaluate(frame, ctx.anims);
        let y = self.position.y.evaluate(frame, ctx.anims);
        let w = self.width.evaluate(frame, ctx.anims);
        let h = self.height.evaluate(frame, ctx.anims);
        let s = &self.style;
        let z = self.z_order;

        let mut push = |x: u16, y: u16, ch: char, z: i32| {
            ops.push(DrawOp { x, y, ch, style: s.clone(), z_order: z });
        };

        // Top edge
        push(x, y, '┌', z);
        for i in 1..w.saturating_sub(1) {
            push(x + i, y, '─', z);
        }
        if w > 1 {
            push(x + w - 1, y, '┐', z);
        }

        // Side edges
        for j in 1..h.saturating_sub(1) {
            push(x, y + j, '│', z);
            if w > 1 {
                push(x + w - 1, y + j, '│', z);
            }
        }

        // Bottom edge
        if h > 1 {
            push(x, y + h - 1, '└', z);
            for i in 1..w.saturating_sub(1) {
                push(x + i, y + h - 1, '─', z);
            }
            if w > 1 {
                push(x + w - 1, y + h - 1, '┘', z);
            }
        }
    }
}
