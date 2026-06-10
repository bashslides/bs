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
use crate::types::{Cell, Color, CommandRegion, Frame, NamedColor, PlayablePresentation, Style};

/// Rows reserved above the canvas for the menu bar.
const CANVAS_OFFSET: u16 = 1;

/// Default kill-after time for a command that does not set its own timeout.
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// A binary currently executing for the active frame. The child runs with piped
/// stdio (it can never touch the real terminal) and is read on background
/// threads, so the event loop stays responsive — arrow keys kill it and move on.
struct RunningCommand {
    region: CommandRegion,
    child: Child,
    start: Instant,
    timeout: Duration,
    rx: Receiver<Vec<u8>>,
    /// Accumulated stdout+stderr bytes.
    out: Vec<u8>,
}

pub struct Player {
    presentation: PlayablePresentation,
    current_frame: usize,
    grid: Vec<Vec<Cell>>,
    fullscreen: bool,
    running: Option<RunningCommand>,
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
        self.render_menubar(stdout)?;
        self.render_full(stdout)?;
        self.render_status(stdout)?;
        self.maybe_start_command(stdout)?;

        loop {
            // Drive any running command: drain output, repaint, finalize on exit.
            if self.running.is_some() {
                self.service_command(stdout)?;
            }

            // Poll briefly while a command runs so output streams in; otherwise
            // wait longer (the loop is idle until the next keypress).
            let poll = if self.running.is_some() {
                Duration::from_millis(30)
            } else {
                Duration::from_millis(200)
            };
            if !event::poll(poll)? {
                continue;
            }

            match event::read()? {
                event::Event::Key(key) => {
                    use event::KeyCode::*;
                    match key.code {
                        // Quit also stops any running binary.
                        Char('q') | Esc => {
                            self.kill_running();
                            break;
                        }
                        // Navigation always interrupts a running binary and
                        // moves on — a slow command can never trap the deck.
                        Right | Char(' ') | Enter => self.nav_forward(stdout)?,
                        Left => self.nav_back(stdout)?,
                        Home => self.nav_to(0, stdout)?,
                        End => {
                            let last = self.presentation.frames.len().saturating_sub(1);
                            self.nav_to(last, stdout)?;
                        }
                        F(11) => {
                            self.fullscreen = !self.fullscreen;
                            if self.fullscreen {
                                stdout.write_all(b"\x1b[10;1t")?;
                            } else {
                                stdout.write_all(b"\x1b[10;0t")?;
                            }
                            stdout.flush()?;
                        }
                        _ => {}
                    }
                }
                event::Event::Resize(_, _) => {
                    self.render_menubar(stdout)?;
                    self.render_full(stdout)?;
                    self.render_status(stdout)?;
                }
                _ => {}
            }
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
        let w = self.presentation.contract.width as usize;
        let h = self.presentation.contract.height as usize;
        self.grid = vec![vec![Cell::default(); w]; h];
        for i in 0..=target {
            self.apply_frame(i)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Terminal output
    // -----------------------------------------------------------------------

    fn render_menubar(&self, stdout: &mut io::Stdout) -> Result<()> {
        let items: &[&str] = &[
            "[←] prev",
            "[→][Space] next",
            "[Home] first",
            "[End] last",
            "[q][Esc] quit",
            "[F11] full",
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
        for (y, row) in self.grid.iter().enumerate() {
            queue!(stdout, cursor::MoveTo(0, y as u16 + CANVAS_OFFSET))?;
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
                        cursor::MoveTo(change.x, change.y + CANVAS_OFFSET),
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
        let status_y = self.presentation.contract.height + CANVAS_OFFSET;
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
        let timeout = Duration::from_millis(region.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));

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
            let timed_out = rc.start.elapsed() > rc.timeout;
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
            queue!(stdout, cursor::MoveTo(x, y + row + CANVAS_OFFSET))?;
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
