//! Soft wrap (#9): long lines fold into several display rows at word
//! boundaries, and the cursor/scroll model counts rows, not lines.
//! Layout reminder: i=up, k=down, j=left, l=right.

use unei::editor::testing::{editor_from, feed};

/// A 10-cell-wide, 6-row window: `scrolloff` clamps to 2 here.
fn narrow(text: &str) -> unei::editor::Editor {
    let mut ed = editor_from(text);
    ed.set_view(10, 6);
    ed
}

#[test]
fn cursor_inside_a_wrapped_line_lands_on_its_row() {
    // width 10 wraps "aaaa bbbb cccc dddd" into "aaaa bbbb " | "cccc dddd"
    let mut ed = narrow("aaaa bbbb cccc dddd\n");
    feed(&mut ed, "10l"); // col 10 = the 'c' opening the second row
    assert_eq!(ed.cursor.col, 10);
    assert_eq!(ed.cursor_display_pos(), (1, 0), "second row, first cell");
    feed(&mut ed, "0"); // back to col 0
    assert_eq!(ed.cursor_display_pos(), (0, 0));
    feed(&mut ed, "$"); // last char: 'd' at col 18 → row 1, cell 8
    assert_eq!(ed.cursor_display_pos(), (1, 8));
}

#[test]
fn scrolling_counts_wrapped_rows_so_the_cursor_stays_visible() {
    // line 1 wraps to three rows at width 10
    let mut ed = narrow("short\nalpha beta gamma delta epsilon\nx\ny\nz\nw\n");
    feed(&mut ed, "G"); // to the last line
    let (row, _) = ed.cursor_display_pos();
    assert!(row < 6, "cursor row {row} is off the 6-row window");
    // the top was chosen by rows that *fit*: line 2 (3 rows before the
    // cursor's), not line 1 (whose 3 rows would push the cursor off)
    assert_eq!(ed.top_line, 2);
    assert_eq!(row, 3);
    // and back up: the wrapped line scrolls into view with its margin
    feed(&mut ed, "gg");
    assert_eq!(ed.top_line, 0);
    assert_eq!(ed.cursor_display_pos(), (0, 0));
}

#[test]
fn zz_centers_the_display_row_not_the_line() {
    let mut ed = narrow("a\nb\nc\nd\nalpha beta gamma delta\nlast\n");
    feed(&mut ed, "Gzz"); // cursor on "last"; line 4 above wraps to 3 rows
    // h/2 = 3 rows above wanted: line 4's three rows fit exactly; adding
    // line 3 would make four → the top is line 4, the cursor on row 3
    assert_eq!(ed.top_line, 4);
    assert_eq!(ed.cursor_display_pos().0, 3);
    // zt honours scrolloff (2 rows) like vim: the only line above is 3 rows
    // tall, so the margin overshoots to 3 rather than dropping below 2
    feed(&mut ed, "zt");
    assert_eq!(ed.top_line, 4);
    assert_eq!(ed.cursor_display_pos().0, 3);
}

#[test]
fn zt_and_zb_place_the_row_on_an_unwrapped_buffer() {
    let mut ed = editor_from(&"line\n".repeat(40));
    ed.set_view(80, 11); // scrolloff clamps to 5
    feed(&mut ed, "20kzt");
    assert_eq!(
        ed.cursor_display_pos().0,
        5,
        "zt: cursor sits scrolloff rows down"
    );
    feed(&mut ed, "zb");
    assert_eq!(
        ed.cursor_display_pos().0,
        5,
        "zb: scrolloff from the bottom of 11 rows"
    );
    feed(&mut ed, "zz");
    assert_eq!(ed.cursor_display_pos().0, 5);
}

#[test]
fn unwrapped_buffers_behave_exactly_as_before() {
    let mut ed = editor_from(&"line\n".repeat(50));
    ed.set_view(80, 10); // scrolloff clamps to 4
    feed(&mut ed, "20k");
    assert_eq!(ed.cursor.line, 20);
    // one row per line: cursor sits 4 rows above the bottom, top = 20-5
    assert_eq!(ed.top_line, 15);
    assert_eq!(ed.cursor_display_pos(), (5, 0));
}
