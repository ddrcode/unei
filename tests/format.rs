//! Format-on-save via treefmt (ticket #21), exercised with a fake treefmt
//! binary placed on PATH. All scenarios run in one test function: PATH is
//! process-global and cargo runs tests in threads.

use std::fs;
use std::path::PathBuf;

use unei::core::buffer::Buffer;
use unei::editor::Editor;
use unei::editor::testing::{feed, text};

fn project(name: &str, with_config: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-fmt-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    if with_config {
        fs::write(dir.join("treefmt.toml"), "# fake config\n").unwrap();
    }
    dir
}

fn editor_on(path: PathBuf, content: &str) -> Editor {
    fs::write(&path, content).unwrap();
    let (buffer, existed) = Buffer::from_path(&path).unwrap();
    assert!(existed);
    let mut ed = Editor::new(buffer);
    ed.set_view(80, 22);
    ed
}

/// Installs a fake `treefmt` on PATH: uppercases the target file's content
/// (arg 5 per the editor's invocation), or sleeps when SLOW is requested.
fn install_fake_treefmt() -> PathBuf {
    let bin = std::env::temp_dir().join(format!("unei-fmt-bin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&bin);
    fs::create_dir_all(&bin).unwrap();
    let script = "#!/bin/sh\n\
        f=\"$5\"\n\
        if [ -n \"$UNEI_FAKE_SLOW\" ]; then sleep 5; exit 0; fi\n\
        if [ -n \"$UNEI_FAKE_FAIL\" ]; then echo boom >&2; exit 1; fi\n\
        tr 'a-z' 'A-Z' < \"$f\" > \"$f.tmp\" && mv \"$f.tmp\" \"$f\"\n";
    let path = bin.join("treefmt");
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let old = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", format!("{}:{old}", bin.display())) };
    bin
}

#[test]
fn format_on_save_scenarios() {
    install_fake_treefmt();

    // 1. config present: save rewrites through the formatter and reloads
    let dir = project("basic", true);
    let mut ed = editor_on(dir.join("a.txt"), "hello\n");
    feed(&mut ed, "Aworld<Esc>:w<CR>");
    assert_eq!(text(&ed), "HELLOWORLD\n", "buffer reloaded formatted");
    assert!(!ed.buffer.is_modified(), "reloaded buffer is clean");
    assert_eq!(
        fs::read_to_string(dir.join("a.txt")).unwrap(),
        "HELLOWORLD\n"
    );
    assert!(ed.message.as_ref().unwrap().text.contains("formatted"));

    // 2. undo restores the pre-format content (one undo step)
    feed(&mut ed, "u");
    assert_eq!(text(&ed), "helloworld\n");

    // 3. no config anywhere: file written verbatim, no formatting
    let dir2 = project("noconfig", false);
    let mut ed = editor_on(dir2.join("b.txt"), "plain\n");
    feed(&mut ed, ":w<CR>");
    assert_eq!(fs::read_to_string(dir2.join("b.txt")).unwrap(), "plain\n");
    assert!(!ed.message.as_ref().unwrap().text.contains("formatted"));

    // 4. formatter failure: file stays written, error shown, buffer intact
    unsafe { std::env::set_var("UNEI_FAKE_FAIL", "1") };
    let dir3 = project("fail", true);
    let mut ed = editor_on(dir3.join("c.txt"), "keep\n");
    feed(&mut ed, ":w<CR>");
    unsafe { std::env::remove_var("UNEI_FAKE_FAIL") };
    assert_eq!(text(&ed), "keep\n");
    let msg = ed.message.as_ref().unwrap();
    assert!(msg.error && msg.text.contains("boom"));
    assert_eq!(fs::read_to_string(dir3.join("c.txt")).unwrap(), "keep\n");

    // 5. config in a parent directory governs nested files
    let dir4 = project("nested", true);
    fs::create_dir_all(dir4.join("src")).unwrap();
    let mut ed = editor_on(dir4.join("src/d.txt"), "deep\n");
    feed(&mut ed, ":w<CR>");
    assert_eq!(text(&ed), "DEEP\n");

    // 6. timeout: the slow formatter is killed, save still succeeds
    unsafe { std::env::set_var("UNEI_FAKE_SLOW", "1") };
    let dir5 = project("slow", true);
    let mut ed = editor_on(dir5.join("e.txt"), "slow\n");
    let t0 = std::time::Instant::now();
    feed(&mut ed, ":w<CR>");
    unsafe { std::env::remove_var("UNEI_FAKE_SLOW") };
    assert!(
        t0.elapsed().as_millis() < 4000,
        "killed well before 5s sleep"
    );
    let msg = ed.message.as_ref().unwrap();
    assert!(msg.error && msg.text.contains("timed out"));
    assert_eq!(fs::read_to_string(dir5.join("e.txt")).unwrap(), "slow\n");
}
