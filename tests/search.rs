//! Search (ticket #8 phase 1): incremental /?, n/N, *, hlsearch, :s.

use unei::editor::testing::{editor_from, editor_with_buffers, feed, text};

#[test]
fn slash_jumps_incrementally_and_enter_commits() {
    let mut ed = editor_from("alpha\nbeta\ngamma\nbeta two\n");
    feed(&mut ed, "/beta");
    assert_eq!(ed.cursor.line, 1, "incremental jump while typing");
    feed(&mut ed, "<CR>");
    assert_eq!((ed.cursor.line, ed.cursor.col), (1, 0));
    assert!(ed.search.highlight);
    assert_eq!(ed.search.count(), 2);
}

#[test]
fn esc_cancels_and_restores_position() {
    let mut ed = editor_from("alpha\nbeta\n");
    feed(&mut ed, "/beta");
    assert_eq!(ed.cursor.line, 1);
    feed(&mut ed, "<Esc>");
    assert_eq!(ed.cursor.line, 0, "cancel returns to origin");
}

#[test]
fn n_and_shift_n_navigate_with_wrap() {
    let mut ed = editor_from("x one\ntwo x\nthree\nx four\n");
    feed(&mut ed, "/x<CR>");
    assert_eq!(ed.cursor.line, 1); // first after origin (0,0): line 1
    feed(&mut ed, "n");
    assert_eq!(ed.cursor.line, 3);
    feed(&mut ed, "n"); // wraps
    assert_eq!(ed.cursor.line, 0);
    assert!(ed.message.as_ref().unwrap().text.contains("BOTTOM"));
    feed(&mut ed, "N"); // wraps back
    assert_eq!(ed.cursor.line, 3);
}

#[test]
fn question_mark_searches_backward() {
    let mut ed = editor_from("one\ntarget\ntwo\ntarget\nthree\n");
    feed(&mut ed, "G");
    feed(&mut ed, "?target<CR>");
    assert_eq!(ed.cursor.line, 3);
    feed(&mut ed, "n"); // n follows the search direction: backward
    assert_eq!(ed.cursor.line, 1);
}

#[test]
fn smartcase_applies() {
    let mut ed = editor_from("Word word WORD\n");
    feed(&mut ed, "/word<CR>");
    assert_eq!(ed.search.count(), 3);
    feed(&mut ed, "/Word<CR>");
    assert_eq!(ed.search.count(), 1);
}

#[test]
fn star_searches_word_under_cursor() {
    let mut ed = editor_from("let val = 1;\nvalue\nlet val2 = val;\n");
    feed(&mut ed, "w"); // on `val`
    feed(&mut ed, "*");
    // whole-word: skips `value` and `val2`, lands on `val` in line 2
    assert_eq!((ed.cursor.line, ed.cursor.col), (2, 11));
    assert_eq!(ed.search.count(), 2);
    feed(&mut ed, "<C-o>"); // star recorded the jump
    assert_eq!(ed.cursor.line, 0);
}

#[test]
fn esc_calms_highlight_noh_too() {
    let mut ed = editor_from("aaa\n");
    feed(&mut ed, "/a<CR>");
    assert!(ed.search.highlight);
    feed(&mut ed, "<Esc>");
    assert!(!ed.search.highlight);
    feed(&mut ed, "n"); // navigating re-lights it
    assert!(ed.search.highlight);
    feed(&mut ed, ":noh<CR>");
    assert!(!ed.search.highlight);
}

#[test]
fn substitute_is_file_scoped_first_match_per_line() {
    let mut ed = editor_from("aa aa\nbb\naa\n");
    feed(&mut ed, ":s/aa/XX/<CR>");
    assert_eq!(text(&ed), "XX aa\nbb\nXX\n");
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("2 substitutions on 2 lines")
    );
    feed(&mut ed, "u");
    assert_eq!(text(&ed), "aa aa\nbb\naa\n", "one undo step");
}

#[test]
fn substitute_g_flag_hits_all_per_line() {
    let mut ed = editor_from("aa aa\naa\n");
    feed(&mut ed, ":%s/aa/X/g<CR>");
    assert_eq!(text(&ed), "X X\nX\n");
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("3 substitutions")
    );
}

#[test]
fn substitute_from_visual_uses_selection_scope() {
    let mut ed = editor_from("aa\naa\naa\naa\n");
    feed(&mut ed, "kVk"); // select lines 1-2
    feed(&mut ed, ":s/aa/X/<CR>");
    assert_eq!(text(&ed), "aa\nX\nX\naa\n");
}

#[test]
fn substitute_capture_groups() {
    let mut ed = editor_from("name: alpha\n");
    feed(&mut ed, ":s/name: (\\w+)/$1: name/<CR>");
    assert_eq!(text(&ed), "alpha: name\n");
}

#[test]
fn substitute_escaped_slash() {
    let mut ed = editor_from("a/b\n");
    feed(&mut ed, ":s/a\\/b/ok/<CR>");
    assert_eq!(text(&ed), "ok\n");
}

#[test]
fn substitute_no_match_reports() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, ":s/zzz/x/<CR>");
    assert_eq!(text(&ed), "abc\n");
    assert!(ed.message.as_ref().unwrap().error);
}

#[test]
fn invalid_pattern_is_safe() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, "/(unclosed");
    assert_eq!(ed.cursor.line, 0); // no jump on invalid
    feed(&mut ed, "<CR>");
    assert!(ed.message.as_ref().unwrap().error);
    feed(&mut ed, "x"); // editor still healthy
    assert_eq!(text(&ed), "bc\n");
}

#[test]
fn empty_slash_repeats_last_search() {
    let mut ed = editor_from("x\nyy\nx\n");
    feed(&mut ed, "/x<CR>");
    assert_eq!(ed.cursor.line, 2);
    feed(&mut ed, "gg");
    feed(&mut ed, "/<CR>");
    assert_eq!(ed.cursor.line, 2);
}

#[test]
fn search_commits_record_the_jumplist() {
    let mut ed = editor_from("one\ntwo\nfindme\n");
    feed(&mut ed, "/findme<CR>");
    assert_eq!(ed.cursor.line, 2);
    feed(&mut ed, "<C-o>");
    assert_eq!(ed.cursor.line, 0, "Ctrl+O returns to where / was pressed");
}

#[test]
fn matches_recompute_after_edits() {
    let mut ed = editor_from("foo\nfoo\n");
    feed(&mut ed, "/foo<CR>");
    assert_eq!(ed.search.count(), 2);
    feed(&mut ed, "ggdd");
    ed.search_ensure_current();
    assert_eq!(ed.search.count(), 1);
}

#[test]
fn search_state_is_per_editor_not_per_buffer_content() {
    let mut ed = editor_with_buffers(&[("a.txt", "hit\n"), ("b.txt", "hit hit\n")]);
    feed(&mut ed, "/hit<CR>");
    assert_eq!(ed.search.count(), 1);
    feed(&mut ed, " b<C-k><CR>"); // switch buffer; pattern survives
    ed.search_ensure_current();
    assert_eq!(ed.search.count(), 2);
    feed(&mut ed, "n");
    assert_eq!(ed.cursor.col, 4);
}
