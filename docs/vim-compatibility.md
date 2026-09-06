# Vim compatibility

unei speaks vim's **grammar** — modal editing, operators × motions × counts,
text objects, registers, dot-repeat — so the muscle memory transfers. It is
not vim-*compatible*: it is a bespoke, zero-config editor with an opinionated
[constitution](rules.md), so some things are ported faithfully, some done
deliberately differently, and some left out on purpose.

This page is the map for a vim user. See [keymap.md](keymap.md) for the full
binding tables and [decisions.md](decisions.md) for the *why* behind the
deviations.

> The single most important difference: **navigation is IJKL, not HJKL**
> (`i` up, `k` down, `j` left, `l` right), and `h` enters insert mode where
> vim uses `i`. Everything below assumes that swap. See the note on it in
> [keymap.md](keymap.md).

## Ported — works the way you expect

**Modes.** Normal, insert, visual (character / line / **block**), command
line, and operator-pending — all present and behave as in vim.

**Motions** (all take counts, all work as operator targets):
`i k j l` (vim's `k j h l`) and arrows; `0` `^` `$` `Home` `End`;
`w W b B e E`; `gg` `G` (with a count line number); `{` `}`;
`f F t T` with `;` `,`; and `%` between matching `()[]{}`.

**Operators & edits.** `d c y` over motions, text objects, and counts
(`2d3w`); `D C Y`; `x X`; `r`; `s S`; `~`; `J` (join); `>>` `<<` and `>`/`<`
over motions/selections; `u` / `Ctrl+R` undo & redo (an insert session is one
unit); and `.` dot-repeat.

**Text objects.** The full around/inner families — word, WORD, paragraph,
the bracket pairs, and quotes — on `d`/`c`/`y` and in visual mode. (The prefix
is `a`/`n`, not `a`/`i` — see *Different*.)

**Marks & jumps.** `m{a-z}` set; `` `x `` / `'x`; `` `` `` / `''`;
`Ctrl+O` / `Ctrl+I` jumplist (crossing buffers), fed by `G`/`gg`/`{`/`}`,
search, marks, `%`, and `gf`.

**Search & substitute.** Incremental `/` `?`, `n` / `N`, `*`, hlsearch with
`:noh`, and `:s/pat/rep/[g]`. (Regex dialect and `:s` scoping differ — see
*Different*.)

**Visual mode.** `v` `V` `Ctrl+V`; operators and `~`; `o` / `O` to swap
ends / block corners; `p` to replace the selection; `gv` to reselect.

**Insert mode.** `Esc` / `Ctrl+C` / `Ctrl+[`; `Enter` copies the indent;
`Tab`; `Backspace` / `Del`; `Ctrl+W` (delete word); `Ctrl+U` (delete to
indent); `Ctrl+T` / `Ctrl+D` (indent / dedent); arrows.

**Command line & files.** `:w` `:q` `:q!` `:wq` `:x` `:qa` `:qa!`
`:bd` `:bd!` `:noh` `:{number}`, and `:w {path}` to name a scratch buffer.

**Windows, buffers, scrolling.** `Ctrl+W` splits/resizing/zoom; `Ctrl+^`
alternate buffer; `Ctrl+D` `Ctrl+U` `Ctrl+F` `Ctrl+B`, `zz` `zt` `zb`;
`ZZ` / `ZQ`.

## Different — same idea, changed on purpose

| Area | unei | vim |
|---|---|---|
| **Navigation** | `IJKL`; `h` = insert, `H` = insert at first non-blank | `HJKL`; `i` = insert |
| **Inner text objects** | prefix **`n`** (`dnw`, `cn(`) | prefix `i` (`diw`) — impossible here, `i` is a motion |
| **Registers** | two: a **yank** register (`p`/`P`) and a **cut** register (`Space p`/`Space P`). No named or numbered registers, no history | many named/numbered registers with `"x` |
| **System clipboard** | yanks mirror out via OSC 52; deletes never do | via `"+`/`"*` or `clipboard` option |
| **Search dialect** | the Rust `regex` crate; smartcase always on | vim's own regex; `ignorecase`/`smartcase` options |
| **Substitute scope** | `:s` acts on the whole file, or the visual selection — no line ranges, no `:%s` | `:s` needs a range; `:%s` for the file |
| **`Y`** | `y$` (nvim default) | `yy` |
| **Visual paste** | never clobbers the register | replaces the register |
| **Opening files** | the fuzzy picker (`Ctrl+P`) or `gf` — there is no `:e path` | `:e`, `:find`, netrw, … |
| **Formatting** | external only, on save (treefmt); no `=` or `gq` | built-in `=`, `gq`, `equalprg`, … |
| **Highlighting** | tree-sitter only, always on | built-in regex syntax + options |
| **Configuration** | compiled in; no `~/.vimrc`, no runtime `:map` | `.vimrc`, `:map`, `:set` at runtime |
| **Terminal** | Kitty only; uses its keyboard protocol (instant `Esc`, distinct `Ctrl+I`/`Tab`) and curly underlines | any terminal |

There are also a few **capabilities vim doesn't have**, worth knowing so you
reach for them: `gcc`/`gc` comment toggling; `gp` live Markdown preview;
`K` as a "tell me about this" key (rust-analyzer hover, or a machine lens for
numbers and 6502/65C02 assembly); `gK` a line-scope type annotator;
`Ctrl+N`/`Ctrl+P` LSP completion.

## Skipped — deliberately absent

From the [constitution](rules.md), by design:

- **Plugins** and any plugin API
- **Scripting / Vimscript** (no `:g`, `:v`, `:normal`, no expression engine)
- **Help files** (`:help`)
- **File browser** (netrw-style)
- **Macro recording** (`q`) — though `.` dot-repeat stays
- **Local leader** (the global leader, `Space`, is present)
- **Tabs** (tbd)
- **Relative line numbers**
- **Mouse support**
- **Terminal buffer** and running shell commands (`:!`, `:terminal`)
- **Multiple search/replace mechanisms** — regex is the only one

Falling out of "one way of doing things" and the above, these vim features are
also **not present**: named/numbered registers, ex global commands, `:s` line
ranges, folding, sessions (`:mksession`), spell check, digraphs, `=`/`gq`
reformatting, and runtime `:map` remapping (the keymap is compiled data — you
change it by editing `config/keymap.rs` and rebuilding, or by forking and
asking Claude to).

## Not yet — on the roadmap, not excluded

These simply aren't built yet (see the issue tracker): soft wrap, a Colemak
keymap variant, auto-triggered completion, a picker preview pane, and a few
small parity items (`gJ`, `Ctrl+A`/`Ctrl+X`, `ge`/`gE`).
