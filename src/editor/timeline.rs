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

    // Row 1: the frame bar (slide range indicator) — always shown, including
    // while typing a jump/select, where it live-previews the chosen slides.
    queue!(
        stdout,
        cursor::MoveTo(0, y),
        terminal::Clear(terminal::ClearType::CurrentLine),
    )?;

    if frame_count == 0 {
        queue!(stdout, style::Print(" (no frames)"))?;
    } else {
        let segs = build_segments(state);
        // Frames highlighted alongside the current (scroll-cursor) frame: an
        // explicit selection, a range being placed, or — while typing a frame
        // jump/select — a live preview of the slides the input resolves to (so
        // the frame bar stays put and shows what's about to be selected).
        let live: Vec<usize> = match &state.mode {
            Mode::FrameSelected { frames } | Mode::FrameRangePlace { frames, .. } => frames.clone(),
            Mode::FrameSelectInput { buf, .. } => {
                super::state::parse_frame_selection(buf, frame_count).unwrap_or_default()
            }
            Mode::FrameJump { buf, .. } => buf
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|&n| (1..=frame_count).contains(&n))
                .map(|n| vec![n - 1])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        render_frame_bar(stdout, width, &segs, current, &live)?;
    }

    // Row 2: Mode + status — or, while typing a frame jump/select, the input
    // field with its instructions sitting on the same row right behind it.
    queue!(
        stdout,
        cursor::MoveTo(0, y + 1),
        terminal::Clear(terminal::ClearType::CurrentLine),
    )?;

    let input_field = match &state.mode {
        Mode::FrameJump { buf, cursor } => Some((
            "Jump to frame: ",
            buf.clone(),
            *cursor,
            format!("(1-{frame_count} · Enter: jump · Esc: cancel)"),
        )),
        Mode::FrameSelectInput { buf, cursor } => Some((
            "Select frames: ",
            buf.clone(),
            *cursor,
            "(e.g. 1,2,3 or 5-12 · Enter: select · Esc: cancel)".to_string(),
        )),
        _ => None,
    };
    if let Some((prefix, buf, cursor, instructions)) = input_field {
        // A parse error (⚠) takes the trailing slot; otherwise the static hint.
        let trailing = match state.status_message.as_deref() {
            Some(s) if s.starts_with('\u{26a0}') => s.to_string(),
            _ => instructions,
        };
        let display = format!("{prefix}{buf}   {trailing}");
        let caret = prefix.chars().count() + cursor;
        super::panel::draw_caret_line(stdout, 0, y + 1, &display, Some(caret), false, width)?;
        return Ok(());
    }

    let mode_str = match &state.mode {
        Mode::Normal => "NORMAL",
        Mode::AddObject { .. } => "ADD OBJECT",
        Mode::SelectObject { .. } => "SELECT OBJECT",
        Mode::SelectedObject { .. } => "SELECTED",
        Mode::ResizeObject { .. } => "RESIZE",
        Mode::EditProperties { .. } => "EDIT PROPERTIES",
        Mode::AnimateProperty { .. } => "ANIMATE",
        Mode::Confirm { .. } => "CONFIRM",
        Mode::MultiSelect { purpose, .. } => match purpose {
            super::state::MultiSelectPurpose::Group => "ADD GROUP",
            super::state::MultiSelectPurpose::Copy => "COPY SELECT",
        },
        Mode::PastePlacing { .. } => "PASTE",
        Mode::AddArt { .. } => "ADD ART",
        Mode::LoadArtFile { .. } => "LOAD ART",
        Mode::Settings { .. } => "SETTINGS",
        Mode::TableAddColumn { .. } => "ADD COL",
        Mode::TableRemoveColumn { .. } => "REMOVE COL",
        Mode::TableEditCellProps { .. } => "EDIT CELLS",
        Mode::SaveAs { .. } => "SAVE AS",
        Mode::FrameMenu => "FRAME",
        Mode::FrameJump { .. } => "JUMP",
        Mode::FrameSelectInput { .. } => "SELECT FRAMES",
        Mode::FrameSelected { .. } => "FRAMES SELECTED",
        Mode::FrameRangePlace { copy: false, .. } => "MOVE RANGE",
        Mode::FrameRangePlace { copy: true, .. } => "COPY RANGE",
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
    selected: &[usize],
) -> anyhow::Result<()> {
    queue!(stdout, style::Print(" "))?;

    // Does the whole bar fit? Each cell is its label plus a trailing space.
    let total: usize = segs.iter().map(|s| s.label().chars().count() + 1).sum::<usize>() + 1;
    if total <= width {
        for seg in segs {
            render_seg(stdout, seg, current, selected)?;
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
        render_seg(stdout, &segs[i], current, selected)?;
        prev = Some(i);
    }
    Ok(())
}

fn render_seg(stdout: &mut io::Stdout, seg: &Seg, current: usize, selected: &[usize]) -> anyhow::Result<()> {
    let label = seg.label();
    let highlight = seg.contains(current) || selected.iter().any(|&f| seg.contains(f));
    if highlight {
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
