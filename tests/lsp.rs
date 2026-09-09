//! rust-analyzer integration (ticket #6) against a canned fake server
//! (tests/fake_lsp.py) installed on PATH as `rust-analyzer`. One test fn:
//! PATH is process-global.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};
use unei::lsp::Severity;

fn install_fake_rust_analyzer() {
    let bin = std::env::temp_dir().join(format!("unei-lsp-bin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&bin);
    fs::create_dir_all(&bin).unwrap();
    let script_path = std::fs::canonicalize("tests/fake_lsp.py").unwrap();
    let script = format!("#!/bin/sh\nexec python3 \"{}\"\n", script_path.display());
    let path = bin.join("rust-analyzer");
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let old = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", format!("{}:{old}", bin.display())) };
}

/// The fake server's didOpen/didChange log: (uri, text) in arrival order.
fn server_docs(log: &std::path::Path) -> Vec<(String, String, String)> {
    let Ok(raw) = fs::read_to_string(log) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|v| {
            (
                v["method"].as_str().unwrap_or("").to_string(),
                v["uri"].as_str().unwrap_or("").to_string(),
                v["text"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

/// Whether the server has received a didChange for a document whose uri
/// ends with `file` and whose text contains `needle`.
fn server_saw_change(log: &std::path::Path, file: &str, needle: &str) -> bool {
    server_docs(log).iter().any(|(m, uri, text)| {
        m == "textDocument/didChange" && uri.ends_with(file) && text.contains(needle)
    })
}

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-lsp-proj-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    fs::write(
        dir.join("src/main.rs"),
        "fn main() {\nlet x = vec![1];\nfn f() {}\n}\n",
    )
    .unwrap();
    fs::write(dir.join("src/other.rs"), "fn other() {}\n").unwrap();
    dir
}

fn editor_on(path: &std::path::Path) -> Editor {
    let (buffer, _) = Buffer::from_path(path).unwrap();
    let mut ed = Editor::new(buffer);
    ed.set_view(80, 22);
    ed
}

/// Pumps lsp_tick until `done` or the deadline.
fn pump(ed: &mut Editor, mut done: impl FnMut(&mut Editor) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        ed.lsp_tick();
        if done(ed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn analyzer_end_to_end_with_fake_server() {
    install_fake_rust_analyzer();
    let dir = project();
    let log = dir.join("server.log");
    unsafe { std::env::set_var("UNEI_FAKE_LSP_LOG", &log) }; // before the server spawns
    let file = dir.join("src/main.rs");
    let mut ed = editor_on(&file);
    ed.set_root(dir.clone());
    let buf = ed.current_buffer_id();

    // diagnostics arrive after didOpen and convert to render spans
    assert!(
        pump(&mut ed, |e| e.diag_view(buf).is_some()),
        "diagnostics never arrived"
    );
    {
        let d = ed.diag_view(buf).unwrap();
        assert_eq!(d.counts, (1, 1));
        let l0 = &d.spans[&0];
        assert_eq!(l0[0], (3, 7, Severity::Error));
        let l1 = &d.spans[&1];
        assert_eq!(l1[0], (0, 4, Severity::Warning));
        assert!(d.ghost[&0].1.contains("bad thing"));
    }

    // the compiler's-eye view (#93): a split projecting the same buffer,
    // annotated once the document-wide hints arrive — a lifetime, a type
    // hint in parts, a parameter name, a chaining type folded into a
    // trailing comment, a closing-brace label
    feed(&mut ed, "<C-w>v");
    feed(&mut ed, "gp");
    assert!(ed.focused_is_preview());
    let rows = |e: &mut Editor| -> Vec<String> {
        let doc = e.preview_doc_for(buf, 70);
        (0..doc.line_count()).map(|i| doc.plain(i)).collect()
    };
    let annotated = |e: &mut Editor| rows(e).iter().any(|r| r.contains(": Vec<i32>"));
    assert!(pump(&mut ed, annotated), "document hints never arrived");
    assert_eq!(
        rows(&mut ed),
        vec![
            "1 fn main<'a>() {",
            "    ↳ bad thing", // the diagnostics published on didOpen, in full
            "2 let x: Vec<i32> = vec![n: 1];  // Vec<i32>",
            "    ↳ iffy thing",
            "3 fn f() {}",
            "4 }  // fn main",
        ]
    );
    // an edit in the source window makes the hints stale at once — the
    // projection drops back to bare source rather than misplace them —
    // and the next flush brings a fresh set
    feed(&mut ed, "<C-w><C-w>");
    assert!(!ed.focused_is_preview());
    feed(&mut ed, "x");
    assert!(!annotated(&mut ed), "stale hints must not be shown");
    assert!(
        pump(&mut ed, annotated),
        "hints for the edited text never arrived"
    );
    feed(&mut ed, "u");
    feed(&mut ed, "<C-w><C-w>");
    assert!(ed.focused_is_preview());
    feed(&mut ed, "<C-w>q"); // close the projection window
    assert_eq!(ed.window_count(), 1);
    assert!(!ed.focused_is_preview());

    // hover opens the info float; any key dismisses it
    feed(&mut ed, "K");
    assert!(pump(&mut ed, |e| e.info_float.is_some()), "no hover float");
    assert!(ed.info_float.as_ref().unwrap()[0].contains("fn main()"));
    feed(&mut ed, "<Esc>");
    assert!(ed.info_float.is_none());

    // the line-scope hover splices TYPE hints (param-name hints filtered)
    // and appends the call signature with parameter types
    feed(&mut ed, "gK");
    assert!(pump(&mut ed, |e| e.info_float.is_some()), "no inlay float");
    let float = ed.info_float.as_ref().unwrap();
    assert!(float[0].contains(": Vec<i32>"));
    assert!(!float[0].contains("noisy:"), "param-name hints are noise");
    assert!(float.last().unwrap().contains("fn vec_of(n: usize)"));
    feed(&mut ed, "<Esc>");

    // goto definition jumps to the served location
    feed(&mut ed, "gd");
    assert!(
        pump(&mut ed, |e| e.cursor.line == 2),
        "definition jump missing"
    );
    assert_eq!(ed.cursor.col, 4);

    // code actions: menu opens, Enter applies the workspace edit
    feed(&mut ed, "gg ca");
    assert!(
        pump(&mut ed, |e| e.actions_menu.is_some()),
        "no actions menu"
    );
    assert_eq!(ed.actions_menu.as_ref().unwrap().actions.len(), 3);
    feed(&mut ed, "<CR>");
    assert!(text(&ed).starts_with("FIXED main()"), "edit applied");
    feed(&mut ed, "u");
    assert!(text(&ed).starts_with("fn main()"), "edit is one undo step");

    // #82 §8: an edit addressed to the end of the document (the line after
    // the final newline) appends, rather than landing at the start of the
    // last visible line
    feed(&mut ed, "gg ca");
    assert!(
        pump(&mut ed, |e| e.actions_menu.is_some()),
        "no actions menu"
    );
    feed(&mut ed, "kk<CR>"); // third action: "append tail"
    assert_eq!(
        text(&ed),
        "fn main() {\nlet x = vec![1];\nfn f() {}\n}\n// tail\n",
        "EOF edit must append"
    );
    feed(&mut ed, "u");

    // #82 §9: every mutation path reaches the server, not only keystrokes
    // (a) insert-mode bracketed paste
    feed(&mut ed, "gg0h");
    ed.paste_external("PASTED");
    feed(&mut ed, "<Esc>");
    assert!(
        pump(&mut ed, |_| server_saw_change(&log, "main.rs", "PASTED")),
        "insert-mode paste never reached the server"
    );
    feed(&mut ed, "u");
    // (b) a reload from disk
    fs::write(&file, "// reloaded\nfn main() {}\n").unwrap();
    feed(&mut ed, ":e!<CR>");
    assert!(text(&ed).starts_with("// reloaded"));
    assert!(
        pump(&mut ed, |_| server_saw_change(&log, "main.rs", "reloaded")),
        "reload never reached the server"
    );
    // (c) an edit followed at once by a buffer switch must still sync: the
    // old single global debounce let the new buffer swallow it
    feed(&mut ed, "ASTRANDED<Esc>");
    feed(&mut ed, "<C-p>other<CR>"); // open src/other.rs via the picker
    assert!(
        pump(&mut ed, |e| e
            .buffer
            .path
            .as_ref()
            .is_some_and(|p| p.ends_with("other.rs"))),
        "did not switch to other.rs"
    );
    assert!(
        pump(&mut ed, |_| server_saw_change(&log, "main.rs", "STRANDED")),
        "edit in the buffer we switched away from never reached the server"
    );
    feed(&mut ed, "<C-^>"); // back to main.rs for what follows
    assert!(ed.buffer.path.as_ref().unwrap().ends_with("main.rs"));

    // macro expansion opens a highlighted scratch split
    feed(&mut ed, " rm");
    assert!(
        pump(&mut ed, |e| e.window_count() == 2),
        "no expansion split"
    );
    assert!(text(&ed).contains("fn expanded()"));
    assert!(
        ed.buffer
            .path
            .as_ref()
            .unwrap()
            .to_string_lossy()
            .contains("expansion:demo")
    );
    feed(&mut ed, "<C-w>q");

    // ghost text toggle
    assert!(ed.ghost_text);
    feed(&mut ed, " dh");
    assert!(!ed.ghost_text);

    // rename (#97): the prompt opens pre-filled with the identifier under
    // the cursor; the server's edit spans two files, the second of which
    // is not open yet; each file is one undo step; :wa writes them all
    feed(&mut ed, "2G0w"); // onto `main` in `fn main() {}`
    assert_eq!(ed.cursor.line, 1);
    feed(&mut ed, " cn");
    assert!(matches!(ed.prompt, unei::editor::Prompt::Rename { .. }));
    assert_eq!(
        ed.cmdline, "main",
        "pre-filled with the name under the cursor"
    );
    assert_eq!(
        ed.cmdline_cursor, 4,
        "cursor at the end of the pre-filled name"
    );
    feed(&mut ed, "<Home>my_"); // and the name is editable, not append-only
    assert_eq!((ed.cmdline.as_str(), ed.cmdline_cursor), ("my_main", 3));
    feed(&mut ed, "<End><C-u>entry<CR>");
    assert!(
        pump(&mut ed, |e| text(e).contains("fn entry() {}")),
        "rename never applied to the current buffer"
    );
    let msg = ed.message.as_ref().unwrap().text.clone();
    assert!(msg.contains("2 places in 2 files"), "{msg}");
    assert!(msg.contains(":wa"), "{msg}");
    let other_id = ed
        .buffer_entries()
        .into_iter()
        .find(|e| e.name.ends_with("other.rs"))
        .expect("other.rs was opened to take the edit");
    assert!(other_id.modified, "edited, not yet written");
    assert!(
        ed.buffer_ref(other_id.id)
            .rope
            .to_string()
            .contains("fn entry() {}")
    );
    feed(&mut ed, "u");
    assert!(text(&ed).contains("fn main() {}"), "one undo step per file");
    feed(&mut ed, "<C-r>");
    feed(&mut ed, ":wa<CR>");
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("2 buffer(s) written")
    );
    assert!(fs::read_to_string(&file).unwrap().contains("fn entry() {}"));
    assert!(
        fs::read_to_string(dir.join("src/other.rs"))
            .unwrap()
            .contains("fn entry() {}")
    );
    // the server's refusal reaches the message line verbatim
    feed(&mut ed, "2G0w cn<C-u>reserved<CR>");
    assert!(
        pump(&mut ed, |e| e.message.as_ref().is_some_and(|m| m
            .text
            .contains("Cannot rename a reserved name"))),
        "server error not shown"
    );
    // local validation never bothers the server
    feed(&mut ed, " cn<C-u>1bad<CR>");
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("not an identifier")
    );
    feed(&mut ed, " cn<CR>"); // unchanged name
    assert!(ed.message.as_ref().unwrap().text.contains("same name"));
    feed(&mut ed, " cn<C-u><CR>"); // emptied = cancel
    assert!(ed.message.as_ref().unwrap().text.contains("cancelled"));

    ed.lsp_shutdown();
}
