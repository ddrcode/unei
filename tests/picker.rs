//! File picker (ticket #3): listing, fuzzy matching, opening, creation.

use std::fs;
use std::path::PathBuf;

use tailored::editor::Editor;
use tailored::editor::testing::{editor_from, feed, text};

/// A throwaway project tree: gitignored and hidden files must not appear.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tailored-picker-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::create_dir_all(dir.join("target")).unwrap();
    fs::write(dir.join("README.md"), "# readme\n").unwrap();
    fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.join("src/term.rs"), "// term\n").unwrap();
    fs::write(dir.join("docs/rules.md"), "rules\n").unwrap();
    fs::write(dir.join(".gitignore"), "/target\n").unwrap();
    fs::write(dir.join("target/skip.txt"), "no\n").unwrap();
    fs::write(dir.join(".hidden.txt"), "no\n").unwrap();
    dir
}

fn editor_in(name: &str) -> (Editor, PathBuf) {
    let dir = project(name);
    let mut ed = editor_from("");
    ed.set_root(dir.clone());
    (ed, dir)
}

fn visible(ed: &Editor) -> Vec<String> {
    let p = ed.file_picker.as_ref().expect("picker open");
    p.matches.iter().map(|m| p.item(m).to_string()).collect()
}

#[test]
fn listing_respects_gitignore_and_hidden() {
    let (mut ed, _dir) = editor_in("listing");
    feed(&mut ed, "<C-p>");
    let items = visible(&ed);
    assert_eq!(
        items,
        ["README.md", "docs/rules.md", "src/main.rs", "src/term.rs"]
    );
}

#[test]
fn leader_p_no_longer_opens_the_picker() {
    // reassigned to cut-paste per #10; Ctrl+P is the picker's home
    let (mut ed, _dir) = editor_in("leaderp");
    feed(&mut ed, " p");
    assert!(ed.file_picker.is_none());
}

#[test]
fn fuzzy_narrows_and_ranks() {
    let (mut ed, _dir) = editor_in("fuzzy");
    feed(&mut ed, "<C-p>term");
    let items = visible(&ed);
    assert_eq!(items[0], "src/term.rs");
    feed(&mut ed, "<C-u>"); // clear query restores everything
    assert_eq!(visible(&ed).len(), 4);
}

#[test]
fn enter_opens_the_selection() {
    let (mut ed, dir) = editor_in("open");
    feed(&mut ed, "<C-p>rules<CR>");
    assert!(ed.file_picker.is_none());
    assert_eq!(ed.buffer_count(), 2);
    assert_eq!(text(&ed), "rules\n");
    let path = ed.buffer.path.clone().unwrap();
    assert_eq!(path, dir.join("docs/rules.md"));
}

#[test]
fn selection_navigation_picks_lower_match() {
    let (mut ed, _dir) = editor_in("nav");
    feed(&mut ed, "<C-p>"); // empty query: alphabetical
    feed(&mut ed, "<Down><Down><CR>"); // third entry: src/main.rs
    assert_eq!(text(&ed), "fn main() {}\n");
}

#[test]
fn ctrl_ik_navigate_too() {
    let (mut ed, _dir) = editor_in("ctrlik");
    feed(&mut ed, "<C-p><C-k><C-i><C-k><C-k><CR>"); // net: third entry
    assert_eq!(text(&ed), "fn main() {}\n");
}

#[test]
fn ctrl_v_opens_in_vertical_split() {
    let (mut ed, _dir) = editor_in("vsplit");
    feed(&mut ed, "<C-p>term<C-v>");
    assert_eq!(ed.window_count(), 2);
    assert_eq!(ed.buffer_count(), 2);
    assert_eq!(text(&ed), "// term\n");
    // the other window still shows the original empty buffer
    feed(&mut ed, "<C-w>j");
    assert_eq!(text(&ed), "\n");
}

#[test]
fn ctrl_x_opens_in_horizontal_split() {
    let (mut ed, _dir) = editor_in("hsplit");
    feed(&mut ed, "<C-p>main<C-x>");
    assert_eq!(ed.window_count(), 2);
    assert_eq!(text(&ed), "fn main() {}\n");
    let rects = ed.window_rects();
    assert_eq!(rects[0].1.x, rects[1].1.x); // stacked, not side by side
}

#[test]
fn opening_same_file_twice_reuses_the_buffer() {
    let (mut ed, _dir) = editor_in("dedupe");
    feed(&mut ed, "<C-p>term<CR>");
    feed(&mut ed, "<C-p>term<CR>");
    assert_eq!(ed.buffer_count(), 2); // empty original + term.rs once
}

#[test]
fn esc_dismisses_without_side_effects() {
    let (mut ed, _dir) = editor_in("esc");
    feed(&mut ed, "<C-p>term<Esc>");
    assert!(ed.file_picker.is_none());
    assert_eq!(ed.buffer_count(), 1);
    assert_eq!(text(&ed), "\n");
    feed(&mut ed, "x"); // and normal mode works again
}

#[test]
fn picker_keys_never_reach_the_buffer() {
    let (mut ed, _dir) = editor_in("isolation");
    feed(&mut ed, "hguard<Esc>"); // buffer content: guard
    feed(&mut ed, "<C-p>ddxu<Esc>"); // would be destructive in normal mode
    assert_eq!(text(&ed), "guard\n");
}

#[test]
fn ctrl_enter_creates_path_with_parents() {
    let (mut ed, dir) = editor_in("create");
    feed(&mut ed, "<C-p>notes/ideas/first.md<C-CR>");
    assert!(ed.file_picker.is_none());
    assert_eq!(
        ed.buffer.path.clone().unwrap(),
        dir.join("notes/ideas/first.md")
    );
    assert!(dir.join("notes/ideas").is_dir(), "parent dirs created");
    assert!(
        !dir.join("notes/ideas/first.md").exists(),
        "file waits for :w"
    );
    feed(&mut ed, "hhello<Esc>:w<CR>");
    assert_eq!(
        fs::read_to_string(dir.join("notes/ideas/first.md")).unwrap(),
        "hello\n"
    );
}

#[test]
fn ctrl_enter_rejects_escaping_paths() {
    let (mut ed, _dir) = editor_in("escape-path");
    feed(&mut ed, "<C-p>../outside.txt<C-CR>");
    assert!(ed.file_picker.is_some(), "picker stays open on refusal");
    assert!(ed.message.as_ref().unwrap().error);
    feed(&mut ed, "<Esc>");
}

#[test]
fn dir_launch_rules_flow_into_the_editor() {
    // resolve() is unit-tested in launch.rs; verify the editor honors root
    let (mut ed, dir) = editor_in("root");
    assert_eq!(ed.root(), dir.as_path());
    feed(&mut ed, "<C-p>README<CR>");
    assert_eq!(ed.buffer.path.clone().unwrap(), dir.join("README.md"));
}
