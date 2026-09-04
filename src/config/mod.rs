//! All configuration is compiled in. Changing anything here requires a rebuild —
//! that is the point (see docs/rules.md).

pub mod keymap;
pub mod palette;

/// Editor options, mirroring the author's nvim `settings.lua`.
pub struct Options {
    /// Display width of a tab character.
    pub tabstop: usize,
    /// Indent width used by Tab in insert mode and auto-indent.
    pub shiftwidth: usize,
    /// Insert spaces instead of tab characters.
    pub expandtab: bool,
    /// Minimal number of screen lines to keep above and below the cursor.
    pub scrolloff: usize,
    /// Show absolute line numbers (relative numbers are banned by the rules).
    pub number: bool,
    /// Highlight the cursor line.
    pub cursorline: bool,
    /// Ensure the file ends with a newline on save (editorconfig: insert_final_newline).
    pub final_newline: bool,
}

pub const OPTIONS: Options = Options {
    tabstop: 4,
    shiftwidth: 4,
    expandtab: true,
    scrolloff: 15,
    number: true,
    cursorline: true,
    final_newline: true,
};
