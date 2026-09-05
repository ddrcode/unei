//! Compiles the bespoke tree-sitter grammars vendored under `grammars/`.
//! Currently just 6502/ACME (#18); its parser.c is generated from
//! `grammars/asm6502/grammar.js` with the tree-sitter CLI and checked in,
//! so the build needs only a C compiler, not the CLI.

use std::path::Path;

fn main() {
    let src = Path::new("grammars/asm6502/src");
    cc::Build::new()
        .include(src)
        .file(src.join("parser.c"))
        .warnings(false)
        .compile("tree_sitter_asm6502");
    println!("cargo:rerun-if-changed=grammars/asm6502/src/parser.c");
}
