//! Renderer — the deterministic rasterizer.
//!
//! Takes `ResolvedScene`s (in-memory, from the engine) and produces a
//! `PlayablePresentation` (serializable, for the player).
//!
//! The renderer is pure and stateless. Given the same input, it always
//! produces the same output. It knows nothing about time, animation,
//! or presentation semantics.

use crate::types::{Cell, CellChange, Frame, PlayablePresentation, ResolvedScene, TerminalContract};

pub struct Renderer;

impl Renderer {
    /// Render a sequence of resolved scenes into a playable presentation.
    ///
    /// The first frame is always a full frame. Subsequent frames are diffs
    /// against the previous frame.
    pub fn render(scenes: &[ResolvedScene], contract: TerminalContract) -> PlayablePresentation {
        let mut frames = Vec::with_capacity(scenes.len());
        let mut prev_grid: Option<Vec<Vec<Cell>>> = None;

        for scene in scenes {
            let grid = Self::rasterize(scene, &contract);
            let frame = match &prev_grid {
                None => Frame::Full {
                    cells: grid.clone(),
                },
                Some(prev) => Frame::Diff {
                    changes: Self::diff(prev, &grid),
                },
            };
            frames.push(frame);
            prev_grid = Some(grid);
        }

        PlayablePresentation {
            contract,
            frames,
            markers: Vec::new(),
        }
    }

    /// Rasterize a resolved scene onto a fixed-size cell grid.
    ///
    /// Draw operations are sorted by z-order so that higher z values
    /// paint over lower ones.
    fn rasterize(scene: &ResolvedScene, contract: &TerminalContract) -> Vec<Vec<Cell>> {
        let w = contract.width as usize;
        let h = contract.height as usize;
        let mut grid = vec![vec![Cell::default(); w]; h];

        let mut ops: Vec<_> = scene.ops.iter().collect();
        ops.sort_by_key(|op| op.z_order);

        for op in ops {
            let x = op.x as usize;
            let y = op.y as usize;
            if x < w && y < h {
                grid[y][x] = Cell {
                    ch: op.ch,
                    style: op.style.clone(),
                };
            }
        }

        grid
    }

    /// Compute a cell-level diff between two grids.
    fn diff(prev: &[Vec<Cell>], next: &[Vec<Cell>]) -> Vec<CellChange> {
        let mut changes = Vec::new();
        for (y, (prev_row, next_row)) in prev.iter().zip(next.iter()).enumerate() {
            for (x, (prev_cell, next_cell)) in prev_row.iter().zip(next_row.iter()).enumerate() {
                if prev_cell != next_cell {
                    changes.push(CellChange {
                        x: x as u16,
                        y: y as u16,
                        cell: next_cell.clone(),
                    });
                }
            }
        }
        changes
    }
}
