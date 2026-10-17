# Git

## Decision

Git hosting providers are used only to build URLs locally (permalinks, commit
links, pull request links). They make no network requests.

## Behaviour

- Commit author avatars are removed everywhere (blame gutter, blame and commit
  tooltips, commit view, git graph). Authors are shown by
  name/email only.
- `GitHostingProvider` has no avatar API; the `git` and `git_hosting_providers`
  crates make no HTTP requests.
- The `git.blame` settings group (`show_avatar`) is removed.

## Git panel

- The panel shows only changes: no tab bar, no "Changes" title and no History
  tab. Commit history lives in the git graph only.
- The git graph opens from a status bar button placed right after the git panel
  button (`Panel::secondary_button`), not from inside the panel. It is shown
  whenever the git panel button is.
- The commit message editor uses `editor_background`, the same colour as the
  editor and terminal cards, so it reads as editable content against the
  chrome-coloured panel.
