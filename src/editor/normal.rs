use crate::config::OPTIONS;
use crate::config::keymap::{self, Key};
use crate::core::buffer::Cursor;
use crate::core::commands::{
    InsertEntry, LeaderCmd, Op, Register, ScrollCmd, SimpleCmd, Token, VisualKind, WinCmd,
};
use crate::core::motion::{self, Motion, MotionCtx, MotionKind};
use crate::core::text::{
    self, CharClass, char_class, first_non_blank, gr_index_at_col, line_content, line_graphemes,
    line_indent, line_len, max_normal_col, text_lines,
};
use crate::core::text::{cell_at_col, col_at_cell};
use crate::editor::windows::SplitDir;

use super::{Awaiting, Editor, Mode};

pub fn handle_key(ed: &mut Editor, key: Key) {
    match ed.pending.awaiting {
        Awaiting::Find(kind) => {
            ed.pending.awaiting = Awaiting::None;
            match key {
                Key::Char(ch) => process_motion(ed, Motion::Find { kind, ch }),
                _ => clear_pending(ed),
            }
        }
        Awaiting::Replace => {
            ed.pending.awaiting = Awaiting::None;
            match key {
                Key::Char(ch) => {
                    let count = ed.pending.take_count().unwrap_or(1);
                    replace_chars(ed, ch, count);
                    clear_pending(ed);
                }
                _ => clear_pending(ed),
            }
        }
        Awaiting::SetMark => {
            ed.pending.awaiting = Awaiting::None;
            if let Key::Char(ch) = key {
                ed.set_mark(ch);
            }
            clear_pending(ed);
        }
        Awaiting::JumpMark { exact } => {
            ed.pending.awaiting = Awaiting::None;
            if let Key::Char(ch) = key {
                ed.jump_mark(ch, exact);
            }
            clear_pending(ed);
        }
        Awaiting::G => {
            ed.pending.awaiting = Awaiting::None;
            match key {
                Key::Char('g') => process_motion(ed, Motion::GotoFirst),
                Key::Char('d') => {
                    ed.drop_recording();
                    ed.analyzer_definition();
                    clear_pending(ed);
                }
                Key::Char('K') => {
                    ed.drop_recording();
                    ed.analyzer_inlay_line();
                    clear_pending(ed);
                }
                Key::Char('v') => {
                    ed.drop_recording();
                    ed.reselect_visual();
                    clear_pending(ed);
                }
                Key::Char('p') => {
                    ed.drop_recording();
                    ed.toggle_preview();
                    clear_pending(ed);
                }
                Key::Char('f') => {
                    ed.drop_recording();
                    ed.goto_file();
                    clear_pending(ed);
                }
                Key::Char('c') => {
                    // gc: comment the selection in visual mode; in normal
                    // mode wait for the second c of gcc
                    if matches!(ed.mode, Mode::Visual(_)) {
                        ed.drop_recording();
                        ed.toggle_comment_visual();
                        clear_pending(ed);
                    } else {
                        ed.pending.awaiting = Awaiting::GComment;
                    }
                }
                _ => clear_pending(ed),
            }
        }
        Awaiting::GComment => {
            ed.pending.awaiting = Awaiting::None;
            match key {
                Key::Char('c') => {
                    ed.drop_recording();
                    let count = ed.pending.take_count().unwrap_or(1);
                    ed.toggle_comment_lines(count);
                    clear_pending(ed);
                }
                _ => clear_pending(ed),
            }
        }
        Awaiting::Z => {
            ed.pending.awaiting = Awaiting::None;
            match key {
                Key::Char('z') => recenter(ed, ScrollCmd::CenterCursor),
                Key::Char('t') => recenter(ed, ScrollCmd::CursorTop),
                Key::Char('b') => recenter(ed, ScrollCmd::CursorBottom),
                _ => {}
            }
            clear_pending(ed);
        }
        Awaiting::ZUpper => {
            ed.pending.awaiting = Awaiting::None;
            match key {
                Key::Char('Z') => ed.save_and_quit(true),
                Key::Char('Q') => ed.close_window_or_quit(true),
                _ => {}
            }
            clear_pending(ed);
        }
        Awaiting::Leader => {
            ed.pending.awaiting = Awaiting::None;
            match keymap::leader_token(key) {
                Some(LeaderCmd::BufferList) => {
                    ed.drop_recording();
                    super::buffer_list::open(ed);
                    clear_pending(ed);
                }
                Some(LeaderCmd::PasteCutAfter) => {
                    ed.drop_recording();
                    paste_cut(ed, true);
                    clear_pending(ed);
                }
                Some(LeaderCmd::PasteCutBefore) => {
                    ed.drop_recording();
                    paste_cut(ed, false);
                    clear_pending(ed);
                }
                Some(LeaderCmd::WindowPrefix) => {
                    ed.pending.awaiting = Awaiting::Window;
                }
                Some(LeaderCmd::CodePrefix) => ed.pending.awaiting = Awaiting::LeaderC,
                Some(LeaderCmd::RustPrefix) => ed.pending.awaiting = Awaiting::LeaderR,
                Some(LeaderCmd::DiagPrefix) => ed.pending.awaiting = Awaiting::LeaderD,
                None => clear_pending(ed),
            }
        }
        Awaiting::LeaderC => {
            ed.pending.awaiting = Awaiting::None;
            if key == Key::Char('a') {
                ed.drop_recording();
                ed.analyzer_code_actions();
            }
            clear_pending(ed);
        }
        Awaiting::LeaderR => {
            ed.pending.awaiting = Awaiting::None;
            if key == Key::Char('m') {
                ed.drop_recording();
                ed.analyzer_expand_macro();
            }
            clear_pending(ed);
        }
        Awaiting::LeaderD => {
            ed.pending.awaiting = Awaiting::None;
            if key == Key::Char('h') {
                ed.drop_recording();
                ed.toggle_ghost_text();
            }
            clear_pending(ed);
        }
        Awaiting::Window => {
            ed.pending.awaiting = Awaiting::None;
            if let Some(cmd) = keymap::window_token(key) {
                ed.drop_recording();
                window_cmd(ed, cmd);
            }
            clear_pending(ed);
        }
        Awaiting::None => dispatch(ed, key),
    }
}

fn window_cmd(ed: &mut Editor, cmd: WinCmd) {
    match cmd {
        WinCmd::Focus(dir) => ed.focus_direction(dir),
        WinCmd::FocusNext => ed.focus_next_window(),
        WinCmd::Resize(dir) => ed.resize_window(dir),
        WinCmd::Equalize => ed.equalize_windows(),
        WinCmd::Rotate => ed.rotate_windows(),
        WinCmd::FlipLayout => ed.flip_layout(),
        WinCmd::ZoomToggle => ed.zoom_toggle(),
        WinCmd::SplitH => ed.split_window(SplitDir::Horizontal, false),
        WinCmd::SplitV => ed.split_window(SplitDir::Vertical, false),
        WinCmd::SplitNew => ed.split_window(SplitDir::Horizontal, true),
        WinCmd::CloseWindow => ed.close_window(false),
        WinCmd::OnlyWindow => ed.only_window(),
    }
}

fn dispatch(ed: &mut Editor, key: Key) {
    if key == Key::Esc {
        clear_pending(ed);
        ed.drop_recording();
        ed.leave_visual();
        ed.search.highlight = false; // vim-modern: Esc calms hlsearch
        return;
    }
    if key == Key::Ctrl('c') {
        clear_pending(ed);
        ed.drop_recording();
        if matches!(ed.mode, Mode::Visual(_)) {
            ed.leave_visual();
        } else {
            ed.err("Type :q! and press <Enter> to abandon all changes and exit");
        }
        return;
    }

    // count digits (0 is a motion when no count is being typed)
    if let Key::Char(d @ '0'..='9') = key {
        let slot = if ed.pending.op.is_some() {
            &mut ed.pending.count2
        } else {
            &mut ed.pending.count1
        };
        if d != '0' || slot.is_some() {
            let v = slot.unwrap_or(0);
            *slot = Some(v.saturating_mul(10) + (d as usize - '0' as usize));
            return;
        }
    }

    if ed.pending.op.is_some()
        && let Some(m) = keymap::operator_extra_motion(key)
    {
        process_motion(ed, m);
        return;
    }

    let visual = matches!(ed.mode, Mode::Visual(_));
    let token = if visual {
        keymap::visual_extra(key).or_else(|| keymap::normal_token(key))
    } else {
        keymap::normal_token(key)
    };
    let Some(token) = token else {
        clear_pending(ed);
        return;
    };

    if visual {
        match token {
            Token::Op(op) => {
                ed.drop_recording();
                visual_operate(ed, op);
                clear_pending(ed);
                return;
            }
            Token::Simple(SimpleCmd::DeleteRight) => {
                ed.drop_recording();
                visual_operate(ed, Op::Delete);
                clear_pending(ed);
                return;
            }
            Token::Simple(SimpleCmd::ToggleCase) => {
                ed.drop_recording();
                visual_toggle_case(ed);
                clear_pending(ed);
                return;
            }
            Token::Simple(SimpleCmd::PasteAfter | SimpleCmd::PasteBefore) => {
                ed.drop_recording();
                visual_paste(ed);
                clear_pending(ed);
                return;
            }
            Token::Visual(kind) => {
                ed.enter_visual(kind);
                clear_pending(ed);
                return;
            }
            Token::Hover => {
                // the machine lens: cycle sum over the selected lines
                ed.lens_cycle_sum();
                clear_pending(ed);
                return;
            }
            Token::Insert(InsertEntry::OpenBelow) => {
                // `o` swaps the selection ends in visual mode
                std::mem::swap(&mut ed.visual_anchor, &mut ed.cursor);
                ed.scroll_to_cursor();
                clear_pending(ed);
                return;
            }
            Token::Insert(InsertEntry::OpenAbove) => {
                // `O` swaps corners horizontally in a block, ends otherwise
                if matches!(ed.mode, Mode::Visual(VisualKind::Block)) {
                    std::mem::swap(&mut ed.visual_anchor.col, &mut ed.cursor.col);
                } else {
                    std::mem::swap(&mut ed.visual_anchor, &mut ed.cursor);
                }
                ed.scroll_to_cursor();
                clear_pending(ed);
                return;
            }
            Token::CmdLine => {
                // remember the selection for :s scope, then open the prompt
                let (l1, l2) = (
                    ed.visual_anchor.line.min(ed.cursor.line),
                    ed.visual_anchor.line.max(ed.cursor.line),
                );
                ed.cmd_selection = Some((l1, l2));
                ed.leave_visual();
                clear_pending(ed);
                ed.drop_recording();
                ed.cmdline.clear();
                ed.prompt = super::Prompt::Command;
                ed.mode = Mode::Command;
                return;
            }
            // motions, counts, find, scroll fall through to normal handling
            Token::Motion(_) | Token::FindStart(_) | Token::Scroll(_) | Token::PrefixG => {}
            // everything else is inert in visual mode
            _ => {
                clear_pending(ed);
                return;
            }
        }
    }

    match token {
        Token::Motion(m) => process_motion(ed, m),
        Token::Op(op) => match ed.pending.op {
            Some(p) if p == op => {
                let count = ed.pending.take_count().unwrap_or(1);
                ed.pending.op = None;
                linewise_op(ed, op, ed.cursor.line, ed.cursor.line + count - 1);
                clear_pending(ed);
            }
            Some(_) => clear_pending(ed),
            None => ed.pending.op = Some(op),
        },
        Token::Insert(entry) => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                ed.pending.take_count();
                enter_insert(ed, entry);
            }
        }
        Token::Simple(cmd) => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                simple(ed, cmd);
                clear_pending(ed);
            }
        }
        Token::Scroll(s) => {
            scroll(ed, s);
            clear_pending(ed);
        }
        Token::FindStart(kind) => ed.pending.awaiting = Awaiting::Find(kind),
        Token::ReplaceStart => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                ed.pending.awaiting = Awaiting::Replace;
            }
        }
        Token::SetMark => ed.pending.awaiting = Awaiting::SetMark,
        Token::JumpMark { exact } => ed.pending.awaiting = Awaiting::JumpMark { exact },
        Token::Leader => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                ed.pending.awaiting = Awaiting::Leader;
            }
        }
        Token::PrefixWindow => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                ed.pending.awaiting = Awaiting::Window;
            }
        }
        Token::AlternateBuffer => {
            clear_pending(ed);
            ed.drop_recording();
            ed.switch_alternate();
        }
        Token::FilePicker => {
            clear_pending(ed);
            ed.drop_recording();
            super::file_picker::open(ed);
        }
        Token::Hover => {
            clear_pending(ed);
            ed.drop_recording();
            ed.hover();
        }
        Token::PrefixG => ed.pending.awaiting = Awaiting::G,
        Token::PrefixZ => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                ed.pending.awaiting = Awaiting::Z;
            }
        }
        Token::PrefixZUpper => {
            if ed.pending.op.is_some() {
                clear_pending(ed);
            } else {
                ed.pending.awaiting = Awaiting::ZUpper;
            }
        }
        Token::Visual(kind) => {
            clear_pending(ed);
            ed.drop_recording();
            ed.enter_visual(kind);
        }
        Token::CmdLine => {
            clear_pending(ed);
            ed.drop_recording();
            ed.cmdline.clear();
            ed.cmd_selection = None;
            ed.prompt = super::Prompt::Command;
            ed.mode = Mode::Command;
        }
        Token::SearchPrompt(forward) => {
            clear_pending(ed);
            ed.drop_recording();
            ed.cmdline.clear();
            ed.prompt = super::Prompt::Search { forward };
            ed.search_origin = Some((ed.cursor, ed.top_line));
            ed.mode = Mode::Command;
        }
    }
}

fn clear_pending(ed: &mut Editor) {
    ed.pending = Default::default();
}

fn process_motion(ed: &mut Editor, motion: Motion) {
    let count = ed.pending.take_count();
    let op = ed.pending.op.take();

    // plain jumps (not operator targets) land on the jumplist, like vim
    if op.is_none()
        && matches!(
            motion,
            Motion::GotoFirst
                | Motion::GotoLast
                | Motion::ParaForward
                | Motion::ParaBack
                | Motion::MatchPair
        )
    {
        ed.record_jump();
    }

    if let Some(op) = op {
        apply_operator(ed, op, motion, count);
    } else {
        let ctx = MotionCtx {
            rope: &ed.buffer.rope,
            goal: ed.goal,
            last_find: ed.last_find,
            tabstop: OPTIONS.tabstop,
            for_operator: false,
        };
        if let Some(out) = motion::resolve(motion, count, ed.cursor, &ctx) {
            ed.cursor = out.cursor;
            ed.goal = out.new_goal;
            if let Some(lf) = out.new_last_find {
                ed.last_find = Some(lf);
            }
        }
    }
    clear_pending(ed);
}

fn abs_of(ed: &Editor, cursor: Cursor) -> usize {
    ed.buffer.rope.line_to_char(cursor.line) + cursor.col
}

/// Char range covered by an inclusive target: extends past the grapheme
/// cluster under the target position.
fn inclusive_end(ed: &Editor, target: Cursor) -> usize {
    let grs = line_graphemes(&line_content(&ed.buffer.rope, target.line), OPTIONS.tabstop);
    let base = ed.buffer.rope.line_to_char(target.line);
    match gr_index_at_col(&grs, target.col) {
        Some(i) => base + grs[i].char_off + grs[i].chars,
        None => base + target.col,
    }
}

fn apply_operator(ed: &mut Editor, op: Op, motion: Motion, count: Option<usize>) {
    // vim's special case: `cw` on a non-blank acts like `ce`
    let motion = match (op, motion) {
        (Op::Change, Motion::WordForward { big }) => {
            let len = line_len(&ed.buffer.rope, ed.cursor.line);
            if ed.cursor.col < len {
                let ch = ed.buffer.rope.char(abs_of(ed, ed.cursor));
                if char_class(ch, false) != CharClass::Blank {
                    Motion::WordEnd { big }
                } else {
                    motion
                }
            } else {
                motion
            }
        }
        _ => motion,
    };

    let ctx = MotionCtx {
        rope: &ed.buffer.rope,
        goal: ed.goal,
        last_find: ed.last_find,
        tabstop: OPTIONS.tabstop,
        for_operator: true,
    };
    let Some(out) = motion::resolve(motion, count, ed.cursor, &ctx) else {
        return;
    };
    if let Some(lf) = out.new_last_find {
        ed.last_find = Some(lf);
    }
    let mut target = out.cursor;

    if out.kind == MotionKind::Linewise {
        let (l1, l2) = if target.line < ed.cursor.line {
            (target.line, ed.cursor.line)
        } else {
            (ed.cursor.line, target.line)
        };
        linewise_op(ed, op, l1, l2);
        return;
    }

    // vim's word-motion adjustment (:h word, nv_wordcmd): a w-family target
    // landing in column 0 of a later line backs up to the end of the previous
    // line; an empty previous line is consumed including its newline.
    let mut word_adjusted_end = None;
    if matches!(motion, Motion::WordForward { .. })
        && target.line > ed.cursor.line
        && target.col == 0
    {
        let prev = target.line - 1;
        let plen = line_len(&ed.buffer.rope, prev);
        let base = ed.buffer.rope.line_to_char(prev);
        word_adjusted_end = Some(base + if plen > 0 { plen } else { 1 });
        target = Cursor::new(prev, plen);
    }

    let a_cur = abs_of(ed, ed.cursor);
    let a_tgt = abs_of(ed, target);
    let (mut start, mut end) = if a_cur <= a_tgt {
        let end = if out.kind == MotionKind::Inclusive {
            inclusive_end(ed, target)
        } else {
            a_tgt
        };
        (a_cur, end)
    } else {
        let end = if out.kind == MotionKind::Inclusive {
            inclusive_end(ed, ed.cursor)
        } else {
            a_cur
        };
        (a_tgt, end)
    };
    if let Some(e) = word_adjusted_end {
        end = e;
    } else if out.kind == MotionKind::Exclusive {
        // vim's general exclusive adjustment (:h exclusive): an exclusive
        // motion ending in column 0 below its start ends at the last character
        // of the previous line instead — and becomes linewise when the start
        // sits within its line's indent.
        let (s_cur, e_cur) = if a_cur <= a_tgt {
            (ed.cursor, target)
        } else {
            (target, ed.cursor)
        };
        if e_cur.col == 0 && e_cur.line > s_cur.line {
            let prev = e_cur.line - 1;
            let indent = line_indent(&ed.buffer.rope, s_cur.line).chars().count();
            if s_cur.col <= indent {
                linewise_op(ed, op, s_cur.line, prev);
                return;
            }
            let plen = line_len(&ed.buffer.rope, prev);
            end = ed.buffer.rope.line_to_char(prev) + plen;
            start = start.min(end);
        }
    }
    if start >= end {
        if op == Op::Change {
            // change with an empty range still enters insert mode (e.g. c0 at col 0)
            change_range(ed, start, start);
        }
        return;
    }
    charwise_op(ed, op, start, end);
}

fn charwise_op(ed: &mut Editor, op: Op, start: usize, end: usize) {
    let text = ed.buffer.rope.slice(start..end).to_string();
    if op == Op::Yank {
        ed.mirror_yank_to_clipboard(&text);
        ed.yank_reg = Some(Register::Char(text));
    } else {
        ed.cut_reg = Some(Register::Char(text));
    }
    ed.goal = None;
    match op {
        Op::Yank => {
            ed.start_yank_flash(crate::editor::FlashRegion::Char(start, end));
            let cur = motion_cursor_of_abs(ed, start);
            if abs_of(ed, ed.cursor) > start {
                ed.cursor = cur;
            }
        }
        Op::Delete => {
            ed.buffer.begin_change(ed.cursor);
            ed.buffer.remove(start..end);
            ed.cursor = motion_cursor_of_abs(ed, start);
            let committed = ed.buffer.end_change();
            ed.note_change_committed(committed);
        }
        Op::Change => change_range(ed, start, end),
    }
}

fn change_range(ed: &mut Editor, start: usize, end: usize) {
    // undo puts the cursor at the start of the changed text
    let s_line = ed
        .buffer
        .rope
        .char_to_line(start.min(ed.buffer.rope.len_chars()))
        .min(text_lines(&ed.buffer.rope) - 1);
    let s_col = start - ed.buffer.rope.line_to_char(s_line);
    ed.buffer.begin_change(Cursor::new(s_line, s_col));
    if end > start {
        ed.buffer.remove(start..end);
    }
    let line = ed
        .buffer
        .rope
        .char_to_line(start.min(ed.buffer.rope.len_chars()));
    let line = line.min(text_lines(&ed.buffer.rope) - 1);
    let col = start - ed.buffer.rope.line_to_char(line);
    ed.cursor = Cursor::new(line, col);
    ed.goal = None;
    ed.mode = Mode::Insert;
}

fn linewise_op(ed: &mut Editor, op: Op, l1: usize, l2: usize) {
    let rope = &ed.buffer.rope;
    let last = text_lines(rope) - 1;
    let (l1, l2) = (l1.min(last), l2.min(last));
    let start = rope.line_to_char(l1);
    // every line carries its newline terminator (buffer invariant)
    let end = if l2 + 1 >= rope.len_lines() {
        rope.len_chars()
    } else {
        rope.line_to_char(l2 + 1)
    };
    let mut text = rope.slice(start..end).to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if op == Op::Yank {
        ed.mirror_yank_to_clipboard(&text);
        ed.yank_reg = Some(Register::Line(text));
    } else {
        ed.cut_reg = Some(Register::Line(text));
    }
    ed.goal = None;

    match op {
        Op::Yank => {
            ed.start_yank_flash(crate::editor::FlashRegion::Line(l1, l2));
            if l1 < ed.cursor.line {
                ed.cursor = Cursor::new(l1, ed.cursor.col);
            }
        }
        Op::Delete => {
            ed.buffer.begin_change(ed.cursor);
            ed.buffer.remove(start..end);
            if ed.buffer.rope.len_chars() == 0 {
                ed.buffer.insert(0, "\n"); // deleting every line leaves one empty line
            }
            let new_last = text_lines(&ed.buffer.rope) - 1;
            let line = l1.min(new_last);
            ed.cursor = Cursor::new(line, first_non_blank(&ed.buffer.rope, line));
            let committed = ed.buffer.end_change();
            ed.note_change_committed(committed);
        }
        Op::Change => {
            let indent = line_indent(&ed.buffer.rope, l1);
            // undo puts the cursor where the change began: at the kept indent
            ed.buffer
                .begin_change(Cursor::new(l1, indent.chars().count()));
            ed.buffer.remove(start..end);
            ed.buffer.insert(start, &format!("{indent}\n"));
            ed.cursor = Cursor::new(l1, indent.chars().count());
            ed.mode = Mode::Insert;
        }
    }
}

fn motion_cursor_of_abs(ed: &Editor, abs: usize) -> Cursor {
    let rope = &ed.buffer.rope;
    let last = text_lines(rope) - 1;
    if rope.len_chars() == 0 {
        return Cursor::default();
    }
    let line = rope.char_to_line(abs.min(rope.len_chars())).min(last);
    let col = abs.saturating_sub(rope.line_to_char(line));
    let col = col.min(max_normal_col(rope, line, OPTIONS.tabstop));
    let grs = line_graphemes(&line_content(rope, line), OPTIONS.tabstop);
    Cursor::new(line, text::snap_to_grapheme(&grs, col))
}

pub(crate) fn enter_insert(ed: &mut Editor, entry: InsertEntry) {
    ed.goal = None;
    let rope = &ed.buffer.rope;
    let line = ed.cursor.line;
    match entry {
        // for the plain entries the undo snapshot records the position where
        // the insert begins, so `u` returns there (clamped)
        InsertEntry::Before => ed.buffer.begin_change(ed.cursor),
        InsertEntry::FirstNonBlank => {
            let content = line_content(rope, line);
            // vim's I on a whitespace-only line inserts after all the blanks
            ed.cursor.col = if content.chars().all(char::is_whitespace) {
                line_len(rope, line)
            } else {
                first_non_blank(rope, line)
            };
            ed.buffer.begin_change(ed.cursor);
        }
        InsertEntry::After => {
            let grs = line_graphemes(&line_content(rope, line), OPTIONS.tabstop);
            ed.cursor.col = match gr_index_at_col(&grs, ed.cursor.col) {
                Some(i) => grs[i].char_off + grs[i].chars,
                None => line_len(rope, line),
            };
            ed.buffer.begin_change(ed.cursor);
        }
        InsertEntry::LineEnd => {
            ed.cursor.col = line_len(rope, line);
            ed.buffer.begin_change(ed.cursor);
        }
        // undoing o/O puts the cursor back on the original line
        InsertEntry::OpenBelow => {
            let indent = line_indent(rope, line);
            let at = if line + 1 >= rope.len_lines() {
                rope.len_chars()
            } else {
                rope.line_to_char(line + 1)
            };
            ed.buffer
                .begin_change(Cursor::new(line, first_non_blank(rope, line)));
            ed.buffer.insert(at, &format!("{indent}\n"));
            ed.cursor = Cursor::new(line + 1, indent.chars().count());
        }
        InsertEntry::OpenAbove => {
            let indent = line_indent(rope, line);
            let at = rope.line_to_char(line);
            ed.buffer
                .begin_change(Cursor::new(line, first_non_blank(rope, line)));
            ed.buffer.insert(at, &format!("{indent}\n"));
            ed.cursor = Cursor::new(line, indent.chars().count());
        }
    }
    ed.mode = Mode::Insert;
}

fn simple(ed: &mut Editor, cmd: SimpleCmd) {
    let count_given = ed.pending.take_count();
    let count = count_given.unwrap_or(1);
    match cmd {
        SimpleCmd::DeleteRight => delete_graphemes(ed, count, true),
        SimpleCmd::DeleteLeft => delete_graphemes(ed, count, false),
        SimpleCmd::ToggleCase => toggle_case(ed, count),
        SimpleCmd::Join => join_lines(ed, count),
        SimpleCmd::PasteAfter => paste(ed, true, count),
        SimpleCmd::PasteBefore => paste(ed, false, count),
        SimpleCmd::Undo => {
            for _ in 0..count {
                match ed.buffer.undo(ed.cursor) {
                    Some(cur) => ed.cursor = cur,
                    None => {
                        ed.err("Already at oldest change");
                        break;
                    }
                }
            }
            ed.goal = None;
        }
        SimpleCmd::Redo => {
            for _ in 0..count {
                match ed.buffer.redo(ed.cursor) {
                    Some(cur) => ed.cursor = cur,
                    None => {
                        ed.err("Already at newest change");
                        break;
                    }
                }
            }
            ed.goal = None;
        }
        SimpleCmd::Repeat => ed.repeat_last_change(count_given),
        SimpleCmd::JumpBack => ed.jump_back(),
        SimpleCmd::JumpForward => ed.jump_forward(),
        SimpleCmd::NextMatch => ed.search_step(true),
        SimpleCmd::PrevMatch => ed.search_step(false),
        SimpleCmd::SearchWord => ed.search_word_under_cursor(),
        SimpleCmd::DeleteToEol => apply_operator(ed, Op::Delete, Motion::LineEnd, Some(count)),
        SimpleCmd::ChangeToEol => apply_operator(ed, Op::Change, Motion::LineEnd, Some(count)),
        SimpleCmd::YankToEol => apply_operator(ed, Op::Yank, Motion::LineEnd, Some(count)),
        SimpleCmd::SubstChar => {
            let len = line_len(&ed.buffer.rope, ed.cursor.line);
            let grs = line_graphemes(
                &line_content(&ed.buffer.rope, ed.cursor.line),
                OPTIONS.tabstop,
            );
            let base = ed.buffer.rope.line_to_char(ed.cursor.line);
            let start = base + ed.cursor.col;
            let end = match gr_index_at_col(&grs, ed.cursor.col) {
                Some(i) => {
                    let e = grs[(i + count - 1).min(grs.len() - 1)];
                    base + e.char_off + e.chars
                }
                None => base + len,
            };
            let text = ed.buffer.rope.slice(start..end).to_string();
            if !text.is_empty() {
                ed.cut_reg = Some(Register::Char(text));
            }
            change_range(ed, start, end);
        }
        SimpleCmd::SubstLine => {
            linewise_op(ed, Op::Change, ed.cursor.line, ed.cursor.line + count - 1)
        }
    }
}

fn delete_graphemes(ed: &mut Editor, count: usize, forward: bool) {
    let rope = &ed.buffer.rope;
    let grs = line_graphemes(&line_content(rope, ed.cursor.line), OPTIONS.tabstop);
    if grs.is_empty() {
        return;
    }
    let base = rope.line_to_char(ed.cursor.line);
    let Some(idx) = gr_index_at_col(&grs, ed.cursor.col) else {
        return;
    };
    let (start, end) = if forward {
        let last = (idx + count - 1).min(grs.len() - 1);
        (
            base + grs[idx].char_off,
            base + grs[last].char_off + grs[last].chars,
        )
    } else {
        if idx == 0 {
            return;
        }
        let first = idx.saturating_sub(count);
        (base + grs[first].char_off, base + grs[idx].char_off)
    };
    if start >= end {
        return;
    }
    let text = ed.buffer.rope.slice(start..end).to_string();
    ed.cut_reg = Some(Register::Char(text));
    ed.buffer.begin_change(motion_cursor_of_abs(ed, start));
    ed.buffer.remove(start..end);
    ed.cursor = motion_cursor_of_abs(ed, start);
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
    ed.goal = None;
}

fn replace_chars(ed: &mut Editor, ch: char, count: usize) {
    if ch == '\n' {
        return;
    }
    let rope = &ed.buffer.rope;
    let grs = line_graphemes(&line_content(rope, ed.cursor.line), OPTIONS.tabstop);
    let Some(idx) = gr_index_at_col(&grs, ed.cursor.col) else {
        return;
    };
    if idx + count > grs.len() {
        return; // not enough characters to replace — vim aborts
    }
    let base = rope.line_to_char(ed.cursor.line);
    let start = base + grs[idx].char_off;
    let last = grs[idx + count - 1];
    let end = base + last.char_off + last.chars;
    ed.buffer.begin_change(ed.cursor);
    ed.buffer.remove(start..end);
    let replacement: String = std::iter::repeat_n(ch, count).collect();
    ed.buffer.insert(start, &replacement);
    ed.cursor = Cursor::new(ed.cursor.line, grs[idx].char_off + count - 1);
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
    ed.goal = None;
}

fn toggle_case(ed: &mut Editor, count: usize) {
    let rope = &ed.buffer.rope;
    let grs = line_graphemes(&line_content(rope, ed.cursor.line), OPTIONS.tabstop);
    let Some(idx) = gr_index_at_col(&grs, ed.cursor.col) else {
        return;
    };
    let base = rope.line_to_char(ed.cursor.line);
    let last = grs[(idx + count - 1).min(grs.len() - 1)];
    let start = base + grs[idx].char_off;
    let end = base + last.char_off + last.chars;
    // like vim, ~ never changes the character count, so multi-char case
    // mappings (ß→SS and friends) are replaced one-to-one or left alone
    let toggled: String = rope
        .slice(start..end)
        .chars()
        .map(|c| {
            let mapped: Vec<char> = if c.is_lowercase() {
                c.to_uppercase().collect()
            } else if c.is_uppercase() {
                c.to_lowercase().collect()
            } else {
                return c;
            };
            match mapped[..] {
                [single] => single,
                _ if c == 'ß' => 'ẞ',
                _ => c,
            }
        })
        .collect();
    ed.buffer.begin_change(ed.cursor);
    ed.buffer.remove(start..end);
    ed.buffer.insert(start, &toggled);
    ed.cursor = motion_cursor_of_abs(
        ed,
        end.min(base + line_len(&ed.buffer.rope, ed.cursor.line)),
    );
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
    ed.goal = None;
}

fn join_lines(ed: &mut Editor, count: usize) {
    let joins = count.max(2) - 1;
    ed.buffer.begin_change(ed.cursor);
    for _ in 0..joins {
        let rope = &ed.buffer.rope;
        let line = ed.cursor.line;
        if line + 1 >= text_lines(rope) {
            break;
        }
        let cur_len = line_len(rope, line);
        let nl = rope.line_to_char(line) + cur_len;
        let next_content = line_content(rope, line + 1);
        let lead_blanks = next_content
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let trimmed_empty = next_content.chars().count() == lead_blanks;
        ed.buffer.remove(nl..nl + 1 + lead_blanks);
        let needs_space = cur_len > 0
            && !trimmed_empty
            && !line_content(&ed.buffer.rope, line).ends_with([' ', '\t']);
        if needs_space {
            ed.buffer.insert(nl, " ");
        }
        ed.cursor = Cursor::new(line, cur_len);
    }
    ed.clamp_cursor();
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
    ed.goal = None;
}

fn paste(ed: &mut Editor, after: bool, count: usize) {
    let Some(reg) = ed.yank_reg.clone() else {
        return;
    };
    paste_register(ed, reg, after, count);
}

/// `<leader>p` / `<leader>P` — put the CUT register (dd+p idiom lives here).
pub(crate) fn paste_cut(ed: &mut Editor, after: bool) {
    let Some(reg) = ed.cut_reg.clone() else {
        ed.msg("cut register is empty");
        return;
    };
    paste_register(ed, reg, after, 1);
}

fn paste_register(ed: &mut Editor, reg: Register, after: bool, count: usize) {
    ed.goal = None;
    ed.buffer.begin_change(ed.cursor);
    match reg {
        Register::Line(text) => {
            let text = text.repeat(count);
            let rope = &ed.buffer.rope;
            let last = text_lines(rope) - 1;
            let line = if after {
                ed.cursor.line + 1
            } else {
                ed.cursor.line
            };
            let at = if line > last {
                rope.len_chars()
            } else {
                rope.line_to_char(line)
            };
            ed.buffer.insert(at, &text);
            let line = line.min(text_lines(&ed.buffer.rope) - 1);
            ed.cursor = Cursor::new(line, first_non_blank(&ed.buffer.rope, line));
        }
        Register::Char(text) => {
            let text = text.repeat(count);
            let rope = &ed.buffer.rope;
            let base = rope.line_to_char(ed.cursor.line);
            let grs = line_graphemes(&line_content(rope, ed.cursor.line), OPTIONS.tabstop);
            let col = if after {
                match gr_index_at_col(&grs, ed.cursor.col) {
                    Some(i) => grs[i].char_off + grs[i].chars,
                    None => line_len(rope, ed.cursor.line),
                }
            } else {
                ed.cursor.col
            };
            let at = base + col;
            ed.buffer.insert(at, &text);
            let end = at + text.chars().count();
            ed.cursor = motion_cursor_of_abs(ed, end.saturating_sub(1));
        }
        Register::Block(segments) => {
            block_paste(ed, &segments, after);
        }
    }
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
}

/// Blockwise put: each segment lands on a successive line at the cursor's
/// display column (short lines are space-padded, missing lines created).
fn block_paste(ed: &mut Editor, segments: &[String], after: bool) {
    let cur_grs = line_graphemes(
        &line_content(&ed.buffer.rope, ed.cursor.line),
        OPTIONS.tabstop,
    );
    let cell = if after {
        cell_at_col(&cur_grs, ed.cursor.col)
            + cur_grs
                .iter()
                .find(|g| g.char_off == ed.cursor.col)
                .map(|g| g.width)
                .unwrap_or(0)
    } else {
        cell_at_col(&cur_grs, ed.cursor.col)
    };
    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        let line = ed.cursor.line + i;
        while line >= text_lines(&ed.buffer.rope) {
            let at = ed.buffer.rope.len_chars();
            ed.buffer.insert(at, "\n");
        }
        let content = line_content(&ed.buffer.rope, line);
        let grs = line_graphemes(&content, OPTIONS.tabstop);
        let width: usize = grs.iter().map(|g| g.width).sum();
        if width < cell {
            // pad to the block column
            let at = ed.buffer.rope.line_to_char(line) + line_len(&ed.buffer.rope, line);
            ed.buffer.insert(at, &" ".repeat(cell - width));
        }
        let content = line_content(&ed.buffer.rope, line);
        let grs = line_graphemes(&content, OPTIONS.tabstop);
        let col = char_col_for_cell(&grs, cell, &content);
        let at = ed.buffer.rope.line_to_char(line) + col;
        ed.buffer.insert(at, segment);
    }
}

/// Char column whose grapheme starts at or after `cell` (line end when past).
fn char_col_for_cell(grs: &[crate::core::text::Gr], cell: usize, content: &str) -> usize {
    for g in grs {
        if g.cell >= cell {
            return g.char_off;
        }
    }
    content.chars().count()
}

// ----------------------------------------------------------------------
// Visual-mode operators
// ----------------------------------------------------------------------

/// The block rectangle of the current selection: lines and cell columns.
fn visual_block_rect(ed: &Editor) -> (usize, usize, usize, usize) {
    let (a, c) = (ed.visual_anchor, ed.cursor);
    let (l1, l2) = (a.line.min(c.line), a.line.max(c.line));
    let cell_of = |cur: Cursor| {
        let grs = line_graphemes(&line_content(&ed.buffer.rope, cur.line), OPTIONS.tabstop);
        cell_at_col(&grs, cur.col)
    };
    let (ca, cc) = (cell_of(a), cell_of(c));
    (l1, l2, ca.min(cc), ca.max(cc))
}

/// Per-line char range of the block on `line`; None when the line is shorter
/// than the block's left edge.
fn block_segment(ed: &Editor, line: usize, left: usize, right: usize) -> Option<(usize, usize)> {
    let content = line_content(&ed.buffer.rope, line);
    let grs = line_graphemes(&content, OPTIONS.tabstop);
    let width: usize = grs.iter().map(|g| g.width).sum();
    if width <= left {
        return None;
    }
    let start = col_at_cell(&grs, left);
    // right edge inclusive: the whole grapheme covering `right`
    let end = grs
        .iter()
        .find(|g| g.cell <= right && right < g.cell + g.width)
        .map(|g| g.char_off + g.chars)
        .unwrap_or_else(|| content.chars().count());
    (end > start).then_some((start, end))
}

fn visual_operate(ed: &mut Editor, op: Op) {
    let Mode::Visual(kind) = ed.mode else { return };
    ed.leave_visual();
    match kind {
        VisualKind::Char => {
            let (a, c) = (ed.visual_anchor, ed.cursor);
            let (first, last) = if abs_of(ed, a) <= abs_of(ed, c) {
                (a, c)
            } else {
                (c, a)
            };
            let start = abs_of(ed, first);
            let end = inclusive_end(ed, last);
            if start >= end {
                if op == Op::Change {
                    change_range(ed, start, start);
                }
            } else {
                charwise_op(ed, op, start, end);
            }
        }
        VisualKind::Line => {
            let (l1, l2) = (
                ed.visual_anchor.line.min(ed.cursor.line),
                ed.visual_anchor.line.max(ed.cursor.line),
            );
            linewise_op(ed, op, l1, l2);
        }
        VisualKind::Block => visual_block_operate(ed, op),
    }
}

fn visual_block_operate(ed: &mut Editor, op: Op) {
    let (l1, l2, left, right) = visual_block_rect(ed);
    let mut segments = Vec::new();
    for line in l1..=l2 {
        let text = block_segment(ed, line, left, right)
            .map(|(s, e)| {
                let content = line_content(&ed.buffer.rope, line);
                content.chars().skip(s).take(e - s).collect::<String>()
            })
            .unwrap_or_default();
        segments.push(text);
    }
    if op == Op::Yank {
        ed.mirror_yank_to_clipboard(&segments.join("\n"));
        ed.yank_reg = Some(Register::Block(segments));
    } else {
        ed.cut_reg = Some(Register::Block(segments));
    }
    ed.goal = None;

    let top_left_col = block_segment(ed, l1, left, right)
        .map(|(s, _)| s)
        .unwrap_or(0);
    match op {
        Op::Yank => {
            ed.start_yank_flash(crate::editor::FlashRegion::Block(l1, l2, left, right));
            ed.cursor = Cursor::new(l1, top_left_col);
        }
        Op::Delete | Op::Change => {
            ed.buffer.begin_change(Cursor::new(l1, top_left_col));
            for line in l1..=l2 {
                if let Some((s, e)) = block_segment(ed, line, left, right) {
                    let base = ed.buffer.rope.line_to_char(line);
                    ed.buffer.remove(base + s..base + e);
                }
            }
            ed.cursor = Cursor::new(l1, top_left_col);
            if op == Op::Delete {
                let committed = ed.buffer.end_change();
                ed.note_change_committed(committed);
            } else {
                // insert on the top line; Esc replicates to the block lines
                ed.set_block_change(crate::editor::BlockChange {
                    top_line: l1,
                    lines: (l1 + 1..=l2).collect(),
                    col: top_left_col,
                });
                ed.goal = None;
                ed.mode = Mode::Insert;
            }
        }
    }
    ed.clamp_cursor();
}

fn visual_toggle_case(ed: &mut Editor) {
    let Mode::Visual(kind) = ed.mode else { return };
    ed.leave_visual();
    ed.buffer.begin_change(ed.cursor);
    match kind {
        VisualKind::Char => {
            let (a, c) = (ed.visual_anchor, ed.cursor);
            let (first, last) = if abs_of(ed, a) <= abs_of(ed, c) {
                (a, c)
            } else {
                (c, a)
            };
            let start = abs_of(ed, first);
            let end = inclusive_end(ed, last);
            toggle_case_range(ed, start, end);
            ed.cursor = first;
        }
        VisualKind::Line => {
            let (l1, l2) = (
                ed.visual_anchor.line.min(ed.cursor.line),
                ed.visual_anchor.line.max(ed.cursor.line),
            );
            let start = ed.buffer.rope.line_to_char(l1);
            let end = ed.buffer.rope.line_to_char(l2) + line_len(&ed.buffer.rope, l2);
            toggle_case_range(ed, start, end);
            ed.cursor = Cursor::new(l1, 0);
        }
        VisualKind::Block => {
            let (l1, l2, left, right) = visual_block_rect(ed);
            for line in l1..=l2 {
                if let Some((s, e)) = block_segment(ed, line, left, right) {
                    let base = ed.buffer.rope.line_to_char(line);
                    toggle_case_range(ed, base + s, base + e);
                }
            }
            ed.cursor = Cursor::new(l1, 0);
        }
    }
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
    ed.clamp_cursor();
}

fn toggle_case_range(ed: &mut Editor, start: usize, end: usize) {
    if start >= end {
        return;
    }
    let toggled: String = ed
        .buffer
        .rope
        .slice(start..end)
        .chars()
        .map(|c| {
            let mapped: Vec<char> = if c.is_lowercase() {
                c.to_uppercase().collect()
            } else if c.is_uppercase() {
                c.to_lowercase().collect()
            } else {
                return c;
            };
            match mapped[..] {
                [single] => single,
                _ if c == 'ß' => 'ẞ',
                _ => c,
            }
        })
        .collect();
    ed.buffer.remove(start..end);
    ed.buffer.insert(start, &toggled);
}

/// Visual paste: replaces the selection with the register WITHOUT the
/// replaced text touching the register — the author's explicit preference
/// (paste the same content in several places).
fn visual_paste(ed: &mut Editor) {
    let Some(reg) = ed.yank_reg.clone() else {
        ed.leave_visual();
        return;
    };
    visual_paste_with(ed, reg);
}

/// Replaces the selection with `reg` — never touching any register (the
/// replaced text is simply gone; the author's multi-paste workflow).
pub(crate) fn visual_paste_with(ed: &mut Editor, reg: Register) {
    let Mode::Visual(kind) = ed.mode else { return };
    ed.leave_visual();
    ed.goal = None;
    ed.buffer.begin_change(ed.cursor);
    match kind {
        VisualKind::Char => {
            let (a, c) = (ed.visual_anchor, ed.cursor);
            let (first, last) = if abs_of(ed, a) <= abs_of(ed, c) {
                (a, c)
            } else {
                (c, a)
            };
            let start = abs_of(ed, first);
            let end = inclusive_end(ed, last);
            if end > start {
                ed.buffer.remove(start..end);
            }
            match reg {
                Register::Char(text) => {
                    ed.buffer.insert(start, &text);
                    let end = start + text.chars().count();
                    ed.cursor = motion_cursor_of_abs(ed, end.saturating_sub(1));
                }
                Register::Line(text) => {
                    // whole lines land on their own line at the gap
                    ed.buffer.insert(start, &format!("\n{text}"));
                    ed.cursor = motion_cursor_of_abs(ed, start + 1);
                }
                Register::Block(segments) => {
                    ed.cursor = motion_cursor_of_abs(ed, start);
                    block_paste(ed, &segments, false);
                }
            }
        }
        VisualKind::Line => {
            let (l1, l2) = (
                ed.visual_anchor.line.min(ed.cursor.line),
                ed.visual_anchor.line.max(ed.cursor.line),
            );
            let rope = &ed.buffer.rope;
            let start = rope.line_to_char(l1);
            let end = if l2 + 1 >= rope.len_lines() {
                rope.len_chars()
            } else {
                rope.line_to_char(l2 + 1)
            };
            ed.buffer.remove(start..end);
            let text = match reg {
                Register::Line(text) => text,
                Register::Char(text) => format!("{text}\n"),
                Register::Block(segments) => segments.iter().fold(String::new(), |mut acc, s| {
                    acc.push_str(s);
                    acc.push('\n');
                    acc
                }),
            };
            ed.buffer.insert(start, &text);
            if ed.buffer.rope.len_chars() == 0 {
                ed.buffer.insert(0, "\n");
            }
            let line = l1.min(text_lines(&ed.buffer.rope) - 1);
            ed.cursor = Cursor::new(line, first_non_blank(&ed.buffer.rope, line));
        }
        VisualKind::Block => {
            let (l1, l2, left, right) = visual_block_rect(ed);
            let top_left = block_segment(ed, l1, left, right)
                .map(|(s, _)| s)
                .unwrap_or(0);
            for line in l1..=l2 {
                if let Some((s, e)) = block_segment(ed, line, left, right) {
                    let base = ed.buffer.rope.line_to_char(line);
                    ed.buffer.remove(base + s..base + e);
                }
            }
            ed.cursor = Cursor::new(l1, top_left);
            match reg {
                Register::Block(segments) => block_paste(ed, &segments, false),
                Register::Char(text) => {
                    let at = abs_of(ed, ed.cursor);
                    ed.buffer.insert(at, &text);
                }
                Register::Line(text) => {
                    let at = ed.buffer.rope.line_to_char(l1);
                    ed.buffer.insert(at, &text);
                }
            }
        }
    }
    let committed = ed.buffer.end_change();
    ed.note_change_committed(committed);
    ed.clamp_cursor();
    ed.scroll_to_cursor();
}

fn scroll(ed: &mut Editor, cmd: ScrollCmd) {
    let h = ed.view.height;
    match cmd {
        ScrollCmd::HalfDown => {
            let n = (h / 2).max(1);
            ed.top_line += n;
            ed.move_vertical(n as isize);
        }
        ScrollCmd::HalfUp => {
            let n = (h / 2).max(1);
            ed.top_line = ed.top_line.saturating_sub(n);
            ed.move_vertical(-(n as isize));
        }
        ScrollCmd::PageDown => {
            let n = h.saturating_sub(2).max(1);
            ed.top_line += n;
            ed.move_vertical(n as isize);
        }
        ScrollCmd::PageUp => {
            let n = h.saturating_sub(2).max(1);
            ed.top_line = ed.top_line.saturating_sub(n);
            ed.move_vertical(-(n as isize));
        }
        ScrollCmd::CenterCursor | ScrollCmd::CursorTop | ScrollCmd::CursorBottom => {
            recenter(ed, cmd)
        }
    }
    let last = text_lines(&ed.buffer.rope) - 1;
    ed.top_line = ed.top_line.min(last);
}

fn recenter(ed: &mut Editor, cmd: ScrollCmd) {
    let h = ed.view.height;
    ed.top_line = match cmd {
        ScrollCmd::CenterCursor => ed.cursor.line.saturating_sub(h / 2),
        ScrollCmd::CursorTop => ed.cursor.line,
        ScrollCmd::CursorBottom => (ed.cursor.line + 1).saturating_sub(h),
        _ => ed.top_line,
    };
}
