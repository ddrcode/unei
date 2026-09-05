//! Key → command tables. This *is* the keymap configuration: edit and rebuild.
//!
//! The layout mirrors `~/.dotfiles/config/nvim/lua/keyboard.lua`: IJKL for
//! navigation (i=up, k=down, j=left, l=right) applied in normal and
//! operator-pending contexts, `h`/`H` enter insert mode in normal mode only —
//! so `dh` still deletes left, exactly like the author's nvim.

use crate::core::commands::{
    FindKind, InsertEntry, LeaderCmd, ListCmd, Op, PickerCmd, ScrollCmd, SimpleCmd, Token,
    VisualKind, WinCmd, WinDir,
};
use crate::core::motion::Motion;

/// The leader key (the author's nvim uses space).
pub const LEADER: Key = Key::Char(' ');

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Alt(char),
    CtrlEnter,
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

        // visual mode
        Key::Char('v') => Token::Visual(VisualKind::Char),
        Key::Char('V') => Token::Visual(VisualKind::Line),
        Key::Ctrl('v') => Token::Visual(VisualKind::Block),

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

        // buffers
        Key::Ctrl('^') | Key::Ctrl('6') => Token::AlternateBuffer,

        // rust-analyzer
        Key::Char('K') => Token::Hover,

        // jumplist
        Key::Ctrl('o') => Token::Simple(SimpleCmd::JumpBack),
        Key::Ctrl('i') | Key::Tab => Token::Simple(SimpleCmd::JumpForward),

        // search
        Key::Char('/') => Token::SearchPrompt(true),
        Key::Char('?') => Token::SearchPrompt(false),
        Key::Char('n') => Token::Simple(SimpleCmd::NextMatch),
        Key::Char('N') => Token::Simple(SimpleCmd::PrevMatch),
        Key::Char('*') => Token::Simple(SimpleCmd::SearchWord),

        // file picker
        Key::Ctrl('p') => Token::FilePicker,

        // windows
        Key::Ctrl('w') => Token::PrefixWindow,

        // prefixes
        k if k == LEADER => Token::Leader,
        Key::Char('g') => Token::PrefixG,
        Key::Char('z') => Token::PrefixZ,
        Key::Char('Z') => Token::PrefixZUpper,
        Key::Char(':') => Token::CmdLine,

        _ => return None,
    })
}

/// Second key of a `<leader>…` chord (mirrors the author's nvim mappings).
pub fn leader_token(key: Key) -> Option<LeaderCmd> {
    match key {
        Key::Char('b') => Some(LeaderCmd::BufferList),
        Key::Char('p') => Some(LeaderCmd::PasteCutAfter),
        Key::Char('P') => Some(LeaderCmd::PasteCutBefore),
        Key::Char('w') => Some(LeaderCmd::WindowPrefix),
        Key::Char('c') => Some(LeaderCmd::CodePrefix),
        Key::Char('r') => Some(LeaderCmd::RustPrefix),
        Key::Char('d') => Some(LeaderCmd::DiagPrefix),
        _ => None,
    }
}

/// Second key of a window chord (`Ctrl+w …` / `<leader>w …`).
///
/// Ticket #4 wanted `l` for "swap layout", but the rules make IJKL navigation
/// universal (`Ctrl+w+i` = panel above is the rules' own example), so `l`
/// focuses right and layout-flip sits on Space (tmux's next-layout key).
pub fn window_token(key: Key) -> Option<WinCmd> {
    Some(match key {
        // focus: IJKL and arrows
        Key::Ctrl('w') => WinCmd::FocusNext,
        Key::Char('i') | Key::Up => WinCmd::Focus(WinDir::Up),
        Key::Char('k') | Key::Down => WinCmd::Focus(WinDir::Down),
        Key::Char('j') | Key::Left => WinCmd::Focus(WinDir::Left),
        Key::Char('l') | Key::Right => WinCmd::Focus(WinDir::Right),

        // resize: left-Alt + navigation, tmux style
        Key::Alt('i') => WinCmd::Resize(WinDir::Up),
        Key::Alt('k') => WinCmd::Resize(WinDir::Down),
        Key::Alt('j') => WinCmd::Resize(WinDir::Left),
        Key::Alt('l') => WinCmd::Resize(WinDir::Right),

        Key::Char('=') => WinCmd::Equalize,
        Key::Char('r') => WinCmd::Rotate,
        Key::Char(' ') => WinCmd::FlipLayout,
        Key::Char('z') => WinCmd::ZoomToggle,
        Key::Char('s') | Key::Char('x') => WinCmd::SplitH,
        Key::Char('v') => WinCmd::SplitV,
        Key::Char('n') => WinCmd::SplitNew,
        Key::Char('q') => WinCmd::CloseWindow,
        Key::Char('o') => WinCmd::OnlyWindow,
        _ => return None,
    })
}

/// Keys inside the buffer-list overlay: IJKL navigation, Enter picks,
/// `x` closes the selected buffer, Esc or `q` dismisses.
pub fn list_token(key: Key) -> Option<ListCmd> {
    Some(match key {
        Key::Char('i') | Key::Up => ListCmd::Up,
        Key::Char('k') | Key::Down => ListCmd::Down,
        Key::Enter => ListCmd::Select,
        Key::Char('x') => ListCmd::CloseBuffer,
        Key::Esc | Key::Char('q') | Key::Ctrl('c') => ListCmd::Dismiss,
        _ => return None,
    })
}

/// Keys inside the file-picker overlay. Plain chars edit the query, so all
/// commands sit on control keys; navigation is Ctrl+i/Ctrl+k (IJKL, distinct
/// from Tab/Enter under the Kitty keyboard protocol) plus the arrows.
pub fn picker_token(key: Key) -> Option<PickerCmd> {
    Some(match key {
        Key::Up | Key::Ctrl('i') => PickerCmd::Up,
        Key::Down | Key::Ctrl('k') => PickerCmd::Down,
        Key::Enter => PickerCmd::Open,
        Key::Ctrl('v') => PickerCmd::OpenVsplit,
        Key::Ctrl('x') => PickerCmd::OpenHsplit,
        Key::CtrlEnter => PickerCmd::CreatePath,
        Key::Ctrl('u') => PickerCmd::ClearQuery,
        Key::Backspace => PickerCmd::DeleteChar,
        Key::Esc | Key::Ctrl('c') => PickerCmd::Dismiss,
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

/// Visual-mode overrides: `h` is a motion again (the insert remap is
/// normal-mode-only, per keyboard.lua), and `x`/`s` alias delete/change.
pub fn visual_extra(key: Key) -> Option<Token> {
    Some(match key {
        Key::Char('h') => Token::Motion(Motion::Left),
        Key::Char('x') => Token::Op(Op::Delete),
        Key::Char('s') => Token::Op(Op::Change),
        _ => return None,
    })
}
