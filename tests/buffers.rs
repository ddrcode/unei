//! Buffer management: switching, the buffer list, closing (ticket #2).
//! Layout reminder: i=up, k=down, j=left, l=right, h=insert; leader is space.

use unei::editor::testing::{editor_from, editor_with_buffers, feed, text};

fn three() -> unei::editor::Editor {
    editor_with_buffers(&[("a.txt", "aaa\n"), ("b.txt", "bbb\n"), ("c.txt", "ccc\n")])
}

#[test]
fn starts_with_first_buffer_current() {
    let ed = three();
    assert_eq!(ed.buffer_count(), 3);
    assert_eq!(ed.current_buffer_id(), 1);
    assert_eq!(text(&ed), "aaa\n");
    let names: Vec<String> = ed.buffer_entries().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["a.txt", "b.txt", "c.txt"]);
}

#[test]
fn list_select_switches_buffer() {
    let mut ed = three();
    feed(&mut ed, " b"); // leader+b opens the list
    assert!(ed.buffer_list.is_some());
    feed(&mut ed, "kk<CR>"); // down twice, pick c.txt
    assert!(ed.buffer_list.is_none());
    assert_eq!(ed.current_buffer_id(), 3);
    assert_eq!(text(&ed), "ccc\n");
}

#[test]
fn list_selection_clamps_and_dismisses() {
    let mut ed = three();
    feed(&mut ed, " b");
    feed(&mut ed, "iii"); // up past the top clamps
    feed(&mut ed, "kkkkkk"); // down past the bottom clamps
    feed(&mut ed, "<Esc>");
    assert!(ed.buffer_list.is_none());
    assert_eq!(ed.current_buffer_id(), 1); // Esc keeps the current buffer

    feed(&mut ed, " b");
    feed(&mut ed, "q"); // q also dismisses
    assert!(ed.buffer_list.is_none());
}

#[test]
fn alternate_toggles_between_last_two() {
    let mut ed = three();
    feed(&mut ed, " bk<CR>"); // switch to b.txt
    assert_eq!(ed.current_buffer_id(), 2);
    feed(&mut ed, "<C-^>");
    assert_eq!(ed.current_buffer_id(), 1);
    feed(&mut ed, "<C-6>"); // Ctrl+6 is the same key
    assert_eq!(ed.current_buffer_id(), 2);
}

#[test]
fn alternate_without_history_errors() {
    let mut ed = three();
    feed(&mut ed, "<C-^>");
    assert_eq!(ed.current_buffer_id(), 1);
    let msg = ed.message.as_ref().expect("message");
    assert!(msg.error && msg.text.contains("E23"));
}

#[test]
fn per_buffer_cursor_is_restored() {
    let mut ed = editor_with_buffers(&[("a.txt", "one\ntwo\nthree\n"), ("b.txt", "xx\n")]);
    feed(&mut ed, "kk"); // line 2 of a.txt
    feed(&mut ed, " bk<CR>"); // to b.txt
    assert_eq!(ed.cursor.line, 0);
    feed(&mut ed, "<C-^>"); // back to a.txt
    assert_eq!(ed.cursor.line, 2);
}

#[test]
fn edits_and_undo_are_per_buffer() {
    let mut ed = editor_with_buffers(&[("a.txt", "aaa\n"), ("b.txt", "bbb\n")]);
    feed(&mut ed, "x"); // a: "aa"
    feed(&mut ed, " bk<CR>x"); // b: "bb"
    assert_eq!(text(&ed), "bb\n");
    feed(&mut ed, "u"); // undoes only b's change
    assert_eq!(text(&ed), "bbb\n");
    feed(&mut ed, "<C-^>");
    assert_eq!(text(&ed), "aa\n"); // a still has its edit
    feed(&mut ed, "u");
    assert_eq!(text(&ed), "aaa\n");
}

#[test]
fn registers_are_shared_across_buffers() {
    let mut ed = editor_with_buffers(&[("a.txt", "hello\n"), ("b.txt", "world\n")]);
    feed(&mut ed, "yy"); // yank "hello"
    feed(&mut ed, " bk<CR>p"); // paste into b.txt
    assert_eq!(text(&ed), "world\nhello\n");
}

#[test]
fn close_from_list_switches_away() {
    let mut ed = three();
    feed(&mut ed, " bx"); // close a.txt (selected = current)
    assert!(ed.buffer_list.is_some()); // list stays open
    assert_eq!(ed.buffer_count(), 2);
    assert_eq!(ed.current_buffer_id(), 2); // moved to the next buffer
    feed(&mut ed, "<Esc>");
    assert_eq!(text(&ed), "bbb\n");
}

#[test]
fn close_modified_buffer_refuses() {
    let mut ed = three();
    feed(&mut ed, "x"); // modify a.txt
    feed(&mut ed, " bx");
    assert_eq!(ed.buffer_count(), 3); // refused
    let msg = ed.message.as_ref().expect("message");
    assert!(msg.error && msg.text.contains("E89"));
    feed(&mut ed, "<Esc>");
    assert_eq!(ed.current_buffer_id(), 1);
}

#[test]
fn bd_closes_current_and_bd_bang_forces() {
    let mut ed = three();
    feed(&mut ed, ":bd<CR>");
    assert_eq!(ed.buffer_count(), 2);
    assert_eq!(ed.current_buffer_id(), 2);

    feed(&mut ed, "x:bd<CR>"); // modified → refused
    assert_eq!(ed.buffer_count(), 2);
    assert!(ed.message.as_ref().unwrap().text.contains("E89"));

    feed(&mut ed, ":bd!<CR>"); // forced
    assert_eq!(ed.buffer_count(), 1);
    assert_eq!(ed.current_buffer_id(), 3);
}

#[test]
fn closing_last_buffer_quits() {
    let mut ed = editor_from("solo\n");
    feed(&mut ed, ":bd<CR>");
    assert!(ed.should_quit);
}

#[test]
fn closing_down_to_last_then_quit() {
    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    feed(&mut ed, ":bd<CR>");
    assert_eq!(ed.buffer_count(), 1);
    assert!(!ed.should_quit);
    feed(&mut ed, ":bd<CR>");
    assert!(ed.should_quit);
}

#[test]
fn close_current_prefers_alternate() {
    let mut ed = three();
    feed(&mut ed, " bkk<CR>"); // to c.txt; alternate = a.txt
    feed(&mut ed, ":bd<CR>"); // close c.txt
    assert_eq!(ed.current_buffer_id(), 1); // lands on the alternate
    assert_eq!(ed.buffer_count(), 2);
}

#[test]
fn closed_buffer_is_no_longer_alternate() {
    let mut ed = three();
    feed(&mut ed, " bk<CR>"); // to b; alternate = a
    feed(&mut ed, " b<CR>"); // list opens on current; selecting b... pick a instead
    // (selecting the current entry is a no-op switch)
    assert_eq!(ed.current_buffer_id(), 2);
    feed(&mut ed, ":bd<CR>"); // close b → to alternate a
    assert_eq!(ed.current_buffer_id(), 1);
    feed(&mut ed, "<C-^>"); // b is gone; no valid alternate
    assert!(ed.message.as_ref().unwrap().text.contains("E23"));
}

#[test]
fn quit_blocked_by_hidden_modified_buffer() {
    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    feed(&mut ed, "x"); // modify a.txt
    feed(&mut ed, " bk<CR>"); // switch to clean b.txt
    feed(&mut ed, ":q<CR>");
    assert!(!ed.should_quit);
    assert!(ed.message.as_ref().unwrap().text.contains("E162"));
    feed(&mut ed, ":q!<CR>");
    assert!(ed.should_quit);
}

#[test]
fn vim_flags_in_buffer_entries() {
    let mut ed = three();
    feed(&mut ed, " bk<CR>x"); // to b.txt, modify it
    let entries = ed.buffer_entries();
    assert!(entries[0].alternate && !entries[0].current);
    assert!(entries[1].current && entries[1].modified);
    assert!(!entries[2].current && !entries[2].alternate);
}

#[test]
fn buffer_numbers_are_stable_after_close() {
    let mut ed = three();
    feed(&mut ed, ":bd<CR>"); // close #1
    let ids: Vec<usize> = ed.buffer_entries().into_iter().map(|e| e.id).collect();
    assert_eq!(ids, [2, 3]); // numbers not reused/renumbered
}

#[test]
fn switching_does_not_disturb_dot_repeat() {
    let mut ed = editor_with_buffers(&[("a.txt", "abc\n"), ("b.txt", "xyz\n")]);
    feed(&mut ed, "x"); // last change: x
    feed(&mut ed, " bk<CR>"); // switch buffers
    feed(&mut ed, "."); // repeats x in the new buffer
    assert_eq!(text(&ed), "yz\n");
}

#[test]
fn buffer_list_keys_do_not_edit_text() {
    let mut ed = three();
    feed(&mut ed, " bxikq<Esc>"); // x closes a buffer; i/k move; q dismisses
    assert_eq!(text(&ed), "bbb\n");
    assert_eq!(ed.buffer_count(), 2);
}

#[test]
fn qa_quits_everything_with_the_modified_guard() {
    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    feed(&mut ed, "<C-w>v<C-w>s"); // several windows
    feed(&mut ed, "x"); // modify
    feed(&mut ed, ":qa<CR>");
    assert!(!ed.should_quit, "qa refuses with unsaved changes");
    assert!(ed.message.as_ref().unwrap().error);
    feed(&mut ed, ":qa!<CR>");
    assert!(ed.should_quit, "qa! forces");

    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    feed(&mut ed, "<C-w>v:qa<CR>"); // clean: quits despite multiple windows
    assert!(ed.should_quit);
}
