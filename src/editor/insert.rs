use crate::config::OPTIONS;
use crate::config::keymap::Key;
use crate::core::buffer::Cursor;
use crate::core::text::{
    CharClass, cell_at_col, char_class, col_at_cell, gr_index_at_col, line_content, line_graphemes,
    line_indent, line_len,
};

use super::{Editor, Mode};

pub fn handle_key(ed: &mut Editor, key: Key) {
    match key {
        Key::Esc | Key::Ctrl('c') | Key::Ctrl('[') => leave_insert(ed),
        Key::Char(c) => insert_text(ed, &c.to_string()),
        Key::Enter => {
            let indent = line_indent(&ed.buffer.rope, ed.cursor.line);
            insert_text(ed, &format!("\n{indent}"));
        }
        Key::Tab => {
            if OPTIONS.expandtab {
                let grs = line_graphemes(
                    &line_content(&ed.buffer.rope, ed.cursor.line),
                    OPTIONS.tabstop,
                );
                let cell = cell_at_col(&grs, ed.cursor.col);
                let n = OPTIONS.shiftwidth - (cell % OPTIONS.shiftwidth);
                insert_text(ed, &" ".repeat(n));
            } else {
                insert_text(ed, "\t");
            }
        }
        Key::Backspace => backspace(ed),
        Key::Delete => delete_forward(ed),
        Key::Ctrl('w') => delete_word_back(ed),
        Key::Ctrl('u') => delete_to_line_start(ed),
        Key::Left => {
            let grs = line_graphemes(
                &line_content(&ed.buffer.rope, ed.cursor.line),
                OPTIONS.tabstop,
            );
            match gr_index_at_col(&grs, ed.cursor.col) {
                Some(i) if i > 0 => ed.cursor.col = grs[i - 1].char_off,
                Some(_) => {}
                None => {
                    if let Some(g) = grs.last() {
                        ed.cursor.col = g.char_off;
                    }
                }
            }
            ed.goal = None;
        }
        Key::Right => {
            let len = line_len(&ed.buffer.rope, ed.cursor.line);
            let grs = line_graphemes(
                &line_content(&ed.buffer.rope, ed.cursor.line),
                OPTIONS.tabstop,
            );
            if let Some(i) = gr_index_at_col(&grs, ed.cursor.col) {
                ed.cursor.col = (grs[i].char_off + grs[i].chars).min(len);
            }
            ed.goal = None;
        }
        Key::Up => vertical(ed, -1),
        Key::Down => vertical(ed, 1),
        Key::Home => {
            ed.cursor.col = 0;
            ed.goal = None;
        }
        Key::End => {
            ed.cursor.col = line_len(&ed.buffer.rope, ed.cursor.line);
            ed.goal = None;
        }
        _ => {}
    }
}

fn leave_insert(ed: &mut Editor) {
    // block-change replication happens inside the same undo transaction
    ed.finish_block_change();
    // an unchanged session leaves no undo entry, but like vim any completed
    // insert — even an empty one — becomes the dot-repeat
    ed.buffer.end_change();
    ed.note_change_committed(true);
    ed.mode = Mode::Normal;
    ed.goal = None;
    // vim moves the cursor one position left when leaving insert mode
    let grs = line_graphemes(
        &line_content(&ed.buffer.rope, ed.cursor.line),
        OPTIONS.tabstop,
    );
    match gr_index_at_col(&grs, ed.cursor.col) {
        Some(i) if i > 0 => ed.cursor.col = grs[i - 1].char_off,
        Some(i) => ed.cursor.col = grs[i].char_off,
        None => ed.cursor.col = grs.last().map(|g| g.char_off).unwrap_or(0),
    }
}

fn abs(ed: &Editor) -> usize {
    ed.buffer.rope.line_to_char(ed.cursor.line) + ed.cursor.col
}

pub(crate) fn insert_text(ed: &mut Editor, text: &str) {
    let at = abs(ed);
    ed.buffer.insert(at, text);
    let chars = text.chars().count();
    if let Some(nl) = text.rfind('\n') {
        let after_nl = text[nl + 1..].chars().count();
        ed.cursor = Cursor::new(ed.cursor.line + text.matches('\n').count(), after_nl);
    } else {
        ed.cursor.col += chars;
    }
    ed.goal = None;
}

fn backspace(ed: &mut Editor) {
    if ed.cursor.col > 0 {
        let grs = line_graphemes(
            &line_content(&ed.buffer.rope, ed.cursor.line),
            OPTIONS.tabstop,
        );
        let idx = gr_index_at_col(&grs, ed.cursor.col).unwrap_or(grs.len());
        if idx == 0 {
            return;
        }
        let prev = grs[idx - 1];
        let base = ed.buffer.rope.line_to_char(ed.cursor.line);
        ed.buffer
            .remove(base + prev.char_off..base + prev.char_off + prev.chars);
        ed.cursor.col = prev.char_off;
    } else if ed.cursor.line > 0 {
        let prev_len = line_len(&ed.buffer.rope, ed.cursor.line - 1);
        let nl = ed.buffer.rope.line_to_char(ed.cursor.line) - 1;
        ed.buffer.remove(nl..nl + 1);
        ed.cursor = Cursor::new(ed.cursor.line - 1, prev_len);
    }
    ed.goal = None;
}

fn delete_forward(ed: &mut Editor) {
    let len = line_len(&ed.buffer.rope, ed.cursor.line);
    if ed.cursor.col < len {
        let grs = line_graphemes(
            &line_content(&ed.buffer.rope, ed.cursor.line),
            OPTIONS.tabstop,
        );
        if let Some(i) = gr_index_at_col(&grs, ed.cursor.col) {
            let g = grs[i];
            let base = ed.buffer.rope.line_to_char(ed.cursor.line);
            ed.buffer
                .remove(base + g.char_off..base + g.char_off + g.chars);
        }
    } else if ed.cursor.line + 1 < crate::core::text::text_lines(&ed.buffer.rope) {
        // join with the next line; the final newline is untouchable
        let at = abs(ed);
        ed.buffer.remove(at..at + 1);
    }
}

fn delete_word_back(ed: &mut Editor) {
    if ed.cursor.col == 0 {
        return;
    }
    let content = line_content(&ed.buffer.rope, ed.cursor.line);
    let chars: Vec<char> = content.chars().collect();
    let mut i = ed.cursor.col;
    while i > 0 && char_class(chars[i - 1], false) == CharClass::Blank {
        i -= 1;
    }
    if i > 0 {
        let cl = char_class(chars[i - 1], false);
        while i > 0 && char_class(chars[i - 1], false) == cl {
            i -= 1;
        }
    }
    let base = ed.buffer.rope.line_to_char(ed.cursor.line);
    ed.buffer.remove(base + i..base + ed.cursor.col);
    ed.cursor.col = i;
    ed.goal = None;
}

fn delete_to_line_start(ed: &mut Editor) {
    let indent = line_indent(&ed.buffer.rope, ed.cursor.line).chars().count();
    let start = if ed.cursor.col > indent { indent } else { 0 };
    if start >= ed.cursor.col {
        return;
    }
    let base = ed.buffer.rope.line_to_char(ed.cursor.line);
    ed.buffer.remove(base + start..base + ed.cursor.col);
    ed.cursor.col = start;
    ed.goal = None;
}

fn vertical(ed: &mut Editor, delta: isize) {
    let last = crate::core::text::text_lines(&ed.buffer.rope) - 1;
    let line = if delta < 0 {
        if ed.cursor.line == 0 {
            return;
        }
        ed.cursor.line - 1
    } else {
        if ed.cursor.line >= last {
            return;
        }
        ed.cursor.line + 1
    };
    let grs = line_graphemes(
        &line_content(&ed.buffer.rope, ed.cursor.line),
        OPTIONS.tabstop,
    );
    let goal = ed.goal.unwrap_or_else(|| cell_at_col(&grs, ed.cursor.col));
    let tgrs = line_graphemes(&line_content(&ed.buffer.rope, line), OPTIONS.tabstop);
    let len = line_len(&ed.buffer.rope, line);
    let width: usize = tgrs.iter().map(|g| g.width).sum();
    let col = if goal >= width {
        len
    } else {
        col_at_cell(&tgrs, goal)
    };
    ed.cursor = Cursor::new(line, col);
    ed.goal = Some(goal);
}
