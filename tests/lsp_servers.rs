//! Two language servers side by side (ticket #118): the fake server
//! (tests/fake_lsp.py) installed on PATH as both `rust-analyzer` and
//! `slang-server`, over a project holding Rust next to SystemVerilog. Each
//! file reaches the server of its language and no other; what a server
//! never advertised is never asked of it. One test fn: PATH is
//! process-global.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::file_picker::PickerKind;
use unei::editor::testing::feed;

fn install_fakes() {
    let bin = std::env::temp_dir().join(format!("unei-lsp2-bin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&bin);
    fs::create_dir_all(&bin).unwrap();
    let script_path = std::fs::canonicalize("tests/fake_lsp.py").unwrap();
    for name in ["rust-analyzer", "slang-server"] {
        let script = format!(
            "#!/bin/sh\nUNEI_FAKE_LSP_NAME={name} exec python3 \"{}\"\n",
            script_path.display()
        );
        let path = bin.join(name);
        fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let old = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", format!("{}:{old}", bin.display())) };
}

/// One line of the fakes' log: which server saw which document how.
struct Seen {
    server: String,
    method: String,
    uri: String,
    language_id: String,
}

fn server_docs(log: &Path) -> Vec<Seen> {
    let Ok(raw) = fs::read_to_string(log) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|v| Seen {
            server: v["server"].as_str().unwrap_or("").to_string(),
            method: v["method"].as_str().unwrap_or("").to_string(),
            uri: v["uri"].as_str().unwrap_or("").to_string(),
            language_id: v["languageId"].as_str().unwrap_or("").to_string(),
        })
        .collect()
}

fn saw(log: &Path, server: &str, method: &str, file: &str) -> bool {
    server_docs(log)
        .iter()
        .any(|s| s.server == server && s.method.ends_with(method) && s.uri.ends_with(file))
}

/// A repo (`.git` is slang-server's root marker) holding a cargo crate and
/// an `hdl/` folder of SystemVerilog.
fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-lsp2-proj-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("hdl")).unwrap();
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
    fs::write(
        dir.join("src/main.rs"),
        "fn main() {\nlet x = vec![1];\nfn f() {}\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("hdl/blink.sv"),
        "module blink(input wire clk, output wire led);\n  logic step = 0;\n  assign led = step;\nendmodule\n",
    )
    .unwrap();
    dir
}

fn editor_on(path: &Path) -> Editor {
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
fn each_language_gets_its_own_server_side_by_side() {
    install_fakes();
    let dir = project();
    let canon = fs::canonicalize(&dir).unwrap();
    let log = dir.join("server.log");
    unsafe { std::env::set_var("UNEI_FAKE_LSP_LOG", &log) }; // before the servers spawn
    let mut ed = editor_on(&dir.join("hdl/blink.sv"));
    ed.set_root(dir.clone());
    let sv = ed.current_buffer_id();

    // a SystemVerilog file starts slang-server — rooted at the repo, the
    // `.git` holder — and its diagnostics arrive like rust-analyzer's do
    assert!(
        pump(&mut ed, |e| e.diag_view(sv).is_some()),
        "slang-server's diagnostics never arrived"
    );
    assert_eq!(ed.diag_view(sv).unwrap().counts, (1, 1));
    assert_eq!(ed.lsp_servers(), vec![("slang-server", canon.clone())]);
    {
        let docs = server_docs(&log);
        let open = docs
            .iter()
            .find(|s| s.method == "textDocument/didOpen")
            .expect("didOpen logged");
        assert_eq!(open.server, "slang-server");
        assert!(open.uri.ends_with("hdl/blink.sv"));
        assert_eq!(open.language_id, "systemverilog", "the spec's languageId");
    }

    // hover, definition: the generic methods work as they do for Rust
    feed(&mut ed, "K");
    assert!(pump(&mut ed, |e| e.info_float.is_some()), "no hover float");
    feed(&mut ed, "<Esc>");
    feed(&mut ed, "gd");
    assert!(
        pump(&mut ed, |e| e.cursor.line == 2),
        "definition jump missing"
    );
    assert_eq!(ed.cursor.col, 4);

    // the line-scope hover (gK) splices the hints but asks for no call
    // signature: slang-server advertised none, and asking would never be
    // answered — the float is the annotated line alone
    feed(&mut ed, "gg");
    feed(&mut ed, "gK");
    assert!(pump(&mut ed, |e| e.info_float.is_some()), "no inlay float");
    {
        let float = ed.info_float.as_ref().unwrap();
        assert_eq!(float.len(), 1, "no signature line: {float:?}");
        assert!(float[0].contains(": Vec<i32>"));
    }
    feed(&mut ed, "<Esc>");

    // rust-analyzer's own method is not asked of another server
    feed(&mut ed, " rm");
    let msg = ed.message.as_ref().unwrap().text.clone();
    assert!(msg.contains("rust-analyzer only"), "{msg}");
    assert_eq!(ed.window_count(), 1, "no expansion split");

    // a Rust file alongside starts rust-analyzer; both clients stay alive
    feed(&mut ed, "<C-p>main<CR>");
    assert!(
        pump(&mut ed, |e| e
            .buffer
            .path
            .as_ref()
            .is_some_and(|p| p.ends_with("main.rs"))),
        "did not switch to main.rs"
    );
    let rs = ed.current_buffer_id();
    assert!(
        pump(&mut ed, |e| e.diag_view(rs).is_some()),
        "rust-analyzer's diagnostics never arrived"
    );
    let mut servers = ed.lsp_servers();
    servers.sort();
    assert_eq!(
        servers,
        vec![
            ("rust-analyzer", canon.clone()),
            ("slang-server", canon.clone())
        ]
    );
    assert!(saw(&log, "rust-analyzer", "didOpen", "src/main.rs"));
    assert!(
        server_docs(&log)
            .iter()
            .any(|s| s.server == "rust-analyzer" && s.language_id == "rust")
    );

    // the diagnostics list gathers every server's publications
    feed(&mut ed, " dd");
    {
        let p = ed.file_picker.as_ref().unwrap();
        assert_eq!(p.kind, PickerKind::Locations);
        assert_eq!(p.matches.len(), 4, "two servers, two diagnostics each");
        assert_eq!(p.item(&p.matches[0]), "E hdl/blink.sv:1: bad thing");
        assert_eq!(p.item(&p.matches[1]), "E src/main.rs:1: bad thing");
    }
    feed(&mut ed, "<Esc>");

    // an edit in the .sv reaches slang-server, and only slang-server
    feed(&mut ed, "<C-^>");
    assert!(ed.buffer.path.as_ref().unwrap().ends_with("blink.sv"));
    feed(&mut ed, "x");
    assert!(
        pump(&mut ed, |_| saw(
            &log,
            "slang-server",
            "didChange",
            "blink.sv"
        )),
        "the edit never reached slang-server"
    );
    for s in server_docs(&log) {
        let (rs_file, sv_file) = (s.uri.ends_with(".rs"), s.uri.ends_with(".sv"));
        assert!(
            (rs_file && s.server == "rust-analyzer") || (sv_file && s.server == "slang-server"),
            "{} saw {}",
            s.server,
            s.uri
        );
    }

    ed.lsp_shutdown();
    assert!(ed.lsp_servers().is_empty());
}
