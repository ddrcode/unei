//! Binary detection and hex formatting, shared by the file picker's preview
//! (a hex glance at the head, #67) and opening a binary as a read-only hex
//! view (#69). One implementation, two consumers.

use std::path::Path;

/// Bytes sampled from a file's head to decide whether it is binary.
const SNIFF_BYTES: u64 = 8 * 1024;

/// Whether a byte sample is binary rather than text: a NUL byte, bytes that
/// don't decode as UTF-8 (a multibyte char clipped by the sample boundary
/// doesn't count), or valid text littered with control chars. A plain NUL
/// test misses short 6502 images whose sampled head carries no NUL.
pub fn is_binary(head: &[u8]) -> bool {
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(text) => {
            let ctrl = text
                .chars()
                .filter(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{c}'))
                .count();
            ctrl * 20 > text.chars().count().max(1) // >5% control ⇒ binary
        }
        Err(e) => {
            // the trailing invalid bytes are a UTF-8 char clipped by the
            // sample boundary — not real binary — only if they actually form
            // the start of one: a lead byte (0xC2..=0xF4) then continuations
            let rest = &head[e.valid_up_to()..];
            let clipped = (1..=3).contains(&rest.len())
                && (0xC2..=0xF4).contains(&rest[0])
                && rest[1..].iter().all(|&b| (0x80..=0xBF).contains(&b));
            !clipped
        }
    }
}

/// Peeks a file's head and reports whether it looks binary, for choosing a
/// hex view over a text buffer. An unreadable file is treated as text so the
/// normal open path surfaces the real error.
pub fn file_looks_binary(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(SNIFF_BYTES).read_to_end(&mut head).is_err() {
        return false;
    }
    is_binary(&head)
}

/// A classic hex dump of the first `max_lines` rows of 16 bytes: offset, the
/// bytes in two octets, then an ASCII gutter (non-printable → `.`).
pub fn hex_dump(data: &[u8], max_lines: usize) -> Vec<String> {
    data.chunks(16)
        .take(max_lines)
        .enumerate()
        .map(|(row, chunk)| {
            let mut hex = String::new();
            for (i, b) in chunk.iter().enumerate() {
                if i == 8 {
                    hex.push(' '); // gap between the two octets
                }
                hex.push_str(&format!("{b:02x} "));
            }
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            // pad the hex column (16×"xx " + 1 gap = 49) so gutters align
            format!("{:08x}  {hex:<49}|{ascii}|", row * 16)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_binary_without_a_nul_byte() {
        // a small 6502 image: high opcode bytes, no NUL in the sampled head —
        // a bare `contains(&0)` check lets this through as garbage text
        let prg = [0x01u8, 0x08, 0xA9, 0x02, 0x8D, 0x20, 0xD0, 0xA2, 0xFF, 0xCA];
        assert!(is_binary(&prg));
        assert!(!is_binary(b"fn main() {}\n// plain rust\n"));
        // a multibyte char clipped by the sample boundary is still text
        let mut cut = "café au lait ".repeat(4).into_bytes();
        cut.push(0xC3); // dangling lead byte of a 2-byte sequence
        assert!(!is_binary(&cut));
    }

    #[test]
    fn hex_dump_has_offset_bytes_and_ascii_gutter() {
        let rows = hex_dump(&[0x00, 0xC0, 0x41, 0x42], 8);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].starts_with("00000000  00 c0 41 42 "), "{}", rows[0]);
        assert!(rows[0].ends_with("|..AB|"), "{}", rows[0]);
    }

    #[test]
    fn hex_dump_respects_the_line_cap() {
        let data = vec![0u8; 16 * 100];
        assert_eq!(hex_dump(&data, 4).len(), 4);
    }

    #[test]
    fn file_looks_binary_sniffs_the_head() {
        let dir = std::env::temp_dir().join(format!("unei-sniff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("a.bin");
        std::fs::write(&bin, [0x01u8, 0x08, 0xA9, 0x02, 0x8D]).unwrap();
        let txt = dir.join("a.txt");
        std::fs::write(&txt, "just some text\n").unwrap();
        assert!(file_looks_binary(&bin));
        assert!(!file_looks_binary(&txt));
        assert!(!file_looks_binary(&dir.join("missing"))); // unreadable ⇒ not binary
        std::fs::remove_dir_all(&dir).ok();
    }
}
