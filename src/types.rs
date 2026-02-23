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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayablePresentation {
    pub contract: TerminalContract,
    pub frames: Vec<Frame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}
