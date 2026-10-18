# Languages

- Tree-sitter grammars are only bundled for languages edited seriously: Bash,
  CSS, Go, HTML (with injected JS/CSS), JavaScript, JSON/JSONC, Markdown,
  Python, Rust, TSX, TypeScript, YAML. They drive highlighting plus
  structure-aware features (auto-indent, brackets, outline, text objects,
  runnables, string/comment scopes).
- C, C++, Diff, Git Commit, Go Mod and Go Work are still registered languages
  (file matching, comments, brackets, language servers such as clangd and
  gopls, Diff's keep-trailing-whitespace settings, the Git Commit editor) but
  have no grammar, so they get rough highlighting only and none of the
  structure-aware features.
- TOML, SQL, Dockerfile, JSDoc, Regex and the hidden Zed Keybind Context
  languages are removed. Their files open as Plain Text and are still
  highlighted by the fallback below.
- Fallback highlighting: any buffer whose language has no tree-sitter grammar
  (including plain text) is highlighted with the Sublime/TextMate syntaxes
  bundled by `two-face` (syntect, Oniguruma regex). The syntax is picked by
  file name/extension, then language name, then the first line (shebang).
  TextMate scopes are mapped onto the theme's syntax captures (keyword,
  string, comment, ...); accuracy is "good enough", not tree-sitter quality.
  - Tokenization is lazy (only up to the rows being shown) and incremental:
    an edit re-tokenizes from the nearest checkpoint until the parser state
    converges with the state recorded before the edit.
  - The status bar language button shows the fallback syntax name (e.g.
    "TOML") when a buffer has no language or is Plain Text.
  - Buffers over 1 MiB and lines over 4096 bytes are not highlighted.
  - Injections (e.g. Markdown code blocks) whose language is unknown or has no
    grammar (```sql, ```c, ```toml, ...) are highlighted the same way, with the
    syntax looked up by the injection's language name. Each injection is
    tokenized from its start, independently of the rest of the buffer;
    injections over 64 KiB are not highlighted.
  - Search query inputs (buffer search, project search, text finder) are
    highlighted with the bundled "Regular Expression" syntax while regex mode
    is on, and stay unhighlighted otherwise.
  - It obeys the same switch as tree-sitter highlighting
    (`semantic_tokens: "full"` turns it off), and LSP semantic tokens layer on
    top as usual.
- HTML uses `vscode-html-language-server` (from `vscode-langservers-extracted`),
  started with `--stdio` and `provideFormatter: true`; `lsp.vscode-html-language-server.settings`
  is passed through as workspace configuration.
- Jupyter notebooks and integrated REPL execution are removed, including kernel
  management, remote kernel launching, execution controls, settings, and shortcuts.
- Python source editing remains supported. `.ipynb` files open as ordinary text.
- `editor::RestartLanguageServer` restarts the servers relevant to the current
  buffer, including servers previously stopped with `editor::StopLanguageServer`.
- There is no managed Node.js runtime and no `node` settings (`node.path`,
  `node.npm_path` are removed). Node-based tooling uses `node`/`npx` from the
  shell `PATH`, locally and on remote servers.
- Language servers are never installed by Zed. A server starts from
  `lsp.<server>.binary.path`, else from a user installation the adapter finds
  (`PATH` or the active toolchain). Node-based servers (vtsls,
  typescript-language-server, eslint, css/html/json, tailwindcss, yaml, bash,
  pyright, basedpyright) then fall back to `npx --yes --package <pkg> <bin>`,
  unpinned: npx prefers a version installed in the project, else the latest
  release from npm's cache, downloading it on first use. typescript-language-server
  still needs `typescript` installed in the workspace. A missing server is
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
