# tree-sitter-just (vendored)

The `justfile` grammar by Casey Rodarmor and contributors, vendored as
generated C — unei compiles `parser.c` + `scanner.c` in `build.rs` and
never needs the tree-sitter CLI. The published crate pins an older
`tree-sitter` than unei links, and Cargo allows one native copy, so the
crate can't be a dependency; the C can.

- Source: https://github.com/casey/tree-sitter-just
- Commit: 5685543a6e64f66335e25518c9ae8ffa1dae3d01 (2026-03-25)
- License: Apache-2.0 (see LICENSE alongside)
- Queries: the nvim-flavoured `queries/just/{highlights,injections}.scm`
  (captures follow the names unei's theme understands)

To update: clone upstream, regenerate nothing — copy `src/parser.c`,
`src/scanner.c`, `src/tree_sitter/*.h` and the two queries here, and
bump the commit above.
