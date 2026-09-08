//! The window chord's modifiers (#95): Shift splits and stays put,
//! Alt splits, projects the new window and stays — the side-by-side
//! editing setup (`Ctrl+w v`, `gp`, `Ctrl+w j`) in one chord.
//! Layout reminder: i=up, k=down, j=left, l=right.

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{editor_from, feed};
use unei::editor::windows::WinId;

/// Geometry the way the renderer sets it — layout area first, the focused
/// view derived from it — so window rects and the view agree.
fn with_path(content: &str, path: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(path));
    let mut ed = Editor::new(b);
    ed.set_window_area(ratatui::layout::Rect::new(0, 0, 160, 41));
    ed
}

/// Every window but the focused one.
fn others(ed: &Editor) -> Vec<WinId> {
    let me = ed.focused_window_id();
    ed.window_rects()
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| *id != me)
        .collect()
}

#[test]
fn shift_splits_keep_the_focus_where_it_is() {
    let mut ed = editor_from("one\ntwo\nthree\n");
    ed.set_window_area(ratatui::layout::Rect::new(0, 0, 160, 41));
    let me = ed.focused_window_id();
    feed(&mut ed, "kk"); // line 2
    feed(&mut ed, "<C-w>V");
    assert_eq!(ed.window_count(), 2);
    assert_eq!(ed.focused_window_id(), me, "V stays put");
    assert_eq!(ed.cursor.line, 2, "and the cursor is untouched");
    feed(&mut ed, "<C-w>X");
    assert_eq!(ed.window_count(), 3);
    assert_eq!(ed.focused_window_id(), me, "X stays put");
    feed(&mut ed, "<C-w>S");
    assert_eq!(ed.window_count(), 4);
    assert_eq!(ed.focused_window_id(), me);
    // the plain split, for contrast, moves
    feed(&mut ed, "<C-w>v");
    assert_ne!(ed.focused_window_id(), me);
}

#[test]
fn alt_split_projects_the_new_window_and_stays() {
    let mut long = String::from("fn main() {\n");
    for i in 0..60 {
        long.push_str(&format!("    let v{i} = {i};\n"));
    }
    long.push_str("}\n");
    let mut ed = with_path(&long, "a.rs");
    let me = ed.focused_window_id();
    feed(&mut ed, "10k");
    feed(&mut ed, "<C-w><A-v>");
    assert_eq!(ed.window_count(), 2);
    assert_eq!(ed.focused_window_id(), me, "focus stays on the source");
    assert!(!ed.focused_is_preview());
    let right = others(&ed)[0];
    assert!(ed.is_preview_window(right), "the new window projects");
    let (line, top) = ed.preview_nav_state(right);
    assert_eq!(line, 10, "reading line is the cursor's line");
    assert_eq!(
        line - top,
        ed.cursor_display_pos().0,
        "…on the cursor's screen row"
    );
    assert!(
        ed.message.as_ref().unwrap().text.contains("compiler's-eye"),
        "named like gp names it"
    );
    // it follows from then on, at its own geometry
    feed(&mut ed, "G");
    ed.sync_preview_follow();
    let (line, top) = ed.preview_nav_state(right);
    assert_eq!(line, 61);
    assert_eq!(line - top, ed.cursor_display_pos().0);
    // and Alt+x stacks one below, also projected, focus still here
    feed(&mut ed, "<C-w><A-x>");
    assert_eq!(ed.window_count(), 3);
    assert_eq!(ed.focused_window_id(), me);
    assert!(others(&ed).iter().all(|id| ed.is_preview_window(*id)));
}

#[test]
fn alt_split_works_for_markdown_too() {
    let mut ed = with_path("# Title\n\ntext\n", "doc.md");
    let me = ed.focused_window_id();
    feed(&mut ed, "<C-w><A-v>");
    assert_eq!(ed.window_count(), 2);
    assert_eq!(ed.focused_window_id(), me);
    assert!(ed.is_preview_window(others(&ed)[0]));
}

#[test]
fn alt_split_refuses_where_gp_would_and_opens_nothing() {
    let mut ed = with_path("notes\n", "a.txt");
    feed(&mut ed, "<C-w><A-v>");
    assert_eq!(ed.window_count(), 1, "no split without a projection");
    assert!(ed.message.as_ref().unwrap().text.contains("no preview"));
}
