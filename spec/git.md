# Git

## Decision

Git support is local-only. There is no notion of a hosting provider (GitHub,
GitLab, …): remote URLs are never parsed into web links, and the `git` crate
makes no HTTP requests.

No git binary is bundled. All git operations use the system `git` found on the
repository's `$PATH`; if none is found, the repository is not opened.

## Behaviour

- No permalinks: the editor's Copy/Open Permalink to Line, the file
  Copy/Open File Permalink actions and their menu entries are removed, along
  with the matching RPCs.
- No pull requests: the `git::CreatePullRequest` action is removed, and a
  successful push only shows a toast with "View Log"; PR/MR links in push
  output are not surfaced.
- Commits show no remote links: blame and commit tooltips show no PR number,
  the blame context menu has no "Open Commit Permalink", and the commit view
  and git graph have no "View on <provider>" button. Remote branches in the
  branch picker use a generic server icon.
- The `git_hosting_providers` setting is removed.
- Commit author avatars are removed everywhere (blame gutter, blame and commit
  tooltips, commit view, git graph). Authors are shown by
  name/email only.
- The `git.blame` settings group (`show_avatar`) is removed.
- No inline blame: the cursor-line blame (end of line or status bar), the
  `git.inline_blame` settings, `editor::ToggleGitBlameInline` and the "Inline
  Git Blame" menu toggle are removed. The `editor::BlameHover` cursor popover
  is removed too. The only blame UI is the gutter (`git::Blame`, menu entry
  "Git Blame").
- The diff base is always HEAD: gutter hunks, file status colours and
  `git::Diff` show working changes. The `git.diff_base` setting, the
  `git::DiffHead`/`git::ToggleDiffBase` actions and the "Diff Against Default
  Branch" editor menu toggle are removed. Branch diffs remain available
  explicitly via `git::DiffBranch` / Compare With Branch.

## Git panel

- The panel shows only changes: no tab bar, no "Changes" title and no History
  tab. Commit history lives in the git graph only.
- The git graph opens from a status bar button placed right after the git panel
  button (`Panel::secondary_button`), not from inside the panel. It is shown
  whenever the git panel button is.
- The commit message editor uses `editor_background`, the same colour as the
  editor and terminal cards, so it reads as editable content against the
  chrome-coloured panel.
- There is no commit modal: the commit editor lives only in the panel, with a
  single top-right button that toggles filling the panel. `git::Commit` /
  `git::Amend` from outside the editor open the panel and focus its commit
  editor (amend loads the last commit message) instead of opening a modal.
- Removing untracked files uses "Delete" wording everywhere (menu items,
  prompts, errors, `git::DeleteUntrackedFiles`), matching the project panel,
  even though files are moved to the system trash. Never say "Trash".

## Commit view

- The header is a single compact row: author, date and email (separated by
  `•`) aligned with the editor text, with the "Commit SHA" copy button on the
  same row at the right. The commit message sits directly below.
