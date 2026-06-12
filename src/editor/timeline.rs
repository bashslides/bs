use std::io;

use crossterm::{cursor, queue, style, terminal};

use crate::engine::source::SceneObject;

use super::state::{EditorState, Mode};
use super::ui::Layout;

pub fn render_timeline(
    stdout: &mut io::Stdout,
    layout: &Layout,
    state: &EditorState,
) -> anyhow::Result<()> {
    let y = layout.timeline_y;
    let width = layout.term_width as usize;
    let frame_count = state.source.frame_count;
    let current = state.current_frame;

    // Row 1: Frame numbers
    queue!(
        stdout,
        cursor::MoveTo(0, y),
        terminal::Clear(terminal::ClearType::CurrentLine),
    )?;

    if frame_count == 0 {
        queue!(stdout, style::Print(" (no frames)"))?;
    } else {
        let segs = build_segments(state);
        render_frame_bar(stdout, width, &segs, current)?;
    }

    // Row 2: Mode + status
    queue!(
        stdout,
        cursor::MoveTo(0, y + 1),
        terminal::Clear(terminal::ClearType::CurrentLine),
    )?;

    let mode_str = match &state.mode {
        Mode::Normal => "NORMAL",
        Mode::AddObject { .. } => "ADD OBJECT",
        Mode::SelectObject { .. } => "SELECT OBJECT",
        Mode::SelectedObject { .. } => "SELECTED",
        Mode::ResizeObject { .. } => "RESIZE",
        Mode::EditProperties { .. } => "EDIT PROPERTIES",
        Mode::AnimateProperty { .. } => "ANIMATE",
        Mode::Confirm { .. } => "CONFIRM",
        Mode::SelectGroupMembers { .. } => "ADD GROUP",
        Mode::AddArt { .. } => "ADD ART",
        Mode::LoadArtFile { .. } => "LOAD ART",
        Mode::Settings { .. } => "SETTINGS",
        Mode::TableAddColumn { .. } => "ADD COL",
        Mode::TableRemoveColumn { .. } => "REMOVE COL",
        Mode::TableEditCellProps { .. } => "EDIT CELLS",
        Mode::FrameMenu => "FRAME",
        Mode::FrameMove { .. } | Mode::FrameMovePlace { .. } => "MOVE FRAME",
        Mode::FrameOverlay { .. } => "OVERLAY FRAME",
    };
    let dirty_str = if state.dirty { " [modified]" } else { "" };
    // Replace newlines so a multi-line label value doesn't scroll the terminal.
    let status: String = state.status_message.as_deref().unwrap_or("")
        .chars().map(|c| if c == '\n' { '↵' } else { c }).collect();

    queue!(
        stdout,
        style::SetAttribute(style::Attribute::Dim),
        style::Print(format!(
            " {mode_str} | Frame {}/{frame_count}{dirty_str} {status}",
            current + 1,
        )),
        style::SetAttribute(style::Attribute::Reset),
    )?;

    Ok(())
}

/// One cell in the frame bar: a single frame, or a collapsed auto-play range
/// `[start, end)` shown as one "lo-hi" block. Frames under an auto-play
/// animation advance on their own, so the whole sequence reads as one unit.
enum Seg {
    Single(usize),
    Range(usize, usize),
}

impl Seg {
    fn contains(&self, frame: usize) -> bool {
        match *self {
            Seg::Single(f) => f == frame,
            Seg::Range(s, e) => s <= frame && frame < e,
        }
    }

    /// The bracketed label, 1-based, e.g. `[ 3]` or `[10-20]`. A range's end is
    /// exclusive, so its 1-based last frame is exactly `e`.
    fn label(&self) -> String {
        match *self {
            Seg::Single(f) => format!("[{:>2}]", f + 1),
            Seg::Range(s, e) => format!("[{}-{}]", s + 1, e),
        }
    }
}

/// Disjoint, sorted auto-play ranges, merging any that **strictly overlap**
/// (share a frame). Overlapping animations auto-advance as one continuous
/// sequence, so they collapse to a single timeline range; adjacent-but-disjoint
/// spans (a manual step sits between them) stay separate.
fn merged_autoplay_ranges(state: &EditorState) -> Vec<(usize, usize)> {
    let mut rs: Vec<(usize, usize)> = state
        .source
        .objects
        .iter()
        .filter_map(|o| match o {
            SceneObject::Animation(a) if a.auto_play && a.frames.start < a.frames.end => {
                Some((a.frames.start, a.frames.end))
            }
            _ => None,
        })
        .collect();
    rs.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in rs {
        match merged.last_mut() {
            Some(last) if s < last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// Build the frame-bar segments: each collapsed auto-play range as one cell,
/// every other frame as its own cell.
fn build_segments(state: &EditorState) -> Vec<Seg> {
    let frame_count = state.source.frame_count;
    let ranges = merged_autoplay_ranges(state);
    let mut segs = Vec::new();
    let mut f = 0;
    while f < frame_count {
        if let Some(&(s, e)) = ranges.iter().find(|&&(s, e)| s <= f && f < e) {
            let e = e.min(frame_count);
            segs.push(Seg::Range(s, e));
            f = e;
        } else {
            segs.push(Seg::Single(f));
            f += 1;
        }
    }
    segs
}

fn render_frame_bar(
    stdout: &mut io::Stdout,
    width: usize,
    segs: &[Seg],
    current: usize,
) -> anyhow::Result<()> {
    queue!(stdout, style::Print(" "))?;

    // Does the whole bar fit? Each cell is its label plus a trailing space.
    let total: usize = segs.iter().map(|s| s.label().chars().count() + 1).sum::<usize>() + 1;
    if total <= width {
        for seg in segs {
            render_seg(stdout, seg, current)?;
        }
        return Ok(());
    }

    // Abbreviated view over the segment list: first, the current segment's
    // vicinity, and last, with "..." marking the skipped gaps.
    let cur = segs.iter().position(|s| s.contains(current)).unwrap_or(0);
    let last = segs.len().saturating_sub(1);
    let mut to_show: Vec<usize> = vec![0];
    if cur > 1 {
        to_show.push(cur - 1);
    }
    to_show.push(cur);
    if cur + 1 < last {
        to_show.push(cur + 1);
    }
    to_show.push(last);
    to_show.sort();
    to_show.dedup();

    let mut prev: Option<usize> = None;
    for &i in &to_show {
        if let Some(p) = prev {
            if i > p + 1 {
                queue!(stdout, style::Print("... "))?;
            }
        }
        render_seg(stdout, &segs[i], current)?;
        prev = Some(i);
    }
    Ok(())
}

fn render_seg(stdout: &mut io::Stdout, seg: &Seg, current: usize) -> anyhow::Result<()> {
    let label = seg.label();
    if seg.contains(current) {
        queue!(
            stdout,
            style::SetAttribute(style::Attribute::Reverse),
            style::Print(&label),
            style::SetAttribute(style::Attribute::Reset),
            style::Print(" "),
        )?;
    } else {
        queue!(stdout, style::Print(format!("{label} ")))?;
    }
    Ok(())
}
