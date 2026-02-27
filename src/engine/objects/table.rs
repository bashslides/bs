use serde::{Deserialize, Serialize};

use crate::types::{Color, DrawOp, NamedColor, Style};

use super::super::source::{Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::Resolve;

// ---------------------------------------------------------------------------
// Word-wrap helper (mirrors Label's logic, without list-continuation indent)
// ---------------------------------------------------------------------------

fn wrap_text_line(line: &str, w: usize) -> Vec<Vec<char>> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() || w == 0 {
        return vec![Vec::new()];
    }
    let mut rows: Vec<Vec<char>> = Vec::new();
    let mut pos = 0usize;
    while pos < chars.len() {
        let remaining = &chars[pos..];
        if remaining.len() <= w {
            let mut row = vec![' '; w];
            for (i, &ch) in remaining.iter().enumerate() {
                row[i] = ch;
            }
            rows.push(row);
            break;
        }
        let chunk = &remaining[..w];
        let (row_len, advance) = match chunk.iter().rposition(|&c| c == ' ') {
            Some(sp) => (sp, sp + 1),
            None => (w, w),
        };
        let mut row = vec![' '; w];
        for (i, &ch) in remaining[..row_len].iter().enumerate() {
            row[i] = ch;
        }
        rows.push(row);
        pos += advance;
        while pos < chars.len() && chars[pos] == ' ' {
            pos += 1;
        }
    }
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

fn wrap_cell_content(content: &str, w: usize) -> Vec<Vec<char>> {
    if w == 0 {
        return vec![Vec::new()];
    }
    let mut result = Vec::new();
    for line in content.split('\n') {
        result.extend(wrap_text_line(line, w));
    }
    if result.is_empty() {
        result.push(Vec::new());
    }
    result
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TableCell {
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
}

fn default_table_width() -> Coordinate {
    Coordinate::Fixed(30.0)
}
fn default_table_height() -> Coordinate {
    Coordinate::Fixed(0.0)
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub position: Position,
    #[serde(default = "default_table_width", deserialize_with = "deserialize_coord_compat")]
    pub width: Coordinate,
    /// When 0, height is computed automatically from cell content.
    #[serde(default = "default_table_height", deserialize_with = "deserialize_coord_compat")]
    pub height: Coordinate,
    /// Fractional column widths; each value is in [0..1] and they should sum to ~1.0.
    pub col_widths: Vec<f64>,
    /// Number of rows in the table.
    pub rows: usize,
    /// Cell data: `cells[row][col]`.  Automatically extended to rows×col_count.
    #[serde(default)]
    pub cells: Vec<Vec<TableCell>>,
    /// Render the first row in bold.
    #[serde(default)]
    pub header_bold: bool,
    /// Draw box-drawing borders around every cell.
    #[serde(default = "default_true")]
    pub borders: bool,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Table {
    pub fn col_count(&self) -> usize {
        self.col_widths.len()
    }

    /// Resize `cells` so it has exactly `rows` rows and `col_count()` columns.
    pub fn normalize_cells(&mut self) {
        let ncols = self.col_widths.len();
        self.cells.resize_with(self.rows, Vec::new);
        for row in &mut self.cells {
            row.resize_with(ncols, TableCell::default);
        }
    }

    /// Compute (col_content_widths, col_x_starts) for a given total table pixel width.
    ///
    /// `col_content_widths[i]` is the number of terminal columns available for text in column i.
    /// `col_x_starts[i]`       is the x offset from the table's left edge to column i's content.
    pub fn layout(&self, total_width: usize) -> (Vec<usize>, Vec<usize>) {
        let ncols = self.col_widths.len();
        if ncols == 0 || total_width == 0 {
            return (vec![], vec![]);
        }
        // Available characters for cell content (exclude vertical border chars).
        let avail = if self.borders {
            total_width.saturating_sub(ncols + 1)
        } else {
            total_width
        };

        // Distribute avail proportionally; last column absorbs rounding error.
        let mut col_content_widths = Vec::with_capacity(ncols);
        let mut used = 0usize;
        for (i, &frac) in self.col_widths.iter().enumerate() {
            let w = if i + 1 == ncols {
                avail.saturating_sub(used)
            } else {
                let raw = (avail as f64 * frac).floor() as usize;
                raw.min(avail.saturating_sub(used))
            };
            col_content_widths.push(w);
            used += w;
        }

        // x offsets of each column's content within the table.
        let mut col_x_starts = Vec::with_capacity(ncols);
        let mut x = if self.borders { 1usize } else { 0usize };
        for (i, &cw) in col_content_widths.iter().enumerate() {
            col_x_starts.push(x);
            x += cw;
            if self.borders && i + 1 < ncols {
                x += 1; // column separator bar
            }
        }

        (col_content_widths, col_x_starts)
    }

    /// Returns the pixel x range (inclusive start, exclusive end) of column `col_idx`
    /// including its border, evaluated at `frame`.  Used by the editor for highlights.
    pub fn col_pixel_range(&self, frame: usize, col_idx: usize) -> Option<(u16, u16)> {
        let total_w = self.width.evaluate(frame) as usize;
        let (cws, starts) = self.layout(total_w);
        let base_x = self.position.x.evaluate(frame);
        let cw = *cws.get(col_idx)?;
        let start = *starts.get(col_idx)?;
        Some((base_x + start as u16, base_x + start as u16 + cw as u16))
    }

    /// Returns the pixel y range (inclusive start, exclusive end) of row `row_idx`
    /// including content only (no border row), evaluated at `frame`.
    pub fn row_pixel_range(&self, frame: usize, row_idx: usize) -> Option<(u16, u16)> {
        let total_w = self.width.evaluate(frame) as usize;
        let (cws, _) = self.layout(total_w);
        let base_y = self.position.y.evaluate(frame);
        let mut y = if self.borders { base_y + 1 } else { base_y };
        for r in 0..self.rows {
            let rh = self.row_height(r, &cws);
            if r == row_idx {
                return Some((y, y + rh as u16));
            }
            y += rh as u16;
            if self.borders {
                y += 1; // separator row
            }
        }
        None
    }

    fn row_height(&self, row_idx: usize, col_content_widths: &[usize]) -> usize {
        let mut max_lines = 1usize;
        for (col_idx, &cw) in col_content_widths.iter().enumerate() {
            if cw == 0 {
                continue;
            }
            let content = self
                .cells
                .get(row_idx)
                .and_then(|r| r.get(col_idx))
                .map(|c| c.content.as_str())
                .unwrap_or("");
            let lines = wrap_cell_content(content, cw).len();
            if lines > max_lines {
                max_lines = lines;
            }
        }
        max_lines
    }
}

// ---------------------------------------------------------------------------
// Standard resolve (for playback / normal editor view)
// ---------------------------------------------------------------------------

impl Resolve for Table {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        self.resolve_internal(frame, None, &[], false, false, ops);
    }
}

impl Table {
    /// Resolve with optional editor overlays:
    /// - `highlighted_col`:  for remove-column preview (that column drawn in red).
    /// - `selected_cells`:   for cell-props mode (those cells in red, others dim).
    /// - `cursor_cell`:      current navigation cursor in cell-props mode.
    /// - `blink_hidden`:     suppress cursor highlight during blink frame.
    pub fn resolve_with_editor_overlay(
        &self,
        frame: usize,
        highlighted_col: Option<usize>,
        selected_cells: &[(usize, usize)],
        cursor_cell: Option<(usize, usize)>,
        blink_hidden: bool,
        ops: &mut Vec<DrawOp>,
    ) {
        let cell_mode = cursor_cell.is_some() || !selected_cells.is_empty();
        self.resolve_internal(
            frame,
            highlighted_col,
            selected_cells,
            cell_mode,
            blink_hidden,
            ops,
        );
        // Draw cursor outline on top if in cell-selection mode and not blink-hidden.
        if let Some((cr, cc)) = cursor_cell {
            if !blink_hidden {
                self.draw_cursor_cell(frame, cr, cc, ops);
            }
        }
    }

    fn draw_cursor_cell(&self, frame: usize, row: usize, col: usize, ops: &mut Vec<DrawOp>) {
        let total_w = self.width.evaluate(frame) as usize;
        let (cws, starts) = self.layout(total_w);
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);

        if col >= cws.len() || row >= self.rows {
            return;
        }
        let cw = cws[col];
        let cx = base_x + starts[col] as u16;

        let mut y = if self.borders { base_y + 1 } else { base_y };
        for r in 0..self.rows {
            let rh = self.row_height(r, &cws);
            if r == row {
                // Draw a bright white underline beneath every cell content line.
                let cursor_style = Style {
                    fg: Some(Color::Named(NamedColor::White)),
                    bg: None,
                    bold: true,
                    dim: false,
                };
                for line in 0..rh as u16 {
                    let ly = y + line;
                    for col_x in 0..cw as u16 {
                        // Only emit if this Op's position would overwrite the content.
                        // Use a very high z_order to sit on top.
                        ops.push(DrawOp {
                            x: cx + col_x,
                            y: ly,
                            ch: ' ', // space means "show bg only"
                            style: cursor_style.clone(),
                            z_order: self.z_order + 100,
                        });
                    }
                }
                break;
            }
            y += rh as u16;
            if self.borders {
                y += 1;
            }
        }
    }

    fn resolve_internal(
        &self,
        frame: usize,
        highlighted_col: Option<usize>,
        selected_cells: &[(usize, usize)],
        cell_mode: bool,
        _blink_hidden: bool,
        ops: &mut Vec<DrawOp>,
    ) {
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);
        let total_width = self.width.evaluate(frame) as usize;
        let ncols = self.col_widths.len();
        let nrows = self.rows;

        if total_width == 0 || ncols == 0 || nrows == 0 {
            return;
        }

        let (col_widths, col_starts) = self.layout(total_width);

        // --- Per-row heights and y offsets ---
        let mut row_heights = vec![1usize; nrows];
        for row_idx in 0..nrows {
            row_heights[row_idx] = self.row_height(row_idx, &col_widths);
        }

        let mut row_y_offsets = vec![0u16; nrows];
        let mut cur_y = if self.borders { 1u16 } else { 0u16 };
        for row_idx in 0..nrows {
            row_y_offsets[row_idx] = cur_y;
            cur_y += row_heights[row_idx] as u16;
            if self.borders {
                cur_y += 1; // row-separator line
            }
        }

        // Helper: pick style for a given (row, col) cell.
        let cell_style = |row_idx: usize, col_idx: usize| -> Style {
            let base = self
                .cells
                .get(row_idx)
                .and_then(|r| r.get(col_idx))
                .and_then(|c| c.style.as_ref())
                .unwrap_or(&self.style);

            let is_header = row_idx == 0 && self.header_bold;
            let is_selected = selected_cells.contains(&(row_idx, col_idx));
            let in_highlighted_col = highlighted_col == Some(col_idx);

            if in_highlighted_col || is_selected {
                // Editor-mode highlight: red text
                Style {
                    fg: Some(Color::Named(NamedColor::Red)),
                    bg: base.bg.clone(),
                    bold: is_header || base.bold,
                    dim: false,
                }
            } else if cell_mode {
                // In cell mode all non-selected cells are dimmed white
                Style {
                    fg: Some(Color::Named(NamedColor::White)),
                    bg: None,
                    bold: false,
                    dim: true,
                }
            } else if is_header {
                Style {
                    fg: base.fg.clone(),
                    bg: base.bg.clone(),
                    bold: true,
                    dim: base.dim,
                }
            } else {
                base.clone()
            }
        };

        // Style for borders
        let border_style = |col_idx_maybe: Option<usize>| -> Style {
            let in_highlighted = highlighted_col.is_some()
                && col_idx_maybe.is_some()
                && col_idx_maybe == highlighted_col;
            // In cell-selection mode borders are dim white (same as non-selected cells)
            if cell_mode && !in_highlighted {
                Style {
                    fg: Some(Color::Named(NamedColor::White)),
                    bg: None,
                    bold: false,
                    dim: true,
                }
            } else if in_highlighted {
                Style {
                    fg: Some(Color::Named(NamedColor::Red)),
                    bg: None,
                    bold: false,
                    dim: false,
                }
            } else {
                self.style.clone()
            }
        };

        let z = self.z_order;

        // --- Borders ---
        if self.borders {
            // Horizontal border lines: top (row_idx=0), between rows, bottom (row_idx=nrows)
            for border_row in 0..=nrows {
                let by = if border_row == 0 {
                    base_y
                } else {
                    base_y + row_y_offsets[border_row - 1] + row_heights[border_row - 1] as u16
                };

                let mut bx = base_x;
                for ci in 0..ncols {
                    let corner_ch = if border_row == 0 {
                        if ci == 0 { '┌' } else { '┬' }
                    } else if border_row == nrows {
                        if ci == 0 { '└' } else { '┴' }
                    } else {
                        if ci == 0 { '├' } else { '┼' }
                    };
                    let bs = border_style(Some(ci).filter(|_| ci > 0));
                    ops.push(DrawOp { x: bx, y: by, ch: corner_ch, style: bs, z_order: z });
                    bx += 1;

                    // Horizontal dashes
                    let cw = col_widths[ci];
                    let dash_bs = border_style(Some(ci));
                    for _ in 0..cw {
                        ops.push(DrawOp { x: bx, y: by, ch: '─', style: dash_bs.clone(), z_order: z });
                        bx += 1;
                    }
                }
                // Rightmost corner
                let last_corner = if border_row == 0 {
                    '┐'
                } else if border_row == nrows {
                    '┘'
                } else {
                    '┤'
                };
                let last_bs = border_style(Some(ncols - 1));
                ops.push(DrawOp { x: bx, y: by, ch: last_corner, style: last_bs, z_order: z });
            }

            // Vertical bars for each content row
            for row_idx in 0..nrows {
                let ry = base_y + row_y_offsets[row_idx];
                for line in 0..row_heights[row_idx] as u16 {
                    let ly = ry + line;
                    // Left outer border
                    let lbs = border_style(None);
                    ops.push(DrawOp { x: base_x, y: ly, ch: '│', style: lbs, z_order: z });
                    // Column separators
                    for ci in 0..ncols {
                        let bar_x = base_x + col_starts[ci] as u16 + col_widths[ci] as u16;
                        // The separator belongs conceptually to the column to its left.
                        let bs = border_style(Some(ci));
                        ops.push(DrawOp { x: bar_x, y: ly, ch: '│', style: bs, z_order: z });
                    }
                }
            }
        }

        // --- Cell content ---
        for row_idx in 0..nrows {
            let ry = base_y + row_y_offsets[row_idx];
            for col_idx in 0..ncols {
                let cw = col_widths[col_idx];
                if cw == 0 {
                    continue;
                }
                let cx = base_x + col_starts[col_idx] as u16;
                let content = self
                    .cells
                    .get(row_idx)
                    .and_then(|r| r.get(col_idx))
                    .map(|c| c.content.as_str())
                    .unwrap_or("");

                let st = cell_style(row_idx, col_idx);
                let wrapped = wrap_cell_content(content, cw);
                for (line_idx, row_chars) in wrapped.iter().enumerate() {
                    let ly = ry + line_idx as u16;
                    for (xi, &ch) in row_chars.iter().enumerate() {
                        if ch != ' ' || st.bg.is_some() {
                            ops.push(DrawOp {
                                x: cx + xi as u16,
                                y: ly,
                                ch,
                                style: st.clone(),
                                z_order: z,
                            });
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Column / row management helpers (called from properties and input modules)
// ---------------------------------------------------------------------------

/// Add a column at position `insert_idx` (0-indexed).
/// Existing column fractions are scaled down proportionally and the new column
/// gets an equal share.
pub fn table_add_column(table: &mut Table, insert_idx: usize) {
    let n = table.col_widths.len();
    let new_n = n + 1;
    let new_frac = 1.0 / new_n as f64;
    let scale = n as f64 / new_n as f64;
    for w in &mut table.col_widths {
        *w *= scale;
    }
    let insert_idx = insert_idx.min(n);
    table.col_widths.insert(insert_idx, new_frac);
    for row in &mut table.cells {
        if row.len() < n {
            row.resize_with(n, TableCell::default);
        }
        row.insert(insert_idx, TableCell::default());
    }
    table.normalize_cells();
}

/// Remove column at `col_idx` (0-indexed).
/// Remaining column fractions are scaled up proportionally.
pub fn table_remove_column(table: &mut Table, col_idx: usize) {
    let n = table.col_widths.len();
    if n <= 1 || col_idx >= n {
        return;
    }
    let removed_frac = table.col_widths[col_idx];
    table.col_widths.remove(col_idx);
    let remaining = 1.0 - removed_frac;
    if remaining > 0.001 {
        let scale = 1.0 / remaining;
        for w in &mut table.col_widths {
            *w *= scale;
        }
    } else {
        // Edge case: redistribute equally
        let m = table.col_widths.len() as f64;
        for w in &mut table.col_widths {
            *w = 1.0 / m;
        }
    }
    for row in &mut table.cells {
        if col_idx < row.len() {
            row.remove(col_idx);
        }
    }
}
