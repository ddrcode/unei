//! CSV preview (ticket #39 phase 3): the grid through the editor — `gp`,
//! column scrolling, follow and jump-to-source — and, through the real
//! `ui::render`, the pinned header and the column counter.

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::feed;

/// A JLCPCB-style bill of materials: byte-order mark, every field quoted,
/// a CJK manufacturer name, an empty price.
const BOM: &str = "\u{feff}\"No.\",\"Quantity\",\"Designator\",\"Value\",\"Manufacturer\",\"LCSC Price\"\n\
\"1\",\"1\",\"C1\",\"10uF\",\"SAMSUNG(三星)\",\"0.0159\"\n\
\"2\",\"7\",\"C4,C5,C6,C7,C8,C9,C10\",\"100nF\",\"SAMSUNG(三星)\",\"0.0014\"\n\
\"3\",\"2\",\"R1,R2\",\"4k7\",\"UNI-ROYAL\",\"\"\n";

fn csv_editor(name: &str, content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(name));
    let mut ed = Editor::new(b);
    ed.set_view(80, 22);
    ed
}

fn preview_text(ed: &mut Editor, width: usize) -> Vec<String> {
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, width);
    (0..doc.line_count()).map(|i| doc.plain(i)).collect()
}

fn screen(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| unei::ui::render(f, ed)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

#[test]
fn gp_projects_a_bom_as_the_spreadsheet_shows_it() {
    let mut ed = csv_editor("pcb/bom.csv", BOM);
    feed(&mut ed, "gp");
    assert!(ed.focused_is_preview(), "csv has a projection");
    let lines = preview_text(&mut ed, 120);
    assert_eq!(
        lines[0],
        format!(
            "1  No. │ Quantity │ {:<21} │ Value │ {:<13} │ LCSC Price",
            "Designator", "Manufacturer"
        ),
        "byte-order mark gone, columns at their natural width"
    );
    assert!(
        lines[1].contains("─┼─"),
        "rule under the header: {:?}",
        lines[1]
    );
    assert_eq!(
        lines[2],
        format!(
            "2    1 │ {:>8} │ {:<21} │ {:<5} │ SAMSUNG(三星) │ {:>10}",
            "1", "C1", "10uF", "0.0159"
        ),
        "numbers right, text left, CJK measured in cells"
    );
    assert_eq!(
        lines[3],
        format!(
            "3    2 │ {:>8} │ C4,C5,C6,C7,C8,C9,C10 │ 100nF │ SAMSUNG(三星) │ {:>10}",
            "7", "0.0014"
        )
    );
    assert_eq!(
        lines[4],
        format!(
            "4    3 │ {:>8} │ {:<21} │ {:<5} │ {:<13} │",
            "2", "R1,R2", "4k7", "UNI-ROYAL"
        ),
        "an empty last cell keeps its grid line and nothing after it"
    );
    assert_eq!(lines.len(), 5);
}

#[test]
fn the_delimiter_is_sniffed_and_a_tsv_is_tab() {
    let mut ed = csv_editor("parts.tsv", "a\tb\n1\t2\n");
    assert_eq!(preview_text(&mut ed, 60)[0], "1  a │ b");
    let mut ed = csv_editor("excel-export.csv", "a;b\n1;2\n");
    assert_eq!(preview_text(&mut ed, 60)[0], "1  a │ b");
    let mut ed = csv_editor("plain.csv", "a,b\n1,2\n");
    assert_eq!(preview_text(&mut ed, 60)[2], "2  1 │ 2");
}

#[test]
fn columns_scroll_with_j_and_l_and_the_position_survives_the_toggle() {
    let mut ed = csv_editor("pcb/bom.csv", BOM);
    feed(&mut ed, "gp");
    let id = ed.focused_window_id();
    let nav = ed.preview_nav_state(id);
    feed(&mut ed, "l");
    assert_eq!(
        ed.preview_nav_state(id),
        nav,
        "l moves columns, not the reading line"
    );
    let lines = preview_text(&mut ed, 120);
    assert!(
        lines[0].starts_with("1 ‹Quantity │ Designator"),
        "{}",
        lines[0]
    );
    assert!(lines[2].starts_with("2 ‹       1 │ C1"), "{}", lines[2]);
    feed(&mut ed, "ll");
    assert!(preview_text(&mut ed, 120)[0].starts_with("1 ‹Value │"));
    feed(&mut ed, "$");
    assert_eq!(preview_text(&mut ed, 120)[0], "1 ‹LCSC Price");
    feed(&mut ed, "l");
    assert_eq!(
        preview_text(&mut ed, 120)[0],
        "1 ‹LCSC Price",
        "the last column stays"
    );
    feed(&mut ed, "j");
    assert!(preview_text(&mut ed, 120)[0].starts_with("1 ‹Manufacturer"));
    feed(&mut ed, "0");
    assert!(preview_text(&mut ed, 120)[0].starts_with("1  No. │"));
    feed(&mut ed, "jj");
    assert!(
        preview_text(&mut ed, 120)[0].starts_with("1  No. │"),
        "the first column stays"
    );
    // leave and come back: still scrolled where it was
    feed(&mut ed, "ll<Esc>");
    assert!(!ed.focused_is_preview());
    feed(&mut ed, "gp");
    assert!(preview_text(&mut ed, 120)[0].starts_with("1 ‹Designator"));
}

#[test]
fn rows_follow_the_cursor_and_enter_jumps_to_the_record() {
    let mut ed = csv_editor("pcb/bom.csv", BOM);
    feed(&mut ed, "kk"); // the 100nF record, source line 2
    feed(&mut ed, "gp");
    let id = ed.focused_window_id();
    let (line, _) = ed.preview_nav_state(id);
    assert_eq!(line, 3, "header, rule, 10uF, 100nF");
    feed(&mut ed, "k<CR>"); // one row down, then jump
    assert!(!ed.focused_is_preview());
    assert_eq!(ed.cursor.line, 3, "landed on the 4k7 record");
}

#[test]
fn markdown_and_rust_projections_do_not_scroll_by_column() {
    let mut ed = csv_editor("doc.md", "# T\n\ntext\n");
    feed(&mut ed, "gp");
    let id = ed.focused_window_id();
    let nav = ed.preview_nav_state(id);
    feed(&mut ed, "l$j0");
    assert_eq!(ed.preview_nav_state(id), nav);
    assert!(ed.focused_is_preview());
}

#[test]
fn the_header_stays_pinned_while_the_body_scrolls() {
    let mut csv = String::from("Pin,Bank,Function\n");
    for i in 1..=40 {
        csv.push_str(&format!("P{i},{},IO{i}\n", i % 8));
    }
    let mut ed = csv_editor("pinout.csv", &csv);
    feed(&mut ed, "gp");
    screen(&mut ed, 60, 12); // the app sizes the view from the terminal before any key
    feed(&mut ed, "G"); // read the last row
    let rows = screen(&mut ed, 60, 12);
    assert!(
        rows[0].contains("Pin │ Bank │ Function"),
        "pinned header:\n{}",
        rows.join("\n")
    );
    assert!(rows[1].contains('┼'), "pinned rule: {:?}", rows[1]);
    assert!(
        rows.iter().any(|r| r.contains("P40")),
        "the last row is on screen:\n{}",
        rows.join("\n")
    );
    assert!(
        !rows.iter().any(|r| r.contains("P1  │")),
        "the first record scrolled away under the header:\n{}",
        rows.join("\n")
    );
    // back at the top the header is simply the first row — nothing doubled
    feed(&mut ed, "g");
    let rows = screen(&mut ed, 60, 12);
    assert!(rows[0].contains("Pin │ Bank │ Function"));
    assert!(rows[2].contains("P1  │"), "{:?}", rows[2]);
    assert_eq!(rows.iter().filter(|r| r.contains("Pin │ Bank")).count(), 1);
}

#[test]
fn the_status_line_counts_columns() {
    let mut ed = csv_editor("pcb/bom.csv", BOM);
    feed(&mut ed, "gp");
    let rows = screen(&mut ed, 120, 10);
    let status = rows
        .iter()
        .find(|r| r.contains("PREVIEW"))
        .expect("focused preview badge");
    assert!(status.contains("col 1/6"), "{status:?}");
    feed(&mut ed, "l");
    let rows = screen(&mut ed, 120, 10);
    let status = rows.iter().find(|r| r.contains("PREVIEW")).unwrap();
    assert!(status.contains("col 2/6"), "{status:?}");
}
