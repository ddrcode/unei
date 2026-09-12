//! Symbol picker (ticket #62): fuzzy-jump to functions/structs/etc. in the
//! current buffer, extracted via tree-sitter. `<leader>s` (Space s).

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::feed;

fn ed(name: &str, content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(name));
    let mut e = Editor::new(b);
    e.set_view(80, 22);
    e
}

const SRC: &str =
    "fn alpha() {}\nstruct Beta;\nimpl Beta {\n    fn method(&self) {}\n}\nfn gamma() {}\n";

#[test]
fn opens_and_lists_symbols() {
    let mut e = ed("a.rs", SRC);
    feed(&mut e, "<Space>s");
    let p = e.file_picker.as_ref().expect("symbol picker open");
    // fn alpha, struct Beta, impl Beta, fn method, fn gamma
    assert_eq!(p.matches.len(), 5);
}

#[test]
fn fuzzy_filters_and_jumps() {
    let mut e = ed("a.rs", SRC);
    feed(&mut e, "<Space>s");
    feed(&mut e, "gamma"); // narrow to fn gamma (line 5)
    feed(&mut e, "<CR>");
    assert!(e.file_picker.is_none());
    assert_eq!(e.cursor.line, 5);
}

#[test]
fn jump_to_method_records_a_jumplist_entry() {
    let mut e = ed("a.rs", SRC);
    feed(&mut e, "<Space>s");
    feed(&mut e, "method"); // the impl method on line 3
    feed(&mut e, "<CR>");
    assert_eq!(e.cursor.line, 3);
    feed(&mut e, "''"); // jumplist back to where we started
    assert_eq!(e.cursor.line, 0);
}

#[test]
fn typing_a_kind_filters_to_it() {
    let mut e = ed("a.rs", SRC);
    feed(&mut e, "<Space>s");
    feed(&mut e, "struct"); // items are "struct Beta", "fn …" — only the struct matches
    let p = e.file_picker.as_ref().unwrap();
    assert_eq!(p.matches.len(), 1);
}

#[test]
fn no_symbols_for_plain_text() {
    let mut e = ed("notes.txt", "just prose\n");
    feed(&mut e, "<Space>s");
    assert!(e.file_picker.is_none());
    assert!(e.message.as_ref().unwrap().text.contains("no symbol"));
}
