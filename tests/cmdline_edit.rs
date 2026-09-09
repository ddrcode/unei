//! The command line is an editable line (#97 follow-up): text is inserted
//! at a cursor that moves, so a pre-filled prompt can be corrected rather
//! than only appended to.

use unei::editor::testing::{editor_from, feed, text};

#[test]
fn the_cursor_moves_and_text_is_inserted_where_it_is() {
    let mut ed = editor_from("hello\n");
    feed(&mut ed, ":wq");
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("wq", 2));
    feed(&mut ed, "<Left>");
    assert_eq!(ed.cmdline_cursor, 1);
    feed(&mut ed, "!"); // between w and q
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("w!q", 2));
    feed(&mut ed, "<Del>"); // deletes the q at the cursor
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("w!", 2));
    feed(&mut ed, "<BS>"); // deletes the ! before it
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("w", 1));
    feed(&mut ed, "<Home>");
    assert_eq!(ed.cmdline_cursor, 0);
    feed(&mut ed, "<Left>"); // clamps
    assert_eq!(ed.cmdline_cursor, 0);
    feed(&mut ed, "<End><Right>"); // clamps the other way
    assert_eq!(ed.cmdline_cursor, 1);
    feed(&mut ed, "<Esc>");
    assert_eq!(ed.cmdline, "");
    assert_eq!(ed.cmdline_cursor, 0);
}

#[test]
fn backspace_at_the_start_keeps_the_line_but_empties_it_to_leave() {
    let mut ed = editor_from("hello\n");
    feed(&mut ed, ":set<Home>");
    feed(&mut ed, "<BS>"); // nothing before the cursor: the prompt stays
    assert_eq!(ed.mode, unei::editor::Mode::Command);
    assert_eq!(ed.cmdline, "set");
    feed(&mut ed, "<End><BS><BS><BS>");
    assert_eq!(ed.cmdline, "");
    feed(&mut ed, "<BS>"); // empty: backspace leaves the prompt (vim)
    assert_eq!(ed.mode, unei::editor::Mode::Normal);
}

#[test]
fn ctrl_u_and_ctrl_w_cut_back_from_the_cursor() {
    let mut ed = editor_from("hello\n");
    feed(&mut ed, ":w some/path.rs<C-w>");
    assert_eq!(ed.cmdline, "w some/path.");
    feed(&mut ed, "<C-w>");
    assert_eq!(ed.cmdline, "w some/path");
    feed(&mut ed, "<C-u>");
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("", 0));
    // Ctrl+U only wipes what precedes the cursor
    feed(&mut ed, "abcd<Left><Left><C-u>");
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("cd", 0));
    feed(&mut ed, "<Esc>");
}

#[test]
fn editing_a_search_pattern_re_runs_it_incrementally() {
    let mut ed = editor_from("alpha\nbeta\ngamma\n");
    feed(&mut ed, "/gxmma"); // typo: no match, cursor stays home
    assert_eq!(ed.cursor.line, 0);
    feed(&mut ed, "<Home><Right><Del>"); // drop the x → "gmma"… still no
    assert_eq!(ed.cmdline, "gmma");
    feed(&mut ed, "a"); // "gamma" — the pattern matches from the cursor
    assert_eq!(ed.cmdline, "gamma");
    feed(&mut ed, "<CR>");
    assert_eq!(ed.cursor.line, 2);
}

#[test]
fn a_paste_lands_at_the_cursor() {
    let mut ed = editor_from("x\n");
    feed(&mut ed, ":wq");
    feed(&mut ed, "<Left>");
    ed.paste_external("!!");
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("w!!q", 3));
    feed(&mut ed, "<Esc>");
    assert_eq!(text(&ed), "x\n", "the prompt never touches the buffer");
}
