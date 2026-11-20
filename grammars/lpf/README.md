# lpf — unei's own grammar

A tree-sitter grammar for Lattice preference files (`.lpf`, the pin and
timing constraints of an ECP5/Diamond/nextpnr project), written for unei
(no ticket; 2026-11-20). No public grammar covers the format, so this is the
#18 route: `grammar.js` is the source, `src/` is generated, `build.rs`
compiles `src/parser.c`.

Regenerate after editing `grammar.js` (the CLI needs a JavaScript runtime
for the DSL — `node` is on the PATH here):

    cd grammars/lpf
    nix shell nixpkgs#tree-sitter --command tree-sitter generate

Generated with tree-sitter 0.27.1 (ABI 15). The grammar is deliberately
small and enumerates no keywords — see `docs/decisions.md`.
