//! Shared boundary types for the ASCII Presentation System.
//!
//! This module defines the two key data contracts:
//! - Engine → Renderer (in-memory): `ResolvedScene` containing `DrawOp`s
//! - Renderer → Player (file): `PlayablePresentation` containing `Frame`s

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shared style primitives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Color {
    Named(NamedColor),
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<Color>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dim: bool,
}

impl Style {
    pub fn is_default(&self) -> bool {
        self.fg.is_none() && self.bg.is_none() && !self.bold && !self.dim
    }
}

// ---------------------------------------------------------------------------
// Engine → Renderer boundary (in-memory only, never serialized)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DrawOp {
    pub x: u16,
    pub y: u16,
    pub ch: char,
    pub style: Style,
    pub z_order: i32,
}

impl DrawOp {
    /// A draw op at (`x`, `y`) computed in `usize` by the caller (base position
    /// plus a per-cell offset), or `None` when the cell lies past `u16::MAX` and
    /// so off any canvas. Renderers `ops.extend(DrawOp::at(..))`, so a far-off
    /// object is clipped instead of overflowing (debug) or wrapping (release).
    pub fn at(x: usize, y: usize, ch: char, style: Style, z_order: i32) -> Option<Self> {
        Some(DrawOp { x: u16::try_from(x).ok()?, y: u16::try_from(y).ok()?, ch, style, z_order })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedScene {
    pub width: u16,
    pub height: u16,
    pub ops: Vec<DrawOp>,
}

// ---------------------------------------------------------------------------
// Renderer → Player boundary (serialized to the playable file)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalContract {
    pub width: u16,
    pub height: u16,
}

/// Largest canvas (`width × height` cells) a deck may declare. Every frame is
/// rasterized into, and replayed onto, a grid this size, so an unchecked
/// 65535×65535 contract would demand tens of GB and abort instead of erroring.
pub const MAX_CANVAS_CELLS: u64 = 1_000_000;

/// Most cells a compiled deck may store: the full first frame plus every diff
/// change (`Renderer::render_bounded`). Each costs ~150 bytes of output JSON,
/// so this keeps a playable well inside the browser's string and wasm limits.
pub const MAX_STORED_CELLS: u64 = 2_000_000;

/// Most draw ops (cells drawn, counting overlaps) one frame may produce while
/// its objects are resolved (`Engine::scenes`). The canvas and each object are
/// bounded already, but not their sum, so many overlapping full-canvas objects
/// would otherwise demand GBs before any stored-cell budget is checked.
pub const MAX_FRAME_OPS: u64 = 8 * MAX_CANVAS_CELLS;

/// Most draw ops a whole deck may produce, summed over its frames
/// (`compile::render_scenes`). Memory is bounded per frame by
/// [`MAX_FRAME_OPS`]; this bounds the total draw ops the renderer sorts and
/// rasterizes, so thousands of identical frames of heavy overlap fail fast
/// instead of running for hours. It does not bound resolve work that emits few
/// ops (e.g. wrapping a huge, mostly clipped text on every frame).
pub const MAX_DECK_OPS: u64 = 200 * MAX_CANVAS_CELLS;

impl TerminalContract {
    /// Reject a canvas larger than [`MAX_CANVAS_CELLS`]. Checked by `compile`
    /// (via `SourcePresentation::validate_contract`) and before `bs play`.
    pub fn validate(&self) -> Result<(), String> {
        let cells = u64::from(self.width) * u64::from(self.height);
        if cells > MAX_CANVAS_CELLS {
            return Err(format!(
                "canvas {}×{} is {cells} cells; the limit is {MAX_CANVAS_CELLS}",
                self.width, self.height
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub ch: char,
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
}

impl Cell {
    /// The character to send to a terminal, exactly one column wide, since the
    /// player prints a row's cells back to back: a control character (`\r`,
    /// `\t`, ESC, …) from a hand-edited deck or a zero-width one (combining
    /// mark, variation selector) is shown as a space, and a char not known to be
    /// one column wide (CJK, emoji) as `?` — see [`narrow_char`].
    pub fn printable(&self) -> char {
        if self.ch.is_control() { ' ' } else { narrow_char(self.ch).unwrap_or(' ') }
    }
}

/// `s` made safe to print as editor UI text, one char per terminal column
/// (so caret columns still line up): a control char — which could otherwise
/// smuggle an escape sequence from a deck string to the terminal — becomes `·`,
/// and every other char goes through [`narrow_char`], a zero-width one
/// becoming `?`. Display only: the stored text is never changed.
pub fn printable_text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '·' } else { narrow_char(c).unwrap_or('?') })
        .collect()
}

/// Map a char to one that fills exactly one terminal column, for everything the
/// terminal player and editor print into the cell grid (deck cells via
/// [`Cell::printable`], `Command` output via `player::layout_output`). Known
/// zero-width chars (combining marks, joiners, variation selectors, bidi/format
/// controls, emoji modifiers and tags) give `None`; chars from the single-width
/// blocks below pass through; anything else, whose width isn't known to be 1,
/// becomes `?`. A hand-rolled table (no Unicode-width dependency), deliberately
/// conservative. The web viewer mirrors it (`narrowChar` in web/viewer.js):
/// change both together.
pub fn narrow_char(c: char) -> Option<char> {
    match c as u32 {
        0x0300..=0x036F | 0x0483..=0x0489 | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0x20D0..=0x20FF
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0xFEFF | 0x1F3FB..=0x1F3FF
        | 0xE0000..=0xE007F | 0xE0100..=0xE01EF => None,
        // Wide chars inside the symbol blocks passed below: emoji presentation,
        // and (since Unicode 16) the Yijing trigram and digram symbols.
        0x2614..=0x2615 | 0x2630..=0x2637 | 0x268A..=0x268F | 0x2648..=0x2653 | 0x267F | 0x2693 | 0x26A1
        | 0x26AA..=0x26AB | 0x26BD..=0x26BE | 0x26C4..=0x26C5 | 0x26CE | 0x26D4
        | 0x26EA | 0x26F2..=0x26F3 | 0x26F5 | 0x26FA | 0x26FD | 0x2705
        | 0x270A..=0x270B | 0x2728 | 0x274C | 0x274E | 0x2753..=0x2755 | 0x2757
        | 0x2795..=0x2797 | 0x27B0 | 0x27BF => Some('?'),
        0x20..=0x7E
        | 0xA0..=0x2FF // Latin-1, Latin Extended-A/B, IPA, modifier letters
        | 0x370..=0x377 | 0x37A..=0x37F | 0x384..=0x38A | 0x38C // Greek
        | 0x38E..=0x3A1 | 0x3A3..=0x482 | 0x48A..=0x52F // Greek, Cyrillic
        | 0x1E00..=0x1EFF // Latin Extended Additional
        | 0x2010..=0x2027 | 0x2030..=0x205E // punctuation
        | 0x20A0..=0x20BF // currency
        | 0x2100..=0x214F | 0x2190..=0x22FF // letterlike, arrows, math
        | 0x2300 // ⌀ (the editor's circle summary)
        | 0x2500..=0x25FC // box drawing, block elements, geometric shapes
        | 0x2600..=0x27BF // misc symbols, dingbats (★, ✓, ✗, …)
        | 0x2800..=0x28FF // braille (spinners)
        => Some(c),
        _ => Some('?'),
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            style: Style::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellChange {
    pub x: u16,
    pub y: u16,
    pub cell: Cell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Full { cells: Vec<Vec<Cell>> },
    Diff { changes: Vec<CellChange> },
}

impl Frame {
    /// Apply this frame onto `grid` without changing its shape: a `Full` frame
    /// is copied cell by cell (missing cells blank, extra ones dropped) and a
    /// diff change outside the grid is skipped, so a malformed playable (ragged
    /// rows, out-of-range diffs) degrades instead of panicking.
    pub fn apply_to(&self, grid: &mut [Vec<Cell>]) {
        match self {
            Frame::Full { cells } => {
                for (y, row) in grid.iter_mut().enumerate() {
                    for (x, cell) in row.iter_mut().enumerate() {
                        *cell = cells
                            .get(y)
                            .and_then(|r| r.get(x))
                            .cloned()
                            .unwrap_or_default();
                    }
                }
            }
            Frame::Diff { changes } => {
                for c in changes {
                    let row = grid.get_mut(c.y as usize);
                    if let Some(cell) = row.and_then(|r| r.get_mut(c.x as usize)) {
                        *cell = c.cell.clone();
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub frame_index: usize,
    pub label: String,
}

/// A runtime command region — the sidecar spec for a `Command` object over one
/// run of frames with the same box geometry. A static command has one region;
/// one whose box is animated has one per run, consecutive and with the same
/// command, so output always lands in the box drawn on the current frame.
///
/// Unlike every other object, a `Command` cannot be baked into the static
/// frames: its output and exit status are only known when the binary runs at
/// play time. The compiler therefore emits this spec alongside the frames, and
/// the player executes the binary and paints its output into `[x..x+w, y..y+h]`
/// (the interior of the box the object draws). Editing/compiling never runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRegion {
    /// Frames this region covers (end exclusive). The player runs the binary
    /// whenever the current frame enters a command's frames, and keeps it while
    /// playback stays within this region.
    pub start_frame: usize,
    pub end_frame: usize,
    /// Interior region where captured output is painted.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Cell where the ✓ / ✗ status indicator is drawn (on the box's top edge).
    pub status_x: u16,
    pub status_y: u16,
    /// Program to run and its arguments.
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Working directory for the child (defaults to the player's cwd).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Kill the child after this many seconds. Omitted ⇒ run with no timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Style applied to the painted output cells.
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
}

/// A runtime loop region — the sidecar spec for a `Loop` object.
///
/// Like a `Command`, a `Loop` is a play-time behavior that cannot be baked into
/// static frames: it draws nothing and instead tells the `Player` to
/// auto-advance across `[start_frame, end_frame)` (end exclusive) on a timer
/// until the presenter navigates out with an arrow key. The compiler emits this
/// spec alongside the frames; the editor and renderer ignore it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopRegion {
    /// First frame of the loop (inclusive).
    pub start_frame: usize,
    /// One past the last frame of the loop (exclusive).
    pub end_frame: usize,
    /// Delay between auto-advanced frames, in milliseconds.
    pub delay_ms: u64,
    /// Number of times to play before moving on; `0` = loop forever.
    #[serde(default)]
    pub count: usize,
    /// Play the range forward then backward (`5,6,7,8,7,6,…`) instead of
    /// restarting (`5,6,7,8,5,6,…`).
    pub bounce: bool,
}

/// A runtime animation region — the sidecar spec for an `Animation` object.
///
/// An `Animation` marks a span of frames over which one or more coordinate
/// animations play (the motion itself lives in `Coordinate::Animated` on the
/// objects). It draws nothing; its job at play time is **auto-play**: when
/// `auto_play` is set, the `Player` auto-advances across the span on a timer.
/// Unlike a `Loop`, animations may overlap — where several auto-play animations
/// cover the same frame boundary, the effective advance delay is the **minimum**
/// of their `delay_ms`. The compiler emits this spec alongside the frames; the
/// editor and renderer ignore it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationRegion {
    /// First frame of the animation (inclusive).
    pub start_frame: usize,
    /// One past the last frame of the animation (exclusive).
    pub end_frame: usize,
    /// Whether the deck auto-advances across this span at play time.
    pub auto_play: bool,
    /// Delay between auto-advanced frames, in milliseconds.
    pub delay_ms: u64,
}

/// A runtime auto-advance region — the sidecar spec for an `AutoAdvance` object.
///
/// Like a `Loop`, an auto-advance is a play-time behavior that cannot be baked
/// into static frames: it draws nothing and instead tells the `Player` to
/// advance to the next frame on its own, after `delay_ms`, for every frame in
/// `[start_frame, end_frame)` (end exclusive). The presenter can still navigate
/// manually at any time; the player suppresses auto-advance on the last frame
/// and while a `Loop` is driving playback. Where an auto-play `Animation` also
/// covers a frame, the effective delay is the **minimum** of the two. The
/// compiler emits this spec alongside the frames; the editor and renderer
/// ignore it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoAdvanceRegion {
    /// First frame that auto-advances (inclusive).
    pub start_frame: usize,
    /// One past the last frame that auto-advances (exclusive).
    pub end_frame: usize,
    /// Delay before advancing to the next frame, in milliseconds.
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayablePresentation {
    pub contract: TerminalContract,
    pub frames: Vec<Frame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandRegion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<LoopRegion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<AnimationRegion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_advances: Vec<AutoAdvanceRegion>,
}

impl PlayablePresentation {
    /// Reconstruct the full cell grid visible at `frame` by replaying the
    /// initial `Frame::Full` plus every `Frame::Diff` up to and including it.
    ///
    /// This is the single source of truth for "what does frame N look like":
    /// the player, the editor preview, and the test harness all go through it,
    /// so they can never disagree about how diffs accumulate. `frame` is
    /// clamped to the last available frame, and the grid always has the
    /// contract's dimensions (see `Frame::apply_to`), so a malformed playable
    /// degrades gracefully instead of panicking.
    pub fn grid_at(&self, frame: usize) -> Vec<Vec<Cell>> {
        let w = self.contract.width as usize;
        let h = self.contract.height as usize;
        let mut grid = vec![vec![Cell::default(); w]; h];
        if self.frames.is_empty() {
            return grid;
        }
        let last = frame.min(self.frames.len() - 1);
        for f in &self.frames[..=last] {
            f.apply_to(&mut grid);
        }
        grid
    }
}
