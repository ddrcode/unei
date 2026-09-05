# Unei

A bespoke, zero-config, vim-like terminal editor — made to measure for exactly one person, and written entirely by an AI.

## Why this exists

After years of Neovim, the editor itself was never the problem — the *configuration* was. Plugins break, APIs drift, and a personal setup becomes a small second job. The usual answer is more customization: another plugin, another layer of config, always approximating the tool you actually want, always with friction.

This project takes the other road. Thanks to AI-assisted development it became practical not to customize an editor, but to **produce one**: hardcode every personal preference directly into the source, compile it, and use it. No plugin API, no config files, no compromises — when the tool is wrong, the tool gets changed. That shift — *from customizing tools to producing them* — is the experiment this repository documents.

**Every line of code here was written by Claude** (Anthropic's Claude Code), directed through GitHub issues and conversation. The human behind it wrote tickets, tested builds, and made decisions; the implementation, tests, and documentation are AI-generated. See [docs/decisions.md](docs/decisions.md) for the running log of design decisions made along the way.

## Core ideas

- **Zero configuration.** All behavior is compiled in. The `config/` module *is* the configuration — editing it requires rebuilding, and that's the point. ([docs/configuration.md](docs/configuration.md))
- **One way of doing things.** One highlighting engine (tree-sitter), one formatter integration (treefmt), one file-opening mechanism (the picker), one search dialect (Rust regex). No fallback chains.
- **Kitty is the terminal.** The editor assumes [Kitty](https://sw.kovidgoyal.net/kitty/) and uses its extensions freely: the keyboard protocol (instant Esc, distinct Ctrl+I/Tab), curly underlines for diagnostics, true color everywhere.
- **A personal keymap.** Navigation is IJKL, not HJKL, with `h` entering insert mode. This is not configurable — it's *the* layout. ([docs/keymap.md](docs/keymap.md))
- **Vim philosophy, not vim compatibility.** It began as a vim-like editor for Rust and Markdown and kept growing. Modal editing, operators × motions, counts, registers — with deliberate deviations where vim's defaults annoy (visual paste never clobbers the register; ghost diagnostics hide while typing), and capabilities vim never had at all (live Markdown preview, per-instruction assembly intelligence).

## Features

- Modal editing: normal, insert, visual (char / line / **block**, with block-change replication), command line
- Vim grammar: operators `d c y` × motions × counts, `f/t` finds, undo/redo as single units, dot-repeat, registers (char / line / block), yank flash
- **Buffers**: `Ctrl+^` alternate, floating buffer list, vim-style numbering
- **Splits**: vim/tmux hybrid — `Ctrl+w` chord, tmux-style resizing, zoom, layout flip; per-window statuslines
- **Fuzzy file picker** (`Ctrl+P`): gitignore-aware listing, nucleo matching, open-in-split, create-file-with-parents
- **Search**: incremental `/` `?`, `n`/`N`, `*`, hlsearch, `:s` substitution — one dialect, Rust regex, smartcase
- **Tree-sitter highlighting**: Rust, Markdown (with fenced-code injection), TOML, YAML, JSON, JS, HTML, Bash, Nix, Python, CSS, and a bespoke **6502/ACME** assembler grammar — material oceanic theme
- **rust-analyzer**: diagnostics with straight-red / **curly-yellow** underlines, end-of-line ghost text, hover (`K`), goto definition (`gd`), code actions, macro expansion, and a line-scope type annotator (`gK`) with no nvim equivalent
- **Markdown preview** (`gp`): a live, side-by-side rendered view that follows the cursor as you edit — headings, tables, task lists, and syntax-highlighted code fences. Vim never came with this.
- **Assembly intelligence vim never had**: on 6502/65C02, `K` shows an instruction's one-line description and the flags it sets, its opcode byte and addressing mode, and cycle counts *including the NMOS-vs-CMOS differences* — plus a cycle **sum** over a visual selection, and a number-base lens (hex / dec / bin / byte split) that works in any file
- **Clipboard**: separate yank and cut registers; yanks mirror to the system clipboard (OSC 52); bracketed paste
- **Format on save** via treefmt (project-level `treefmt.toml`; silence otherwise)
- Jumplist (`Ctrl+O`/`Ctrl+I`), dimmed inactive panels, full-width cursorline, scrolloff 15

## Make it yours

unei is not built to be configured. It is built to be **forked and re-told**.

That is the whole thesis, not a slogan. A traditional editor ships a binary plus a configuration language to bend it toward you — a plugin API, config files, an approximation with friction. unei has none of that, on purpose: its configuration language is the one that produced it — *a conversation with Claude, over the source itself*. For its author that meant hardcoding every preference. For you it means forking the repo and describing, in plain language, the editor **you** want.

The preference surface is small and data-shaped by design. Point a Claude session at your fork and ask:

- **Other keys?** The bindings are data tables in [`src/config/keymap.rs`](src/config/keymap.rs) — IJKL lives there precisely so a variant is a table edit. *"Navigate with WASD; put insert on E."*
- **Other colors?** [`src/config/palette.rs`](src/config/palette.rs) and [`src/config/theme.rs`](src/config/theme.rs). *"Make it Gruvbox."*
- **Other languages?** The grammar registry is [`src/config/languages.rs`](src/config/languages.rs), one entry each. *"Add Go, drop the ones I don't use."*
- **Fewer features?** Most are self-contained modules — `lens.rs` (the assembly & number lens), `preview.rs` (markdown preview), the rust-analyzer client under `lsp/`. *"Remove the assembly support entirely."*
- **Not on Kitty?** That one is load-bearing — but ask, and find out what it costs.

Tell your session to read [docs/decisions.md](docs/decisions.md) first. It records *why* each choice was made — IJKL over HJKL, tree-sitter as the only colorizer, snapshot undo — so the re-tailoring goes with the grain instead of fighting design it can't see the reason for.

What you get is not *your config for unei*; it is *your editor*, which merely started as unei. That is the experiment: when producing software is this cheap, a personal fork replaces the config file. Configuration, in the old sense, is dead — which may deserve a blog post of its own.

## Running

```sh
nix develop          # toolchain via the flake (rustc, cargo, rust-analyzer, treefmt…)
cargo build --release
./target/release/unei            # scratch buffer
./target/release/unei src/       # directory → opens into the file picker
./target/release/unei foo.rs bar.md
```

Best experienced in Kitty. Under tmux, italics and instant-Esc depend on your tmux terminfo (`tmux-256color` recommended).

## Status

Personal daily driver for Rust, Markdown, and 6502 assembly. Development is ticket-by-ticket in this repo's issues; the roadmap *is* the issue list.

## The name

The same four keys, a different keyboard. Navigation here is **IJKL** — `i` up, `k` down, `j` left, `l` right — and if you type those four physical keys on a [Colemak](https://colemak.com) layout, they spell **unei** (QWERTY `I J K L` sit where Colemak types `U N E I`). Colemak support is on the roadmap; the name got there first. It also reads as 運営 — Japanese *un'ei*, "operations," the running of a thing — and keeps company with the author's other project, Maiko.

## Docs

- [docs/keymap.md](docs/keymap.md) — every binding, and why IJKL
- [docs/configuration.md](docs/configuration.md) — the config-as-code tour
- [docs/architecture.md](docs/architecture.md) — how it's built
- [docs/decisions.md](docs/decisions.md) — the decision log
- [docs/rules.md](docs/rules.md) — the project constitution
