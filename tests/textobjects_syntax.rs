//! Syntax-aware text objects (#78): `af`/`nf` (function) and `ac`/`nc` (class)
//! resolved via tree-sitter. Layout reminder: i=up, k=down, j=left, l=right.

use unei::editor::testing::{editor_with_buffers, feed, text};

fn run(src: &str, keys: &str) -> String {
    let mut e = editor_with_buffers(&[("t.rs", src)]);
    feed(&mut e, keys);
    text(&e)
}

#[test]
fn daf_deletes_the_whole_function() {
    assert_eq!(
        run("fn a() {\n    x();\n}\nfn b() {}\n", "daf"),
        "fn b() {}\n"
    );
}

#[test]
fn daf_targets_the_function_the_cursor_is_in() {
    // down to fn b, delete it, leave fn a
    assert_eq!(
        run("fn a() {}\nfn b() {\n    y();\n}\n", "kkkdaf"),
        "fn a() {}\n"
    );
}

#[test]
fn nf_is_the_body_keeping_the_signature() {
    assert_eq!(
        run("fn a() {\n    x();\n    y();\n}\n", "kdnf"),
        "fn a() {\n}\n"
    );
}

#[test]
fn dac_deletes_the_enclosing_impl_from_a_method() {
    let src = "impl Foo {\n    fn m(&self) {\n        z();\n    }\n}\nfn keep() {}\n";
    assert_eq!(run(src, "kkdac"), "fn keep() {}\n");
}

#[test]
fn daf_inside_an_impl_removes_only_the_method() {
    let src = "impl Foo {\n    fn m(&self) {\n        z();\n    }\n}\n";
    assert_eq!(run(src, "kkdaf"), "impl Foo {\n}\n");
}

#[test]
fn caf_replaces_a_function() {
    // change-around-function opens an insert where the function was
    assert_eq!(
        run("fn old() {\n    a();\n}\n", "caffn new() {}<Esc>"),
        "fn new() {}\n"
    );
}
