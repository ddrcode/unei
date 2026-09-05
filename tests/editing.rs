//! End-to-end editing flows through the modal state machine.
//! Remember the layout: i=up, k=down, j=left, l=right, h=insert.

use tailored::editor::testing::{editor_from, feed, text};

macro_rules! golf {
    ($name:ident, $before:expr, $keys:expr, $after:expr) => {
        #[test]
        fn $name() {
            let mut ed = editor_from($before);
            feed(&mut ed, $keys);
            assert_eq!(text(&ed), $after, "keys: {}", $keys);
        }
    };
}

// insert mode entries (h family)
golf!(insert_before, "world\n", "hhello <Esc>", "hello world\n");
golf!(
    insert_at_line_start,
    "  world\n",
    "$Hhello <Esc>",
    "  hello world\n"
);
golf!(append_after_cursor, "wld\n", "aor<Esc>", "world\n");
golf!(append_shifted, "hello\n", "A!<Esc>", "hello!\n");
golf!(
    open_below_keeps_indent,
    "    fn x()\n",
    "oy<Esc>",
    "    fn x()\n    y\n"
);
golf!(open_above, "b\n", "Oa<Esc>", "a\nb\n");

// deletion
golf!(x_deletes_char, "abc\n", "x", "bc\n");
golf!(x_with_count, "abcdef\n", "3x", "def\n");
golf!(x_clamps_at_eol, "ab\n", "5x", "\n");
golf!(dd_deletes_line, "one\ntwo\nthree\n", "kdd", "one\nthree\n");
golf!(dd_with_count, "a\nb\nc\nd\n", "2dd", "c\nd\n");
golf!(dd_last_line, "a\nb\n", "kdd", "a\n");
golf!(dd_only_line, "solo\n", "dd", "\n");
golf!(dw_deletes_word, "one two three\n", "dw", "two three\n");
golf!(
    dw_last_word_stays_on_line,
    "one two\nnext\n",
    "wdw",
    "one \nnext\n"
);
golf!(d_dollar, "hello world\n", "wd$", "hello \n");
golf!(de_inclusive, "one two\n", "de", " two\n");
golf!(d_down_linewise, "a\nb\nc\n", "dk", "c\n");
golf!(d_up_linewise, "a\nb\nc\n", "Gdi", "a\n");
golf!(dj_deletes_left, "abc\n", "ldj", "bc\n");
golf!(dh_still_deletes_left, "abc\n", "ldh", "bc\n");
golf!(dgg_to_top, "a\nb\nc\n", "Gdgg", "\n");
// vim's exclusive-motion adjustments (:h word, :h exclusive), nvim-verified
golf!(
    dw_on_trailing_blank_keeps_newline,
    "one \nbar\n",
    "3ldw",
    "one\nbar\n"
);
golf!(
    dw_whole_word_leaves_empty_line,
    "foo\nbar\n",
    "dw",
    "\nbar\n"
);
golf!(
    dw_stops_at_empty_line_word,
    "foo\n\nbar\n",
    "dw",
    "\n\nbar\n"
);
golf!(dw_on_empty_line_eats_it, "\nbar\nx\n", "dw", "bar\nx\n");
golf!(
    d_para_from_indent_goes_linewise,
    "  one\ntwo\n\nrest\n",
    "^d}",
    "\nrest\n"
);
golf!(
    d_para_mid_line_backs_up,
    "one\ntwo\n\nX Y\n",
    "ld}",
    "o\n\nX Y\n"
);
golf!(d_brace_back_col0_linewise, "a\nb\nc\nd\n", "3Gd{", "c\nd\n");
golf!(
    d_brace_back_mid_col_charwise,
    "aa\nbb\ncc\ndd\n",
    "3Gld{",
    "c\ndd\n"
);
golf!(
    y_brace_back_col0_linewise,
    "aa\nbb\ncc\n",
    "3Gy{P",
    "aa\nbb\naa\nbb\ncc\n"
);
golf!(d_to_line_via_count_g, "a\nb\nc\nd\n", "d2G", "c\nd\n");
golf!(dfx_inclusive, "abcxdef\n", "dfx", "def\n");
golf!(dtx_exclusive_of_target, "abcxdef\n", "dtx", "xdef\n");

// change
golf!(
    cw_acts_like_ce,
    "hello world\n",
    "cwbye<Esc>",
    "bye world\n"
);
golf!(cw_on_blank, "a   b\n", "lcwx<Esc>", "axb\n");
golf!(cc_keeps_indent, "    old\n", "ccnew<Esc>", "    new\n");
golf!(c_dollar, "hello world\n", "wC!<Esc>", "hello !\n");
golf!(s_substitutes_char, "abc\n", "sx<Esc>", "xbc\n");
golf!(substitute_line, "  abc\ndef\n", "Sxyz<Esc>", "  xyz\ndef\n");

// yank / paste
golf!(yy_p_below, "one\ntwo\n", "yyp", "one\none\ntwo\n");
golf!(yy_p_at_last_line, "one\n", "yyp", "one\none\n");
golf!(yy_big_p_above, "one\ntwo\n", "kyyP", "one\ntwo\ntwo\n");
golf!(yw_p_charwise, "one two\n", "ywwp", "one tone wo\n");
// the swap idiom via the cut register (#10 split)
golf!(x_then_p_swaps_chars, "ab\n", "x p", "ba\n");
golf!(
    dd_p_moves_line_down,
    "one\ntwo\nthree\n",
    "dd p",
    "two\none\nthree\n"
);
golf!(paste_with_count, "ab\n", "yl3p", "aaaab\n");

// undo / redo / repeat
golf!(undo_dd, "one\ntwo\n", "ddu", "one\ntwo\n");
golf!(undo_insert_is_one_change, "x\n", "hhello<Esc>u", "x\n");
golf!(redo_after_undo, "one\ntwo\n", "ddu<C-r>", "two\n");
golf!(dot_repeats_x, "abcd\n", "x.", "cd\n");
golf!(dot_repeats_dd, "a\nb\nc\n", "dd.", "c\n");
golf!(dot_repeats_insert, "x\n", "hab<Esc>.", "aabbx\n");
golf!(
    dot_repeats_cw,
    "foo foo foo\n",
    "cwbar<Esc>w.",
    "bar bar foo\n"
);
golf!(undo_after_dot, "abc\n", "x.u", "bc\n");

// counts and combinations
golf!(count_motion_delete, "a b c d e\n", "d3w", "d e\n");
golf!(count_times_count, "a b c d e f g\n", "2d2w", "e f g\n");
golf!(three_j_left, "abcdef\n", "$3jx", "abdef\n");

// join
golf!(join_two_lines, "one\ntwo\n", "J", "one two\n");
golf!(join_trims_indent, "one\n    two\n", "J", "one two\n");
golf!(join_count, "a\nb\nc\n", "3J", "a b c\n");
golf!(join_empty_next, "one\n\ntwo\n", "J", "one\ntwo\n");

// replace / toggle case
golf!(replace_char, "abc\n", "rx", "xbc\n");
golf!(replace_with_count, "abcdef\n", "3rx", "xxxdef\n");
golf!(replace_count_too_big_aborts, "ab\n", "5rx", "ab\n");
golf!(toggle_case, "abC\n", "3~", "ABc\n");

// find on line
golf!(till_semicolon_repeat, "a.b.c.d\n", "t.;x", "a..c.d\n");
golf!(find_backward, "abcabc\n", "$Fax", "abcbc\n");

// insert-mode editing keys
golf!(
    enter_auto_indents,
    "    code\n",
    "Amore<CR>next<Esc>",
    "    codemore\n    next\n"
);
golf!(
    backspace_joins_lines,
    "ab\ncd\n",
    "kh<BS><BS><Esc>",
    "acd\n"
);
golf!(tab_inserts_spaces, "\n", "h<Tab>x<Esc>", "    x\n");
golf!(ctrl_w_deletes_word, "\n", "hfoo bar<C-w><Esc>", "foo \n");
golf!(
    ctrl_u_deletes_to_indent,
    "\n",
    "h    abc<C-u>x<Esc>",
    "    x\n"
);

// unicode
golf!(x_deletes_grapheme_cluster, "e\u{301}x\n", "x", "x\n");
golf!(wide_chars_delete, "日本語 ok\n", "dw", "ok\n");
golf!(toggle_unicode_case, "łódź\n", "~~", "ŁÓdź\n");

// edge cases
golf!(empty_buffer_motions_are_safe, "", "wbeklij$0x", "\n");
golf!(delete_on_empty_line, "a\n\nb\n", "kx", "a\n\nb\n");
golf!(o_on_empty_buffer, "", "ohello<Esc>", "\nhello\n");
golf!(paste_without_register, "abc\n", "p", "abc\n");
golf!(cw_at_eol, "ab\n", "$cwX<Esc>", "aX\n");

#[test]
fn cursor_position_after_dd() {
    let mut ed = editor_from("one\n  two\nthree\n");
    feed(&mut ed, "dd");
    assert_eq!(ed.cursor.line, 0);
    assert_eq!(ed.cursor.col, 2); // first non-blank of "  two"
}

#[test]
fn esc_moves_cursor_left_after_insert() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, "A!<Esc>");
    assert_eq!(ed.cursor.col, 3); // on '!'
}

#[test]
fn quit_commands() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, ":q<CR>");
    assert!(ed.should_quit);

    let mut ed = editor_from("abc\n");
    feed(&mut ed, "x:q<CR>");
    assert!(!ed.should_quit); // modified, refuses
    feed(&mut ed, ":q!<CR>");
    assert!(ed.should_quit);

    let mut ed = editor_from("abc\n");
    feed(&mut ed, "xZQ");
    assert!(ed.should_quit);
}

#[test]
fn goto_line_via_cmdline() {
    let mut ed = editor_from("a\nb\n  c\nd\n");
    feed(&mut ed, ":3<CR>");
    assert_eq!(ed.cursor.line, 2);
    assert_eq!(ed.cursor.col, 2);
}

#[test]
fn unknown_command_reports_error() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, ":nonsense<CR>");
    let msg = ed.message.as_ref().expect("expected an error message");
    assert!(msg.error);
    assert!(msg.text.contains("nonsense"));
}

#[test]
fn vertical_motion_keeps_goal_column() {
    let mut ed = editor_from("longer line\nx\nanother long line\n");
    feed(&mut ed, "$");
    let col_before = ed.cursor.col;
    feed(&mut ed, "kk"); // down twice through the short line
    assert_eq!(ed.cursor.line, 2);
    assert!(ed.cursor.col > 0);
    feed(&mut ed, "ii");
    assert_eq!(ed.cursor.line, 0);
    assert_eq!(ed.cursor.col, col_before);
}

#[test]
fn scrolloff_keeps_context() {
    let content = (1..=100).map(|i| format!("line {i}\n")).collect::<String>();
    let mut ed = editor_from(&content);
    ed.set_view(80, 20);
    feed(&mut ed, "50G");
    // scrolloff is clamped to (h-1)/2 = 9 for a 20-row view
    assert_eq!(ed.top_line, 39);
    feed(&mut ed, "gg");
    assert_eq!(ed.top_line, 0);
}
