//! Completion popup mechanics (ticket #22, iteration one). The trigger
//! needs a live rust-analyzer, so these inject a synthetic candidate list
//! and exercise the filter / cycle / accept / refilter behaviour.

use std::path::PathBuf;

use unei::core::buffer::{Buffer, Cursor};
use unei::editor::Editor;
use unei::editor::testing::{feed, text};
use unei::lsp::CompletionItem;

fn ed(name: &str, content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(name));
    let mut e = Editor::new(b);
    e.set_view(80, 22);
    e
}

fn item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        insert_text: label.to_string(),
        filter_text: label.to_string(),
        sort_text: label.to_string(),
        detail: None,
        kind: None,
    }
}

fn items(labels: &[&str]) -> Vec<CompletionItem> {
    labels.iter().map(|l| item(l)).collect()
}

fn selected_label(e: &Editor) -> Option<String> {
    e.completion
        .as_ref()?
        .selected_item()
        .map(|i| i.label.clone())
}

#[test]
fn filters_by_prefix_and_accepts() {
    let mut e = ed("a.rs", "let x = ve\n");
    feed(&mut e, "A"); // append → insert mode, cursor after "ve" (col 10)
    e.inject_completion(items(&["vec", "verbose", "value"]), 8); // prefix "ve"
    let m = e.completion.as_ref().unwrap();
    // "value" doesn't contain "ve" → filtered out; the rest ranked by label
    assert_eq!(m.len(), 2);
    assert_eq!(selected_label(&e).as_deref(), Some("vec"));
    feed(&mut e, "<CR>"); // accept via the insert-mode popup path
    assert_eq!(text(&e), "let x = vec\n");
    assert!(e.completion.is_none());
    assert_eq!(e.cursor.col, 11);
}

#[test]
fn ctrl_n_cycles_and_enter_accepts_in_insert() {
    let mut e = ed("a.rs", "let x = ve\n");
    feed(&mut e, "A"); // append at line end → insert mode, cursor after "ve"
    e.inject_completion(items(&["vec", "verbose"]), 8);
    feed(&mut e, "<C-n>"); // select next → verbose
    assert_eq!(selected_label(&e).as_deref(), Some("verbose"));
    feed(&mut e, "<CR>"); // accept
    assert_eq!(text(&e), "let x = verbose\n");
    assert!(e.completion.is_none());
}

#[test]
fn typing_narrows_the_list() {
    let mut e = ed("a.rs", "let x = ve\n");
    feed(&mut e, "A");
    e.inject_completion(items(&["vec", "verbose"]), 8);
    assert_eq!(e.completion.as_ref().unwrap().len(), 2);
    feed(&mut e, "c"); // now prefix "vec"
    let m = e.completion.as_ref().expect("still open");
    assert_eq!(m.len(), 1);
    assert_eq!(selected_label(&e).as_deref(), Some("vec"));
}

#[test]
fn typing_past_all_matches_closes_it() {
    let mut e = ed("a.rs", "let x = ve\n");
    feed(&mut e, "A");
    e.inject_completion(items(&["vec", "verbose"]), 8);
    feed(&mut e, "z"); // "vez" matches nothing
    assert!(e.completion.is_none());
    assert_eq!(text(&e), "let x = vez\n"); // the keystroke still typed
}

#[test]
fn esc_closes_popup_and_leaves_insert() {
    let mut e = ed("a.rs", "let x = ve\n");
    feed(&mut e, "A");
    e.inject_completion(items(&["vec"]), 8);
    feed(&mut e, "<Esc>");
    assert!(e.completion.is_none());
    // back in normal mode: a normal-mode command now takes effect
    feed(&mut e, "gg");
    assert_eq!(e.cursor.line, 0);
}

#[test]
fn empty_prefix_keeps_all_candidates() {
    let mut e = ed("a.rs", "obj.\n");
    e.cursor = Cursor::new(0, 4); // after the dot
    e.inject_completion(items(&["len", "push", "iter"]), 4); // empty prefix
    assert_eq!(e.completion.as_ref().unwrap().len(), 3);
}

#[test]
fn trigger_in_non_rust_is_inert() {
    let mut e = ed("notes.txt", "hello\n");
    feed(&mut e, "A");
    feed(&mut e, "<C-n>"); // triggers request_completion → rust-only guard
    assert!(e.completion.is_none());
    assert!(
        e.message.as_ref().unwrap().text.contains("Rust only"),
        "expected a rust-only message"
    );
}
