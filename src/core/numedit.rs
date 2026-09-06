//! In-place increment/decrement of the number under the cursor (`Ctrl+A` /
//! `Ctrl+X`, #14). Pure text logic: find the number at or after a column,
//! add a delta, and re-render it preserving base, prefix, digit width and
//! letter case. The read-side complement is the machine lens (`lens.rs`).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Radix {
    Dec,
    Hex,
    Bin,
}

impl Radix {
    fn value(self) -> u128 {
        match self {
            Radix::Dec => 10,
            Radix::Hex => 16,
            Radix::Bin => 2,
        }
    }
    fn is_digit(self, c: char) -> bool {
        match self {
            Radix::Dec => c.is_ascii_digit(),
            Radix::Hex => c.is_ascii_hexdigit(),
            Radix::Bin => c == '0' || c == '1',
        }
    }
}

/// Finds the number at or after char column `col` on `line`, applies `delta`,
/// and returns `(start, end, replacement)` in char units — or `None` when the
/// line has no number at/after the cursor. Recognised: decimal (signed), hex
/// (`0x`/`0X`/`$`) and binary (`0b`/`0B`/`%`); the `$`/`%` asm spellings ride
/// along because the lens already reads them.
pub fn number_edit(line: &str, col: usize, delta: i64) -> Option<(usize, usize, String)> {
    let ch: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut i = 0;
    while i < ch.len() {
        // a number can begin here only at a token boundary (so the middle of
        // `0xff` or an identifier isn't mistaken for the start of one)
        let boundary = i == 0 || !(is_word(ch[i - 1]) || ch[i - 1] == '$' || ch[i - 1] == '%');
        if boundary && let Some((end, radix, prefix_len, signed)) = match_number(&ch, i) {
            if end > col {
                let token: String = ch[i..end].iter().collect();
                return apply(&token, radix, prefix_len, signed, delta).map(|s| (i, end, s));
            }
            i = end;
            continue;
        }
        i += 1;
    }
    None
}

/// Tries to read a number token starting exactly at `i`, returning its end,
/// base, prefix length (chars before the digits), and whether it carries a
/// decimal sign.
fn match_number(ch: &[char], i: usize) -> Option<(usize, Radix, usize, bool)> {
    let run = |from: usize, radix: Radix| {
        ch[from..]
            .iter()
            .take_while(|&&c| radix.is_digit(c))
            .count()
    };

    // prefixed, unsigned forms first
    if ch.get(i) == Some(&'0') && matches!(ch.get(i + 1), Some('x' | 'X')) {
        let d = run(i + 2, Radix::Hex);
        if d > 0 {
            return Some((i + 2 + d, Radix::Hex, 2, false));
        }
    }
    if ch.get(i) == Some(&'0') && matches!(ch.get(i + 1), Some('b' | 'B')) {
        let d = run(i + 2, Radix::Bin);
        if d > 0 {
            return Some((i + 2 + d, Radix::Bin, 2, false));
        }
    }
    if ch.get(i) == Some(&'$') {
        let d = run(i + 1, Radix::Hex);
        if d > 0 {
            return Some((i + 1 + d, Radix::Hex, 1, false));
        }
    }
    if ch.get(i) == Some(&'%') {
        let d = run(i + 1, Radix::Bin);
        if d > 0 {
            return Some((i + 1 + d, Radix::Bin, 1, false));
        }
    }
    // signed or unsigned decimal
    let (signed, ds) = if ch.get(i) == Some(&'-') {
        (true, i + 1)
    } else {
        (false, i)
    };
    let d = run(ds, Radix::Dec);
    if d > 0 {
        Some((ds + d, Radix::Dec, 0, signed))
    } else {
        None
    }
}

fn apply(token: &str, radix: Radix, prefix_len: usize, signed: bool, delta: i64) -> Option<String> {
    let tc: Vec<char> = token.chars().collect();
    let mut idx = 0;
    let neg = signed && tc[0] == '-';
    if signed {
        idx = 1;
    }
    let prefix: String = tc[idx..idx + prefix_len].iter().collect();
    let digits: String = tc[idx + prefix_len..].iter().collect();
    let width = digits.chars().count();
    let upper = digits.chars().any(|c| c.is_ascii_uppercase());

    let magnitude = i128::from_str_radix(&digits, radix.value() as u32).ok()?;
    let mut val = if neg { -magnitude } else { magnitude } + delta as i128;

    let (out_neg, mag) = match radix {
        Radix::Dec => (val < 0, val.unsigned_abs()),
        // hex/binary are unsigned — a literal can't go below zero
        _ => {
            if val < 0 {
                val = 0;
            }
            (false, val as u128)
        }
    };
    let body = render(mag, radix.value(), width, upper);
    let sign = if out_neg { "-" } else { "" };
    Some(format!("{sign}{prefix}{body}"))
}

/// Renders `mag` in `base`, left-padded with zeros to at least `width` digits,
/// using upper- or lower-case hex letters.
fn render(mag: u128, base: u128, width: usize, upper: bool) -> String {
    let alphabet: &[u8] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Vec::new();
    let mut m = mag;
    while m > 0 {
        out.push(alphabet[(m % base) as usize] as char);
        m /= base;
    }
    while out.len() < width.max(1) {
        out.push('0');
    }
    out.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(line: &str, col: usize, delta: i64) -> Option<(usize, usize, String)> {
        number_edit(line, col, delta)
    }

    #[test]
    fn decimal_increment_and_width() {
        assert_eq!(edit("42", 0, 1), Some((0, 2, "43".into())));
        assert_eq!(edit("007", 1, 1), Some((0, 3, "008".into()))); // keeps width
        assert_eq!(edit("9", 0, 1), Some((0, 1, "10".into()))); // grows
        assert_eq!(edit("x = 5", 0, 3), Some((4, 5, "8".into()))); // number after cursor
    }

    #[test]
    fn decimal_signed() {
        assert_eq!(edit("-5", 0, 1), Some((0, 2, "-4".into())));
        assert_eq!(edit("-1", 0, 1), Some((0, 2, "0".into()))); // crosses zero
        assert_eq!(edit("0", 0, -1), Some((0, 1, "-1".into())));
    }

    #[test]
    fn hex_forms_preserve_prefix_case_width() {
        assert_eq!(edit("0xff", 0, 1), Some((0, 4, "0x100".into())));
        assert_eq!(edit("0x0F", 0, 1), Some((0, 4, "0x10".into()))); // upper kept
        assert_eq!(edit("$FE", 0, 1), Some((0, 3, "$FF".into())));
        assert_eq!(edit("$00", 0, -1), Some((0, 3, "$00".into()))); // clamps at 0
    }

    #[test]
    fn binary_forms() {
        assert_eq!(edit("0b0001", 0, 1), Some((0, 6, "0b0010".into())));
        assert_eq!(edit("%1010", 0, 1), Some((0, 5, "%1011".into())));
    }

    #[test]
    fn count_delta_and_no_number() {
        assert_eq!(edit("val 10", 0, 5), Some((4, 6, "15".into())));
        assert_eq!(edit("no digits here", 0, 1), None);
        // cursor past the only number ⇒ nothing to the right
        assert_eq!(edit("7 x", 2, 1), None);
    }

    #[test]
    fn cursor_inside_the_number_still_edits_it() {
        // col 2 is the middle of 1234 → still the target
        assert_eq!(edit("1234", 2, 1), Some((0, 4, "1235".into())));
    }
}
