//! Semantic commands the keymap resolves keys into.

use crate::core::motion::Motion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Delete,
    Change,
    Yank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertEntry {
    /// `h` — insert before the cursor (vim `i`).
    Before,
    /// `H` — insert before the first non-blank (vim `I`).
    FirstNonBlank,
    /// `a`
    After,
    /// `A`
    LineEnd,
    /// `o`
    OpenBelow,
    /// `O`
    OpenAbove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindKind {
    /// `f`
    To,
    /// `F`
    ToBack,
    /// `t`
    Till,
    /// `T`
    TillBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollCmd {
    HalfDown,
    HalfUp,
    PageDown,
    PageUp,
    CenterCursor,
    CursorTop,
    CursorBottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleCmd {
    /// `x`
    DeleteRight,
    /// `X`
    DeleteLeft,
    /// `~`
    ToggleCase,
    /// `J`
    Join,
    /// `p`
    PasteAfter,
    /// `P`
    PasteBefore,
    /// `u`
    Undo,
    /// `Ctrl+r`
    Redo,
    /// `.`
    Repeat,
    /// `D`
    DeleteToEol,
    /// `C`
    ChangeToEol,
    /// `Y` — nvim-style `y$`
    YankToEol,
    /// `s`
    SubstChar,
    /// `S`
    SubstLine,
}

/// What a key means in normal mode, before pending state is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Motion(Motion),
    Op(Op),
    Insert(InsertEntry),
    Simple(SimpleCmd),
    Scroll(ScrollCmd),
    FindStart(FindKind),
    ReplaceStart,
    PrefixG,
    PrefixZ,
    PrefixZUpper,
    CmdLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}
