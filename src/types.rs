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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub ch: char,
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub frame_index: usize,
    pub label: String,
}

/// A runtime command region — the sidecar spec for a `Command` object.
///
/// Unlike every other object, a `Command` cannot be baked into the static
/// frames: its output and exit status are only known when the binary runs at
/// play time. The compiler therefore emits this spec alongside the frames, and
/// the player executes the binary and paints its output into `[x..x+w, y..y+h]`
/// (the interior of the box the object draws). Editing/compiling never runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRegion {
    /// Frames on which this command is active (end exclusive). The player runs
    /// the binary whenever the current frame enters this range.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayablePresentation {
    pub contract: TerminalContract,
    pub frames: Vec<Frame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandRegion>,
}

impl PlayablePresentation {
    /// Reconstruct the full cell grid visible at `frame` by replaying the
    /// initial `Frame::Full` plus every `Frame::Diff` up to and including it.
    ///
    /// This is the single source of truth for "what does frame N look like":
    /// the player, the editor preview, and the test harness all go through it,
    /// so they can never disagree about how diffs accumulate. `frame` is
    /// clamped to the last available frame, and diff changes that fall outside
    /// the contract's dimensions are skipped — a malformed diff degrades
    /// gracefully instead of panicking.
    pub fn grid_at(&self, frame: usize) -> Vec<Vec<Cell>> {
        let w = self.contract.width as usize;
        let h = self.contract.height as usize;
        let mut grid = vec![vec![Cell::default(); w]; h];
        if self.frames.is_empty() {
            return grid;
        }
        let last = frame.min(self.frames.len() - 1);
        for f in &self.frames[..=last] {
            match f {
                Frame::Full { cells } => grid = cells.clone(),
                Frame::Diff { changes } => {
                    for c in changes {
                        let x = c.x as usize;
                        let y = c.y as usize;
                        if y < grid.len() && x < grid[0].len() {
                            grid[y][x] = c.cell.clone();
                        }
                    }
                }
            }
        }
        grid
    }
}
