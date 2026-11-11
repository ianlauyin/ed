# Languages

- Language settings: each language's complete `LanguageSettings` is a
  function in its own module in `crates/languages/src` (e.g. `rust::settings()`;
  Diff, Env, Git Commit and Plain Text have small modules of their own),
  registered with the language. There is no global default to override and no
  merging, so a language's settings are read in one place. Languages that share
  settings call the same function (JSON/JSONC, TypeScript/TSX/JavaScript,
  Markdown/Markdown-Inline, Go Mod/Go Work). Buffers without a
  registered language use `LanguageSettings::fallback()` (in `language`).
  - Per-language fields are only those a language may need to differ in: tab
    size, hard tabs, soft wrap (off or at the editor width), preferred line
    length, wrap guides, formatter (auto or a language server), code actions on
    format, trailing whitespace / final newline on save, LSP vs tree-sitter
    document symbols, rewrap scope (comments or anywhere), on-type formatting,
    auto-indent on paste, and word completions (fallback or disabled). The
    types live in `language::language_settings`; there is no
    `settings_content` crate.
  - Everything else is fixed: inlay hints, indent guides and LSP completion
    options are constants in `language::language_settings`; the rest is inlined
    where used. Files are always formatted on save (save-without-format still
    skips it); there are no external-command formatters or modifications-only
    formatting. Soft-wrapped lines keep the original line's indentation. Indent
    guides use indent-aware colors with no background. Whitespace is drawn
    only inside selections. Auto-indent is syntax-aware. LSP completions
    replace the text after the cursor only when the completion ends with it.
    LSP folding ranges are off. Other always-on behaviour with no setting: comment/list continuation,
    list indent on tab, autoclose and auto-surround, linked edits, completions
    on input and their documentation, word diff, language tasks (LSP tasks
    preferred), and language servers.
  - JSX tag auto-close applies to languages whose `config.toml` has a
    `[jsx_tag_auto_close]` section (JavaScript, TSX); there is no setting.
  - Extra file globs that select a language before path suffixes (`tsconfig*.json`
    → JSONC, `.env.*` → Env, agent rule files → Markdown) are registered with the
    language's settings. `config.toml` carries no settings (tab size, hard tabs,
    soft wrap, auto-indent on paste).
  - There are no per-language task variables, so the Rust run/target-dir and
    Python test-runner overrides are gone.
- Tree-sitter grammars are only bundled for languages edited seriously: Bash,
  CSS, Go, HTML (with injected JS/CSS), JavaScript, JSON/JSONC, Markdown,
  Python, Rust, TSX, TypeScript, YAML. They drive highlighting plus
  structure-aware features (auto-indent, brackets, outline, text objects,
  runnables, string/comment scopes). They are always compiled in (no feature gate).
- One language, one file: everything for a language lives in
  `crates/languages/src`. Its module (`rust.rs`) has a `languages()` function
  returning its `LanguageDef`s: tree-sitter grammar, LSP adapters, task
  context, manifest name, semantic token rules, settings and extra file globs.
  Next to it, its directory (`rust/`) holds `config.toml` and the tree-sitter
  queries, embedded and looked up by directory name. `languages::init` is the
  single place that lists the modules and registers every definition.
  - Modules that cover several languages return them all (Go/Go Mod/Go Work,
    JSON/JSONC, Markdown/Markdown-Inline, Terraform/Terraform Vars,
    TSX/TypeScript/JavaScript). Env uses the Bash grammar, JSONC the JSON
    grammar and JavaScript the TSX grammar.
  - The grammar is passed to the registry with the loaded language. There is
    no grammar-name registry and `config.toml` has no `grammar` key.
- Markdown uses the upstream `tree-sitter-md` crate from crates.io (no fork).
  Outlines use LSP document symbols by default
  (`lsp_document_symbols: true`), but Markdown overrides this to tree-sitter
  (`false`) because there is no Markdown language server; headings make up the
  outline. LSP symbols are fetched per buffer, so in a multibuffer refreshing one
  file never cancels another file's pending fetch.
- Diff, Git Commit, Go Mod, Go Work, Terraform, Terraform Vars and TOML are
  still registered languages (file matching, comments, brackets, gopls for Go
  Mod/Work, terraform-ls for Terraform, tombi for TOML, Diff's
  keep-trailing-whitespace settings, the Git Commit editor)
  but have no grammar, so they get rough highlighting only and none of the
  structure-aware features.
- TOML's language server is `tombi` (`tombi lsp`). It provides formatting
  (the TOML formatter under `"auto"`), lint diagnostics, completions and schema
  validation; configuration comes from the project's `tombi.toml` /
  `[tool.tombi]`.
- Terraform (`.tf`) and Terraform Vars (`.tfvars`) use `terraform-ls`
  (`terraform-ls serve`, from `PATH` only) with LSP language ids `terraform`
  and `terraform-vars`. It provides completions, navigation, references,
  hover, diagnostics, document symbols (the outline) and formatting (the
  formatter under `"auto"`). Other HCL files (`.hcl`, stacks, tests) are not
  registered and stay Plain Text.
- rust-analyzer's check runs `cargo clippy`. Clippy's Cargo-group lints
  (`wildcard_dependencies`, etc.) have no span and are reported at line 1 of
  the clippy config, once per crate and target. rust-analyzer drops
  diagnostics for files outside its VFS, and a virtual workspace root isn't in
  it, so the adapter adds the worktree root's `clippy.toml` / `.clippy.toml`
  (absolute paths) to `vfs.extraIncludes`. Exact duplicate diagnostics in a
  publish are collapsed to one. These warnings show in the diagnostics pane
  under the clippy config, not at the offending `Cargo.toml` line.
- C and C++ are not registered languages (no clangd, comments, brackets or
  per-language settings): their files open as Plain Text and are highlighted
  by the fallback below.
- SQL, Dockerfile, JSDoc, Regex and the hidden Zed Keybind Context
  languages are removed. Their files open as Plain Text and are still
  highlighted by the fallback below.
- A buffer's language comes only from its file (name/extension, first
  line) or a manual pick in the language selector. Untitled buffers
  are never detected from their contents (no ML detection, no
  `language_detection` setting) and stay Plain Text until saved or picked.
- The status bar language button and the language selector follow the buffer
  under the cursor, including in multi-buffer views (diffs, commits), and act
  on the pane shown in the current mode (the history pane in Git History).
- Buffers loaded from disk defer tree-sitter parsing:
  most bulk loads (project search, multi-file diffs) are never displayed, and
  a syntax tree costs many times the file's size. Parsing starts, and then
  stays on, when an editor draws any of the buffer's rows, or when a feature
  reads the tree off-screen (outline items without LSP symbols, a remote task
  context). Off-screen readers wait for that parse to finish before reading.
  Until then the buffer reports its file's language and resolves scopes from
  it. Edits to a still-deferred buffer skip autoindent: it needs the pre-edit
  syntax tree, and a blocking parse to get one isn't worth it for buffers
  nobody looks at. Diff base texts are not deferred this way (see `git.md`).
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
  started with `--stdio` and `provideFormatter: true`.
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
  - Built-in Python tasks (run file/module/selection, pytest) run
    through `uv run python …` from the worktree root.
  - `terminal.detect_venv` (bool, default true): terminals and tasks opened in a
    worktree with a `.venv` get `VIRTUAL_ENV` set and `.venv/bin` prepended to
    `PATH`; no activation script is run.
- Messages to a language server (requests, responses, notifications) are written
  in the order they were sent, through one queue. Notifications are serialized
  lazily but never overtake an earlier request, so e.g. a formatting request
  always sees the document version from the `didChange` sent before it.
- `initialize` sends the workspace as `workspaceFolders` plus `rootUri` (for
  servers that predate workspace folders); the long-deprecated `rootPath` is
  not sent.
- `editor::RestartLanguageServer` restarts the servers relevant to the current
  buffer, including servers previously stopped with `editor::StopLanguageServer`.
- The status-bar LSP menu keeps a stopped server listed as "Stopped" under the
  worktree it last ran in, so "Restart Server" and "Restart All Servers" stay
  reachable after stopping one or all servers.
- JSON schemas are not bundled. json-language-server downloads the SchemaStore
  schemas for `tsconfig*.json` and `package.json` itself on each start (no
  schema completion/validation offline); JSONC files get an inline
  trailing-comma schema. The `vscode/content` schema request is not handled.
- There is no managed Node.js runtime and no `node` settings (`node.path`,
  `node.npm_path` are removed). Node-based tooling uses `node`/`npx` from the
  shell `PATH`.
- Language servers are never installed by ed and there is no setting for a
  server's binary path (`lsp.<server>.binary.path` is removed). A server starts
  from a user installation the adapter finds (`PATH`, or the project). Node-based
  servers (css/html/json, yaml, bash, tombi) then fall back to
  `npx --yes --package <pkg> <bin>`, unpinned: npx prefers a version installed
  in the project, else the latest release from npm's cache, downloading it on
  first use.
- A missing server is not an error: its binary status is "Not Installed"
  (`BinaryStatus::NotInstalled`, raised by `find_binary` returning
  `ServerNotInstalled`). The LSP menu still lists it, greyed out like a stopped
  server and labelled "Not Installed", with "View Message" naming the binary
  it looked for on `PATH`; the activity indicator and the LSP button's
  indicator dot show nothing. Restarting the server after installing it starts
  it. A server is tied to its worktree when a buffer registers with it, before
  it runs, and the LSP button records that even before its menu exists;
  otherwise a server that never starts would have no menu entry. Any other
  startup problem (spawn or initialize failure, tsgo finding
  TypeScript < 7) is still a failure ("Failed to run …", "Error").
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
- `stylelint` (`stylelint-language-server --stdio`, from
  `@stylelint/language-server`) also runs for CSS, after the CSS server and
  Biome. It provides lint diagnostics and code actions
  (`source.fixAll.stylelint`) using the project's own `stylelint` and config; in
  projects without `stylelint` installed it reports nothing. It resolves from the
  nearest `node_modules/.bin` at or above the worktree root, then `PATH`, then
  npx (the server needs Node ≥ 22.17). Formatting stays on Biome.
- There are no `lsp.<server>` settings. Each adapter hardcodes its server's
  binary arguments, initialization options and workspace configuration
  (rust-analyzer: nightly rustfmt, clippy check, all cargo features). A
  settings change never restarts a server, since nothing that settings control
  feeds into a server's identity. yaml/json don't resolve relative schema paths
  against the worktree.
- YAML files under a `kube/` directory (`**/kube/**/*.yaml`, `*.yml`) use
  yaml-language-server's built-in `kubernetes` schema for completion, hover
  and validation. Core Kubernetes types only; a
  `# yaml-language-server: $schema=<url>` modeline covers other files and CRDs.
- Server binary statuses no longer include checking-for-update or downloading.
- No Vue.js support: no grammar, language server, `tsserver/request` bridge,
  default settings or `.vue` file icon. `.vue` files open as Plain Text.
- No Tailwind CSS or Emmet support: the `tailwindcss-language-server` and
  `emmet-language-server` adapters, Emmet abbreviation actions and the
  editor's inline input block are removed.
- No LSP call hierarchy: no Show Incoming/Outgoing Calls actions, menu items or
  picker. The client does not advertise `callHierarchy` capability, so servers
  never offer it.
- Grammar configs cannot scope language servers to syntax regions
  (`scope_opt_in_language_servers` / `opt_into_language_servers` removed).
- Which servers run for a language, and their order, is decided in code only:
  every adapter registered for the language runs, in registration order (the
  first is the primary server). The `languages.<lang>.language_servers` setting
  (`"!name"` / `"..."` syntax) is removed, as are "available" adapters that only
  start when named in settings. There is no `enable_language_server` setting.
- No direnv integration (`load_direnv` removed). A project's environment is the
  login shell's environment captured after `cd` into the worktree, so shell
  hooks (direnv, mise, asdf) still apply if configured in the user's shell.
- Yarn Plug'n'Play is not supported. LSP locations inside `.zip` archives or Yarn
  `__virtual__` paths are not resolved, and tsgo only uses
  `node_modules/typescript` (never `.yarn/sdks`).
  Yarn remains usable as a package manager / task runner (`yarn.lock` detection).
- Prettier is not a server, plugin host or setting: no Prettier server, default
  Prettier install, plugins, `prettier` language setting, or `"prettier"` formatter
  value. Built-in formatting for web languages uses the Biome or Prettier CLI:
  - `formatter: "auto"` formats CSS, GraphQL, JavaScript, JSON, JSONC, TSX and
    TypeScript with the CLI formatter the project configures: the nearest
    ancestor of the file holding a Biome config (`biome.json(c)`) or a Prettier
    config (`.prettierrc*`, `prettier.config.*`, or a `prettier` key in
    `package.json`) decides, Biome winning a tie. With neither, no CLI formatter
    runs and `"auto"` uses the primary language server like other languages. The
    language list is hardcoded with no setting.
  - Prettier resolves in order: nearest `node_modules/.bin/prettier` above the
    file, `prettier` on the shell `PATH`, then `npx --yes prettier`. Each format
    runs `prettier --stdin-filepath <path>` from the nearest ancestor holding a
    Prettier config (else the file's directory); unsaved buffers use a synthetic
    `untitled.<ext>` path. Failures are logged, not shown.
  - Biome resolves in order: nearest `node_modules/.bin/biome` above the file,
    `biome` on the shell `PATH`, then `npx --yes @biomejs/biome`.
  - Each format runs `biome format --stdin-file-path <path>` once, from the
    nearest ancestor directory holding `biome.json`/`biome.jsonc` (else the file's
    directory), so the project's config applies even to Biome 1.x, which only
    looks in the working directory. Unsaved buffers use
    `untitled.<ext>`. Editor settings such as `tab_size` are not passed to Biome.
  - Format Selections formats the whole buffer and keeps only edits that overlap
    the selections.
  - As with other `"auto"` formatters, Biome and Prettier failures are logged and not shown to the user.
- External library sources (e.g. dependency crates) open as library worktrees,
  see "External libraries" in `project-panel.md`. Each language recognizes its
  own library paths through a `LibraryProvider` registered by the `languages`
  crate. Rust recognizes, by directory layout only (any `CARGO_HOME` /
  `RUSTUP_HOME`):
  - `registry/src/<index>/<name>-<version>/`, named `<name>-<version>`
  - `git/checkouts/<repo>-<hash>/<rev>/`, named `<repo>@<rev>`
  - `lib/rustlib/src/rust/library/<crate>/` (std sources), named `<crate>`
