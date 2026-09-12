//! Jumplist (Ctrl+o / Ctrl+i) and Ctrl+w Ctrl+w window cycling.

use unei::editor::testing::{editor_from, editor_with_buffers, feed, text};

#[test]
fn ctrl_o_means_previous_buffer_not_previous_position() {
    // #104: within one buffer there is no "previous buffer" to go to —
    // the jumps are still recorded (for `''`), but Ctrl+O doesn't walk them
    let long = (1..=60).map(|i| format!("l{i}\n")).collect::<String>();
    let mut ed = editor_from(&long);
    feed(&mut ed, "5G"); // jump: records line 0
    feed(&mut ed, "G"); // jump: records line 4
    assert_eq!(ed.cursor.line, 59);
    feed(&mut ed, "<C-o>");
    assert_eq!(ed.cursor.line, 59, "stays: nothing to go back to");
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("no previous buffer")
    );
    feed(&mut ed, "''"); // where the last jump came from: still the mark's job
    assert_eq!(ed.cursor.line, 4);
}

#[test]
fn ctrl_o_and_ctrl_i_walk_the_buffer_history() {
    let mut ed = editor_with_buffers(&[
        (
            "a.txt",
            "a1
a2
a3
",
        ),
        (
            "b.txt",
            "b1
b2
b3
",
        ),
        (
            "c.txt", "c1
",
        ),
    ]);
    feed(&mut ed, "k"); // a.txt line 1
    feed(&mut ed, " b<C-k><CR>"); // → b.txt
    feed(&mut ed, "G"); // a within-buffer jump in b.txt: recorded, but not a stop
    feed(&mut ed, " b<C-k><C-k><CR>"); // → c.txt
    assert_eq!(text(&ed), "c1\n");
    feed(&mut ed, "<C-o>");
    assert_eq!(text(&ed), "b1\nb2\nb3\n", "one press: the previous buffer");
    assert_eq!(ed.cursor.line, 2, "at the position b.txt was left");
    feed(&mut ed, "<C-o>");
    assert_eq!(text(&ed), "a1\na2\na3\n", "one more: the one before");
    assert_eq!(ed.cursor.line, 1);
    feed(&mut ed, "<C-o>");
    assert_eq!(text(&ed), "a1\na2\na3\n", "oldest: stays");
    feed(&mut ed, "<C-i>");
    assert_eq!(text(&ed), "b1\nb2\nb3\n");
    feed(&mut ed, "<Tab>"); // Tab is Ctrl+I too
    assert_eq!(text(&ed), "c1\n", "back where history was entered");
    feed(&mut ed, "<C-i>");
    assert_eq!(text(&ed), "c1\n", "newest: stays");
    assert!(ed.message.as_ref().unwrap().text.contains("no next buffer"));
}

#[test]
fn paragraph_jumps_still_feed_the_back_mark() {
    let mut ed = editor_from("a\n\nb\n\nc\n");
    feed(&mut ed, "}}");
    assert_eq!(ed.cursor.line, 3);
    feed(&mut ed, "''"); // the pre-jump mark, not Ctrl+O, walks positions
    assert_eq!(ed.cursor.line, 1);
}

#[test]
fn ctrl_o_crosses_buffers() {
    // the reported wish: get back to the previous buffer
    let mut ed = editor_with_buffers(&[("a.txt", "aaa\nbbb\n"), ("b.txt", "xxx\n")]);
    feed(&mut ed, "k"); // line 1 in a.txt
    feed(&mut ed, " b<C-k><CR>"); // switch to b.txt (records the jump)
    assert_eq!(text(&ed), "xxx\n");
    feed(&mut ed, "<C-o>");
    assert_eq!(text(&ed), "aaa\nbbb\n", "back in the previous buffer");
    assert_eq!(ed.cursor.line, 1, "at the position we left");
    feed(&mut ed, "<C-i>");
    assert_eq!(text(&ed), "xxx\n");
}

#[test]
fn stale_buffer_entries_are_dropped() {
    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    feed(&mut ed, " b<C-k><CR>"); // to b.txt, jump recorded from a.txt
    feed(&mut ed, ":bd!<CR>"); // closes b.txt, back in a.txt
    assert_eq!(text(&ed), "a\n");
    feed(&mut ed, "<C-o><C-o>"); // history may reference either; never panics
    assert_eq!(ed.buffer_count(), 1);
    assert_eq!(text(&ed), "a\n");
}

#[test]
fn ctrl_w_ctrl_w_cycles_windows() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v<C-w>s"); // three windows
    let start = ed.focused_window_id();
    let mut seen = vec![start];
    for _ in 0..2 {
        feed(&mut ed, "<C-w><C-w>");
        seen.push(ed.focused_window_id());
    }
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 3, "cycle visits every window");
    feed(&mut ed, "<C-w><C-w>");
    assert_eq!(ed.focused_window_id(), start, "and wraps around");
}

#[test]
fn single_window_cycle_is_noop() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, "<C-w><C-w>x");
    assert_eq!(text(&ed), "bc\n"); // x still lands in the buffer
}
