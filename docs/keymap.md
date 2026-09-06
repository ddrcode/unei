# Keymap

The layout everything else is built around: **IJKL navigation** (one row up
from vim's HJKL, arrow-shaped), with `h` — freed from moving left — entering
insert mode. It mirrors the author's long-standing Neovim remap and applies
everywhere navigation appears: motions, window focus, list selection.

```
        i               ↑
      j k l    =    ←   ↓   →        h → insert mode (vim's i)
```

Uppercase `I J K L` keep their vim meanings where they had one (`J` joins,
`K` hovers); vim's `H`/`L` screen jumps don't exist. In operator-pending and
visual contexts `h` is a **left motion** again (`dh` deletes left), because
the insert remap is a normal-mode affair. There are no `i`-prefixed text
objects for the same reason — the inner-object prefix is **`n`** (#12).

The tables live in `src/config/keymap.rs` as data. Leader is **Space**.

## Normal mode

### Motions (all take counts; all work as operator targets)

| Keys | Motion |
|---|---|
| `i` `k` `j` `l`, arrows | up, down, left, right |
| `0` `^` `$`, Home/End | line start, first non-blank, line end |
| `w` `W` `b` `B` `e` `E` | word / WORD forward, back, end |
| `gg` `G` | first line, last line (count: `:N`-style goto) |
| `{` `}` | paragraph back / forward |
| `f`/`F`/`t`/`T` + char, `;` `,` | find on line, repeat, repeat reversed |
| `%` | jump to the matching bracket (`()[]{}`); also an operator target (`d%`) |

### Operators & edits

| Keys | Action |
|---|---|
| `d` `c` `y` + motion | delete / change / yank (`dd` `cc` `yy` linewise; `2d3w` = six words) |
| `D` `C` `Y` | to end of line (`Y` = `y$`, nvim-style) |
| `x` `X` | delete char right / left |
| `r`+char | replace char(s) |
| `s` `S` | substitute char / line |
| `~` | toggle case |
| `J` / `gJ` | join lines (with / without a space at the seam) |
| `Ctrl+A` / `Ctrl+X` | increment / decrement the number under (or after) the cursor — decimal, `0x`/`$` hex, `0b`/`%` binary; keeps width and case, counts multiply (`5 Ctrl+A`) |
| `>>` `<<`, `>`/`<` + motion | shift lines right / left by a shiftwidth (`3>>`, `>%`, `>G`); one undo step, dot-repeatable |
| `p` `P` | paste the **yank** register after / before (char, line, or block) |
| `Space p` / `Space P` | paste the **cut** register after / before (the `dd`+`p` line-move lives here) |
| `gcc` | toggle line comment on the current line (`3gcc` for three lines) |
| `u` / `Ctrl+R` | undo / redo (insert session = one unit) |
| `.` | repeat last change (`3.` replaces the count) |

`gcc`/`gc` toggle: they comment when any target line is bare, uncomment when
all are commented. The token follows the language (`//` for Rust/JS, `#`
for YAML/TOML/Python/Nix/Bash); in assembly it comes from the file's
`asm:` modeline leader (`; asm: …` → `;`, `# asm: …` → `#`), so an
undeclared dialect is left alone.

### Text objects

After an operator (`d` `c` `y`) or in visual mode, `a` (around) or `n`
(inner — vim's `i`, which the layout can't spare) selects an object.

| Object | With `a` / `n` |
|---|---|
| `w` `W` | word / WORD (`daw`, `dnw`) |
| `p` | paragraph (linewise) |
| `(` `)` `b`, `{` `}` `B`, `[` `]`, `<` `>` | bracket pair — inner or including the brackets |
| `"` `'` `` ` `` | quoted string on the line |

`a` is around (includes the brackets/quotes, or a word's trailing space);
`n` is inner. Counts extend words and paragraphs (`d2nw`). A bare `a`/`n`
outside this context keeps its normal meaning (append / next match).

### Mode changes

| Keys | Action |
|---|---|
| `h` `H` | insert before cursor / at first non-blank |
| `a` `A` | append after cursor / at line end |
| `o` `O` | open line below / above |
| `R` | Replace (overtype) mode — typed chars replace those under the cursor; `Backspace` restores what it covered |
| `v` `V` `Ctrl+V` | visual char / line / block |
| `:` | command line |

### Preview (markdown)

| Keys | Action |
|---|---|
| `gp` | toggle the window between source and rendered preview |
| in preview: `i`/`k`, `Ctrl+D/U/F/B`, `g`/`G` | navigate the projection |
| in preview: `Enter` | jump to the source at the mapped line |
| in preview: `h` | jump to the source **and start editing** (insert mode) |
| in preview: `Esc` | back to source view |
| in preview: `:q` | close the panel (the command line works from a preview) |

A preview window **follows** the window editing the same buffer — scroll or
type in the source and the projection tracks you live, its reading line
highlighted where your cursor is. `Ctrl+W v` then `gp` is the side-by-side
writing setup. GFM tables render as aligned grids (`:---:` alignment
honored).

### Search

| Keys | Action |
|---|---|
| `/` `?` | incremental search forward / backward (Rust regex, smartcase) |
| `n` / `N` | next / previous match (wraps, follows direction) |
| `*` | whole-word search for the word under the cursor |
| `Esc` | calm match highlighting (`:noh` too) |
| `:s/pat/rep/[g]` | substitute — file scope, or selection scope from visual mode |

### Files, buffers, jumps

| Keys | Action |
|---|---|
| `Ctrl+P` | fuzzy file picker |
| `Space b` | buffer list |
| `Space s` | symbol picker — fuzzy-jump to a function / struct / … in the current file |
| `Ctrl+^` / `Ctrl+6` | alternate buffer |
| `Ctrl+O` / `Ctrl+I` (`Tab`) | jumplist back / forward (crosses buffers) |
| `m{a-z}` | set a mark at the cursor (per buffer) |
| `` `{a-z} `` / `'{a-z}` | jump to a mark — exact position / first non-blank of its line |
| `` `` `` / `''` | jump back to where the last jump started (toggles) |
| `gf` | open the file named under the cursor (current dir, then root; tries `.rs`) |

### Machine lens

`K` is the single "tell me about this" key. Rust files ask rust-analyzer
(below); everywhere else the editor answers from compiled-in knowledge.
Any key dismisses the float.

| Context | `K` shows |
|---|---|
| a numeric literal, any file | all bases, signedness, lo/hi bytes, byte order |
| a 65C02/6502 instruction | one-line description + flags, opcode byte, addressing mode, length, cycles + penalties |
| visual selection of asm lines | cycle sum — penalties as ranges, unknowns counted, never guessed |

Assembly knowledge activates only when the file declares its dialect in a
**modeline** within the first five lines — `; asm: 65c02 acme` — with
whatever comment leader your assembler accepts (`;`, `#`, `//`, …).
`65c02` and `6502` (NMOS) carry separate timing tables; where history
disagrees the float shows both, and 65C02-only instructions are called
out in a `6502` file.

### rust-analyzer

| Keys | Action |
|---|---|
| `K` | hover (type + docs); any key dismisses |
| `gd` | goto definition |
| `gK` | line-scope hover: the line with all types spliced in, plus the call signature |
| `Space c a` | code actions menu |
| `Space r m` | expand macro (into a split) |
| `Space d h` | toggle end-of-line diagnostic text |

### Windows (`Ctrl+W` or `Space w`, then…)

| Key | Action |
|---|---|
| `i` `k` `j` `l`, arrows | focus window in that direction |
| `Ctrl+W` | cycle to next window |
| `Alt+i/k/j/l` | resize, tmux-style (push the border) |
| `s` / `x` | horizontal split |
| `v` | vertical split |
| `n` | horizontal split with a new empty buffer |
| `q` | close window (last one quits) |
| `o` | only — close all others |
| `=` | equalize sizes |
| `r` | rotate windows in their container |
| `Space` | flip container layout (side-by-side ↔ stacked) |
| `z` | zoom toggle (statusline shows `[Z]`) |

### Scrolling & misc

| Keys | Action |
|---|---|
| `Ctrl+D` / `Ctrl+U` | half page down / up |
| `Ctrl+F` / `Ctrl+B`, PgDn/PgUp | page down / up |
| `zz` `zt` `zb` | cursor to center / top / bottom |
| `ZZ` / `ZQ` | save-and-quit / force-quit |
| `gv` | reselect last visual selection |
| `Esc` | clear pending input |

## Visual mode

Motions extend the selection; `h` is a left motion here.

| Keys | Action |
|---|---|
| `d` / `x` | delete selection |
| `c` / `s` | change selection (block: type once, Esc replicates to every line) |
| `y` | yank (with flash) |
| `gc` | toggle line comments on the selected lines |
| `>` / `<` | shift the selected lines right / left |
| `~` | toggle case |
| `p` | replace selection with register — **never clobbers the register** |
| `o` / `O` | swap ends / swap block corners |
| `v` `V` `Ctrl+V` | switch kind (same kind exits) |
| `Esc`, `Ctrl+C` | back to normal |

## Insert mode

| Keys | Action |
|---|---|
| `Esc`, `Ctrl+C`, `Ctrl+[` | back to normal |
| `Ctrl+N` / `Ctrl+P` | completion: open at the cursor, then cycle next / previous (rust-analyzer, Rust only) |
| with the popup open: `Enter` / `Tab` | accept the selected candidate |
| with the popup open: `Ctrl+E` | dismiss (keep typing); `Esc` dismisses and leaves insert |
| `Enter` | new line, copying the current indent |
| `Tab` | spaces to the next 4-column stop |
| `Backspace` / `Del` | delete back / forward (joins lines at edges) |
| `Ctrl+W` | delete word back |
| `Ctrl+T` / `Ctrl+D` | indent / dedent the current line |
| `Ctrl+U` | delete to indent |
| arrows, Home/End | movement without leaving insert |

## Registers & system clipboard

Yanks (`y`) and cuts (`d`/`c`/`x`/`s`) live in **separate registers** — a
delete never overwrites what you copied (#10). `p` pastes the yank,
`Space p` pastes the cut. Every yank also mirrors to the **system
clipboard** (OSC 52); deletes never do. Terminal paste (Cmd+V, bracketed)
inserts literally in insert mode, puts charwise in normal mode, and
replaces the selection in visual mode — never interpreted as keystrokes.

## Command line

`:w` `:q` `:q!` `:wq` `:x` — with splits open, quit commands close the window
first; the last window checks *all* buffers for unsaved changes. `:bd`/`:bd!`
close the buffer. `:{number}` jumps to a line. `:w {path}` writes the buffer
to a path and binds it there (how a bare-launch scratch buffer gets a home).

## Overlays

- **File picker** — type to filter; `Ctrl+I`/`Ctrl+K` or arrows select; `Enter` opens, `Ctrl+V`/`Ctrl+X` open in vertical/horizontal split; `Ctrl+Enter` creates the typed path (parents included); `Ctrl+U` clears; `Esc` closes. On a wide terminal a **preview pane** shows the selected file's head, syntax-highlighted (scroll-free — open it for more; binary/empty files are noted).
- **Buffer list** — `i`/`k` select, `Enter` switches, `x` closes a buffer, `Esc`/`q` dismiss.
- **Symbol picker** (`Space s`) — the current file's definitions from tree-sitter (functions, structs, enums, traits, impls, …); type to fuzzy-filter (typing a kind like `fn` narrows to those), `Enter` jumps and records the jumplist. Rust for now; conservative by design.
- **Code actions** — `i`/`k` select, `Enter` applies, `Esc`/`q` dismiss.
- **Hover float** — any key dismisses (`Esc`/`q`/`K` do nothing else).
