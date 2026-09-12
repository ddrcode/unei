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
(`2d3w`); `D C Y`; `x X`; `r`; `s S`; `~`; `J` / `gJ` (join, with / without a
space); `Ctrl+A` / `Ctrl+X` (increment / decrement the number under the
cursor — decimal, `0x`/`$` hex, `0b`/`%` binary, preserving width and case);
`>>` `<<` and `>`/`<` over motions/selections; `u` / `Ctrl+R` undo & redo (an
insert session is one unit); and `.` dot-repeat.

**Text objects.** The full around/inner families — word, WORD, paragraph,
the bracket pairs, and quotes — on `d`/`c`/`y` and in visual mode, plus
**syntax-aware `f` (function) and `c` (class/type)** via tree-sitter, which
vim needs a plugin for. (The prefix is `a`/`n`, not `a`/`i` — see *Different*.)

**Marks & jumps.** `m{a-z}` set; `` `x `` / `'x`; `` `` `` / `''` (the
pre-jump position, fed by `G`/`gg`/`{`/`}`, search, marks, `%`, `gf`);
`Ctrl+O` / `Ctrl+I` exist but mean something narrower — see Different.

**Search & substitute.** Incremental `/` `?`, `n` / `N`, `*`, hlsearch with
`:noh`, and `:s/pat/rep/[g]`. (Regex dialect and `:s` scoping differ — see
*Different*.)

**Visual mode.** `v` `V` `Ctrl+V`; operators and `~`; `o` / `O` to swap
ends / block corners; `p` to replace the selection; `gv` to reselect.

**Insert mode.** `Esc` / `Ctrl+C` / `Ctrl+[`; `Enter` copies the indent;
`Tab`; `Backspace` / `Del`; `Ctrl+W` (delete word); `Ctrl+U` (delete to
indent); `Ctrl+T` / `Ctrl+D` (indent / dedent); arrows.

**Replace mode.** `R` overtypes — typed characters replace the ones under
the cursor, so the line keeps its length (tab-aligned trailing comments
stay put), and `Backspace` restores what was covered.

**Command line & files.** `:w` `:w!` `:q` `:q!` `:wq` `:wq!` `:x` `:x!`
`:qa` `:qa!` `:bd` `:bd!` `:noh` `:{number}`, `:w {path}` to name a scratch
buffer, and `:e` / `:e!` to reload the current file from disk. A file changed
on disk makes `:w` refuse (vim's "file has been changed" warning, as a hard
stop rather than a y/n prompt) until `:w!` or `:e`; a *clean* buffer reloads
on its own, like `autoread`.

**Windows, buffers, scrolling.** `Ctrl+W` splits/resizing/zoom; `Ctrl+^`
alternate buffer; `Ctrl+D` `Ctrl+U` `Ctrl+F` `Ctrl+B`, `zz` `zt` `zb`;
`ZZ` / `ZQ`. **Soft wrap** at word boundaries — vim's `wrap` + `linebreak`,
always on; `zz`/`zt`/`zb` and `scrolloff` count display rows.

## Different — same idea, changed on purpose

| Area | unei | vim |
|---|---|---|
| **Navigation** | `IJKL`; `h` = insert, `H` = insert at first non-blank | `HJKL`; `i` = insert |
| **Inner text objects** | prefix **`n`** (`dnw`, `cn(`) | prefix `i` (`diw`) — impossible here, `i` is a motion |
| **Registers** | two: a **yank** register (`p`/`P`) and a **cut** register (`Space p`/`Space P`); linewise deletes land in both, charwise deletes only in cut. No named or numbered registers, no history | many named/numbered registers with `"x` |
| **System clipboard** | yanks mirror out via OSC 52; deletes never do | via `"+`/`"*` or `clipboard` option |
| **Search dialect** | the Rust `regex` crate; smartcase always on | vim's own regex; `ignorecase`/`smartcase` options |
| **Substitute scope** | `:s` acts on the whole file, or the visual selection — no line ranges, no `:%s` | `:s` needs a range; `:%s` for the file |
| **`Y`** | `y$` (nvim default) | `yy` |
| **Visual paste** | never clobbers the register | replaces the register |
| **Opening files** | the fuzzy picker (`Ctrl+P`) or `gf` — there is no `:e path` | `:e`, `:find`, netrw, … |
| **Formatting** | external only, on save (treefmt); no `=` or `gq` | built-in `=`, `gq`, `equalprg`, … |
| **Long lines** | always soft-wrapped at word boundaries; no horizontal scrolling, no `nowrap` | `wrap`/`nowrap`, `linebreak`, sideways scrolling |
| **`Ctrl+O` / `Ctrl+I`** | previous / next *buffer*, at the position it was left; jumps inside a buffer are skipped (`''` and marks cover those) | the full jumplist, position by position, several presses to leave a buffer |
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

Two small motions are skipped as low-value for this workflow rather than on
principle: **`ge`/`gE`** (back to the previous word end) and **count-prefixed
insert** (`3ihello` → `hellohellohello`, `3o…`). Trivial to add if daily use
ever misses them.

## Not yet — on the roadmap, not excluded

These simply aren't built yet (see the issue tracker): a Colemak keymap
variant, auto-triggered completion, `Ctrl+E`/`Ctrl+Y` (scroll a line without
moving the cursor), and display-row motions over wrapped lines (vim's
`gj`/`gk` — `i`/`k` stay buffer-line motions).
