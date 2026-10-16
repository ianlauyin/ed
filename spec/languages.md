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
- Node-based tooling requires system Node.js 22 or newer and npm, discovered
  on the shell's `PATH` or selected through `node.path` and `node.npm_path`.
  Both local and remote runtimes report missing or invalid installations and never
  download or fall back to a managed Node.js runtime. Failed discovery can be retried
  after fixing the installation; changes to configured paths invalidate discovery.
- `node.ignore_system_version` is removed. npm package installation, custom CA
  certificates, and user npm configuration remain supported.
- The Node runtime does not accept or forward editor proxy settings to npm.
  Network routing can use a system-wide TUN proxy when needed.
- Language servers are never downloaded or installed by Zed. A server starts only
  from `lsp.<server>.binary.path` or from a user installation the adapter finds
  (`PATH`, project `node_modules`, or the active toolchain). A missing server is
  reported through the existing failed-server status ("Failed to run …" in the
  activity indicator, "Error" in the LSP menu) naming the server and the setting.
- `lsp.<server>.fetch` and `lsp.<server>.binary.ignore_system_version` are removed;
  `binary.arguments` and `binary.env` still apply to whichever binary is used.
- Server binary statuses no longer include checking-for-update or downloading.
