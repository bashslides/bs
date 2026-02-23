use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{FrameRange, Position};
use super::{font, Resolve};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub text: String,
    pub position: Position,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
    /// Fill character used for the large glyphs (default: `█`).
    #[serde(default = "default_header_char")]
    pub ch: char,
}

fn default_header_char() -> char {
    '█'
}

impl Resolve for Header {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);

        let has_bg = self.style.bg.is_some();
        let bg_style = if has_bg {
            Style {
                fg: None,
                bg: self.style.bg.clone(),
                bold: false,
                dim: false,
            }
        } else {
            Style::default()
        };

        let mut cursor_x = base_x;
        for ch in self.text.chars() {
            let upper = ch.to_ascii_uppercase();
            if let Some(glyph) = font::glyph(upper) {
                for (row, line) in glyph.iter().enumerate() {
                    for (col, c) in line.chars().enumerate() {
                        if c != ' ' {
                            ops.push(DrawOp {
                                x: cursor_x + col as u16,
                                y: base_y + row as u16,
                                ch: self.ch,
                                style: self.style.clone(),
                                z_order: self.z_order,
                            });
                        } else if has_bg {
                            ops.push(DrawOp {
                                x: cursor_x + col as u16,
                                y: base_y + row as u16,
                                ch: ' ',
                                style: bg_style.clone(),
                                z_order: self.z_order,
                            });
                        }
                    }
                }
                // Fill the inter-character gap column with bg spaces
                if has_bg {
                    let gap_x = cursor_x + glyph[0].len() as u16;
                    for row in 0..5u16 {
                        ops.push(DrawOp {
                            x: gap_x,
                            y: base_y + row,
                            ch: ' ',
                            style: bg_style.clone(),
                            z_order: self.z_order,
                        });
                    }
                }
                cursor_x += glyph[0].len() as u16 + 1; // +1 inter-character gap
            }
        }
    }
}
