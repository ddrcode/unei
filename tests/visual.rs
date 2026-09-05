//! Visual mode (ticket #11): char, line, and block selections.
//! Layout reminder: i=up, k=down, j=left, l=right; h is a LEFT motion in
//! visual mode (the insert remap is normal-mode-only).

use tailored::editor::Mode;
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

// entry, exit, kind switching
#[test]
fn visual_modes_enter_and_exit() {
    use tailored::core::commands::VisualKind;
    let mut ed = editor_from("abc\n");
    feed(&mut ed, "v");
    assert_eq!(ed.mode, Mode::Visual(VisualKind::Char));
    feed(&mut ed, "V");
    assert_eq!(ed.mode, Mode::Visual(VisualKind::Line));
    feed(&mut ed, "<C-v>");
    assert_eq!(ed.mode, Mode::Visual(VisualKind::Block));
    feed(&mut ed, "<C-v>"); // same kind again leaves
    assert_eq!(ed.mode, Mode::Normal);
    feed(&mut ed, "v<Esc>");
    assert_eq!(ed.mode, Mode::Normal);
    feed(&mut ed, "v<C-c>");
    assert_eq!(ed.mode, Mode::Normal);
}

// charwise selections
golf!(vd_deletes_inclusive, "abcdef\n", "vlld", "def\n");
golf!(vd_backwards_same_result, "abcdef\n", "llvjjd", "def\n");
golf!(vh_is_left_motion, "abcdef\n", "llvhd", "adef\n");
golf!(vy_then_paste, "abc\n", "vly$p", "abcab\n");
golf!(v_dollar_deletes_to_eol, "hello world\n", "wv$d", "hello \n");
golf!(
    vc_changes_selection,
    "hello world\n",
    "vec-hi<Esc>",
    "-hi world\n"
);
golf!(v_x_aliases_delete, "abcd\n", "vlx", "cd\n");
golf!(v_s_aliases_change, "abcd\n", "vlsXY<Esc>", "XYcd\n");
golf!(v_tilde_toggles, "aBcD\n", "vll~", "AbCD\n");
golf!(vw_spans_words, "one two three\n", "vwd", "wo three\n");
golf!(v_multiline_char_delete, "abc\ndef\nghi\n", "lvkkld", "a\n");
golf!(v_find_motion, "abcxdef\n", "vfxd", "def\n");
golf!(v_count_motion, "abcdef\n", "v3ld", "ef\n");

// o swaps ends
golf!(v_o_extends_backward, "abcdef\n", "llvo0d", "def\n");

// linewise
golf!(
    big_v_deletes_line,
    "one\ntwo\nthree\n",
    "Vd",
    "two\nthree\n"
);
golf!(big_v_spans_lines, "one\ntwo\nthree\n", "Vkd", "three\n");
golf!(big_v_yank_paste, "one\ntwo\n", "Vy}p", "one\ntwo\none\n");
golf!(
    big_v_change_keeps_indent,
    "  old stuff\nrest\n",
    "Vcnew<Esc>",
    "  new\nrest\n"
);
golf!(v_tilde_lines, "ab\ncd\n", "Vk~", "AB\nCD\n");

// gv reselect
golf!(gv_reselects, "abcdef\n", "vll<Esc>gvd", "def\n");

// blockwise
golf!(
    block_delete_rect,
    "abcd\nefgh\nijkl\n",
    "l<C-v>kkld",
    "ad\neh\nil\n"
);
golf!(
    block_delete_ragged,
    "abcdef\nab\nabcdef\n",
    "ll<C-v>kkld",
    "abef\nab\nabef\n"
);
golf!(
    block_yank_paste,
    "12ab\n34cd\n",
    "<C-v>kly0P",
    "1212ab\n3434cd\n"
);
golf!(
    block_change_replicates,
    "foo_a\nfoo_b\nfoo_c\n",
    "<C-v>kkllcbar<Esc>",
    "bar_a\nbar_b\nbar_c\n"
);
golf!(
    block_change_skips_short_lines,
    "aaaa\nz\naaaa\n",
    "ll<C-v>kkcXX<Esc>",
    "aaXXa\nz\naaXXa\n"
);
golf!(block_tilde, "abc\ndef\n", "<C-v>kl~", "ABc\nDEf\n");

// visual paste replaces without clobbering the register (author preference)
golf!(
    v_paste_keeps_register,
    "one two\n",
    "veywvepbvep",
    "one one\n"
);

#[test]
fn v_paste_repeatedly_same_content() {
    // select in several places, paste the SAME yank each time
    let mut ed = editor_from("aaa\nbbb\nccc\n");
    feed(&mut ed, "vlly"); // yank "aaa"
    feed(&mut ed, "kVp"); // replace line 2
    feed(&mut ed, "kVp"); // replace line 3 with the same content
    assert_eq!(text(&ed), "aaa\naaa\naaa\n");
}

// block paste creating and padding lines
// ragged block: the short line contributes an empty segment, pasted as nothing
golf!(
    block_paste_ragged_rows,
    "abcd\nx\nabcd\n",
    "l<C-v>kkly0P",
    "bcabcd\nx\nbcabcd\n"
);

// selection ops are one undo step
golf!(visual_ops_single_undo, "abc\ndef\n", "Vkdu", "abc\ndef\n");
golf!(
    block_change_single_undo,
    "aa\nbb\n",
    "<C-v>kcX<Esc>u",
    "aa\nbb\n"
);
