//! The picker on screen (#114): long paths read whole, the directory part
//! dimmed, the preview beside the list — or under it when the terminal is
//! narrow. Rendered through the real `ui::render` into a test backend.

use std::fs;
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use unei::config::palette;
use unei::editor::Editor;
use unei::editor::testing::{editor_from, feed};

const DEEP: &str = "grammars/asm6502/src/tree_sitter/alloc.h";

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-render-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("grammars/asm6502/src/tree_sitter")).unwrap();
    fs::write(dir.join(DEEP), "// alloc\n").unwrap();
    fs::write(dir.join("README.md"), "# readme\n").unwrap();
    dir
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
fn a_deep_path_reads_whole_on_a_wide_terminal() {
    let dir = project();
    let mut ed = editor_from("");
    ed.set_root(dir.clone());
    feed(&mut ed, "<C-p>");
    let rows = screen(&mut ed, 140, 40);
    let row = rows
        .iter()
        .find(|r| r.contains(DEEP))
        .unwrap_or_else(|| panic!("the full path is not on screen:\n{}", rows.join("\n")));
    assert!(!row.contains('…'), "nothing truncated: {row}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_directory_is_dim_and_the_name_is_not() {
    let dir = project();
    let mut ed = editor_from("");
    ed.set_root(dir.clone());
    feed(&mut ed, "<C-p>"); // both rows listed; README.md is the selected one
    let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
    term.draw(|f| unei::ui::render(f, &mut ed)).unwrap();
    let buf = term.backend().buffer().clone();
    // find the unselected deep row and sample a directory cell and a name cell
    let mut found = false;
    for y in 0..40 {
        let text: String = (0..140).map(|x| buf[(x, y)].symbol().to_string()).collect();
        if let Some(col) = text.find(DEEP) {
            let x0 = text[..col].chars().count() as u16;
            let dir_cell = &buf[(x0, y)]; // 'g' of grammars
            let name_cell = &buf[(x0 + 33, y)]; // 'a' of alloc.h
            if dir_cell.bg == palette::FLOAT_BG {
                // an unselected row: the float's background
                assert_eq!(dir_cell.fg, palette::COMMENT, "directory is dimmed");
                assert_eq!(name_cell.fg, palette::FG, "the name is not");
                found = true;
            }
        }
    }
    assert!(found, "an unselected row with the deep path");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_narrow_terminal_stacks_list_over_preview() {
    let dir = project();
    let mut ed = editor_from("");
    ed.set_root(dir.clone());
    feed(&mut ed, "<C-p>alloc");
    let rows = screen(&mut ed, 80, 40);
    let list_y = rows
        .iter()
        .position(|r| r.contains(DEEP))
        .expect("path row");
    let preview_y = rows
        .iter()
        .position(|r| r.contains("// alloc"))
        .expect("preview content");
    assert!(
        preview_y > list_y,
        "preview below the list: {preview_y} > {list_y}"
    );
    let joined = rows.join("\n");
    assert!(
        joined.contains(" alloc.h "),
        "preview pane titled with the file name"
    );
    let _ = fs::remove_dir_all(&dir);
}
