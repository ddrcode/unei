//! Split windows (ticket #4): focus, resize, layout ops, zoom, closing.
//! Layout reminder: i=up, k=down, j=left, l=right; window chord is
//! Ctrl+w or <Space>w.

use unei::editor::Editor;
use unei::editor::testing::{editor_from, editor_with_buffers, feed, text};

fn rect_of(ed: &Editor, id: usize) -> ratatui::layout::Rect {
    ed.window_rects()
        .into_iter()
        .find(|(w, _)| *w == id)
        .map(|(_, r)| r)
        .expect("window exists")
}

#[test]
fn starts_with_single_window() {
    let ed = editor_from("a\n");
    assert_eq!(ed.window_count(), 1);
    assert_eq!(ed.focused_window_id(), 1);
}

#[test]
fn split_h_focuses_new_below() {
    let mut ed = editor_from("hello\n");
    feed(&mut ed, "<C-w>s");
    assert_eq!(ed.window_count(), 2);
    let new = ed.focused_window_id();
    assert_ne!(new, 1);
    // splitbelow: the new window sits below the old one
    assert!(rect_of(&ed, new).y > rect_of(&ed, 1).y);
    // both show the same buffer at the same position
    assert_eq!(text(&ed), "hello\n");
}

#[test]
fn split_v_focuses_new_right() {
    let mut ed = editor_from("hello\n");
    feed(&mut ed, "<C-w>v");
    let new = ed.focused_window_id();
    assert!(rect_of(&ed, new).x > rect_of(&ed, 1).x);
}

#[test]
fn leader_w_is_the_same_chord() {
    let mut ed = editor_from("x\n");
    feed(&mut ed, " ws");
    assert_eq!(ed.window_count(), 2);
}

#[test]
fn x_is_an_alias_of_s() {
    let mut ed = editor_from("x\n");
    feed(&mut ed, "<C-w>x");
    assert_eq!(ed.window_count(), 2);
}

#[test]
fn focus_moves_with_ijkl() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>s"); // focused: new window below
    let below = ed.focused_window_id();
    feed(&mut ed, "<C-w>i"); // up
    assert_eq!(ed.focused_window_id(), 1);
    feed(&mut ed, "<C-w>k"); // down
    assert_eq!(ed.focused_window_id(), below);

    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let right = ed.focused_window_id();
    feed(&mut ed, "<C-w>j"); // left
    assert_eq!(ed.focused_window_id(), 1);
    feed(&mut ed, "<C-w>l"); // right
    assert_eq!(ed.focused_window_id(), right);
}

#[test]
fn focus_at_edge_stays_put() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v<C-w>j"); // back to window 1 (leftmost)
    feed(&mut ed, "<C-w>j");
    assert_eq!(ed.focused_window_id(), 1);
}

#[test]
fn same_buffer_two_windows_share_edits() {
    let mut ed = editor_from("shared\n");
    feed(&mut ed, "<C-w>sxx"); // edit in the new window
    assert_eq!(text(&ed), "ared\n");
    feed(&mut ed, "<C-w>i"); // other window, same buffer
    assert_eq!(text(&ed), "ared\n");
    assert_eq!(ed.buffer_count(), 1);
}

#[test]
fn windows_keep_their_own_cursors() {
    let mut ed = editor_from("one\ntwo\nthree\n");
    feed(&mut ed, "G"); // cursor line 2
    feed(&mut ed, "<C-w>s"); // new window inherits the position
    feed(&mut ed, "gg"); // move only in the new window
    assert_eq!(ed.cursor.line, 0);
    feed(&mut ed, "<C-w>i");
    assert_eq!(ed.cursor.line, 2); // old window unchanged
}

#[test]
fn split_new_creates_empty_buffer() {
    let mut ed = editor_from("content\n");
    feed(&mut ed, "<C-w>n");
    assert_eq!(ed.window_count(), 2);
    assert_eq!(ed.buffer_count(), 2);
    assert_eq!(text(&ed), "\n"); // fresh empty buffer is focused
}

#[test]
fn close_window_returns_to_neighbor() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>s<C-w>q");
    assert_eq!(ed.window_count(), 1);
    assert_eq!(ed.focused_window_id(), 1);
    assert!(!ed.should_quit); // buffer still open in the remaining window
}

#[test]
fn close_last_window_quits() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>q");
    assert!(ed.should_quit);
}

#[test]
fn close_last_window_checks_modified() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "x<C-w>q");
    assert!(!ed.should_quit);
    assert!(ed.message.as_ref().unwrap().text.contains("E37"));
}

#[test]
fn only_window_closes_the_rest() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>s<C-w>v"); // three windows
    assert_eq!(ed.window_count(), 3);
    let focused = ed.focused_window_id();
    feed(&mut ed, "<C-w>o");
    assert_eq!(ed.window_count(), 1);
    assert_eq!(ed.focused_window_id(), focused);
}

#[test]
fn q_closes_window_before_quitting() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>s:q<CR>");
    assert_eq!(ed.window_count(), 1);
    assert!(!ed.should_quit);
    feed(&mut ed, ":q<CR>");
    assert!(ed.should_quit);
}

#[test]
fn q_on_split_ignores_modified_buffer_still_visible() {
    // 'hidden' semantics: closing one of two windows never blocks
    let mut ed = editor_from("a\n");
    feed(&mut ed, "x<C-w>s:q<CR>");
    assert_eq!(ed.window_count(), 1);
    assert!(!ed.should_quit);
    // ...but the final window checks everything
    feed(&mut ed, ":q<CR>");
    assert!(!ed.should_quit);
    assert!(ed.message.as_ref().unwrap().text.contains("E37"));
}

#[test]
fn equalize_after_uneven_resize() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let before = rect_of(&ed, 1).width;
    feed(&mut ed, "<C-w><A-l>"); // grow the focused (right) window leftward border
    let grown = rect_of(&ed, ed.focused_window_id()).width;
    feed(&mut ed, "<C-w>=");
    let evened = rect_of(&ed, 1).width;
    assert_ne!(before, grown - 5, "resize moved the border"); // moved by step
    assert_eq!(evened, before);
}

#[test]
fn resize_moves_border_tmux_style() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v"); // focused = right window
    let right_before = rect_of(&ed, ed.focused_window_id()).width;
    feed(&mut ed, "<C-w><A-j>"); // push the shared border left → right grows
    let right_after = rect_of(&ed, ed.focused_window_id()).width;
    assert_eq!(right_after, right_before + 5);
    feed(&mut ed, "<C-w><A-l>"); // push it back right → right shrinks
    assert_eq!(rect_of(&ed, ed.focused_window_id()).width, right_before);
}

#[test]
fn alt_arrows_resize_like_the_letters() {
    // tmux's M-Arrow binding, and the one Alt combination macOS delivers
    // without any terminal configuration (arrows have nothing to compose)
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let right = ed.focused_window_id();
    let before = rect_of(&ed, right).width;
    feed(&mut ed, "<C-w><A-Left>");
    assert_eq!(rect_of(&ed, right).width, before + 5);
    feed(&mut ed, "<C-w><A-Right>");
    assert_eq!(rect_of(&ed, right).width, before);
    // a plain arrow in the chord still means focus, not resize
    feed(&mut ed, "<C-w><Left>");
    assert_ne!(ed.focused_window_id(), right, "focus moved left");
    assert_eq!(rect_of(&ed, right).width, before, "nothing resized");
}

#[test]
fn resize_respects_minimum_width() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    for _ in 0..40 {
        feed(&mut ed, "<C-w><A-j>"); // grow right window relentlessly
    }
    assert!(rect_of(&ed, 1).width >= 12);
}

#[test]
fn rotate_swaps_positions_keeps_focus_with_window() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let new = ed.focused_window_id();
    let (x1, xn) = (rect_of(&ed, 1).x, rect_of(&ed, new).x);
    assert!(x1 < xn);
    feed(&mut ed, "<C-w>r");
    assert!(rect_of(&ed, 1).x > rect_of(&ed, new).x); // swapped
    assert_eq!(ed.focused_window_id(), new); // focus follows the window
    let _ = (x1, xn);
}

#[test]
fn flip_layout_toggles_direction() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let new = ed.focused_window_id();
    assert_eq!(rect_of(&ed, 1).y, rect_of(&ed, new).y); // side by side
    feed(&mut ed, "<C-w><Space>");
    assert_eq!(rect_of(&ed, 1).x, rect_of(&ed, new).x); // now stacked
    assert!(rect_of(&ed, new).y > rect_of(&ed, 1).y);
    feed(&mut ed, "<C-w><Space>");
    assert_eq!(rect_of(&ed, 1).y, rect_of(&ed, new).y); // and back
}

#[test]
fn zoom_makes_focused_fullscreen_and_back() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let area = unei::editor::testing::window_area(&ed);
    feed(&mut ed, "<C-w>z");
    assert!(ed.is_zoomed());
    let rects = ed.window_rects();
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].1, area);
    feed(&mut ed, "<C-w>z");
    assert!(!ed.is_zoomed());
    assert_eq!(ed.window_rects().len(), 2);
}

#[test]
fn zoom_ends_when_focus_moves() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v<C-w>z");
    assert!(ed.is_zoomed());
    feed(&mut ed, "<C-w>j");
    assert!(!ed.is_zoomed());
    assert_eq!(ed.focused_window_id(), 1);
}

#[test]
fn zoom_on_single_window_is_noop() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>z");
    assert!(!ed.is_zoomed());
}

#[test]
fn nested_splits_layout_and_navigation() {
    // [1 | (2 over 3)]: v-split, then h-split on the right
    let mut ed = editor_from("a\n");
    feed(&mut ed, "<C-w>v");
    let two = ed.focused_window_id();
    feed(&mut ed, "<C-w>s");
    let three = ed.focused_window_id();
    assert_eq!(ed.window_count(), 3);
    assert_eq!(rect_of(&ed, two).x, rect_of(&ed, three).x);
    assert!(rect_of(&ed, three).y > rect_of(&ed, two).y);
    feed(&mut ed, "<C-w>j"); // left crosses to window 1
    assert_eq!(ed.focused_window_id(), 1);
    feed(&mut ed, "<C-w>l"); // and right again (top of the pair)
    assert_eq!(ed.focused_window_id(), two);
}

#[test]
fn windows_can_show_different_buffers() {
    let mut ed = editor_with_buffers(&[("a.txt", "aaa\n"), ("b.txt", "bbb\n")]);
    feed(&mut ed, "<C-w>v"); // both windows show a.txt
    feed(&mut ed, " b<C-k><CR>"); // focused window switches to b.txt
    assert_eq!(text(&ed), "bbb\n");
    feed(&mut ed, "<C-w>j");
    assert_eq!(text(&ed), "aaa\n"); // other window kept a.txt
    assert_eq!(ed.window_count(), 2);
}

#[test]
fn alternate_is_per_window() {
    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")]);
    feed(&mut ed, " b<C-k><CR>"); // window 1: b.txt (alternate a)
    feed(&mut ed, "<C-w>v"); // window 2: b.txt, no alternate of its own
    feed(&mut ed, " b<C-k><C-k><CR>"); // window 2: c.txt (alternate b)
    feed(&mut ed, "<C-^>");
    assert_eq!(text(&ed), "b\n"); // window 2's alternate
    feed(&mut ed, "<C-w>j<C-^>");
    assert_eq!(text(&ed), "a\n"); // window 1's alternate survived
}

#[test]
fn closing_buffer_shown_in_another_window_reassigns_it() {
    let mut ed = editor_with_buffers(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    feed(&mut ed, "<C-w>v"); // both show a.txt
    feed(&mut ed, " b<C-k><CR>"); // focused: b.txt
    feed(&mut ed, "<C-w>j"); // focus window 1 (a.txt)
    feed(&mut ed, ":bd<CR>"); // close a.txt
    assert_eq!(ed.buffer_count(), 1);
    assert_eq!(text(&ed), "b\n");
    feed(&mut ed, "<C-w>l"); // the other window was showing a.txt too
    assert_eq!(text(&ed), "b\n"); // reassigned, not dangling
}

#[test]
fn split_refuses_when_too_small() {
    let mut ed = editor_from("a\n");
    for _ in 0..12 {
        feed(&mut ed, "<C-w>v"); // keep splitting until it can't fit
    }
    assert!(ed.window_count() < 12);
    // the last refusal left a message
    assert!(ed.message.as_ref().is_some_and(|m| m.text.contains("E36")));
}

#[test]
fn window_chord_cancels_cleanly_on_unknown_key() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, "<C-w>Zx"); // Z is not a window command; x then deletes
    assert_eq!(ed.window_count(), 1);
    assert_eq!(text(&ed), "bc\n");
}

/// What `ui::render` does before drawing: hand the focused window its true
/// text dimensions. The event loop draws after every key, so scroll math
/// always runs against dimensions set by a previous frame.
fn render_pass(ed: &mut Editor) {
    ed.set_window_area(unei::editor::testing::window_area(ed));
    let rects = ed.window_rects();
    let focused = ed.focused_window_id();
    if let Some((_, r)) = rects.iter().find(|(id, _)| *id == focused) {
        let lines = unei::core::text::text_lines(&ed.buffer.rope);
        let gutter = unei::config::gutter_width(lines);
        ed.set_view(
            r.width.saturating_sub(gutter) as usize,
            r.height.saturating_sub(1) as usize,
        );
    }
}

#[test]
fn focus_switch_preserves_scroll_position() {
    // the reported bug: two columns, left column split horizontally, long
    // file in the right column - switching away and back lost the scroll
    let long = (1..=60).map(|i| format!("line {i}\n")).collect::<String>();
    let mut ed = editor_with_buffers(&[("short.txt", "a\nb\n"), ("long.txt", &long)]);
    feed(&mut ed, "<C-w>v"); // right column, focused
    render_pass(&mut ed);
    let right = ed.focused_window_id();
    feed(&mut ed, " b<C-k><CR>"); // open the long file there
    feed(&mut ed, "30G"); // scroll somewhere below the top
    render_pass(&mut ed);
    let (line, top) = (ed.cursor.line, ed.top_line);
    assert!(top > 0, "top of the file must be scrolled out");

    feed(&mut ed, "<C-w>j"); // to the left column
    render_pass(&mut ed);
    feed(&mut ed, "<C-w>s"); // split it horizontally (short bottom pane)
    render_pass(&mut ed); // the frame that used to plant stale dimensions
    feed(&mut ed, "kik"); // wiggle around in the small pane
    render_pass(&mut ed);
    feed(&mut ed, "<C-w>l"); // and back to the right column
    render_pass(&mut ed);
    assert_eq!(ed.focused_window_id(), right);
    assert_eq!(ed.cursor.line, line, "cursor position survives");
    assert_eq!(ed.top_line, top, "scroll position survives");
}

#[test]
fn focus_switch_between_horizontal_splits_keeps_top_line() {
    // the off-by-one variant: sibling h-splits with slightly different heights
    let long = (1..=60).map(|i| format!("l{i}\n")).collect::<String>();
    let mut ed = editor_from(&long);
    feed(&mut ed, "<C-w>s"); // bottom window focused, same buffer
    render_pass(&mut ed);
    feed(&mut ed, "40G");
    render_pass(&mut ed);
    let top = ed.top_line;
    feed(&mut ed, "<C-w>i");
    render_pass(&mut ed);
    feed(&mut ed, "<C-w>k"); // up and straight back
    render_pass(&mut ed);
    assert_eq!(ed.top_line, top);
}

#[test]
fn zoom_rescrolls_for_the_larger_viewport() {
    let long = (1..=60).map(|i| format!("z{i}\n")).collect::<String>();
    let mut ed = editor_from(&long);
    feed(&mut ed, "<C-w>s30G");
    render_pass(&mut ed);
    let small_top = ed.top_line;
    feed(&mut ed, "<C-w>z"); // zoom: taller viewport, scrolloff re-applies
    render_pass(&mut ed);
    assert!(ed.is_zoomed());
    assert!(ed.top_line <= small_top);
    assert_eq!(ed.cursor.line, 29); // cursor itself never moves
}
