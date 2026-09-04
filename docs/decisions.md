# Design decisions

Append-only log of decisions that shape the implementation. Rules live in
[rules.md](rules.md); this file records how they get applied.

## 2026-09-03 — Text coloring: tree-sitter only

The single text-coloring method (rules: "Single way of doing things") is
**tree-sitter**. It covers Rust, Markdown, and a future gasm-style RISC-V
grammar, works instantly without a language server, and survives
rust-analyzer restarts. LSP (ticket #6) provides diagnostics, hover, and
navigation — never colors. Confirmed by the author.

## 2026-09-03 — Keymap tables are data, not scattered bindings

Key → command mapping lives in `src/config/keymap.rs` as match tables over
semantic tokens, mirroring `keyboard.lua` from the author's dotfiles: IJKL
remapped in normal/visual/operator-pending contexts, `h`/`H` in normal mode
only (so `dh` deletes left). This keeps a future Colemak variant (the
author's nvim has a toggleable shim) a data change, not a rewrite.

## 2026-09-03 — Buffer invariant: every line is newline-terminated

The rope always ends with `\n`; an empty buffer is `"\n"` (one empty line).
Files are normalized on load. This mirrors vim's line-array model, keeps
linewise operators trivial (`line..line+1` always spans a terminator), and
matches editorconfig's `insert_final_newline`. Only LF is supported.

## 2026-09-03 — Undo is rope snapshots, not operation logs

Ropey ropes are persistent structures, so cloning is cheap; undo stores
`(rope, cursor, version)` snapshots per change transaction. Insert sessions
(entry through Esc, including the opening edit of `o`/`cw`) are one
transaction, like vim. Dot-repeat replays the recorded key sequence of the
last buffer-changing command.

## 2026-09-03 — Ticket #1 ships without soft wrap

The author's nvim uses `wrap` + `linebreak` (relevant for Markdown prose),
but #1 renders `nowrap` with horizontal scrolling. Soft wrap changes cursor
and viewport math substantially and deserves its own ticket.
