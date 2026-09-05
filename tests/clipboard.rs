//! The yank/cut register split + system clipboard mirroring (ticket #10).

use tailored::editor::testing::{editor_from, feed, text};

#[test]
fn deletes_do_not_clobber_the_yank() {
    let mut ed = editor_from("keep\nnoise\ntarget\n");
    feed(&mut ed, "yy"); // yank "keep"
    feed(&mut ed, "kdd"); // delete "noise" — cut register only
    feed(&mut ed, "kp"); // p pastes the YANK
    assert_eq!(text(&ed), "keep\ntarget\nkeep\n");
}

#[test]
fn repeated_paste_after_many_deletes() {
    // the exact annoyance from #10: paste the same thing in several places
    let mut ed = editor_from("one\ntwo\nthree\nfour\n");
    feed(&mut ed, "yy"); // yank "one"
    feed(&mut ed, "kdd"); // delete two
    feed(&mut ed, "p"); // still pastes "one"
    feed(&mut ed, "kk"); // wait: kk = down twice
    feed(&mut ed, "dd");
    feed(&mut ed, "p");
    let t = text(&ed);
    assert_eq!(
        t.matches("one").count(),
        3,
        "yank survived every delete: {t}"
    );
}

#[test]
fn leader_p_pastes_the_cut() {
    let mut ed = editor_from("first\nsecond\n");
    feed(&mut ed, "dd"); // cut "first"
    feed(&mut ed, " p"); // paste cut after → the dd+p line-move idiom
    assert_eq!(text(&ed), "second\nfirst\n");
}

#[test]
fn leader_shift_p_pastes_cut_before() {
    let mut ed = editor_from("aa\nbb\n");
    feed(&mut ed, "ddk P");
    assert_eq!(text(&ed), "aa\nbb\n");
}

#[test]
fn x_lands_in_cut_register() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, "xl p"); // x cuts 'a'; move right; leader-p pastes after 'c'
    assert_eq!(text(&ed), "bca\n");
}

#[test]
fn change_lands_in_cut_register() {
    let mut ed = editor_from("old new\n");
    feed(&mut ed, "cwfresh<Esc>"); // "old" → cut register
    feed(&mut ed, "$ p");
    assert_eq!(text(&ed), "fresh newold\n");
}

#[test]
fn visual_delete_cuts_yank_survives() {
    let mut ed = editor_from("alpha beta\n");
    feed(&mut ed, "vey"); // yank "alpha"
    feed(&mut ed, "wved"); // visually delete "beta" → cut
    feed(&mut ed, "$p"); // yank register intact
    assert!(text(&ed).contains("alpha"), "{}", text(&ed));
    feed(&mut ed, " p"); // and beta sits in the cut register
    assert!(text(&ed).contains("beta"), "{}", text(&ed));
}

#[test]
fn only_yanks_mirror_to_system_clipboard() {
    let mut ed = editor_from("copy me\nnot me\n");
    assert!(ed.last_clipboard.is_none());
    feed(&mut ed, "yy");
    assert_eq!(ed.last_clipboard.as_deref(), Some("copy me\n"));
    feed(&mut ed, "kdd"); // a delete must not touch the clipboard
    assert_eq!(ed.last_clipboard.as_deref(), Some("copy me\n"));
    feed(&mut ed, "vly"); // charwise yank mirrors too (cursor is on "copy me")
    assert_eq!(ed.last_clipboard.as_deref(), Some("co"));
}

#[test]
fn block_yank_mirrors_joined() {
    let mut ed = editor_from("ab\ncd\n");
    feed(&mut ed, "<C-v>ky");
    assert_eq!(ed.last_clipboard.as_deref(), Some("a\nc"));
}

#[test]
fn external_paste_in_insert_mode_is_literal() {
    let mut ed = editor_from("\n");
    feed(&mut ed, "h");
    ed.paste_external("hello dd world"); // dd must not delete anything
    feed(&mut ed, "<Esc>");
    assert_eq!(text(&ed), "hello dd world\n");
}

#[test]
fn external_paste_in_normal_mode_is_one_undo() {
    let mut ed = editor_from("ab\n");
    feed(&mut ed, "l");
    ed.paste_external("XY\nZ");
    assert_eq!(text(&ed), "aXY\nZb\n");
    feed(&mut ed, "u");
    assert_eq!(text(&ed), "ab\n");
}

#[test]
fn external_paste_replaces_visual_selection() {
    let mut ed = editor_from("replace me now\n");
    feed(&mut ed, "wve"); // select "me"
    ed.paste_external("US");
    assert_eq!(text(&ed), "replace US now\n");
}

#[test]
fn external_paste_normalizes_crlf() {
    let mut ed = editor_from("\n");
    feed(&mut ed, "h");
    ed.paste_external("a\r\nb\rc");
    feed(&mut ed, "<Esc>");
    assert_eq!(text(&ed), "a\nb\nc\n");
}

#[test]
fn empty_cut_register_reports() {
    let mut ed = editor_from("abc\n");
    feed(&mut ed, " p");
    assert_eq!(text(&ed), "abc\n");
    assert!(ed.message.as_ref().unwrap().text.contains("cut register"));
}
