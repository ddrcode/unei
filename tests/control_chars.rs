//! Control characters on screen: a file with CRLF line ends (a Digikey cart,
//! a Lattice pinout) used to send the raw CR to the terminal, which dragged
//! the rest of every row back to column 0. Rendered through the real
//! `ui::render` into a test backend: the CR is a visible `␍`, one cell wide.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use unei::editor::Editor;
use unei::editor::testing::{editor_from, feed};

fn screen(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| unei::ui::render(f, ed)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

#[test]
fn a_carriage_return_is_a_visible_cell_not_a_cursor_move() {
    let mut ed = editor_from("Index,Qty\r\n1,2\r\n");
    let rows = screen(&mut ed, 40, 6);
    assert!(rows[0].contains("Index,Qty␍"), "{:?}", rows[0]);
    assert!(rows[1].contains("1,2␍"), "{:?}", rows[1]);
    assert!(!rows[0].contains('\r') && !rows[1].contains('\r'));
}

#[test]
fn the_cursor_lands_on_the_control_picture_at_line_end() {
    // `$` goes to the last char of the line — the CR — and the screen cell
    // under it is its picture, so the geometry agrees with the text
    let mut ed = editor_from("ab\r\n");
    feed(&mut ed, "$");
    assert_eq!(ed.cursor.col, 2);
    let rows = screen(&mut ed, 20, 4);
    let cells: Vec<char> = rows[0].chars().collect();
    let pos = cells.iter().position(|&c| c == '␍').expect("picture drawn");
    assert_eq!(&cells[pos - 2..pos], &['a', 'b']);
}
