# Languages

- Tree-sitter grammars are only bundled for languages edited seriously: Bash,
  CSS, Go, HTML (with injected JS/CSS), JavaScript, JSON/JSONC, Markdown,
  Python, Rust, TSX, TypeScript, YAML. They drive highlighting plus
  structure-aware features (auto-indent, brackets, outline, text objects,
  runnables, string/comment scopes).
- The One Dark theme gives each code kind its own colour
  or style, so they never fall back to plain text or share a colour: variables,
  parameters (italic), properties, function calls, method calls, macros
  (`function.special`), declaration keywords (italic),
  control-flow keywords (`keyword.control`, upright), types, operators,
  strings, numbers, booleans and constants. Builtin constants (`null`, CSS
  values like `flex`) share the boolean colour.
  - Rust, JavaScript, TypeScript and TSX tag function/method definitions
    `@function.definition`, separately from calls.
  - CSS: selectors, properties, `--custom-properties` (variable), functions,
    values (`constant.builtin`), numbers and units each differ.
  - JSON/JSONC and TOML: keys use the property colour; values are coloured by
    kind; TOML table headers use the type colour. Fallback TOML highlighting and
    tombi's semantic tokens (`key`, `table`, `boolean`, date/time) map to the
    same captures.
  - Default semantic-token rules map `function`/`method` tokens with a
    `declaration`/`definition` modifier to `function.definition`, and
    `keyword` + `controlFlow` to `keyword.control`, so language servers keep
    the tree-sitter distinction.
- Markdown uses the upstream `tree-sitter-md` crate from crates.io (no fork).
  Outlines and breadcrumbs use LSP document symbols by default
  (`document_symbols: "on"`), but Markdown overrides this to tree-sitter
  (`"off"`) because there is no Markdown language server; headings make up the
  outline.
- C, C++, Diff, Git Commit, Go Mod, Go Work and TOML are still registered
  languages (file matching, comments, brackets, gopls for Go Mod/Work, tombi
  for TOML, Diff's keep-trailing-whitespace settings, the Git Commit editor)
  but have no grammar, so they get rough highlighting only and none of the
  structure-aware features.
- TOML's language server is `tombi` (`tombi lsp`). It provides formatting
  (the TOML formatter under `"auto"`), lint diagnostics, completions and schema
  validation; configuration comes from the project's `tombi.toml` /
  `[tool.tombi]`.
- C/C++ have no built-in language server: clangd support (adapter, semantic
  token rules, inactive regions, switch source/header) is removed.
- SQL, Dockerfile, JSDoc, Regex and the hidden Zed Keybind Context
  languages are removed. Their files open as Plain Text and are still
  highlighted by the fallback below.
- A buffer's language comes only from its file (name/extension, first
  line, modeline) or a manual pick in the language selector. Untitled buffers
  are never detected from their contents (no ML detection, no
  `language_detection` setting) and stay Plain Text until saved or picked.
- Fallback highlighting: any buffer whose language has no tree-sitter grammar
  (including plain text) is highlighted with the Sublime/TextMate syntaxes
  bundled by `two-face` (syntect, Oniguruma regex). The syntax is picked by
  file name/extension, then language name, then the first line (shebang).
  A single file opened on its own (the file is its own worktree root) is
  detected by its file name like any other file.
  TextMate scopes are mapped onto the theme's syntax captures (keyword,
  string, comment, ...); accuracy is "good enough", not tree-sitter quality.
  - A few bundled syntaxes are replaced by patched copies vendored in
    `crates/language/src/fallback_syntaxes`, loaded as a small separate set
    that is looked up first (rebuilding the bundled set is too slow). TOML is
    patched for TOML 1.1 inline tables: spanning lines, comments and a
    trailing comma, as tombi formats them.
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
  - LSP semantic tokens layer on top as usual.
- HTML uses `vscode-html-language-server` (from `vscode-langservers-extracted`),
  started with `--stdio` and `provideFormatter: true`; `lsp.vscode-html-language-server.settings`
  is passed through as workspace configuration.
- Jupyter notebooks and integrated REPL execution are removed, including kernel
  management, remote kernel launching, execution controls, settings, and shortcuts.
- Python source editing remains supported. `.ipynb` files open as ordinary text.
- Python tooling is uv-only:
  - The only Python language servers are `ty` (types, completions) and `ruff`
    (lint, format, organize imports); both run by default and `ruff` is the
    formatter. pyright, basedpyright and pylsp are removed.
  - Each resolves from the worktree's `.venv/bin`, then `PATH`, then
    `uvx <tool> server` (latest release from uv's cache).
  - There is no toolchain/virtual-environment discovery, selector, status bar
    item or persisted toolchain. The only environment ed knows is `.venv` at
    the worktree root; ty finds it on its own.
  - Built-in Python tasks (run file/module/selection, pytest/unittest) run
    through `uv run python …` from the worktree root.
  - `terminal.detect_venv` (bool, default true): terminals and tasks opened in a
    worktree with a `.venv` get `VIRTUAL_ENV` set and `.venv/bin` prepended to
    `PATH`; no activation script is run.
- Messages to a language server (requests, responses, notifications) are written
  in the order they were sent, through one queue. Notifications are serialized
  lazily but never overtake an earlier request, so e.g. a formatting request
  always sees the document version from the `didChange` sent before it.
- `editor::RestartLanguageServer` restarts the servers relevant to the current
  buffer, including servers previously stopped with `editor::StopLanguageServer`.
- The status-bar LSP menu keeps a stopped server listed as "Stopped" under the
  worktree it last ran in, so "Restart Server" and "Restart All Servers" stay
  reachable after stopping one or all servers.
- Debug builds trust every worktree (as with `session.trust_all_worktrees`), so
  dev and `edctl` launches never stop at the worktree trust prompt.
- JSON language-server schema-content requests resolve the last supplied URI;
  an empty URI list returns an error.
- There is no managed Node.js runtime and no `node` settings (`node.path`,
  `node.npm_path` are removed). Node-based tooling uses `node`/`npx` from the
  shell `PATH`, locally and on remote servers.
- Language servers are never installed by ed and there is no setting for a
  server's binary path (`lsp.<server>.binary.path` is removed). A server starts
  from a user installation the adapter finds (`PATH`, or the project). Node-based
  servers (css/html/json, yaml, bash, tombi) then fall back to
  `npx --yes --package <pkg> <bin>`, unpinned: npx prefers a version installed
  in the project, else the latest release from npm's cache, downloading it on
  first use. A missing server is reported through the existing failed-server
  status ("Failed to run …" in the activity indicator, "Error" in the LSP menu)
  naming the server and the binary it looked for on `PATH`.
- `lsp.<server>.fetch` and `lsp.<server>.binary.ignore_system_version` are removed;
  `binary.arguments` and `binary.env` still apply to whichever binary is used.
- JavaScript, TypeScript and TSX run two servers, `tsgo` and `biome`. vtsls,
  typescript-language-server and ESLint are removed.
  - `tsgo` is the TypeScript 7 native language server (`tsc --lsp --stdio`).
    It provides completions, navigation, rename, hover, inlay hints and type
    errors. Only TypeScript 7+ is supported: the nearest
    `node_modules/typescript` at or above the worktree root is used when its
    major version is ≥ 7, and the server refuses to start (naming the found
    version) when it is older. Projects without `typescript` run
    `npx --yes --package typescript@^7 tsc --lsp --stdio`. `tsc` on `PATH` is
    never used, since it is commonly an old global install.
  - JavaScript gets the same tsgo features. Type errors only show where the
    project opts in (`checkJs` / `// @ts-check`).
- `biome` (`biome lsp-proxy`) runs for JavaScript, TypeScript, TSX, JSON, JSONC
  and CSS (alongside the JSON and CSS servers for those). It provides lint
  diagnostics and code actions (`source.fixAll.biome`,
  `source.organizeImports.biome`). It resolves like the Biome formatter, from the
  worktree root. Formatting stays on the Biome CLI (below).
- `lsp.tsgo.settings` / `lsp.biome.settings` are passed through as workspace
  configuration.
- `stylelint` (`stylelint-language-server --stdio`, from
  `@stylelint/language-server`) also runs for CSS, after the CSS server and
  Biome. It provides lint diagnostics and code actions
  (`source.fixAll.stylelint`) using the project's own `stylelint` and config; in
  projects without `stylelint` installed it reports nothing. It resolves from the
  nearest `node_modules/.bin` at or above the worktree root, then `PATH`, then
  npx (the server needs Node ≥ 22.17). `lsp.stylelint.settings` is sent as the
  server's `stylelint` configuration section (e.g. `validate`, `configFile`).
  Formatting stays on Biome.
- Server binary statuses no longer include checking-for-update or downloading.
- No Vue.js support: no grammar, language server, `tsserver/request` bridge,
  default settings or `.vue` file icon. `.vue` files open as Plain Text.
- No Tailwind CSS or Emmet support: the `tailwindcss-language-server` and
  `emmet-language-server` adapters, Emmet abbreviation actions and the
  editor's inline input block are removed.
- Grammar configs cannot scope language servers to syntax regions
  (`scope_opt_in_language_servers` / `opt_into_language_servers` removed).
- Which servers run for a language, and their order, is decided in code only:
  every adapter registered for the language runs, in registration order (the
  first is the primary server). The `languages.<lang>.language_servers` setting
  (`"!name"` / `"..."` syntax) is removed, as are "available" adapters that only
  start when named in settings. `enable_language_server: false` still turns all
  servers off for a language.
- No direnv integration (`load_direnv` removed). A project's environment is the
  login shell's environment captured after `cd` into the worktree, so shell
  hooks (direnv, mise, asdf) still apply if configured in the user's shell.
- Yarn Plug'n'Play is not supported. LSP locations inside `.zip` archives or Yarn
  `__virtual__` paths are not resolved, and tsgo only uses
  `node_modules/typescript` (never `.yarn/sdks`).
  Yarn remains usable as a package manager / task runner (`yarn.lock` detection).
- Prettier is removed (no Prettier server, default Prettier install, plugins,
  `prettier` language setting, or `"prettier"` formatter value). Built-in formatting
  for web languages uses Biome instead:
  - `formatter: "auto"` formats CSS, GraphQL, JavaScript, JSON, JSONC, TSX and
    TypeScript with Biome; the list is hardcoded with no setting. Other languages
    under `"auto"` use the primary language server.
  - Biome resolves in order: nearest `node_modules/.bin/biome` above the file,
    `biome` on the shell `PATH`, then `npx --yes @biomejs/biome`.
  - Each format runs `biome format --stdin-file-path <path>` once, from the
    nearest ancestor directory holding `biome.json`/`biome.jsonc` (else the file's
    directory), so the project's config applies even to Biome 1.x, which only
    looks in the working directory. Unsaved buffers use
    `untitled.<ext>`. Editor settings such as `tab_size` are not passed to Biome.
  - Format Selections formats the whole buffer and keeps only edits that overlap
    the selections.
  - As with other `"auto"` formatters, Biome failures are logged and not shown to the user.
- External library sources (e.g. dependency crates) open as library worktrees,
  see "External libraries" in `project-panel.md`. Each language recognizes its
  own library paths through a `LibraryProvider` registered by the `languages`
  crate. Rust recognizes, by directory layout only (any `CARGO_HOME` /
  `RUSTUP_HOME`):
  - `registry/src/<index>/<name>-<version>/`, named `<name>-<version>`
  - `git/checkouts/<repo>-<hash>/<rev>/`, named `<repo>@<rev>`
  - `lib/rustlib/src/rust/library/<crate>/` (std sources), named `<crate>`
