//! Edge cases and unicode: empty buffers, huge counts, grapheme clusters,
//! wide chars, and operations at the buffer edges.
//! Remember the layout: i=up, k=down, j=left, l=right, h=insert.

use unei::editor::testing::{editor_from, feed, text};

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

// empty buffer: everything is safe and sane
golf!(empty_buffer_dd, "", "dd", "\n");
golf!(empty_buffer_x_join_paste_undo, "", "xJpu", "\n");
golf!(empty_buffer_motion_storm, "", "}{WBE^$Ggg0x", "\n");
golf!(empty_buffer_append_line, "", "A!<Esc>", "!\n");
golf!(empty_buffer_open_above, "", "Ox<Esc>", "x\n\n");
golf!(empty_buffer_yank_paste_line, "", "yyp", "\n\n");

// single-character buffer
golf!(single_char_x, "a", "x", "\n");
golf!(single_char_motion_storm, "a\n", "$0welbj~", "A\n");

// huge counts clamp without panics (vim clamps all of these too)
golf!(
    huge_count_w_clamps_to_last_char,
    "one two\n",
    "1000wx",
    "one tw\n"
);
golf!(huge_count_goto_clamps, "a\nb\nc\n", "500Gx", "a\nb\n\n");
golf!(huge_count_dd_clamps, "a\nb\nc\n", "k100dd", "a\n");
golf!(huge_count_dd_whole_buffer, "a\nb\nc\n", "100dd", "\n");
golf!(huge_count_x_stops_at_eol, "abc\ndef\n", "50x", "\ndef\n");
golf!(huge_count_left_right_clamp, "abc\n", "999l999jx", "bc\n");
golf!(huge_count_down_clamps, "a\nb\n", "999kx", "a\n\n");
golf!(huge_count_up_clamps, "a\nb\n", "G999ix", "\nb\n");
golf!(huge_count_operator_word, "one two\n", "d1000w", "\n");
// vim fails a `1000}`/`1000{` outright (cursor stays); Unei clamps to the
// buffer edges — after the round trip both end up back on 'a'
golf!(
    huge_count_para_roundtrip,
    "a\n\nb\n\nc\n",
    "1000}1000{x",
    "\n\nb\n\nc\n"
);
golf!(
    exact_count_para_forward,
    "a\n\nb\n\nc\n",
    "3}x",
    "a\n\nb\n\n\n"
);
golf!(huge_count_replace_aborts_wide, "日本\n", "5r語", "日本\n");

// combining marks (e + U+0301) are single units
golf!(
    tilde_uppercases_base_keeps_mark,
    "e\u{301}x\n",
    "~",
    "E\u{301}x\n"
);
golf!(
    tilde_count_spans_cluster,
    "e\u{301}x\n",
    "2~",
    "E\u{301}X\n"
);
golf!(replace_cluster_with_ascii, "e\u{301}x\n", "ra", "ax\n");
golf!(subst_cluster, "e\u{301}x\n", "sX<Esc>", "Xx\n");
golf!(x_back_deletes_cluster, "ae\u{301}b\n", "$X", "ab\n");
golf!(dl_deletes_cluster, "e\u{301}x\n", "dl", "x\n");
golf!(left_motion_lands_on_cluster, "ae\u{301}b\n", "$jx", "ab\n");

// emoji, including ZWJ sequences, are single units
golf!(
    x_deletes_zwj_family,
    "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}x\n",
    "x",
    "x\n"
);
golf!(x_deletes_flag_pair, "\u{1f1f5}\u{1f1f1}x\n", "x", "x\n");
golf!(
    x_deletes_skin_tone_emoji,
    "\u{1f44d}\u{1f3fd}x\n",
    "x",
    "x\n"
);
golf!(
    motion_over_zwj_sequence,
    "\u{1f469}\u{200d}\u{1f680}ab\n",
    "lx",
    "\u{1f469}\u{200d}\u{1f680}b\n"
);
golf!(dw_emoji_run, "\u{1f642}\u{1f642} word\n", "dw", "word\n");

// CJK wide chars
golf!(x_deletes_wide_char, "日本\n", "x", "本\n");
golf!(dollar_lands_on_wide, "ab日\n", "$x", "ab\n");
golf!(dw_cjk_stops_before_latin, "日本語abc\n", "dw", "abc\n");
golf!(replace_ascii_with_wide_count, "ab\n", "2r語", "語語\n");
golf!(append_after_wide_eol, "日本\n", "A!<Esc>", "日本!\n");
// cut text returns via <leader>p (the #10 register split)
golf!(paste_after_wide_char, "日本\n", "x p", "本日\n");
golf!(x_tab_before_wide, "\t日\n", "x", "日\n");

// operations at the very last char of the buffer
golf!(append_at_last_char, "ab\n", "$a!<Esc>", "ab!\n");
golf!(dl_at_last_char, "ab\n", "$dl", "a\n");
golf!(d_dollar_at_last_char, "ab\n", "$d$", "a\n");

// ~ leaves non-cased scripts alone (but still advances)
golf!(tilde_noncased_unchanged, "日本語\n", "3~", "日本語\n");
golf!(tilde_skips_noncased_moves_on, "日a\n", "~~", "日A\n");
golf!(tilde_mixed_scripts, "aΩb日\n", "4~", "AωB日\n");
golf!(tilde_eszett, "ßx\n", "~", "ẞx\n");

// J at the end of the buffer
golf!(join_on_last_line_noop, "a\nb\n", "kJ", "a\nb\n");
golf!(join_count_clamps, "a\nb\n", "10J", "a b\n");

// multi-line charwise register pasted into the middle of a line
// (de from 'b' crosses the newline, yielding charwise "b\ncd")
golf!(
    paste_multiline_charwise_mid_line,
    "ab\ncd\nXY\n",
    "ldek p",
    "a\nXb\ncdY\n"
);
golf!(
    paste_multiline_charwise_before,
    "ab\ncd\nXY\n",
    "ldek P",
    "a\nb\ncdXY\n"
);
// vim's exclusive-motion adjustment: y} ending in column 0 backs up to the
// end of the previous line, so the register is "ne\ntwo" without a newline
golf!(
    yank_para_paste_mid_line,
    "one\ntwo\n\nX Y\n",
    "ly}4G0p",
    "one\ntwo\n\nXne\ntwo Y\n"
);

// blank-lines-only buffer
golf!(blank_buffer_ops_safe, "\n\n\n", "wx}x{x0x^x$x", "\n\n\n");

// whitespace-only lines: vim's I lands after all the blanks
golf!(insert_h_on_blank_line, "   \n", "Hx<Esc>", "   x\n");

// 0 (and friends) on an empty line
golf!(zero_on_empty_line, "a\n\nb\n", "k0$^x~", "a\n\nb\n");

#[test]
fn blank_lines_word_and_para_motions() {
    let mut ed = editor_from("\n\n\n");
    feed(&mut ed, "}");
    assert_eq!(ed.cursor.line, 2);
    feed(&mut ed, "{");
    assert_eq!(ed.cursor.line, 0);
    feed(&mut ed, "w"); // each empty line is a word
    assert_eq!(ed.cursor.line, 1);
    feed(&mut ed, "w");
    assert_eq!(ed.cursor.line, 2);
    feed(&mut ed, "b");
    assert_eq!(ed.cursor.line, 1);
    feed(&mut ed, "b");
    assert_eq!(ed.cursor.line, 0);
}

#[test]
fn caret_on_whitespace_only_line_goes_to_last_col() {
    let mut ed = editor_from("    \n");
    feed(&mut ed, "0^");
    assert_eq!(ed.cursor.col, 3); // all-blank line: ^ stops on the last char
}

#[test]
fn goal_column_through_wide_chars() {
    let mut ed = editor_from("abcdef\n日本語\nabcdef\n");
    feed(&mut ed, "4l"); // col 4, display cell 4
    assert_eq!(ed.cursor.col, 4);
    feed(&mut ed, "k"); // cell 4 falls inside 語 (cells 4-5)
    assert_eq!((ed.cursor.line, ed.cursor.col), (1, 2));
    feed(&mut ed, "k"); // goal column survives the wide line
    assert_eq!((ed.cursor.line, ed.cursor.col), (2, 4));
    feed(&mut ed, "ii"); // back up through it
    assert_eq!((ed.cursor.line, ed.cursor.col), (0, 4));
}

#[test]
fn goal_column_through_tab_and_wide_mix() {
    // line 1 cells: \t -> 0-3, 日 -> 4-5, x -> 6
    let mut ed = editor_from("abcdefgh\n\t日x\nabcdefgh\n");
    feed(&mut ed, "5l");
    feed(&mut ed, "k"); // cell 5 falls inside 日
    assert_eq!((ed.cursor.line, ed.cursor.col), (1, 1));
    feed(&mut ed, "k");
    assert_eq!((ed.cursor.line, ed.cursor.col), (2, 5));
    feed(&mut ed, "gg2l"); // cell 2
    feed(&mut ed, "k"); // cell 2 falls inside the tab
    assert_eq!((ed.cursor.line, ed.cursor.col), (1, 0));
    feed(&mut ed, "$"); // end of the mixed line is 'x'
    assert_eq!(ed.cursor.col, 2);
    feed(&mut ed, "0");
    assert_eq!(ed.cursor.col, 0);
}
