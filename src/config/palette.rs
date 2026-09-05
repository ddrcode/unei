//! Minimal chrome palette until ticket #5 (full material-oceanic colorscheme).
//! Values follow material.nvim "oceanic" plus the author's LineNr/SignColumn override.

use ratatui::style::Color;

pub const BG: Color = Color::Rgb(0x26, 0x32, 0x38);
pub const FG: Color = Color::Rgb(0xEE, 0xFF, 0xFF);
pub const COMMENT: Color = Color::Rgb(0x54, 0x6E, 0x7A);

/// Visual-selection background (material-ish, distinct from cursorline).
pub const SELECTION_BG: Color = Color::Rgb(0x3C, 0x50, 0x5E);
/// Yank-flash background (warm, IncSearch-like).
pub const YANK_FLASH_BG: Color = Color::Rgb(0x55, 0x4A, 0x28);

/// Custom gutter background (the user's `LineNr { bg = "#233939" }`).
pub const GUTTER_BG: Color = Color::Rgb(0x23, 0x39, 0x39);
pub const GUTTER_FG: Color = Color::Rgb(0x54, 0x6E, 0x7A);
pub const GUTTER_CURRENT_FG: Color = Color::Rgb(0xEE, 0xFF, 0xFF);

pub const CURSORLINE_BG: Color = Color::Rgb(0x2A, 0x37, 0x3E);
/// Floating windows use the darker contrast bg (material `floating_windows`).
pub const FLOAT_BG: Color = Color::Rgb(0x1E, 0x28, 0x2D);
pub const STATUSLINE_BG: Color = Color::Rgb(0x1B, 0x26, 0x2B);
pub const STATUSLINE_FG: Color = Color::Rgb(0xB0, 0xBE, 0xC5);

pub const MODE_NORMAL: Color = Color::Rgb(0x82, 0xAA, 0xFF);
pub const MODE_INSERT: Color = Color::Rgb(0xC3, 0xE8, 0x8D);
pub const MODE_PENDING: Color = Color::Rgb(0xFF, 0xCB, 0x6B);
pub const MODE_COMMAND: Color = Color::Rgb(0xC7, 0x92, 0xEA);
pub const MODE_LABEL_FG: Color = Color::Rgb(0x1B, 0x26, 0x2B);

pub const ERROR_FG: Color = Color::Rgb(0xF0, 0x71, 0x78);

/// Share of the original color kept in inactive panels (the rest blends
/// into the background) — the Shade-nvim look, softened per the author's
/// taste (50% read as slightly too dim).
const INACTIVE_KEEP: u16 = 60; // percent

/// Dims a foreground for an inactive panel by blending it toward `BG`.
pub fn dimmed(color: Color) -> Color {
    let Color::Rgb(r, g, b) = color else {
        return color;
    };
    let Color::Rgb(br, bg_, bb) = BG else {
        return color;
    };
    let mix = |c: u8, back: u8| -> u8 {
        ((c as u16 * INACTIVE_KEEP + back as u16 * (100 - INACTIVE_KEEP)) / 100) as u8
    };
    Color::Rgb(mix(r, br), mix(g, bg_), mix(b, bb))
}

// material oceanic accents (see config/theme.rs for the capture mapping)
pub const RED: Color = Color::Rgb(0xF0, 0x71, 0x78);
pub const GREEN: Color = Color::Rgb(0xC3, 0xE8, 0x8D);
pub const YELLOW: Color = Color::Rgb(0xFF, 0xCB, 0x6B);
pub const BLUE: Color = Color::Rgb(0x82, 0xAA, 0xFF);
pub const CYAN: Color = Color::Rgb(0x89, 0xDD, 0xFF);
pub const PURPLE: Color = Color::Rgb(0xC7, 0x92, 0xEA);
pub const ORANGE: Color = Color::Rgb(0xF7, 0x8C, 0x6C);
pub const PALE: Color = Color::Rgb(0xB0, 0xC9, 0xFF);
