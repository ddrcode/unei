//! Line-comment toggling (ticket #53): `gcc` for the current line(s),
//! `gc` for a visual selection. Each toggles — comment if any target line
//! is bare, uncomment if all are commented.
//!
//! The comment token is a property of the file's language. For assembly it
//! comes from the modeline (`; asm: …` vs `# asm: …`), since the leader
//! varies by assembler and only the file knows which — consistent with the
//! honesty rule: no dialect declared, no commenting (see [`crate::lens`]).

use crate::config::languages;
use crate::core::buffer::Buffer;

/// The line-comment token for a buffer, or None when the file type has no
/// line comment (or an asm file hasn't declared its dialect).
pub fn line_comment(buffer: &Buffer) -> Option<String> {
    let path = buffer.path.as_deref()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("s") | Some("asm")) {
        return crate::lens::asm_line_comment(buffer.rope.lines().map(|l| l.to_string()).take(5));
    }
    let token = match languages::detect(Some(path))? {
        "rust" | "javascript" => "//",
        "yaml" | "toml" | "bash" | "nix" | "python" => "#",
        _ => return None,
    };
    Some(token.to_string())
}

/// Whether a line is already commented with `token` (ignoring indentation).
pub fn is_commented(line: &str, token: &str) -> bool {
    line.trim_start().starts_with(token)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn buf(name: &str, text: &str) -> Buffer {
        let mut b = Buffer::from_text(text);
        b.path = Some(PathBuf::from(name));
        b
    }

    #[test]
    fn token_by_language() {
        assert_eq!(line_comment(&buf("a.rs", "")).as_deref(), Some("//"));
        assert_eq!(line_comment(&buf("a.yaml", "")).as_deref(), Some("#"));
        assert_eq!(line_comment(&buf("a.toml", "")).as_deref(), Some("#"));
        assert_eq!(line_comment(&buf("a.py", "")).as_deref(), Some("#"));
        assert_eq!(line_comment(&buf("a.js", "")).as_deref(), Some("//"));
        // no line-comment syntax
        assert_eq!(line_comment(&buf("a.json", "")), None);
        assert_eq!(line_comment(&buf("a.css", "")), None);
    }

    #[test]
    fn asm_token_from_modeline() {
        assert_eq!(
            line_comment(&buf("g.s", "; asm: 65c02 acme\nlda #1\n")).as_deref(),
            Some(";")
        );
        assert_eq!(
            line_comment(&buf("g.s", "# asm: rv32e gas\nli a0, 1\n")).as_deref(),
            Some("#")
        );
        // no modeline → no commenting (honesty rule)
        assert_eq!(line_comment(&buf("g.s", "lda #1\n")), None);
    }
}
