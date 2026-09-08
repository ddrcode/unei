//! Markdown preview (ticket #39 phase 1): projection, LineMap, follow, keys.

use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};

const DOC: &str = "# Title\n\nSome *emphasis* and **strong** and `code`.\n\n## Section two\n\n- alpha\n- beta\n\n```rust\nfn demo() {}\n```\n\n> a quote line\n";

fn md_editor(content: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from("doc.md"));
    let mut ed = Editor::new(b);
    ed.set_view(80, 22);
    ed
}

fn preview_text(ed: &mut Editor) -> Vec<String> {
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, 60);
    doc.lines
        .iter()
        .map(|frags| frags.iter().map(|(t, _)| t.as_str()).collect::<String>())
        .collect()
}

#[test]
fn renders_structure_prettily() {
    let mut ed = md_editor(DOC);
    let lines = preview_text(&mut ed);
    let all = lines.join("\n");
    assert!(all.contains("▌ Title"), "heading bar: {all}");
    assert!(all.contains("• alpha"), "bullets");
    assert!(all.contains("fn demo() {}"), "code content");
    assert!(all.contains("▎ a quote line"), "quote bar");
    assert!(!all.contains("**strong**"), "markers stripped");
    assert!(!all.contains("# Title"), "heading marker stripped");
    assert!(all.contains("strong"));
}

#[test]
fn markers_are_styled_not_textual() {
    use ratatui::style::Modifier;
    let mut ed = md_editor("plain *it* **bold** text\n");
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, 60);
    let has_bold = doc
        .lines
        .iter()
        .flatten()
        .any(|(t, s)| t.contains("bold") && s.add_modifier.contains(Modifier::BOLD));
    let has_italic = doc
        .lines
        .iter()
        .flatten()
        .any(|(t, s)| t.contains("it") && s.add_modifier.contains(Modifier::ITALIC));
    assert!(has_bold && has_italic);
}

#[test]
fn fenced_rust_is_syntax_highlighted() {
    let mut ed = md_editor("```rust\nfn x() {}\n```\n");
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, 60);
    let purple = ratatui::style::Color::Rgb(0xC7, 0x92, 0xEA);
    let keyword_colored = doc
        .lines
        .iter()
        .flatten()
        .any(|(t, s)| t == "fn" && s.fg == Some(purple));
    assert!(keyword_colored, "fn should render keyword-purple");
}

#[test]
fn long_paragraphs_wrap() {
    let long = format!("# T\n\n{}\n", "word ".repeat(40));
    let mut ed = md_editor(&long);
    let lines = preview_text(&mut ed);
    assert!(lines.iter().filter(|l| l.contains("word")).count() > 2);
    assert!(lines.iter().all(|l| l.chars().count() <= 100));
}

#[test]
fn line_map_supports_follow_and_jump() {
    let mut ed = md_editor(DOC);
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, 60);
    // "## Section two" sits on source line 4
    let v = doc.view_line_for_source(4);
    assert!(doc.lines[v].iter().any(|(t, _)| t.contains("Section two")));
    assert_eq!(doc.source_line_for_view(v), 4);
    // fence content maps to its own source line (line 10: fn demo)
    let vf = doc.view_line_for_source(10);
    let content: String = doc.lines[vf].iter().map(|(t, _)| t.as_str()).collect();
    assert!(
        content.contains("fn demo"),
        "vf={vf} content={content:?} map={:?}",
        doc.map
    );
}

#[test]
fn gp_toggles_and_esc_returns() {
    let mut ed = md_editor(DOC);
    assert!(!ed.focused_is_preview());
    feed(&mut ed, "gp");
    assert!(ed.focused_is_preview());
    feed(&mut ed, "<Esc>");
    assert!(!ed.focused_is_preview());
}

#[test]
fn gp_refuses_file_types_without_a_projection() {
    // rust has one since #93; plain text does not
    let mut b = Buffer::from_text("notes\n");
    b.path = Some(PathBuf::from("a.txt"));
    let mut ed = Editor::new(b);
    ed.set_view(80, 22);
    feed(&mut ed, "gp");
    assert!(!ed.focused_is_preview());
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("markdown and rust")
    );
}

#[test]
fn preview_navigation_and_jump_to_source() {
    let mut ed = md_editor(DOC);
    feed(&mut ed, "gp");
    let id = ed.focused_window_id();
    let (l0, _) = ed.preview_nav_state(id);
    feed(&mut ed, "kkk"); // three lines down the projection
    let (l1, _) = ed.preview_nav_state(id);
    assert_eq!(l1, l0 + 3);
    feed(&mut ed, "G");
    let buf = ed.current_buffer_id();
    let last = ed.preview_doc_for(buf, 78).line_count() - 1;
    let (lg, _) = ed.preview_nav_state(id);
    assert_eq!(lg, last);
    // Enter jumps back to the mapped source line, leaving preview
    feed(&mut ed, "g"); // top
    feed(&mut ed, "kkkk<CR>");
    assert!(!ed.focused_is_preview());
    assert!(ed.cursor.line > 0, "landed at a mapped source line");
}

#[test]
fn editing_keys_are_inert_in_preview() {
    let mut ed = md_editor("# X\n\nbody\n");
    let before = text(&ed);
    feed(&mut ed, "gp");
    feed(&mut ed, "ddxu. y"); // destruction attempt (`h` legitimately edits)
    assert_eq!(text(&ed), before);
    assert!(ed.focused_is_preview());
}

#[test]
fn side_by_side_follow() {
    let mut long = String::from("# Top\n\n");
    for i in 1..=40 {
        long.push_str(&format!("paragraph number {i} with some text\n\n"));
    }
    let mut ed = md_editor(&long);
    ed.set_view(80, 22);
    feed(&mut ed, "<C-w>v"); // split: both show doc.md
    feed(&mut ed, "gp"); // right window becomes the preview
    let preview_id = ed.focused_window_id();
    feed(&mut ed, "<C-w>j"); // focus source on the left
    assert!(!ed.focused_is_preview());
    feed(&mut ed, "G"); // bottom of the source
    ed.sync_preview_follow();
    let (_, top) = ed.preview_nav_state(preview_id);
    assert!(top > 10, "preview followed to the end, top={top}");
    feed(&mut ed, "gg");
    ed.sync_preview_follow();
    let (_, top) = ed.preview_nav_state(preview_id);
    assert_eq!(top, 0, "preview followed back to the top");
}

#[test]
fn edits_refresh_the_projection() {
    let mut ed = md_editor("# Old\n");
    let lines = preview_text(&mut ed);
    assert!(lines[0].contains("Old"));
    feed(&mut ed, "A Extended<Esc>");
    let lines = preview_text(&mut ed);
    assert!(lines[0].contains("Old Extended"));
}

#[test]
fn task_lists_render_checks() {
    let mut ed = md_editor("- [x] done thing\n- [ ] todo thing\n");
    let lines = preview_text(&mut ed);
    let all = lines.join("\n");
    assert!(all.contains("✓ done thing"), "{all}");
    assert!(all.contains("○ todo thing"));
}

#[test]
fn tables_render_as_aligned_grids() {
    let mut ed = md_editor("| Name | Count |\n|:-----|------:|\n| ab | 1 |\n| c | 12 |\n");
    let lines = preview_text(&mut ed);
    let all = lines.join("\n");
    assert!(all.contains("Name"), "{all}");
    assert!(
        all.contains("\u{2500}\u{253c}\u{2500}"),
        "header rule: {all}"
    );
    assert!(!all.contains('|'), "raw pipes stripped: {all}");
    let row1 = lines.iter().find(|l| l.contains("ab")).unwrap();
    let row2 = lines.iter().find(|l| l.contains(" c ")).unwrap();
    assert!(row1.contains("\u{2502}     1"), "right-aligned 1: {row1:?}");
    assert!(
        row2.contains("\u{2502}    12"),
        "right-aligned 12: {row2:?}"
    );
    // every table row maps to its own source line: follow lands mid-table
    let buf = ed.current_buffer_id();
    let doc = ed.preview_doc_for(buf, 60);
    let rule = doc
        .lines
        .iter()
        .position(|l| l.iter().any(|(t, _)| t.contains('\u{253c}')))
        .unwrap();
    assert_eq!(doc.source_line_for_view(rule), 1);
    assert_eq!(doc.source_line_for_view(rule + 1), 2);
    assert_eq!(doc.source_line_for_view(rule + 2), 3);
}

#[test]
fn h_jumps_to_source_and_enters_insert() {
    let mut ed = md_editor(DOC);
    feed(&mut ed, "gp");
    feed(&mut ed, "G"); // last rendered line: the quote (source line 13)
    feed(&mut ed, "h");
    assert!(!ed.focused_is_preview(), "h leaves the projection");
    feed(&mut ed, "X<Esc>");
    let quote_line = text(&ed).lines().nth(13).unwrap_or("").to_string();
    assert!(
        quote_line.starts_with("X>"),
        "insert began on the mapped source line: {quote_line:?}"
    );
}

#[test]
fn colon_q_closes_the_preview_panel() {
    let mut ed = md_editor(DOC);
    feed(&mut ed, "<C-w>v");
    assert_eq!(ed.window_count(), 2);
    feed(&mut ed, "gp");
    assert!(ed.focused_is_preview());
    feed(&mut ed, ":q<Enter>");
    assert_eq!(ed.window_count(), 1, ":q closes the preview panel");
    assert!(!ed.focused_is_preview());
    // the surviving window edits normally
    feed(&mut ed, "aZ<Esc>");
    assert!(text(&ed).contains('Z'));
}
