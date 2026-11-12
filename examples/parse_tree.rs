//! How unei's registry parses a file: parse and highlight timings, ERROR and
//! MISSING node counts, and — with a depth argument — the tree itself, field
//! names and anonymous nodes included (what a query needs to see).
//! `cargo run --release --example parse_tree -- <file> [depth]`

use std::time::Instant;

use tree_sitter::{Node, Parser, TreeCursor};
use unei::config::languages;

fn walk(cur: &mut TreeCursor, src: &[u8], depth: usize, max: usize, out: &mut String) {
    let n = cur.node();
    let indent = "  ".repeat(depth);
    let field = cur
        .field_name()
        .map(|f| format!("{f}: "))
        .unwrap_or_default();
    if n.is_named() {
        let leaf = if n.child_count() == 0 {
            format!(" {:?}", n.utf8_text(src).unwrap_or(""))
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{indent}{field}({}{leaf} [{}..{}]\n",
            n.kind(),
            n.start_byte(),
            n.end_byte()
        ));
    } else {
        out.push_str(&format!("{indent}{field}{:?}\n", n.kind()));
    }
    if depth < max && cur.goto_first_child() {
        loop {
            walk(cur, src, depth + 1, max, out);
            if !cur.goto_next_sibling() {
                break;
            }
        }
        cur.goto_parent();
    }
}

fn count(n: Node, errors: &mut usize, missing: &mut usize) {
    if n.is_error() {
        *errors += 1;
    }
    if n.is_missing() {
        *missing += 1;
    }
    let mut c = n.walk();
    for child in n.children(&mut c) {
        count(child, errors, missing);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: parse_tree <file> [depth]");
    let depth: Option<usize> = args.get(2).and_then(|s| s.parse().ok());
    let src = std::fs::read_to_string(path).expect("readable file");
    let lang = languages::detect(Some(std::path::Path::new(path))).expect("a registered language");
    let config = languages::config_for(lang).expect("grammar builds");
    let mut parser = Parser::new();
    parser.set_language(&config.language).expect("language");
    let t = Instant::now();
    let tree = parser.parse(&src, None).expect("parse");
    let parse_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let spans = unei::syntax::highlight_text(&src, lang);
    let highlight_ms = t.elapsed().as_secs_f64() * 1e3;
    let (mut errors, mut missing) = (0, 0);
    count(tree.root_node(), &mut errors, &mut missing);
    eprintln!(
        "{path}: {lang}, {} lines, {} bytes — parse {parse_ms:.1} ms, parse+highlight {highlight_ms:.1} ms, {errors} ERROR, {missing} MISSING, {} spans",
        src.lines().count(),
        src.len(),
        spans.iter().map(Vec::len).sum::<usize>()
    );
    if let Some(max) = depth {
        let mut out = String::new();
        walk(
            &mut tree.root_node().walk(),
            src.as_bytes(),
            0,
            max,
            &mut out,
        );
        print!("{out}");
    }
}
