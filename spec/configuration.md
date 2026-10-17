# Configuration

## Decision

Settings are constructed entirely in Rust as one complete `SettingsContent`
object shared by every project. There is no default-settings JSON or separate
user/default settings layer. Language metadata supplies language conventions;
explicit per-language values in code take precedence. Keymaps and snippets are
also constructed in Rust, including the personal bindings and snippets.
Changing persistent settings, shortcuts, or snippets requires editing code and
rebuilding. There is no settings UI, keymap editor, snippet UI, settings/keymap
migrator, or Vim/Helix mode.

## Behaviour

- Stateless mode is removed. Databases use persistent storage, with an in-memory
  fallback only when storage cannot be opened. `ZED_STATELESS` has no effect;
  single-instance checks are bypassed only in debug builds.
- User, global, server, and `.zed/settings.json` files are not loaded, watched,
  created, or rewritten. There are no settings profiles or project overrides.
  Changing persistent preferences requires editing the built-in settings and
  rebuilding. Existing settings controls affect only the running session.
- Built-in preferences preserve the applicable values from the personal Zed
  configuration, including language overrides and Rust language-server options.
  Entries for removed features are omitted.
- Semantic token fallback mappings are constructed in Rust. Personal rules take
  precedence over language/extension rules, which take precedence over fallback
  mappings. Rule order and theme-style fallback order are preserved. There is no
  bundled semantic-token settings JSON or default-rules viewer.
- Settings-file actions, shortcuts, menu entries, and welcome/error links are
  removed, along with the project/server settings commands and profile selector.
  The default-settings view is removed too.
- User and bundled keymap JSON files are removed from the loading path. There is
  no keymap watcher, file editor/viewer action, base-keymap selection, or keymap
  JSON schema. EditorConfig formatting conventions remain supported; task files
  are not (see `tasks.md`).
- The built-in keymap retains contexts, binding order, and keyboard-position
  equivalents. Changing the keyboard layout rebuilds the bindings and refreshes
  menu shortcuts.
- Personal bindings are applied last: global `cmd-[`/`cmd-]` navigate backward/
  forward, Editor `f1` opens documentation and `cmd-shift-u` toggles case, and
  Terminal `shift-enter` sends Escape followed by carriage return. The obsolete
  collaboration-panel unbind is omitted.
- Remote projects receive the client's in-memory preferences; remote settings
  files and project settings messages cannot override them.
- `zed://settings` URLs are not handled. Settings-file JSON schemas, completions,
  validation, and language-server configuration schema probing are removed.
  JSON language support remains for source files and action inputs.
- The upstream documentation website preprocessor is removed; this fork does not
  build the upstream mdBook site.
- The command palette has no "Add/Change Keybinding" flow; secondary confirm runs
  the selected command.
- Snippets are built in (`snippet` crate): Rust has `main`, `async_main`,
  `tests`, `test`, and `async_test`. Language snippets are offered with global
  ones and do not require a project. The user snippets directory is not loaded or
  watched, and there is no snippet picker/configure action or snippets JSON schema.
- `vim_mode`, `helix_mode`, and the `vim` settings section do not exist. There is
  no Vim keymap layer, mode indicator, Vim/Helix toggles (quick action bar), or Vim-style keybinding notation.
- Journaling is removed: no journal entry command or `journal` settings section.
- Onboarding is removed: no first-launch onboarding page, `zed::OpenOnboarding`,
  base keymap picker, or multibuffer hint toolbar. There is no VS Code/Cursor
  settings importer (actions or the underlying translation code). The first
  launch opens an empty editor like any other launch. The welcome page
  (`zed::ShowWelcome`) remains, without a "Return to Onboarding" button.
- Bookmarks are removed: no bookmark actions, labels, navigation, project view,
  bookmark gutter markers, or `gutter.bookmarks` setting (the gutter hover
  button now copies the file location; see editor.md). Existing saved bookmarks are
  discarded when the workspace database is upgraded.
