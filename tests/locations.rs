//! The location list (#98, #99) without a server: rows in grep's shape,
//! accents when nothing is typed, fuzzy filtering, Enter jumps.

use std::fs;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::file_picker::{GrepHit, LocationRow, PickerKind, location_row, open_locations};
use unei::editor::testing::feed;

#[test]
fn location_list_filters_accents_and_jumps() {
    let dir = std::env::temp_dir().join(format!("unei-loc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/a.rs"),
        "fn alpha() {}\nfn beta() { alpha() }\n",
    )
    .unwrap();
    fs::write(dir.join("src/b.rs"), "use crate::alpha;\n").unwrap();
    let (buf, _) = Buffer::from_path(&dir.join("src/a.rs")).unwrap();
    let mut ed = Editor::new(buf);
    ed.set_view(100, 30);
    ed.set_root(dir.clone());

    let row = |rel: &str, line: usize, text: &str, b0: usize, b1: usize| {
        let (display, accent) = location_row(rel, line, text, b0, b1);
        LocationRow {
            display,
            accent,
            hit: GrepHit {
                path: rel.to_string(),
                line,
                col: b0,
            },
        }
    };
    let rows = vec![
        row("src/a.rs", 0, "fn alpha() {}", 3, 8),
        row("src/a.rs", 1, "fn beta() { alpha() }", 12, 17),
        row("src/b.rs", 0, "use crate::alpha;", 11, 16),
    ];
    open_locations(&mut ed, " test ", rows);
    let p = ed.file_picker.as_ref().unwrap();
    assert_eq!(p.kind, PickerKind::Locations);
    assert_eq!(p.matches.len(), 3, "everything, in the source's order");
    assert_eq!(p.item(&p.matches[0]), "src/a.rs:1: fn alpha() {}");
    assert_eq!(
        p.matches[0].indices,
        vec![15, 16, 17, 18, 19],
        "accented name"
    );
    assert_eq!(p.preview_title(), "src/a.rs:1");

    feed(&mut ed, "b.rs");
    let p = ed.file_picker.as_ref().unwrap();
    assert_eq!(p.matches.len(), 1, "fuzzy-filtered");
    assert!(p.item(&p.matches[0]).starts_with("src/b.rs:1:"));
    feed(&mut ed, "<CR>");
    assert!(ed.file_picker.is_none());
    assert!(ed.buffer.path.as_ref().unwrap().ends_with("b.rs"));
    assert_eq!((ed.cursor.line, ed.cursor.col), (0, 11));
    let _ = fs::remove_dir_all(&dir);
}
