use serde::{Deserialize, Serialize};

use crate::types::{Color, DrawOp, NamedColor, Style};

use super::super::source::{AnimSpans, Coordinate, FrameRange, Position, deserialize_coord_compat};
use super::{Resolve, ResolveCtx};

/// Rows past this many can never be visible (each row is at least one cell
/// tall, so row `u16::MAX` already starts beyond any canvas), so layout and
/// rendering only consider this many — a huge `rows` stays cheap. Exact: such a
/// table's natural height already exceeds any explicit `height`, so no row
/// padding is redistributed by the cap.
pub const MAX_ROWS: usize = u16::MAX as usize;

// ---------------------------------------------------------------------------
// Word-wrap helpers
// ---------------------------------------------------------------------------
//
// Table cells wrap with the shared `wrap` helper (no list-continuation indent),
// so the cell wrap and the `Label` wrap can never diverge.

fn wrap_text_line(line: &str, w: usize) -> Vec<Vec<char>> {
    let indexed = super::wrap::wrap_line_indexed(0, line, w, 0);
    super::wrap::indexed_to_chars(line, indexed)
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

/// Like [`wrap_cell_content`], but each cell carries the source char index (into
/// `content`, counting newlines) it displays, or `None` for padding. Used to map
/// the edit caret onto the exact wrapped cell so it can be drawn inverted.
fn wrap_cell_content_indexed(content: &str, w: usize) -> Vec<Vec<Option<usize>>> {
    if w == 0 {
        return vec![Vec::new()];
    }
    let mut result = Vec::new();
    let mut base = 0usize;
    for line in content.split('\n') {
        result.extend(super::wrap::wrap_line_indexed(base, line, w, 0));
        base += line.chars().count() + 1; // +1 for the consumed '\n'
    }
    if result.is_empty() {
        result.push(Vec::new());
    }
    result
}

/// Style for the block cursor: the highlighted character's colors inverted.
fn caret_block_style(st: &Style) -> Style {
    Style {
        fg: st.bg.clone().or(Some(Color::Named(NamedColor::Black))),
        bg: st.fg.clone().or(Some(Color::Named(NamedColor::White))),
        bold: st.bold,
        dim: false,
    }
}

/// Position (wrapped line, column) for a caret that has no glyph to invert —
/// an empty cell, a caret on a newline (the end of its line), the trailing
/// append slot, or a space the wrap dropped. The caret marks the gap *before*
/// its char (insert-before), so it sits just past the previous drawn char of
/// its own logical line, or at column 0 of an empty line.
fn caret_blank_pos(content: &str, w: usize, caret: usize) -> (usize, usize) {
    let mut row0 = 0;
    let mut base = 0;
    for line in content.split('\n') {
        let rows = if w == 0 { vec![Vec::new()] } else { super::wrap::wrap_line_indexed(base, line, w, 0) };
        let end = base + line.chars().count();
        if caret <= end && !rows.is_empty() {
            // Just past the last drawn char before the caret on this line.
            let mut at = (rows.len() - 1, None);
            for (r, row) in rows.iter().enumerate() {
                for (x, src) in row.iter().enumerate() {
                    if src.is_some_and(|i| i < caret) {
                        at = (r, Some(x));
                    }
                }
            }
            let (r, x) = at;
            let len = rows[r].len();
            let col = match x {
                Some(x) if len > 0 => (x + 1).min(len - 1),
                _ => 0,
            };
            return (row0 + r, col);
        }
        row0 += rows.len();
        base = end + 1;
    }
    (0, 0)
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

    /// Resize `cells` so it has exactly `rows` rows and `col_count()` columns
    /// (clamping `rows` to [`MAX_ROWS`] first, so this never materialises an
    /// absurd row count).
    pub fn normalize_cells(&mut self) {
        self.rows = self.rows.min(MAX_ROWS);
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
    /// including its bounding border columns, evaluated at `frame`.  Used by the
    /// editor for highlights.
    ///
    /// When `borders` is on the range spans from the column's left vertical bar
    /// through its right vertical bar (so adjacent columns share a border
    /// column).  When borders are off it is exactly the content range.
    pub fn col_pixel_range(&self, frame: usize, anims: &AnimSpans, col_idx: usize) -> Option<(u16, u16)> {
        let total_w = self.width.evaluate(frame, anims) as usize;
        let (cws, starts) = self.layout(total_w);
        let base_x = usize::from(self.position.x.evaluate(frame, anims));
        let cw = *cws.get(col_idx)?;
        let start = base_x + *starts.get(col_idx)?;
        if self.borders {
            // `start` >= 1 with borders, so the left bar at `start - 1` is in range.
            Some((sat_u16(start - 1), sat_u16(start + cw + 1)))
        } else {
            Some((sat_u16(start), sat_u16(start + cw)))
        }
    }

    /// Returns the pixel y range (inclusive start, exclusive end) of row `row_idx`
    /// including content only (no border row), evaluated at `frame`.
    pub fn row_pixel_range(&self, frame: usize, anims: &AnimSpans, row_idx: usize) -> Option<(u16, u16)> {
        let total_w = self.width.evaluate(frame, anims) as usize;
        let (cws, _) = self.layout(total_w);
        let heights = self.row_heights(frame, anims, &cws);
        let base_y = usize::from(self.position.y.evaluate(frame, anims));
        let mut y = base_y + usize::from(self.borders);
        for r in 0..heights.len() {
            let rh = heights[r];
            if r == row_idx {
                return Some((sat_u16(y), sat_u16(y + rh)));
            }
            y += rh;
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

    /// Total natural (content-fit) height in terminal rows for `frame`,
    /// ignoring any explicit `height`. This is the smallest height the table can
    /// occupy — an explicit `height` only pads beyond it, never clips below. The
    /// editor seeds vertical resizes from this so each step is visible instead of
    /// being swallowed while the requested height is still under the content.
    pub fn natural_height(&self, frame: usize, anims: &AnimSpans) -> u16 {
        let total_width = self.width.evaluate(frame, anims) as usize;
        let (cws, _) = self.layout(total_width);
        let nrows = self.rows.min(MAX_ROWS);
        if nrows == 0 {
            return 0;
        }
        let content: usize = (0..nrows).map(|r| self.row_height(r, &cws)).sum();
        let border_rows = if self.borders { 1 + nrows } else { 0 };
        sat_u16(content + border_rows)
    }

    /// Per-row content heights for `frame`.
    ///
    /// Each row is at least tall enough for its wrapped content. When an
    /// explicit `height` is set (non-zero) and exceeds the natural height, the
    /// surplus is distributed across rows (top to bottom) so the table fills
    /// the requested height. Rows whose content is taller than the budget are
    /// never clipped — an explicit height only pads, it never shrinks.
    fn row_heights(&self, frame: usize, anims: &AnimSpans, col_content_widths: &[usize]) -> Vec<usize> {
        let nrows = self.rows.min(MAX_ROWS);
        let mut heights = vec![1usize; nrows];
        for r in 0..nrows {
            heights[r] = self.row_height(r, col_content_widths);
        }

        let total_height = self.height.evaluate(frame, anims) as usize;
        if total_height > 0 && nrows > 0 {
            // Border rows consumed by the chrome: top border + one separator/
            // bottom border after each row.
            let border_rows = if self.borders { 1 + nrows } else { 0 };
            let content_target = total_height.saturating_sub(border_rows);
            let natural: usize = heights.iter().sum();
            if content_target > natural {
                let mut extra = content_target - natural;
                let mut i = 0usize;
                while extra > 0 {
                    heights[i % nrows] += 1;
                    i += 1;
                    extra -= 1;
                }
            }
        }

        heights
    }
}

// ---------------------------------------------------------------------------
// Standard resolve (for playback / normal editor view)
// ---------------------------------------------------------------------------

impl Resolve for Table {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        self.resolve_internal(ctx, None, &[], false, false, None, ops);
    }
}

impl Table {
    /// Resolve with optional editor overlays:
    /// - `highlighted_col`:  for remove-column preview (that column drawn in red).
    /// - `selected_cells`:   for cell-props mode (those cells in red, others dim).
    /// - `cursor_cell`:      current navigation cursor in cell-props mode.
    /// - `blink_hidden`:     suppress cursor highlight during blink frame.
    /// - `editing_caret`:    `(row, col, char_index)` of the cell being text-edited;
    ///                       that character is drawn inverted (the block cursor).
    pub fn resolve_with_editor_overlay(
        &self,
        ctx: &ResolveCtx,
        highlighted_col: Option<usize>,
        selected_cells: &[(usize, usize)],
        cursor_cell: Option<(usize, usize)>,
        blink_hidden: bool,
        editing_caret: Option<(usize, usize, usize)>,
        ops: &mut Vec<DrawOp>,
    ) {
        // Only dim the table when cells are explicitly selected (Space-toggled).
        // Don't dim just because the navigation cursor is present.
        let cell_mode = !selected_cells.is_empty();
        self.resolve_internal(
            ctx,
            highlighted_col,
            selected_cells,
            cell_mode,
            blink_hidden,
            editing_caret,
            ops,
        );
        // Draw cursor outline on top if in cell-selection mode and not blink-hidden.
        if let Some((cr, cc)) = cursor_cell {
            if !blink_hidden {
                self.draw_cursor_cell(ctx.frame, ctx.anims, cr, cc, ops);
            }
        }
    }

    fn draw_cursor_cell(&self, frame: usize, anims: &AnimSpans, row: usize, col: usize, ops: &mut Vec<DrawOp>) {
        if !self.borders {
            return;
        }
        let total_w = self.width.evaluate(frame, anims) as usize;
        let (cws, starts) = self.layout(total_w);
        let base_x = usize::from(self.position.x.evaluate(frame, anims));
        let base_y = usize::from(self.position.y.evaluate(frame, anims));
        let ncols = self.col_widths.len();

        if col >= cws.len() || row >= self.rows.min(MAX_ROWS) {
            return;
        }
        let cw = cws[col];
        if cw == 0 {
            return;
        }
        let cx = base_x + starts[col];

        // Walk down to find the y offset and height of this row.
        let heights = self.row_heights(frame, anims, &cws);
        let mut y = base_y + 1; // first content row (after top border)
        for r in 0..row {
            y += heights[r] + 1; // +1 for the separator border row
        }
        let rh = heights[row];

        let cursor_style = Style {
            fg: Some(Color::Named(NamedColor::Yellow)),
            bg: None,
            bold: true,
            dim: false,
        };
        let z = self.z_order.saturating_add(100);

        // Border positions surrounding this cell's content area.
        let top_y    = y - 1;
        let bot_y    = y + rh;
        let left_x   = cx - 1; // safe: starts[col] >= 1 when borders=true
        let right_x  = cx + cw;

        // Helper: pick the correct box-drawing corner character.
        // border_row / border_col are indices into the "corner grid" (0..=nrows, 0..=ncols).
        let corner = |br: usize, bc: usize| -> char {
            match (br == 0, br == self.rows, bc == 0, bc == ncols) {
                (true,  _,     true,  _    ) => '┌',
                (true,  _,     _,     true ) => '┐',
                (_,     true,  true,  _    ) => '└',
                (_,     true,  _,     true ) => '┘',
                (true,  _,     _,     _    ) => '┬',
                (_,     true,  _,     _    ) => '┴',
                (_,     _,     true,  _    ) => '├',
                (_,     _,     _,     true ) => '┤',
                _                            => '┼',
            }
        };

        // Top border row
        for bx in left_x..=right_x {
            let ch = if bx == left_x  { corner(row,     col    ) }
                     else if bx == right_x { corner(row,     col + 1) }
                     else                  { '─' };
            ops.extend(DrawOp::at(bx, top_y, ch, cursor_style.clone(), z));
        }

        // Bottom border row
        for bx in left_x..=right_x {
            let ch = if bx == left_x  { corner(row + 1, col    ) }
                     else if bx == right_x { corner(row + 1, col + 1) }
                     else                  { '─' };
            ops.extend(DrawOp::at(bx, bot_y, ch, cursor_style.clone(), z));
        }

        // Left and right vertical bars
        for by in y..y + rh {
            ops.extend(DrawOp::at(left_x, by, '│', cursor_style.clone(), z));
            ops.extend(DrawOp::at(right_x, by, '│', cursor_style.clone(), z));
        }
    }

    fn resolve_internal(
        &self,
        ctx: &ResolveCtx,
        highlighted_col: Option<usize>,
        selected_cells: &[(usize, usize)],
        cell_mode: bool,
        _blink_hidden: bool,
        editing_caret: Option<(usize, usize, usize)>,
        ops: &mut Vec<DrawOp>,
    ) {
        let (frame, anims) = (ctx.frame, ctx.anims);
        if !self.frames.contains(frame) {
            return;
        }
        let base_x = usize::from(self.position.x.evaluate(frame, anims));
        let base_y = usize::from(self.position.y.evaluate(frame, anims));
        let total_width = self.width.evaluate(frame, anims) as usize;
        let ncols = self.col_widths.len();
        let nrows = self.rows.min(MAX_ROWS);
        // Exclusive canvas edges: work stops there (cells beyond are clipped).
        let (vis_w, vis_h) = ctx.visible(base_x, base_y);
        let (max_x, max_y) = (base_x + vis_w, base_y + vis_h);

        if total_width == 0 || ncols == 0 || nrows == 0 {
            return;
        }

        let (col_widths, col_starts) = self.layout(total_width);

        // --- Per-row heights and y offsets ---
        let row_heights = self.row_heights(frame, anims, &col_widths);

        let mut row_y_offsets = vec![0usize; nrows];
        let mut cur_y = usize::from(self.borders);
        for row_idx in 0..nrows {
            row_y_offsets[row_idx] = cur_y;
            cur_y += row_heights[row_idx];
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
                    base_y + row_y_offsets[border_row - 1] + row_heights[border_row - 1]
                };
                if by >= max_y {
                    break;
                }

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
                    ops.extend(DrawOp::at(bx, by, corner_ch, bs, z));
                    bx += 1;

                    // Horizontal dashes
                    let cw = col_widths[ci];
                    let dash_bs = border_style(Some(ci));
                    for i in 0..cw.min(max_x.saturating_sub(bx)) {
                        ops.extend(DrawOp::at(bx + i, by, '─', dash_bs.clone(), z));
                    }
                    bx += cw;
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
                ops.extend(DrawOp::at(bx, by, last_corner, last_bs, z));
            }

            // Vertical bars for each content row
            for row_idx in 0..nrows {
                let ry = base_y + row_y_offsets[row_idx];
                for line in 0..row_heights[row_idx].min(max_y.saturating_sub(ry)) {
                    let ly = ry + line;
                    // Left outer border
                    let lbs = border_style(None);
                    ops.extend(DrawOp::at(base_x, ly, '│', lbs, z));
                    // Column separators
                    for ci in 0..ncols {
                        let bar_x = base_x + col_starts[ci] + col_widths[ci];
                        // The separator belongs conceptually to the column to its left.
                        let bs = border_style(Some(ci));
                        ops.extend(DrawOp::at(bar_x, ly, '│', bs, z));
                    }
                }
            }
        }

        // --- Cell content ---
        for row_idx in 0..nrows {
            let ry = base_y + row_y_offsets[row_idx];
            if ry >= max_y {
                break;
            }
            for col_idx in 0..ncols {
                let cw = col_widths[col_idx];
                if cw == 0 {
                    continue;
                }
                let cx = base_x + col_starts[col_idx];
                let content = self
                    .cells
                    .get(row_idx)
                    .and_then(|r| r.get(col_idx))
                    .map(|c| c.content.as_str())
                    .unwrap_or("");

                let st = cell_style(row_idx, col_idx);

                // The cell being text-edited draws a block cursor: the caret
                // character is rendered with inverted colors (and an empty/append
                // caret as an inverted blank), so it reads like a normal cursor.
                let caret = editing_caret.and_then(|(r, c, pos)| {
                    if r == row_idx && c == col_idx {
                        Some(pos)
                    } else {
                        None
                    }
                });

                if let Some(caret) = caret {
                    let chars: Vec<char> = content.chars().collect();
                    let idx_grid = wrap_cell_content_indexed(content, cw);
                    let mut drawn = false;
                    for (line_idx, row_src) in idx_grid.iter().enumerate() {
                        let ly = ry + line_idx;
                        for (xi, src) in row_src.iter().enumerate() {
                            let is_caret = *src == Some(caret);
                            let ch = src.map(|i| chars[i]).unwrap_or(' ');
                            if is_caret {
                                ops.extend(DrawOp::at(
                                    cx + xi,
                                    ly,
                                    ch,
                                    caret_block_style(&st),
                                    z,
                                ));
                                drawn = true;
                            } else if ch != ' ' || st.bg.is_some() {
                                ops.extend(DrawOp::at(
                                    cx + xi,
                                    ly,
                                    ch,
                                    st.clone(),
                                    z,
                                ));
                            }
                        }
                    }
                    // No glyph at the caret (empty cell, a newline, or the trailing
                    // append slot): draw an inverted blank where typing continues.
                    if !drawn {
                        let (bly, bxi) = caret_blank_pos(content, cw, caret);
                        ops.extend(DrawOp::at(
                            cx + bxi,
                            ry + bly,
                            ' ',
                            caret_block_style(&st),
                            z,
                        ));
                    }
                    continue;
                }

                let wrapped = wrap_cell_content(content, cw);
                for (line_idx, row_chars) in wrapped.iter().enumerate() {
                    let ly = ry + line_idx;
                    for (xi, &ch) in row_chars.iter().enumerate() {
                        if ch != ' ' || st.bg.is_some() {
                            ops.extend(DrawOp::at(
                                cx + xi,
                                ly,
                                ch,
                                st.clone(),
                                z,
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// `v` as a `u16`, saturating at `u16::MAX` (past the edge of any canvas).
fn sat_u16(v: usize) -> u16 {
    u16::try_from(v).unwrap_or(u16::MAX)
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

#[cfg(test)]
mod wrap_tests {
    use super::*;

    // The indexed wrap must place each glyph at the same cell as the plain wrap,
    // and report the source char index that produced it (None for padding).
    #[test]
    fn indexed_wrap_matches_plain_wrap_and_maps_indices() {
        let content = "abcd";
        let plain = wrap_cell_content(content, 3);
        let indexed = wrap_cell_content_indexed(content, 3);
        let chars: Vec<char> = content.chars().collect();

        // Same shape.
        assert_eq!(plain.len(), indexed.len());
        for (prow, irow) in plain.iter().zip(indexed.iter()) {
            assert_eq!(prow.len(), irow.len());
            for (pc, ic) in prow.iter().zip(irow.iter()) {
                let mapped = ic.map(|i| chars[i]).unwrap_or(' ');
                assert_eq!(*pc, mapped);
            }
        }
        // "abcd" / width 3 wraps to ["abc","d  "]; index 3 ('d') is row 1 col 0.
        assert_eq!(indexed[1][0], Some(3));
        assert_eq!(indexed[1][1], None);
    }

    // Newlines are consumed by wrapping but still advance the source index, so a
    // caret on the first char of the second line maps to the right index.
    #[test]
    fn indexed_wrap_counts_newlines_in_source_offsets() {
        let content = "ab\ncd"; // indices: a0 b1 \n2 c3 d4
        let indexed = wrap_cell_content_indexed(content, 5);
        assert_eq!(indexed.len(), 2);
        assert_eq!(indexed[0][0], Some(0)); // 'a'
        assert_eq!(indexed[0][1], Some(1)); // 'b'
        assert_eq!(indexed[1][0], Some(3)); // 'c' — newline (2) skipped
        assert_eq!(indexed[1][1], Some(4)); // 'd'
    }

    // A trailing newline opens an empty wrapped row; the append caret lands at
    // its column 0 (a freshly opened line), not after the previous text.
    #[test]
    fn caret_blank_pos_opens_a_new_line_after_trailing_newline() {
        let content = "abc\n";
        let len = content.chars().count();
        assert_eq!(caret_blank_pos(content, 5, len), (1, 0));
    }

    // A caret on a mid-text newline is the end of *its* line (insert-before),
    // and one on an empty line is that line's column 0 — not the last row.
    #[test]
    fn caret_blank_pos_on_a_newline_ends_its_own_line() {
        let content = "ab\n\ncd"; // a0 b1 \n2 \n3 c4 d5
        assert_eq!(caret_blank_pos(content, 5, 2), (0, 2)); // after "ab"
        assert_eq!(caret_blank_pos(content, 5, 3), (1, 0)); // the empty line
        assert_eq!(caret_blank_pos(content, 5, 6), (2, 2)); // append after "cd"
    }
}
