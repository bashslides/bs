pub mod config;
mod input;
mod menubar;
mod object_defaults;
mod panel;
mod preview;
mod properties;
pub mod state;
mod timeline;
mod ui;

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::{cursor, event, execute, terminal};

use input::Action;
use state::EditorState;
use ui::Layout;

pub struct Editor {
    state: EditorState,
    fullscreen: bool,
}

impl Editor {
    pub fn open(path: &str) -> Result<Self> {
        let state = EditorState::open(path)?;
        Ok(Editor { state, fullscreen: false })
    }

    pub fn run(&mut self) -> Result<()> {
        let mut stdout = io::stdout();

        terminal::enable_raw_mode()?;
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(terminal::ClearType::All),
        )?;

        let result = self.main_loop(&mut stdout);

        let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();

        result
    }

    fn main_loop(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        self.full_redraw(stdout)?;

        loop {
            let event = event::read()?;
            let action = input::handle_event(&mut self.state, event);

            match action {
                Action::Continue => {}
                Action::Redraw => self.full_redraw(stdout)?,
                Action::BlinkSelection => {
                    self.full_redraw(stdout)?;
                    thread::sleep(Duration::from_millis(100));
                    self.state.blink_hidden = true;
                    self.full_redraw(stdout)?;
                    thread::sleep(Duration::from_millis(100));
                    self.state.blink_hidden = false;
                    self.full_redraw(stdout)?;
                }
                Action::ToggleFullscreen => {
                    self.fullscreen = !self.fullscreen;
                    if self.fullscreen {
                        stdout.write_all(b"\x1b[10;1t")?;
                    } else {
                        stdout.write_all(b"\x1b[10;0t")?;
                    }
                    stdout.flush()?;
                    self.full_redraw(stdout)?;
                }
                Action::Quit => {
                    if self.state.dirty {
                        self.state.status_message =
                            Some("Unsaved changes! q again to quit, Ctrl-s to save".into());
                        self.full_redraw(stdout)?;
                        // Wait for next key
                        if let event::Event::Key(k) = event::read()? {
                            if k.code == event::KeyCode::Char('q') {
                                break;
                            }
                            // Check for Ctrl-s
                            if k.code == event::KeyCode::Char('s')
                                && k.modifiers.contains(event::KeyModifiers::CONTROL)
                            {
                                match self.state.save() {
                                    Ok(()) => {
                                        // Show "Saved" for 2 seconds before clearing
                                        self.full_redraw(stdout)?;
                                        thread::sleep(Duration::from_secs(2));
                                    }
                                    Err(e) => {
                                        self.state.status_message =
                                            Some(format!("Save failed: {e}"));
                                    }
                                }
                            }
                        }
                        self.state.status_message = None;
                        self.full_redraw(stdout)?;
                    } else {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn full_redraw(&self, stdout: &mut io::Stdout) -> Result<()> {
        let (term_w, term_h) = terminal::size()?;
        let layout = Layout::compute(term_w, term_h, &self.state.mode);

        execute!(stdout, terminal::Clear(terminal::ClearType::All))?;

        // Draw menu bar
        menubar::render_menubar(stdout, &layout, &self.state)?;

        // Draw canvas
        preview::render_canvas_production(stdout, &layout, &self.state)?;

        // Draw right panel (handles AddObject, SelectObject, Confirm, EditProperties, AnimateProperty)
        panel::render_right_panel(stdout, &layout, &self.state)?;

        // Draw timeline
        timeline::render_timeline(stdout, &layout, &self.state)?;

        stdout.flush()?;
        Ok(())
    }
}
