//! Replace (overtype) mode — `R`. Typed chars replace the ones under the
//! cursor so line length is preserved; Backspace restores what was covered.

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::testing::{feed, text};
use unei::editor::{Editor, Mode};

fn ed(content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from("a.s"));
    let mut e = Editor::new(b);
    e.set_view(80, 22);
    e
}

#[test]
fn overtypes_in_place() {
    let mut e = ed("abcdef\n");
    feed(&mut e, "RXY");
    assert_eq!(text(&e), "XYcdef\n");
    assert_eq!(e.mode, Mode::Replace);
}

#[test]
fn appends_past_line_end() {
    let mut e = ed("ab\n");
    feed(&mut e, "RXYZ"); // X,Y overtype; Z appends
    assert_eq!(text(&e), "XYZ\n");
}

#[test]
fn backspace_restores_overwritten() {
    let mut e = ed("abc\n");
    feed(&mut e, "RXY"); // -> "XYc"
    assert_eq!(text(&e), "XYc\n");
    feed(&mut e, "<BS>"); // restore b
    assert_eq!(text(&e), "Xbc\n");
    assert_eq!(e.cursor.col, 1);
    feed(&mut e, "<BS>"); // restore a
    assert_eq!(text(&e), "abc\n");
    assert_eq!(e.cursor.col, 0);
}

#[test]
fn backspace_deletes_appended() {
    let mut e = ed("ab\n");
    feed(&mut e, "RXYZ"); // "XYZ", Z appended
    feed(&mut e, "<BS>"); // Z was appended -> delete
    assert_eq!(text(&e), "XY\n");
}

#[test]
fn tab_aligned_comment_stays_put() {
    // the assembly use case: overtype the mnemonic, keep the aligned comment
    let mut e = ed("nop\t; keep\n");
    feed(&mut e, "Rlda"); // replace n,o,p
    assert_eq!(text(&e), "lda\t; keep\n"); // length preserved, comment aligned
}

#[test]
fn esc_returns_to_normal_and_undo_restores() {
    let mut e = ed("hello\n");
    feed(&mut e, "RJJJ<Esc>"); // overwrite hel -> "JJJlo"
    assert_eq!(text(&e), "JJJlo\n");
    assert_eq!(e.mode, Mode::Normal);
    feed(&mut e, "u"); // one undo unit for the whole session
    assert_eq!(text(&e), "hello\n");
}
