//! Shared text-editing buffer used by every in-editor text field — object
//! property values, the multi-line text overlay, and table cell-style values.
//!
//! It owns a string plus a char-index cursor and turns key events into edits, so
//! the individual mode handlers no longer each re-implement cursor motion,
//! deletion, newline insertion, and the byte/char-index bookkeeping. Rendering is
//! deliberately *not* part of this type: callers lay the buffer out however their
//! widget needs (narrow panel field vs. wide overlay) using `line_col`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone)]
pub struct TextEdit {
    pub buf: String,
    /// Cursor as a char index in `0..=len`.
    pub cursor: usize,
}

/// What a key press meant to the text editor.
pub enum TextAction {
    /// Buffer and/or cursor changed; re-render.
    Edited,
    /// Commit requested (Enter, without the newline modifier).
    Commit,
    /// Cancel requested (Esc).
    Cancel,
    /// Not a key the text editor consumes.
    Ignored,
}

impl TextEdit {
    /// Start with the cursor at `cursor` (clamped into range).
    pub fn new(buf: String, cursor: usize) -> Self {
        let len = buf.chars().count();
        Self { buf, cursor: cursor.min(len) }
    }

    pub fn len(&self) -> usize {
        self.buf.chars().count()
    }

    fn byte_idx(&self, char_idx: usize) -> usize {
        self.buf
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.buf.len())
    }

    /// `(line, col)` of the cursor, counting `\n`-delimited logical lines.
    pub fn line_col(&self) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        for (i, ch) in self.buf.chars().enumerate() {
            if i == self.cursor {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    pub fn line_count(&self) -> usize {
        self.buf.chars().filter(|&c| c == '\n').count() + 1
    }

    fn line_col_to_cursor(&self, target_line: usize, target_col: usize) -> usize {
        let mut line = 0;
        let mut col = 0;
        for (i, ch) in self.buf.chars().enumerate() {
            if line == target_line && col == target_col {
                return i;
            }
            if ch == '\n' {
                if line == target_line {
                    return i; // clamp: target_col past end of this line
                }
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        self.len()
    }

    fn insert_char(&mut self, c: char) {
        let bi = self.byte_idx(self.cursor);
        self.buf.insert(bi, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let start = self.byte_idx(self.cursor - 1);
            let end = self.byte_idx(self.cursor);
            self.buf.drain(start..end);
            self.cursor -= 1;
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.len() {
            let start = self.byte_idx(self.cursor);
            let end = self.byte_idx(self.cursor + 1);
            self.buf.drain(start..end);
        }
    }

    fn move_up(&mut self) {
        let (line, col) = self.line_col();
        if line > 0 {
            self.cursor = self.line_col_to_cursor(line - 1, col);
        }
    }

    fn move_down(&mut self) {
        let (line, col) = self.line_col();
        if line + 1 < self.line_count() {
            self.cursor = self.line_col_to_cursor(line + 1, col);
        }
    }

    fn move_home(&mut self) {
        let (line, _) = self.line_col();
        self.cursor = self.line_col_to_cursor(line, 0);
    }

    fn move_end(&mut self) {
        let (line, _) = self.line_col();
        let line_len = self.buf.split('\n').nth(line).map(|s| s.chars().count()).unwrap_or(0);
        self.cursor = self.line_col_to_cursor(line, line_len);
    }

    /// Feed a key. `newline` is true when this key is the configured
    /// insert-newline binding (so plain Enter can still mean "commit").
    /// Shift+Enter is always treated as a newline too, where the terminal
    /// reports it, so the binding works the same across every text field.
    pub fn handle_key(&mut self, key: &KeyEvent, newline: bool) -> TextAction {
        let shift_enter =
            key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT);
        if newline || shift_enter {
            self.insert_char('\n');
            return TextAction::Edited;
        }
        match key.code {
            KeyCode::Enter => TextAction::Commit,
            KeyCode::Esc => TextAction::Cancel,
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
                self.cursor = self.cursor.saturating_sub(1);
                TextAction::Edited
            }
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                self.cursor = (self.cursor + 1).min(self.len());
                TextAction::Edited
            }
            KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
                self.move_up();
                TextAction::Edited
            }
            KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                self.move_down();
                TextAction::Edited
            }
            KeyCode::Home if key.modifiers == KeyModifiers::NONE => {
                self.move_home();
                TextAction::Edited
            }
            KeyCode::End if key.modifiers == KeyModifiers::NONE => {
                self.move_end();
                TextAction::Edited
            }
            KeyCode::Backspace => {
                self.backspace();
                TextAction::Edited
            }
            KeyCode::Delete if key.modifiers == KeyModifiers::NONE => {
                self.delete();
                TextAction::Edited
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char(c);
                TextAction::Edited
            }
            _ => TextAction::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_motion() {
        let mut t = TextEdit::new("ab".into(), 2);
        t.insert_char('c');
        assert_eq!(t.buf, "abc");
        assert_eq!(t.cursor, 3);
        t.cursor = t.cursor.saturating_sub(1);
        t.backspace();
        assert_eq!(t.buf, "ac");
        assert_eq!(t.cursor, 1);
    }

    #[test]
    fn multiline_line_col_and_vertical_motion() {
        // "ab\ncde", cursor after 'e' (index 6)
        let mut t = TextEdit::new("ab\ncde".into(), 6);
        assert_eq!(t.line_col(), (1, 3));
        assert_eq!(t.line_count(), 2);
        t.move_up(); // col clamps to line 0 length (2)
        assert_eq!(t.line_col(), (0, 2));
    }

    #[test]
    fn newline_inserts_rather_than_commits() {
        let mut t = TextEdit::new("x".into(), 1);
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(t.handle_key(&key, true), TextAction::Edited));
        assert_eq!(t.buf, "x\n");
        assert!(matches!(t.handle_key(&key, false), TextAction::Commit));
    }
}
