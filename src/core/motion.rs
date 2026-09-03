use ropey::Rope;

use crate::core::buffer::Cursor;
use crate::core::commands::FindKind;
use crate::core::text::{
    self, CharClass, cell_at_col, char_class, col_at_cell, first_non_blank, gr_index_at_col,
    line_content, line_graphemes, line_len, max_normal_col, text_lines,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    FirstNonBlank,
    LineEnd,
    WordForward {
        big: bool,
    },
    WordBack {
        big: bool,
    },
    WordEnd {
        big: bool,
    },
    /// `gg`
    GotoFirst,
    /// `G`
    GotoLast,
    /// `}`
    ParaForward,
    /// `{`
    ParaBack,
    Find {
        kind: FindKind,
        ch: char,
    },
    /// `;`
    RepeatFind,
    /// `,`
    RepeatFindRev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionKind {
    Linewise,
    Exclusive,
    Inclusive,
}

pub struct MotionCtx<'a> {
    pub rope: &'a Rope,
    /// Sticky display column for vertical movement (usize::MAX = line end).
    pub goal: Option<usize>,
    pub last_find: Option<(FindKind, char)>,
    pub tabstop: usize,
    /// Operator targets may sit on the newline (col == line length).
    pub for_operator: bool,
}

pub struct MotionOutcome {
    pub cursor: Cursor,
    pub kind: MotionKind,
    /// `Some` keeps/sets the sticky goal column; `None` resets it.
    pub new_goal: Option<usize>,
    pub new_last_find: Option<(FindKind, char)>,
}

fn outcome(cursor: Cursor, kind: MotionKind) -> MotionOutcome {
    MotionOutcome {
        cursor,
        kind,
        new_goal: None,
        new_last_find: None,
    }
}

fn cls_at(rope: &Rope, i: usize, big: bool) -> CharClass {
    char_class(rope.char(i), big)
}

pub fn next_word_start(rope: &Rope, abs: usize, big: bool) -> usize {
    let len = rope.len_chars();
    if len == 0 || abs >= len {
        return len;
    }
    let mut i = abs;
    let start = cls_at(rope, i, big);
    i += 1;
    if start != CharClass::Blank {
        while i < len && rope.char(i) != '\n' && cls_at(rope, i, big) == start {
            i += 1;
        }
    }
    while i < len && cls_at(rope, i, big) == CharClass::Blank {
        // stop on an empty line: its position is the char right after a newline
        // that is immediately followed by another newline (or EOF)
        if rope.char(i) == '\n' && i + 1 < len && rope.char(i + 1) == '\n' {
            return i + 1;
        }
        i += 1;
    }
    i
}

pub fn word_end(rope: &Rope, abs: usize, big: bool) -> usize {
    let len = rope.len_chars();
    if len == 0 {
        return 0;
    }
    let mut i = abs + 1;
    while i < len && cls_at(rope, i, big) == CharClass::Blank {
        i += 1;
    }
    if i >= len {
        return len - 1;
    }
    let cl = cls_at(rope, i, big);
    while i + 1 < len && cls_at(rope, i + 1, big) == cl {
        i += 1;
    }
    i
}

pub fn prev_word_start(rope: &Rope, abs: usize, big: bool) -> usize {
    if abs == 0 {
        return 0;
    }
    let mut i = abs - 1;
    while i > 0 && cls_at(rope, i, false) == CharClass::Blank {
        if rope.char(i) == '\n' && rope.char(i - 1) == '\n' {
            return i; // empty line
        }
        i -= 1;
    }
    if cls_at(rope, i, big) == CharClass::Blank {
        return i; // start of buffer on a blank
    }
    let cl = cls_at(rope, i, big);
    while i > 0 && cls_at(rope, i - 1, big) == cl {
        i -= 1;
    }
    i
}

fn is_blank_line(rope: &Rope, line: usize) -> bool {
    line_len(rope, line) == 0
}

fn para_forward(rope: &Rope, line: usize) -> usize {
    let last = text_lines(rope) - 1;
    let mut l = line;
    while l < last && is_blank_line(rope, l) {
        l += 1;
    }
    while l < last && !is_blank_line(rope, l) {
        l += 1;
    }
    l
}

fn para_back(rope: &Rope, line: usize) -> usize {
    let mut l = line;
    while l > 0 && is_blank_line(rope, l) {
        l -= 1;
    }
    while l > 0 && !is_blank_line(rope, l) {
        l -= 1;
    }
    l
}

/// Finds the target column for f/F/t/T on the cursor line. `skip_adjacent`
/// makes a repeated `t`/`T` useful instead of being stuck on the spot.
fn find_in_line(
    rope: &Rope,
    cursor: Cursor,
    kind: FindKind,
    ch: char,
    count: usize,
    skip_adjacent: bool,
) -> Option<usize> {
    let chars: Vec<char> = line_content(rope, cursor.line).chars().collect();
    let mut remaining = count;
    match kind {
        FindKind::To | FindKind::Till => {
            let from = if skip_adjacent && kind == FindKind::Till {
                cursor.col + 2
            } else {
                cursor.col + 1
            };
            for (c, cur) in chars.iter().enumerate().skip(from) {
                if *cur == ch {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(if kind == FindKind::Till { c - 1 } else { c });
                    }
                }
            }
            None
        }
        FindKind::ToBack | FindKind::TillBack => {
            let until = if skip_adjacent && kind == FindKind::TillBack {
                cursor.col.saturating_sub(1)
            } else {
                cursor.col
            };
            for c in (0..until.min(chars.len())).rev() {
                if chars[c] == ch {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(if kind == FindKind::TillBack { c + 1 } else { c });
                    }
                }
            }
            None
        }
    }
}

fn abs_of(rope: &Rope, cursor: Cursor) -> usize {
    rope.line_to_char(cursor.line) + cursor.col
}

/// Converts an absolute char index back to a cursor, clamped to valid
/// normal-mode or operator-target positions.
fn cursor_of_abs(rope: &Rope, abs: usize, tabstop: usize, for_operator: bool) -> Cursor {
    let len = rope.len_chars();
    let last = text_lines(rope) - 1;
    if abs >= len {
        return if for_operator {
            Cursor::new(last, line_len(rope, last))
        } else {
            Cursor::new(last, max_normal_col(rope, last, tabstop))
        };
    }
    let line = rope.char_to_line(abs).min(last);
    let mut col = abs - rope.line_to_char(line);
    let limit = if for_operator {
        line_len(rope, line)
    } else {
        max_normal_col(rope, line, tabstop)
    };
    if col > limit {
        col = limit;
    }
    // snap into grapheme boundaries
    let grs = line_graphemes(&line_content(rope, line), tabstop);
    if col < line_len(rope, line) {
        col = text::snap_to_grapheme(&grs, col);
    }
    Cursor::new(line, col)
}

pub fn resolve(
    motion: Motion,
    count: Option<usize>,
    cursor: Cursor,
    ctx: &MotionCtx,
) -> Option<MotionOutcome> {
    let rope = ctx.rope;
    let n = count.unwrap_or(1).max(1);
    let last_line = text_lines(rope) - 1;
    let grs = line_graphemes(&line_content(rope, cursor.line), ctx.tabstop);

    match motion {
        Motion::Left => {
            if grs.is_empty() {
                return Some(outcome(Cursor::new(cursor.line, 0), MotionKind::Exclusive));
            }
            let idx = gr_index_at_col(&grs, cursor.col).unwrap_or(grs.len());
            let target = grs[idx.saturating_sub(n).min(grs.len() - 1)].char_off;
            Some(outcome(
                Cursor::new(cursor.line, target),
                MotionKind::Exclusive,
            ))
        }
        Motion::Right => {
            if grs.is_empty() {
                return Some(outcome(cursor, MotionKind::Exclusive));
            }
            let idx = gr_index_at_col(&grs, cursor.col).unwrap_or(grs.len() - 1);
            let max_idx = if ctx.for_operator {
                grs.len()
            } else {
                grs.len() - 1
            };
            let tgt_idx = (idx + n).min(max_idx);
            let col = if tgt_idx == grs.len() {
                line_len(rope, cursor.line)
            } else {
                grs[tgt_idx].char_off
            };
            Some(outcome(
                Cursor::new(cursor.line, col),
                MotionKind::Exclusive,
            ))
        }
        Motion::Up | Motion::Down => {
            let line = if motion == Motion::Up {
                if cursor.line == 0 {
                    return None;
                }
                cursor.line.saturating_sub(n)
            } else {
                if cursor.line == last_line {
                    return None;
                }
                (cursor.line + n).min(last_line)
            };
            let goal = ctx.goal.unwrap_or_else(|| cell_at_col(&grs, cursor.col));
            let tgrs = line_graphemes(&line_content(rope, line), ctx.tabstop);
            let col = if goal == usize::MAX {
                tgrs.last().map(|g| g.char_off).unwrap_or(0)
            } else {
                col_at_cell(&tgrs, goal)
            };
            Some(MotionOutcome {
                cursor: Cursor::new(line, col),
                kind: MotionKind::Linewise,
                new_goal: Some(goal),
                new_last_find: None,
            })
        }
        Motion::LineStart => Some(outcome(Cursor::new(cursor.line, 0), MotionKind::Exclusive)),
        Motion::FirstNonBlank => Some(outcome(
            Cursor::new(cursor.line, first_non_blank(rope, cursor.line)),
            MotionKind::Exclusive,
        )),
        Motion::LineEnd => {
            let line = (cursor.line + n - 1).min(last_line);
            let col = max_normal_col(rope, line, ctx.tabstop);
            Some(MotionOutcome {
                cursor: Cursor::new(line, col),
                kind: MotionKind::Inclusive,
                new_goal: Some(usize::MAX),
                new_last_find: None,
            })
        }
        Motion::WordForward { big } => {
            let mut abs = abs_of(rope, cursor);
            for _ in 0..n {
                abs = next_word_start(rope, abs, big);
            }
            Some(outcome(
                cursor_of_abs(rope, abs, ctx.tabstop, ctx.for_operator),
                MotionKind::Exclusive,
            ))
        }
        Motion::WordBack { big } => {
            let mut abs = abs_of(rope, cursor);
            for _ in 0..n {
                abs = prev_word_start(rope, abs, big);
            }
            Some(outcome(
                cursor_of_abs(rope, abs, ctx.tabstop, false),
                MotionKind::Exclusive,
            ))
        }
        Motion::WordEnd { big } => {
            let mut abs = abs_of(rope, cursor);
            for _ in 0..n {
                abs = word_end(rope, abs, big);
            }
            Some(outcome(
                cursor_of_abs(rope, abs, ctx.tabstop, false),
                MotionKind::Inclusive,
            ))
        }
        Motion::GotoFirst | Motion::GotoLast => {
            let line = match count {
                Some(c) => (c - 1).min(last_line),
                None if motion == Motion::GotoFirst => 0,
                None => last_line,
            };
            Some(outcome(
                Cursor::new(line, first_non_blank(rope, line)),
                MotionKind::Linewise,
            ))
        }
        Motion::ParaForward => {
            let mut line = cursor.line;
            for _ in 0..n {
                line = para_forward(rope, line);
            }
            let col = if is_blank_line(rope, line) {
                0
            } else {
                max_normal_col(rope, line, ctx.tabstop)
            };
            Some(outcome(Cursor::new(line, col), MotionKind::Exclusive))
        }
        Motion::ParaBack => {
            let mut line = cursor.line;
            for _ in 0..n {
                line = para_back(rope, line);
            }
            Some(outcome(Cursor::new(line, 0), MotionKind::Exclusive))
        }
        Motion::Find { kind, ch } => {
            let col = find_in_line(rope, cursor, kind, ch, n, false)?;
            let k = match kind {
                FindKind::To | FindKind::Till => MotionKind::Inclusive,
                _ => MotionKind::Exclusive,
            };
            Some(MotionOutcome {
                cursor: Cursor::new(cursor.line, col),
                kind: k,
                new_goal: None,
                new_last_find: Some((kind, ch)),
            })
        }
        Motion::RepeatFind => {
            let (kind, ch) = ctx.last_find?;
            let col = find_in_line(rope, cursor, kind, ch, n, true)?;
            let k = match kind {
                FindKind::To | FindKind::Till => MotionKind::Inclusive,
                _ => MotionKind::Exclusive,
            };
            Some(outcome(Cursor::new(cursor.line, col), k))
        }
        Motion::RepeatFindRev => {
            let (kind, ch) = ctx.last_find?;
            let rev = match kind {
                FindKind::To => FindKind::ToBack,
                FindKind::ToBack => FindKind::To,
                FindKind::Till => FindKind::TillBack,
                FindKind::TillBack => FindKind::Till,
            };
            let col = find_in_line(rope, cursor, rev, ch, n, true)?;
            let k = match rev {
                FindKind::To | FindKind::Till => MotionKind::Inclusive,
                _ => MotionKind::Exclusive,
            };
            Some(outcome(Cursor::new(cursor.line, col), k))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(rope: &Rope) -> MotionCtx<'_> {
        MotionCtx {
            rope,
            goal: None,
            last_find: None,
            tabstop: 4,
            for_operator: false,
        }
    }

    fn mv(rope: &Rope, cur: (usize, usize), m: Motion) -> (usize, usize) {
        let out = resolve(m, None, Cursor::new(cur.0, cur.1), &ctx(rope)).unwrap();
        (out.cursor.line, out.cursor.col)
    }

    #[test]
    fn word_forward_basics() {
        let r = Rope::from_str("foo bar_baz  qux\n");
        assert_eq!(mv(&r, (0, 0), Motion::WordForward { big: false }), (0, 4));
        assert_eq!(mv(&r, (0, 4), Motion::WordForward { big: false }), (0, 13));
    }

    #[test]
    fn word_forward_punctuation_and_big() {
        let r = Rope::from_str("foo.bar qux\n");
        assert_eq!(mv(&r, (0, 0), Motion::WordForward { big: false }), (0, 3));
        assert_eq!(mv(&r, (0, 3), Motion::WordForward { big: false }), (0, 4));
        assert_eq!(mv(&r, (0, 0), Motion::WordForward { big: true }), (0, 8));
    }

    #[test]
    fn word_forward_stops_on_empty_line() {
        let r = Rope::from_str("foo\n\nbar\n");
        assert_eq!(mv(&r, (0, 0), Motion::WordForward { big: false }), (1, 0));
        assert_eq!(mv(&r, (1, 0), Motion::WordForward { big: false }), (2, 0));
    }

    #[test]
    fn word_back_and_end() {
        let r = Rope::from_str("foo bar baz\n");
        assert_eq!(mv(&r, (0, 8), Motion::WordBack { big: false }), (0, 4));
        assert_eq!(mv(&r, (0, 5), Motion::WordBack { big: false }), (0, 4));
        assert_eq!(mv(&r, (0, 0), Motion::WordEnd { big: false }), (0, 2));
        assert_eq!(mv(&r, (0, 2), Motion::WordEnd { big: false }), (0, 6));
    }

    #[test]
    fn word_at_buffer_end_clamps() {
        let r = Rope::from_str("foo bar\n");
        assert_eq!(mv(&r, (0, 4), Motion::WordForward { big: false }), (0, 6));
        assert_eq!(mv(&r, (0, 6), Motion::WordEnd { big: false }), (0, 6));
    }

    #[test]
    fn vertical_uses_goal_column() {
        let r = Rope::from_str("a long line\nx\nanother long\n");
        let out = resolve(Motion::Down, None, Cursor::new(0, 6), &ctx(&r)).unwrap();
        assert_eq!((out.cursor.line, out.cursor.col), (1, 0));
        let mut c = ctx(&r);
        c.goal = out.new_goal;
        let out2 = resolve(Motion::Down, None, out.cursor, &c).unwrap();
        assert_eq!((out2.cursor.line, out2.cursor.col), (2, 6));
    }

    #[test]
    fn vertical_at_edges_fails() {
        let r = Rope::from_str("a\nb\n");
        assert!(resolve(Motion::Up, None, Cursor::new(0, 0), &ctx(&r)).is_none());
        assert!(resolve(Motion::Down, None, Cursor::new(1, 0), &ctx(&r)).is_none());
        assert_eq!(mv(&r, (1, 0), Motion::Up), (0, 0));
    }

    #[test]
    fn goto_with_count() {
        let r = Rope::from_str("one\ntwo\n  three\n");
        let out = resolve(Motion::GotoLast, Some(3), Cursor::new(0, 0), &ctx(&r)).unwrap();
        assert_eq!((out.cursor.line, out.cursor.col), (2, 2));
        let out = resolve(Motion::GotoFirst, None, Cursor::new(2, 0), &ctx(&r)).unwrap();
        assert_eq!(out.cursor.line, 0);
    }

    #[test]
    fn find_and_till() {
        let r = Rope::from_str("abcabc\n");
        let m = Motion::Find {
            kind: FindKind::To,
            ch: 'c',
        };
        assert_eq!(mv(&r, (0, 0), m), (0, 2));
        let out = resolve(m, Some(2), Cursor::new(0, 0), &ctx(&r)).unwrap();
        assert_eq!(out.cursor.col, 5);
        let m = Motion::Find {
            kind: FindKind::Till,
            ch: 'c',
        };
        assert_eq!(mv(&r, (0, 0), m), (0, 1));
        let m = Motion::Find {
            kind: FindKind::ToBack,
            ch: 'a',
        };
        assert_eq!(mv(&r, (0, 5), m), (0, 3));
        // miss -> None
        assert!(
            resolve(
                Motion::Find {
                    kind: FindKind::To,
                    ch: 'z'
                },
                None,
                Cursor::new(0, 0),
                &ctx(&r)
            )
            .is_none()
        );
    }

    #[test]
    fn repeat_till_is_not_stuck() {
        let r = Rope::from_str("xaxa\n");
        let m = Motion::Find {
            kind: FindKind::Till,
            ch: 'a',
        };
        let out = resolve(m, None, Cursor::new(0, 0), &ctx(&r)).unwrap();
        assert_eq!(out.cursor.col, 0); // already right before 'a'
        let mut c = ctx(&r);
        c.last_find = Some((FindKind::Till, 'a'));
        let out = resolve(Motion::RepeatFind, None, Cursor::new(0, 0), &c).unwrap();
        assert_eq!(out.cursor.col, 2);
    }

    #[test]
    fn paragraph_motions() {
        let r = Rope::from_str("a\nb\n\nc\nd\n\n\ne\n");
        assert_eq!(mv(&r, (0, 0), Motion::ParaForward), (2, 0));
        assert_eq!(mv(&r, (2, 0), Motion::ParaForward), (5, 0));
        assert_eq!(mv(&r, (7, 0), Motion::ParaBack), (6, 0));
        assert_eq!(mv(&r, (3, 0), Motion::ParaBack), (2, 0));
        assert_eq!(mv(&r, (1, 0), Motion::ParaBack), (0, 0));
    }

    #[test]
    fn left_right_respect_graphemes() {
        let r = Rope::from_str("ae\u{301}z\n"); // a, é (2 chars), z
        assert_eq!(mv(&r, (0, 0), Motion::Right), (0, 1));
        assert_eq!(mv(&r, (0, 1), Motion::Right), (0, 3));
        assert_eq!(mv(&r, (0, 3), Motion::Left), (0, 1));
        // right clamps at last grapheme in normal mode
        assert_eq!(mv(&r, (0, 3), Motion::Right), (0, 3));
    }
}
