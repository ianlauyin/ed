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
  tab. Commit history lives in Git History mode only.
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

## Git History mode

Commit history is kept apart from editing: the window is either in Workspace
mode (docks, editor panes, status bar) or in Git History mode, which shows only
one pane holding the git graph and commit tabs. Commit buffers are read-only
blobs with no language server, so they are not mixed into editor panes.

- Round "Workspace" / "Git History" tabs in the title bar, right after the
  branch picker, switch modes. They are shown whenever the branch is. "Git
  History" returns to the open history tabs, or opens the repository graph if
  there are none. `workspace::ShowEditor` and `git_graph::Open` switch from the
  keyboard. The active tab has no fill: it shows full-contrast text and an
  accent-coloured icon, while inactive tabs are muted. Both show the ghost
  hover background, so the filled pill no longer pulls focus from the content.
- Git History mode hides the docks, editor panes and status bar. The history
  pane is a normal pane (folder tabs, card, toolbar with buffer search) that
  never splits or zooms. Its tabs are not restored after a restart.
- The graph and every commit view open only in the history pane: the graph
  from `git_graph::Open` / File History, commits from the graph, blame, commit
  tooltips, the stash picker and the git panel. Opening one switches to Git
  History mode. The repository-wide graph is always the first tab.
- "Open File" on a commit diff excerpt opens that revision as a read-only tab
  in the history pane, since it can't use workspace features. Buffers that are
  project files still open in the editor. The revision tab keeps its commit's
  sidebar, with the file revealed (folders expanded, row highlighted) whenever
  the tab is focused.
- Focusing a dock panel or an editor pane (opening a file, Open File in Project
  from a commit, File Finder, …) switches back to Workspace mode. Closing the
  last history tab does too.
- The commit details panel (author, refs, email, SHA, message, changed files)
  is a flat, resizable sidebar on the left, like the project panel, with the
  history tabs to its right. Its width is shared by every history tab. It
  shows the active tab's commit: the graph's selected commit or the commit
  view's commit. It has no close button; the graph selects the newest commit
  once commits load (unless a specific commit is being selected, e.g. "Show in
  Git Graph"), so the sidebar is always shown except for an empty history.

## Commit view

- The diff card has no header of its own; commit details are in the history
  sidebar. The toolbar keeps buffer search and "Show in Git Graph".
- Clicking a changed file in the sidebar scrolls the diff to it, instead
  of opening a single-file commit view as it does for the graph's selected commit.
- While one of the commit's revision tabs is active, clicking a changed file
  opens that file's revision instead (like "Open File" on its excerpt, so it
  follows the preview-tab settings and lands on the first hunk). The click
  always acts on the visible tab's mode: diff stays diff, revision stays
  revision.

## Git graph

- Search matches the whole commit message (`git log --grep --fixed-strings`)
  or, for a 7+ character hex query, a commit hash prefix. Every place that
  shows the matched text highlights it: the subject column, the detail panel's
  commit message (all occurrences) and, for hash queries, the SHA column.
  Highlight ranges follow the case-sensitivity toggle.
