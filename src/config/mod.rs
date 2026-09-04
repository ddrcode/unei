//! All configuration is compiled in. Changing anything here requires a rebuild —
//! that is the point (see docs/rules.md).

pub mod keymap;
pub mod languages;
pub mod palette;
pub mod theme;

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
    /// Cells a window border moves per `Ctrl+w Alt+j/l` press (tmux uses 5).
    pub resize_step_cols: u16,
    /// Rows a window border moves per `Ctrl+w Alt+i/k` press.
    pub resize_step_rows: u16,
}

/// Width of the line-number gutter for a buffer of `lines_total` lines
/// (shared by the renderer and the window-geometry code).
pub fn gutter_width(lines_total: usize) -> u16 {
    if OPTIONS.number {
        (lines_total.to_string().len().max(3) + 1) as u16
    } else {
        0
    }
}

pub const OPTIONS: Options = Options {
    tabstop: 4,
    shiftwidth: 4,
    expandtab: true,
    scrolloff: 15,
    number: true,
    cursorline: true,
    final_newline: true,
    resize_step_cols: 5,
    resize_step_rows: 2,
};
