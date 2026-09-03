//! Grapheme- and width-aware helpers on top of the rope.
//!
//! Only LF line endings are supported (the author's editorconfig mandates LF).

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    Blank,
    Word,
    Punct,
}

pub fn char_class(c: char, big: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Blank
    } else if big || c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

/// Number of lines as the user sees them: the empty slot after a trailing
/// newline does not count, but an empty buffer still has one line.
pub fn text_lines(rope: &Rope) -> usize {
    let n = rope.len_chars();
    if n == 0 {
        return 1;
    }
    if rope.char(n - 1) == '\n' {
        (rope.len_lines() - 1).max(1)
    } else {
        rope.len_lines()
    }
}

/// Line length in chars, excluding the trailing newline.
pub fn line_len(rope: &Rope, line: usize) -> usize {
    let slice = rope.line(line);
    let mut len = slice.len_chars();
    if len > 0 && slice.char(len - 1) == '\n' {
        len -= 1;
    }
    len
}

/// Line content without the trailing newline.
pub fn line_content(rope: &Rope, line: usize) -> String {
    let mut s = rope.line(line).to_string();
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// One grapheme cluster of a line, with char offset and display geometry.
#[derive(Debug, Clone, Copy)]
pub struct Gr {
    /// Char offset within the line.
    pub char_off: usize,
    /// Number of chars in the cluster.
    pub chars: usize,
    /// First display cell (tab-aware).
    pub cell: usize,
    /// Display width in cells.
    pub width: usize,
}

pub fn line_graphemes(line: &str, tabstop: usize) -> Vec<Gr> {
    let mut out = Vec::new();
    let mut char_off = 0;
    let mut cell = 0;
    for g in line.graphemes(true) {
        let chars = g.chars().count();
        let width = if g == "\t" {
            tabstop - (cell % tabstop)
        } else {
            g.width().max(1)
        };
        out.push(Gr {
            char_off,
            chars,
            cell,
            width,
        });
        char_off += chars;
        cell += width;
    }
    out
}

/// Index into `grs` of the grapheme containing char column `col`, if any.
pub fn gr_index_at_col(grs: &[Gr], col: usize) -> Option<usize> {
    grs.iter().position(|g| col < g.char_off + g.chars)
}

/// Snap a char column to the start of its grapheme cluster.
pub fn snap_to_grapheme(grs: &[Gr], col: usize) -> usize {
    match gr_index_at_col(grs, col) {
        Some(i) => grs[i].char_off,
        None => grs.last().map(|g| g.char_off + g.chars).unwrap_or(0),
    }
}

/// Display cell of char column `col` (the cell after the last grapheme when
/// `col` is at end of line).
pub fn cell_at_col(grs: &[Gr], col: usize) -> usize {
    match gr_index_at_col(grs, col) {
        Some(i) => grs[i].cell,
        None => grs.last().map(|g| g.cell + g.width).unwrap_or(0),
    }
}

/// Char column whose grapheme covers display cell `cell`, clamped to the last
/// grapheme start (normal-mode style). Empty line yields 0.
pub fn col_at_cell(grs: &[Gr], cell: usize) -> usize {
    for g in grs {
        if cell < g.cell + g.width {
            return g.char_off;
        }
    }
    grs.last().map(|g| g.char_off).unwrap_or(0)
}

/// Char column of the first non-blank character; all-blank lines yield the
/// last column (vim's `^` behavior), empty lines 0.
pub fn first_non_blank(rope: &Rope, line: usize) -> usize {
    let len = line_len(rope, line);
    let slice = rope.line(line);
    for i in 0..len {
        if char_class(slice.char(i), false) != CharClass::Blank {
            return i;
        }
    }
    len.saturating_sub(1)
}

/// Leading whitespace of a line (for auto-indent).
pub fn line_indent(rope: &Rope, line: usize) -> String {
    let content = line_content(rope, line);
    content
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Last grapheme-start column usable in normal mode (0 on an empty line).
pub fn max_normal_col(rope: &Rope, line: usize, tabstop: usize) -> usize {
    let grs = line_graphemes(&line_content(rope, line), tabstop);
    grs.last().map(|g| g.char_off).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_lines_counts_like_vim() {
        assert_eq!(text_lines(&Rope::from_str("")), 1);
        assert_eq!(text_lines(&Rope::from_str("a")), 1);
        assert_eq!(text_lines(&Rope::from_str("a\n")), 1);
        assert_eq!(text_lines(&Rope::from_str("a\nb")), 2);
        assert_eq!(text_lines(&Rope::from_str("a\nb\n")), 2);
        assert_eq!(text_lines(&Rope::from_str("\n")), 1);
        assert_eq!(text_lines(&Rope::from_str("\n\n")), 2);
    }

    #[test]
    fn graphemes_handle_tabs_and_wide_chars() {
        let grs = line_graphemes("a\tb", 4);
        assert_eq!(grs[0].width, 1);
        assert_eq!(grs[1].width, 3); // tab from cell 1 to next stop at 4
        assert_eq!(grs[2].cell, 4);

        let grs = line_graphemes("日本", 4);
        assert_eq!(grs[0].width, 2);
        assert_eq!(grs[1].cell, 2);
    }

    #[test]
    fn grapheme_clusters_are_units() {
        // é as e + combining acute: one grapheme, two chars
        let grs = line_graphemes("e\u{301}x", 4);
        assert_eq!(grs.len(), 2);
        assert_eq!(grs[0].chars, 2);
        assert_eq!(snap_to_grapheme(&grs, 1), 0);
        assert_eq!(snap_to_grapheme(&grs, 2), 2);
    }

    #[test]
    fn first_non_blank_variants() {
        let rope = Rope::from_str("  abc\n    \n\n");
        assert_eq!(first_non_blank(&rope, 0), 2);
        assert_eq!(first_non_blank(&rope, 1), 3); // all blanks -> last col
        assert_eq!(first_non_blank(&rope, 2), 0); // empty line
    }
}
