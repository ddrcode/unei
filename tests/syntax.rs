//! Tree-sitter highlighting (ticket #17): detection, captures, injections,
//! and cache invalidation — through the editor's own cache.

use std::path::PathBuf;

use tailored::config::theme;
use tailored::core::buffer::Buffer;
use tailored::editor::Editor;
use tailored::editor::testing::feed;

fn editor_with(name: &str, content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(name));
    let mut ed = Editor::new(b);
    ed.set_view(80, 22);
    ed
}

/// Capture names active on `line`, as (start, end, name).
fn spans(ed: &Editor, line: usize) -> Vec<(u32, u32, &'static str)> {
    let id = ed.current_buffer_id();
    ed.syntax
        .line_spans(id, line)
        .iter()
        .map(|(s, e, c)| (*s, *e, theme::CAPTURES[*c as usize]))
        .collect()
}

fn capture_at(ed: &Editor, line: usize, col: u32) -> Option<&'static str> {
    spans(ed, line)
        .into_iter()
        .find(|(s, e, _)| *s <= col && col < *e)
        .map(|(_, _, n)| n)
}

#[test]
fn rust_basics_are_captured() {
    let mut ed = editor_with(
        "main.rs",
        "fn main() {\n    // greet\n    let s = \"hi\";\n}\n",
    );
    assert!(ed.ensure_syntax(1));
    assert_eq!(capture_at(&ed, 0, 0), Some("keyword")); // fn
    assert_eq!(capture_at(&ed, 0, 3), Some("function")); // main
    assert_eq!(capture_at(&ed, 1, 6), Some("comment")); // // greet
    assert_eq!(capture_at(&ed, 2, 4), Some("keyword")); // let
    assert_eq!(capture_at(&ed, 2, 13), Some("string")); // "hi"
}

#[test]
fn unregistered_language_renders_plain() {
    let mut ed = editor_with("script.sh", "echo hi\n");
    assert!(!ed.ensure_syntax(1));
    assert!(!ed.syntax.has_highlights(1));
    assert!(spans(&ed, 0).is_empty());
}

#[test]
fn pathless_buffer_renders_plain() {
    let mut ed = Editor::new(Buffer::from_text("fn main() {}\n"));
    assert!(!ed.ensure_syntax(1));
}

#[test]
fn toml_keys_and_strings() {
    let mut ed = editor_with("Cargo.toml", "[package]\nname = \"tailored\"\n");
    ed.ensure_syntax(1);
    // bare keys are @type in the official toml query (innermost capture
    // over the pair-level @property wash) — same rendering as nvim
    assert_eq!(capture_at(&ed, 0, 1), Some("type")); // [package] table name
    assert_eq!(capture_at(&ed, 1, 0), Some("type")); // name key
    assert_eq!(capture_at(&ed, 1, 5), Some("operator")); // =
    assert_eq!(capture_at(&ed, 1, 7), Some("string"));
}

#[test]
fn cargo_lock_detects_as_toml() {
    let mut ed = editor_with("Cargo.lock", "version = 4\n");
    assert!(ed.ensure_syntax(1));
    assert_eq!(capture_at(&ed, 0, 0), Some("type"));
}

#[test]
fn markdown_headings_and_inline() {
    let mut ed = editor_with("notes.md", "# Title\n\nsome `code` here\n");
    ed.ensure_syntax(1);
    assert_eq!(capture_at(&ed, 0, 2), Some("text.title"));
    // `code` is highlighted by the injected inline grammar
    assert_eq!(capture_at(&ed, 2, 6), Some("text.literal"));
}

#[test]
fn markdown_rust_fence_is_injected() {
    let mut ed = editor_with("doc.md", "```rust\nfn x() {}\n```\n");
    ed.ensure_syntax(1);
    assert_eq!(capture_at(&ed, 1, 0), Some("keyword")); // fn inside the fence
    assert_eq!(capture_at(&ed, 1, 3), Some("function"));
}

#[test]
fn edits_refresh_the_cache() {
    let mut ed = editor_with("edit.rs", "fn main() {}\n");
    ed.ensure_syntax(1);
    assert_eq!(capture_at(&ed, 0, 0), Some("keyword"));
    feed(&mut ed, "xx"); // "fn" -> " main..." no keyword at 0
    ed.ensure_syntax(1);
    assert_ne!(capture_at(&ed, 0, 0), Some("keyword"));
    feed(&mut ed, "uu"); // undo both deletes
    ed.ensure_syntax(1);
    assert_eq!(capture_at(&ed, 0, 0), Some("keyword"));
}

#[test]
fn multiline_tokens_split_per_line() {
    let mut ed = editor_with("m.rs", "/* one\ntwo */\nfn f() {}\n");
    ed.ensure_syntax(1);
    assert_eq!(capture_at(&ed, 0, 0), Some("comment"));
    assert_eq!(capture_at(&ed, 1, 0), Some("comment"));
    assert_eq!(capture_at(&ed, 2, 0), Some("keyword"));
}

#[test]
fn unicode_columns_are_char_based() {
    // the keyword after a wide-char string must align by chars, not bytes
    let mut ed = editor_with("u.rs", "let s = \"日本語\"; let t = 1;\n");
    ed.ensure_syntax(1);
    assert_eq!(capture_at(&ed, 0, 0), Some("keyword")); // first let
    assert_eq!(capture_at(&ed, 0, 16), Some("keyword")); // second let
    assert_eq!(capture_at(&ed, 0, 9), Some("string")); // inside the string
}

#[test]
fn closing_a_buffer_drops_its_cache() {
    let mut ed = editor_with("a.rs", "fn a() {}\n");
    ed.ensure_syntax(1);
    assert!(ed.syntax.has_highlights(1));
    feed(&mut ed, ":bd<CR>"); // last buffer: editor quits, cache still dropped
    assert!(ed.should_quit);
}

#[test]
fn oversized_buffers_render_plain() {
    let big = "x".repeat(3 * 1024 * 1024);
    let mut ed = editor_with("big.rs", &big);
    ed.ensure_syntax(1);
    assert!(spans(&ed, 0).is_empty());
}
