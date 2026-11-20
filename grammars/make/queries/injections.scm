;; Recipe lines and `!=` / `$(shell …)` commands are the shell's. Make's own
;; references inside them (`$(CC)`, `$@`) are the content node's named
;; children, which an injection leaves to the parent — so they keep make's
;; colours and bash never sees `$(CC)` as a command substitution.

((shell_text) @injection.content
  (#set! injection.language "bash"))

((shell_command) @injection.content
  (#set! injection.language "bash"))
