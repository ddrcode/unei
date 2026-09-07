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
    /// CJK scripts form their own word classes, like vim's utf_class: a run of
    /// kanji and a run of latin are separate small words.
    Script(u8),
}

fn script_class(c: char) -> Option<u8> {
    Some(match c as u32 {
        0x3040..=0x309F => 1,                     // hiragana
        0x30A0..=0x30FF => 2,                     // katakana
        0x3400..=0x4DBF | 0x4E00..=0x9FFF => 3,   // CJK ideographs
        0xF900..=0xFAFF | 0x20000..=0x2FA1F => 3, // CJK compat / ext
        0xAC00..=0xD7A3 => 4,                     // hangul
        _ => return None,
    })
}

pub fn char_class(c: char, big: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Blank
    } else if big {
        CharClass::Word
    } else if let Some(s) = script_class(c) {
        CharClass::Script(s)
    } else if c.is_alphanumeric() || c == '_' {
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
    /// A space or tab — a soft-wrap break opportunity (#9).
    pub is_blank: bool,
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
            is_blank: g == " " || g == "\t",
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

/// One display row of a soft-wrapped line (#9): the half-open cell range
/// `[start_cell, end_cell)` it shows and the char columns `[start_col,
/// end_col)` those cells hold. A line always has at least one row; an empty
/// line has one empty row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrapRow {
    pub start_cell: usize,
    pub end_cell: usize,
    pub start_col: usize,
    pub end_col: usize,
}

/// Soft-wraps a line's graphemes into rows of at most `width` cells, breaking
/// after whitespace when a row would overflow (vim's `wrap` + `linebreak`);
/// a single run longer than the row is broken hard at the cell limit. The
/// trailing space stays at the end of the row it closes, as in vim.
pub fn wrap_rows(grs: &[Gr], width: usize) -> Vec<WrapRow> {
    let width = width.max(1);
    let total_cells = grs.last().map(|g| g.cell + g.width).unwrap_or(0);
    let total_cols = grs.last().map(|g| g.char_off + g.chars).unwrap_or(0);
    let mut rows = Vec::new();
    let mut row_start = 0usize; // index into grs
    let mut last_break: Option<usize> = None; // index just after a whitespace cluster
    let mut i = 0usize;
    while i < grs.len() {
        let g = &grs[i];
        let row_cell0 = grs[row_start].cell;
        if g.cell + g.width > row_cell0 + width && i > row_start {
            // this cluster would overflow the row. If it is itself a blank,
            // the row closes right after it — "alpha beta" stays together and
            // the space is swallowed by the wrap (vim shows it clipped at the
            // edge). Otherwise close at the last blank, or hard-break here.
            let cut = if g.is_blank {
                i + 1
            } else {
                match last_break {
                    Some(b) if b > row_start => b,
                    _ => i,
                }
            };
            let end_g = grs.get(cut);
            rows.push(WrapRow {
                start_cell: grs[row_start].cell,
                end_cell: end_g.map(|e| e.cell).unwrap_or(total_cells),
                start_col: grs[row_start].char_off,
                end_col: end_g.map(|e| e.char_off).unwrap_or(total_cols),
            });
            row_start = cut;
            last_break = None;
            i = cut;
            continue;
        }
        if g.is_blank {
            last_break = Some(i + 1); // break *after* the space, vim-style
        }
        i += 1;
    }
    // the final row — unless a swallowed trailing blank already closed the
    // line exactly (then there is nothing left to show)
    if row_start < grs.len() || rows.is_empty() {
        let start = grs.get(row_start);
        rows.push(WrapRow {
            start_cell: start.map(|g| g.cell).unwrap_or(total_cells),
            end_cell: total_cells,
            start_col: start.map(|g| g.char_off).unwrap_or(total_cols),
            end_col: total_cols,
        });
    }
    rows
}

/// Index of the row (within `rows`) that holds char column `col`; a column
/// at or past the line's end lands on the last row.
pub fn row_of_col(rows: &[WrapRow], col: usize) -> usize {
    rows.iter()
        .position(|r| col < r.end_col)
        .unwrap_or(rows.len().saturating_sub(1))
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

    fn wrapped(line: &str, width: usize) -> Vec<(usize, usize)> {
        wrap_rows(&line_graphemes(line, 4), width)
            .into_iter()
            .map(|r| (r.start_col, r.end_col))
            .collect()
    }
    fn row_texts(line: &str, width: usize) -> Vec<String> {
        wrapped(line, width)
            .into_iter()
            .map(|(s, e)| line.chars().skip(s).take(e - s).collect())
            .collect()
    }

    #[test]
    fn wrap_breaks_after_whitespace_like_linebreak() {
        // 10 cells wide: "the quick " (10) | "brown fox" (9)
        assert_eq!(
            row_texts("the quick brown fox", 10),
            ["the quick ", "brown fox"]
        );
        // the space closing a row stays on it, exactly like vim
        assert_eq!(row_texts("aaa bbb", 4), ["aaa ", "bbb"]);
    }

    #[test]
    fn wrap_hard_breaks_a_word_longer_than_the_row() {
        assert_eq!(row_texts("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        // a long word after a short one: break at the space, then hard-break
        assert_eq!(row_texts("ab cdefghij", 4), ["ab ", "cdef", "ghij"]);
    }

    #[test]
    fn wrap_edge_cases() {
        assert_eq!(wrapped("", 10), [(0, 0)]); // one empty row
        assert_eq!(row_texts("exact", 5), ["exact"]); // fits exactly: one row
        assert_eq!(row_texts("exact!", 5), ["exact", "!"]);
        // a tab counts by its expanded width (tabstop 4)
        assert_eq!(row_texts("\tab", 4), ["\t", "ab"]);
    }

    #[test]
    fn row_of_col_maps_cursor_to_its_row() {
        let rows = wrap_rows(&line_graphemes("aaa bbb ccc", 4), 4); // "aaa ","bbb ","ccc"
        assert_eq!(rows.len(), 3);
        assert_eq!(row_of_col(&rows, 0), 0);
        assert_eq!(row_of_col(&rows, 3), 0); // the closing space
        assert_eq!(row_of_col(&rows, 4), 1);
        assert_eq!(row_of_col(&rows, 10), 2);
        assert_eq!(row_of_col(&rows, 11), 2); // past the end: last row
        assert_eq!(row_of_col(&wrap_rows(&[], 4), 0), 0);
    }

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
