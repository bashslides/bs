use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{FrameRange, Position};
use super::{font, Resolve, ResolveCtx};

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

/// Blank glyph-rows inserted between wrapped header lines.
const LINE_GAP: u16 = 1;

impl Header {
    /// Split the header text into lines that each fit within `avail` glyph
    /// columns, breaking on word boundaries. A word that is wider than `avail`
    /// on its own still gets a line to itself (we never break mid-word).
    fn wrap_lines(&self, avail: u16) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in self.text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if current.is_empty() || font::text_width(&candidate) <= avail {
                current = candidate;
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        lines.push(current);
        lines
    }

    /// Render a single line of glyphs starting at `(base_x, base_y)`.
    fn render_line(&self, text: &str, base_x: u16, base_y: u16, ops: &mut Vec<DrawOp>) {
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
        for ch in text.chars() {
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
                    for row in 0..font::GLYPH_HEIGHT {
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

impl Resolve for Header {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        let (frame, canvas_width) = (ctx.frame, ctx.canvas_width);
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame, ctx.anims);
        let base_y = self.position.y.evaluate(frame, ctx.anims);

        // Glyph columns available from the header's left edge to the right
        // edge of the canvas. Each wrapped line restarts at `base_x`, so the
        // next line drops one glyph height plus a one-row gap below.
        let avail = canvas_width.saturating_sub(base_x);
        let stride = font::GLYPH_HEIGHT + LINE_GAP;
        for (line_idx, line) in self.wrap_lines(avail).iter().enumerate() {
            let line_y = base_y + line_idx as u16 * stride;
            self.render_line(line, base_x, line_y, ops);
        }
    }
}
