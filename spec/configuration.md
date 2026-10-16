# Configuration

## Decision

Settings and keymaps are configured only by editing JSON files. There is no
settings UI, no keymap editor, no settings/keymap migrator, and no Vim or Helix
modal editing.

## Behaviour

- `zed::OpenSettingsFile` (`cmd-,`) opens `settings.json`; `zed::OpenKeymapFile`
  (`cmd-k cmd-s`) opens `keymap.json`. Project, default settings, and default
  keymap files remain openable. The settings-window, keymap-editor, and
  "open settings at path/page" actions are removed.
- `zed://settings` URLs are not handled.
- The command palette has no "Add/Change Keybinding" flow; secondary confirm runs
  the selected command.
- Settings and keymap files are parsed as-is. Outdated formats are not migrated
  in memory or on disk, and there is no migration banner or notification; invalid
  files show the parse error with a link to open the file.
- `vim_mode`, `helix_mode`, and the `vim` settings section do not exist. There is
  no Vim keymap layer, mode indicator, Vim/Helix toggles (quick action bar), or Vim-style keybinding notation. Keymap layers are User > Base >
  Default.
- Journaling is removed: no journal entry command or `journal` settings section.
- Onboarding is removed: no first-launch onboarding page, `zed::OpenOnboarding`,
  base keymap picker, or multibuffer hint toolbar. There is no VS Code/Cursor
  settings importer (actions or the underlying translation code). The first
  launch opens an empty editor like any other launch. The welcome page
  (`zed::ShowWelcome`) remains, without a "Return to Onboarding" button.
- Bookmarks are removed: no bookmark actions, labels, navigation, project view,
  gutter controls, or `gutter.bookmarks` setting. Existing saved bookmarks are
  discarded when the workspace database is upgraded.
