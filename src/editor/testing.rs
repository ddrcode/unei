//! Headless driving of the editor for tests: feed keys in a compact vim-like
//! notation, inspect the buffer.

use crate::config::keymap::Key;
use crate::core::buffer::Buffer;

use super::Editor;

pub fn editor_from(text: &str) -> Editor {
    let mut ed = Editor::new(Buffer::from_text(text));
    ed.set_view(80, 22);
    ed
}

/// Editor with several named buffers; the first is displayed. No file IO —
/// the names only become paths (so don't `:w` in such tests).
pub fn editor_with_buffers(specs: &[(&str, &str)]) -> Editor {
    assert!(!specs.is_empty());
    let buffers = specs
        .iter()
        .map(|(name, content)| {
            let mut b = Buffer::from_text(content);
            b.path = Some(std::path::PathBuf::from(name));
            b
        })
        .collect();
    let mut ed = Editor::with_buffers(buffers);
    ed.set_view(80, 22);
    ed
}

/// Parses `"wdw<Esc>2k<C-r>"`-style specs. `<lt>` is a literal `<`.
pub fn keys(spec: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '<' {
            out.push(Key::Char(c));
            continue;
        }
        let mut name = String::new();
        for n in chars.by_ref() {
            if n == '>' {
                break;
            }
            name.push(n);
        }
        let key = match name.as_str() {
            "Esc" => Key::Esc,
            "CR" | "Enter" => Key::Enter,
            "BS" => Key::Backspace,
            "Tab" => Key::Tab,
            "Del" => Key::Delete,
            "Up" => Key::Up,
            "Down" => Key::Down,
            "Left" => Key::Left,
            "Right" => Key::Right,
            "Home" => Key::Home,
            "End" => Key::End,
            "PgUp" => Key::PageUp,
            "PgDn" => Key::PageDown,
            "lt" => Key::Char('<'),
            s if s.starts_with("C-") && s.chars().count() == 3 => {
                Key::Ctrl(s.chars().nth(2).unwrap())
            }
            s if s.starts_with("A-") && s.chars().count() == 3 => {
                Key::Alt(s.chars().nth(2).unwrap())
            }
            "Space" => Key::Char(' '),
            other => panic!("unknown key spec <{other}>"),
        };
        out.push(key);
    }
    out
}

pub fn feed(ed: &mut Editor, spec: &str) {
    for key in keys(spec) {
        ed.handle_key(key);
    }
}

pub fn text(ed: &Editor) -> String {
    ed.buffer.rope.to_string()
}

/// The screen region the window tree occupies in tests (see `editor_from`).
pub fn window_area(ed: &Editor) -> ratatui::layout::Rect {
    let _ = ed;
    ratatui::layout::Rect::new(0, 0, 80, 23)
}
