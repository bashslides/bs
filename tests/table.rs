//! Tests for the table object — the most complex element and the one with a
//! known history of bugs.
//!
//! Two layers are covered:
//!   * pure layout/column math (`layout`, `normalize_cells`, add/remove column)
//!   * end-to-end rendering through the compile -> render pipeline (borders,
//!     content placement, header styling).

mod common;

use ascii_presenter::engine::objects::table::{table_add_column, table_remove_column, Table};
use ascii_presenter::types::Frame;
use common::{char_at, frame_lines, render_json};

// ---------------------------------------------------------------------------
// Construction helper
// ---------------------------------------------------------------------------

/// Deserialize a bare `Table` (not wrapped in a `SceneObject`) from JSON so we
/// get all the serde defaults (borders = true, width = 30, etc.).
fn table_from_json(json: &str) -> Table {
    serde_json::from_str(json).expect("table JSON should parse")
}

// ---------------------------------------------------------------------------
// Pure layout math
// ---------------------------------------------------------------------------

#[test]
fn layout_splits_two_even_columns_and_reserves_border_columns() {
    // width 13, borders on, two equal columns.
    // avail = 13 - (ncols + 1) = 10 -> 5 + 5.
    // content x-starts begin at 1 (left border) with a 1-col separator between.
    let t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 13, "col_widths": [0.5, 0.5], "rows": 1,
             "frames": { "start": 0, "end": 1 } }"#,
    );

    let (widths, starts) = t.layout(13);
    assert_eq!(widths, vec![5, 5]);
    assert_eq!(starts, vec![1, 7]);
}

#[test]
fn layout_gives_rounding_remainder_to_the_last_column() {
    // width 20, borders on, [0.25, 0.25, 0.5].
    // avail = 16 -> floor(4), floor(4), remainder 8.
    let t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 20, "col_widths": [0.25, 0.25, 0.5], "rows": 1,
             "frames": { "start": 0, "end": 1 } }"#,
    );

    let (widths, starts) = t.layout(20);
    assert_eq!(widths, vec![4, 4, 8]);
    assert_eq!(starts, vec![1, 6, 11]);
    // Content widths sum to the available space (total - borders).
    assert_eq!(widths.iter().sum::<usize>(), 16);
}

#[test]
fn layout_without_borders_uses_the_full_width() {
    let t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 10, "col_widths": [0.5, 0.5], "rows": 1, "borders": false,
             "frames": { "start": 0, "end": 1 } }"#,
    );

    let (widths, starts) = t.layout(10);
    assert_eq!(widths, vec![5, 5]);
    assert_eq!(starts, vec![0, 5]);
}

// ---------------------------------------------------------------------------
// Cell normalization and column management
// ---------------------------------------------------------------------------

#[test]
fn normalize_cells_fills_a_full_rows_by_cols_grid() {
    let mut t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "col_widths": [0.5, 0.5], "rows": 3, "cells": [],
             "frames": { "start": 0, "end": 1 } }"#,
    );

    t.normalize_cells();
    assert_eq!(t.cells.len(), 3, "one entry per row");
    assert!(t.cells.iter().all(|r| r.len() == 2), "each row padded to col_count");
}

#[test]
fn add_column_rescales_fractions_and_widens_every_row() {
    let mut t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "col_widths": [0.5, 0.5], "rows": 2,
             "cells": [
                 [{ "content": "a" }, { "content": "b" }],
                 [{ "content": "c" }, { "content": "d" }]
             ],
             "frames": { "start": 0, "end": 1 } }"#,
    );

    table_add_column(&mut t, 1);

    assert_eq!(t.col_count(), 3);
    // Fractions stay normalized and even.
    assert!((t.col_widths.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!(t.col_widths.iter().all(|w| (w - 1.0 / 3.0).abs() < 1e-9));
    // Every row gained a cell; row count is unchanged.
    assert_eq!(t.cells.len(), 2);
    assert!(t.cells.iter().all(|r| r.len() == 3));
    // The new (empty) cell landed at the insert index.
    assert_eq!(t.cells[0][1].content, "");
    assert_eq!(t.cells[0][2].content, "b");
}

#[test]
fn remove_column_rescales_remaining_fractions() {
    let mut t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "col_widths": [0.25, 0.25, 0.5], "rows": 1,
             "cells": [[{ "content": "a" }, { "content": "b" }, { "content": "c" }]],
             "frames": { "start": 0, "end": 1 } }"#,
    );

    table_remove_column(&mut t, 0);

    assert_eq!(t.col_count(), 2);
    assert!((t.col_widths.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    // 0.25 and 0.5 rescaled by 1/0.75 -> 1/3 and 2/3.
    assert!((t.col_widths[0] - 1.0 / 3.0).abs() < 1e-9);
    assert!((t.col_widths[1] - 2.0 / 3.0).abs() < 1e-9);
    // The cell under the removed column is gone.
    assert_eq!(t.cells[0].len(), 2);
    assert_eq!(t.cells[0][0].content, "b");
}

#[test]
fn remove_column_is_a_noop_on_a_single_column_table() {
    let mut t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "col_widths": [1.0], "rows": 1,
             "frames": { "start": 0, "end": 1 } }"#,
    );

    table_remove_column(&mut t, 0);
    assert_eq!(t.col_count(), 1, "a table must keep at least one column");
}

// ---------------------------------------------------------------------------
// End-to-end rendering
// ---------------------------------------------------------------------------

#[test]
fn bordered_table_draws_a_box_with_centered_content() {
    // 1x1 table, width 6, borders on. layout(6): avail = 4 -> one column of 4,
    // content starts at x=1. Expected 3-row box:
    //   ┌────┐
    //   │Hi  │
    //   └────┘
    let p = render_json(
        r#"{
            "width": 6, "height": 3, "frame_count": 1,
            "objects": [
                { "type": "table",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 6, "col_widths": [1.0], "rows": 1,
                  "cells": [[{ "content": "Hi" }]],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), '┌');
    assert_eq!(char_at(&p, 0, 5, 0), '┐');
    assert_eq!(char_at(&p, 0, 0, 1), '│');
    assert_eq!(char_at(&p, 0, 1, 1), 'H');
    assert_eq!(char_at(&p, 0, 2, 1), 'i');
    assert_eq!(char_at(&p, 0, 5, 1), '│');
    assert_eq!(char_at(&p, 0, 0, 2), '└');
    assert_eq!(char_at(&p, 0, 5, 2), '┘');
}

#[test]
fn borderless_table_renders_only_content() {
    let p = render_json(
        r#"{
            "width": 6, "height": 2, "frame_count": 1,
            "objects": [
                { "type": "table",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 6, "col_widths": [1.0], "rows": 1, "borders": false,
                  "cells": [[{ "content": "Hi" }]],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), 'H');
    assert_eq!(char_at(&p, 0, 1, 0), 'i');
    // No box-drawing characters anywhere.
    let lines = frame_lines(&p, 0);
    assert!(
        lines.iter().all(|l| !l.contains('│') && !l.contains('┌') && !l.contains('─')),
        "borderless table should not draw any border glyphs"
    );
}

#[test]
fn header_bold_makes_only_the_first_row_bold() {
    // 2x1 bordered table. Content rows land at y=1 (header) and y=3.
    let p = render_json(
        r#"{
            "width": 6, "height": 5, "frame_count": 1,
            "objects": [
                { "type": "table",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 6, "col_widths": [1.0], "rows": 2, "header_bold": true,
                  "cells": [[{ "content": "A" }], [{ "content": "B" }]],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    let Frame::Full { cells } = &p.frames[0] else {
        panic!("first frame must be Full");
    };
    // Header glyph 'A' at (1, 1) is bold; body glyph 'B' at (1, 3) is not.
    assert_eq!(cells[1][1].ch, 'A');
    assert!(cells[1][1].style.bold, "header row should be bold");
    assert_eq!(cells[3][1].ch, 'B');
    assert!(!cells[3][1].style.bold, "body row should not be bold");
}

// ---------------------------------------------------------------------------
// Explicit height: pad short tables, never clip tall content
// ---------------------------------------------------------------------------

#[test]
fn explicit_height_pads_a_short_table() {
    // 1x1 table whose content needs 1 row. Natural bordered height is 3, but
    // height=5 is requested, so the single row is padded to fill it: the
    // bottom border moves from y=2 down to y=4.
    let p = render_json(
        r#"{
            "width": 6, "height": 5, "frame_count": 1,
            "objects": [
                { "type": "table",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 6, "height": 5, "col_widths": [1.0], "rows": 1,
                  "cells": [[{ "content": "Hi" }]],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 0, 0), '┌', "top border");
    assert_eq!(char_at(&p, 0, 1, 1), 'H', "content stays on the first row");
    // Interior rows are blank, padded out to the requested height.
    assert_eq!(char_at(&p, 0, 1, 2), ' ');
    assert_eq!(char_at(&p, 0, 1, 3), ' ');
    assert_eq!(char_at(&p, 0, 0, 4), '└', "bottom border pushed down to honor height");
    assert_eq!(char_at(&p, 0, 5, 4), '┘');
}

#[test]
fn explicit_height_never_clips_taller_content() {
    // Content wraps to three lines but height=1 is requested. The smaller
    // height must not clip — all three lines still render.
    let p = render_json(
        r#"{
            "width": 8, "height": 6, "frame_count": 1,
            "objects": [
                { "type": "table",
                  "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "width": 8, "height": 1, "col_widths": [1.0], "rows": 1,
                  "cells": [[{ "content": "AB\nCD\nEF" }]],
                  "frames": { "start": 0, "end": 1 } }
            ]
        }"#,
    );

    assert_eq!(char_at(&p, 0, 1, 1), 'A');
    assert_eq!(char_at(&p, 0, 1, 2), 'C');
    assert_eq!(char_at(&p, 0, 1, 3), 'E', "third line not clipped by a small height");
    assert_eq!(char_at(&p, 0, 0, 4), '└', "bottom border sits below all content");
}

// ---------------------------------------------------------------------------
// col_pixel_range spans the column's borders (matches its documented contract)
// ---------------------------------------------------------------------------

#[test]
fn col_pixel_range_includes_bounding_borders_when_bordered() {
    // width 13, [0.5, 0.5]: content starts [1, 7], widths [5, 5].
    // Column 0 spans its left bar (x=0) through its right bar (x=6) -> [0, 7).
    // Column 1 spans x=6 through x=12 -> [6, 13).
    let t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 13, "col_widths": [0.5, 0.5], "rows": 1,
             "frames": { "start": 0, "end": 1 } }"#,
    );

    assert_eq!(t.col_pixel_range(0, 0), Some((0, 7)));
    assert_eq!(t.col_pixel_range(0, 1), Some((6, 13)));
}

#[test]
fn col_pixel_range_is_content_only_without_borders() {
    let t = table_from_json(
        r#"{ "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
             "width": 10, "col_widths": [0.5, 0.5], "rows": 1, "borders": false,
             "frames": { "start": 0, "end": 1 } }"#,
    );

    assert_eq!(t.col_pixel_range(0, 0), Some((0, 5)));
    assert_eq!(t.col_pixel_range(0, 1), Some((5, 10)));
}
