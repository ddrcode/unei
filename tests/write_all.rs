//! `:wa` (#97): every modified buffer with a file is written — the parked
//! ones too — and unnamed ones are reported rather than skipped silently.

use std::fs;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::feed;

#[test]
fn wa_writes_current_and_parked_buffers() {
    let dir = std::env::temp_dir().join(format!("unei-wa-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    fs::write(&a, "alpha\n").unwrap();
    fs::write(&b, "beta\n").unwrap();
    let (ba, _) = Buffer::from_path(&a).unwrap();
    let (bb, _) = Buffer::from_path(&b).unwrap();
    let mut ed = Editor::with_buffers(vec![ba, bb]);
    ed.set_view(80, 20);
    feed(&mut ed, "A one<Esc>"); // edit a (current)
    feed(&mut ed, " bk<CR>"); // buffer list: down to b, select
    assert!(ed.buffer.path.as_ref().unwrap().ends_with("b.txt"));
    feed(&mut ed, "A two<Esc>");
    feed(&mut ed, "<C-^>"); // back to a: b is parked and modified
    assert!(ed.buffer.path.as_ref().unwrap().ends_with("a.txt"));
    assert!(ed.buffer_entries().iter().all(|e| e.modified));
    feed(&mut ed, ":wa<CR>");
    assert_eq!(fs::read_to_string(&a).unwrap(), "alpha one\n");
    assert_eq!(fs::read_to_string(&b).unwrap(), "beta two\n");
    assert!(ed.buffer_entries().iter().all(|e| !e.modified), "all clean");
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("2 buffer(s) written")
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn wa_reports_unnamed_buffers() {
    let mut ed = Editor::new(Buffer::from_text("scratch\n"));
    ed.set_view(80, 20);
    feed(&mut ed, "A!<Esc>:wa<CR>");
    let msg = ed.message.as_ref().unwrap().text.clone();
    assert!(msg.contains("0 buffer(s) written"), "{msg}");
    assert!(msg.contains("1 unnamed buffer(s) not written"), "{msg}");
}
