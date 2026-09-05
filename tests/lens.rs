//! The machine lens (#45): K dispatch, modeline gating, cycle sums.

use std::path::PathBuf;

use tailored::core::buffer::Buffer;
use tailored::editor::Editor;
use tailored::editor::testing::feed;

fn editor(content: &str, name: &str) -> Editor {
    let mut b = Buffer::from_text(content);
    b.path = Some(PathBuf::from(name));
    let mut ed = Editor::new(b);
    ed.set_view(80, 24);
    ed
}

fn float_text(ed: &Editor) -> String {
    ed.info_float
        .as_ref()
        .map(|l| l.join("\n"))
        .unwrap_or_default()
}

#[test]
fn number_lens_works_in_any_file() {
    // a yaml benchmark file, cursor onto the decimal value
    let mut ed = editor("inputs: [133215, 33]\n", "udiv.yaml");
    feed(&mut ed, "f1");
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(float.contains("hex $0002085F"), "{float}");
    assert!(float.contains("dec 133215"), "{float}");
    assert!(float.contains("bytes 5F 08 02 00 (le)"), "{float}");
}

#[test]
fn number_lens_shows_signedness_and_bytes() {
    let mut ed = editor("mask $FF3A here\n", "notes.md");
    feed(&mut ed, "fF");
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(float.contains("16-bit"), "{float}");
    assert!(float.contains("i16 -198"), "{float}");
    assert!(float.contains("lo $3A · hi $FF"), "{float}");
}

#[test]
fn opcode_lens_requires_modeline() {
    let src = "        lda ($3e),y\n";
    let mut ed = editor(src, "code.s");
    feed(&mut ed, "fl");
    feed(&mut ed, "K");
    assert!(ed.info_float.is_none(), "no modeline, no opcode lens");

    let src = "; asm: 65c02 acme\n        lda ($3e),y\n";
    let mut ed = editor(src, "code.s");
    feed(&mut ed, "k"); // down to the instruction (IJKL)
    feed(&mut ed, "fl");
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(float.contains("LDA — load accumulator (N,Z)"), "{float}");
    assert!(float.contains("(zp),Y · 2 bytes"), "{float}");
    assert!(float.contains("5 cycles · +1 if page crossed"), "{float}");
}

#[test]
fn opcode_lens_sees_through_a_label() {
    // the reported bug: `s2   ROL $34` (bare label) showed nothing
    let src = "; asm: 65c02 acme\ns2   rol $34\n";
    let mut ed = editor(src, "code.s");
    feed(&mut ed, "k");
    feed(&mut ed, "fr"); // cursor onto the mnemonic
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(float.contains("ROL — rotate left through carry"), "{float}");
    assert!(float.contains("5 cycles"), "{float}"); // $34 is zero page
}

#[test]
fn nmos_and_cmos_disagree_where_history_did() {
    let src = "; asm: 6502 ca65\n        jmp ($fffc)\n        stz $10\n";
    let mut ed = editor(src, "old.s");
    feed(&mut ed, "k");
    feed(&mut ed, "fj");
    feed(&mut ed, "K");
    assert!(float_text(&ed).starts_with("JMP"), "{}", float_text(&ed));
    assert!(float_text(&ed).contains("5 cycles"), "{}", float_text(&ed));
    feed(&mut ed, "<Esc>"); // dismiss float
    feed(&mut ed, "k");
    feed(&mut ed, "fs");
    feed(&mut ed, "K");
    assert!(
        float_text(&ed).contains("not available on NMOS 6502"),
        "{}",
        float_text(&ed)
    );
}

#[test]
fn cursor_on_number_wins_over_opcode() {
    let src = "; asm: 65c02 acme\n        lda #$9f42\n";
    let mut ed = editor(src, "code.s");
    feed(&mut ed, "k");
    feed(&mut ed, "f9");
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(float.contains("dec 40770"), "number, not LDA: {float}");
}

#[test]
fn visual_cycle_sum_counts_the_compare_chain() {
    // the 8-cycle MSB-first compare from x16-math's udiv-6502-opt.s
    let src = "; asm: 65c02 acme\n        lda $17\n        cmp $1b\n        bcc sub\n";
    let mut ed = editor(src, "udiv.s");
    feed(&mut ed, "k"); // onto lda
    feed(&mut ed, "Vkk"); // select the three lines
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(
        float.contains("3 instructions — 8 cycles (65C02)"),
        "{float}"
    );
    assert!(float.contains("+1 per taken branch (×1)"), "{float}");
}

#[test]
fn cycle_sum_reports_what_it_cannot_know() {
    let src = "; asm: 65c02 acme\nfn start\n        syscall 3\n        lda #1\n";
    let mut ed = editor(src, "os.s");
    feed(&mut ed, "kk"); // onto syscall (fn line is unknown too)
    feed(&mut ed, "Vk");
    feed(&mut ed, "K");
    let float = float_text(&ed);
    assert!(float.contains("1 instruction — 2 cycles"), "{float}");
    assert!(float.contains("not counted: 1 line(s)"), "{float}");
}

#[test]
fn k_without_anything_to_say_stays_calm() {
    let mut ed = editor("just prose here\n", "notes.md");
    feed(&mut ed, "K");
    assert!(ed.info_float.is_none());
    assert!(
        ed.message
            .as_ref()
            .unwrap()
            .text
            .contains("nothing to tell"),
        "a message, not silence"
    );
}
