//! Bitmap font for large text rendering.
//!
//! Each glyph is 5 rows tall with variable width. All rows within a single
//! glyph are guaranteed to have the same length. A non-space character in a
//! row means "filled"; a space means "empty".

/// Return the 5-row bitmap for `ch`, or `None` if the character is not in
/// the font.  The caller should handle case folding before calling this.
pub fn glyph(ch: char) -> Option<[&'static str; 5]> {
    let g = match ch {
        'A' => [" ### ", "#   #", "#####", "#   #", "#   #"],
        'B' => ["#### ", "#   #", "#### ", "#   #", "#### "],
        'C' => [" ### ", "#   #", "#    ", "#   #", " ### "],
        'D' => ["#### ", "#   #", "#   #", "#   #", "#### "],
        'E' => ["#####", "#    ", "###  ", "#    ", "#####"],
        'F' => ["#####", "#    ", "###  ", "#    ", "#    "],
        'G' => [" ### ", "#    ", "#  ##", "#   #", " ### "],
        'H' => ["#   #", "#   #", "#####", "#   #", "#   #"],
        'I' => ["###", " # ", " # ", " # ", "###"],
        'J' => ["  ###", "   # ", "   # ", "#  # ", " ##  "],
        'K' => ["#   #", "#  # ", "###  ", "#  # ", "#   #"],
        'L' => ["#    ", "#    ", "#    ", "#    ", "#####"],
        'M' => ["#   #", "## ##", "# # #", "#   #", "#   #"],
        'N' => ["#   #", "##  #", "# # #", "#  ##", "#   #"],
        'O' => [" ### ", "#   #", "#   #", "#   #", " ### "],
        'P' => ["#### ", "#   #", "#### ", "#    ", "#    "],
        'Q' => [" ### ", "#   #", "# # #", "#  # ", " ## #"],
        'R' => ["#### ", "#   #", "#### ", "#  # ", "#   #"],
        'S' => [" ####", "#    ", " ### ", "    #", "#### "],
        'T' => ["#####", "  #  ", "  #  ", "  #  ", "  #  "],
        'U' => ["#   #", "#   #", "#   #", "#   #", " ### "],
        'V' => ["#   #", "#   #", "#   #", " # # ", "  #  "],
        'W' => ["#   #", "#   #", "# # #", "## ##", "#   #"],
        'X' => ["#   #", " # # ", "  #  ", " # # ", "#   #"],
        'Y' => ["#   #", " # # ", "  #  ", "  #  ", "  #  "],
        'Z' => ["#####", "   # ", "  #  ", " #   ", "#####"],

        '0' => [" ### ", "#   #", "#   #", "#   #", " ### "],
        '1' => [" # ", "## ", " # ", " # ", "###"],
        '2' => [" ### ", "#   #", "  ## ", " #   ", "#####"],
        '3' => [" ### ", "#   #", "  ## ", "#   #", " ### "],
        '4' => ["#  # ", "#  # ", "#####", "   # ", "   # "],
        '5' => ["#####", "#    ", "#### ", "    #", "#### "],
        '6' => [" ### ", "#    ", "#### ", "#   #", " ### "],
        '7' => ["#####", "   # ", "  #  ", " #   ", " #   "],
        '8' => [" ### ", "#   #", " ### ", "#   #", " ### "],
        '9' => [" ### ", "#   #", " ####", "   # ", " ### "],

        ' ' => ["   ", "   ", "   ", "   ", "   "],
        '!' => ["#", "#", "#", " ", "#"],
        '.' => [" ", " ", " ", " ", "#"],
        '-' => ["     ", "     ", "#####", "     ", "     "],
        '?' => [" ### ", "#   #", "  ## ", "     ", "  #  "],
        ':' => [" ", "#", " ", "#", " "],

        _ => return None,
    };
    debug_assert!(
        g.iter().all(|row| row.len() == g[0].len()),
        "glyph '{ch}' has inconsistent row widths",
    );
    Some(g)
}

/// Compute the rendered width of `text` in glyph columns, including 1-column
/// spacing between characters.
pub fn text_width(text: &str) -> u16 {
    let mut width: u16 = 0;
    let mut first = true;
    for ch in text.chars() {
        if let Some(g) = glyph(ch.to_ascii_uppercase()) {
            if !first {
                width += 1; // inter-character spacing
            }
            width += g[0].len() as u16;
            first = false;
        }
    }
    width
}

/// The height of every glyph (constant).
pub const GLYPH_HEIGHT: u16 = 5;
