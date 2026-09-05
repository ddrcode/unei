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
    dir
}

fn editor_on(path: &std::path::Path) -> Editor {
    let (buffer, _) = Buffer::from_path(path).unwrap();
    let mut ed = Editor::new(buffer);
    ed.set_view(80, 22);
    ed
}

/// Pumps lsp_tick until `done` or the deadline.
fn pump(ed: &mut Editor, mut done: impl FnMut(&Editor) -> bool) -> bool {
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
    let file = dir.join("src/main.rs");
    let mut ed = editor_on(&file);
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
    assert_eq!(ed.actions_menu.as_ref().unwrap().actions.len(), 2);
    feed(&mut ed, "<CR>");
    assert!(text(&ed).starts_with("FIXED main()"), "edit applied");
    feed(&mut ed, "u");
    assert!(text(&ed).starts_with("fn main()"), "edit is one undo step");

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

    ed.lsp_shutdown();
}
