//! Small vim parity items (ticket #14): %, marks, gf, :w {file}.

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};

fn ed(name: &str, content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(name));
    let mut e = Editor::new(b);
    e.set_view(80, 22);
    e
}

// ---- % (match pair) ---------------------------------------------------

#[test]
fn percent_jumps_between_brackets() {
    let mut e = ed("a.rs", "foo(bar)baz\n");
    feed(&mut e, "lll"); // cursor on '(' at col 3
    assert_eq!(e.cursor.col, 3);
    feed(&mut e, "%");
    assert_eq!(e.cursor.col, 7); // ')'
    feed(&mut e, "%");
    assert_eq!(e.cursor.col, 3); // back to '('
}

#[test]
fn percent_finds_first_bracket_on_line() {
    // vim: from before a bracket, % seeks the first one on the line
    let mut e = ed("a.rs", "let x = foo(y);\n");
    feed(&mut e, "%"); // from col 0, first bracket is '(' → jump to ')'
    assert_eq!(e.cursor.col, 13); // ')'
}

#[test]
fn percent_matches_across_lines_and_nesting() {
    let mut e = ed("a.rs", "fn f() {\n    g(a(b));\n}\n");
    feed(&mut e, "$"); // end of line 0, on '{'
    assert_eq!(e.cursor.line, 0);
    feed(&mut e, "%");
    assert_eq!((e.cursor.line, e.cursor.col), (2, 0)); // matching '}'
}

#[test]
fn d_percent_deletes_inclusive() {
    let mut e = ed("a.rs", "foo(bar)baz\n");
    feed(&mut e, "lll"); // on '('
    feed(&mut e, "d%");
    assert_eq!(text(&e), "foobaz\n");
}

#[test]
fn percent_inert_without_brackets() {
    let mut e = ed("a.rs", "plain line\n");
    feed(&mut e, "lll");
    feed(&mut e, "%");
    assert_eq!(e.cursor.col, 3); // unmoved
}

// ---- marks ------------------------------------------------------------

#[test]
fn mark_set_and_jump_exact() {
    let mut e = ed("a.rs", "line one\nline two\nline three\n");
    feed(&mut e, "ll"); // (0,2)
    feed(&mut e, "ma");
    feed(&mut e, "G"); // last line
    assert!(e.cursor.line >= 2);
    feed(&mut e, "`a"); // exact jump back
    assert_eq!((e.cursor.line, e.cursor.col), (0, 2));
}

#[test]
fn mark_jump_linewise_first_non_blank() {
    let mut e = ed("a.rs", "    indented\nsecond\n");
    feed(&mut e, "$"); // end of line 0
    feed(&mut e, "ma");
    feed(&mut e, "G");
    feed(&mut e, "'a"); // linewise → first non-blank (col 4)
    assert_eq!((e.cursor.line, e.cursor.col), (0, 4));
}

#[test]
fn back_jump_toggles() {
    let mut e = ed("a.rs", "one\ntwo\nthree\nfour\n");
    feed(&mut e, "ll"); // (0,2)
    feed(&mut e, "G"); // records pre-jump (0,2)
    let g_line = e.cursor.line;
    feed(&mut e, "``"); // back to (0,2)
    assert_eq!((e.cursor.line, e.cursor.col), (0, 2));
    feed(&mut e, "``"); // toggle back to the G spot
    assert_eq!(e.cursor.line, g_line);
}

#[test]
fn jump_to_unset_mark_errs() {
    let mut e = ed("a.rs", "hello\n");
    feed(&mut e, "`z");
    assert!(e.message.as_ref().unwrap().text.contains("Mark not set"));
    assert_eq!(e.cursor.col, 0);
}

// ---- gf and :w {file} -------------------------------------------------

#[test]
fn gf_opens_file_under_cursor() {
    let dir = std::env::temp_dir().join(format!("unei-gf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("helper.rs"), "pub fn help() {}\n").unwrap();
    let main = dir.join("main.rs");
    std::fs::write(&main, "mod helper;\n").unwrap();
    let mut b = Buffer::from_text("mod helper;\n");
    b.path = Some(main);
    let mut e = Editor::new(b);
    e.set_view(80, 22);
    feed(&mut e, "fh"); // land on the 'h' of "helper"
    feed(&mut e, "gf");
    assert!(
        e.buffer
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("helper.rs")),
        "opened {:?}",
        e.buffer.path
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn write_named_path_binds_buffer() {
    let dir = std::env::temp_dir().join(format!("unei-w-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("out.rs");
    let _ = std::fs::remove_file(&target);
    let mut e = ed("scratch.rs", "hello world\n");
    feed(&mut e, &format!(":w {}<CR>", target.display()));
    assert!(target.is_file(), "file was written");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello world\n");
    assert_eq!(e.buffer.path.as_deref(), Some(target.as_path()));
    std::fs::remove_dir_all(&dir).ok();
}
