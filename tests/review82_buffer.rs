//! Regressions from the #82 review: saved-state identity across undo, safe
//! file replacement (permissions, symlinks, temp files), save-as guards,
//! stale marks, and undo-transaction lifecycle.

use std::fs;
use std::path::{Path, PathBuf};

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{editor_from, feed, text};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-r82-{name}-{}", std::process::id()));
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

// §1 — undo branches must not alias the saved state
#[test]
fn edit_after_undo_past_a_save_is_modified() {
    let dir = tmp("saved-id");
    let f = dir.join("a.txt");
    fs::write(&f, "base\n").unwrap();
    let mut ed = open(&f);
    feed(&mut ed, "Ax<Esc>:w<CR>uAy<Esc>");
    assert_eq!(fs::read_to_string(&f).unwrap(), "basex\n");
    assert_eq!(text(&ed), "basey\n");
    assert!(
        ed.buffer.is_modified(),
        "different text than saved must read modified"
    );
    // and undoing back to exactly the saved text reads clean again
    feed(&mut ed, "u");
    assert_eq!(text(&ed), "base\n");
    assert!(ed.buffer.is_modified()); // "base" was never saved ("basex" was)
    feed(&mut ed, "<C-r>");
    assert_eq!(text(&ed), "basey\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn version_is_monotonic_across_undo_so_caches_invalidate() {
    let mut ed = editor_from("a\n");
    let v0 = ed.buffer.version();
    feed(&mut ed, "Ab<Esc>");
    let v1 = ed.buffer.version();
    feed(&mut ed, "u");
    let v2 = ed.buffer.version();
    assert!(
        v1 > v0 && v2 > v1,
        "undo must produce a new version, not rewind to {v0}"
    );
}

// §2 — permissions survive a save
#[cfg(unix)]
#[test]
fn save_preserves_permission_bits() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("perms");
    let f = dir.join("run.sh");
    fs::write(&f, "#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o700)).unwrap();
    let mut ed = open(&f);
    feed(&mut ed, "Aecho bye<Esc>:w<CR>");
    let mode = fs::metadata(&f).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "executable/private bits must survive :w");
    let _ = fs::remove_dir_all(&dir);
}

// §3 — writing through a symlink updates the target, keeps the link
#[cfg(unix)]
#[test]
fn save_writes_through_a_symlink() {
    let dir = tmp("symlink");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    fs::write(&target, "one\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut ed = open(&link);
    feed(&mut ed, "otwo<Esc>:w<CR>");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link must survive"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "one\ntwo\n");
    let _ = fs::remove_dir_all(&dir);
}

// §4 — a pre-planted temp path can't redirect the write
#[cfg(unix)]
#[test]
fn planted_temp_symlink_cannot_hijack_a_save() {
    let dir = tmp("tmpname");
    let f = dir.join("a.txt");
    let victim = dir.join("victim.txt");
    fs::write(&f, "mine\n").unwrap();
    fs::write(&victim, "precious\n").unwrap();
    // plant a symlink at the first temp name this process would try
    let planted = dir.join(format!(".a.txt.unei.{}.0.tmp", std::process::id()));
    std::os::unix::fs::symlink(&victim, &planted).unwrap();
    let mut ed = open(&f);
    feed(&mut ed, "Aedit<Esc>:w<CR>");
    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        "precious\n",
        "victim untouched"
    );
    assert!(!fs::symlink_metadata(&f).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&f).unwrap(), "mineedit\n");
    let _ = fs::remove_dir_all(&dir);
}

// §5 — save-as and new files respect what's on disk
#[test]
fn save_as_refuses_to_replace_an_existing_file_without_bang() {
    let dir = tmp("saveas");
    let existing = dir.join("existing.txt");
    fs::write(&existing, "keep me\n").unwrap();
    let mut ed = editor_from("scratch\n");
    feed(&mut ed, &format!(":w {}<CR>", existing.display()));
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep me\n");
    assert!(
        ed.buffer.path.is_none(),
        "a refused :w path must not rebind"
    );
    feed(&mut ed, &format!(":w! {}<CR>", existing.display()));
    assert_eq!(fs::read_to_string(&existing).unwrap(), "scratch\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_new_file_that_appears_externally_is_a_conflict() {
    let dir = tmp("appeared");
    let f = dir.join("new.txt");
    let mut ed = open(&f); // does not exist yet: [New File]
    feed(&mut ed, "hmine<Esc>");
    fs::write(&f, "someone else got here first\n").unwrap();
    feed(&mut ed, ":w<CR>");
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        "someone else got here first\n"
    );
    feed(&mut ed, ":w!<CR>");
    assert_eq!(fs::read_to_string(&f).unwrap(), "mine\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn failed_save_as_restores_the_previous_binding() {
    let dir = tmp("restore");
    let f = dir.join("a.txt");
    fs::write(&f, "x\n").unwrap();
    let mut ed = open(&f);
    let missing_dir = dir.join("no-such-dir").join("b.txt");
    feed(&mut ed, &format!(":w {}<CR>", missing_dir.display()));
    assert_eq!(ed.buffer.path.as_deref(), Some(f.as_path()));
    feed(&mut ed, "Ay<Esc>:w<CR>"); // the old binding still works
    assert_eq!(fs::read_to_string(&f).unwrap(), "xy\n");
    let _ = fs::remove_dir_all(&dir);
}

// §6 — a mark on a deleted line must not panic
#[test]
fn jumping_to_a_mark_past_the_end_clamps_instead_of_panicking() {
    let mut ed = editor_from("a\nb\nc\nd\n");
    feed(&mut ed, "GmaggdG'a");
    assert_eq!(text(&ed), "\n");
    assert_eq!(ed.cursor.line, 0);
}

// §12a — a no-op insert session must not destroy redo
#[test]
fn empty_insert_session_keeps_redo() {
    let mut ed = editor_from("a\n");
    feed(&mut ed, "Ax<Esc>u");
    assert_eq!(text(&ed), "a\n");
    feed(&mut ed, "h<Esc>"); // enter and leave insert without typing
    feed(&mut ed, "<C-r>");
    assert_eq!(
        text(&ed),
        "ax\n",
        "redo must survive an empty insert session"
    );
}
