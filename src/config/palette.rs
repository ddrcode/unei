//! Minimal chrome palette until ticket #5 (full material-oceanic colorscheme).
//! Values follow material.nvim "oceanic" plus the author's LineNr/SignColumn override.

use ratatui::style::Color;

pub const BG: Color = Color::Rgb(0x26, 0x32, 0x38);
pub const FG: Color = Color::Rgb(0xEE, 0xFF, 0xFF);
pub const COMMENT: Color = Color::Rgb(0x54, 0x6E, 0x7A);

/// Custom gutter background (the user's `LineNr { bg = "#233939" }`).
pub const GUTTER_BG: Color = Color::Rgb(0x23, 0x39, 0x39);
pub const GUTTER_FG: Color = Color::Rgb(0x54, 0x6E, 0x7A);
pub const GUTTER_CURRENT_FG: Color = Color::Rgb(0xEE, 0xFF, 0xFF);

pub const CURSORLINE_BG: Color = Color::Rgb(0x2A, 0x37, 0x3E);
pub const STATUSLINE_BG: Color = Color::Rgb(0x1B, 0x26, 0x2B);
pub const STATUSLINE_FG: Color = Color::Rgb(0xB0, 0xBE, 0xC5);

pub const MODE_NORMAL: Color = Color::Rgb(0x82, 0xAA, 0xFF);
pub const MODE_INSERT: Color = Color::Rgb(0xC3, 0xE8, 0x8D);
pub const MODE_PENDING: Color = Color::Rgb(0xFF, 0xCB, 0x6B);
pub const MODE_COMMAND: Color = Color::Rgb(0xC7, 0x92, 0xEA);
pub const MODE_LABEL_FG: Color = Color::Rgb(0x1B, 0x26, 0x2B);

pub const ERROR_FG: Color = Color::Rgb(0xF0, 0x71, 0x78);
