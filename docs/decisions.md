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

## 2026-09-04 — Hard Kitty dependency is allowed

The author (who runs Kitty exclusively, on all machines) approved building on
a hard Kitty dependency — including refusing to start outside Kitty — once
the editor adopts features that need it (e.g. Kitty keyboard protocol
unconditionally, synchronized output, styled underlines for diagnostics).
Until such a feature lands, the current graceful behavior stays. The dev-time
tmux harness (tests drive the binary through tmux) must keep working —
any hard check needs an escape hatch for it (e.g. an env override).

## 2026-09-04 — Buffers: checkout model with stable ids

`Editor` keeps the displayed buffer checked out in its `buffer` field (with
cursor/scroll as plain fields), so the whole editing core keeps disjoint
field borrows. Background buffers are parked in slots with their view state;
switching swaps them. Buffer numbers are creation-ordered and never reused
(vim-style). Registers and f/t state are editor-global; undo, cursor and
scroll are per-buffer.

## 2026-09-04 — Splits: `Ctrl+w l` navigates; layout-flip is `Ctrl+w Space`

Ticket #4 assigned `key+l` to "swap layout", but the rules make IJKL
navigation universal and give `Ctrl+w+i` (panel above) as their own example
— so `l` focuses the window to the right and layout-flip (toggling a
container between horizontal and vertical) sits on `Ctrl+w Space`, matching
tmux's next-layout key. `Ctrl+w x` is a plain alias of `s` per the ticket
(it shadows vim's exchange-windows; `Ctrl+w r` covers swapping). Windows
form a tree with same-direction splits flattened into one container (vim's
frames); the focused window's view state is checked out into the editor
fields exactly like the current buffer is (same pattern, two levels), and
each window keeps its own alternate buffer, vim-style. Resize follows tmux
semantics (push the border in the pressed direction; steps live in
`config::OPTIONS`). Zoom is a render-level flag that any window operation
clears.

## 2026-09-04 — File picker: nucleo + ignore, one mechanism each

Fuzzy matching is nucleo (the Helix engine) and file listing is ripgrep's
`ignore` walker (gitignore honored even outside git repos, hidden files
skipped) — one matcher, one walker, no fallbacks. The picker is the single
file-opening mechanism (no `:e`); it dedupes against open buffers by
canonical path. Launched on a directory the editor opens straight into the
picker (resolving the ticket's TBD). Ctrl+Enter creates the queried path:
parent directories eagerly, the file itself on first `:w`; new paths must
stay inside the working folder. Preview is #26.

## 2026-09-03 — Ticket #1 ships without soft wrap

The author's nvim uses `wrap` + `linebreak` (relevant for Markdown prose),
but #1 renders `nowrap` with horizontal scrolling. Soft wrap changes cursor
and viewport math substantially and deserves its own ticket.
