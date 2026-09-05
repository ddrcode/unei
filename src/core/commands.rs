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
    Leader,
    /// `Ctrl+w` — window chord prefix.
    PrefixWindow,
    /// `Ctrl+^` — switch to the alternate (previously shown) buffer.
    AlternateBuffer,
    /// `Ctrl+p` — file picker.
    FilePicker,
    /// `K` — hover: type/docs of the item under the cursor (rust-analyzer).
    Hover,
    CmdLine,
}

/// Second key of a `<leader>…` chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderCmd {
    /// `<leader>b` — buffer list.
    BufferList,
    /// `<leader>p` — file picker.
    FilePicker,
    /// `<leader>w` — same as `Ctrl+w`.
    WindowPrefix,
    /// `<leader>c…` — code chord (`ca` = code actions).
    CodePrefix,
    /// `<leader>r…` — rust chord (`rm` = expand macro).
    RustPrefix,
    /// `<leader>d…` — diagnostics chord (`dh` = toggle ghost text).
    DiagPrefix,
}

/// Keys inside the buffer-list overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListCmd {
    Up,
    Down,
    Select,
    CloseBuffer,
    Dismiss,
}

/// Keys inside the file-picker overlay (typed chars edit the query and are
/// handled before this table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerCmd {
    Up,
    Down,
    /// Enter — open in the focused window.
    Open,
    /// `Ctrl+v` — open in a vertical split.
    OpenVsplit,
    /// `Ctrl+x` — open in a horizontal split.
    OpenHsplit,
    /// `Ctrl+Enter` — create the file the query names (with parent dirs).
    CreatePath,
    /// `Ctrl+u` — clear the query.
    ClearQuery,
    DeleteChar,
    Dismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinDir {
    Up,
    Down,
    Left,
    Right,
}

/// Second key of a window chord (`Ctrl+w …` / `<leader>w …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinCmd {
    Focus(WinDir),
    /// tmux-style: push the window border in that direction.
    Resize(WinDir),
    /// `=`
    Equalize,
    /// `r` — rotate windows within their container.
    Rotate,
    /// `Space` — flip the container between horizontal and vertical.
    FlipLayout,
    /// `z` — full-screen the focused window / restore.
    ZoomToggle,
    /// `s` (and `x`, per ticket #4) — horizontal split.
    SplitH,
    /// `v` — vertical split.
    SplitV,
    /// `n` — horizontal split with a fresh empty buffer.
    SplitNew,
    /// `q` — close the focused window (last window quits the editor).
    CloseWindow,
    /// `o` — close every other window.
    OnlyWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}
