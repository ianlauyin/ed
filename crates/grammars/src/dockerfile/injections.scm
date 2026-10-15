((shell_command) @injection.content
  (#set! injection.language "bash"))

((run_instruction
  (heredoc_block
    (heredoc_content) @injection.content))
  (#set! injection.language "bash"))
