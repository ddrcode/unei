//! Terminal lifecycle: raw mode, alternate screen, Kitty keyboard protocol,
//! a panic-safe restore path — and the Kitty-flavored ratatui backend that
//! renders real undercurl for warning diagnostics.
//!
//! ratatui's `Modifier` cannot express undercurl, so two otherwise-unused
//! bits are repurposed (see `ui`): RAPID_BLINK = straight red underline
//! (errors), SLOW_BLINK = curly yellow underline (warnings).

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::cursor::{Hide, MoveTo, Show};
use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::style::{
    Attribute, Color as CtColor, Colors, Print, ResetColor, SetAttribute, SetColors,
};
use ratatui::crossterm::terminal::{Clear as CtClear, ClearType as CtClearType};
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::crossterm::{execute, queue};
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};

use crate::config::keymap::Key;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static ENHANCED: AtomicBool = AtomicBool::new(false);

pub fn init() -> Result<Terminal<KittyBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    // Kitty is the one supported terminal; the flag makes a lone Esc
    // unambiguous, so it arrives with zero disambiguation delay. The support
    // query can fail through wrappers, so a TERM naming kitty is trusted
    // outright (inside tmux TERM is tmux-*/screen-* and the query decides).
    let kitty_term = std::env::var("TERM").is_ok_and(|t| t.contains("kitty"));
    if kitty_term || matches!(supports_keyboard_enhancement(), Ok(true)) {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        ENHANCED.store(true, Ordering::SeqCst);
    }
    ACTIVE.store(true, Ordering::SeqCst);
    Ok(Terminal::new(KittyBackend::new(io::stdout()))?)
}

/// The custom backend: crossterm rendering plus Kitty underline extensions.
pub struct KittyBackend<W: Write> {
    writer: W,
}

impl<W: Write> KittyBackend<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

fn ct_color(c: Color) -> CtColor {
    match c {
        Color::Reset => CtColor::Reset,
        Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
        Color::Black => CtColor::Black,
        Color::White => CtColor::White,
        _ => CtColor::Reset, // the palette is all-RGB; nothing else is used
    }
}

/// Error underline (straight, red) — material error color.
const UNDERLINE_ERROR: &str = "\x1b[4:1m\x1b[58:2::240:113:120m";
/// Warning underline (curly, yellow) — the Kitty flex from the rules.
const UNDERLINE_WARN: &str = "\x1b[4:3m\x1b[58:2::255:203:107m";
const UNDERLINE_OFF: &str = "\x1b[4:0m\x1b[59m";

impl<W: Write> Backend for KittyBackend<W> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        use ratatui::crossterm::queue;
        let mut last: Option<(u16, u16)> = None;
        let mut style = (Color::Reset, Color::Reset, Modifier::empty());
        for (x, y, cell) in content {
            if last != Some((x.wrapping_sub(1), y)) {
                queue!(self.writer, MoveTo(x, y))?;
            }
            last = Some((x, y));
            let want = (cell.fg, cell.bg, cell.modifier);
            if want != style {
                queue!(self.writer, SetAttribute(Attribute::Reset), ResetColor)?;
                queue!(
                    self.writer,
                    SetColors(Colors::new(ct_color(cell.fg), ct_color(cell.bg)))
                )?;
                let m = cell.modifier;
                if m.contains(Modifier::BOLD) {
                    queue!(self.writer, SetAttribute(Attribute::Bold))?;
                }
                if m.contains(Modifier::DIM) {
                    queue!(self.writer, SetAttribute(Attribute::Dim))?;
                }
                if m.contains(Modifier::ITALIC) {
                    queue!(self.writer, SetAttribute(Attribute::Italic))?;
                }
                if m.contains(Modifier::UNDERLINED) {
                    queue!(self.writer, SetAttribute(Attribute::Underlined))?;
                }
                if m.contains(Modifier::REVERSED) {
                    queue!(self.writer, SetAttribute(Attribute::Reverse))?;
                }
                if m.contains(Modifier::CROSSED_OUT) {
                    queue!(self.writer, SetAttribute(Attribute::CrossedOut))?;
                }
                // smuggled diagnostics underlines (Kitty SGR extensions)
                if m.contains(Modifier::RAPID_BLINK) {
                    queue!(self.writer, Print(UNDERLINE_ERROR))?;
                } else if m.contains(Modifier::SLOW_BLINK) {
                    queue!(self.writer, Print(UNDERLINE_WARN))?;
                } else if style
                    .2
                    .intersects(Modifier::RAPID_BLINK | Modifier::SLOW_BLINK)
                {
                    queue!(self.writer, Print(UNDERLINE_OFF))?;
                }
                style = want;
            }
            queue!(self.writer, Print(cell.symbol()))?;
        }
        queue!(self.writer, SetAttribute(Attribute::Reset), ResetColor)?;
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        ratatui::crossterm::execute!(self.writer, Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        ratatui::crossterm::execute!(self.writer, Show)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        let (x, y) = ratatui::crossterm::cursor::position()?;
        Ok(Position::new(x, y))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let p = position.into();
        ratatui::crossterm::execute!(self.writer, MoveTo(p.x, p.y))
    }

    fn clear(&mut self) -> io::Result<()> {
        ratatui::crossterm::execute!(self.writer, CtClear(CtClearType::All))
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        let ct = match clear_type {
            ClearType::All => CtClearType::All,
            ClearType::AfterCursor => CtClearType::FromCursorDown,
            ClearType::BeforeCursor => CtClearType::FromCursorUp,
            ClearType::CurrentLine => CtClearType::CurrentLine,
            ClearType::UntilNewLine => CtClearType::UntilNewLine,
        };
        ratatui::crossterm::execute!(self.writer, CtClear(ct))
    }

    fn size(&self) -> io::Result<Size> {
        let (w, h) = ratatui::crossterm::terminal::size()?;
        Ok(Size::new(w, h))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        let (w, h) = ratatui::crossterm::terminal::size()?;
        Ok(WindowSize {
            columns_rows: Size::new(w, h),
            pixels: Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
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
        DisableBracketedPaste,
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
    let alt = ev.modifiers.contains(KeyModifiers::ALT);
    Some(match ev.code {
        KeyCode::Char(c) if ctrl => match c {
            '[' => Key::Esc,
            _ => Key::Ctrl(c.to_ascii_lowercase()),
        },
        KeyCode::Char(c) if alt => Key::Alt(c.to_ascii_lowercase()),
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Esc => Key::Esc,
        KeyCode::Enter if ctrl => Key::CtrlEnter,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Delete => Key::Delete,
        KeyCode::Up if alt => Key::AltUp,
        KeyCode::Down if alt => Key::AltDown,
        KeyCode::Left if alt => Key::AltLeft,
        KeyCode::Right if alt => Key::AltRight,
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
