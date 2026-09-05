//! Comment toggling (ticket #53): gcc line(s), gc selection; per-language
//! token, asm token from the modeline.

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

#[test]
fn gcc_toggles_current_line_rust() {
    let mut e = ed("a.rs", "let x = 1;\nlet y = 2;\n");
    feed(&mut e, "gcc");
    assert_eq!(text(&e), "// let x = 1;\nlet y = 2;\n");
    feed(&mut e, "gcc"); // toggle back
    assert_eq!(text(&e), "let x = 1;\nlet y = 2;\n");
}

#[test]
fn gcc_preserves_indentation() {
    let mut e = ed("a.rs", "    deep();\n");
    feed(&mut e, "gcc");
    assert_eq!(text(&e), "    // deep();\n");
    feed(&mut e, "gcc");
    assert_eq!(text(&e), "    deep();\n");
}

#[test]
fn count_comments_multiple_lines() {
    let mut e = ed("a.toml", "a = 1\nb = 2\nc = 3\n");
    feed(&mut e, "3gcc");
    assert_eq!(text(&e), "# a = 1\n# b = 2\n# c = 3\n");
}

#[test]
fn visual_gc_toggles_selection() {
    // V on line 0, k extends down one line (IJKL: k is down), gc comments
    let mut e = ed("a.py", "one\ntwo\nthree\n");
    feed(&mut e, "Vkgc");
    assert_eq!(text(&e), "# one\n# two\nthree\n");
}

#[test]
fn mixed_block_comments_all_then_uncomments() {
    // one already-commented line among bare ones → first toggle comments
    // all (not a mix), second uncomments all
    let mut e = ed("a.rs", "// a\nb\nc\n");
    feed(&mut e, "3gcc");
    assert_eq!(text(&e), "// // a\n// b\n// c\n");
    feed(&mut e, "3gcc");
    assert_eq!(text(&e), "// a\nb\nc\n");
}

#[test]
fn asm_uses_modeline_leader() {
    let mut e = ed("g.s", "; asm: 65c02 acme\n        lda #1\n");
    feed(&mut e, "kgcc"); // down to the instruction, comment it
    assert_eq!(text(&e), "; asm: 65c02 acme\n        ; lda #1\n");
    feed(&mut e, "gcc");
    assert_eq!(text(&e), "; asm: 65c02 acme\n        lda #1\n");
}

#[test]
fn asm_without_modeline_is_inert() {
    let mut e = ed("g.s", "        lda #1\n");
    feed(&mut e, "gcc");
    assert_eq!(text(&e), "        lda #1\n"); // no dialect → no comment
    assert!(e.message.as_ref().unwrap().text.contains("no line-comment"));
}
