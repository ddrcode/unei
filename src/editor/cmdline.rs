use crate::config::keymap::Key;
use crate::core::buffer::Cursor;
use crate::search::Direction;

use super::{Editor, Mode, Prompt};

/// Byte offset of char index `at` in `s` (its end when past the last char).
pub(crate) fn byte_at(s: &str, at: usize) -> usize {
    s.char_indices().nth(at).map(|(b, _)| b).unwrap_or(s.len())
}

/// The command line is an editable line: text is inserted at the cursor,
/// which moves with the arrows and Home/End, so a pre-filled prompt (the
/// rename of #97) can be edited rather than only appended to.
pub fn handle_key(ed: &mut Editor, key: Key) {
    let len = ed.cmdline.chars().count();
    match key {
        Key::Esc | Key::Ctrl('c') | Key::Ctrl('[') => {
            ed.cmdline.clear();
            ed.cmdline_cursor = 0;
            ed.mode = Mode::Normal;
            cancel_search_prompt(ed);
        }
        Key::Backspace => {
            if ed.cmdline.is_empty() {
                // backspacing off the start of an empty line leaves the
                // prompt, vim-style
                ed.mode = Mode::Normal;
                cancel_search_prompt(ed);
            } else if ed.cmdline_cursor > 0 {
                let at = byte_at(&ed.cmdline, ed.cmdline_cursor - 1);
                ed.cmdline.remove(at);
                ed.cmdline_cursor -= 1;
                incremental(ed);
            }
        }
        Key::Delete => {
            if ed.cmdline_cursor < len {
                let at = byte_at(&ed.cmdline, ed.cmdline_cursor);
                ed.cmdline.remove(at);
                incremental(ed);
            }
        }
        Key::Left => ed.cmdline_cursor = ed.cmdline_cursor.saturating_sub(1),
        Key::Right => ed.cmdline_cursor = (ed.cmdline_cursor + 1).min(len),
        Key::Home => ed.cmdline_cursor = 0,
        Key::End => ed.cmdline_cursor = len,
        Key::Char(c) => {
            let at = byte_at(&ed.cmdline, ed.cmdline_cursor);
            ed.cmdline.insert(at, c);
            ed.cmdline_cursor += 1;
            incremental(ed);
        }
        Key::Ctrl('u') => {
            // vim's c_CTRL-U: wipe what precedes the cursor
            let at = byte_at(&ed.cmdline, ed.cmdline_cursor);
            ed.cmdline.replace_range(..at, "");
            ed.cmdline_cursor = 0;
            incremental(ed);
        }
        Key::Ctrl('w') => {
            // vim's c_CTRL-W: wipe the word before the cursor
            let chars: Vec<char> = ed.cmdline.chars().collect();
            let mut start = ed.cmdline_cursor;
            while start > 0 && chars[start - 1].is_whitespace() {
                start -= 1;
            }
            let word = start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_');
            while start > 0 {
                let c = chars[start - 1];
                let keep = if word {
                    c.is_alphanumeric() || c == '_'
                } else {
                    !c.is_whitespace() && !c.is_alphanumeric() && c != '_'
                };
                if !keep {
                    break;
                }
                start -= 1;
            }
            let (from, to) = (
                byte_at(&ed.cmdline, start),
                byte_at(&ed.cmdline, ed.cmdline_cursor),
            );
            ed.cmdline.replace_range(from..to, "");
            ed.cmdline_cursor = start;
            incremental(ed);
        }
        Key::Enter => {
            let cmd = std::mem::take(&mut ed.cmdline);
            ed.cmdline_cursor = 0;
            ed.mode = Mode::Normal;
            match ed.prompt {
                Prompt::Command => execute(ed, cmd.trim()),
                Prompt::Search { forward } => accept_search(ed, &cmd, forward),
                Prompt::Rename { line, col } => ed.analyzer_rename(line, col, cmd.trim()),
            }
        }
        _ => {}
    }
}

/// Saves the pre-prompt search state once, on the first prompt keystroke.
fn stash_search(ed: &mut Editor) {
    if ed.search_saved.is_none() {
        ed.search_saved = Some((
            ed.search.pattern.clone(),
            ed.search.direction,
            ed.search.highlight,
        ));
    }
}

/// Live preview: recompile, re-highlight, and jump from the origin.
fn incremental(ed: &mut Editor) {
    let Prompt::Search { forward } = ed.prompt else {
        return;
    };
    stash_search(ed);
    let direction = if forward {
        Direction::Forward
    } else {
        Direction::Backward
    };
    let ok = ed.search.set_pattern(&ed.cmdline.clone(), direction);
    let Some((origin, top)) = ed.search_origin else {
        return;
    };
    if !ok {
        // invalid or empty: sit at the origin with no matches shown
        ed.cursor = origin;
        ed.top_line = top;
        return;
    }
    ed.search_ensure_current();
    let found = if forward {
        ed.search.next_after(origin.line, origin.col as u32)
    } else {
        ed.search.prev_before(origin.line, origin.col as u32)
    };
    match found {
        Some(((line, col, _), _)) => {
            ed.cursor = Cursor::new(line, col as usize);
            ed.goal = None;
            ed.clamp_cursor();
            ed.scroll_to_cursor();
        }
        None => {
            ed.cursor = origin;
            ed.top_line = top;
        }
    }
}

fn cancel_search_prompt(ed: &mut Editor) {
    if let Some((origin, top)) = ed.search_origin.take() {
        ed.cursor = origin;
        ed.top_line = top;
        ed.clamp_cursor();
    }
    if let Some((pattern, direction, highlight)) = ed.search_saved.take() {
        if pattern.is_empty() {
            ed.search = crate::search::Search::default();
        } else {
            ed.search.set_pattern(&pattern, direction);
            ed.search.highlight = highlight;
        }
    }
}

fn accept_search(ed: &mut Editor, typed: &str, forward: bool) {
    let origin = ed.search_origin.take().map(|(c, _)| c);
    let saved = ed.search_saved.take();
    if typed.is_empty() {
        // bare Enter repeats the previous search, vim-style
        if let Some((pattern, _, _)) = saved {
            if pattern.is_empty() {
                ed.err("E35: No previous search");
                return;
            }
            let direction = if forward {
                Direction::Forward
            } else {
                Direction::Backward
            };
            ed.search.set_pattern(&pattern, direction);
        }
    }
    if !ed.search.is_active() {
        ed.err(format!("E383: Invalid pattern: {}", ed.search.pattern));
        return;
    }
    // the jumplist remembers where the search started
    if let Some(origin) = origin {
        let landing = ed.cursor;
        ed.cursor = origin;
        ed.record_jump();
        ed.cursor = landing;
        if typed.is_empty() {
            // repeat-search hasn't jumped yet during preview
            ed.search_step(true);
        }
    }
    let prefix = if forward { '/' } else { '?' };
    ed.msg(format!("{prefix}{}", ed.search.pattern));
}

fn execute(ed: &mut Editor, cmd: &str) {
    if cmd.is_empty() {
        return;
    }
    if let Ok(n) = cmd.parse::<usize>() {
        ed.goto_line(n.saturating_sub(1));
        return;
    }
    match cmd {
        "w" => {
            ed.save(false);
        }
        "w!" => {
            ed.save(true); // overwrite even if the file changed on disk (#80)
        }
        "e" => ed.reload_from_disk(false),
        "e!" => ed.reload_from_disk(true),
        "q" => ed.close_window_or_quit(false),
        "q!" => ed.close_window_or_quit(true),
        "qa" | "quita" | "qall" => ed.quit(false),
        "qa!" | "quita!" | "qall!" => ed.quit(true),
        "wa" | "wall" => ed.save_all(),
        "wq" => ed.save_and_quit(false, false),
        "wq!" => ed.save_and_quit(false, true),
        "x" => ed.save_and_quit(true, false),
        "x!" => ed.save_and_quit(true, true),
        _ if cmd.starts_with("w ") => ed.save_as(&cmd[2..], false),
        _ if cmd.starts_with("w! ") => ed.save_as(&cmd[3..], true),
        "bd" | "bdelete" => ed.close_buffer(ed.current_buffer_id(), false),
        "bd!" | "bdelete!" => ed.close_buffer(ed.current_buffer_id(), true),
        "noh" | "nohl" | "nohlsearch" => ed.search.highlight = false,
        _ if cmd.starts_with("s/") || cmd.starts_with("%s/") => ed.substitute(cmd),
        _ => ed.err(format!("E492: Not an editor command: {cmd}")),
    }
}
