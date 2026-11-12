# tree-sitter-systemverilog (vendored)

Gonzalo M. Larumbe's full IEEE 1800-2023 SystemVerilog grammar, vendored as
generated C — unei compiles `parser.c` in `build.rs` and never needs the
tree-sitter CLI. The published crate pins `tree-sitter 0.25`, unei links
0.27, and Cargo allows one native copy, so the crate can't be a dependency;
the C can (the route opened for `just`, #108).

- Source: https://github.com/gmlarumbe/tree-sitter-systemverilog
- Commit: 08c5e379b9b6022d4c433a9249e6d54dedd051ba (2026-10-21) — the
  revision nvim-treesitter pins
- License: MIT (see LICENSE alongside)
- Size: 65 MB of generated tables (21 567 parse states). Compiles in ~3.5 s
  at any optimisation level and adds ~23 MB to the binary — measured, see
  docs/decisions.md under #117. Git stores it compressed (~3 MB).
- Queries: nvim-treesitter's `runtime/queries/systemverilog/{highlights,
  injections}.scm` at 2c30e515ebe79037ab8d15f7e59c0e2690f50626 (2025-10-30),
  Apache-2.0 (`queries/LICENSE`). Upstream's own `highlights.scm` leaves bare
  literals uncoloured; nvim's does not, and its captures resolve through the
  theme's dotted-prefix mapping. The `comment` injection names a grammar unei
  doesn't register and is skipped; the macro-body injection resolves through
  the `verilog` alias.
- Symbols and text objects: unei's own queries in `src/config/languages.rs`.

To update: clone upstream, copy `src/parser.c` and `src/tree_sitter/*.h`
here (no scanner); refresh the two queries from nvim-treesitter; bump the
commits above.
