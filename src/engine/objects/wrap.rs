//! Shared word-wrap used by `Label` and `Table` cells.
//!
//! Wrapping happens only at space characters; the space at the break point is
//! consumed so a wrapped row never starts with an accidental leading space.
//! When no space fits within the available width the line is hard-broken. On
//! the first row a space inside the line's leading indentation, its list
//! marker (the first `indent` cells) or the spaces after it is not a break point — breaking there
//! would strand the bullet or emit a blank row — so the row hard-breaks instead.
//!
//! [`wrap_line_indexed`] is the single source of truth: it returns, per visual
//! row, one slot per cell carrying the source character index it displays (or
//! `None` for padding). Callers that only want glyphs map the indices back to
//! chars via [`indexed_to_chars`]; callers that need to place a caret keep the
//! indices. Deriving both from one function means the visible glyphs and their
//! source indices can never drift apart — and `Label` and `Table` can no longer
//! grow divergent copies of the wrap algorithm.

/// Wrap one logical line to `w` cells wide.
///
/// `base` is added to every emitted index so a multi-line caller can keep a
/// running offset across lines. `indent` is the number of leading pad cells
/// applied to every row *after* the first — used for hanging list-item
/// continuation; pass `0` for no indent.
///
/// Returns one row per visual line, each of length `w`. An empty input line or
/// `w == 0` yields a single empty row.
pub fn wrap_line_indexed(
    base: usize,
    line: &str,
    w: usize,
    indent: usize,
) -> Vec<Vec<Option<usize>>> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() || w == 0 {
        return vec![Vec::new()];
    }
    let mut rows: Vec<Vec<Option<usize>>> = Vec::new();
    let mut pos = 0usize;
    let mut first = true;
    // First-row cells that are indentation or list marker (and the spaces after
    // it), not content: a soft break must leave at least one content cell after
    // them on the row.
    let lead = chars.iter().take_while(|&&c| c == ' ').count().max(indent);
    let protect = lead + chars.iter().skip(lead).take_while(|&&c| c == ' ').count();

    while pos < chars.len() {
        let first_row = first;
        let col0 = if first { 0 } else { indent.min(w.saturating_sub(1)) };
        first = false;
        let avail = w - col0;

        let remaining = &chars[pos..];
        if remaining.len() <= avail {
            // Everything fits on this row.
            let mut row = vec![None; w];
            for i in 0..remaining.len() {
                row[col0 + i] = Some(base + pos + i);
            }
            rows.push(row);
            break;
        }

        // Find the last space within the available width for a soft break.
        let chunk = &remaining[..avail];
        let soft = chunk.iter().rposition(|&c| c == ' ');
        let (row_len, advance) = match soft {
            Some(sp) if !first_row || sp > protect => (sp, sp + 1), // break before the space, skip it
            _ => (avail, avail), // hard break
        };

        let mut row = vec![None; w];
        for i in 0..row_len {
            row[col0 + i] = Some(base + pos + i);
        }
        rows.push(row);
        pos += advance;

        // Skip any additional leading spaces so the next row starts on a word.
        while pos < chars.len() && chars[pos] == ' ' {
            pos += 1;
        }
    }

    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

/// Map an indexed wrap of a single line back to glyph rows, substituting `' '`
/// for padding cells. Only valid when the indices point into `line` (i.e. the
/// wrap was produced with `base == 0` for this same line).
pub fn indexed_to_chars(line: &str, grid: Vec<Vec<Option<usize>>>) -> Vec<Vec<char>> {
    let chars: Vec<char> = line.chars().collect();
    grid.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|idx| idx.map(|i| chars[i]).unwrap_or(' '))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_longer_than_the_width_is_hard_broken() {
        // No spaces to break on, so the word is split at the width boundary.
        let chars = indexed_to_chars("abcdef", wrap_line_indexed(0, "abcdef", 3, 0));
        assert_eq!(chars.len(), 2);
        assert_eq!(chars[0], vec!['a', 'b', 'c']);
        assert_eq!(chars[1], vec!['d', 'e', 'f']);
    }

    #[test]
    fn a_long_first_word_never_strands_the_marker_or_drops_the_indent() {
        let rows = |line: &str, indent| -> Vec<String> {
            indexed_to_chars(line, wrap_line_indexed(0, line, 10, indent))
                .into_iter()
                .map(|r| r.into_iter().collect::<String>().trim_end().to_string())
                .collect()
        };
        assert_eq!(rows("- averylongword", 2), ["- averylon", "  gword"]);
        assert_eq!(rows("1. averylongword", 3), ["1. averylo", "   ngword"]);
        assert_eq!(rows("    /usr/local/bin/x", 0), ["    /usr/l", "ocal/bin/x"]);
        // A break after real content is still a soft break.
        assert_eq!(rows("- ab averylongword", 2), ["- ab", "  averylon", "  gword"]);
    }

    #[test]
    fn continuation_indent_is_clamped_below_the_width() {
        // indent (5) >= width (2): continuation rows clamp the indent to w-1 so
        // at least one column is always available for text.
        let chars = indexed_to_chars("abcd", wrap_line_indexed(0, "abcd", 2, 5));
        assert_eq!(chars[0], vec!['a', 'b']);
        assert_eq!(chars[1], vec![' ', 'c']);
        assert_eq!(chars[2], vec![' ', 'd']);
    }
}
