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
use state::{EditorState, FrameClipboard, Mode};
use ui::Layout;

pub struct Editor {
    /// Every open presentation. Always non-empty; `active` indexes into it.
    decks: Vec<EditorState>,
    /// Index of the deck currently being edited / rendered.
    active: usize,
    /// The cross-deck frame clipboard: a contiguous block of frames yanked from
    /// one deck, ready to paste into another. Shared across all decks.
    frame_clip: Option<FrameClipboard>,
}

impl Editor {
    /// Open a single presentation (CLI convenience / back-compat wrapper).
    pub fn open(path: &str) -> Result<Self> {
        Self::open_many(std::slice::from_ref(&path.to_string()))
    }

    /// Open one or more presentations as parallel decks; the first is active.
    pub fn open_many(paths: &[String]) -> Result<Self> {
        let mut decks = Vec::with_capacity(paths.len());
        for p in paths {
            decks.push(EditorState::open(p)?);
        }
        if decks.is_empty() {
            decks.push(EditorState::open("untitled.json")?);
        }
        Ok(Editor { decks, active: 0, frame_clip: None })
    }

    fn active(&self) -> &EditorState {
        &self.decks[self.active]
    }

    fn active_mut(&mut self) -> &mut EditorState {
        &mut self.decks[self.active]
    }

    /// Refresh the active deck's [`state::WorkspaceView`] so the menu bar and the
    /// presentations switcher panel can show the other open decks and the
    /// cross-deck frame clipboard while staying `&EditorState`-only.
    fn sync_workspace_view(&mut self) {
        let names: Vec<String> = self
            .decks
            .iter()
            .map(|d| {
                let base = std::path::Path::new(&d.file_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(d.file_path.as_str());
                if d.dirty {
                    format!("{base} *")
                } else {
                    base.to_string()
                }
            })
            .collect();
        let frames = self.frame_clip.as_ref().map(|c| c.frame_count).unwrap_or(0);
        let active = self.active;
        let view = &mut self.decks[active].workspace;
        view.deck_names = names;
        view.active = active;
        view.frame_clip_frames = frames;
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
            let action = input::handle_event(self.active_mut(), event);

            match action {
                Action::Continue => {}
                Action::Redraw => pending_redraw = true,
                Action::BlinkSelection => {
                    self.full_redraw(stdout)?;
                    thread::sleep(Duration::from_millis(100));
                    self.active_mut().blink_hidden = true;
                    self.full_redraw(stdout)?;
                    thread::sleep(Duration::from_millis(100));
                    self.active_mut().blink_hidden = false;
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::ToggleFullscreen => {
                    let f = !self.active().fullscreen;
                    self.active_mut().fullscreen = f;
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::SwitchDeck(i) => {
                    if i < self.decks.len() {
                        self.active = i;
                    }
                    self.active_mut().mode = Mode::Normal;
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::OpenDeck(path) => {
                    self.open_or_focus(&path);
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::CopyFrameBlock { lo, hi } => {
                    let clip = state::copy_frame_block(&self.active().source, lo, hi);
                    let n = clip.frame_count;
                    self.frame_clip = Some(clip);
                    let st = self.active_mut();
                    st.mode = Mode::Normal;
                    st.status_message = Some(format!(
                        "Yanked {n} frame(s) — switch decks ([p]), then [f]rame → [p]aste frames"
                    ));
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::PasteFrameBlock { target, before } => {
                    self.paste_frame_block(target, before);
                    self.full_redraw(stdout)?;
                    pending_redraw = false;
                }
                Action::Quit => {
                    if self.handle_quit(stdout)? {
                        break;
                    }
                    pending_redraw = false;
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

    /// Open `path` as a new deck, or focus the existing one if it's already open.
    fn open_or_focus(&mut self, path: &str) {
        if let Some(i) = self.decks.iter().position(|d| d.file_path == path) {
            self.active = i;
            self.active_mut().mode = Mode::Normal;
            self.active_mut().status_message = Some(format!("Switched to {path}"));
            return;
        }
        match EditorState::open(path) {
            Ok(st) => {
                self.decks.push(st);
                self.active = self.decks.len() - 1;
                self.active_mut().status_message = Some(format!("Opened {path}"));
            }
            Err(e) => {
                self.active_mut().mode = Mode::Normal;
                self.active_mut().status_message = Some(format!("Open failed: {e}"));
            }
        }
    }

    /// Paste the cross-deck frame clipboard into the active deck at `target`.
    fn paste_frame_block(&mut self, target: usize, before: bool) {
        let Some(clip) = self.frame_clip.clone() else {
            self.active_mut().mode = Mode::Normal;
            return;
        };
        let mismatch =
            clip.width != self.active().source.width || clip.height != self.active().source.height;
        let (new_current, count) =
            state::paste_frame_block(&mut self.active_mut().source, &clip, target, before);
        let st = self.active_mut();
        st.current_frame = new_current;
        st.dirty = true;
        st.mode = Mode::Normal;
        st.status_message = Some(if mismatch {
            format!(
                "Pasted {count} frame(s) — ⚠ source canvas {}×{} differs from this deck",
                clip.width, clip.height
            )
        } else {
            format!("Pasted {count} frame(s)")
        });
    }

    /// Handle a quit request across all open decks. Returns `true` if the editor
    /// should exit. Blocks exit while any deck has unsaved changes until the user
    /// confirms (q again to discard all) or saves the active deck (Ctrl-s).
    fn handle_quit(&mut self, stdout: &mut io::Stdout) -> Result<bool> {
        let dirty: Vec<String> = self
            .decks
            .iter()
            .filter(|d| d.dirty)
            .map(|d| {
                std::path::Path::new(&d.file_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(d.file_path.as_str())
                    .to_string()
            })
            .collect();
        if dirty.is_empty() {
            return Ok(true);
        }
        self.active_mut().status_message = Some(format!(
            "Unsaved: {} — q again to discard all, Ctrl-s saves this deck",
            dirty.join(", ")
        ));
        self.full_redraw(stdout)?;
        if let event::Event::Key(k) = event::read()? {
            if k.code == event::KeyCode::Char('q') {
                return Ok(true);
            }
            if k.code == event::KeyCode::Char('s')
                && k.modifiers.contains(event::KeyModifiers::CONTROL)
            {
                match self.active_mut().save() {
                    Ok(()) => {
                        self.full_redraw(stdout)?;
                        thread::sleep(Duration::from_secs(2));
                    }
                    Err(e) => {
                        self.active_mut().status_message = Some(format!("Save failed: {e}"));
                    }
                }
            }
        }
        self.active_mut().status_message = None;
        self.full_redraw(stdout)?;
        Ok(false)
    }

    fn full_redraw(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        self.sync_workspace_view();
        let state = self.active();
        let (term_w, term_h) = terminal::size()?;
        let layout = Layout::compute(term_w, term_h, &state.mode, state.fullscreen);

        // Queue the clear (don't `execute!`) so it is flushed together with the
        // full repaint below in the single `flush()` at the end of this function.
        // `execute!` would flush the blank screen on its own, producing a visible
        // blank-then-paint flash on every keypress.
        queue!(stdout, terminal::Clear(terminal::ClearType::All))?;

        // In fullscreen ("no bars") mode the menu bar and timeline are hidden so
        // the canvas fills the whole screen.
        if !state.fullscreen {
            menubar::render_menubar(stdout, &layout, state)?;
        }

        // Draw canvas
        preview::render_canvas_production(stdout, &layout, state)?;

        // Draw right panel (handles AddObject, MultiSelect, SelectAction, Confirm, EditProperties, AnimateProperty)
        panel::render_right_panel(stdout, &layout, state)?;

        if !state.fullscreen {
            timeline::render_timeline(stdout, &layout, state)?;
        }

        // Multi-line text editing draws a centred overlay over the canvas; a
        // no-op unless a Text property value is currently being edited.
        panel::render_text_overlay(stdout, &layout, state)?;
        // The "Save As" filename popup (no-op unless in that mode).
        panel::render_save_as_overlay(stdout, &layout, state)?;

        stdout.flush()?;
        Ok(())
    }
}
