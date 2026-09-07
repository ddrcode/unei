//! Bracketed paste goes to whoever owns the keyboard (#82 §11): an open
//! picker takes it as query text; the buffer list and a focused preview
//! swallow it. The source buffer is never touched behind an overlay.

use unei::editor::file_picker;
use unei::editor::testing::{editor_from, editor_with_buffers, feed, text};

#[test]
fn paste_into_an_open_picker_edits_the_query_not_the_buffer() {
    let mut ed = editor_from("source\n");
    file_picker::open(&mut ed);
    ed.paste_external("src/ma\nin");
    assert_eq!(text(&ed), "source\n", "source buffer untouched");
    let p = ed.file_picker.as_ref().expect("picker still open");
    assert_eq!(p.query, "src/ma in", "pasted into the query, flattened");
}

#[test]
fn paste_with_the_buffer_list_open_is_swallowed() {
    let mut ed = editor_from("source\n");
    feed(&mut ed, " b"); // Space b: buffer list
    assert!(ed.buffer_list.is_some());
    ed.paste_external("junk");
    assert_eq!(text(&ed), "source\n");
}

#[test]
fn paste_into_a_focused_preview_never_reaches_the_source() {
    let mut ed = editor_with_buffers(&[("notes.md", "# hi\n")]);
    feed(&mut ed, "gp"); // the focused window becomes the markdown preview
    assert!(
        ed.focused_is_preview(),
        "preview must be focused for this test"
    );
    ed.paste_external("junk");
    assert_eq!(
        text(&ed),
        "# hi\n",
        "a navigation-only preview must not mutate the source"
    );
}
