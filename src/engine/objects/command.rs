use serde::{Deserialize, Serialize};

use crate::types::{CommandRegion, DrawOp, Style};

use super::super::source::{deserialize_coord_compat, Coordinate, FrameRange, Position};
use super::Resolve;

/// A "run a binary and show its output" object.
///
/// At compile time this draws only a bordered box (a placeholder you can frame
/// and decorate with other objects) — its interior is left blank because the
/// binary's output is unknown until play time. The command spec itself is
/// emitted as a [`CommandRegion`] sidecar (see [`Command::region`]); the player
/// runs the binary, paints stdout/stderr into the interior, and marks success
/// with a ✓ or failure with a ✗ on the top edge. The editor and compiler never
/// execute the binary, so editing a deck is always safe.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Title drawn on the top edge; defaults to `$ command args…`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Command {
    /// The title shown on the box's top edge.
    pub fn display_title(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        if self.args.is_empty() {
            format!("$ {}", self.command)
        } else {
            format!("$ {} {}", self.command, self.args.join(" "))
        }
    }

    /// Resolve this command into its runtime sidecar spec for the given frame.
    ///
    /// The interior region is the inside of the box (one cell of border on each
    /// side); the status indicator sits on the top edge near the right corner.
    pub fn region(&self, frame: usize) -> CommandRegion {
        let bx = self.position.x.evaluate(frame);
        let by = self.position.y.evaluate(frame);
        let bw = self.width.evaluate(frame);
        let bh = self.height.evaluate(frame);

        CommandRegion {
            start_frame: self.frames.start,
            end_frame: self.frames.end,
            x: bx + 1,
            y: by + 1,
            w: bw.saturating_sub(2),
            h: bh.saturating_sub(2),
            status_x: bx + bw.saturating_sub(2),
            status_y: by,
            command: self.command.clone(),
            args: self.args.clone(),
            cwd: self.cwd.clone(),
            timeout_ms: self.timeout_ms,
            style: self.style.clone(),
        }
    }
}

impl Resolve for Command {
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

        // Title on the top edge (one z-level above the box).
        let title = self.display_title();
        for (i, ch) in title.chars().enumerate() {
            let tx = x + 2 + i as u16;
            if tx < x + w.saturating_sub(1) {
                ops.push(DrawOp { x: tx, y, ch, style: s.clone(), z_order: z + 1 });
            }
        }
    }
}
