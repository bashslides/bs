//! Player — the runtime playback controller.
//!
//! Consumes a `PlayablePresentation` and drives it to the terminal.
//! The player does not interpret semantics or perform rendering decisions;
//! it treats the presentation as an immutable, authoritative visual script.

use std::io::{self, Read, Write};
use std::process::{Child, Command as ProcCommand, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use crossterm::{cursor, event, execute, queue, style, terminal};

use crate::menubar::print_menu_item;
use crate::types::{
    Cell, Color, CommandRegion, Frame, LoopRegion, NamedColor, PlayablePresentation, Style,
};

/// Rows reserved above the canvas for the menu bar (when not in fullscreen).
const CANVAS_OFFSET: u16 = 1;

/// A binary currently executing for the active frame. The child runs with piped
/// stdio (it can never touch the real terminal) and is read on background
/// threads, so the event loop stays responsive — arrow keys kill it and move on.
struct RunningCommand {
    region: CommandRegion,
    child: Child,
    start: Instant,
    /// Kill-after duration, or `None` to run with no timeout.
    timeout: Option<Duration>,
    rx: Receiver<Vec<u8>>,
    /// Accumulated stdout+stderr bytes.
    out: Vec<u8>,
}

/// State for a loop that is currently auto-playing. A flat deck has at most one
/// active loop at a time (loops never overlap or nest).
struct LoopPlay {
    region: LoopRegion,
    /// Current sweep direction (only meaningful when `region.bounce`).
    forward: bool,
    /// Completed plays so far (used to stop after `region.count`).
    iterations: usize,
    /// When the next frame should be shown.
    deadline: Instant,
}

pub struct Player {
    presentation: PlayablePresentation,
    current_frame: usize,
    grid: Vec<Vec<Cell>>,
    fullscreen: bool,
    running: Option<RunningCommand>,
    loop_play: Option<LoopPlay>,
    /// When the deck should auto-advance because an auto-play animation covers
    /// the current frame boundary (and no loop is driving). `None` = wait for a
    /// keypress. Loops, when active, drive advancement instead (see `loop_play`).
    auto_deadline: Option<Instant>,
}

impl Player {
    pub fn new(presentation: PlayablePresentation) -> Self {
        let w = presentation.contract.width as usize;
        let h = presentation.contract.height as usize;
        Self {
            presentation,
            current_frame: 0,
            grid: vec![vec![Cell::default(); w]; h],
            fullscreen: false,
            running: None,
            loop_play: None,
            auto_deadline: None,
        }
    }

    /// Play the presentation in the terminal.
    ///
    /// Sets up the terminal, enters the event loop, and restores the terminal
    /// on exit (even on error).
    pub fn play(&mut self) -> Result<()> {
        let (term_w, term_h) = terminal::size()?;
        let need_w = self.presentation.contract.width;
        let need_h = self.presentation.contract.height;
        // +2: one row for menu bar, one row for status bar
        if term_w < need_w || term_h < need_h + 2 {
            bail!(
                "Terminal too small: need {}x{}, have {}x{}",
                need_w,
                need_h + 2,
                term_w,
                term_h,
            );
        }

        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(terminal::ClearType::All),
        )?;

        let result = self.run_loop(&mut stdout);

        // Always restore terminal state.
        let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();

        result
    }

    // -----------------------------------------------------------------------
    // Event loop
    // -----------------------------------------------------------------------

    fn run_loop(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        self.apply_frame(0)?;
        self.redraw_all(stdout)?;
        self.maybe_start_command(stdout)?;
        // Frame 0 may itself sit inside a loop, or under an auto-play animation.
        self.arm_loop(None);
        self.schedule_auto();

        loop {
            // Drive any running command: drain output, repaint, finalize on exit.
            if self.running.is_some() {
                self.service_command(stdout)?;
            }

            // Poll briefly while a command runs so output streams in; otherwise
            // wait longer (the loop is idle until the next keypress). While a
            // loop auto-plays, never wait past its next-frame deadline.
            let mut poll = if self.running.is_some() {
                Duration::from_millis(30)
            } else {
                Duration::from_millis(200)
            };
            if let Some(lp) = &self.loop_play {
                poll = poll.min(lp.deadline.saturating_duration_since(Instant::now()));
            }
            if let Some(dl) = self.auto_deadline {
                poll = poll.min(dl.saturating_duration_since(Instant::now()));
            }

            if !event::poll(poll)? {
                // No key arrived — advance on whichever timer elapsed. A loop, if
                // active, drives playback; otherwise an auto-play animation does.
                let now = Instant::now();
                if self.loop_play.as_ref().is_some_and(|lp| now >= lp.deadline) {
                    self.loop_tick(stdout)?;
                } else if self.auto_deadline.is_some_and(|dl| now >= dl) {
                    self.auto_tick(stdout)?;
                }
                continue;
            }

            match event::read()? {
                event::Event::Key(key) => {
                    use event::KeyCode::*;
                    match key.code {
                        // Quit also stops any running binary.
                        Char('q') => {
                            self.kill_running();
                            break;
                        }
                        // Esc leaves fullscreen first; otherwise it quits.
                        Esc => {
                            if self.fullscreen {
                                self.fullscreen = false;
                                self.redraw_all(stdout)?;
                            } else {
                                self.kill_running();
                                break;
                            }
                        }
                        // Navigation always interrupts a running binary and
                        // moves on — a slow command (or a loop) can never trap
                        // the deck. Inside a loop, → breaks out to the first
                        // frame after it and ← to the first frame before it.
                        Right | Char(' ') | Enter => {
                            if let Some(lp) = self.loop_play.take() {
                                let last = self.presentation.frames.len().saturating_sub(1);
                                let target = lp.region.end_frame.min(last);
                                self.nav_to(target, stdout)?;
                                self.arm_loop(Some(span(&lp.region)));
                            } else {
                                self.nav_forward(stdout)?;
                                self.arm_loop(None);
                            }
                        }
                        Left => {
                            if let Some(lp) = self.loop_play.take() {
                                let target = lp.region.start_frame.saturating_sub(1);
                                self.nav_to(target, stdout)?;
                                self.arm_loop(Some(span(&lp.region)));
                            } else {
                                self.nav_back(stdout)?;
                                self.arm_loop(None);
                            }
                        }
                        Home => {
                            self.stop_loop();
                            self.nav_to(0, stdout)?;
                            self.arm_loop(None);
                        }
                        End => {
                            self.stop_loop();
                            let last = self.presentation.frames.len().saturating_sub(1);
                            self.nav_to(last, stdout)?;
                            self.arm_loop(None);
                        }
                        // Toggle "no bars" fullscreen: hide the menu/status bars
                        // and give the canvas the whole screen.
                        Char('f') => {
                            self.fullscreen = !self.fullscreen;
                            self.redraw_all(stdout)?;
                        }
                        _ => {}
                    }
                    // After any navigation, re-arm the auto-play timer for the
                    // frame we landed on (disarmed while a loop drives playback).
                    self.schedule_auto();
                }
                event::Event::Resize(_, _) => {
                    self.redraw_all(stdout)?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Loop playback
    // -----------------------------------------------------------------------

    /// The loop region active on `frame`, if any.
    fn loop_region_for(&self, frame: usize) -> Option<LoopRegion> {
        self.presentation
            .loops
            .iter()
            .find(|l| frame >= l.start_frame && frame < l.end_frame)
            .cloned()
    }

    /// Begin auto-playing the loop on the current frame, if there is one and one
    /// is not already running. `exclude` suppresses re-arming a specific span —
    /// used right after a break-out so the loop we just left does not restart
    /// when the break-out target lands back inside it (e.g. a loop pinned to the
    /// deck's first or last frame).
    fn arm_loop(&mut self, exclude: Option<(usize, usize)>) {
        if self.loop_play.is_some() {
            return;
        }
        if let Some(region) = self.loop_region_for(self.current_frame) {
            if exclude == Some(span(&region)) {
                return;
            }
            // First wait crosses `current`→`current+1`; an auto-play animation
            // covering it sets the pace (min delay), else the loop's own delay.
            let d = self
                .auto_advance_delay(self.current_frame, true)
                .unwrap_or(region.delay_ms);
            let deadline = Instant::now() + Duration::from_millis(d.max(1));
            self.loop_play = Some(LoopPlay {
                region,
                forward: true,
                iterations: 0,
                deadline,
            });
        }
    }

    fn stop_loop(&mut self) {
        self.loop_play = None;
    }

    // -----------------------------------------------------------------------
    // Animation auto-play
    // -----------------------------------------------------------------------

    /// The effective auto-advance delay across the boundary leaving `from` in
    /// the given direction (forward = `from`→`from+1`, backward = `from`→`from-1`),
    /// taken as the **minimum** `delay_ms` over every auto-play animation for
    /// which that boundary is *internal* (both frames lie within its span).
    /// `None` when no auto-play animation covers the boundary.
    fn auto_advance_delay(&self, from: usize, forward: bool) -> Option<u64> {
        // The boundary is between `lo` and `lo + 1`.
        let lo = if forward { from } else { from.checked_sub(1)? };
        self.presentation
            .animations
            .iter()
            .filter(|a| a.auto_play && a.start_frame <= lo && lo + 1 < a.end_frame)
            .map(|a| a.delay_ms)
            .min()
    }

    /// (Re)arm the non-loop auto-advance timer for the current frame. A loop, if
    /// active, drives advancement itself, so the animation timer stays disarmed
    /// while one is running.
    fn schedule_auto(&mut self) {
        self.auto_deadline = if self.loop_play.is_some() {
            None
        } else {
            self.auto_advance_delay(self.current_frame, true)
                .map(|d| Instant::now() + Duration::from_millis(d.max(1)))
        };
    }

    /// Advance one frame because an auto-play animation's delay elapsed (no loop
    /// active). Re-arms the loop (in case we stepped into one) and the animation
    /// timer for the new frame.
    fn auto_tick(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let last = self.presentation.frames.len().saturating_sub(1);
        if self.current_frame < last {
            self.nav_forward(stdout)?;
            self.arm_loop(None);
        }
        self.schedule_auto();
        Ok(())
    }

    /// Advance the active loop by one frame (called when its delay elapses).
    fn loop_tick(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let Some(lp) = self.loop_play.as_ref() else {
            return Ok(());
        };
        let region = lp.region.clone();
        let (next, next_forward, completed) = loop_next(
            region.start_frame,
            region.end_frame,
            self.current_frame,
            lp.forward,
            region.bounce,
        );
        let iterations = lp.iterations + usize::from(completed);

        // A finite loop that has played its full count stops and continues the
        // deck just past the loop (an infinite loop, count 0, never gets here).
        if region.count != 0 && iterations >= region.count {
            self.stop_loop();
            let last = self.presentation.frames.len().saturating_sub(1);
            let target = region.end_frame.min(last);
            self.nav_to(target, stdout)?;
            self.arm_loop(Some(span(&region)));
            return Ok(());
        }

        // Otherwise show the next loop frame. A loop step can jump backward
        // (bounce / restart), so rebuild the grid from scratch rather than
        // diffing — same path as `nav_back`.
        self.kill_running();
        self.current_frame = next;
        self.rebuild_grid(next)?;
        self.render_full(stdout)?;
        self.render_status(stdout)?;
        self.maybe_start_command(stdout)?;
        // The next wait crosses `next`→ in `next_forward`; let an auto-play
        // animation covering that boundary set the pace, else the loop's delay.
        let next_delay = self
            .auto_advance_delay(next, next_forward)
            .unwrap_or(region.delay_ms);
        if let Some(lp) = self.loop_play.as_mut() {
            lp.forward = next_forward;
            lp.iterations = iterations;
            lp.deadline = Instant::now() + Duration::from_millis(next_delay.max(1));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    fn nav_forward(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let last = self.presentation.frames.len().saturating_sub(1);
        if self.current_frame >= last {
            return Ok(());
        }
        // If the current frame had a command (live or finished), its output is
        // overlaid on the grid — rebuild from scratch rather than diffing.
        let overlaid = self.running.is_some() || self.region_for(self.current_frame).is_some();
        self.kill_running();
        self.current_frame += 1;
        if overlaid {
            self.rebuild_grid(self.current_frame)?;
            self.render_full(stdout)?;
        } else {
            self.apply_frame(self.current_frame)?;
            self.render_diff(stdout, self.current_frame)?;
        }
        self.render_status(stdout)?;
        self.maybe_start_command(stdout)
    }

    fn nav_back(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        if self.current_frame == 0 {
            return Ok(());
        }
        self.kill_running();
        self.current_frame -= 1;
        self.rebuild_grid(self.current_frame)?;
        self.render_full(stdout)?;
        self.render_status(stdout)?;
        self.maybe_start_command(stdout)
    }

    fn nav_to(&mut self, target: usize, stdout: &mut io::Stdout) -> Result<()> {
        self.kill_running();
        self.current_frame = target;
        self.rebuild_grid(target)?;
        self.render_full(stdout)?;
        self.render_status(stdout)?;
        self.maybe_start_command(stdout)
    }

    // -----------------------------------------------------------------------
    // Grid management
    // -----------------------------------------------------------------------

    fn apply_frame(&mut self, index: usize) -> Result<()> {
        match &self.presentation.frames[index] {
            Frame::Full { cells } => {
                self.grid = cells.clone();
            }
            Frame::Diff { changes } => {
                for change in changes {
                    let x = change.x as usize;
                    let y = change.y as usize;
                    if y < self.grid.len() && x < self.grid[0].len() {
                        self.grid[y][x] = change.cell.clone();
                    }
                }
            }
        }
        Ok(())
    }

    fn rebuild_grid(&mut self, target: usize) -> Result<()> {
        // Full replay shares one implementation with the editor preview and the
        // test harness (see `PlayablePresentation::grid_at`).
        self.grid = self.presentation.grid_at(target);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Terminal output
    // -----------------------------------------------------------------------

    /// Rows reserved above the canvas: one for the menu bar, or none in
    /// fullscreen ("no bars") mode where the canvas owns the whole screen.
    fn canvas_offset(&self) -> u16 {
        if self.fullscreen {
            0
        } else {
            CANVAS_OFFSET
        }
    }

    /// Clear and repaint everything for the current fullscreen state: the menu
    /// and status bars are drawn only when not in fullscreen (`render_status`
    /// self-guards on `fullscreen`; `render_menubar` is gated here).
    fn redraw_all(&self, stdout: &mut io::Stdout) -> Result<()> {
        queue!(stdout, terminal::Clear(terminal::ClearType::All))?;
        if !self.fullscreen {
            self.render_menubar(stdout)?;
        }
        self.render_full(stdout)?;
        self.render_status(stdout)?;
        Ok(())
    }

    fn render_menubar(&self, stdout: &mut io::Stdout) -> Result<()> {
        let items: &[&str] = &[
            "[←] prev",
            "[→][Space] next",
            "[Home] first",
            "[End] last",
            "[q][Esc] quit",
            "[f]ull",
        ];

        queue!(
            stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(terminal::ClearType::CurrentLine),
            style::Print(" "),
        )?;
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                queue!(stdout, style::Print("  "))?;
            }
            print_menu_item(stdout, item)?;
        }
        stdout.flush()?;
        Ok(())
    }

    fn render_full(&self, stdout: &mut io::Stdout) -> Result<()> {
        let offset = self.canvas_offset();
        for (y, row) in self.grid.iter().enumerate() {
            queue!(stdout, cursor::MoveTo(0, y as u16 + offset))?;
            for cell in row {
                let cs = to_content_style(&cell.style);
                queue!(
                    stdout,
                    style::PrintStyledContent(style::StyledContent::new(cs, cell.ch))
                )?;
            }
        }
        stdout.flush()?;
        Ok(())
    }

    fn render_diff(&self, stdout: &mut io::Stdout, frame_index: usize) -> Result<()> {
        match &self.presentation.frames[frame_index] {
            Frame::Diff { changes } => {
                for change in changes {
                    let cs = to_content_style(&change.cell.style);
                    queue!(
                        stdout,
                        cursor::MoveTo(change.x, change.y + self.canvas_offset()),
                        style::PrintStyledContent(style::StyledContent::new(cs, change.cell.ch)),
                    )?;
                }
                stdout.flush()?;
            }
            Frame::Full { .. } => {
                // Full frame — just re-render everything.
                self.render_full(stdout)?;
            }
        }
        Ok(())
    }

    fn render_status(&self, stdout: &mut io::Stdout) -> Result<()> {
        // Fullscreen ("no bars") owns the whole screen — no footer at all. Guard
        // here so every caller (navigation, loop steps, full repaint) honours it.
        if self.fullscreen {
            return Ok(());
        }
        let status_y = self.presentation.contract.height + self.canvas_offset();
        let (_, term_h) = terminal::size()?;
        if status_y >= term_h {
            return Ok(()); // No room for status bar.
        }

        let total = self.presentation.frames.len();
        let status = format!(
            " Frame {}/{} | \u{2190}\u{2192}: navigate | q: quit ",
            self.current_frame + 1,
            total,
        );

        let mut cs = style::ContentStyle::default();
        cs.attributes.set(style::Attribute::Dim);

        queue!(
            stdout,
            cursor::MoveTo(0, status_y),
            style::PrintStyledContent(style::StyledContent::new(cs, status)),
        )?;
        stdout.flush()?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Command execution
    // -----------------------------------------------------------------------

    /// The command spec active on `frame`, if any.
    fn region_for(&self, frame: usize) -> Option<CommandRegion> {
        self.presentation
            .commands
            .iter()
            .find(|c| frame >= c.start_frame && frame < c.end_frame)
            .cloned()
    }

    /// Start the command for the current frame, if there is one.
    fn maybe_start_command(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        let Some(region) = self.region_for(self.current_frame) else {
            return Ok(());
        };
        self.start_command(region, stdout)
    }

    /// Spawn the binary with piped stdio and background readers. A spawn failure
    /// is rendered as an error in the box rather than crashing the player.
    fn start_command(&mut self, region: CommandRegion, stdout: &mut io::Stdout) -> Result<()> {
        let timeout = region.timeout_secs.map(Duration::from_secs);

        let mut cmd = ProcCommand::new(&region.command);
        cmd.args(&region.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &region.cwd {
            cmd.current_dir(cwd);
        }

        match cmd.spawn() {
            Ok(mut child) => {
                let (tx, rx) = mpsc::channel();
                if let Some(out) = child.stdout.take() {
                    spawn_reader(out, tx.clone());
                }
                if let Some(err) = child.stderr.take() {
                    spawn_reader(err, tx);
                }
                self.running = Some(RunningCommand {
                    region,
                    child,
                    start: Instant::now(),
                    timeout,
                    rx,
                    out: Vec::new(),
                });
            }
            Err(e) => {
                let msg = format!("failed to run '{}': {e}", region.command);
                self.paint_command(stdout, &region, Some(false), msg.as_bytes())?;
            }
        }
        Ok(())
    }

    /// Stop the running binary (if any) and reap it.
    fn kill_running(&mut self) {
        if let Some(mut rc) = self.running.take() {
            let _ = rc.child.kill();
            let _ = rc.child.wait();
        }
    }

    /// Service the running command: drain output, repaint, and on exit (or
    /// timeout) finalize with a ✓ / ✗ indicator. Called once per loop tick.
    fn service_command(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        enum Outcome {
            Running,
            Done(bool, Vec<u8>),
        }

        let outcome = {
            let Some(rc) = self.running.as_mut() else {
                return Ok(());
            };
            while let Ok(chunk) = rc.rx.try_recv() {
                rc.out.extend_from_slice(&chunk);
            }
            let timed_out = rc.timeout.is_some_and(|t| rc.start.elapsed() > t);
            if timed_out {
                let _ = rc.child.kill();
            }
            match rc.child.try_wait() {
                Ok(Some(status)) => {
                    // Child exited: drain the rest (readers hit EOF and finish).
                    while let Ok(chunk) = rc.rx.recv() {
                        rc.out.extend_from_slice(&chunk);
                    }
                    let mut out = std::mem::take(&mut rc.out);
                    if timed_out {
                        out.extend_from_slice(b"\n[timed out]");
                    }
                    Outcome::Done(status.success() && !timed_out, out)
                }
                Ok(None) if timed_out => {
                    let _ = rc.child.wait();
                    while let Ok(chunk) = rc.rx.recv() {
                        rc.out.extend_from_slice(&chunk);
                    }
                    let mut out = std::mem::take(&mut rc.out);
                    out.extend_from_slice(b"\n[timed out]");
                    Outcome::Done(false, out)
                }
                Ok(None) => Outcome::Running,
                Err(_) => Outcome::Done(false, std::mem::take(&mut rc.out)),
            }
        };

        let region = self.running.as_ref().unwrap().region.clone();
        match outcome {
            Outcome::Running => {
                let out = self.running.as_ref().unwrap().out.clone();
                self.paint_command(stdout, &region, None, &out)?;
            }
            Outcome::Done(success, out) => {
                self.running = None;
                self.paint_command(stdout, &region, Some(success), &out)?;
            }
        }
        Ok(())
    }

    /// Paint captured output into the box interior, tailing to the last lines
    /// that fit. `status` is `None` while running, `Some(true/false)` on exit
    /// (drawing a ✓ / ✗ on the top edge).
    fn paint_command(
        &mut self,
        stdout: &mut io::Stdout,
        region: &CommandRegion,
        status: Option<bool>,
        out: &[u8],
    ) -> Result<()> {
        let w = region.w as usize;
        let h = region.h as usize;
        let rows = layout_output(out, w, h);

        let gh = self.grid.len();
        let gw = if gh > 0 { self.grid[0].len() } else { 0 };

        for (row, line) in rows.iter().enumerate() {
            let gy = region.y as usize + row;
            if gy >= gh {
                break;
            }
            for (col, ch) in line.chars().enumerate() {
                let gx = region.x as usize + col;
                if gx >= gw {
                    break;
                }
                self.grid[gy][gx] = Cell {
                    ch,
                    style: region.style.clone(),
                };
            }
        }

        if let Some(success) = status {
            let sx = region.status_x as usize;
            let sy = region.status_y as usize;
            if sy < gh && sx < gw {
                let (ch, color) = if success {
                    ('✓', NamedColor::Green)
                } else {
                    ('✗', NamedColor::Red)
                };
                self.grid[sy][sx] = Cell {
                    ch,
                    style: Style {
                        fg: Some(Color::Named(color)),
                        bold: true,
                        ..Style::default()
                    },
                };
            }
        }

        self.render_region(stdout, region.x, region.y, region.w, region.h)?;
        if status.is_some() {
            self.render_region(stdout, region.status_x, region.status_y, 1, 1)?;
        }
        Ok(())
    }

    /// Render a rectangular slice of the grid to the terminal.
    fn render_region(&self, stdout: &mut io::Stdout, x: u16, y: u16, w: u16, h: u16) -> Result<()> {
        let gh = self.grid.len();
        for row in 0..h {
            let gy = (y + row) as usize;
            if gy >= gh {
                break;
            }
            let gw = self.grid[gy].len();
            queue!(stdout, cursor::MoveTo(x, y + row + self.canvas_offset()))?;
            for col in 0..w {
                let gx = (x + col) as usize;
                if gx >= gw {
                    break;
                }
                let cell = &self.grid[gy][gx];
                let cs = to_content_style(&cell.style);
                queue!(
                    stdout,
                    style::PrintStyledContent(style::StyledContent::new(cs, cell.ch))
                )?;
            }
        }
        stdout.flush()?;
        Ok(())
    }
}

/// The `(start, end)` span of a loop region, used as an identity key when
/// suppressing an immediate re-arm after breaking out.
fn span(r: &LoopRegion) -> (usize, usize) {
    (r.start_frame, r.end_frame)
}

/// Compute the next frame of a loop given the current position and direction.
///
/// Returns `(next_frame, next_forward, completed_iteration)`:
/// - **non-bounce**: plays `start..end` forward, then jumps back to `start`;
///   each wrap to `start` counts as one completed iteration.
/// - **bounce**: plays forward to `end-1`, then backward to `start` (a triangle
///   wave, endpoints shown once per turn); returning to `start` completes one
///   iteration.
///
/// A single-frame range (`end == start + 1`) has nowhere to move, so it stays
/// put and completes an iteration on every tick.
fn loop_next(
    start: usize,
    end: usize,
    current: usize,
    forward: bool,
    bounce: bool,
) -> (usize, bool, bool) {
    let last = end.saturating_sub(1);
    let (next, next_forward) = if !bounce {
        // Forward sweep; wrap from the last frame back to the first.
        if current < last {
            (current + 1, true)
        } else {
            (start, true)
        }
    } else if last == start {
        // Single-frame range: nowhere to move.
        (start, true)
    } else if forward {
        if current < last {
            (current + 1, true)
        } else {
            (last - 1, false) // turn around at the top
        }
    } else if current > start {
        (current - 1, false)
    } else {
        (start + 1, true) // turn around at the bottom
    };
    // Landing back on the first frame (a wrap, or the bottom of a bounce) is one
    // completed pass.
    (next, next_forward, next == start)
}

/// Read a child pipe to EOF on a background thread, forwarding chunks. The
/// thread exits on EOF, read error, or when the receiver is dropped.
fn spawn_reader<R: Read + Send + 'static>(mut r: R, tx: Sender<Vec<u8>>) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Lay captured bytes out into a `w`×`h` text region: strip ANSI, keep the last
/// `h` lines (tail), and clip/pad each line to exactly `w` columns. Pure, so the
/// player's output rendering can be tested without a terminal.
pub fn layout_output(bytes: &[u8], w: usize, h: usize) -> Vec<String> {
    let text = strip_ansi(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.len().saturating_sub(h);
    (0..h)
        .map(|row| {
            let line = lines.get(first + row).copied().unwrap_or("");
            let mut s: String = line.chars().take(w).collect();
            let len = s.chars().count();
            if len < w {
                s.extend(std::iter::repeat(' ').take(w - len));
            }
            s
        })
        .collect()
}

/// Strip ANSI escape sequences and carriage returns so output paints cleanly
/// into the fixed cell grid.
fn strip_ansi(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    // Consume until the final byte (@-~) of the CSI sequence.
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                } else {
                    chars.next(); // drop the single escaped char
                }
            }
            '\r' => {}
            '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Style conversion
// ---------------------------------------------------------------------------

pub fn to_content_style(s: &Style) -> style::ContentStyle {
    let mut cs = style::ContentStyle::default();
    if let Some(fg) = &s.fg {
        cs.foreground_color = Some(to_ct_color(fg));
    }
    if let Some(bg) = &s.bg {
        cs.background_color = Some(to_ct_color(bg));
    }
    if s.bold {
        cs.attributes.set(style::Attribute::Bold);
    }
    if s.dim {
        cs.attributes.set(style::Attribute::Dim);
    }
    cs
}

pub fn to_ct_color(c: &Color) -> style::Color {
    match c {
        Color::Named(n) => match n {
            NamedColor::Black => style::Color::Black,
            NamedColor::Red => style::Color::Red,
            NamedColor::Green => style::Color::Green,
            NamedColor::Yellow => style::Color::Yellow,
            NamedColor::Blue => style::Color::Blue,
            NamedColor::Magenta => style::Color::Magenta,
            NamedColor::Cyan => style::Color::Cyan,
            NamedColor::White => style::Color::White,
        },
        Color::Rgb { r, g, b } => style::Color::Rgb {
            r: *r,
            g: *g,
            b: *b,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{loop_next, Player};
    use crate::types::{
        AnimationRegion, Cell, Frame, PlayablePresentation, TerminalContract,
    };

    /// A player over `frames` blank frames carrying the given animation regions.
    fn player_with(frames: usize, animations: Vec<AnimationRegion>) -> Player {
        let pres = PlayablePresentation {
            contract: TerminalContract { width: 1, height: 1 },
            frames: (0..frames).map(|_| Frame::Full { cells: vec![vec![Cell::default()]] }).collect(),
            markers: Vec::new(),
            commands: Vec::new(),
            loops: Vec::new(),
            animations,
        };
        Player::new(pres)
    }

    fn anim(start: usize, end: usize, delay: u64) -> AnimationRegion {
        AnimationRegion { start_frame: start, end_frame: end, auto_play: true, delay_ms: delay }
    }

    #[test]
    fn auto_advance_delay_covers_only_internal_boundaries() {
        // Animation spans frames [2,5): internal forward boundaries are 2→3 and
        // 3→4 (both frames in span). 4→5 is NOT internal (5 is the exclusive end).
        let p = player_with(8, vec![anim(2, 5, 300)]);
        assert_eq!(p.auto_advance_delay(2, true), Some(300));
        assert_eq!(p.auto_advance_delay(3, true), Some(300));
        assert_eq!(p.auto_advance_delay(4, true), None); // boundary 4→5 not internal
        assert_eq!(p.auto_advance_delay(1, true), None); // before the span
    }

    #[test]
    fn auto_advance_delay_takes_the_minimum_over_overlapping_animations() {
        // Two auto-play animations overlap on the 4→5 boundary; the faster one
        // (200 ms) sets the pace there.
        let p = player_with(10, vec![anim(0, 6, 500), anim(3, 8, 200)]);
        assert_eq!(p.auto_advance_delay(1, true), Some(500)); // only the first covers 1→2
        assert_eq!(p.auto_advance_delay(4, true), Some(200)); // both cover 4→5 → min
        assert_eq!(p.auto_advance_delay(6, true), Some(200)); // only the second covers 6→7
    }

    #[test]
    fn auto_advance_delay_handles_backward_boundaries() {
        // Backward from frame 4 crosses 3→4, internal to [2,5).
        let p = player_with(8, vec![anim(2, 5, 300)]);
        assert_eq!(p.auto_advance_delay(4, false), Some(300));
        assert_eq!(p.auto_advance_delay(2, false), None); // boundary 1→2 not internal
        assert_eq!(p.auto_advance_delay(0, false), None); // no boundary below 0
    }

    #[test]
    fn auto_advance_delay_ignores_non_auto_play_animations() {
        let mut a = anim(2, 6, 300);
        a.auto_play = false;
        let p = player_with(8, vec![a]);
        assert_eq!(p.auto_advance_delay(3, true), None);
    }

    /// Replay a loop from `start` for `steps` ticks, collecting the frames shown
    /// and how many iterations completed.
    fn run(start: usize, end: usize, bounce: bool, steps: usize) -> (Vec<usize>, usize) {
        let mut frame = start;
        let mut forward = true;
        let mut iters = 0;
        let mut seq = vec![frame];
        for _ in 0..steps {
            let (next, nf, done) = loop_next(start, end, frame, forward, bounce);
            frame = next;
            forward = nf;
            iters += usize::from(done);
            seq.push(frame);
        }
        (seq, iters)
    }

    #[test]
    fn non_bounce_wraps_to_start() {
        // 5,6,7,8 then back to 5; each wrap is one completed iteration.
        let (seq, iters) = run(5, 9, false, 5);
        assert_eq!(seq, vec![5, 6, 7, 8, 5, 6]);
        assert_eq!(iters, 1);
    }

    #[test]
    fn bounce_ping_pongs_without_duplicating_endpoints() {
        // 5,6,7,8,7,6,5,6,8… — endpoints shown once per turn; one iteration is a
        // full there-and-back (return to start).
        let (seq, iters) = run(5, 9, true, 6);
        assert_eq!(seq, vec![5, 6, 7, 8, 7, 6, 5]);
        assert_eq!(iters, 1);
    }

    #[test]
    fn single_frame_range_completes_each_tick() {
        // A one-frame loop has nowhere to go; it just counts ticks.
        let (seq, iters) = run(3, 4, true, 3);
        assert_eq!(seq, vec![3, 3, 3, 3]);
        assert_eq!(iters, 3);
        let (_, iters_nb) = run(3, 4, false, 2);
        assert_eq!(iters_nb, 2);
    }

    #[test]
    fn two_frame_bounce_alternates() {
        // start..end = 5..7 → frames 5,6. Bounce: 5,6,5,6,… one iteration per
        // return to start.
        let (seq, iters) = run(5, 7, true, 4);
        assert_eq!(seq, vec![5, 6, 5, 6, 5]);
        assert_eq!(iters, 2);
    }
}
