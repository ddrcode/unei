// Bespoke tree-sitter grammar for Lattice preference files (.lpf) — the pin
// and timing constraints that sit beside an ECP5 project's SystemVerilog
// (no ticket; asked for 2026-11-20). Scope: what highlighting needs. A
// preference is a command word, its arguments — quoted names, numbers,
// KEY=VALUE settings, further keywords — and a terminating `;`; a comment
// runs from `#` or `//` to the end of the line. Keywords are not
// enumerated: Diamond's and nextpnr's vocabularies differ and grow, and an
// unknown command must not become an ERROR. Both write keywords in
// capitals, so the highlight query takes the command by position and the
// rest by case.

module.exports = grammar({
  name: 'lpf',

  extras: ($) => [/\s/, $.comment],

  rules: {
    source_file: ($) => repeat($.statement),

    statement: ($) =>
      seq(field('command', $.word), repeat($._argument), ';'),

    _argument: ($) => choice($.assignment, $.string, $.number, $.word),

    // PULLMODE=NONE, DRIVE=4, CONFIG_IOVOLTAGE=3.3
    assignment: ($) =>
      seq(
        field('key', $.word),
        '=',
        field('value', choice($.string, $.number, $.word)),
      ),

    // "clk_25mhz", "led[0]", "G2" — names and sites are quoted
    string: () => /"[^"\n]*"/,

    number: () => /-?[0-9]+(\.[0-9]+)?/,

    // a keyword, a setting, or an unquoted name (inst/sub, net$12)
    word: () => /[A-Za-z_][A-Za-z0-9_.\[\]*$:\/-]*/,

    comment: () => token(choice(seq('#', /.*/), seq('//', /.*/))),
  },
});
