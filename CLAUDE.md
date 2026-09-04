# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

tailorED is a bespoke, personal vim-like terminal editor written in Rust (Ratatui as renderer). It is intentionally opinionated and zero-config: a monolith with no plugins and no external configuration. Work is ticket-driven: one GitHub issue → one branch → one PR.

## Architecture

The crate is a library (headless, fully testable editor core) plus a thin binary (`src/main.rs`, terminal event loop):

- `config/` — ALL configuration, compiled in: options (`OPTIONS`), the key→command tables (`keymap.rs` — the IJKL layout lives here), chrome palette. Behavior changes happen here first.
- `core/` — buffer (ropey rope + snapshot undo + newline invariant), grapheme/width-aware text helpers, motion resolution (`motion.rs` returns target + linewise/inclusive/exclusive kind), shared command enums.
- `editor/` — the modal state machine: `Editor::handle_key` consumes `Key`s with no terminal coupling; `normal.rs` (operators, pending state, registers, dot-repeat), `insert.rs`, `cmdline.rs`, `buffer_list.rs` (overlay); multi-buffer via a checkout model (current buffer lives in `Editor.buffer`, others parked in slots — see docs/decisions.md); `testing.rs` feeds key specs like `"cwfoo<Esc>"` for tests.
- `ui/` — Ratatui rendering only; reads editor state, never mutates semantics.
- `term.rs` — terminal lifecycle (raw mode, Kitty keyboard protocol, panic-safe restore).

Tests live in module `#[cfg(test)]` blocks and `tests/editing.rs` (golf-style: buffer + keys → expected buffer). Every behavior change should come with golf tests; remember the IJKL layout when writing key specs.

Design decisions that interpret the rules (tree-sitter-only highlighting, buffer newline invariant, keymap-as-data, snapshot undo) are logged in `docs/decisions.md` — append new ones there.

## Authoritative rules

`docs/rules.md` is the project constitution. Read it before making design decisions. **Agents must not modify `docs/rules.md` except in a dedicated, rules-only PR.**

Key constraints from it:

- **Zero-config, zero-plugin monolith.** All behavior is embedded in source; changes require recompilation. Minimal configurability (e.g. key mappings) lives in Rust files under a `config` module. When in doubt, skip configurability entirely.
- **Single way of doing things.** One mechanism per feature, no fallbacks — e.g. exactly one text-coloring method (not built-in + TreeSitter + LSP), one search/replace (standard regexp-based only).
- **Formatting is external.** Formatters run as external tools on save only; the editor just follows current indentation.
- **Kitty terminal is assumed.** Use Kitty extensions (e.g. curly underlines), unicode, and nerd fonts freely. No terminal integration in the other direction: no terminal buffer, no running system commands from the editor.
- **Deliberately excluded features** (do not add): plugins, scripting language, help files, file browser, macro recording, local leader, mouse support, relative line numbers, tabs (tbd).
- **Non-standard navigation keys**: IJKL instead of HJKL (I = up, K = down, J = left, L = right); H enters insert mode (Shift+H inserts at line start). This applies everywhere navigation keys appear, e.g. `Ctrl+w+i` moves to the panel above.

## Development environment

The toolchain comes from the Nix flake (direnv auto-loads it via `.envrc`; otherwise run `nix develop`). It provides `rustc`, `cargo`, `clippy`, `rustfmt`, `rust-analyzer`, and `cargo-mutants`.

Standard Cargo workflow:

- Build: `cargo build`
- Run: `cargo run`
- Test all: `cargo test`
- Test one: `cargo test <test_name>`
- Lint: `cargo clippy`
- Format: `cargo fmt`
- Mutation testing: `cargo mutants` (output in `mutants.out*/`, gitignored)
