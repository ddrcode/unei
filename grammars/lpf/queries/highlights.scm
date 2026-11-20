;; Lattice preference files: a command, its arguments, a `;`. Patterns are
;; ordered for the engine's first-wins ties — a setting's key and value
;; before the general "a capitalised word is a keyword" rule.

(comment) @comment

(string) @string

(number) @number

;; KEY=VALUE — the setting and what it is set to
(assignment
  key: (word) @property)

(assignment
  value: (word) @constant)

"=" @operator

";" @punctuation.delimiter

;; the command that opens a preference …
(statement
  command: (word) @keyword)

;; … and every other word in capitals: LPF keywords are written so, while
;; names are quoted or lower-case (BLKNAME cpu0)
((word) @keyword
  (#match? @keyword "^[A-Z][A-Z0-9_]*$"))
