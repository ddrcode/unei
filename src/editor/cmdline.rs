use crate::config::keymap::Key;

use super::{Editor, Mode};

pub fn handle_key(ed: &mut Editor, key: Key) {
    match key {
        Key::Esc | Key::Ctrl('c') | Key::Ctrl('[') => {
            ed.cmdline.clear();
            ed.mode = Mode::Normal;
        }
        Key::Backspace => {
            if ed.cmdline.pop().is_none() {
                ed.mode = Mode::Normal;
            }
        }
        Key::Char(c) => ed.cmdline.push(c),
        Key::Enter => {
            let cmd = std::mem::take(&mut ed.cmdline);
            ed.mode = Mode::Normal;
            execute(ed, cmd.trim());
        }
        _ => {}
    }
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
            ed.save();
        }
        "q" => ed.quit(false),
        "q!" => ed.quit(true),
        "wq" => ed.save_and_quit(false),
        "x" => ed.save_and_quit(true),
        _ => ed.err(format!("E492: Not an editor command: {cmd}")),
    }
}
