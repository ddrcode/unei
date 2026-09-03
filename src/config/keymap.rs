//! Key → command tables. This *is* the keymap configuration: edit and rebuild.
//!
//! The layout mirrors `~/.dotfiles/config/nvim/lua/keyboard.lua`: IJKL for
//! navigation (i=up, k=down, j=left, l=right) applied in normal and
//! operator-pending contexts, `h`/`H` enter insert mode in normal mode only —
//! so `dh` still deletes left, exactly like the author's nvim.

use crate::core::commands::{FindKind, InsertEntry, Op, ScrollCmd, SimpleCmd, Token};
use crate::core::motion::Motion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Meaning of a key in normal mode (digits are handled by the pending-state
/// machine before this table is consulted).
pub fn normal_token(key: Key) -> Option<Token> {
    Some(match key {
        // IJKL navigation
        Key::Char('i') => Token::Motion(Motion::Up),
        Key::Char('k') => Token::Motion(Motion::Down),
        Key::Char('j') => Token::Motion(Motion::Left),
        Key::Char('l') => Token::Motion(Motion::Right),

        // insert-mode entries (h replaces vim's i)
        Key::Char('h') => Token::Insert(InsertEntry::Before),
        Key::Char('H') => Token::Insert(InsertEntry::FirstNonBlank),
        Key::Char('a') => Token::Insert(InsertEntry::After),
        Key::Char('A') => Token::Insert(InsertEntry::LineEnd),
        Key::Char('o') => Token::Insert(InsertEntry::OpenBelow),
        Key::Char('O') => Token::Insert(InsertEntry::OpenAbove),

        // line / word motions
        Key::Char('0') | Key::Home => Token::Motion(Motion::LineStart),
        Key::Char('^') => Token::Motion(Motion::FirstNonBlank),
        Key::Char('$') | Key::End => Token::Motion(Motion::LineEnd),
        Key::Char('w') => Token::Motion(Motion::WordForward { big: false }),
        Key::Char('W') => Token::Motion(Motion::WordForward { big: true }),
        Key::Char('b') => Token::Motion(Motion::WordBack { big: false }),
        Key::Char('B') => Token::Motion(Motion::WordBack { big: true }),
        Key::Char('e') => Token::Motion(Motion::WordEnd { big: false }),
        Key::Char('E') => Token::Motion(Motion::WordEnd { big: true }),
        Key::Char('G') => Token::Motion(Motion::GotoLast),
        Key::Char('{') => Token::Motion(Motion::ParaBack),
        Key::Char('}') => Token::Motion(Motion::ParaForward),

        // find on line
        Key::Char('f') => Token::FindStart(FindKind::To),
        Key::Char('F') => Token::FindStart(FindKind::ToBack),
        Key::Char('t') => Token::FindStart(FindKind::Till),
        Key::Char('T') => Token::FindStart(FindKind::TillBack),
        Key::Char(';') => Token::Motion(Motion::RepeatFind),
        Key::Char(',') => Token::Motion(Motion::RepeatFindRev),

        // arrows work everywhere
        Key::Up => Token::Motion(Motion::Up),
        Key::Down => Token::Motion(Motion::Down),
        Key::Left => Token::Motion(Motion::Left),
        Key::Right => Token::Motion(Motion::Right),

        // operators
        Key::Char('d') => Token::Op(Op::Delete),
        Key::Char('c') => Token::Op(Op::Change),
        Key::Char('y') => Token::Op(Op::Yank),

        // simple commands
        Key::Char('x') => Token::Simple(SimpleCmd::DeleteRight),
        Key::Char('X') => Token::Simple(SimpleCmd::DeleteLeft),
        Key::Char('r') => Token::ReplaceStart,
        Key::Char('~') => Token::Simple(SimpleCmd::ToggleCase),
        Key::Char('J') => Token::Simple(SimpleCmd::Join),
        Key::Char('p') => Token::Simple(SimpleCmd::PasteAfter),
        Key::Char('P') => Token::Simple(SimpleCmd::PasteBefore),
        Key::Char('u') => Token::Simple(SimpleCmd::Undo),
        Key::Ctrl('r') => Token::Simple(SimpleCmd::Redo),
        Key::Char('.') => Token::Simple(SimpleCmd::Repeat),
        Key::Char('D') => Token::Simple(SimpleCmd::DeleteToEol),
        Key::Char('C') => Token::Simple(SimpleCmd::ChangeToEol),
        Key::Char('Y') => Token::Simple(SimpleCmd::YankToEol),
        Key::Char('s') => Token::Simple(SimpleCmd::SubstChar),
        Key::Char('S') => Token::Simple(SimpleCmd::SubstLine),

        // scrolling
        Key::Ctrl('d') => Token::Scroll(ScrollCmd::HalfDown),
        Key::Ctrl('u') => Token::Scroll(ScrollCmd::HalfUp),
        Key::Ctrl('f') | Key::PageDown => Token::Scroll(ScrollCmd::PageDown),
        Key::Ctrl('b') | Key::PageUp => Token::Scroll(ScrollCmd::PageUp),

        // prefixes
        Key::Char('g') => Token::PrefixG,
        Key::Char('z') => Token::PrefixZ,
        Key::Char('Z') => Token::PrefixZUpper,
        Key::Char(':') => Token::CmdLine,

        _ => return None,
    })
}

/// Extra motion meanings that only exist after an operator. In the author's
/// nvim `h` is remapped in normal mode only, so operator-pending `h` keeps
/// vim's left-motion.
pub fn operator_extra_motion(key: Key) -> Option<Motion> {
    match key {
        Key::Char('h') => Some(Motion::Left),
        _ => None,
    }
}
