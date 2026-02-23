//! Player — the runtime playback controller.
//!
//! Consumes a `PlayablePresentation` and drives it to the terminal.
//! The player does not interpret semantics or perform rendering decisions;
//! it treats the presentation as an immutable, authoritative visual script.

use std::io::{self, Write};

use anyhow::{bail, Result};
use crossterm::{cursor, event, execute, queue, style, terminal};

use crate::menubar::print_menu_item;
use crate::types::{Cell, Color, Frame, NamedColor, PlayablePresentation, Style};

/// Rows reserved above the canvas for the menu bar.
const CANVAS_OFFSET: u16 = 1;

pub struct Player {
    presentation: PlayablePresentation,
    current_frame: usize,
    grid: Vec<Vec<Cell>>,
    fullscreen: bool,
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

        loop {
            match event::read()? {
                event::Event::Key(key) => {
                    use event::KeyCode::*;
                    match key.code {
                        Char('q') | Esc => break,
                        Right | Char(' ') | Enter => {
                            let last = self.presentation.frames.len().saturating_sub(1);
                            if self.current_frame < last {
                                self.current_frame += 1;
                                self.apply_frame(self.current_frame)?;
                                self.render_diff(stdout, self.current_frame)?;
                                self.render_status(stdout)?;
                            }
                        }
                        Left => {
                            if self.current_frame > 0 {
                                self.current_frame -= 1;
                                self.rebuild_grid(self.current_frame)?;
                                self.render_full(stdout)?;
                                self.render_status(stdout)?;
                            }
                        }
                        Home => {
                            self.current_frame = 0;
                            self.rebuild_grid(0)?;
                            self.render_full(stdout)?;
                            self.render_status(stdout)?;
                        }
                        End => {
                            let last = self.presentation.frames.len().saturating_sub(1);
                            self.current_frame = last;
                            self.rebuild_grid(last)?;
                            self.render_full(stdout)?;
                            self.render_status(stdout)?;
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
