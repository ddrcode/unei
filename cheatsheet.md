# unei cheatsheet

Personal quick-ref. Terse on purpose. Reminder: **navigation is IJKL**, and
`h` inserts.

## Move

| key | |
|---|---|
| `i` `k` `j` `l` | up / down / left / right (arrows work too) |
| `w` `W` `b` `B` `e` `E` | word / WORD fwd-start, back, fwd-end |
| `0` `^` `$` | line start / first non-blank / end |
| `gg` `G` · `NG` | top / bottom · line N |
| `{` `}` | paragraph back / fwd |
| `f` `F` `t` `T` {c} · `;` `,` | to/till char fwd/back · repeat / reverse |
| `%` | matching `()[]{}` |
| `Ctrl+d` `Ctrl+u` · `Ctrl+f` `Ctrl+b` | half / full page |
| `zz` `zt` `zb` | cursor to center / top / bottom |
| `Ctrl+o` `Ctrl+i` | jumplist back / forward |

## Enter insert

`h` before · `H` first non-blank · `a` after · `A` eol · `o` below · `O` above
· `R` overtype (replace mode)

## Edit

| key | |
|---|---|
| `x` `X` | delete char right / left |
| `r`{c} · `s` · `S` | replace char · subst char · subst line |
| `d` `c` `y` + motion/obj | delete / change / yank (`dd` `cc` `yy` lines) |
| `D` `C` `Y` | to end of line |
| `~` · `J` · `gJ` | toggle case · join · join no-space |
| `>>` `<<` · `>`/`<` +motion | indent / dedent (`3>>`, `>%`, visual `>`) |
| `Ctrl+A` `Ctrl+X` | number +/- (dec, `$`/`0x` hex, `%`/`0b` bin; count multiplies) |
| `p` `P` | paste **yank** reg after / before |
| `Space p` `Space P` | paste **cut** reg (the `dd`+`p` move) |
| `u` `Ctrl+R` · `.` | undo / redo · repeat last change |
| `gcc` · `N gcc` · `gc`(visual) | toggle comment: line / N lines / selection |

Insert-mode: `Ctrl+W` del word · `Ctrl+U` del to indent · `Ctrl+T`/`Ctrl+D`
indent/dedent · `Ctrl+N`/`Ctrl+P` completion (Rust) · `Esc`/`Ctrl+C`.

## Text objects — after `d`/`c`/`y` or in visual · `a` around · **`n` inner**

| obj | |
|---|---|
| `w` `W` · `p` | word / WORD · paragraph |
| `(` `)` `b` · `{` `}` `B` · `[` `]` · `<` `>` | bracket pairs |
| `"` `'` `` ` `` | quotes |
| **`f`** | **function** (tree-sitter) — `daf` kill, `caf` rewrite, `vaf` select, `nf` body |
| **`c`** | **class/type** — impl / struct / enum / trait / mod — `dac`, `vnc` |

Cursor can be *anywhere* inside for `f`/`c`. Examples: `caf` `dnf` `yac` `vaf`.

## Marks & jumps

`m`{a-z} set · `` `x `` exact / `'x` line · `` `` `` / `''` back-toggle ·
`gf` open file under cursor · `gd` goto def (LSP)

## Search

`/` `?` incremental · `n` `N` next/prev · `*` word under cursor · `:noh` ·
`:s/pat/rep/[g]` (whole file, or visual selection)

## `K` — tell me about this

`K` = LSP hover (Rust) **or** machine lens (number bases; 6502/65C02 opcode) ·
visual `K` = **cycle sum** over the selection · `gK` = line type annotator ·
number lens works in **any** file. Asm needs a modeline in the first 5 lines:
`; asm: 65c02 acme` (leader-agnostic; drives grammar + lens + comment token).

## Pickers

`Ctrl+P` files · `Space b` buffers (`Ctrl+D` closes one) · `Space s` symbols · **`Space g` grep** ·
`gp` preview toggle: markdown rendered · **Rust = compiler's-eye view** (types, lifetimes, param names, full diagnostics) · **`Ctrl+w Alt+v`** = split + project + stay (side-by-side in one go) · `Ctrl+w V`/`X` = split and stay
Inside: type to filter (grep = regex) · `Ctrl+I`/`Ctrl+K` or arrows ·
`Enter` open · `Ctrl+V`/`Ctrl+X` v/h split · `Ctrl+Enter` create path ·
`Ctrl+U` clear · `Esc` close

## Windows — `Ctrl+w` (or `Space w`) then

`s`/`x` hsplit · `v` vsplit · `n` new-buffer split · `i`/`k`/`j`/`l` focus ·
`Ctrl+w` cycle · `q` close · `o` only · `=` equalize · `r` rotate ·
`Space` flip layout · `z` zoom

## Command line & files

`:w` `:q` `:q!` `:wq` `:x` · `:wa` write all · `:qa` `:qa!` · `:w {path}` save-as · `:bd` ·
`:e` reload from disk · `:e!` reload, drop my edits · `:w!` overwrite a file
changed on disk (`[!]` in statusline = changed underneath you; clean buffers
reload themselves) ·
`:{N}` goto line · `ZZ` save+quit · `ZQ` quit! · `Ctrl+^` alternate buffer

## Leader chords (`Space` …)

`Space c a` code actions · **`Space c r` references** (list; type to filter, Enter jumps) · **`Space d d` diagnostics list** (all files, errors first) · **`Space c n` rename** (prompt pre-filled; `Ctrl+U` clears; `:wa` after a multi-file one) · `Space r m` expand macro · `Space d h` toggle
diagnostic ghost text · `Space b`/`s`/`g` pickers · `Space p`/`P` cut-reg paste

## Opening the unusual

- **`.prg`** → 65C02 **disassembly** (re-assemblable ACME; `K` annotates it)
- **binary** (`.bin`/`.rom`/…) → read-only **hex view** `[RO]`
- picker preview of a binary → hex head; of text → syntax-highlighted head
