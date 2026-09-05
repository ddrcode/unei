// Bespoke tree-sitter grammar for 6502/65C02 assembly in ACME syntax
// (tailorED ticket #18, simplest form). Scope: what highlighting needs —
// comments, labels, mnemonics, ACME pseudo-ops (!byte, * =, +macro), and
// operand atoms (hex/bin/dec, strings, char, symbol refs). Not a full
// expression grammar; operands are a flat token run, which is plenty for
// coloring. Line-oriented, relying on the editor's newline-terminated
// buffer invariant.

const MNEMONICS = [
  'adc', 'and', 'asl', 'bcc', 'bcs', 'beq', 'bit', 'bmi', 'bne', 'bpl',
  'brk', 'bvc', 'bvs', 'clc', 'cld', 'cli', 'clv', 'cmp', 'cpx', 'cpy',
  'dec', 'dex', 'dey', 'eor', 'inc', 'inx', 'iny', 'jmp', 'jsr', 'lda',
  'ldx', 'ldy', 'lsr', 'nop', 'ora', 'pha', 'php', 'pla', 'plp', 'rol',
  'ror', 'rti', 'rts', 'sbc', 'sec', 'sed', 'sei', 'sta', 'stx', 'sty',
  'tax', 'tay', 'tsx', 'txa', 'txs', 'tya',
  // 65C02 additions
  'bra', 'phx', 'phy', 'plx', 'ply', 'stz', 'trb', 'tsb', 'wai', 'stp',
  'rmb0', 'rmb1', 'rmb2', 'rmb3', 'rmb4', 'rmb5', 'rmb6', 'rmb7',
  'smb0', 'smb1', 'smb2', 'smb3', 'smb4', 'smb5', 'smb6', 'smb7',
  'bbr0', 'bbr1', 'bbr2', 'bbr3', 'bbr4', 'bbr5', 'bbr6', 'bbr7',
  'bbs0', 'bbs1', 'bbs2', 'bbs3', 'bbs4', 'bbs5', 'bbs6', 'bbs7',
];
const ci = (w) =>
  w.split('').map((c) => (/[a-z]/.test(c) ? `[${c}${c.toUpperCase()}]` : c)).join('');
const MNEM = new RegExp('(?:' + MNEMONICS.map(ci).join('|') + ')');

module.exports = grammar({
  name: 'asm6502',
  extras: () => [/[ \t]+/],

  rules: {
    source_file: ($) => repeat($._line),

    _line: ($) =>
      seq(
        optional(
          choice(
            $.pc_assignment,
            seq($.label, optional($._statement)),
            $._statement,
            '{',
            '}',
          ),
        ),
        optional($.comment),
        /\r?\n/,
      ),

    _statement: ($) => choice($.instruction, $.directive, $.macro_call),

    label: ($) => seq(field('name', $._symbol), optional(':')),

    pc_assignment: ($) => seq('*', '=', repeat($._operand_tok)),

    instruction: ($) =>
      seq(field('mnemonic', $.mnemonic), repeat($._operand_tok)),
    mnemonic: () => token(prec(2, MNEM)),

    directive: ($) => seq(field('name', $.directive_name), repeat($._operand_tok)),
    directive_name: () => /![A-Za-z0-9_]+/,

    macro_call: ($) => seq(field('name', $.macro_name), repeat($._operand_tok)),
    macro_name: () => /\+[A-Za-z_][A-Za-z0-9_]*/,

    _operand_tok: ($) =>
      choice(
        $.number,
        $.string,
        $.char,
        $.identifier,
        '#', '<', '>', '(', ')', '[', ']', '{', '}', ',',
        '+', '-', '*', '/', '=', '&', '|', '^', '~',
      ),

    number: () => token(choice(/\$[0-9A-Fa-f]+/, /%[01]+/, /[0-9]+/)),
    string: () => /"[^"\n]*"/,
    char: () => /'[^'\n]'?/,
    identifier: ($) => $._symbol,
    _symbol: () => /[.@]?[A-Za-z_][A-Za-z0-9_]*/,
    comment: () => /;[^\n]*/,
  },
});
