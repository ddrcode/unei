# unei - rules

This document described the project idea and rules that meant to be followed during the development. The rules shall not be modified by agents without dedicated, rules-only PR.

## The purpose of the project

It's personal editor without ambition of being widely used. I may make it public, but it's going to remain highly opinionated and personal.
The main reason for creating it is my tiredness of dealing with expiring/changing plugins and constant need for maitaining the configuration.
After years of using neovim I have my own way of using the editor and I am happy to completely hardcode it in zero-config solution. Hence unei.
If there is some inspiration here - think about lightweight, fast, minimalistic editors like Zed or Helix, but purely modern-terminal centric (Kitty) with (Neo)Vim philosophy.

## Programming language

Rust. With Ratatui as renderer, unless there is a better alternative.

## Zero-config, zero pluigin approach

Unlike vim-family, this project is a monolith. It assumes (and never will) no plugins and even no external configuration. Everything is embedded in the source code and any changes require recompilation.
Some minimalistic configurability should exist (i.e. key mapping), but it should be embedded in Rust files. `config` module should be used for that. In many cases configuration can be skipped entirely.

## nvim features that are NOT expected to be present in unei

- terminal buffer and any form of terminal integration (i.e. execution of system commands)
- scripting language
- help files
- vim-style file browser
- recording macros
- local leader (leader is present)
- plugins
- tabs (tbd)
- multiple ways of search/replace; the editor allows only standard regexp-based search and replace
- relative line numbers
- mouse support

## Single way of doing things

unei approach should always be - don't duplicate ways of doing things. i.e. nvim has multiple ways of text coloring - built-in  (regexp-based?), TreeSitter, language server - based.
unei should rely on a single method with no fallback. That applies to other features like formatting, etc.

## Formatting

Formatting should be provided by external tools, only on save. No built-in formatters. The editor itself should just follow current indentation.

## Terminal features

Assume the editor runs under Kitty terminal. It can (and is expected to) use Kitty terminal extensions to make the UI prettier (i.e. curly highlight of warnings). Support of unicode and nerd fonts should be assumed upfront.

## Terminal integraion

None. I believe editor runs in terminal not terminal in the editor.

## Keymapping

I use non-standard navigation keys - instead of vim-style HJKL, I prefer IJKL (I-up, k-down, j-left, l-right) and H as a key for switching to insert mode.
That should apply everywhere where navigatrion keys are involved, i.e. `Ctrl+w+i` to navigate to panel above current one. Or `Shift+H` to insert at the beginning of the line.

