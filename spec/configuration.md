# Configuration

## Decision

Settings are hardcoded in Rust: each remaining `Settings` type (worktree,
terminal) builds its own value in `Settings::built_in()`, shared by every
project. There is no `SettingsContent`/`*SettingsContent` intermediate, no
default-settings JSON, and no separate user/default settings layer. Language settings are not part of it: each
language's complete settings are hardcoded next to its language support (see
"Language settings" in `languages.md`). Keymaps and snippets are
also constructed in Rust, including the personal bindings and snippets.
Changing persistent settings, shortcuts, or snippets requires editing code and
rebuilding. There is no settings UI, keymap editor, snippet UI, settings/keymap
migrator, or Vim/Helix mode.

## Behaviour

- There is no database (no SQLite, key-value store, or `db`/`sqlez`/`session`
  crates). App state lives in two JSON files:
  - `~/.local/share/ed/projects.json`: recent projects (local root folders in open
    order), most recent first and
    capped at 100, each flagged `open` while it has a window (the open list
    is the recents, not a separate list). Window bounds are not saved. It is
    read once at launch; unreadable contents count as empty. Every change
    rewrites the whole file. A project moves to the front of the recents when
    opened or its window is focused.
    There is no Remote Servers modal.
  - `<first root folder>/.ed/workspace.json`, per project: the pane tree with
    each tab's kind and its own JSON state, pinned/active/preview tabs, which
    pane is zoomed, dock visibility/active panel/zoom, recent navigation
    history, and the window's border colour (`border_color`, an index into the theme's
    accent colours). A restored zoomed pane comes back zoomed and active.
    Unreadable files are ignored.
    Identical contents aren't rewritten. Windows with no root folder aren't
    persisted.
    The one hand-edited section is `lint`: `"on"` / `"off"` switches for the
    linters, `clippy` (default on; off runs plain `cargo check` in
    rust-analyzer), `stylelint` (on) and `biome` (off). A server
    that is off isn't started and shows as not installed in the LSP menu.
    Switches are read from disk when a server starts, including the first
    start on project open (before the worktree scan sees `.ed/`). Saving the root
    `.ed/workspace.json` in the editor restarts rust-analyzer, stylelint and
    biome (even if `lint` didn't change), so a switch applies at once; an edit
    made outside the editor still needs a manual restart. The
    app keeps `lint` when it rewrites the file and adds any missing switch
    with its default, so every switch is always listed; it only reads `lint`
    from each worktree's own root.
- Tabs persist only their own state. An editor tab on a file keeps only its
  path: on restore the file is read from disk, and nothing else (scroll,
  selections, folds, unsaved edits) is kept, so editing or scrolling a file
  doesn't rewrite `workspace.json`. Untitled buffers aren't persisted at all.
  Image and markdown-preview tabs keep their path (and preview mode), branch
  diffs their base, and other git diff tabs nothing. Tabs on files inside a
  root folder's `.ed/` aren't persisted at all: it's project config, and
  recording a tab on `workspace.json` would rewrite the file it shows.
- No hot exit: unsaved edits are never persisted. Closing a window (with or
  without a root folder) or quitting prompts to save its dirty buffers,
  untitled ones included.
- Restore on startup (not configurable) reopens the projects flagged open at
  quit (closing a window clears its flag; quitting keeps it; removing a
  project from the recents drops it), most recent on top, falling back to the
  most recent project whose folders still exist. If nothing is restored, a
  window opens with an empty buffer.
- Autosave (not configurable) saves a buffer when focus leaves it and when the
  window deactivates; closing a dirty, autosavable buffer
  saves it instead of prompting. Reopening a file in the same pane restores
  its selections, folds and scroll position.
- Not persisted: panel sizes, window bounds (every new window, restored
  ones included, is created filling the visible area of its display so all
  windows match; not zoomed, since macOS zooms a not-yet-shown window against
  the wrong screen. The display is ed's frontmost window's, which works even
  when ed isn't the active app, else the screen the user is on (gpui
  `App::main_display`: on macOS the screen under the mouse cursor, since at
  launch `NSScreen.mainScreen` is the menu bar's screen, i.e. the primary
  display unless displays have separate Spaces)), dock layout for new
  windows, native (macOS) window state, the Text Finder's last query and
  filters, git panel commit drafts/signoff/commit-editor collapse, and the registry of git worktrees ed created.
- There is no restricted mode or worktree trust: every opened project is
  trusted, so its project settings apply and its language servers start
  without a prompt.
- Single-instance checks are bypassed only in debug builds.
- Settings types use ordinary Serde derives without optional-field macros or
  custom flattened-settings parsing. Ordinary optional fields reject invalid
  values and remain `None` when missing. Settings JSON error recovery and
  field-path diagnostics are removed. Bundled semantic-token rules use ordinary
  Serde errors; callers identify the source file when reporting failures.
- There is one settings layer and no merging anywhere, so settings types have
  no merge trait or derive macros (`settings_macros` is removed). Lists and maps
  are plain values; there is no `"..."` splicing or extend-only list type.
  There is no `settings` crate, store, or registry: each remaining settings type
  (`WorktreeSettings`, `TerminalSettings`) exposes `Type::get()`, a lazily
  built `&'static` value. Settings never change at runtime, so nothing
  observes settings changes (no `SettingsStore` global, no `settings_changed`
  handlers).
- User, global, server, and `.ed/settings.json` files are not loaded, watched,
  created, or rewritten. There are no settings profiles or project overrides.
  Changing persistent preferences requires editing the built-in settings and
  rebuilding. Existing settings controls affect only the running session.
- Built-in preferences preserve the applicable values from the personal Zed
  configuration, including per-language values and Rust language-server options.
  Entries for removed features are omitted. Preferences that never varied are
  removed as settings and hardcoded:
  - title bar: no expanded menu bar, no open-on-hover menus; the project
    name, worktree and branch pickers, and the branch change-status icon are
    always shown; there are no title bar settings;
  - terminal: no title breadcrumbs, no count badge, no bell sound (the tab
    still marks the bell), does not open on startup, always docked at the
    bottom (no `dock` or `default_width` setting); always uses the buffer
    font family, fallbacks and size with normal weight and ligatures off (no
    terminal font settings); line height is 1.3; the cursor always blinks
    (programs cannot turn blinking off); alternate scroll is always on; new
    shells start in the active project directory, then the first project,
    then home (no `working_directory`, `blinking`, `alternate_scroll` or
    `line_height` setting);
  - worktree: symlinked directories are scanned only once expanded (no
    `scan_symlinks` setting);
  - git panel: no file icons, no count badge, does not open on startup;
    there are no git panel settings (see `git.md`);
  - git: there are no `git` settings (see `git.md`);
  - project panel: folder labels are not bold; there are no project panel
    settings (see `project-panel.md`);
  - status bar: no active file name item; the cursor position item always
    uses the long selection format (`2 selections, 3 lines, 12 characters`);
  - file finder: file icons, width `window − 768px` (at least the default
    modal width), the active file is skipped as the first history match unless
    the query has a position, ignored files follow the worktree unless toggled,
    and the query is pre-filled from the focused selection; file icons also
    show in the open-path prompt and language selector;
  - tab switcher: previews the selected tab while cycling;
  - diagnostics: status bar button shown; LSP pull diagnostics on with a 50ms
    debounce; inline diagnostics show errors only, 150ms debounce, 4 columns
    after the line end;
  - image viewer: file sizes in binary units (KiB, MiB);
  - mouse cursor hides on typing and on keystrokes that resolve to an action;
  - workspace: a panel's focus toggle never closes the panel; the window title
    is fixed to `<project names> — <active file name>` (no title template, so
    no settings or Git branch subscriptions drive title updates);
  - language servers: there are no global LSP settings; requests time out
    after 120s (`lsp::LSP_REQUEST_TIMEOUT`), buffers with a line over 20000
    characters skip language servers, prompts without actions auto-dismiss
    after 5s, and the status bar LSP button is always shown;
  - no accessible mode.
- Markdown preview preferences are hardcoded too: UI font family and size for
  text, buffer font family for code, semibold (600) headings, content centered
  and capped at 1200px, and no auto-opening of markdown files in the preview.
  Preview zoom (cmd +/-/0) is in-memory only.
- Fonts and font sizes are not settings; see "Fonts" in `ui-layout.md`. There
  are no theme settings: buffer line height (1.618) and unnecessary-code fade
  (0.3) are constants in `theme::built_in`.
- Semantic token rules are per language only, with no fallback rules: each
  language's `semantic_token_rules.json` (Rust, Go, Python) is complete and is
  registered with the language's settings in `AllLanguageSettings`. TypeScript,
  TSX and JavaScript share one deliberately partial file: only `class`, `enum`
  and `enumMember` are mapped, since those are the tokens where tsgo beats
  tree-sitter (identifiers in value position) and the built-in theme has a
  distinct colour; token types without a rule produce no highlight. A language
  without rules gets no semantic highlighting (tree-sitter only), and editors
  don't request semantic tokens for it. Earlier rules take precedence; style names are tried in
  order. The Rust file is written against the built-in theme: one theme key per
  token type rust-analyzer emits by default, no fallback chains, and no rules
  for specialised punctuation/operator tokens (not enabled). Enums and
  all other types share `type`; enum variants use `variant` (they sit in value
  and pattern position, so they read as constructors, not types); modules are
  `variable`; lifetimes and
  labels use `lifetime`; format specifiers use `string.escape`. Three rules
  carry what only the server knows: `unsafe` is orange on any token type (a
  modifier-only rule, kept first); `mutable` bindings (variables, parameters,
  `self`, statics) are underlined, but not `&mut self` methods or compound
  assignments; and `?` (`operator` + `controlFlow`) is coloured as a keyword. There is no
  default-rules viewer.
- Settings-file actions, shortcuts, menu entries, and welcome/error links are
  removed, along with the project/server settings commands and profile selector.
  The default-settings view is removed too.
- User and bundled keymap JSON files are removed from the loading path. There is
  no keymap watcher, file editor/viewer action, base-keymap selection, or keymap
  JSON schema. Task files are not supported (see `tasks.md`).
- EditorConfig and file modelines (vim/emacs `vim:`/`-*-` comments) are not
  supported, so language settings come only from code. There is no
  project-local settings layer.
  Settings lookups are global only (`Type::get()`); there is no
  per-file/worktree location argument.
- The built-in keymap lives in `ed` (`keymap.rs`) and is written
  directly as `KeyBinding`s, with no intermediate spec type: each group sets a
  context, then binds keystrokes to action names (optionally with JSON input).
  There are no override or disabled bindings: a deeper context wins, and at the
  same depth the later binding wins, so each key is bound once per context. Invalid actions, contexts, or keystrokes
  panic at startup and fail the keymap unit tests.
- The built-in keymap retains contexts and binding order. Only the US/ABC QWERTY
  layout is supported: macOS has no per-layout key-equivalent remapping, so
  shortcuts always bind to the literal key. The keymap, menu bar, and dock menu
  are built once at startup.
- Keybindings should be simple single keystrokes. There is no UI for pending
  multi-stroke input (no which-key popup or status bar indicator).
- Personal bindings sit in their normal context groups: global `cmd-[`/`cmd-]`
  navigate backward/forward (Editor `cmd-[`/`cmd-]` still outdent/indent),
  Editor `f1` opens documentation and `cmd-shift-u` toggles case, and Terminal
  `shift-enter` sends Escape followed by carriage return.
- JSON schemas are not generated: nothing derives `JsonSchema` and `schemars`
  is not a dependency. Actions still derive `Deserialize`, because the built-in
  keymap builds actions from a name plus JSON input. Bindings may omit input,
  so an action whose fields have non-`false` defaults uses struct-level
  `#[serde(default)]` with a hand-written `Default`; `Default::default()` and
  deserializing `{}` give the same action. Built-in data that is never omitted
  (settings, task templates, language `config.toml`) states every such field
  explicitly instead of relying on serde defaults. The JSON language server
  only gets the SchemaStore `tsconfig.json`/`package.json` schema URLs and a
  trailing-comma schema for JSONC files (see `languages.md`); there are no settings, keymap, or
  per-action schemas, and no language-server configuration schema probing.
- The upstream documentation website preprocessor is removed; this fork does not
  build the upstream mdBook site.
- The command palette has no "Add/Change Keybinding" flow; secondary confirm runs
  the selected command. It lists every available action as-is: there is no
  global action filter or query interceptor (`command_palette_hooks` is removed).
- The command palette keeps no persisted state: commands are listed
  alphabetically with no usage-based ranking, there is no query history
  (up/down only moves the selection), and no `command_palette` setting.
- Pickers keep no persisted state: every picker opens at its fixed size.
  `PickerDelegate` has no `name()` key.
- Terminal-panel (dock) terminals keep no persisted state: they and their
  splits are not restored after a restart, and the panel opens a fresh shell
  when it is first shown empty.
- Terminal mode's layout is saved in `.ed/workspace.json` (`terminal`): its
  splits, which pane is zoomed (restored zoomed and active) and, per tab, the
  shell's working directory. Shells start afresh there, without output,
  processes or custom titles. Restoring is lazy: nothing starts until terminal
  mode is first shown in the window, and until then the saved layout is
  written back unchanged. A directory that no longer exists falls back to the
  default. With no terminals left, nothing is saved.
- App-level actions (quit, about, font size, `NoAction`/`Unbind`, ...)
  use the `ed` namespace (`ed::Quit`, shown as "ed: quit"), not upstream's
  `zed`. Keymaps using `zed::*` names do not resolve.
- Actions resolve only by their current names. Deprecated aliases and action
  deprecation metadata are removed; built-in bindings use the current names.
- Snippets are built in (`snippet` crate): Rust has `main`, `async_main`,
  `tests`, `test`, and `async_test`. Language snippets are offered with global
  ones and do not require a project. The user snippets directory is not loaded or
  watched, and there is no snippet picker/configure action or snippets JSON schema.
- `vim_mode`, `helix_mode`, and the `vim` settings section do not exist. There is
  no Vim keymap layer, mode indicator, Vim/Helix toggles (quick action bar), or Vim-style keybinding notation.
- Journaling is removed: no journal entry command or `journal` settings section.
- Onboarding is removed: no first-launch onboarding page, `ed::OpenOnboarding`,
  base keymap picker, or multibuffer hint toolbar. There is no VS Code/Cursor
  settings importer (actions or the underlying translation code). The first
  launch opens an empty editor like any other launch.
- The welcome page is removed: no `ed::ShowWelcome` action, no Get Started /
  Recent Projects page, and no `welcome::OpenRecentProject` (cmd-1…5) bindings.
  Every empty pane shows the keybinding hints instead (see ui-layout.md).
- Bookmarks are removed: no bookmark actions, labels, navigation, project view,
  bookmark gutter markers, or `gutter.bookmarks` setting (the gutter hover
  button now copies the file location; see editor.md).
