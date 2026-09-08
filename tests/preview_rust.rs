//! The compiler's-eye view (#93) without a server: `gp` on a Rust buffer
//! projects the numbered source, follows the editing window, and jumps
//! back. What rust-analyzer adds is covered in tests/lsp.rs.

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};

const SRC: &str = "fn main() {\n    let x = 1;\n    let y = x + 1;\n    println!(\"{y}\");\n}\n";

fn rs_editor(content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from("a.rs"));
    let mut ed = Editor::new(b);
    ed.set_view(80, 22);
    ed
}

fn projection(ed: &mut Editor) -> Vec<String> {
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, 78);
    (0..doc.line_count()).map(|i| doc.plain(i)).collect()
}

#[test]
fn gp_projects_rust_as_the_numbered_source() {
    let mut ed = rs_editor(SRC);
    feed(&mut ed, "gp");
    assert!(ed.focused_is_preview(), "rust has a projection since #93");
    assert!(
        ed.message.as_ref().unwrap().text.contains("compiler's-eye"),
        "names the view"
    );
    let lines = projection(&mut ed);
    assert_eq!(lines[0], "1 fn main() {");
    assert_eq!(lines[1], "2     let x = 1;");
    assert_eq!(lines[4], "5 }");
    assert_eq!(lines.len(), 5, "one row per line without hints");
    feed(&mut ed, "<Esc>");
    assert!(!ed.focused_is_preview());
    assert_eq!(text(&ed), SRC, "projecting never touches the buffer");
}

#[test]
fn the_projection_follows_the_source_window() {
    let mut long = String::from("fn main() {\n");
    for i in 0..60 {
        long.push_str(&format!("    let v{i} = {i};\n"));
    }
    long.push_str("}\n");
    let mut ed = rs_editor(&long);
    feed(&mut ed, "<C-w>v");
    feed(&mut ed, "gp");
    let preview_id = ed.focused_window_id();
    feed(&mut ed, "<C-w>j");
    assert!(!ed.focused_is_preview());
    feed(&mut ed, "G");
    ed.sync_preview_follow();
    let (line, top) = ed.preview_nav_state(preview_id);
    assert_eq!(line, 61, "reading line is the last source line's row");
    assert!(top > 30, "followed to the end, top={top}");
    // side by side, the reading line sits on the source cursor's screen
    // row — the eye moves straight across, not up (#94 review)
    assert_eq!(line - top, ed.cursor_display_pos().0);
    feed(&mut ed, "30i"); // up thirty lines: the cursor lands mid-window
    ed.sync_preview_follow();
    let (line, top) = ed.preview_nav_state(preview_id);
    assert_eq!(line, 31);
    assert_eq!(line - top, ed.cursor_display_pos().0);
    feed(&mut ed, "gg");
    ed.sync_preview_follow();
    assert_eq!(ed.preview_nav_state(preview_id), (0, 0));
}

#[test]
fn enter_and_h_land_on_the_mapped_source_line() {
    let mut ed = rs_editor(SRC);
    feed(&mut ed, "gp");
    feed(&mut ed, "kk<CR>"); // two rows down, jump
    assert!(!ed.focused_is_preview());
    assert_eq!(ed.cursor.line, 2);
    feed(&mut ed, "gp"); // reopens at the source line we're on
    feed(&mut ed, "kh"); // one row down, start editing there
    assert!(!ed.focused_is_preview());
    assert_eq!(ed.cursor.line, 3);
    feed(&mut ed, "// <Esc>");
    assert!(
        text(&ed).contains("//     println!"),
        "insert began on that line"
    );
}

#[test]
fn edits_in_the_source_refresh_the_projection() {
    let mut ed = rs_editor(SRC);
    assert_eq!(projection(&mut ed)[1], "2     let x = 1;");
    feed(&mut ed, "kA // note<Esc>");
    assert_eq!(projection(&mut ed)[1], "2     let x = 1; // note");
}
