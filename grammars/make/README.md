# make — queries only

The Makefile grammar is the [`tree-sitter-make`](https://github.com/tree-sitter-grammars/tree-sitter-make)
crate (1.1.1), linked as a normal dependency: it depends only on
`tree-sitter-language`, so nothing is vendored here.

What lives here are unei's **own queries**. The crate's bundled
`highlights.scm` is written in nvim-treesitter's legacy capture names
(`@conditional`, `@include`, `@exception`) that the theme's dotted-prefix
mapping cannot resolve, and it never colours a rule's target. These are
after nvim-treesitter's current `make` queries, in the theme's vocabulary
and ordered for the engine's first-wins ties (specific before general —
see docs/decisions.md, 2026-11-20).
