# Design decisions

Append-only log of decisions that shape the implementation. Rules live in
[rules.md](rules.md); this file records how they get applied.

## 2026-09-06 — External changes: never overwrite, reload when safe (ticket #80)

A bug, not a feature gap: the editor wrote over files that an agent or
treefmt had changed underneath it, silently. The fix records each file's
**disk stamp** — (mtime, size) — when read or written (size rides along so a
rewrite inside one mtime tick still registers), and treats a moved stamp as
"someone else touched this". Two consequences. **`:w` refuses** on a moved
stamp (`:w!` / `:wq!` / `:x!` overwrite) — a hard stop rather than vim's
y/n prompt, because the editor has one message line and two commands read
better than a modal question. And a **one-second poll** on the current buffer
(the main loop already ticks for the LSP) applies vim's `autoread` logic:
a *clean* buffer reloads itself as one undoable change — an agent's edit just
appears, `u` brings the old text back — while a *dirty* buffer is flagged
`[!]`, warned once, and never touched; `:e` reloads (refusing over unsaved
edits), `:e!` discards and reloads. Our own writes and the treefmt round-trip
re-stamp through `mark_saved`, so they never read as external. Only the
current buffer is polled — a parked buffer is checked the moment it's shown.
Deletion on disk is deliberately not an alarm: the next `:w` simply recreates
the file. `:e {path}` stays absent — the picker is the one way to open.

## 2026-09-06 — Live grep: in-process, the picker's third source (ticket #72)

Project-wide grep (`Space g`) runs **in-process** — the `ignore` walker (the
file picker already uses it) plus unei's own `search::compile` regex (the same
smartcase Rust-regex dialect as `/`) — never a shelled-out `rg`. That keeps
the rule against running system commands intact AND reuses one search dialect
instead of adding a second. It's the picker's third source after files (#26)
and symbols (#62), but a genuinely different pipeline: the query *is* the
regex and re-runs on every keystroke (not a fuzzy filter over a static list),
each result carries a `(path, line, col)` jump target, and the preview frames
the matched file on the hit line rather than showing its head. The light
`PickerKind` enum earns its third variant here — but not a trait-based source
framework: three concrete branches still read more clearly than an abstraction
over three. Responsiveness is bought with bounds, not threads: a two-char
minimum, a 500-match cap, a 512 KiB per-file cap, and UTF-8-only reads (binary
skipped) keep each synchronous keystroke cheap on the modest repos unei
targets; if a giant tree ever makes it stutter, the LSP's reader-thread +
30 ms tick is the async pattern to borrow. This very likely retires the parked
line-search idea (#64) — grep is the stronger large-file/large-project nav.

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

## 2026-09-04 — Syntax highlighting: own tree-sitter engine, full reparse

Highlighting (ticket #17) drives tree-sitter core directly instead of the
`tree-sitter-highlight` crate: that crate's merged event stream mangles
ordering across injection layers (markdown inline/fences lost or misnested
captures), while direct control gives deterministic layering — captures
paint a per-byte canvas parents-before-children, injected languages parse
with included ranges and paint on top, innermost last. Queries come bundled
with the grammar crates (nvim-treesitter-lineage, including `@none` as the
reset capture); the capture→style table lives in `config/theme.rs` and the
grammar registry in `config/languages.rs` (adding/removing a language is a
registry entry + a Cargo dependency). Buffers re-highlight by full reparse
when their version changes, lazily at render — measured at ~5-10ms per
keystroke on a 1000-line Rust file (release); the named future optimization
is incremental parsing via `InputEdit` plumbed through `Buffer::insert/remove`.
Scope: highlighting only — indentation patterns and folds are separate
tickets. Files over 2MB and unregistered languages render plain (no
fallback, per the rules).

## 2026-09-04 — Formatting: treefmt is the single integration

Per the rules (external formatters, on save only) and the #21 discussion:
the editor shells out to `treefmt` after every successful write — config
discovery walks up from the file for `treefmt.toml`/`.treefmt.toml`; no
config means no formatting, silently, and no fallback chain. The flow is
write → format → reload: the reload is one undoable change and leaves the
buffer clean. The process is killed after `OPTIONS.format_timeout_ms`
(1500ms). Formatter choice lives in each project's treefmt.toml — project
config, not editor config, so zero-config holds. This repo dogfoods it
(treefmt.toml: rustfmt + nixpkgs-fmt; both in the dev shell).

## 2026-09-05 — rust-analyzer: hand-rolled client, utf-8 positions

The LSP client (ticket #6) is ~500 lines of std: a reader thread feeding a
channel, JSON-RPC framing by hand, and a poll-based main loop (30ms) so
server messages land while the editor idles. No async runtime, no lsp-types
crate — serde_json values and a typed event enum at the editor boundary.
utf-8 position encoding is negotiated so LSP columns are byte offsets
(converted to editor char columns in exactly one module). Documents sync
full-content, debounced 200ms. Diagnostics render as underlines — straight
red for errors, curly yellow for warnings via a custom ratatui backend that
repurposes the two blink modifier bits for Kitty underline SGR (ratatui
cannot express undercurl) — plus toggleable end-of-line ghost text and
statusline counts. The line-scope hover (`gK`) splices inlay-hint labels
into the current line. Scratch macro expansions get a fake `.rs` path for
highlighting and are never written unless explicitly saved.

## 2026-09-05 — Visual mode: three kinds, one non-yanking paste

Visual char/line/block (ticket #11) reuse the normal-mode dispatcher: in a
visual mode, motions extend the selection and operators consume it. `h` is
a left motion again (keyboard.lua remaps it in normal mode only) and `x`/`s`
alias delete/change. Block operations work in display cells (correct across
tabs and wide chars); a blockwise register kind joins char/line, and block
`c` replicates the top-line insert to every block line on Esc, inside the
same undo step. Deviation from vim, at the author's request (#10): visual
paste NEVER writes the replaced text to the register — select elsewhere and
paste the same content repeatedly. Deferred: visual `J`/`r`/`>`, dot-repeat
of visual operators, `p` count.

## 2026-09-05 — Search: Rust regex dialect, two scopes only

Search (ticket #8 phase 1) speaks exactly one pattern dialect: the Rust
`regex` crate — a deliberate break from vim's regex, per the single-way
rule. Smartcase (their nvim setting) is the only case behavior. `:s`
deviates from vim by design: no line ranges exist — bare `:s/…/…/[g]` is
FILE-scoped (`:%s` is an accepted spelling of the same thing), and invoked
from a visual selection it is selection-scoped. Vim's per-line first-match
vs `g` semantics are kept. Esc in normal mode calms hlsearch (the modern
mapping); `n`/`N` re-light it. Match caches are keyed by (buffer, version)
like every other derived view. Deferred to phase 2 (per the #8 comments):
multi-file fuzzy search and the popup replace form; also search-as-motion
(`d/…`).

## 2026-09-05 — Registers: yank/cut split, clipboard mirrors yanks

The unnamed register is replaced by two (ticket #10, option 2 as decided):
yanks write the YANK register (read by `p`/`P` and visual paste), deletes
and changes write the CUT register (read by `<leader>p`/`<leader>P` —
which the file picker vacated; it lives on Ctrl+P alone). Consequence
embraced deliberately: the `dd`+`p` line-move becomes `dd`+`<leader>p`,
in exchange for `p` always meaning "paste what I copied". No register
history — per the author, "if I need it one day, I will ask". Yanks (only)
mirror to the system clipboard via OSC 52 write; paste from the system
arrives through Kitty's bracketed paste (literal in insert, charwise put
in normal, selection-replace in visual), never as interpreted keys.

## 2026-09-06 — Text objects: `a` around, `n` inner (ticket #12)

The IJKL layout has no room for vim's `i`-objects: `i` is the up-motion in
operator-pending mode, so `diw` can't exist (the author's own nvim has the
same hole). The open question in #12 was whether inner objects get an
alternative prefix or are dropped. **Decided: `n` is the inner prefix**
(`dnw`, `cn(`, …) alongside the `a`-family — a natural mirror of the i→h
swap that freed `n`, and unused in operator-pending mode. `a`/`n` only
take on object meaning after an operator or in visual mode; a bare `a`
still appends and a bare `n` is still search-next. Ranges are computed by
plain text scanning, not tree-sitter (objects are structural, and the
one-mechanism rule keeps highlighting the sole tree-sitter consumer).
Objects: `w`/`W`, `p` (linewise), the bracket pairs (`(` `)` `b`; `{` `}`
`B`; `[` `]`; `<` `>`), and quotes (`"` `'` `` ` ``); on `d`/`c`/`y` and
in visual mode.

## 2026-09-06 — Syntax text objects: tree-sitter earns a second object class (ticket #78)

This **amends** the #12 stance ("objects are structural, not syntactic;
plain-scan, not tree-sitter"). That held for *lexical* objects — words,
paragraphs, brackets, quotes — which a scanner resolves exactly and tree-
sitter would only overcomplicate. But "select the enclosing **function** /
**type**" is inherently syntactic: no scanner can do it, and tree-sitter —
already the grammar authority for highlighting and the symbol picker — is the
only honest source. So `f` (function) and `c` (class: impl/struct/enum/union/
trait/mod) join the `a`/`n` family, resolved from a per-language `textobjects`
query (the same registry mechanism as the symbols query, #62). This does *not*
break the single-way rule: each object still has exactly one mechanism —
lexical objects stay plain-scan, syntactic objects are tree-sitter — with no
overlap or fallback between them. `af` is the whole node (linewise); `nf` its
body, braces and surrounding whitespace trimmed. The cursor may sit anywhere
inside the node (innermost enclosing match wins), so `daf` from deep in a
method deletes that method, `dac` the enclosing impl. Rust first; another
language is a `textobjects` query away.

## 2026-09-05 — Completion: LSP-only, manual, no snippets (ticket #22)

Iteration one is deliberately the smallest useful thing: Rust only,
rust-analyzer only, **manual** trigger. `Ctrl+N`/`Ctrl+P` in insert mode
request `textDocument/completion` through the same async client and
`lsp_tick` pump as hover — the response opens a cursor-anchored popup a
tick later; no auto-trigger-on-`.` (that debounce/round-trip-per-keystroke
complexity is iteration two). The server's list is fetched once and
filtered/ranked client-side as you keep typing (prefix hits before
substring hits, then the server's sortText), so narrowing costs no new
round-trip. Accept (`Enter`/`Tab`) replaces the typed prefix and folds
into the open insert session's single undo. **No snippets** (the ticket's
"hell no"): the client advertises `snippetSupport: false`, and any snippet
insert that still arrives is reduced to plain text — accepting a method
inserts its name, not a `${1:…}` placeholder dance. The `completion`
capability must be advertised in `initialize` or rust-analyzer returns
null. Local-word / ctags fallbacks and single-line AI completion stay
future, per the ticket.

## 2026-09-05 — The machine lens: K is total, and the half-life rule

Ticket #45. `K` means "tell me about the thing under the cursor" — one
verb, providers by context: rust-analyzer hover in rust files, the
machine lens everywhere else (numeric literals in any file type;
instruction facts and visual-selection cycle sums in assembly). What may
be compiled in is decided by the fact's HALF-LIFE: language and silicon
knowledge frozen for decades (65C02 timing, unchanged since 1983)
compiles in beside the grammars; codebase knowledge (a project's
register map, placeholder latencies) never does — that belongs to the
project's own docs, or someday its own language server. Corollaries:
cycle counting exists only where timing IS ISA knowledge (the 6502
family — on pipelined ISAs cycles belong to a core, so a future riscv
provider speaks of encodings, not time); and all assembly knowledge is
gated by the in-file modeline `; asm: <isa> [<assembler>]` (first five
lines, comment leader opaque — the format #18's highlighting will share)
because colors may guess a dialect but numbers must know it. Mnemonics
are an open set — macros look exactly like opcodes — so the lens answers
only from its table and reports the rest as not-counted, never guessed.

## 2026-09-06 — Disassembler: the opcode table, run backwards (ticket #65)

Opening a `.prg` shows its 65C02 disassembly instead of binary garbage.
The decode table is built once by INVERTING the forward opcode table
(`opcode_byte`) — the same ~150 (mnemonic, mode) rows the cycle table and
the lens already trust, re-mapped byte→instruction — so it cannot drift
from it; a unit test asserts every entry re-encodes to its own byte.
Output is re-assemblable ACME carrying an `; asm: 65c02 acme` modeline and
address+bytes trailing comments, which means the disassembly highlights
and answers `K` through the exact machinery that produced it: the table
decodes the image, then annotates its own output. This keeps the
single-mechanism rule — no bespoke listing renderer, no illegal-opcode
tables (undefined bytes are honest `!byte` data, and decoding resumes
after them). Detection is by extension (`.prg` = 2-byte load address then
image, unambiguous in the CBM/X16 world we target); the buffer binds to a
synthetic `<name>.prg.disasm.s` path so it colours and a `:w` saves the
disassembly as source rather than clobbering the binary. It is 65C02-only
by construction (it decodes the CMOS bit ops at `$x7`/`$xF`); an NMOS-only
image would mis-read those few slots — acceptable, and the emitted
modeline states the chip it assumed.

## 2026-09-06 — Binary files: read-only hex view (tickets #67, #69)

The editor never renders a binary as text. A `.prg` opens disassembled
(#65); everything else the editor can't decode — ROMs, `.bin`, object
files — opens as a full **hex dump in a read-only buffer** (#69), and the
picker previews a binary's head the same way (#67). One implementation of
binary-sniffing and hex-formatting lives in `core::hex`, shared by the
preview (first lines) and the open path (whole file, capped) — a single
mechanism, two consumers. `read_only` is a real `Buffer` flag: mutations
are inert, `save` is refused, and normal-mode edit commands report
"read-only" instead of silently swallowing the key, so the file on disk is
safe while you scroll, search and yank from it. Detection sniffs the head
(a NUL byte, non-UTF-8 that isn't a boundary-clipped multibyte char, or
dense control bytes) — a bare NUL test misses short 6502 images that carry
none. The alternative, simply refusing to open binaries, was rejected as
less useful for ROM inspection and less consistent with the disassembler's
"open the special file as a view" shape.

## 2026-09-05 — Preview: a per-window projection, a mode only on focus

Ticket #39 phase 1 resolves its own open question: the VIEW TRANSFORM is
window state (source vs rendered projection — two windows can show one
buffer both ways at once), and PREVIEW the keyboard mode simply is what
the editor is in while a projecting window holds focus (navigation only,
Enter jumps to source, mutating keys inert). The projection is a pure
function of (buffer, width) cached by version — the syntax-cache pattern —
and every rendered line carries its source line (the LineMap), which is
what makes follow-scrolling and jump-to-source possible and why a bespoke
renderer beats shelling out to any external previewer. Markdown renders
via the tree-sitter grammars already in the binary; fenced code highlights
through the grammar registry. Wrapped-paragraph lines all map to the
block's first source line (block-granular follow — fine at phase 1). Next
phases per the ticket: the rust fully-annotated view, csv tables.

## 2026-09-03 — Ticket #1 ships without soft wrap

The author's nvim uses `wrap` + `linebreak` (relevant for Markdown prose),
but #1 renders `nowrap` with horizontal scrolling. Soft wrap changes cursor
and viewport math substantially and deserves its own ticket.
