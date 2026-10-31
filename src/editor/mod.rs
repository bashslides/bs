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
use crossterm::{event, queue, terminal};

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
    /// An existing file the user was warned about in save-as; Enter on the same
    /// path again overwrites it. Cleared once the save-as prompt closes.
    overwrite_ok: Option<String>,
}

/// A path's identity for "is this the same file?": canonical when it exists,
/// else its canonical parent plus file name, else the lexically absolute path.
fn normalize_path(path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    let parent = match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => std::path::Path::new("."),
    };
    match (parent.canonicalize(), p.file_name()) {
        (Ok(d), Some(name)) => d.join(name),
        _ => std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()),
    }
}

/// Whether two deck paths name the same file (`a.json` vs `./a.json`, a
/// symlink, …). Also used by `bs compile` to refuse writing over its source.
pub fn same_file(a: &str, b: &str) -> bool {
    normalize_path(a) == normalize_path(b)
}

impl Editor {
    /// Open a single presentation (CLI convenience / back-compat wrapper).
    pub fn open(path: &str) -> Result<Self> {
        Self::open_many(std::slice::from_ref(&path.to_string()))
    }

    /// Open one or more presentations as parallel decks; the first is active.
    /// A path naming a file that is already open is skipped, so two decks can
    /// never save over each other.
    pub fn open_many(paths: &[String]) -> Result<Self> {
        let mut decks: Vec<EditorState> = Vec::with_capacity(paths.len());
        for p in paths {
            if !decks.iter().any(|d| same_file(&d.file_path, p)) {
                decks.push(EditorState::open(p)?);
            }
        }
        if decks.is_empty() {
            decks.push(EditorState::open("untitled.json")?);
        }
        Ok(Editor { decks, active: 0, frame_clip: None, overwrite_ok: None })
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
        // Keyboard enhancement disambiguates key events so modifier+Enter combos
        // (e.g. Shift-Enter to insert a newline in a cell) are reported
        // distinctly. The guard restores the terminal on return *and* on panic.
        let _term = crate::term::TerminalGuard::enter(true)?;
        self.main_loop(&mut stdout)
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
            // The overwrite confirmation holds only while the prompt still
            // shows the path it was given for.
            let confirmed = match &self.active().mode {
                Mode::SaveAs { buf, .. } => self.overwrite_ok.as_deref() == Some(buf.trim()),
                _ => false,
            };
            if !confirmed {
                self.overwrite_ok = None;
            }

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
                Action::SaveAs(path) => {
                    self.save_as(&path);
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
        if let Some(i) = self.decks.iter().position(|d| same_file(&d.file_path, path)) {
            self.active = i;
            self.active_mut().mode = Mode::Normal;
            let warn = self.active().deck_warning();
            self.active_mut().status_message = Some(format!("Switched to {path}{warn}"));
            return;
        }
        match EditorState::open(path) {
            Ok(st) => {
                // Keep open's own notes (deck problem, dropped links, unknown
                // fields) rather than replacing them.
                let notes = st.status_message.as_ref().map(|m| format!(" — {m}")).unwrap_or_default();
                self.decks.push(st);
                self.active = self.decks.len() - 1;
                self.active_mut().status_message = Some(format!("Opened {path}{notes}"));
            }
            Err(e) => {
                self.active_mut().mode = Mode::Normal;
                self.active_mut().status_message = Some(format!("Open failed: {e}"));
            }
        }
    }

    /// Save the active deck to `path` and adopt it. Refused while another open
    /// deck is bound to that file (both would save over each other); an existing
    /// file other than the deck's own needs a second Enter to overwrite. A
    /// refusal leaves the save-as prompt open so the path can be corrected.
    fn save_as(&mut self, path: &str) {
        let active = self.active;
        let elsewhere = self
            .decks
            .iter()
            .enumerate()
            .any(|(i, d)| i != active && same_file(&d.file_path, path));
        let st = &mut self.decks[active];
        if elsewhere {
            st.status_message =
                Some(format!("{path} is open in another deck — switch to it or pick another name"));
            return;
        }
        let own = same_file(&st.file_path, path);
        // symlink_metadata: a dangling symlink counts as an existing file too.
        let exists = std::fs::symlink_metadata(path).is_ok();
        if !own && exists && self.overwrite_ok.as_deref() != Some(path) {
            self.overwrite_ok = Some(path.to_string());
            st.status_message = Some(format!("{path} exists — Enter again to overwrite, Esc to cancel"));
            return;
        }
        self.overwrite_ok = None;
        match st.save_as(path) {
            Ok(()) => st.mode = Mode::Normal,
            Err(e) => st.status_message = Some(format!("Save failed: {e}")),
        }
    }

    /// Paste the cross-deck frame clipboard into the active deck at `target`.
    fn paste_frame_block(&mut self, target: usize, before: bool) {
        let Some(clip) = self.frame_clip.clone() else {
            self.active_mut().mode = Mode::Normal;
            return;
        };
        if let Some(e) = state::frame_growth_error(&self.active().source, clip.frame_count) {
            let st = self.active_mut();
            st.mode = Mode::Normal;
            st.status_message = Some(format!("⚠ {e}"));
            return;
        }
        if let Some(why) = state::paste_frame_block_refusal(&self.active().source, &clip, target, before) {
            self.active_mut().status_message = Some(why);
            return; // stay in FramePastePlace so another slide can be picked
        }
        let mismatch =
            clip.width != self.active().source.width || clip.height != self.active().source.height;
        let (new_current, count) =
            state::paste_frame_block(&mut self.active_mut().source, &clip, target, before);
        let st = self.active_mut();
        st.current_frame = new_current;
        // A split target object's continuation is moved right after it, which
        // shifts later indices: forget the copied-from objects (as a frame copy
        // does), so a linked paste can never link an unrelated object.
        st.clipboard_sources.clear();
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
        let mut save_failed = false;
        // Wait for a key *press*: the release of the q that opened this prompt
        // (reported on Windows) must not count as the confirming q.
        let k = loop {
            let ev = event::read()?;
            if let Some(k) = crate::term::key_press(&ev) {
                break k;
            }
            if let event::Event::Resize(..) = ev {
                self.full_redraw(stdout)?;
            }
        };
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
                    save_failed = true;
                }
            }
        }
        if !save_failed {
            self.active_mut().status_message = None;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_tmp as tmp;

    /// The same path spelled with a `/./` before the file name.
    fn dotted(path: &str) -> String {
        let p = std::path::Path::new(path);
        format!("{}/./{}", p.parent().unwrap().display(), p.file_name().unwrap().to_string_lossy())
    }

    #[test]
    fn same_file_sees_through_spelling_whether_or_not_the_file_exists() {
        let a = tmp("bs_same_file_a.json");
        let dotted = dotted(&a);
        assert!(same_file(&a, &dotted)); // absent: canonical parent + name
        std::fs::write(&a, "{}").unwrap();
        assert!(same_file(&a, &dotted)); // present: canonical path
        assert!(!same_file(&a, &tmp("bs_same_file_b.json")));
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn frame_paste_is_refused_at_the_deck_limit() {
        use crate::engine::source::MAX_FRAME_COUNT;
        let mut ed = Editor::open_many(&[tmp("bs_paste_at_limit_absent.json")]).unwrap();
        ed.frame_clip = Some(state::copy_frame_block(&ed.active().source, 0, 0));
        let st = ed.active_mut();
        st.source.width = 10;
        st.source.height = 10;
        st.source.frame_count = MAX_FRAME_COUNT;
        ed.paste_frame_block(0, false);
        assert_eq!(ed.active().source.frame_count, MAX_FRAME_COUNT);
        assert!(ed.active().status_message.as_deref().unwrap().contains("exceeds the limit"));
        assert!(!ed.active().dirty);
    }

    #[test]
    fn opening_from_the_hub_keeps_the_unknown_field_warning() {
        let a = tmp("bs_hub_open_a.json");
        let b = tmp("bs_hub_open_comment.json");
        std::fs::write(&b, r#"{"width":10,"height":3,"frame_count":1,"_comment":"x","objects":[]}"#).unwrap();
        let mut ed = Editor::open_many(&[a]).unwrap();
        ed.open_or_focus(&b);
        let msg = ed.active().status_message.clone().unwrap_or_default();
        assert!(msg.starts_with(&format!("Opened {b}")), "got: {msg}");
        assert!(msg.contains("_comment will be dropped on save"), "got: {msg}");
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn a_frame_block_paste_forgets_the_copied_from_objects() {
        // Pasting into the middle of an object splits it, shifting later object
        // indices; a copied-from index kept across that would point at another
        // object, so the paste forgets it.
        let mut ed = Editor::open_many(&[tmp("bs_frame_paste_sources.json")]).unwrap();
        ed.decks[0].source = serde_json::from_str(
            r#"{ "width": 10, "height": 2, "frame_count": 4, "objects": [
                { "type": "label", "text": "A", "position": { "x": { "fixed": 0 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 4 } },
                { "type": "label", "text": "B", "position": { "x": { "fixed": 2 }, "y": { "fixed": 0 } },
                  "frames": { "start": 0, "end": 1 } }
            ] }"#,
        )
        .unwrap();
        ed.decks[0].clipboard_sources = vec![1];
        ed.frame_clip = Some(state::copy_frame_block(&ed.decks[0].source, 1, 1));
        ed.paste_frame_block(1, false);
        assert_eq!(ed.decks[0].source.frame_count, 5, "the block was pasted");
        assert!(ed.decks[0].clipboard_sources.is_empty());
    }

    #[test]
    fn open_many_opens_the_same_file_once() {
        let a = tmp("bs_open_many_dedupe.json");
        let dotted = dotted(&a);
        let ed = Editor::open_many(&[a.clone(), dotted, a]).unwrap();
        assert_eq!(ed.decks.len(), 1);
    }

    #[test]
    fn save_as_refuses_another_decks_file_and_confirms_an_overwrite() {
        let (a, b, c) = (tmp("bs_save_as_a.json"), tmp("bs_save_as_b.json"), tmp("bs_save_as_c.json"));
        let mut ed = Editor::open_many(&[a.clone(), b.clone()]).unwrap();
        ed.active = 1;

        ed.save_as(&a); // bound to deck 0: refused, nothing written
        assert_eq!(ed.decks[1].file_path, b);
        assert!(!std::path::Path::new(&a).exists());

        std::fs::write(&c, "{}").unwrap();
        ed.save_as(&c); // existing file: first Enter only warns
        assert_eq!(std::fs::read_to_string(&c).unwrap(), "{}");
        ed.save_as(&c); // second Enter overwrites and adopts
        assert_eq!(ed.decks[1].file_path, c);
        assert_ne!(std::fs::read_to_string(&c).unwrap(), "{}");
        let _ = std::fs::remove_file(&c);
    }
}
