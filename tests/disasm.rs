//! The `.prg` disassembler (#65): opening a compiled 6502 image shows its
//! 65C02 disassembly instead of binary, decoded by the machine lens's own
//! opcode table.

use unei::core::buffer::Buffer;

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("unei-disasm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn opens_a_prg_as_disassembly() {
    let dir = tmp("open");
    let prg = dir.join("spin.prg");
    // load $c000, then a border-flash spin:
    //   lda #$02 / sta $d020 / ldx #$ff / dex / bne (loop) / rts
    let bytes = [
        0x00, 0xC0, // load address $c000 (little-endian)
        0xA9, 0x02, // lda #$02
        0x8D, 0x20, 0xD0, // sta $d020
        0xA2, 0xFF, // ldx #$ff
        0xCA, // dex        ($c007)
        0xD0, 0xFD, // bne $c007  ($c008 + 2 - 3)
        0x60, // rts
    ];
    std::fs::write(&prg, bytes).unwrap();

    let buf = Buffer::from_prg(&prg).unwrap();
    let text = buf.rope.to_string();

    assert!(text.contains("; asm: 65c02 acme"), "{text}"); // highlights + lens
    assert!(text.contains("* = $c000"), "{text}");
    assert!(text.contains("lda #$02"), "{text}");
    assert!(text.contains("sta $d020"), "{text}");
    assert!(text.contains("ldx #$ff"), "{text}");
    assert!(text.contains("dex"), "{text}");
    assert!(text.contains("bne $c007"), "{text}"); // branch target resolved
    assert!(text.contains("rts"), "{text}");

    // bound to a synthetic `.s` path: highlights, and `:w` saves source
    // rather than clobbering the binary
    let path = buf.path.unwrap().to_string_lossy().into_owned();
    assert!(path.ends_with("spin.prg.disasm.s"), "{path}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_too_small_prg_is_an_error_not_a_panic() {
    let dir = tmp("small");
    let prg = dir.join("tiny.prg");
    std::fs::write(&prg, [0x01]).unwrap(); // only 1 byte: not even a load address
    assert!(Buffer::from_prg(&prg).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
