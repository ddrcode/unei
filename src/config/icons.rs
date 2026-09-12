//! Nerd-font file-type icons for the picker (#73). A glyph + colour chosen by
//! well-known filename or extension; the set is distilled from
//! nvim-web-devicons (the de-facto mapping). Kitty with a nerd font is
//! assumed (see rules.md), so the private-use glyphs render.

use ratatui::style::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// Fallback for anything unrecognised: a plain document glyph.
const DEFAULT: (char, Color) = ('\u{f15b}', rgb(150, 150, 150));

/// The icon and colour for a path, by exact filename first, then extension.
pub fn icon_for(path: &str) -> (char, Color) {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if let Some(hit) = by_name(name) {
        return hit;
    }
    // extension = text after the last dot, but not the whole (dotfile) name
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => by_ext(&ext.to_ascii_lowercase()),
        _ => DEFAULT,
    }
}

fn by_name(name: &str) -> Option<(char, Color)> {
    Some(match name {
        "Cargo.toml" | "Cargo.lock" => ('\u{e7a8}', rgb(222, 165, 132)), // rust
        "flake.lock" => ('\u{f313}', rgb(126, 186, 228)),                // nix
        "Makefile" | "makefile" => ('\u{e779}', rgb(110, 130, 135)),
        "justfile" | "Justfile" | "JUSTFILE" | ".justfile" => ('\u{e779}', rgb(110, 130, 135)),
        "Dockerfile" => ('\u{f308}', rgb(76, 110, 180)),
        ".gitignore" | ".gitattributes" | ".gitmodules" => ('\u{e702}', rgb(241, 76, 40)),
        _ => return None,
    })
}

fn by_ext(ext: &str) -> (char, Color) {
    match ext {
        "rs" => ('\u{e7a8}', rgb(222, 165, 132)),
        "s" | "asm" => ('\u{f2db}', rgb(108, 128, 134)), // a chip, fittingly
        "toml" => ('\u{e615}', rgb(157, 66, 33)),
        "yaml" | "yml" => ('\u{e615}', rgb(109, 128, 134)),
        "json" => ('\u{e60b}', rgb(203, 203, 65)),
        "md" | "markdown" => ('\u{e73e}', rgb(81, 154, 186)),
        "nix" => ('\u{f313}', rgb(126, 186, 228)),
        "lock" => ('\u{f023}', rgb(187, 187, 187)),
        "sh" | "bash" | "zsh" | "fish" => ('\u{f489}', rgb(77, 90, 94)),
        "js" | "mjs" | "cjs" => ('\u{e74e}', rgb(203, 203, 65)),
        "ts" => ('\u{e628}', rgb(81, 154, 186)),
        "html" | "htm" => ('\u{e736}', rgb(227, 76, 38)),
        "css" => ('\u{e749}', rgb(81, 154, 186)),
        "py" => ('\u{e606}', rgb(255, 188, 3)),
        "c" => ('\u{e61e}', rgb(89, 158, 255)),
        "h" | "hpp" | "hxx" => ('\u{e61e}', rgb(160, 116, 196)),
        "cpp" | "cc" | "cxx" => ('\u{e61d}', rgb(89, 158, 255)),
        "lua" => ('\u{e620}', rgb(81, 160, 207)),
        "go" => ('\u{e627}', rgb(81, 154, 186)),
        "vim" => ('\u{e62b}', rgb(66, 150, 80)),
        "txt" | "text" => ('\u{f15c}', rgb(187, 187, 187)),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => ('\u{f1c5}', rgb(160, 116, 196)),
        "prg" | "bin" | "rom" | "o" => ('\u{f471}', rgb(160, 116, 196)), // binary
        _ => DEFAULT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_by_extension_and_name() {
        assert_eq!(icon_for("src/main.rs").0, '\u{e7a8}');
        assert_eq!(icon_for("boot.s").0, '\u{f2db}');
        assert_eq!(icon_for("Cargo.toml").0, '\u{e7a8}'); // name beats .toml
        assert_eq!(icon_for("notes.toml").0, '\u{e615}');
        assert_eq!(icon_for(".gitignore").0, '\u{e702}'); // dotfile by name
        assert_eq!(icon_for("weird.unknownext"), DEFAULT);
        assert_eq!(icon_for("no_extension_at_all"), DEFAULT);
    }
}
