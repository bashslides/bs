pub mod config;
mod input;
mod menubar;
mod object_defaults;
mod panel;
mod preview;
mod properties;
pub mod state;
mod textedit;
mod timeline;
mod ui;

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{cursor, event, execute, queue, terminal};

use input::Action;
use state::EditorState;
use ui::Layout;

pub struct Editor {
    state: EditorState,
}

impl Editor {
    pub fn open(path: &str) -> Result<Self> {
        let state = EditorState::open(path)?;
        Ok(Editor { state })
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

        // Ask the terminal to disambiguate key events so modifier+Enter combos
        // (e.g. Shift-Enter to insert a newline in a cell) are reported distinctly.
        // Not all terminals support this; fall back silently when unsupported.
        let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
        if enhanced {
            let _ = execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
        }

        let result = self.main_loop(&mut stdout);

        if enhanced {
            let _ = execute!(stdout, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();

        result
    }

    fn main_loop(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        self.full_redraw(stdout)?;

        // When input arrives faster than we can paint (e.g. holding an arrow key
        // to scroll the property list), repainting once per event causes thrash.
        // Instead, `Action::Redraw` only marks the screen dirty; we coalesce by
        // deferring the actual paint until the pending-event queue has drained
        // (see the `event::poll` check after the match). At normal typing speed
        // the queue is empty between keys, so the redraw still happens immediately
        // — there is no added latency, only burst coalescing.
        let mut pending_redraw = false;

        loop {
            let event = event::read()?;
            let action = input::handle_event(&mut self.state, event);

            match action {
                Action::Continue => {}
                Action::Redraw => pending_redraw = true,
                Action::BlinkSelection => {
                    self.full_redraw(stdout)?;
                    thread::sleep(Duration::from_millis(100));
                    self.state.blink_hidden = true;
                    self.full_redraw(stdout)?;
                    thread::sleep(Duration::from_millis(100));
                    self.state.blink_hidden = false;
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::ToggleFullscreen => {
                    self.state.fullscreen = !self.state.fullscreen;
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
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
                        pending_redraw = false;
                    } else {
                        break;
                    }
                }
            }

            // Coalesce deferred redraws: paint only once the input burst has
            // drained. `event::poll(0)` is non-blocking, so between keystrokes at
            // normal speed this fires immediately; only while events are already
            // queued do we skip ahead and let the accumulated state changes batch
            // into a single repaint.
            if pending_redraw && !event::poll(Duration::from_secs(0))? {
                self.full_redraw(stdout)?;
                pending_redraw = false;
            }
        }

        Ok(())
    }

    fn full_redraw(&self, stdout: &mut io::Stdout) -> Result<()> {
        let (term_w, term_h) = terminal::size()?;
        let layout = Layout::compute(term_w, term_h, &self.state.mode, self.state.fullscreen);

        // Queue the clear (don't `execute!`) so it is flushed together with the
        // full repaint below in the single `flush()` at the end of this function.
        // `execute!` would flush the blank screen on its own, producing a visible
        // blank-then-paint flash on every keypress.
        queue!(stdout, terminal::Clear(terminal::ClearType::All))?;

        // In fullscreen ("no bars") mode the menu bar and timeline are hidden so
        // the canvas fills the whole screen.
        if !self.state.fullscreen {
            menubar::render_menubar(stdout, &layout, &self.state)?;
        }

        // Draw canvas
        preview::render_canvas_production(stdout, &layout, &self.state)?;

        // Draw right panel (handles AddObject, SelectObject, Confirm, EditProperties, AnimateProperty)
        panel::render_right_panel(stdout, &layout, &self.state)?;

        if !self.state.fullscreen {
            timeline::render_timeline(stdout, &layout, &self.state)?;
        }

        // Multi-line text editing draws a centred overlay over the canvas; a
        // no-op unless a Text property value is currently being edited.
        panel::render_text_overlay(stdout, &layout, &self.state)?;
        // The "Save As" filename popup (no-op unless in that mode).
        panel::render_save_as_overlay(stdout, &layout, &self.state)?;

        stdout.flush()?;
        Ok(())
    }
}
