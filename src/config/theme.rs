//! Capture-name → style mapping (material oceanic). The capture list is the
//! vocabulary handed to every grammar's highlight configuration; queries'
//! dotted captures fall back to their longest configured prefix (the
//! tree-sitter highlighter does this matching).

use ratatui::style::{Color, Modifier, Style};

use super::palette;

/// Recognized capture names. Order defines the indices stored in the syntax
/// cache; keep `STYLES` parallel.
pub const CAPTURES: &[&str] = &[
    "none",
    "attribute",
    "boolean",
    "comment",
    "constant",
    "constructor",
    "escape",
    "function",
    "keyword",
    "label",
    "number",
    "operator",
    "property",
    "punctuation",
    "string.escape",
    "string.special",
    "string",
    "type",
    "variable.builtin",
    "variable.parameter",
    "text.title",
    "text.emphasis",
    "text.strong",
    "text.literal",
    "text.uri",
    "text.reference",
];

const fn fg(color: Color) -> (Color, Modifier) {
    (color, Modifier::empty())
}

/// Parallel to `CAPTURES`.
const STYLES: &[(Color, Modifier)] = &[
    fg(palette::FG),                       // none (query-level reset)
    (palette::YELLOW, Modifier::ITALIC),   // attribute
    fg(palette::ORANGE),                   // boolean
    (palette::COMMENT, Modifier::ITALIC),  // comment
    fg(palette::ORANGE),                   // constant
    fg(palette::YELLOW),                   // constructor
    fg(palette::CYAN),                     // escape
    fg(palette::BLUE),                     // function
    fg(palette::PURPLE),                   // keyword
    (palette::CYAN, Modifier::ITALIC),     // label
    fg(palette::ORANGE),                   // number
    fg(palette::CYAN),                     // operator
    fg(palette::PALE),                     // property
    fg(palette::CYAN),                     // punctuation
    fg(palette::CYAN),                     // string.escape
    fg(palette::ORANGE),                   // string.special
    fg(palette::GREEN),                    // string
    fg(palette::YELLOW),                   // type
    fg(palette::RED),                      // variable.builtin
    fg(palette::ORANGE),                   // variable.parameter
    (palette::BLUE, Modifier::BOLD),       // text.title
    (palette::FG, Modifier::ITALIC),       // text.emphasis
    (palette::YELLOW, Modifier::BOLD),     // text.strong
    fg(palette::GREEN),                    // text.literal
    (palette::CYAN, Modifier::UNDERLINED), // text.uri
    fg(palette::PALE),                     // text.reference
];

/// Style for a capture index from the syntax cache.
pub fn capture_style(index: usize) -> Style {
    let (color, modifier) = STYLES.get(index).copied().unwrap_or(fg(palette::FG));
    Style::default().fg(color).add_modifier(modifier)
}

/// Index of a capture name (test helper).
pub fn capture_index(name: &str) -> Option<usize> {
    CAPTURES.iter().position(|c| *c == name)
}
