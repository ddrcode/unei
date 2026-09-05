//! Undo, redo, and dot-repeat semantics.
//! Remember the layout: i=up, k=down, j=left, l=right, h=insert (vim's i),
//! H=insert at first non-blank (vim's I).
//!
//! Every expectation here is real vim behavior, verified against a headless
//! nvim oracle (keys fed with `feedkeys(..., 'tx')` so undo blocks split as
//! they do interactively) and translated through the IJKL keymap.

use unei::editor::testing::{editor_from, feed, text};

macro_rules! golf {
    ($(#[$meta:meta])* $name:ident, $before:expr, $keys:expr, $after:expr) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            let mut ed = editor_from($before);
            feed(&mut ed, $keys);
            assert_eq!(text(&ed), $after, "keys: {}", $keys);
        }
    };
}

// each insert entry, with its typed text, is a single undo unit
golf!(undo_insert_a_one_unit, "wld\n", "aor<Esc>u", "wld\n");
golf!(
    undo_insert_shift_a_one_unit,
    "hello\n",
    "A!!!<Esc>u",
    "hello\n"
);
golf!(undo_insert_o_one_unit, "a\n", "oxy<Esc>u", "a\n");
golf!(undo_insert_shift_o_one_unit, "b\n", "Oxy<Esc>u", "b\n");
golf!(
    undo_insert_shift_h_one_unit,
    "  world\n",
    "$Hyo <Esc>u",
    "  world\n"
);
golf!(
    undo_insert_with_newline_one_unit,
    "one\ntwo\n",
    "A end<CR>x<Esc>u",
    "one\ntwo\n"
);
golf!(
    undo_insert_with_backspace_one_unit,
    "ab\ncd\n",
    "kh<BS><BS>X<Esc>u",
    "ab\ncd\n"
);
golf!(
    undo_insert_with_ctrl_w_one_unit,
    "\n",
    "hfoo bar<C-w>x<Esc>u",
    "\n"
);

// entering insert and Esc-ing without typing leaves no undo entry —
// u afterwards undoes the previous change instead
golf!(
    empty_insert_h_leaves_no_undo_entry,
    "abcd\n",
    "xh<Esc>u",
    "abcd\n"
);
golf!(
    empty_insert_a_leaves_no_undo_entry,
    "abcd\n",
    "xa<Esc>u",
    "abcd\n"
);
golf!(
    empty_insert_shift_a_leaves_no_undo_entry,
    "abcd\n",
    "xA<Esc>u",
    "abcd\n"
);
// ...except o/O, which change the buffer even with nothing typed
golf!(
    o_without_typing_is_still_a_change,
    "ab\n",
    "xo<Esc>u",
    "b\n"
);
golf!(
    shift_o_without_typing_is_still_a_change,
    "ab\n",
    "xO<Esc>u",
    "b\n"
);

// change commands plus their insert session are one unit
golf!(undo_cw_one_unit, "foo bar\n", "wcwqux<Esc>u", "foo bar\n");
golf!(
    undo_cc_one_unit,
    "    old\n",
    "ccnew stuff<Esc>u",
    "    old\n"
);
golf!(undo_s_one_unit, "abc\n", "sxyz<Esc>u", "abc\n");
golf!(
    undo_shift_s_one_unit,
    "  abc\ndef\n",
    "Sxyz<Esc>u",
    "  abc\ndef\n"
);

// walking the undo/redo stacks
golf!(undo_walks_back, "abcd\n", "xxxuu", "bcd\n");
golf!(undo_walks_back_to_origin, "abcd\n", "xxxuuu", "abcd\n");
golf!(undo_count, "abcd\n", "xxx2u", "bcd\n");
golf!(
    undo_count_overshoot_stops_at_oldest,
    "abc\n",
    "x9u",
    "abc\n"
);
golf!(undo_interleaved_ops, "aa\nbb\ncc\n", "xddu", "a\nbb\ncc\n");
golf!(redo_walks_forward, "abcd\n", "xxuu<C-r><C-r>", "cd\n");
golf!(redo_count, "abcd\n", "xxxuuu2<C-r>", "cd\n");
golf!(redo_with_nothing_to_redo_is_noop, "abc\n", "<C-r>", "abc\n");
golf!(new_change_clears_redo, "abcd\n", "xxurZ<C-r>", "Zcd\n");
golf!(redo_insert_session, "x\n", "habc<Esc>u<C-r>", "abcx\n");
golf!(undo_paste, "one\ntwo\n", "yypu", "one\ntwo\n");
golf!(undo_replace, "abc\n", "rZu", "abc\n");

// undo puts the cursor back at the change (checked via a follow-up rX)
golf!(
    undo_restores_cursor_charwise,
    "hello world\n",
    "wdwurX",
    "hello Xorld\n"
);
golf!(
    undo_restores_cursor_linewise,
    "one\ntwo\nthree\n",
    "GddurX",
    "one\ntwo\nXhree\n"
);
golf!(
    undo_restores_cursor_column_mid_line,
    "one\ntwo\nthree\n",
    "kllddurX",
    "one\ntwX\nthree\n"
);

// dot repeats simple change commands
golf!(dot_repeats_r, "aaaa\n", "rbl.", "bbaa\n");
golf!(dot_repeats_tilde, "abcd\n", "~.", "ABcd\n");
golf!(dot_repeats_join, "a\nb\nc\n", "J.", "a b c\n");
golf!(dot_repeats_dfx, "axbxcx\n", "dfx.", "cx\n");
golf!(
    dot_repeats_shift_d,
    "abcdef\nabcdef\n",
    "3lDk.",
    "abc\nab\n"
);
golf!(dot_repeats_charwise_paste, "ab\n", "yl$p.", "abaa\n");
golf!(
    dot_repeats_linewise_paste,
    "one\ntwo\n",
    "yyp.",
    "one\none\none\ntwo\n"
);

// dot repeats whole insert sessions
golf!(dot_repeats_o, "a\n", "oxy<Esc>.", "a\nxy\nxy\n");
golf!(dot_repeats_shift_o, "b\n", "Oa<Esc>.", "a\na\nb\n");
golf!(
    dot_repeats_shift_a,
    "ab\ncdef\n",
    "A!<Esc>k.",
    "ab!\ncdef!\n"
);
golf!(
    dot_repeats_shift_h,
    "  ab\n  cd\n",
    "$HX<Esc>k.",
    "  Xab\n  Xcd\n"
);
golf!(
    dot_repeats_insert_with_newline,
    "one\ntwo\n",
    "A end<CR>x<Esc>k.",
    "one end\nx\ntwo end\nx\n"
);
golf!(
    dot_repeats_insert_with_backspace,
    "z\n",
    "hab<BS>c<Esc>.",
    "aaccz\n"
);
golf!(dot_repeats_s, "abc abc\n", "sX<Esc>w.", "Xbc Xbc\n");

// dot repeat: counts, position, and repetition
golf!(dot_repeats_embedded_count, "abcdef\n", "2x.", "ef\n");
golf!(dot_applies_at_new_cursor, "abc\nabc\n", "xk.", "bc\nbc\n");
golf!(dot_repeated_thrice, "abcdef\n", "x...", "ef\n");
golf!(
    undo_after_dot_undoes_only_the_repeat,
    "a\nb\nc\n",
    "dd.u",
    "b\nc\n"
);

// what dot must not do
golf!(dot_with_no_prior_change_is_noop, "abc\n", ".", "abc\n");
golf!(motion_alone_is_not_recorded, "abc def\n", "w.", "abc def\n");
golf!(dot_skips_intervening_motion, "abcd\n", "xl.", "bd\n");
golf!(dot_ignores_undo, "abcd\n", "xu.", "bcd\n");
golf!(dot_ignores_redo, "abcd\n", "xu<C-r>.", "cd\n");
golf!(
    dot_ignores_colon_command,
    "a\nb\nc\n",
    "x:2<CR>.",
    "\n\nc\n"
);

// vim-verified fine points
golf!(count_on_dot_overrides_count, "abcdef\n", "x3.", "ef\n");
golf!(empty_insert_becomes_the_dot, "abcd\n", "xh<Esc>.", "bcd\n");
golf!(
    undo_of_append_puts_cursor_on_last_char,
    "abc\n",
    "A!!<Esc>urX",
    "abX\n"
);
golf!(
    undo_of_subst_line_puts_cursor_on_first_non_blank,
    "  abc\ndef\n",
    "Sxyz<Esc>urX",
    "  Xbc\ndef\n"
);
