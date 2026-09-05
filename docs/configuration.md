# Configuration

There is none — at runtime. Everything lives in `src/config/` as plain Rust,
and changing anything means editing a constant and rebuilding:

```sh
$EDITOR src/config/mod.rs && cargo build --release
```

That's the deal this project makes: no config files to drift, no plugin API
to break, one source of truth. This document is the tour of what's there.

## `config/mod.rs` — `OPTIONS`

| Field | Default | Meaning |
|---|---|---|
| `tabstop` | 4 | display width of a tab |
| `shiftwidth` | 4 | indent unit (insert-mode Tab) |
| `expandtab` | true | Tab inserts spaces |
| `scrolloff` | 15 | context lines kept around the cursor |
| `number` | true | absolute line numbers (relative is banned by the rules) |
| `cursorline` | true | highlight the cursor line, full width |
| `final_newline` | true | ensure trailing newline on save |
| `resize_step_cols` | 5 | `Ctrl+W Alt+j/l` border step |
| `resize_step_rows` | 2 | `Ctrl+W Alt+i/k` border step |
| `format_on_save` | true | run treefmt after every write |
| `format_timeout_ms` | 1500 | formatter budget before it's killed |
| `yank_flash_ms` | 250 | yank highlight duration; 0 disables |

## `config/keymap.rs` — the keymap

Key → command tables, one function per context (`normal_token`,
`leader_token`, `window_token`, `picker_token`, `list_token`,
`visual_extra`, `operator_extra_motion`) plus the `LEADER` constant (Space).
Rebinding a key is moving a match arm. See [keymap.md](keymap.md) for the
current layout.

## `config/palette.rs` — chrome colors

Material-oceanic constants for everything non-syntax: backgrounds, gutter,
statusline mode badges, floats, selection (`SELECTION_BG`), yank flash
(`YANK_FLASH_BG`), and `INACTIVE_KEEP` — the percentage of color kept in
unfocused panels (60%; the Shade-nvim dimming).

## `config/theme.rs` — syntax styles

The tree-sitter capture → style table: `CAPTURES` (names) and `STYLES`
(color + modifiers), kept in lockstep by a compile-time assert. Dotted
captures fall back to their longest configured prefix (`keyword.control`
inherits `keyword`). Comments are italic; markdown titles bold blue; the
whole material-oceanic mapping is ~30 lines and begs to be tweaked.

## `config/languages.rs` — the grammar registry

One `LangSpec` per language: name, file extensions, exact filenames
(`Cargo.lock` → toml), the grammar function, its bundled queries, and an
optional `highlights_extra` — a query snippet appended after the bundled
one whose patterns win ties (used to recolor toml/yaml keys without
vendoring whole query files).

**Adding a language** is a two-step recipe:

1. `cargo add tree-sitter-<lang>`
2. Add the `LangSpec` entry (grammar + `HIGHLIGHTS_QUERY` + injections
   query if the crate ships one), bump the array size, rebuild.

Injections resolve by registry name and aliases — markdown fences saying
` ```js ` reach JavaScript because `js` is a registered alias.

## Formatting: the one external config

The editor itself stays zero-config, but *formatters* are project
configuration: drop a `treefmt.toml` in a project root and every save runs
through it. No file → no formatting. This repo's own `treefmt.toml`
(rustfmt + nixpkgs-fmt) is the example.

## Where the reasons live

Behavior-shaping decisions and their whys are logged append-only in
[decisions.md](decisions.md); the immutable project constitution is
[rules.md](rules.md).
