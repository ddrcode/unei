//! Hot-path probe: the editor's real workloads, timed (ticket #90).
//!
//!     cargo run --release --example perf_probe
//!
//! Every workload goes through the public API the editor itself uses, so
//! the numbers reflect what a keystroke, a frame or a picker query costs.
//! Each prints `name<TAB>ms`; take the min over a few runs, and compare
//! builds or commits — the absolute values are only meaningful on one
//! machine. `PROBE_HEIGHT` (default 60) sets the window height of the
//! editing workloads, which is what the scroll math scales with (#89).

use std::path::PathBuf;
use std::time::Instant;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use unei::core::text::{line_graphemes, wrap_rows};
use unei::editor::testing::{editor_from, editor_with_buffers, feed};
use unei::{syntax, ui};

fn timed(name: &str, f: impl FnOnce()) {
    let t = Instant::now();
    f();
    println!("{name}\t{:.1}", t.elapsed().as_secs_f64() * 1000.0);
}

fn main() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let big = std::fs::read_to_string(repo.join("src/editor/normal.rs")).unwrap();
    let height: usize = std::env::var("PROBE_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    // Frames: scroll through a highlighted Rust file, editing every 10th
    // line so a tree-sitter reparse lands in the mix.
    timed("render_300_frames_30_reparses", || {
        let mut ed = editor_with_buffers(&[("normal.rs", &big)]);
        ed.set_view(200, height);
        let mut term = Terminal::new(TestBackend::new(200, height as u16 + 2)).unwrap();
        for i in 0..300 {
            feed(&mut ed, "k");
            if i % 10 == 0 {
                feed(&mut ed, "hx<Esc>");
            }
            term.draw(|f| ui::render(f, &mut ed)).unwrap();
        }
    });

    // Tree-sitter parse plus capture walk from scratch (the picker preview).
    timed("highlight_text_x20", || {
        let mut n = 0;
        for _ in 0..20 {
            n += syntax::highlight_text(&big, "rust").len();
        }
        assert!(n > 0);
    });

    // The soft-wrap primitive over every line, 200 passes.
    timed("wrap_all_lines_x200", || {
        let mut n = 0;
        for _ in 0..200 {
            for line in big.lines() {
                n += wrap_rows(&line_graphemes(line, 4), 80).len();
            }
        }
        assert!(n > 0);
    });

    // Live grep over this repo, one search per typed character.
    timed("grep_5_queries_x3", || {
        let mut ed = editor_from("x\n");
        ed.set_root(repo.clone());
        for _ in 0..3 {
            feed(&mut ed, "<Space>gEditor<Esc>");
        }
    });

    // Fuzzy file picker over a large tree (the cargo registry, ~20k files)
    // when one is around; this repo otherwise.
    let tree = cargo_registry().unwrap_or_else(|| repo.clone());
    timed("picker_walk_and_7_refilters", || {
        let mut ed = editor_from("x\n");
        ed.set_root(tree.clone());
        feed(&mut ed, "<C-p>editmod<Esc>");
    });

    // Insert mode: 8k keystrokes; then undo/redo and 500 dd; then 1000
    // search hops through the file.
    let mut ed = editor_with_buffers(&[("normal.rs", &big)]);
    ed.set_view(200, height);
    timed("type_8k_keys", || {
        let mut spec = String::from("h");
        for _ in 0..100 {
            spec.push_str(
                "let value = compute(alpha, beta) + gamma * delta - epsilon / zeta; // note<CR>",
            );
        }
        spec.push_str("<Esc>");
        feed(&mut ed, &spec);
    });
    timed("undo_redo_500dd", || {
        feed(&mut ed, "u<C-r>gg500ddu<C-r>");
    });
    timed("search_1000_hops", || {
        feed(&mut ed, "/fn <CR>");
        for _ in 0..1000 {
            feed(&mut ed, "n");
        }
    });
}

fn cargo_registry() -> Option<PathBuf> {
    let src = PathBuf::from(std::env::var("HOME").ok()?).join(".cargo/registry/src");
    std::fs::read_dir(src)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
}
