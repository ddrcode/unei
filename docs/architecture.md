# Architecture

A library crate holding a fully headless editor, plus a thin binary that
connects it to a terminal. Everything interesting is testable without a TTY.

```
src/
├── config/       all compiled-in configuration (options, keymap, palette,
│                 theme, grammar registry)
├── core/         text model: rope buffer, motions, shared command enums
├── editor/       the modal state machine (headless)
├── syntax.rs     tree-sitter highlight cache
├── preview/      projections: markdown as the reader sees it, Rust as the
│                 compiler sees it — pure functions of the buffer (+ hints)
├── lsp/          rust-analyzer client (transport + protocol)
├── format.rs     treefmt-on-save
├── launch.rs     CLI → working folder + buffers rules
├── ui/           ratatui rendering (reads editor state, never mutates it)
├── term.rs       terminal lifecycle + the Kitty-flavored backend
└── main.rs       the event loop
```

## The text model (`core/`)

- **`buffer.rs`** — a `ropey` rope with an invariant: every line is
  newline-terminated (an empty buffer is `"\n"`). Undo is a stack of rope
  snapshots (ropes are persistent, cloning is cheap) grouped into
  transactions: one insert session, one operator application, one applied
  code action = one undo step. Saves are atomic (temp file + rename).
- **`text.rs`** — grapheme- and width-aware helpers: cluster boundaries,
  tab-aware display cells, char↔cell conversion. Cursor columns are
  *chars*; rendering positions are *cells*; LSP positions are *bytes*
  (utf-8 negotiated) — each conversion lives in exactly one place.
- **`motion.rs`** — motions resolve to a target plus a kind (linewise /
  inclusive / exclusive). Vim's exclusive-motion adjustments (`:h
  exclusive`) are implemented faithfully, nvim-verified.

## The editor (`editor/`)

`Editor::handle_key` consumes semantic `Key`s; no terminal types anywhere.
Normal/visual share a dispatcher (`normal.rs`) that owns pending state
(counts, operators, chords), registers (char/line/block), dot-repeat (key
replay), and visual selections. `insert.rs` and `cmdline.rs` are small.

**The checkout model** — used twice, deliberately:

- Of all buffers, the *current* one is checked out into `Editor.buffer`
  with its cursor/scroll as plain fields; the rest are parked in slots.
- Of all windows, the *focused* one's view state is checked out into the
  same flat fields; the rest live in the split tree (`windows.rs`).

Editing code therefore borrows disjoint fields (`&ed.buffer.rope` next to
`&mut ed.cursor`) with zero refactoring cost as buffers and splits were
added. Switching = swapping state in and out.

Overlays (file picker, buffer list, code actions, hover float) intercept
keys before the modal dispatch. `analyzer.rs` is the LSP glue and owns
byte↔char conversion.

## Rendering (`ui/`, `term.rs`)

Per frame: compute window rectangles from the split tree → for each window
build styled spans per visible line. `styled_visible` walks graphemes once,
layering: syntax capture colors → diagnostic underlines → selection / yank
flash backgrounds → inactive-panel dimming (50/60% blend toward the
background) → cursorline, then pads to full width. End-of-line ghost
diagnostics render into the padding.

ratatui can't express undercurl, so `term.rs` ships a custom backend that
repurposes the two unused blink modifier bits: `RAPID_BLINK` becomes a
straight red underline, `SLOW_BLINK` a curly yellow one (Kitty SGR `4:3`
with colored underline `58`). The backend also runs the Kitty keyboard
protocol (instant Esc, `Ctrl+I` ≠ Tab).

## Syntax highlighting (`syntax.rs`)

Drives tree-sitter core directly (the ready-made highlighter mangles event
order across injection layers). Per buffer version: parse, run the
highlight query, paint captures onto a per-byte canvas sorted so parents
paint before children — identical ranges tie-break first-pattern-wins for
bundled queries, registry overrides always win — then recurse into
injections (markdown fences → rust, `<script>` → javascript, …) painting
innermost-last. The canvas folds into per-line char spans; the whole thing
recomputes lazily at render when the buffer version changed (a few ms for
1k-line files; incremental parsing via `InputEdit` is the named upgrade
path).

## rust-analyzer (`lsp/`)

~500 lines of std: a reader thread parses Content-Length frames into a
channel; requests go straight to the child's stdin. No async runtime, no
lsp-types — serde_json values in, a typed `Event` enum out. utf-8 position
encoding is negotiated so columns are byte offsets. Documents sync
full-content, debounced 200 ms; saves trigger flycheck. Server→client
requests (configuration, progress, applyEdit) are answered so the server
never stalls. The main loop polls the terminal at 30 ms and pumps LSP
events between keys; workspace edits apply bottom-up as one undo step per
buffer.

## The event loop (`main.rs`)

```
loop {
  draw if dirty
  poll(30ms) → handle key(s)
  pump LSP events
  pump yank-flash timer
}
```

## Testing

- **Golf tests** (`tests/*.rs`): buffer + key sequence → expected buffer,
  through the full modal machine (`"cwfoo<Esc>"`). ~400 of them.
- **Oracles**: subtle vim semantics were verified against headless nvim
  (`nvim --headless -u NONE` + `feedkeys`) before being encoded as tests.
- **Fake servers on PATH**: a python LSP stand-in (`tests/fake_lsp.py`)
  drives the whole analyzer flow headlessly; a fake `treefmt` script does
  the same for formatting (timeout kill included).
- **Live verification**: features that are *visual* (colors, underlines,
  dimming, flash) are verified by scripted tmux sessions inspecting raw
  SGR output — tests assert data, the terminal is checked for truth.
