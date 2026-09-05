; 6502/65C02 (ACME) highlighting — Unei #18.

(comment) @comment

(mnemonic) @keyword
(directive_name) @attribute
(macro_name) @function

(label) @label

(number) @number
(string) @string
(char) @constant

; registers A/X/Y (must precede the general symbol rule: first pattern wins)
((identifier) @variable.builtin
 (#match? @variable.builtin "^[AaXxYy]$"))
(identifier) @variable

"#" @operator
["<" ">" "+" "-" "*" "/" "&" "|" "^" "~" "="] @operator
["(" ")" "[" "]" "{" "}" ","] @punctuation
