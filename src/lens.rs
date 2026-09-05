//! The machine lens (ticket #45): editor-owned knowledge behind `K`.
//!
//! `K` is the one "tell me about the thing under the cursor" key. Where an
//! LSP exists it answers; everywhere else this module does, from knowledge
//! frozen enough to compile in (the half-life rule, docs/decisions.md):
//!
//! - the NUMBER LENS, in any file: bases, signedness, byte splits;
//! - the 65C02/6502 OPCODE LENS in assembly files: a one-line
//!   description with the flags it sets, the opcode byte, addressing
//!   mode, length, and cycles with penalty footnotes;
//! - the CYCLE SUM over a visual selection of straight-line code.
//!
//! Assembly knowledge activates only when the file declares its dialect in
//! a modeline (`; asm: 65c02 acme`, first five lines, comment leader
//! agnostic — the format agreed in #18). Colors may guess; numbers must
//! know: no modeline, no opcode lens. Timing is ISA knowledge only for the
//! 6502 family — on pipelined ISAs cycles belong to a core, not the
//! language, so there the lens will speak of encodings, not time.

// ----------------------------------------------------------------------
// modeline
// ----------------------------------------------------------------------

/// The 6502 lineage a file declared. `Other` keeps the raw ISA string for
/// future providers (riscv…) — recognized, but no timing knowledge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Family {
    Cmos65c02,
    Nmos6502,
    Other(String),
}

/// Parses `<leader> asm: <isa> [<assembler>]` within the given lines
/// (callers pass at most the first five). The comment leader is opaque:
/// a short run of punctuation, whatever the file's toolchain accepts.
pub fn modeline(lines: impl Iterator<Item = String>) -> Option<Family> {
    for line in lines {
        let s = line.trim();
        let mut chars = s.char_indices().peekable();
        let mut punct = 0usize;
        let mut rest = 0usize;
        while let Some((i, ch)) = chars.peek().copied() {
            if ch.is_ascii_punctuation() && punct < 3 {
                punct += 1;
                rest = i + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        if punct == 0 {
            continue;
        }
        let body = s[rest..].trim_start();
        let Some(spec) = body.strip_prefix("asm:") else {
            continue;
        };
        let spec = spec.trim_end_matches("*/").trim();
        let isa = spec.split_whitespace().next().unwrap_or("");
        if isa.is_empty() {
            continue;
        }
        return Some(match isa.to_ascii_lowercase().as_str() {
            "65c02" => Family::Cmos65c02,
            "6502" => Family::Nmos6502,
            other => Family::Other(other.to_string()),
        });
    }
    None
}

// ----------------------------------------------------------------------
// number lens
// ----------------------------------------------------------------------

/// Extracts the numeric literal containing the cursor column (chars), if
/// any: `$9F42`, `%1010`, `0xFF`, `0b101`, `1234`, each with an optional
/// `#` immediate prefix.
pub fn literal_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let col = col.min(chars.len() - 1);
    let is_lit = |c: char| c.is_ascii_alphanumeric() || matches!(c, '$' | '%' | '#' | '_');
    if !is_lit(chars[col]) {
        return None;
    }
    let mut start = col;
    while start > 0 && is_lit(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col + 1;
    while end < chars.len() && is_lit(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    let word = word.trim_start_matches('#');
    // `$`/`%` mean bases only as prefixes; at the end they're prose (100%)
    let word = word.trim_end_matches(['%', '$', '#']);
    let word = word.trim_matches('_');
    if word.is_empty() {
        None
    } else {
        Some(word.replace('_', ""))
    }
}

fn parse_number(word: &str) -> Option<(u64, u32)> {
    let (digits, radix) = if let Some(h) = word.strip_prefix('$') {
        (h, 16)
    } else if let Some(h) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        (h, 16)
    } else if let Some(b) = word.strip_prefix('%') {
        (b, 2)
    } else if let Some(b) = word.strip_prefix("0b").or_else(|| word.strip_prefix("0B")) {
        (b, 2)
    } else {
        (word, 10)
    };
    if digits.is_empty() {
        return None;
    }
    u64::from_str_radix(digits, radix).ok().map(|v| (v, radix))
}

/// Rounds a value (and how it was written) up to a natural register width.
fn bits_for(value: u64, written_digits: usize, radix: u32) -> u32 {
    let written = match radix {
        16 => written_digits as u32 * 4,
        2 => written_digits as u32,
        _ => 0,
    };
    let needed = (64 - value.leading_zeros()).max(1).max(written);
    match needed {
        0..=8 => 8,
        9..=16 => 16,
        17..=32 => 32,
        _ => 64,
    }
}

fn group(s: &str, every: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(every) {
            out.push('\'');
        }
        out.push(*c);
    }
    out
}

/// The number float: every base at once, signed views, byte anatomy.
pub fn number_hover(word: &str) -> Option<Vec<String>> {
    let (value, radix) = parse_number(word)?;
    let digits = word
        .trim_start_matches(['$', '%'])
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches("0b")
        .trim_start_matches("0B")
        .len();
    let bits = bits_for(value, digits, radix);
    let mut out = Vec::new();
    out.push(format!("{word} — {bits}-bit"));
    let hexw = (bits / 4) as usize;
    let signed = match bits {
        8 => (value as u8 as i8) as i64,
        16 => (value as u16 as i16) as i64,
        32 => (value as u32 as i32) as i64,
        _ => value as i64,
    };
    let mut line2 = format!("hex ${value:0hexw$X} · dec {value}", hexw = hexw);
    if signed < 0 {
        line2.push_str(&format!(" · i{bits} {signed}"));
    }
    out.push(line2);
    if bits <= 32 {
        let binw = bits as usize;
        out.push(format!(
            "bin %{}",
            group(&format!("{value:0binw$b}", binw = binw), 4)
        ));
    }
    if bits == 16 {
        let (lo, hi) = (value & 0xFF, value >> 8);
        out.push(format!(
            "lo ${lo:02X} · hi ${hi:02X} · swap ${:04X}",
            (lo << 8) | hi
        ));
    } else if bits == 32 {
        let b = (value as u32).to_le_bytes();
        out.push(format!(
            "bytes {:02X} {:02X} {:02X} {:02X} (le)",
            b[0], b[1], b[2], b[3]
        ));
    }
    Some(out)
}

// ----------------------------------------------------------------------
// 6502 / 65C02 opcode knowledge
// ----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    ZpX,
    ZpY,
    Abs,
    AbsX,
    AbsY,
    IndX,
    IndY,
    ZpInd,
    Ind,
    IndAbsX,
    Rel,
    ZpRel,
}

impl Mode {
    fn bytes(self) -> u8 {
        match self {
            Mode::Imp | Mode::Acc => 1,
            Mode::Imm
            | Mode::Zp
            | Mode::ZpX
            | Mode::ZpY
            | Mode::IndX
            | Mode::IndY
            | Mode::ZpInd
            | Mode::Rel => 2,
            Mode::Abs | Mode::AbsX | Mode::AbsY | Mode::Ind | Mode::IndAbsX | Mode::ZpRel => 3,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Mode::Imp => "implied",
            Mode::Acc => "accumulator",
            Mode::Imm => "#imm",
            Mode::Zp => "zp",
            Mode::ZpX => "zp,X",
            Mode::ZpY => "zp,Y",
            Mode::Abs => "abs",
            Mode::AbsX => "abs,X",
            Mode::AbsY => "abs,Y",
            Mode::IndX => "(zp,X)",
            Mode::IndY => "(zp),Y",
            Mode::ZpInd => "(zp)",
            Mode::Ind => "(abs)",
            Mode::IndAbsX => "(abs,X)",
            Mode::Rel => "relative",
            Mode::ZpRel => "zp,rel",
        }
    }
}

/// One (mnemonic, mode) row: 65C02 cycles, +1-if-page-crossed flag, NMOS
/// cycles where they differ, CMOS-only marker, branch-penalty marker.
#[derive(Clone, Copy)]
pub struct Op {
    pub mode: Mode,
    pub cyc: u8,
    pub page: bool,
    pub nmos: Option<u8>,
    pub cmos_only: bool,
    pub branch: bool,
}

const fn op(mode: Mode, cyc: u8) -> Op {
    Op {
        mode,
        cyc,
        page: false,
        nmos: None,
        cmos_only: false,
        branch: false,
    }
}
const fn pg(mut o: Op) -> Op {
    o.page = true;
    o
}
const fn nm(mut o: Op, c: u8) -> Op {
    o.nmos = Some(c);
    o
}
const fn cm(mut o: Op) -> Op {
    o.cmos_only = true;
    o
}
const fn br(mut o: Op) -> Op {
    o.branch = true;
    o
}

use Mode::*;

const LOAD_SHAPE: &[Op] = &[
    op(Imm, 2),
    op(Zp, 3),
    op(ZpX, 4),
    op(Abs, 4),
    pg(op(AbsX, 4)),
    pg(op(AbsY, 4)),
    op(IndX, 6),
    pg(op(IndY, 5)),
    cm(op(ZpInd, 5)),
];
const RMW_SHAPE: &[Op] = &[
    cm(op(Acc, 2)),
    op(Zp, 5),
    op(ZpX, 6),
    op(Abs, 6),
    op(AbsX, 7),
];
const SHIFT_SHAPE: &[Op] = &[
    op(Acc, 2),
    op(Zp, 5),
    op(ZpX, 6),
    op(Abs, 6),
    nm(pg(op(AbsX, 6)), 7),
];
const BRANCH_SHAPE: &[Op] = &[br(op(Rel, 2))];
const IMP2: &[Op] = &[op(Imp, 2)];

const LDX_OPS: &[Op] = &[
    op(Imm, 2),
    op(Zp, 3),
    op(ZpY, 4),
    op(Abs, 4),
    pg(op(AbsY, 4)),
];
const LDY_OPS: &[Op] = &[
    op(Imm, 2),
    op(Zp, 3),
    op(ZpX, 4),
    op(Abs, 4),
    pg(op(AbsX, 4)),
];
const STA_OPS: &[Op] = &[
    op(Zp, 3),
    op(ZpX, 4),
    op(Abs, 4),
    op(AbsX, 5),
    op(AbsY, 5),
    op(IndX, 6),
    op(IndY, 6),
    cm(op(ZpInd, 5)),
];
const STX_OPS: &[Op] = &[op(Zp, 3), op(ZpY, 4), op(Abs, 4)];
const STY_OPS: &[Op] = &[op(Zp, 3), op(ZpX, 4), op(Abs, 4)];
const STZ_OPS: &[Op] = &[
    cm(op(Zp, 3)),
    cm(op(ZpX, 4)),
    cm(op(Abs, 4)),
    cm(op(AbsX, 5)),
];
const CPXY_OPS: &[Op] = &[op(Imm, 2), op(Zp, 3), op(Abs, 4)];
const BIT_OPS: &[Op] = &[
    op(Zp, 3),
    op(Abs, 4),
    cm(op(Imm, 2)),
    cm(op(ZpX, 4)),
    cm(pg(op(AbsX, 4))),
];
const TRB_OPS: &[Op] = &[cm(op(Zp, 5)), cm(op(Abs, 6))];
const BRA_OPS: &[Op] = &[cm(pg(op(Rel, 3)))];
const JMP_OPS: &[Op] = &[op(Abs, 3), nm(op(Ind, 6), 5), cm(op(IndAbsX, 6))];
const JSR_OPS: &[Op] = &[op(Abs, 6)];
const RET_OPS: &[Op] = &[op(Imp, 6)];
const BRK_OPS: &[Op] = &[op(Imp, 7)];
const PUSH_OPS: &[Op] = &[op(Imp, 3)];
const PULL_OPS: &[Op] = &[op(Imp, 4)];
const PUSH_C_OPS: &[Op] = &[cm(op(Imp, 3))];
const PULL_C_OPS: &[Op] = &[cm(op(Imp, 4))];
const WAI_OPS: &[Op] = &[cm(op(Imp, 3))];
const RMB_SHAPE: &[Op] = &[cm(op(Zp, 5))];
const BBR_SHAPE: &[Op] = &[cm(br(op(ZpRel, 5)))];

/// The table. Mnemonics resolve case-insensitively; `RMB0`–`SMB7` and
/// `BBR0`–`BBS7` resolve by prefix.
pub fn ops_for(mnemonic: &str) -> Option<&'static [Op]> {
    let m = mnemonic.to_ascii_uppercase();
    let m = m.as_str();
    if m.len() == 4 {
        let (head, digit) = m.split_at(3);
        if digit.chars().all(|c| c.is_ascii_digit()) {
            match head {
                "RMB" | "SMB" => return Some(RMB_SHAPE),
                "BBR" | "BBS" => return Some(BBR_SHAPE),
                _ => {}
            }
        }
    }
    Some(match m {
        "LDA" | "AND" | "ORA" | "EOR" | "CMP" | "ADC" | "SBC" => LOAD_SHAPE,
        "LDX" => LDX_OPS,
        "LDY" => LDY_OPS,
        "STA" => STA_OPS,
        "STX" => STX_OPS,
        "STY" => STY_OPS,
        "STZ" => STZ_OPS,
        "CPX" | "CPY" => CPXY_OPS,
        "BIT" => BIT_OPS,
        "TRB" | "TSB" => TRB_OPS,
        "INC" | "DEC" => RMW_SHAPE,
        "INX" | "INY" | "DEX" | "DEY" => IMP2,
        "ASL" | "LSR" | "ROL" | "ROR" => SHIFT_SHAPE,
        "BCC" | "BCS" | "BEQ" | "BNE" | "BMI" | "BPL" | "BVC" | "BVS" => BRANCH_SHAPE,
        "BRA" => BRA_OPS,
        "JMP" => JMP_OPS,
        "JSR" => JSR_OPS,
        "RTS" | "RTI" => RET_OPS,
        "BRK" => BRK_OPS,
        "PHA" | "PHP" => PUSH_OPS,
        "PLA" | "PLP" => PULL_OPS,
        "PHX" | "PHY" => PUSH_C_OPS,
        "PLX" | "PLY" => PULL_C_OPS,
        "TAX" | "TAY" | "TXA" | "TYA" | "TSX" | "TXS" => IMP2,
        "CLC" | "SEC" | "CLI" | "SEI" | "CLD" | "SED" | "CLV" => IMP2,
        "NOP" => IMP2,
        "WAI" | "STP" => WAI_OPS,
        _ => return None,
    })
}

/// Guesses the addressing mode from an operand's spelling. Bare labels
/// assume absolute (the assembler's default too).
fn infer_mode(mnemonic: &str, operand: &str) -> Mode {
    let m = mnemonic.to_ascii_uppercase();
    let o = operand.trim();
    let low = o.to_ascii_lowercase();
    if m.len() == 4 && (m.starts_with("BBR") || m.starts_with("BBS")) {
        return ZpRel;
    }
    if matches!(
        m.as_str(),
        "BCC" | "BCS" | "BEQ" | "BNE" | "BMI" | "BPL" | "BVC" | "BVS" | "BRA"
    ) {
        return Rel;
    }
    if o.is_empty() {
        return Imp;
    }
    if low == "a" {
        return Acc;
    }
    if o.starts_with('#') {
        return Imm;
    }
    let zp_sized = |s: &str| {
        let s = s.trim();
        if let Some(h) = s.strip_prefix('$') {
            h.len() <= 2
        } else if let Some(b) = s.strip_prefix('%') {
            b.len() <= 8
        } else {
            s.parse::<u64>().map(|v| v < 256).unwrap_or(false)
        }
    };
    if let Some(inner) = low.strip_prefix('(') {
        if let Some(addr) = inner.strip_suffix(",x)") {
            return if m == "JMP" || !zp_sized(addr) {
                IndAbsX
            } else {
                IndX
            };
        }
        if inner.ends_with("),y") {
            return IndY;
        }
        if inner.ends_with(')') {
            return if m == "JMP" { Ind } else { ZpInd };
        }
    }
    if let Some(addr) = low.strip_suffix(",x") {
        return if zp_sized(addr) { ZpX } else { AbsX };
    }
    if let Some(addr) = low.strip_suffix(",y") {
        return if zp_sized(addr) { ZpY } else { AbsY };
    }
    if zp_sized(o) { Zp } else { Abs }
}

fn find_op(ops: &'static [Op], mode: Mode) -> Option<&'static Op> {
    ops.iter().find(|o| o.mode == mode).or_else(|| {
        // a bare label guessed abs may really be zp (and vice versa)
        let partner = match mode {
            Abs => Zp,
            Zp => Abs,
            AbsX => ZpX,
            ZpX => AbsX,
            AbsY => ZpY,
            ZpY => AbsY,
            _ => return None,
        };
        ops.iter().find(|o| o.mode == partner)
    })
}

/// The machine-code byte for a (mnemonic, mode) pair — the same on NMOS
/// and CMOS for shared instructions; the 65C02 additions have their own.
/// None for a pair the chip doesn't encode. The mnemonic + mode already
/// determine it, so this is a pure lookup, not new knowledge.
pub fn opcode_byte(mnemonic: &str, mode: Mode) -> Option<u8> {
    let m = mnemonic.to_ascii_uppercase();
    // RMB0-7 / SMB0-7 / BBR0-7 / BBS0-7: a base plus bit×$10
    if m.len() == 4 && m.as_bytes()[3].is_ascii_digit() {
        let bit = m.as_bytes()[3] - b'0';
        let base = match &m[..3] {
            "RMB" => 0x07u8,
            "SMB" => 0x87,
            "BBR" => 0x0F,
            "BBS" => 0x8F,
            _ => return None,
        };
        return Some(base + bit * 0x10);
    }
    // ALU / load-store-compare group (cc=01): aaa<<5 | bbb<<2 | 1,
    // and the 65C02 (zp) form at aaa<<5 | $12.
    let alu = |aaa: u8| -> Option<u8> {
        let bbb: u8 = match mode {
            IndX => 0,
            Zp => 1,
            Imm => 2,
            Abs => 3,
            IndY => 4,
            ZpX => 5,
            AbsY => 6,
            AbsX => 7,
            ZpInd => return Some((aaa << 5) | 0x12),
            _ => return None,
        };
        Some((aaa << 5) | (bbb << 2) | 0x01)
    };
    match m.as_str() {
        "ORA" => return alu(0),
        "AND" => return alu(1),
        "EOR" => return alu(2),
        "ADC" => return alu(3),
        "STA" => return alu(4),
        "LDA" => return alu(5),
        "CMP" => return alu(6),
        "SBC" => return alu(7),
        _ => {}
    }
    Some(match (m.as_str(), mode) {
        ("LDX", Imm) => 0xA2,
        ("LDX", Zp) => 0xA6,
        ("LDX", ZpY) => 0xB6,
        ("LDX", Abs) => 0xAE,
        ("LDX", AbsY) => 0xBE,
        ("LDY", Imm) => 0xA0,
        ("LDY", Zp) => 0xA4,
        ("LDY", ZpX) => 0xB4,
        ("LDY", Abs) => 0xAC,
        ("LDY", AbsX) => 0xBC,
        ("STX", Zp) => 0x86,
        ("STX", ZpY) => 0x96,
        ("STX", Abs) => 0x8E,
        ("STY", Zp) => 0x84,
        ("STY", ZpX) => 0x94,
        ("STY", Abs) => 0x8C,
        ("STZ", Zp) => 0x64,
        ("STZ", ZpX) => 0x74,
        ("STZ", Abs) => 0x9C,
        ("STZ", AbsX) => 0x9E,
        ("CPX", Imm) => 0xE0,
        ("CPX", Zp) => 0xE4,
        ("CPX", Abs) => 0xEC,
        ("CPY", Imm) => 0xC0,
        ("CPY", Zp) => 0xC4,
        ("CPY", Abs) => 0xCC,
        ("BIT", Zp) => 0x24,
        ("BIT", Abs) => 0x2C,
        ("BIT", Imm) => 0x89,
        ("BIT", ZpX) => 0x34,
        ("BIT", AbsX) => 0x3C,
        ("TRB", Zp) => 0x14,
        ("TRB", Abs) => 0x1C,
        ("TSB", Zp) => 0x04,
        ("TSB", Abs) => 0x0C,
        ("INC", Acc) => 0x1A,
        ("INC", Zp) => 0xE6,
        ("INC", ZpX) => 0xF6,
        ("INC", Abs) => 0xEE,
        ("INC", AbsX) => 0xFE,
        ("DEC", Acc) => 0x3A,
        ("DEC", Zp) => 0xC6,
        ("DEC", ZpX) => 0xD6,
        ("DEC", Abs) => 0xCE,
        ("DEC", AbsX) => 0xDE,
        ("INX", _) => 0xE8,
        ("INY", _) => 0xC8,
        ("DEX", _) => 0xCA,
        ("DEY", _) => 0x88,
        ("ASL", Acc) => 0x0A,
        ("ASL", Zp) => 0x06,
        ("ASL", ZpX) => 0x16,
        ("ASL", Abs) => 0x0E,
        ("ASL", AbsX) => 0x1E,
        ("LSR", Acc) => 0x4A,
        ("LSR", Zp) => 0x46,
        ("LSR", ZpX) => 0x56,
        ("LSR", Abs) => 0x4E,
        ("LSR", AbsX) => 0x5E,
        ("ROL", Acc) => 0x2A,
        ("ROL", Zp) => 0x26,
        ("ROL", ZpX) => 0x36,
        ("ROL", Abs) => 0x2E,
        ("ROL", AbsX) => 0x3E,
        ("ROR", Acc) => 0x6A,
        ("ROR", Zp) => 0x66,
        ("ROR", ZpX) => 0x76,
        ("ROR", Abs) => 0x6E,
        ("ROR", AbsX) => 0x7E,
        ("JMP", Abs) => 0x4C,
        ("JMP", Ind) => 0x6C,
        ("JMP", IndAbsX) => 0x7C,
        ("JSR", Abs) => 0x20,
        ("RTS", _) => 0x60,
        ("RTI", _) => 0x40,
        ("BRK", _) => 0x00,
        ("BPL", _) => 0x10,
        ("BMI", _) => 0x30,
        ("BVC", _) => 0x50,
        ("BVS", _) => 0x70,
        ("BCC", _) => 0x90,
        ("BCS", _) => 0xB0,
        ("BNE", _) => 0xD0,
        ("BEQ", _) => 0xF0,
        ("BRA", _) => 0x80,
        ("CLC", _) => 0x18,
        ("SEC", _) => 0x38,
        ("CLI", _) => 0x58,
        ("SEI", _) => 0x78,
        ("CLV", _) => 0xB8,
        ("CLD", _) => 0xD8,
        ("SED", _) => 0xF8,
        ("TAX", _) => 0xAA,
        ("TXA", _) => 0x8A,
        ("TAY", _) => 0xA8,
        ("TYA", _) => 0x98,
        ("TSX", _) => 0xBA,
        ("TXS", _) => 0x9A,
        ("PHA", _) => 0x48,
        ("PLA", _) => 0x68,
        ("PHP", _) => 0x08,
        ("PLP", _) => 0x28,
        ("PHX", _) => 0xDA,
        ("PLX", _) => 0xFA,
        ("PHY", _) => 0x5A,
        ("PLY", _) => 0x7A,
        ("NOP", _) => 0xEA,
        ("WAI", _) => 0xCB,
        ("STP", _) => 0xDB,
        _ => return None,
    })
}

/// Splits an assembly line into (mnemonic, operand), skipping a leading
/// label, and stripping a `;` comment. Returns None for blank, directive,
/// or label-only lines.
///
/// Labels come with or without a colon (Acme and ca65 both allow bare),
/// which collides with the mnemonic slot. Two signals resolve it: a
/// column-0 token is a label unless it names an opcode (un-indented code),
/// while an indented first token is the mnemonic itself (or a macro the
/// caller will find unknown). So `s2   ROL $34` yields ROL, not s2.
fn split_line(line: &str) -> Option<(String, String)> {
    let code = line.split(';').next().unwrap_or("");
    if code.trim().is_empty() {
        return None;
    }
    let indented = code.starts_with([' ', '\t']);
    let mut s = code.trim();
    if let Some(colon) = s.find(':')
        && s[..colon]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '@')
    {
        s = s[colon + 1..].trim();
    }
    if s.is_empty() || s.starts_with(['.', '!', '*', '=', '+', '-']) || s.contains('=') {
        return None;
    }
    let peel = |t: &str| match t.find(char::is_whitespace) {
        Some(i) => (t[..i].to_string(), t[i..].trim_start().to_string()),
        None => (t.to_string(), String::new()),
    };
    let alnum = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric());
    let (t0, r0) = peel(s);
    if indented || ops_for(&t0).is_some() {
        // first token is the mnemonic position (indented line, or a
        // column-0 opcode written without indentation)
        return alnum(&t0).then_some((t0, r0));
    }
    // column-0 token that isn't an opcode is a bare label; take the next
    let (t1, r1) = peel(&r0);
    alnum(&t1).then_some((t1, r1))
}

/// One-line description of what a mnemonic does, with the flags it sets.
/// ISA-independent (LDA loads the accumulator on NMOS and CMOS alike);
/// `RMB`/`SMB`/`BBR`/`BBS` resolve by prefix.
pub fn describe(mnemonic: &str) -> Option<&'static str> {
    let m = mnemonic.to_ascii_uppercase();
    let key = if m.len() == 4 && m.as_bytes()[3].is_ascii_digit() {
        &m[..3]
    } else {
        m.as_str()
    };
    Some(match key {
        "LDA" => "load accumulator (N,Z)",
        "LDX" => "load X (N,Z)",
        "LDY" => "load Y (N,Z)",
        "STA" => "store accumulator",
        "STX" => "store X",
        "STY" => "store Y",
        "STZ" => "store zero (65C02)",
        "TAX" => "transfer A→X (N,Z)",
        "TAY" => "transfer A→Y (N,Z)",
        "TXA" => "transfer X→A (N,Z)",
        "TYA" => "transfer Y→A (N,Z)",
        "TSX" => "transfer SP→X (N,Z)",
        "TXS" => "transfer X→SP",
        "PHA" => "push accumulator",
        "PHP" => "push processor status",
        "PLA" => "pull accumulator (N,Z)",
        "PLP" => "pull processor status (all flags)",
        "PHX" => "push X (65C02)",
        "PHY" => "push Y (65C02)",
        "PLX" => "pull X (65C02) (N,Z)",
        "PLY" => "pull Y (65C02) (N,Z)",
        "ADC" => "add with carry (N,V,Z,C)",
        "SBC" => "subtract with carry (N,V,Z,C)",
        "CMP" => "compare with accumulator (N,Z,C)",
        "CPX" => "compare with X (N,Z,C)",
        "CPY" => "compare with Y (N,Z,C)",
        "INC" => "increment memory or A (N,Z)",
        "DEC" => "decrement memory or A (N,Z)",
        "INX" => "increment X (N,Z)",
        "INY" => "increment Y (N,Z)",
        "DEX" => "decrement X (N,Z)",
        "DEY" => "decrement Y (N,Z)",
        "AND" => "AND with accumulator (N,Z)",
        "ORA" => "OR with accumulator (N,Z)",
        "EOR" => "exclusive-OR with accumulator (N,Z)",
        "BIT" => "test bits (N=b7, V=b6, Z)",
        "TRB" => "test and reset bits (65C02) (Z)",
        "TSB" => "test and set bits (65C02) (Z)",
        "ASL" => "arithmetic shift left (N,Z,C)",
        "LSR" => "logical shift right (N,Z,C)",
        "ROL" => "rotate left through carry (N,Z,C)",
        "ROR" => "rotate right through carry (N,Z,C)",
        "JMP" => "jump",
        "JSR" => "jump to subroutine",
        "RTS" => "return from subroutine",
        "RTI" => "return from interrupt",
        "BRK" => "force break / software interrupt",
        "BCC" => "branch if carry clear",
        "BCS" => "branch if carry set",
        "BEQ" => "branch if equal (Z=1)",
        "BNE" => "branch if not equal (Z=0)",
        "BMI" => "branch if minus (N=1)",
        "BPL" => "branch if plus (N=0)",
        "BVC" => "branch if overflow clear",
        "BVS" => "branch if overflow set",
        "BRA" => "branch always (65C02)",
        "CLC" => "clear carry",
        "SEC" => "set carry",
        "CLI" => "clear interrupt disable",
        "SEI" => "set interrupt disable",
        "CLD" => "clear decimal mode",
        "SED" => "set decimal mode",
        "CLV" => "clear overflow",
        "NOP" => "no operation",
        "WAI" => "wait for interrupt (65C02)",
        "STP" => "stop the processor (65C02)",
        "RMB" => "reset memory bit (65C02)",
        "SMB" => "set memory bit (65C02)",
        "BBR" => "branch if bit reset (65C02)",
        "BBS" => "branch if bit set (65C02)",
        _ => return None,
    })
}

/// The opcode float for the instruction on this line, or None when the
/// line isn't a known instruction. `family` gates what "cycles" means.
pub fn opcode_hover(line: &str, family: &Family) -> Option<Vec<String>> {
    let (mn, operand) = split_line(line)?;
    let ops = ops_for(&mn)?;
    let mode = infer_mode(&mn, &operand);
    let mnu = mn.to_ascii_uppercase();
    let title = match describe(&mnu) {
        Some(desc) => format!("{mnu} — {desc}"),
        None => mnu.clone(),
    };
    let Some(entry) = find_op(ops, mode) else {
        let modes: Vec<&str> = ops.iter().map(|o| o.mode.label()).collect();
        return Some(vec![title, format!("modes: {}", modes.join(" · "))]);
    };
    let mut out = vec![title];
    let nmos = matches!(family, Family::Nmos6502);
    if nmos && entry.cmos_only {
        out.push("65C02 only — not available on NMOS 6502".into());
        return Some(out);
    }
    let byte = match opcode_byte(&mnu, entry.mode) {
        Some(b) => format!("${b:02X} · "),
        None => String::new(),
    };
    out.push(format!(
        "{byte}{} · {} byte{}",
        entry.mode.label(),
        entry.mode.bytes(),
        if entry.mode.bytes() == 1 { "" } else { "s" }
    ));
    let cyc = if nmos {
        entry.nmos.unwrap_or(entry.cyc)
    } else {
        entry.cyc
    };
    let mut timing = format!("{cyc} cycles");
    if entry.branch {
        timing.push_str(" · +1 taken · +1 more on page cross");
    } else if entry.page && !(nmos && entry.nmos.is_some()) {
        timing.push_str(" · +1 if page crossed");
    }
    if !nmos && let Some(n) = entry.nmos {
        timing.push_str(&format!(" (NMOS: {n})"));
    }
    out.push(timing);
    if matches!(mnu.as_str(), "ADC" | "SBC") {
        out.push("+1 cycle in decimal mode (65C02)".into());
    }
    Some(out)
}

/// Sums cycles over selected lines: straight-line honesty — base cycles
/// summed, penalties reported as ranges, unknowns counted, never guessed.
pub fn cycle_sum(lines: &[String], family: &Family) -> Vec<String> {
    let nmos = matches!(family, Family::Nmos6502);
    let mut instructions = 0usize;
    let mut cycles = 0usize;
    let mut branches = 0usize;
    let mut page_risk = 0usize;
    let mut unknown = 0usize;
    let mut cmos_clash = 0usize;
    for line in lines {
        let Some((mn, operand)) = split_line(line) else {
            continue;
        };
        let Some(ops) = ops_for(&mn) else {
            unknown += 1;
            continue;
        };
        let Some(entry) = find_op(ops, infer_mode(&mn, &operand)) else {
            unknown += 1;
            continue;
        };
        if nmos && entry.cmos_only {
            cmos_clash += 1;
            continue;
        }
        instructions += 1;
        cycles += if nmos {
            entry.nmos.unwrap_or(entry.cyc) as usize
        } else {
            entry.cyc as usize
        };
        if entry.branch {
            branches += 1;
        } else if entry.page && !(nmos && entry.nmos.is_some()) {
            page_risk += 1;
        }
    }
    let chip = if nmos { "NMOS 6502" } else { "65C02" };
    let mut out = vec![format!(
        "{instructions} instruction{} — {cycles} cycles ({chip})",
        if instructions == 1 { "" } else { "s" }
    )];
    if branches > 0 {
        out.push(format!(
            "+1 per taken branch (×{branches}), +1 more on page cross"
        ));
    }
    if page_risk > 0 {
        out.push(format!("+1 possible page cross (×{page_risk})"));
    }
    if unknown > 0 {
        out.push(format!("not counted: {unknown} line(s) — macro or unknown"));
    }
    if cmos_clash > 0 {
        out.push(format!("{cmos_clash} instruction(s) are 65C02-only!"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fam() -> Family {
        Family::Cmos65c02
    }

    #[test]
    fn modeline_leaders() {
        for l in [
            "; asm: 65c02 acme",
            ";; asm: 65c02",
            "# asm: 65c02 gas",
            "// asm: 65c02",
            "/* asm: 65c02 gas */",
            "@ asm: 65C02",
        ] {
            assert_eq!(
                modeline(std::iter::once(l.to_string())),
                Some(Family::Cmos65c02),
                "{l}"
            );
        }
        assert_eq!(
            modeline(std::iter::once("# asm: rv32e gas".to_string())),
            Some(Family::Other("rv32e".into()))
        );
        assert_eq!(
            modeline(std::iter::once("the asm: directive".to_string())),
            None
        );
        assert_eq!(modeline(std::iter::once("lda #$10".to_string())), None);
    }

    #[test]
    fn number_bases() {
        let h = number_hover("$9F42").unwrap();
        assert!(h[0].contains("16-bit"));
        assert!(h[1].contains("dec 40770"));
        assert!(h[1].contains("i16 -24766"));
        assert!(h[2].contains("%1001'1111'0100'0010"));
        assert!(h[3].contains("lo $42 · hi $9F · swap $429F"));

        let h = number_hover("255").unwrap();
        assert!(h[0].contains("8-bit"));
        assert!(h[1].contains("hex $FF"));

        let h = number_hover("%11110000").unwrap();
        assert!(h[1].contains("dec 240"));
        assert!(number_hover("hello").is_none());
    }

    #[test]
    fn literal_extraction() {
        let line = "    lda #$9f42   ; load";
        let col = line.find('9').unwrap();
        assert_eq!(literal_at(line, col).as_deref(), Some("$9f42"));
        assert_eq!(literal_at("done 100%", 6).as_deref(), Some("100"));
        assert_eq!(literal_at("abc", 1).as_deref(), Some("abc")); // parses to None later
    }

    #[test]
    fn opcode_cycles_spot_checks() {
        // line 0 = description, line 1 = encoding, line 2 = timing
        let h = opcode_hover("   lda ($3e),y", &fam()).unwrap();
        assert!(h[0].contains("LDA — load accumulator"), "{h:?}");
        assert!(h[1].contains("(zp),Y"), "{h:?}");
        assert!(h[2].contains("5 cycles · +1 if page crossed"), "{h:?}");

        let h = opcode_hover("jmp ($fffc)", &fam()).unwrap();
        assert!(h[2].contains("6 cycles (NMOS: 5)"), "{h:?}");

        let h = opcode_hover("asl $1000,x", &Family::Nmos6502).unwrap();
        assert!(h[2].starts_with("7 cycles"), "{h:?}");
        let h = opcode_hover("asl $1000,x", &fam()).unwrap();
        assert!(h[2].contains("6 cycles · +1 if page crossed"), "{h:?}");

        let h = opcode_hover("bne loop", &fam()).unwrap();
        assert!(h[2].contains("2 cycles · +1 taken"), "{h:?}");

        let h = opcode_hover("stz $10", &Family::Nmos6502).unwrap();
        assert!(h[1].contains("not available"), "{h:?}");

        let h = opcode_hover("loop:  dex", &fam()).unwrap();
        assert!(h[2].starts_with("2 cycles"), "{h:?}");

        assert!(opcode_hover("syscall SYSFN_PRINT", &fam()).is_none());
        assert!(opcode_hover(".section .text", &fam()).is_none());
    }

    #[test]
    fn label_before_mnemonic_is_seen() {
        // the reported bug: a bare (colon-less) label hid the instruction
        for line in ["s2   ROL $34", "s2:  ROL $34", "loop    rol $34"] {
            let h = opcode_hover(line, &fam()).unwrap();
            assert!(h[0].contains("ROL — rotate left"), "{line}: {h:?}");
            assert!(h[1].contains("zp"), "{line}: {h:?}"); // $34 is zero page
            assert!(h[2].starts_with("5 cycles"), "{line}: {h:?}");
        }
        // an un-indented instruction with no label still resolves
        let h = opcode_hover("lda #$10", &fam()).unwrap();
        assert!(h[0].contains("LDA"), "{h:?}");
        // a bare label alone is not an instruction
        assert!(opcode_hover("forever", &fam()).is_none());
        // a labelled macro stays unknown (label stripped, macro not in table)
        assert!(opcode_hover("s2  syscall 3", &fam()).is_none());
    }

    const ALL_MNEMONICS: &[&str] = &[
        "LDA", "LDX", "LDY", "STA", "STX", "STY", "STZ", "AND", "ORA", "EOR", "ADC", "SBC", "CMP",
        "CPX", "CPY", "BIT", "TRB", "TSB", "INC", "DEC", "INX", "INY", "DEX", "DEY", "ASL", "LSR",
        "ROL", "ROR", "JMP", "JSR", "RTS", "RTI", "BRK", "BCC", "BCS", "BEQ", "BNE", "BMI", "BPL",
        "BVC", "BVS", "BRA", "CLC", "SEC", "CLI", "SEI", "CLD", "SED", "CLV", "TAX", "TAY", "TXA",
        "TYA", "TSX", "TXS", "PHA", "PHP", "PLA", "PLP", "PHX", "PHY", "PLX", "PLY", "NOP", "WAI",
        "STP",
    ];

    #[test]
    fn opcode_bytes_spot_checks() {
        assert_eq!(opcode_byte("LDA", Imm), Some(0xA9));
        assert_eq!(opcode_byte("LDA", IndY), Some(0xB1));
        assert_eq!(opcode_byte("LDA", ZpInd), Some(0xB2)); // 65C02 (zp)
        assert_eq!(opcode_byte("STA", Abs), Some(0x8D));
        assert_eq!(opcode_byte("JMP", Ind), Some(0x6C));
        assert_eq!(opcode_byte("JSR", Abs), Some(0x20));
        assert_eq!(opcode_byte("BNE", Rel), Some(0xD0));
        assert_eq!(opcode_byte("ROL", Zp), Some(0x26));
        assert_eq!(opcode_byte("INX", Imp), Some(0xE8));
        assert_eq!(opcode_byte("BRK", Imp), Some(0x00));
        assert_eq!(opcode_byte("STZ", Abs), Some(0x9C));
        assert_eq!(opcode_byte("RMB3", Zp), Some(0x37));
        assert_eq!(opcode_byte("BBS3", ZpRel), Some(0xBF));
    }

    #[test]
    fn opcode_bytes_unique_and_cover_the_cycle_table() {
        // every (mnemonic, mode) the cycle table lists must encode to a
        // byte, and no two pairs may share one — a transcription tripwire
        let mut seen: std::collections::HashMap<u8, String> = std::collections::HashMap::new();
        let mut check = |mn: &str| {
            for o in ops_for(mn).unwrap_or_else(|| panic!("no ops: {mn}")) {
                let b =
                    opcode_byte(mn, o.mode).unwrap_or_else(|| panic!("no byte: {mn} {:?}", o.mode));
                if let Some(prev) = seen.insert(b, format!("{mn} {:?}", o.mode)) {
                    panic!("byte ${b:02X}: {prev} vs {mn} {:?}", o.mode);
                }
            }
        };
        for mn in ALL_MNEMONICS {
            check(mn);
        }
        for base in ["RMB", "SMB", "BBR", "BBS"] {
            for d in 0..8 {
                check(&format!("{base}{d}"));
            }
        }
        assert!(seen.len() > 140, "only {} opcodes mapped", seen.len());
    }

    #[test]
    fn descriptions_cover_the_table() {
        // every mnemonic the opcode table knows also has a description
        for m in [
            "LDA", "STZ", "PHX", "ADC", "BIT", "ROL", "JSR", "BRA", "CLV", "WAI", "RMB0", "BBS3",
        ] {
            assert!(describe(m).is_some(), "no description for {m}");
        }
        assert!(describe("SYSCALL").is_none());
    }

    #[test]
    fn cycle_sum_msb_compare() {
        // the MSB-first compare chain from x16-math's udiv-6502-opt.s:
        // 8 cycles when the first compare decides (lda zp 3, cmp zp 3,
        // bcc rel 2)
        let lines: Vec<String> = ["  lda $17", "  cmp $1b", "  bcc next"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = cycle_sum(&lines, &fam());
        assert!(out[0].contains("3 instructions — 8 cycles"), "{out:?}");
        assert!(out[1].contains("+1 per taken branch (×1)"), "{out:?}");
    }

    #[test]
    fn cycle_sum_honesty() {
        let lines: Vec<String> = ["  syscall 3", "  lda #1", "; comment", "  stz $10"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = cycle_sum(&lines, &Family::Nmos6502);
        assert!(out[0].contains("1 instruction — 2 cycles"), "{out:?}");
        assert!(out.iter().any(|l| l.contains("not counted: 1")), "{out:?}");
        assert!(out.iter().any(|l| l.contains("65C02-only")), "{out:?}");
    }
}
