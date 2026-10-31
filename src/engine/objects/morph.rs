use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, Style};

use super::super::source::{FrameRange, Position};
use super::{Resolve, ResolveCtx};

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
    /// `width` is the whole grid's width (the longest row of `from` or `to`), so
    /// the horizontal wipes move one straight front across ragged art; `rows` is
    /// the total row count.
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
    /// The morph's own range, kept as the blend's timing while an explicit
    /// group range has replaced `frames` (set by
    /// [`SourcePresentation::effective_objects`](crate::engine::source::SourcePresentation::effective_objects)):
    /// the group only gates where the morph shows. `None` = `frames`.
    #[serde(skip)]
    pub timing: Option<FrameRange>,
    /// Set when this morph is a piece cut from a longer blend (a frame copy of
    /// part of it): its range shows the blend from `offset` frames into one
    /// `length` frames long, so the piece looks exactly as those frames did.
    /// `None` = the blend runs over its own range. See [`Morph::keep_blend`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<BlendWindow>,
}

/// Where a cut [`Morph`] sits in the blend it was cut from (see `Morph::blend`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlendWindow {
    /// Frames into the original blend at which this range starts.
    pub offset: i64,
    /// Length in frames of the original blend.
    pub length: usize,
}

impl Morph {
    /// Keep the blend as it is when this morph's range becomes a piece showing
    /// the frames `[s, e)` of its current range (a copy of part of it, a split):
    /// call it before moving the range. Records a [`BlendWindow`] (none for the
    /// whole range), so the piece shows on each frame what that frame showed.
    pub fn keep_blend(&mut self, s: usize, e: usize) {
        let (offset, length) = self.window();
        let offset = offset.saturating_add(s as i64).saturating_sub(self.frames.start as i64);
        self.blend = if offset == 0 && length == e.saturating_sub(s) {
            None
        } else {
            Some(BlendWindow { offset, length })
        };
    }

    /// `(offset, length)` of the blend this range shows (its own when uncut).
    fn window(&self) -> (i64, usize) {
        match self.blend {
            Some(b) => (b.offset, b.length),
            None => {
                let r = self.timing.as_ref().unwrap_or(&self.frames);
                (0, r.end.saturating_sub(r.start))
            }
        }
    }

    /// Progress `t ∈ [0, 1]` of the morph on `frame`: 0 on the first frame of its
    /// own range ([`timing`](Self::timing), else `frames`), 1 on the last,
    /// clamped outside it. A range of a single frame (or empty) has no span to
    /// interpolate across, so it stays at 0 (shows `from`).
    pub fn progress(&self, frame: usize) -> f64 {
        let r = self.timing.as_ref().unwrap_or(&self.frames);
        let (offset, span) = self.window();
        if span <= 1 {
            return 0.0;
        }
        // Saturating: `offset` may be any i64 in a hand-edited deck.
        let local = offset.saturating_add(frame as i64).saturating_sub(r.start as i64).max(0);
        (local as f64 / (span - 1) as f64).clamp(0.0, 1.0)
    }
}

impl Morph {
    /// The art this morph shows on `frame`: one line per grid row, as wide as
    /// that row in `from`/`to`, each cell picked exactly as [`Resolve`] picks it
    /// at [`progress`](Self::progress)`(frame)`. A morph whose `from` and `to`
    /// are both this text draws the same cells on any frame.
    pub fn art_at(&self, frame: usize) -> String {
        let t = self.progress(frame);
        let from = grid(&self.from);
        let to = grid(&self.to);
        let rows = from.len().max(to.len());
        let grid_w = from.iter().chain(&to).map(Vec::len).max().unwrap_or(0);
        let lines: Vec<String> = (0..rows)
            .map(|row| {
                let from_row = from.get(row);
                let to_row = to.get(row);
                let width = from_row.map_or(0, |r| r.len()).max(to_row.map_or(0, |r| r.len()));
                (0..width)
                    .map(|col| {
                        let ch_from = from_row.and_then(|r| r.get(col)).copied().unwrap_or(' ');
                        let ch_to = to_row.and_then(|r| r.get(col)).copied().unwrap_or(' ');
                        if t > self.mode.threshold(col, row, grid_w, rows) { ch_to } else { ch_from }
                    })
                    .collect()
            })
            .collect();
        lines.join("\n")
    }

    /// A static clone of this morph frozen at what it shows on `frame`
    /// (`from` = `to` = [`art_at`](Self::art_at)`(frame)`, its own timing):
    /// for continuing or copying a morph's state onto other frames without
    /// re-timing its blend. The caller sets the clone's `frames`.
    pub fn frozen_at(&self, frame: usize) -> Morph {
        let art = self.art_at(frame);
        Morph { from: art.clone(), to: art, timing: None, blend: None, ..self.clone() }
    }
}

/// Split art text into a grid of character rows.
fn grid(art: &str) -> Vec<Vec<char>> {
    art.split('\n').map(|line| line.chars().collect()).collect()
}

impl Resolve for Morph {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        let frame = ctx.frame;
        if !self.frames.contains(frame) {
            return;
        }
        let t = self.progress(frame);
        let from = grid(&self.from);
        let to = grid(&self.to);
        let rows = from.len().max(to.len());
        let grid_w = from.iter().chain(&to).map(Vec::len).max().unwrap_or(0);
        let base_x = self.position.x.evaluate(frame, ctx.anims);
        let base_y = self.position.y.evaluate(frame, ctx.anims);
        let has_bg = self.style.bg.is_some();

        for row in 0..rows {
            let from_row = from.get(row);
            let to_row = to.get(row);
            let width = from_row.map_or(0, |r| r.len()).max(to_row.map_or(0, |r| r.len()));
            for col in 0..width {
                let ch_from = from_row.and_then(|r| r.get(col)).copied().unwrap_or(' ');
                let ch_to = to_row.and_then(|r| r.get(col)).copied().unwrap_or(' ');
                let ch = if t > self.mode.threshold(col, row, grid_w, rows) {
                    ch_to
                } else {
                    ch_from
                };
                if ch == ' ' && !has_bg {
                    continue; // transparent
                }
                ops.extend(DrawOp::at(
                    usize::from(base_x) + col,
                    usize::from(base_y) + row,
                    ch,
                    self.style.clone(),
                    self.z_order,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::source::{AnimSpans, Coordinate};

    /// Render a morph at `frame` (no animations needed — positions are Fixed).
    fn render(m: &Morph, frame: usize) -> Vec<DrawOp> {
        let anims = AnimSpans::default();
        let mut ops = Vec::new();
        m.resolve(&ResolveCtx { frame, canvas_width: 80, canvas_height: 24, anims: &anims }, &mut ops);
        ops
    }

    #[test]
    fn frozen_at_draws_what_the_morph_shows_on_that_frame() {
        // Ragged rows of different widths, every mode, every frame of the span:
        // the frozen clone (on any frame of its own) draws the same cells.
        for mode in [MorphMode::Dissolve, MorphMode::WipeRight, MorphMode::WipeLeft,
                     MorphMode::WipeDown, MorphMode::WipeUp] {
            let m = morph("ab\ncdef\n g", "WXYZ\nV\n\nUU", 2, 7, mode);
            for f in 2..7 {
                let mut frozen = m.frozen_at(f);
                frozen.frames = FrameRange { start: 10, end: 12 };
                assert_eq!(cells(&render(&frozen, 11)), cells(&render(&m, f)), "{mode:?} frame {f}");
            }
        }
    }

    fn cells(ops: &[DrawOp]) -> Vec<(u16, u16, char)> {
        ops.iter().map(|o| (o.x, o.y, o.ch)).collect()
    }

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
            timing: None,
            blend: None,
        }
    }

    /// Reconstruct the visible glyph at `(x, y)` from a render, ' ' if none.
    fn char_at(ops: &[DrawOp], x: u16, y: u16) -> char {
        ops.iter().find(|o| o.x == x && o.y == y).map(|o| o.ch).unwrap_or(' ')
    }

    #[test]
    fn a_huge_blend_window_saturates_instead_of_overflowing() {
        let mut m = morph("a", "b", 2, 6, MorphMode::Dissolve);
        for offset in [i64::MAX, i64::MIN] {
            m.blend = Some(BlendWindow { offset, length: usize::MAX });
            let t = m.progress(5);
            assert!((0.0..=1.0).contains(&t), "offset {offset}: {t}");
            m.keep_blend(5, 6);
        }
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
        let ops = render(&m, 0);
        assert_eq!(char_at(&ops, 0, 0), 'A');
        assert_eq!(char_at(&ops, 1, 0), 'B');

        let ops = render(&m, 3); // last frame → progress 1 → fully `to`
        assert_eq!(char_at(&ops, 0, 0), 'X');
        assert_eq!(char_at(&ops, 1, 0), 'Y');
    }

    #[test]
    fn wipe_right_flips_left_cells_before_right_cells() {
        // 4-wide row, mid-progress: leftmost cells have already flipped to `to`,
        // rightmost cells still show `from`.
        let m = morph("AAAA", "BBBB", 0, 11, MorphMode::WipeRight); // span 11
        let ops = render(&m, 5); // t = 0.5
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
        let ops = render(&m, 0);
        // 2x2 of '#'
        assert_eq!(ops.len(), 4);
        // At full progress the second row / second column become spaces (skipped).
        let ops = render(&m, 3);
        assert_eq!(char_at(&ops, 0, 0), '#');
        assert_eq!(char_at(&ops, 1, 0), ' '); // beyond `to` width → space
        assert_eq!(char_at(&ops, 0, 1), ' '); // beyond `to` height → space
    }

    #[test]
    fn outside_the_range_emits_nothing() {
        let m = morph("A", "B", 2, 4, MorphMode::Dissolve);
        assert!(render(&m, 0).is_empty());
        assert!(render(&m, 4).is_empty());
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
