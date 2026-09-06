//! `Ctrl+A` / `Ctrl+X` increment-decrement and `gJ` (join without a space) — #14.
//! Layout reminder: i=up, k=down, j=left, l=right, h=insert.

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

// increment / decrement
golf!(ctrl_a_increments, "41\n", "<C-a>", "42\n");
golf!(ctrl_x_decrements, "43\n", "<C-x>", "42\n");
golf!(ctrl_a_with_count, "10\n", "5<C-a>", "15\n");
golf!(ctrl_a_hex_grows, "0xff\n", "<C-a>", "0x100\n");
golf!(ctrl_a_dollar_hex_uppercase, "$FE\n", "<C-a>", "$FF\n");
golf!(
    ctrl_a_number_after_cursor,
    "val = 7\n",
    "<C-a>",
    "val = 8\n"
);
golf!(ctrl_x_crosses_zero, "0\n", "<C-x>", "-1\n");
golf!(ctrl_a_noop_without_number, "hello\n", "<C-a>", "hello\n");
golf!(ctrl_a_dot_repeats, "1\n", "<C-a>..", "4\n");

// gJ — join without a space (vs J which inserts one)
golf!(join_gj_no_space, "foo\nbar\n", "gJ", "foobar\n");
golf!(join_j_adds_space, "foo\nbar\n", "J", "foo bar\n");
golf!(join_gj_keeps_indent, "foo\n    bar\n", "gJ", "foo    bar\n");
golf!(join_gj_count_three, "a\nb\nc\n", "3gJ", "abc\n");
