//! Terminal session guard shared by the editor and the player.
//!
//! [`TerminalGuard::enter`] puts the terminal into raw mode on the alternate
//! screen, with autowrap off (an over-long line is cut, never scrolls); the
//! terminal is restored when the guard drops *and* from a panic hook, which runs
//! before the default hook prints — so a panic leaves a usable shell with a
//! readable message instead of a raw, cursorless alternate screen. Bracketed
//! paste is on while the guard holds, so a paste is one `Event::Paste`.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{cursor, execute, terminal};

/// Whether a guard currently holds the terminal (so restore runs exactly once).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether keyboard-enhancement flags were pushed and must be popped.
static ENHANCED: AtomicBool = AtomicBool::new(false);
static HOOK: Once = Once::new();

/// Restores the terminal on drop. Hold it for the whole interactive session.
pub struct TerminalGuard(());

impl TerminalGuard {
    /// Enter raw mode + the alternate screen with a hidden cursor and a cleared
    /// screen. With `enhance`, also ask the terminal to disambiguate key events
    /// when it supports that (silently skipped otherwise).
    pub fn enter(enhance: bool) -> io::Result<Self> {
        HOOK.call_once(|| {
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore();
                prev(info);
            }));
        });
        terminal::enable_raw_mode()?;
        ACTIVE.store(true, Ordering::SeqCst);
        // From here on, an early return drops the guard and restores.
        let guard = TerminalGuard(());
        let mut stdout = io::stdout();
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            // Defence in depth: an over-long line is cut at the edge instead of
            // wrapping (and scrolling the screen when on the last row).
            terminal::DisableLineWrap,
            terminal::Clear(terminal::ClearType::All),
        )?;
        // A paste arrives as one `Event::Paste`, not as typed keys whose line
        // breaks would read as Enter (commit) and the rest as commands. Best
        // effort: a terminal without it still sends keys.
        let _ = execute!(stdout, EnableBracketedPaste);
        if enhance && terminal::supports_keyboard_enhancement().unwrap_or(false) {
            let pushed = execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
            ENHANCED.store(pushed.is_ok(), Ordering::SeqCst);
        }
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

/// The key of a key-*press* event; `None` for releases, repeats and non-key
/// events. Windows always reports releases (Unix only with keyboard
/// enhancement), so the editor and the player act on presses only.
pub fn key_press(ev: &crossterm::event::Event) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{Event, KeyEventKind};
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Press => Some(*k),
        _ => None,
    }
}

/// Undo [`TerminalGuard::enter`] once; a no-op when no guard is active.
fn restore() {
    if !ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let mut stdout = io::stdout();
    if ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout, DisableBracketedPaste);
    let _ = execute!(stdout, terminal::EnableLineWrap, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

#[cfg(test)]
mod tests {
    use super::key_press;

    #[test]
    fn key_press_ignores_releases_and_non_key_events() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let q = |kind| Event::Key(KeyEvent::new_with_kind(KeyCode::Char('q'), KeyModifiers::NONE, kind));
        assert!(key_press(&q(KeyEventKind::Release)).is_none());
        assert!(key_press(&q(KeyEventKind::Repeat)).is_none());
        assert!(key_press(&Event::Resize(80, 24)).is_none());
        assert_eq!(key_press(&q(KeyEventKind::Press)).map(|k| k.code), Some(KeyCode::Char('q')));
    }
}
