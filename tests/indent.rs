//! Indent operators (ticket #13): >>/<<, > / < over motions & selections,
//! insert-mode Ctrl+T/Ctrl+D, dot-repeat and single-undo.

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};

fn ed(content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from("a.rs"));
    let mut e = Editor::new(b);
    e.set_view(80, 22);
    e
}

#[test]
fn shift_right_then_left() {
    let mut e = ed("foo\n");
    feed(&mut e, ">>");
    assert_eq!(text(&e), "    foo\n");
    feed(&mut e, "<lt><lt>");
    assert_eq!(text(&e), "foo\n");
}

#[test]
fn count_shifts_multiple_lines() {
    let mut e = ed("a\nb\nc\n");
    feed(&mut e, "2>>");
    assert_eq!(text(&e), "    a\n    b\nc\n");
}

#[test]
fn shift_over_motion_and_to_end() {
    let mut e = ed("a\nb\nc\n");
    feed(&mut e, ">k"); // current + one down (k=down in IJKL)
    assert_eq!(text(&e), "    a\n    b\nc\n");
    let mut e = ed("a\nb\nc\n");
    feed(&mut e, ">G");
    assert_eq!(text(&e), "    a\n    b\n    c\n");
}

#[test]
fn shift_to_matching_bracket() {
    let mut e = ed("fn f() {\nx\n}\n");
    feed(&mut e, "$"); // on '{'
    feed(&mut e, ">%");
    assert_eq!(text(&e), "    fn f() {\n    x\n    }\n");
}

#[test]
fn dedent_saturates_at_zero() {
    let mut e = ed("  x\n"); // 2 spaces, less than shiftwidth
    feed(&mut e, "<lt><lt>");
    assert_eq!(text(&e), "x\n");
}

#[test]
fn blank_lines_are_left_alone() {
    let mut e = ed("a\n\nb\n");
    feed(&mut e, "3>>");
    assert_eq!(text(&e), "    a\n\n    b\n");
}

#[test]
fn cursor_lands_on_first_non_blank() {
    let mut e = ed("foo\n");
    feed(&mut e, ">>");
    assert_eq!((e.cursor.line, e.cursor.col), (0, 4));
}

#[test]
fn dot_repeats_the_shift() {
    let mut e = ed("x\n");
    feed(&mut e, ">>");
    feed(&mut e, ".");
    assert_eq!(text(&e), "        x\n"); // two shiftwidths
}

#[test]
fn shift_is_one_undo_unit() {
    let mut e = ed("a\nb\nc\n");
    feed(&mut e, "3>>");
    feed(&mut e, "u");
    assert_eq!(text(&e), "a\nb\nc\n");
}

#[test]
fn visual_shift_selection() {
    let mut e = ed("a\nb\nc\n");
    feed(&mut e, "Vk>"); // visual line, extend down (k), shift
    assert_eq!(text(&e), "    a\n    b\nc\n");
}

#[test]
fn insert_ctrl_t_and_ctrl_d() {
    let mut e = ed("foo\n");
    feed(&mut e, "A"); // insert at end, cursor col 3
    feed(&mut e, "<C-t>");
    assert_eq!(text(&e), "    foo\n");
    assert_eq!(e.cursor.col, 7); // cursor rode the indent
    feed(&mut e, "<C-d>");
    assert_eq!(text(&e), "foo\n");
    assert_eq!(e.cursor.col, 3);
}
