//! Blockwise operations against vim's rules (#112), every expectation
//! measured with headless nvim at tabstop=4: a ragged paste is padded to
//! the block's width wherever text follows, `$` reaches each row's own
//! end, a tab under the paste column is split, the cursor lands on the
//! pasted block's top-left. Layout: i=up, k=down, j=left, l=right.

use unei::editor::testing::{editor_from, feed, text};

macro_rules! golf {
    ($name:ident, $before:expr, $keys:expr, $after:expr) => {
        #[test]
        fn $name() {
            let mut ed = editor_from($before);
            feed(&mut ed, $keys);
            assert_eq!(text(&ed), $after);
        }
    };
}

// -- padding keeps a ragged block rectangular ------------------------------

golf!(
    ragged_paste_pads_short_rows_where_text_follows,
    "AAAx\nB\nCCCx\n11zz\n22zz\n33zz\n",
    "gg0<C-v>2klly3klp",
    "AAAx\nB\nCCCx\n11AAAzz\n22B  zz\n33CCCzz\n"
);
golf!(
    empty_row_in_a_block_still_holds_its_width,
    "AAAA\nx\nCCCC\n11zz\n22zz\n33zz\n",
    "gg0ll<C-v>2kly3k0lp",
    "AAAA\nx\nCCCC\n11AAzz\n22  zz\n33CCzz\n"
);
golf!(
    ragged_block_cut_pastes_back_aligned,
    "AAAx\nB\nCCCx\n11zz\n22zz\n33zz\n",
    "gg0<C-v>2klld3kl<Space>p",
    "x\n\nx\n11AAAzz\n22B  zz\n33CCCzz\n"
);
golf!(
    no_padding_where_nothing_follows,
    "AAAx\nB\nCCCx\n11\n22\n33zz\n",
    "gg0<C-v>2klly3klp",
    "AAAx\nB\nCCCx\n11AAA\n22B\n33CCCzz\n"
);
golf!(
    deliberate_difference_empty_row_onto_short_target_leaves_no_trailing_spaces,
    // vim writes "2   " here; unei never leaves trailing whitespace
    "AAAA\nx\nCCCC\n11zz\n2\n33zz\n",
    "gg0ll<C-v>2kly3k0lp",
    "AAAA\nx\nCCCC\n11AAzz\n2\n33CCzz\n"
);
golf!(
    short_targets_are_padded_up_to_the_column_and_missing_lines_created,
    "XX\nYY\nZZ\nlast\n",
    "gg0<C-v>2kly3kp",
    "XX\nYY\nZZ\nlXXast\n YY\n ZZ\n"
);
golf!(
    count_repeats_each_row_side_by_side,
    "XX\nYY\naaaa\nbbbb\n",
    "gg0<C-v>kly2k3p",
    "XX\nYY\naXXXXXXaaa\nbYYYYYYbbb\n"
);

// -- `$` reaches the end of every row --------------------------------------

golf!(
    dollar_block_yank_takes_each_row_to_its_own_end,
    "abcdef\nab\n\n\n",
    "gg0<C-v>k$y2kp",
    "abcdef\nab\nabcdef\nab\n"
);
golf!(
    dollar_block_is_as_wide_as_its_widest_row_when_pasted_mid_line,
    "abcdef\nab\n11zz\n22zz\n",
    "gg0<C-v>k$y2klp",
    "abcdef\nab\n11abcdefzz\n22ab    zz\n"
);
golf!(
    dollar_block_delete_clears_each_row_to_its_end,
    "abcdef\nab\nX\n",
    "gg0<C-v>k$d",
    "\n\nX\n"
);
golf!(
    dollar_block_delete_then_paste,
    "abcdef\nab\n11zz\n22zz\n",
    "gg0<C-v>k$d2kl<Space>p",
    "\n\n11abcdefzz\n22ab    zz\n"
);
golf!(
    dollar_block_change_replaces_each_row_to_its_end,
    "abcdef\nab\nX\n",
    "gg0<C-v>k$cQ<Esc>",
    "Q\nQ\nX\n"
);

#[test]
fn dollar_marks_the_selection_as_reaching_line_ends_until_a_horizontal_move() {
    let mut ed = editor_from("abcdef\nab\n");
    feed(&mut ed, "<C-v>k$");
    assert!(ed.block_to_line_end());
    feed(&mut ed, "j"); // left: a real column again
    assert!(!ed.block_to_line_end());
}

// -- the paste column inside a wider grapheme -------------------------------

golf!(
    paste_inside_a_tab_splits_it_into_spaces,
    "XX\nYY\nab zz\n\tzz\n",
    "gg0<C-v>kly2kllP",
    "XX\nYY\nabXX zz\n  YY  zz\n"
);
golf!(
    paste_inside_a_wide_char_pads_before_it_and_pushes_it_right,
    "XX\nYY\nab zz\n日本zz\n",
    "gg0<C-v>kly2klP",
    "XX\nYY\naXXb zz\n YY日本zz\n"
);
golf!(
    paste_after_a_wide_char,
    "XX\nYY\n日本zz\n日本zz\n",
    "gg0<C-v>kly2klp",
    "XX\nYY\n日本XXzz\n日本YYzz\n"
);

// -- cursor, repeat, undo, and the drawing that started it -------------------

golf!(
    cursor_lands_on_the_top_left_of_the_pasted_block,
    "XX\nYY\naaaa\nbbbb\n",
    "gg0<C-v>kly2klpx",
    "XX\nYY\naaXaa\nbbYYbb\n"
);
golf!(
    cursor_after_capital_p_too,
    "XX\nYY\naaaa\nbbbb\n",
    "gg0<C-v>kly2klPx",
    "XX\nYY\naXaaa\nbYYbbb\n"
);
golf!(
    dot_repeats_a_block_paste_from_the_new_cursor,
    "XX\nYY\naaaa\nbbbb\n",
    "gg0<C-v>kly2kp.",
    "XX\nYY\naXXXXaaa\nbYYYYbbb\n"
);
golf!(
    block_paste_is_one_undo_step,
    "XX\nYY\naaaa\nbbbb\n",
    "gg0<C-v>kly2kpu",
    "XX\nYY\naaaa\nbbbb\n"
);
golf!(
    a_box_drawing_moves_as_a_block,
    "┌──┐ a\n│1 │ b\n│2 │ c\n└──┘ d\n......\n......\n......\n......\n",
    "gg0<C-v>3kllly4klp",
    "┌──┐ a\n│1 │ b\n│2 │ c\n└──┘ d\n..┌──┐....\n..│1 │....\n..│2 │....\n..└──┘....\n"
);
golf!(
    the_right_half_of_a_box_pastes_aligned_after_multibyte_cells,
    "┌──┐ a\n│1 │ b\n└──┘ c\nxxxxxx\nxxxxxx\nxxxxxx\n",
    "gg0ll<C-v>2kly3k0lp", // p after col 1: the half lands at cell 2
    "┌──┐ a\n│1 │ b\n└──┘ c\nxx─┐xxxx\nxx │xxxx\nxx─┘xxxx\n"
);
