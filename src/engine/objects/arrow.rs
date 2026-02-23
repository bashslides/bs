use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{Coordinate, FrameRange, deserialize_coord_compat};
use super::Resolve;

fn default_true() -> bool {
    true
}

// Directional families: (right, left, down, up).
// Any member of a family is mapped to the appropriate sibling based on direction.
const HEAD_FAMILIES: &[(char, char, char, char)] = &[
    ('▶', '◀', '▼', '▲'),
    ('>', '<', 'v', '^'),
    ('→', '←', '↓', '↑'),
];

/// Given a user-chosen head char, return the variant that points in the
/// horizontal direction (positive_dir=true → right, false → left).
fn head_char_h(ch: char, positive_dir: bool) -> char {
    for &(r, l, d, u) in HEAD_FAMILIES {
        if ch == r || ch == l || ch == d || ch == u {
            return if positive_dir { r } else { l };
        }
    }
    ch // symmetric / unknown: use as-is
}

/// Given a user-chosen head char, return the variant that points in the
/// vertical direction (positive_dir=true → down, false → up).
fn head_char_v(ch: char, positive_dir: bool) -> char {
    for &(r, l, d, u) in HEAD_FAMILIES {
        if ch == r || ch == l || ch == d || ch == u {
            return if positive_dir { d } else { u };
        }
    }
    ch // symmetric / unknown: use as-is
}

/// Return the vertical-segment counterpart of a horizontal body char.
/// e.g. '─' → '│', '═' → '║', everything else → itself.
fn body_char_vertical(ch: char) -> char {
    match ch {
        '─' => '│',
        '═' => '║',
        _   => ch,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arrow {
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub x1: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub y1: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub x2: Coordinate,
    #[serde(deserialize_with = "deserialize_coord_compat")]
    pub y2: Coordinate,
    /// Whether to draw an ASCII arrowhead at (x2, y2).
    #[serde(default = "default_true")]
    pub head: bool,
    /// Custom arrowhead character; None = auto-select based on direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_ch: Option<char>,
    /// Custom body character; None = auto-select (─ or │).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_ch: Option<char>,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

// ---------------------------------------------------------------------------
// Resolve
// ---------------------------------------------------------------------------
//
// Routing strategy: orthogonal (Manhattan) L-shape.
//
//  • Mostly-horizontal (|dx| >= |dy|): H-first  →  ──────────┐
//                                                             │
//                                                             v
//
//  • Mostly-vertical   (|dy| >  |dx|): V-first  →  │
//                                                   │
//                                                   └──────>
//
// Axis-aligned arrows are rendered directly without a bend.
// All body segments use box-drawing chars; arrowheads use >, <, v, ^.

impl Resolve for Arrow {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }

        let x1 = self.x1.evaluate(frame) as i32;
        let y1 = self.y1.evaluate(frame) as i32;
        let x2 = self.x2.evaluate(frame) as i32;
        let y2 = self.y2.evaluate(frame) as i32;

        let dx_abs = (x2 - x1).abs();
        let dy_abs = (y2 - y1).abs();
        let sx = (x2 - x1).signum(); // ±1 or 0
        let sy = (y2 - y1).signum();

        let s = &self.style;
        let z = self.z_order;

        // Emit one cell, skipping off-screen coordinates.
        let emit = |ops: &mut Vec<DrawOp>, x: i32, y: i32, ch: char| {
            if x >= 0 && y >= 0 {
                ops.push(DrawOp { x: x as u16, y: y as u16, ch, style: s.clone(), z_order: z });
            }
        };

        // Single point.
        if dx_abs == 0 && dy_abs == 0 {
            emit(ops, x1, y1, '*');
            return;
        }

        // Head chars: auto-rotate based on direction; custom chars use family tables.
        let h_head = match self.head_ch {
            Some(ch) => head_char_h(ch, sx >= 0),
            None     => if sx >= 0 { '▶' } else { '◀' },
        };
        let v_head = match self.head_ch {
            Some(ch) => head_char_v(ch, sy >= 0),
            None     => if sy >= 0 { '▼' } else { '▲' },
        };
        // Body chars: derive vertical counterpart from horizontal for known pairs.
        let h_body = self.body_ch.unwrap_or('─');
        let v_body = self.body_ch.map(body_char_vertical).unwrap_or('│');

        // ── Pure vertical ──────────────────────────────────────────────────
        if dx_abs == 0 {
            let mut y = y1;
            while y != y2 {
                emit(ops, x1, y, v_body);
                y += sy;
            }
            emit(ops, x1, y2, if self.head { v_head } else { v_body });
            return;
        }

        // ── Pure horizontal ────────────────────────────────────────────────
        if dy_abs == 0 {
            let mut x = x1;
            while x != x2 {
                emit(ops, x, y1, h_body);
                x += sx;
            }
            emit(ops, x2, y1, if self.head { h_head } else { h_body });
            return;
        }

        // ── Diagonal: orthogonal routing ───────────────────────────────────
        if dx_abs >= dy_abs {
            // H-first: long horizontal segment at y1, then short vertical at x2.
            // Arrowhead points in the vertical direction (v / ^).
            //
            //   ─ ─ ─ ─ ─ ─ ─ ─ ┐
            //                    │
            //                    v

            let corner = match (sx, sy) {
                ( 1,  1) => '┐',
                ( 1, -1) => '┘',
                (-1,  1) => '┌',
                _        => '└',
            };

            // Horizontal body: x1 .. x2 (exclusive)
            let mut x = x1;
            while x != x2 {
                emit(ops, x, y1, h_body);
                x += sx;
            }
            // Corner
            emit(ops, x2, y1, corner);
            // Vertical body: y1+sy .. y2 (exclusive)
            let mut y = y1 + sy;
            while y != y2 {
                emit(ops, x2, y, v_body);
                y += sy;
            }
            // Head / tail
            emit(ops, x2, y2, if self.head { v_head } else { v_body });
        } else {
            // V-first: short vertical segment at x1, then long horizontal at y2.
            // Arrowhead points in the horizontal direction (> / <).
            //
            //   │
            //   │
            //   └ ─ ─ ─ ─ ─ ─ ─ >

            let corner = match (sx, sy) {
                ( 1,  1) => '└',
                ( 1, -1) => '┌',
                (-1,  1) => '┘',
                _        => '┐',
            };

            // Vertical body: y1 .. y2 (exclusive)
            let mut y = y1;
            while y != y2 {
                emit(ops, x1, y, v_body);
                y += sy;
            }
            // Corner
            emit(ops, x1, y2, corner);
            // Horizontal body: x1+sx .. x2 (exclusive)
            let mut x = x1 + sx;
            while x != x2 {
                emit(ops, x, y2, h_body);
                x += sx;
            }
            // Head / tail
            emit(ops, x2, y2, if self.head { h_head } else { h_body });
        }
    }
}
