//! External changes (#80): the editor must never silently overwrite a file
//! something else modified (an agent, treefmt), must offer a reload, and a
//! clean buffer should pick the change up on its own.

use std::fs;
use std::path::{Path, PathBuf};

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-ext-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(path: &Path) -> Editor {
    let (buf, _) = Buffer::from_path(path).unwrap();
    let mut ed = Editor::new(buf);
    ed.set_view(80, 22);
    ed
}

#[test]
fn write_refuses_after_external_change_and_bang_overwrites() {
    let dir = tmp("w");
    let f = dir.join("a.txt");
    fs::write(&f, "original\n").unwrap();
    let mut ed = open(&f);
    // an agent rewrites the file underneath us
    fs::write(&f, "rewritten by an agent\n").unwrap();
    feed(&mut ed, "ochange<Esc>");
    feed(&mut ed, ":w<CR>");
    // NOT clobbered
    assert_eq!(fs::read_to_string(&f).unwrap(), "rewritten by an agent\n");
    feed(&mut ed, ":w!<CR>");
    assert_eq!(fs::read_to_string(&f).unwrap(), "original\nchange\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn e_reloads_a_clean_buffer_and_e_bang_discards_edits() {
    let dir = tmp("e");
    let f = dir.join("b.txt");
    fs::write(&f, "one\n").unwrap();
    let mut ed = open(&f);
    fs::write(&f, "one\ntwo\n").unwrap();
    feed(&mut ed, ":e<CR>");
    assert_eq!(text(&ed), "one\ntwo\n");
    assert!(!ed.buffer.is_modified());
    // dirty it, change the disk again: :e refuses, :e! discards
    feed(&mut ed, "olocal<Esc>");
    fs::write(&f, "one\ntwo\nthree\n").unwrap();
    feed(&mut ed, ":e<CR>");
    assert!(text(&ed).contains("local"), "unsaved edits must survive :e");
    feed(&mut ed, ":e!<CR>");
    assert_eq!(text(&ed), "one\ntwo\nthree\n");
    assert!(!ed.buffer.is_modified());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn poll_auto_reloads_a_clean_buffer() {
    let dir = tmp("poll");
    let f = dir.join("c.txt");
    fs::write(&f, "v1\n").unwrap();
    let mut ed = open(&f);
    assert!(!ed.check_disk(), "nothing changed yet");
    fs::write(&f, "v2, longer\n").unwrap(); // treefmt / agent edit
    assert!(ed.check_disk());
    assert_eq!(text(&ed), "v2, longer\n");
    assert!(!ed.buffer.is_modified());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn poll_flags_a_dirty_buffer_instead_of_reloading() {
    let dir = tmp("flag");
    let f = dir.join("d.txt");
    fs::write(&f, "base\n").unwrap();
    let mut ed = open(&f);
    feed(&mut ed, "omine<Esc>"); // unsaved local edit
    fs::write(&f, "base\ntheirs\n").unwrap(); // external edit
    assert!(ed.check_disk());
    assert!(ed.buffer.disk_conflict, "flagged → [!] in the statusline");
    assert!(text(&ed).contains("mine"), "local edits kept");
    assert_eq!(fs::read_to_string(&f).unwrap(), "base\ntheirs\n");
    feed(&mut ed, ":w<CR>");
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        "base\ntheirs\n",
        "plain :w still refuses"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn our_own_save_is_not_an_external_change() {
    let dir = tmp("own");
    let f = dir.join("e.txt");
    fs::write(&f, "x\n").unwrap();
    let mut ed = open(&f);
    feed(&mut ed, "oy<Esc>:w<CR>");
    assert_eq!(fs::read_to_string(&f).unwrap(), "x\ny\n");
    assert!(!ed.check_disk(), "our write re-stamped the file");
    assert!(!ed.buffer.disk_conflict);
    let _ = fs::remove_dir_all(&dir);
}
