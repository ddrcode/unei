//! Text objects (ticket #12). Operators and visual mode take an object
//! after the prefix `a` (around) or `n` (inner — `n` because the IJKL swap
//! that moved `i` to insert freed it; see docs/decisions.md). Each resolver
//! returns a char range `start..end` (end exclusive) and whether it is
//! linewise. These are the LEXICAL objects — words, paragraphs, brackets,
//! quotes — computed by plain text scanning, which resolves them exactly.
//! The SYNTACTIC objects (`f` function, `c` class) can't be scanned; they
//! live in `syntax::text_object` via tree-sitter (#78). Each object still has
//! exactly one mechanism — no overlap, no fallback (docs/decisions.md).

use ropey::Rope;

use crate::core::buffer::Cursor;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Word {
        big: bool,
    },
    Paragraph,
    /// A bracket pair, e.g. `('(' , ')')`.
    Pair(char, char),
    /// A quote character, e.g. `"`.
    Quote(char),
}

/// Maps the object key to its kind: `w`/`W`, `p`, the bracket pairs
/// (`(` `)` `b`; `{` `}` `B`; `[` `]`; `<` `>`), and the quotes.
pub fn parse(ch: char) -> Option<ObjKind> {
    Some(match ch {
        'w' => ObjKind::Word { big: false },
        'W' => ObjKind::Word { big: true },
        'p' => ObjKind::Paragraph,
        '(' | ')' | 'b' => ObjKind::Pair('(', ')'),
        '{' | '}' | 'B' => ObjKind::Pair('{', '}'),
        '[' | ']' => ObjKind::Pair('[', ']'),
        '<' | '>' => ObjKind::Pair('<', '>'),
        '"' => ObjKind::Quote('"'),
        '\'' => ObjKind::Quote('\''),
        '`' => ObjKind::Quote('`'),
        _ => return None,
    })
}

/// A resolved object: `start..end` in chars, and whether it spans whole
/// lines (paragraphs) so the operator treats it linewise.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Range {
    pub start: usize,
    pub end: usize,
    pub linewise: bool,
}

pub fn resolve(
    rope: &Rope,
    cursor: Cursor,
    kind: ObjKind,
    around: bool,
    count: usize,
) -> Option<Range> {
    let base = rope.line_to_char(cursor.line);
    let line_len = crate::core::text::line_len(rope, cursor.line);
    let abs = base + cursor.col.min(line_len);
    match kind {
        ObjKind::Word { big } => word_object(rope, abs, big, around, count.max(1)),
        ObjKind::Paragraph => paragraph_object(rope, cursor.line, around, count.max(1)),
        ObjKind::Pair(open, close) => pair_object(rope, abs, open, close, around),
        ObjKind::Quote(q) => quote_object(rope, cursor, q, around),
    }
}

// ----------------------------------------------------------------------
// words
// ----------------------------------------------------------------------

#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Word,
    Punct,
    Blank,
}

fn class(c: char, big: bool) -> Class {
    if c == ' ' || c == '\t' {
        Class::Blank
    } else if big || c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

/// `nw`/`aw` (and `nW`/`aW`): the run of same-class characters under the
/// cursor; the around form adds trailing whitespace, or leading whitespace
/// when there is none trailing. Does not cross line boundaries.
fn word_object(rope: &Rope, abs: usize, big: bool, around: bool, count: usize) -> Option<Range> {
    let len = rope.len_chars();
    if len == 0 {
        return None;
    }
    let abs = abs.min(len - 1);
    if rope.char(abs) == '\n' {
        return None;
    }
    let same =
        |i: usize, cl: Class| i < len && rope.char(i) != '\n' && class(rope.char(i), big) == cl;

    let mut start = abs;
    let cl0 = class(rope.char(abs), big);
    while start > 0 && same(start - 1, cl0) {
        start -= 1;
    }
    let mut end = abs;
    // extend across `count` word-runs (each run + following run for counts)
    let mut remaining = count;
    loop {
        let cl = class(rope.char(end), big);
        while same(end + 1, cl) {
            end += 1;
        }
        remaining -= 1;
        if remaining == 0 || end + 1 >= len || rope.char(end + 1) == '\n' {
            break;
        }
        end += 1; // step into the next run
    }
    end += 1; // exclusive

    if !around {
        return Some(Range {
            start,
            end,
            linewise: false,
        });
    }
    // around: trailing whitespace, else leading whitespace
    let mut a_end = end;
    while a_end < len && matches!(rope.char(a_end), ' ' | '\t') {
        a_end += 1;
    }
    if a_end > end {
        return Some(Range {
            start,
            end: a_end,
            linewise: false,
        });
    }
    let mut a_start = start;
    while a_start > 0 && matches!(rope.char(a_start - 1), ' ' | '\t') {
        a_start -= 1;
    }
    Some(Range {
        start: a_start,
        end,
        linewise: false,
    })
}

// ----------------------------------------------------------------------
// paragraphs
// ----------------------------------------------------------------------

fn blank_line(rope: &Rope, line: usize) -> bool {
    crate::core::text::line_content(rope, line)
        .trim()
        .is_empty()
}

/// `np`/`ap`: the block of like (blank / non-blank) lines around the
/// cursor; the around form adds the following blank lines (or preceding).
fn paragraph_object(rope: &Rope, line: usize, around: bool, count: usize) -> Option<Range> {
    let last = crate::core::text::text_lines(rope).saturating_sub(1);
    let on_blank = blank_line(rope, line);
    let mut l1 = line;
    while l1 > 0 && blank_line(rope, l1 - 1) == on_blank {
        l1 -= 1;
    }
    let mut l2 = line;
    let mut blocks = count;
    loop {
        let block_blank = blank_line(rope, l2);
        while l2 < last && blank_line(rope, l2 + 1) == block_blank {
            l2 += 1;
        }
        blocks -= 1;
        if blocks == 0 || l2 >= last {
            break;
        }
        l2 += 1;
    }
    if around {
        let after_blank = !blank_line(rope, l2);
        let mut a2 = l2;
        while a2 < last && blank_line(rope, a2 + 1) == after_blank {
            a2 += 1;
        }
        if a2 > l2 {
            l2 = a2;
        } else {
            while l1 > 0 && blank_line(rope, l1 - 1) != on_blank {
                l1 -= 1;
            }
        }
    }
    let start = rope.line_to_char(l1);
    let end = if l2 + 1 >= rope.len_lines() {
        rope.len_chars()
    } else {
        rope.line_to_char(l2 + 1)
    };
    Some(Range {
        start,
        end,
        linewise: true,
    })
}

// ----------------------------------------------------------------------
// bracket pairs
// ----------------------------------------------------------------------

/// `n(`/`a(` etc.: the pair enclosing the cursor (inclusive when around).
/// Scans outward across lines, honoring nesting of the same pair type.
fn pair_object(rope: &Rope, abs: usize, open: char, close: char, around: bool) -> Option<Range> {
    let len = rope.len_chars();
    if len == 0 {
        return None;
    }
    let abs = abs.min(len - 1);
    // find the enclosing open: the cursor char itself may be the open
    let open_pos = if rope.char(abs) == open {
        abs
    } else {
        let mut depth = 0i32;
        let mut i = abs;
        loop {
            let c = rope.char(i);
            if c == close && i != abs {
                depth += 1;
            } else if c == open {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            if i == 0 {
                return None;
            }
            i -= 1;
        }
        i
    };
    // matching close forward from just after the open
    let mut depth = 0i32;
    let mut j = open_pos + 1;
    let close_pos = loop {
        if j >= len {
            return None;
        }
        let c = rope.char(j);
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                break j;
            }
            depth -= 1;
        }
        j += 1;
    };
    if around {
        Some(Range {
            start: open_pos,
            end: close_pos + 1,
            linewise: false,
        })
    } else {
        Some(Range {
            start: open_pos + 1,
            end: close_pos,
            linewise: false,
        })
    }
}

// ----------------------------------------------------------------------
// quotes
// ----------------------------------------------------------------------

/// `n"`/`a"` (and `'`, `` ` ``): the quoted span on the cursor line. Quotes
/// don't nest, so they pair left-to-right; the cursor's enclosing pair, or
/// the next one after it, wins.
fn quote_object(rope: &Rope, cursor: Cursor, q: char, around: bool) -> Option<Range> {
    let base = rope.line_to_char(cursor.line);
    let line = crate::core::text::line_content(rope, cursor.line);
    // char columns of each quote on the line, in order
    let cols: Vec<usize> = line
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == q)
        .map(|(i, _)| i)
        .collect();
    if cols.len() < 2 {
        return None;
    }
    let cur = cursor.col;
    // walk pairs (0,1),(2,3),… find the one covering the cursor or after it
    let mut chosen: Option<(usize, usize)> = None;
    let mut i = 0;
    while i + 1 < cols.len() {
        let (o, c) = (cols[i], cols[i + 1]);
        if cur <= c {
            chosen = Some((o, c));
            break;
        }
        i += 2;
    }
    let (o, c) = chosen?;
    if around {
        // include a trailing space if present, else a leading one (vim)
        let chars: Vec<char> = line.chars().collect();
        let mut end = c + 1;
        if end < chars.len() && matches!(chars[end], ' ' | '\t') {
            end += 1;
            return Some(Range {
                start: base + o,
                end: base + end,
                linewise: false,
            });
        }
        let mut start = o;
        if start > 0 && matches!(chars[start - 1], ' ' | '\t') {
            start -= 1;
        }
        Some(Range {
            start: base + start,
            end: base + c + 1,
            linewise: false,
        })
    } else {
        Some(Range {
            start: base + o + 1,
            end: base + c,
            linewise: false,
        })
    }
}
