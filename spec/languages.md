# Languages

- Dockerfile support is built in and uses native Tree-sitter syntax highlighting.
- Files named `Dockerfile`, `Containerfile`, or `dockerfile`, and files with those
  extensions, are recognized automatically.
- Shell commands and `RUN` heredoc bodies use Bash syntax highlighting.
- Dockerfile support does not register a language server or require external tools.
- Jupyter notebooks and integrated REPL execution are removed, including kernel
  management, remote kernel launching, execution controls, settings, and shortcuts.
- Python source editing remains supported. `.ipynb` files open as ordinary text.
- `editor::RestartLanguageServer` restarts the servers relevant to the current
  buffer, including servers previously stopped with `editor::StopLanguageServer`.
