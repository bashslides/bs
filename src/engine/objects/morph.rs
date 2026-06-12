use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{FrameRange, Position};
use super::Resolve;

/// How a [`Morph`] transitions each cell from the `from` art to the `to` art as
/// playback progresses. Every mode is a pure function of the cell position and
/// the progress `t ∈ [0, 1]`, so the result is identical in the editor preview,
/// the compiler, and the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MorphMode {
    /// Cells flip in a deterministic pseudo-random order (a dissolve/melt).
    #[default]
    Dissolve,
    /// Cells flip left→right (a column wipe).
    WipeRight,
    /// Cells flip right→left.
    WipeLeft,
    /// Cells flip top→bottom (a row wipe).
    WipeDown,
    /// Cells flip bottom→top.
    WipeUp,
}

impl MorphMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            MorphMode::Dissolve => "dissolve",
            MorphMode::WipeRight => "wipe-right",
            MorphMode::WipeLeft => "wipe-left",
            MorphMode::WipeDown => "wipe-down",
            MorphMode::WipeUp => "wipe-up",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim() {
            "dissolve" => Some(MorphMode::Dissolve),
            "wipe-right" => Some(MorphMode::WipeRight),
            "wipe-left" => Some(MorphMode::WipeLeft),
            "wipe-down" => Some(MorphMode::WipeDown),
            "wipe-up" => Some(MorphMode::WipeUp),
            _ => None,
        }
    }

    /// The progress threshold at which the cell at `(col, row)` switches from the
    /// `from` glyph to the `to` glyph: the cell shows `to` once `t > threshold`.
    /// `width` is the current row's width, `rows` the total row count.
    fn threshold(&self, col: usize, row: usize, width: usize, rows: usize) -> f64 {
        match self {
            MorphMode::Dissolve => cell_hash(col, row),
            MorphMode::WipeRight => frac(col, width),
            MorphMode::WipeLeft => frac(width.saturating_sub(1).saturating_sub(col), width),
            MorphMode::WipeDown => frac(row, rows),
            MorphMode::WipeUp => frac(rows.saturating_sub(1).saturating_sub(row), rows),
        }
    }
}

/// `n / d` as a fraction in `[0, 1)`, or 0 when `d == 0`.
fn frac(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}

/// A deterministic per-cell value in `[0, 1)` used by the dissolve mode. Pure
/// function of the coordinates (no RNG state) so every render stage agrees.
fn cell_hash(col: usize, row: usize) -> f64 {
    let mut h = (col as u64)
        .wrapping_mul(73856093)
        ^ (row as u64).wrapping_mul(19349663).wrapping_add(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    (h % 1000) as f64 / 1000.0
}

/// A content morph between two ASCII-art grids. Both grids are stored inline
/// (copied from the art library when added, like [`super::Art`]), so a
/// presentation never depends on the library.
///
/// Over the object's `frames` range the morph progresses from `from` (on the
/// first frame) to `to` (on the last frame): at progress `t` each cell shows the
/// `to` glyph once `t` passes that cell's [`MorphMode`] threshold, otherwise the
/// `from` glyph. The two grids share the object position (overlaid corner-to-
/// corner); cells past one grid's extent are treated as spaces, so a smaller
/// shape grows into / shrinks out of a larger one. As with `Art`, spaces are
/// transparent unless a background colour is set.
///
/// The whole effect is baked into static frames at compile time — there is no
/// play-time behaviour — so the editor's live Engine+Renderer preview shows the
/// morph exactly as it will play.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Morph {
    pub position: Position,
    /// Art shown at the start of the range (progress 0).
    pub from: String,
    /// Art morphed into by the end of the range (progress 1).
    pub to: String,
    /// Display-only names of the source pieces (`"ball→square"`), for the panel.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mode: MorphMode,
    #[serde(default)]
    pub style: Style,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Morph {
    /// Progress `t ∈ [0, 1]` of the morph on `frame`: 0 on the first frame of the
    /// range, 1 on the last. A range of a single frame (or empty) has no span to
    /// interpolate across, so it stays at 0 (shows `from`).
    pub fn progress(&self, frame: usize) -> f64 {
        let span = self.frames.end.saturating_sub(self.frames.start);
        if span <= 1 {
            return 0.0;
        }
        let local = frame.saturating_sub(self.frames.start);
        (local as f64 / (span - 1) as f64).clamp(0.0, 1.0)
    }
}

/// Split art text into a grid of character rows.
fn grid(art: &str) -> Vec<Vec<char>> {
    art.split('\n').map(|line| line.chars().collect()).collect()
}

impl Resolve for Morph {
    fn resolve(&self, frame: usize, _canvas_width: u16, ops: &mut Vec<DrawOp>) {
        if !self.frames.contains(frame) {
            return;
        }
        let t = self.progress(frame);
        let from = grid(&self.from);
        let to = grid(&self.to);
        let rows = from.len().max(to.len());
        let base_x = self.position.x.evaluate(frame);
        let base_y = self.position.y.evaluate(frame);
        let has_bg = self.style.bg.is_some();

        for row in 0..rows {
            let from_row = from.get(row);
            let to_row = to.get(row);
            let width = from_row.map_or(0, |r| r.len()).max(to_row.map_or(0, |r| r.len()));
            for col in 0..width {
                let ch_from = from_row.and_then(|r| r.get(col)).copied().unwrap_or(' ');
                let ch_to = to_row.and_then(|r| r.get(col)).copied().unwrap_or(' ');
                let ch = if t > self.mode.threshold(col, row, width, rows) {
                    ch_to
                } else {
                    ch_from
                };
                if ch == ' ' && !has_bg {
                    continue; // transparent
                }
                ops.push(DrawOp {
                    x: base_x + col as u16,
                    y: base_y + row as u16,
                    ch,
                    style: self.style.clone(),
                    z_order: self.z_order,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::source::Coordinate;

    fn morph(from: &str, to: &str, start: usize, end: usize, mode: MorphMode) -> Morph {
        Morph {
            position: Position { x: Coordinate::Fixed(0.0), y: Coordinate::Fixed(0.0) },
            from: from.into(),
            to: to.into(),
            name: String::new(),
            mode,
            style: Style::default(),
            frames: FrameRange { start, end },
            z_order: 0,
        }
    }

    /// Reconstruct the visible glyph at `(x, y)` from a render, ' ' if none.
    fn char_at(ops: &[DrawOp], x: u16, y: u16) -> char {
        ops.iter().find(|o| o.x == x && o.y == y).map(|o| o.ch).unwrap_or(' ')
    }

    #[test]
    fn progress_runs_zero_to_one_across_the_range() {
        let m = morph("a", "b", 2, 6, MorphMode::Dissolve); // span 4, frames 2..=5
        assert_eq!(m.progress(2), 0.0);
        assert_eq!(m.progress(5), 1.0);
        assert!((m.progress(3) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn first_frame_is_from_last_frame_is_to() {
        let m = morph("AB", "XY", 0, 4, MorphMode::Dissolve);
        let mut ops = Vec::new();
        m.resolve(0, 80, &mut ops);
        assert_eq!(char_at(&ops, 0, 0), 'A');
        assert_eq!(char_at(&ops, 1, 0), 'B');

        let mut ops = Vec::new();
        m.resolve(3, 80, &mut ops); // last frame → progress 1 → fully `to`
        assert_eq!(char_at(&ops, 0, 0), 'X');
        assert_eq!(char_at(&ops, 1, 0), 'Y');
    }

    #[test]
    fn wipe_right_flips_left_cells_before_right_cells() {
        // 4-wide row, mid-progress: leftmost cells have already flipped to `to`,
        // rightmost cells still show `from`.
        let m = morph("AAAA", "BBBB", 0, 11, MorphMode::WipeRight); // span 11
        let mut ops = Vec::new();
        m.resolve(5, 80, &mut ops); // t = 0.5
        // thresholds: col/4 → 0, .25, .5, .75; t=0.5 > 0 and > .25 (flip), not > .5/.75
        assert_eq!(char_at(&ops, 0, 0), 'B');
        assert_eq!(char_at(&ops, 1, 0), 'B');
        assert_eq!(char_at(&ops, 2, 0), 'A');
        assert_eq!(char_at(&ops, 3, 0), 'A');
    }

    #[test]
    fn out_of_grid_cells_are_transparent_spaces() {
        // `from` is wider/taller than `to`; at progress 0 only the from glyphs show.
        let m = morph("##\n##", "#", 0, 4, MorphMode::Dissolve);
        let mut ops = Vec::new();
        m.resolve(0, 80, &mut ops);
        // 2x2 of '#'
        assert_eq!(ops.len(), 4);
        // At full progress the second row / second column become spaces (skipped).
        let mut ops = Vec::new();
        m.resolve(3, 80, &mut ops);
        assert_eq!(char_at(&ops, 0, 0), '#');
        assert_eq!(char_at(&ops, 1, 0), ' '); // beyond `to` width → space
        assert_eq!(char_at(&ops, 0, 1), ' '); // beyond `to` height → space
    }

    #[test]
    fn outside_the_range_emits_nothing() {
        let m = morph("A", "B", 2, 4, MorphMode::Dissolve);
        let mut ops = Vec::new();
        m.resolve(0, 80, &mut ops);
        assert!(ops.is_empty());
        m.resolve(4, 80, &mut ops);
        assert!(ops.is_empty());
    }

    #[test]
    fn mode_string_round_trips() {
        for m in [
            MorphMode::Dissolve,
            MorphMode::WipeRight,
            MorphMode::WipeLeft,
            MorphMode::WipeDown,
            MorphMode::WipeUp,
        ] {
            assert_eq!(MorphMode::from_str_opt(m.as_str()), Some(m));
        }
        assert_eq!(MorphMode::from_str_opt("nope"), None);
    }
}
