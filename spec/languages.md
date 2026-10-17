# Languages

- Dockerfile support is built in and uses native Tree-sitter syntax highlighting.
- Files named `Dockerfile`, `Containerfile`, or `dockerfile`, and files with those
  extensions, are recognized automatically.
- Shell commands and `RUN` heredoc bodies use Bash syntax highlighting.
- Dockerfile support does not register a language server or require external tools.
- HTML support is built in (ported from upstream's `extensions/html`): native
  Tree-sitter grammar for `.html`, `.htm`, `.shtml`, with JavaScript/CSS injected
  into `<script>`, `<style>`, `style=` and `on*=` attributes.
- HTML uses `vscode-html-language-server` (from `vscode-langservers-extracted`),
  started with `--stdio` and `provideFormatter: true`; `lsp.vscode-html-language-server.settings`
  is passed through as workspace configuration.
- TOML support is built in (ported from `zed-extensions/toml`): native Tree-sitter
  grammar (`tree-sitter-toml-ng`, the maintained fork) for `.toml`, `Cargo.lock`,
  `Pipfile`, `uv.lock`. No language server is registered.
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
- Yarn Plug'n'Play is not supported. LSP locations inside `.zip` archives or Yarn
  `__virtual__` paths are not resolved, and the TypeScript servers only use
  `node_modules/typescript/lib` as the TypeScript SDK (never `.yarn/sdks`).
  Yarn remains usable as a package manager / task runner (`yarn.lock` detection).
- Prettier is removed (no Prettier server, default Prettier install, plugins,
  `prettier` language setting, or `"prettier"` formatter value). Built-in formatting
  for web languages uses Biome instead:
  - `formatter: "auto"` formats CSS, GraphQL, JavaScript, JSON, JSONC, TSX and
    TypeScript with Biome; the list is hardcoded with no setting. Other languages
    under `"auto"` use the primary language server.
  - Biome resolves in order: nearest `node_modules/.bin/biome` above the file,
    `biome` on the shell `PATH`, then `npx --yes @biomejs/biome`.
  - Each format runs `biome format --stdin-file-path <path>` once, from the file's
    directory, so the project's `biome.json` applies. Unsaved buffers use
    `untitled.<ext>`. Editor settings such as `tab_size` are not passed to Biome.
  - Format Selections formats the whole buffer and keeps only edits that overlap
    the selections.
  - As with other `"auto"` formatters, Biome failures are logged and not shown to the user.
