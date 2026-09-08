//! Semantic commands the keymap resolves keys into.

use crate::core::motion::Motion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Delete,
    Change,
    Yank,
    /// `>` — shift lines right by one shiftwidth (always linewise).
    Indent,
    /// `<` — shift lines left by one shiftwidth.
    Dedent,
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
    /// `Ctrl+o` — jumplist back.
    JumpBack,
    /// `Ctrl+i` / `Tab` — jumplist forward.
    JumpForward,
    /// `n` / `N` — next / previous search match.
    NextMatch,
    PrevMatch,
    /// `*` — search the word under the cursor.
    SearchWord,
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
    /// `Ctrl+A` — increment the number under (or after) the cursor.
    Increment,
    /// `Ctrl+X` — decrement it.
    Decrement,
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
    /// `R` — enter Replace (overtype) mode.
    ReplaceMode,
    /// `m` — set a mark; awaits the mark letter.
    SetMark,
    /// `` ` `` (exact) / `'` (line) — jump to a mark; awaits the letter.
    JumpMark {
        exact: bool,
    },
    PrefixG,
    PrefixZ,
    PrefixZUpper,
    Leader,
    /// `Ctrl+w` — window chord prefix.
    PrefixWindow,
    /// `v` / `V` / `Ctrl+v` — enter visual mode.
    Visual(VisualKind),
    /// `Ctrl+^` — switch to the alternate (previously shown) buffer.
    AlternateBuffer,
    /// `Ctrl+p` — file picker.
    FilePicker,
    /// `K` — hover: type/docs of the item under the cursor (rust-analyzer).
    Hover,
    CmdLine,
    /// `/` and `?` — incremental search prompts.
    SearchPrompt(bool /* forward */),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualKind {
    Char,
    Line,
    Block,
}

/// Second key of a `<leader>…` chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderCmd {
    /// `<leader>b` — buffer list.
    BufferList,
    /// `<leader>p` / `<leader>P` — paste the CUT register after / before
    /// (the file picker moved to Ctrl+P alone, per #10).
    PasteCutAfter,
    PasteCutBefore,
    /// `<leader>w` — same as `Ctrl+w`.
    WindowPrefix,
    /// `<leader>c…` — code chord (`ca` = code actions).
    CodePrefix,
    /// `<leader>r…` — rust chord (`rm` = expand macro).
    RustPrefix,
    /// `<leader>d…` — diagnostics chord (`dh` = toggle ghost text).
    DiagPrefix,
    /// `<leader>s` — symbol picker for the current buffer (#62).
    Symbols,
    /// `<leader>g` — live grep across the project (#72).
    Grep,
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

/// How a window splits (the layout tree in editor/windows.rs re-exports it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    /// vim `:split` — children stacked top to bottom.
    Horizontal,
    /// vim `:vsplit` — children side by side, 1-column separator between.
    Vertical,
}

/// Second key of a window chord (`Ctrl+w …` / `<leader>w …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinCmd {
    Focus(WinDir),
    /// `Ctrl+w Ctrl+w` — cycle to the next window.
    FocusNext,
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
    /// `S`/`X`/`V` — split, but stay: focus remains where it is.
    SplitStay(SplitDir),
    /// `Alt+s`/`Alt+x`/`Alt+v` — split, project the new window (`gp`),
    /// and stay: the side-by-side editing setup in one chord.
    SplitPreview(SplitDir),
    /// `q` — close the focused window (last window quits the editor).
    CloseWindow,
    /// `o` — close every other window.
    OnlyWindow,
}

/// What the unnamed register holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Register {
    Char(String),
    Line(String),
    /// One segment per selected line (blockwise yank/delete).
    Block(Vec<String>),
}
