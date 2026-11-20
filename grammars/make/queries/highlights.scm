;; Makefile highlights — unei's own, after nvim-treesitter's `make` query,
;; written in the theme's capture vocabulary and ordered for the engine's
;; first-wins ties: the specific pattern comes before the general one.
;; The grammar itself is the tree-sitter-make crate; only the queries live
;; here (see README.md).

(comment) @comment

;; --- rules ----------------------------------------------------------------

;; special targets are directives to make, not things it builds
(rule
  (targets
    (word) @keyword
    (#any-of? @keyword
      ".DEFAULT" ".DELETE_ON_ERROR" ".EXPORT_ALL_VARIABLES" ".IGNORE" ".INTERMEDIATE"
      ".LOW_RESOLUTION_TIME" ".NOTPARALLEL" ".ONESHELL" ".PHONY" ".POSIX" ".PRECIOUS"
      ".SECONDARY" ".SECONDEXPANSION" ".SILENT" ".SUFFIXES" ".WAIT")))

(rule
  (targets
    (word) @function))

(rule
  (targets
    (concatenation
      (word) @function)))

;; what .PHONY lists are targets
(rule
  (targets) @_phony
  (prerequisites
    (word) @function)
  (#eq? @_phony ".PHONY"))

(rule
  [
    ":"
    "&:"
    "::"
    "|"
  ] @operator)

;; recipe-line prefixes: silence, ignore errors, always run
(recipe_line
  [
    "@"
    "-"
    "+"
  ] @punctuation.special)

;; --- directives -----------------------------------------------------------

[
  "ifeq"
  "ifneq"
  "ifdef"
  "ifndef"
  "else"
  "endif"
  "define"
  "endef"
  "include"
  "sinclude"
  "-include"
  "export"
  "unexport"
  "override"
  "undefine"
  "private"
  "vpath"
] @keyword

(ifeq_directive
  [
    "("
    ","
    ")"
  ] @punctuation.bracket)

(ifneq_directive
  [
    "("
    ","
    ")"
  ] @punctuation.bracket)

(include_directive
  filenames: (list
    (word) @string.special.path))

;; --- variables ------------------------------------------------------------

;; make's own variables, where they are set and where they are read
(variable_assignment
  name: (word) @variable.builtin
  (#any-of? @variable.builtin
    ".DEFAULT_GOAL" ".EXTRA_PREREQS" ".FEATURES" ".INCLUDE_DIRS" ".RECIPEPREFIX" ".SHELLFLAGS"
    ".VARIABLES" "MAKE" "MAKEARGS" "MAKECMDGOALS" "MAKEFILE_LIST" "MAKEFLAGS" "MAKE_RESTARTS"
    "MAKE_TERMERR" "MAKE_TERMOUT" "MAKE_VERSION" "SHELL"))

(variable_reference
  (word) @variable.builtin
  (#any-of? @variable.builtin
    ".DEFAULT_GOAL" ".EXTRA_PREREQS" ".FEATURES" ".INCLUDE_DIRS" ".RECIPEPREFIX" ".SHELLFLAGS"
    ".VARIABLES" "MAKE" "MAKEARGS" "MAKECMDGOALS" "MAKEFILE_LIST" "MAKEFLAGS" "MAKE_RESTARTS"
    "MAKE_TERMERR" "MAKE_TERMOUT" "MAKE_VERSION" "SHELL"))

(VPATH_assignment
  name: "VPATH" @variable.builtin
  operator: _ @operator)

(RECIPEPREFIX_assignment
  name: ".RECIPEPREFIX" @variable.builtin
  operator: _ @operator)

;; a variable reads the same colour where it is defined and where it is used
(variable_assignment
  name: (word) @string.special.symbol
  operator: _ @operator)

(shell_assignment
  name: (word) @string.special.symbol
  operator: "!=" @operator)

(define_directive
  name: (word) @string.special.symbol)

(define_directive
  operator: _ @operator)

(undefine_directive
  variable: (word) @string.special.symbol)

(export_directive
  variables: (list
    (word) @string.special.symbol))

(unexport_directive
  variables: (list
    (word) @string.special.symbol))

(ifdef_directive
  variable: (word) @string.special.symbol)

(ifndef_directive
  variable: (word) @string.special.symbol)

(variable_reference
  (word) @string.special.symbol)

(substitution_reference
  text: (word) @string.special.symbol)

;; `$@`, `$<`, `$(@D)` … — make's `self`
(automatic_variable) @variable.builtin

;; `$(` … `)` around references and calls
(variable_reference
  [
    "$"
    "$$"
    "("
    ")"
    "{"
    "}"
  ] @operator)

(substitution_reference
  [
    "$"
    "$$"
    "("
    ")"
    "{"
    "}"
    ":"
    "="
  ] @operator)

(function_call
  [
    "$"
    "$$"
    "("
    ")"
  ] @operator)

(function_call
  function: _ @function.builtin)

(shell_function
  [
    "$"
    "$$"
    "("
    ")"
  ] @operator)

(shell_function
  function: "shell" @function.builtin)

;; --- text -----------------------------------------------------------------

(string) @string

;; `$$` and `//`
(escape) @string.escape

;; line continuation
"\\" @punctuation.special
