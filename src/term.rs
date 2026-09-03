//! Terminal lifecycle: raw mode, alternate screen, Kitty keyboard protocol,
//! and a panic-safe restore path.

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::crossterm::{execute, queue};

use crate::config::keymap::Key;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static ENHANCED: AtomicBool = AtomicBool::new(false);

pub fn init() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Kitty is the one supported terminal; the flag removes the Esc-key
    // ambiguity delay. Still guarded so a plain terminal survives.
    if matches!(supports_keyboard_enhancement(), Ok(true)) {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        ENHANCED.store(true, Ordering::SeqCst);
    }
    ACTIVE.store(true, Ordering::SeqCst);
    Ok(Terminal::new(CrosstermBackend::new(io::stdout()))?)
}

/// Idempotent; also called from the panic hook.
pub fn restore() {
    if !ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let mut stdout = io::stdout();
    if ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = queue!(stdout, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        stdout,
        SetCursorStyle::DefaultUserShape,
        LeaveAlternateScreen,
        ratatui::crossterm::cursor::Show,
    );
    let _ = disable_raw_mode();
}

pub fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        original(info);
    }));
}

/// Maps a crossterm key event onto the editor's key vocabulary.
pub fn convert(ev: KeyEvent) -> Option<Key> {
    if ev.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    Some(match ev.code {
        KeyCode::Char(c) if ctrl => match c {
            '[' => Key::Esc,
            _ => Key::Ctrl(c.to_ascii_lowercase()),
        },
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Esc => Key::Esc,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Delete => Key::Delete,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    })
}
