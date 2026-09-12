//! Compiles the tree-sitter grammars vendored under `grammars/` as
//! generated C, so the build needs only a C compiler, never the CLI:
//! 6502/ACME (#18, ours — `parser.c` generated from `grammar.js`) and
//! justfile (#108, upstream's `parser.c` + `scanner.c`, see its README).

use std::path::Path;

fn grammar(dir: &str, files: &[&str], lib: &str) {
    let src = Path::new("grammars").join(dir).join("src");
    let mut build = cc::Build::new();
    build.include(&src).warnings(false);
    for f in files {
        build.file(src.join(f));
        println!("cargo:rerun-if-changed=grammars/{dir}/src/{f}");
    }
    build.compile(lib);
}

fn main() {
    grammar("asm6502", &["parser.c"], "tree_sitter_asm6502");
    grammar("just", &["parser.c", "scanner.c"], "tree_sitter_just");
}
