//! Text objects (ticket #12): `a` (around) / `n` (inner) families on
//! d/c/y and in visual mode. `i`-objects can't exist (i is up-motion).

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

// ---- words ------------------------------------------------------------

#[test]
fn inner_word_delete() {
    let mut e = ed("foo bar baz\n");
    feed(&mut e, "w"); // on 'bar'
    feed(&mut e, "dnw");
    assert_eq!(text(&e), "foo  baz\n");
}

#[test]
fn around_word_delete_eats_trailing_space() {
    let mut e = ed("foo bar baz\n");
    feed(&mut e, "w");
    feed(&mut e, "daw");
    assert_eq!(text(&e), "foo baz\n");
}

#[test]
fn change_inner_word() {
    let mut e = ed("foo bar baz\n");
    feed(&mut e, "w");
    feed(&mut e, "cnwquux<Esc>");
    assert_eq!(text(&e), "foo quux baz\n");
}

#[test]
fn inner_word_from_mid_word() {
    let mut e = ed("hello world\n");
    feed(&mut e, "lll"); // mid 'hello'
    feed(&mut e, "dnw");
    assert_eq!(text(&e), " world\n");
}

#[test]
fn big_word_object() {
    let mut e = ed("a.b.c next\n");
    feed(&mut e, "dnW"); // WORD = "a.b.c"
    assert_eq!(text(&e), " next\n");
}

// ---- pairs ------------------------------------------------------------

#[test]
fn inner_and_around_parens() {
    let mut e = ed("x = foo(a, b);\n");
    feed(&mut e, "f(l"); // inside the parens
    feed(&mut e, "dn(");
    assert_eq!(text(&e), "x = foo();\n");

    let mut e = ed("x = foo(a, b);\n");
    feed(&mut e, "f(l");
    feed(&mut e, "da(");
    assert_eq!(text(&e), "x = foo;\n");
}

#[test]
fn paren_aliases_b_and_on_bracket() {
    let mut e = ed("(a, b)\n");
    feed(&mut e, "dnb"); // b == parens
    assert_eq!(text(&e), "()\n");
    let mut e = ed("(a, b)\n");
    feed(&mut e, "dn)"); // cursor on '(' at col 0 still works
    assert_eq!(text(&e), "()\n");
}

#[test]
fn inner_braces_multiline() {
    let mut e = ed("fn f() {\n    body;\n}\n");
    feed(&mut e, "j"); // wait: j is LEFT; go down with k
    feed(&mut e, "k"); // into the body line
    feed(&mut e, "dn{");
    assert_eq!(text(&e), "fn f() {}\n");
}

#[test]
fn change_inner_parens() {
    let mut e = ed("call(old)\n");
    feed(&mut e, "f(l");
    feed(&mut e, "cn(new<Esc>");
    assert_eq!(text(&e), "call(new)\n");
}

#[test]
fn brackets_and_angles() {
    let mut e = ed("arr[idx] vec<T>\n");
    feed(&mut e, "f[l");
    feed(&mut e, "dn[");
    assert_eq!(text(&e), "arr[] vec<T>\n");
    feed(&mut e, "f<lt>l");
    feed(&mut e, "dn<lt>");
    assert_eq!(text(&e), "arr[] vec<>\n");
}

// ---- quotes -----------------------------------------------------------

#[test]
fn inner_and_around_quotes() {
    let mut e = ed("say \"hello\" ok\n");
    feed(&mut e, "f\"l"); // inside the quotes
    feed(&mut e, "dn\"");
    assert_eq!(text(&e), "say \"\" ok\n");

    let mut e = ed("say 'hi' ok\n");
    feed(&mut e, "f'l");
    feed(&mut e, "da'"); // quotes + trailing space
    assert_eq!(text(&e), "say ok\n");
}

// ---- paragraphs -------------------------------------------------------

#[test]
fn inner_and_around_paragraph() {
    let mut e = ed("a\nb\n\nc\n");
    feed(&mut e, "dnp"); // inner: lines a,b
    assert_eq!(text(&e), "\nc\n");

    let mut e = ed("a\nb\n\nc\n");
    feed(&mut e, "dap"); // + trailing blank line
    assert_eq!(text(&e), "c\n");
}

// ---- visual + guards --------------------------------------------------

#[test]
fn visual_selects_object_then_operates() {
    let mut e = ed("foo(bar)\n");
    feed(&mut e, "f(");
    feed(&mut e, "vn(d"); // visual, select inner parens, delete
    assert_eq!(text(&e), "foo()\n");
}

#[test]
fn a_and_n_keep_normal_meaning_without_operator() {
    // plain `a` still appends
    let mut e = ed("hi\n");
    feed(&mut e, "aX<Esc>");
    assert_eq!(text(&e), "hXi\n");
}
