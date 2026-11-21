# Git

## Decision

Git support is local-only. There is no notion of a hosting provider (GitHub,
GitLab, …): remote URLs are never parsed into web links, and the `git` crate
makes no HTTP requests.

No git binary is bundled. All git operations use the system `git` found on the
repository's `$PATH`; if none is found, the repository is not opened.

## Behaviour

- Every repository is trusted (there is no restricted mode): git runs with
  the user's own config, so hooks, credential helpers, `core.sshCommand` and
  external diff tools all apply. Only `core.fsmonitor` and
  `log.showSignature` are forced off.
- Editing git's global config (`~/.gitconfig` or
  `$XDG_CONFIG_HOME/git/config`, `~/.config/git/config` by default) refreshes
  local repositories, as git reads both.
- No permalinks: the editor's Copy/Open Permalink to Line, the file
  Copy/Open File Permalink actions and their menu entries are removed.
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
- The HEAD text behind a file's diff is only syntax-parsed once that diff can
  show deleted lines (the file, or its staged index, differs from HEAD); after
  that it stays parsed. Every open buffer has a HEAD diff (gutter, project
  search results), so unchanged files would otherwise hold a second syntax
  tree each.
- File changes refresh git status for just the changed paths (coalesced to
  their common ancestors), but above 64 paths the whole repository is scanned
  instead: git matches every pathspec against every index entry, so many
  pathspecs cost more than one full scan on large repositories.
- There are no `git` settings; the former defaults are hardcoded: git status
  and diffs always on; gutter hunks in tracked files, recalculated without
  debounce; unstaged hunks filled, staged hollow; stage/restore hunk buttons
  shown; git panel paths show the file name first; the branch picker shows
  commit authors; file diffs open showing the full file; linked worktrees are
  created under `<repo parent>/worktrees/<repo name>/`.

## Active folder and repository

- The project has one active folder (a visible worktree) and an optional
  active repository: the innermost repository containing the folder, or a
  repository nested inside it that was picked explicitly. Picking a repository
  also makes its folder active. Both are session-only.
- The window header's project menu is the only switcher (no repository picker
  in the git panel or Git History); `git::SelectRepo` and
  `title_bar::ToggleProjectMenu` open it. It lists every open folder, each
  followed by the repositories nested in it (submodules, vendored repositories)
  as indented rows, with branch, a dot when it has changes, and a check on the
  active row; then recent projects. Nested rows can't be removed. The menu's
  folder section shows whenever there is more than one row.
- A folder outside any repository can be picked: the header then shows only
  its name (no worktree / branch, no Git History button in the mode switch) and the
  git panel offers to initialize a repository.
- Two folders in one repository are separate rows; the check follows the one
  picked. With a nested repository active the header reads
  `folder/path/to/repo`.
- Focusing a file makes its folder and innermost repository active (none for
  a file outside any repository).
- When the active repository is removed, the active folder's own repository
  takes over (or none); when the active folder is removed, any remaining
  repository does.

## Git panel

- The panel never names the repository or branch: the window header right
  above does. The row above the commit card only holds the remote button
  (Fetch / Push / Pull / Publish), and is left out without one.
- There is one Pull: `git pull --rebase --autostash`. No merge pull, so local
  commits never produce merge commits, and uncommitted changes don't block
  the pull.
  - Dirty buffers that have a file are saved first (autostash rewrites files
    on disk); cancelling a save cancels the pull. Untitled buffers are left
    alone.
  - Git owns the stash, so a rebase that stops on conflicts keeps the
    uncommitted changes until `rebase --continue` / `--abort` reapplies them.
    The stop is reported as an error with git's own hint.
  - While a rebase is stopped on conflicts (git's `rebase-merge` /
    `rebase-apply` state directory exists; `REBASE_HEAD` can go stale) the remote
    button row shows "Rebase in progress" with Abort and Continue
    (`git::RebaseAbort` / `git::RebaseContinue`) instead. Continue is disabled
    until every conflict is staged; both save dirty buffers first.
  - Actions that don't apply mid-rebase are disabled rather than left to
    fail or confuse: commit / amend, uncommit, fetch / pull / push and the
    stash entries. Staging, restoring and resolving stay available. Branch
    switching is not gated.
  - When the reapplied changes conflict with what was pulled, the pull still
    succeeds: the toast says the changes are still in the stash, to be dropped
    after resolving.
  - Trade-off: staged changes come back unstaged, and local merge commits are
    linearised.

- The panel shows only changes: no tab bar, no "Changes" title and no History
  tab. Commit history lives in Git History mode only.
- The commit message editor is a floating card like the editor panes:
  `editor_background`, `CARD_RADIUS` corners, spanning the dock's full width
  (no extra inset), so its gaps to the window edge and to the center cards are
  the same `CARD_GAP` as between any two cards. The branch row and commit
  text then share one left edge. No divider lines above or below: the branch
  row, last-commit row and title-length warning sit directly on the
  chrome-coloured panel. The card stays a card when collapsed (just the commit
  button row) or filling the panel.
- There is no commit modal: the commit editor lives only in the panel, with a
  single top-right button that toggles filling the panel. While filled, the
  button stays visible in the accent colour (like the pane zoom toggle) so the
  expanded state is obvious. `git::Commit` /
  `git::Amend` from outside the editor open the panel and focus its commit
  editor (amend loads the last commit message) instead of opening a modal.
- Not configurable: docked left only (360px default width), always has a
  status bar button, shows status icons (labels keep the default colour),
  folder icons in tree view, per-file and total diff stats, and no commit
  title length warning. Clicking an entry opens the project diff;
  secondary-click opens the single-file diff. The commit editor starts
  expanded. `git init` falls back to `main` when git has no
  `init.defaultBranch`. Untracked files are not folded in the project diff.
- View options (tree/list, sort by path/name, group by none/status/staging)
  are session-wide state shared by every git panel and project diff, not
  persisted; each launch starts in tree view grouped by status.
- Removing untracked files uses "Delete" wording everywhere (menu items,
  prompts, errors, `git::DeleteUntrackedFiles`), matching the project panel,
  even though files are moved to the system trash. Never say "Trash".

## Git History mode

Commit history is kept apart from editing: the window is either in Workspace
mode (docks, editor panes) or in Git History mode, which shows only
one pane holding the git graph and commit tabs. Commit buffers are read-only
blobs with no language server, so they are not mixed into editor panes.

- An icon-only, segmented "Editor" / "Git History" switch at the end of the
  window header, after Go Back / Go Forward, switches modes (`ui-layout.md`).
  It is shown whenever the project is in a git repository. "Git
  History" (`mode::GitHistory`) follows the active repository (the folder picked
  in the window header): it returns to the active history tab if that tab belongs
  to the active repository, otherwise it opens (or reuses) that repository's
  graph. A tab belongs to its graph's repository, or to the repository of the
  commit in its sidebar. Mode-switching actions live in the `mode`
  namespace (`mode::Editor`, `mode::GitHistory`), one per mode.
- While Git History is shown, picking another folder (changing the active
  repository) applies the same rule, so the history switches to that
  repository's graph; tabs of other repositories stay open. Picking a folder
  or repository from the header menu while Git History is shown always brings
  its repository graph to the front (`git_graph::Open`), even when it is
  already the active one, so re-picking the current folder leaves a commit tab
  or another repository's graph. The active tab has no fill: it shows full-contrast text and an
  accent-coloured icon, while inactive tabs are muted. Both show the ghost
  hover background, so the filled pill no longer pulls focus from the content.
- Git History mode hides the docks, editor panes and the status bar (it
  offers too little there to be worth the space). There is no Git Graph button;
  picking a folder in the window header returns to its graph (below). The history area is
  its own pane group, apart from the editor panes: normal panes (folder tabs,
  card, the same toolbar items as editor panes) that split like editor panes:
  dragging a tab to a pane edge, `pane::Split*`, and "open to the side"
  (Option-click on a preview button) all split within the history group and
  stay in Git History mode. As in Editor mode, splitting for "open to the
  side" neither activates nor focuses the new pane: focus stays where it was,
  and the focused pane is always the active one. Preview actions from the keyboard target the
  history tab when Git History is shown.
  Its tab bars have, on the focused pane, Zoom
  (back/forward is the window header's, shared with Editor mode); there is no New… menu, since files and terminals belong
  to Editor mode. Zoom shows only that pane, without the commit sidebar
  (as zooming hides docks, other panes and the status bar in Editor mode). Every history tab is read-only, so the tab context menu
  leaves out "Close Multibuffers". History tabs and
  splits are not restored after a restart, but survive switching modes.
- The graph and commit views open in the active history pane (the last
  focused one); reopening a graph or commit that is already open activates or
  replaces its tab in whichever pane holds it. An emptied history pane closes
  unless it is the last one.
- The graph and every commit view open only in a history pane: the graph
  from `git_graph::Open` / File History, commits from the graph, blame, commit
  tooltips, the stash picker and the git panel. Opening one switches to Git
  History mode. A new repository-wide graph is always the first tab of its pane.
- Git History always holds the repository graph: whatever opens a history tab
  (File History, a commit, a revision) also opens its repository's graph as
  the first tab of that pane, behind it, unless that graph is already open, so
  closing the tab lands on the graph instead of leaving the mode.
  The repository-wide graph tab is permanent (`Item::is_permanent`): it has no
  close button, no Pin/Unpin entries, and every close path (Close, Close All,
  middle click, max tabs) skips it, so Git History mode is left through the
  window header switch. File history graphs close as usual.
- "Open Revision" (the buffer header button, "Open File" elsewhere) on a
  commit diff excerpt opens that revision as a read-only tab in the active
  history pane, since it can't use workspace features. Buffers that are
  project files still open in the editor. The revision tab keeps its commit's
  sidebar, with the file revealed (folders expanded, row highlighted) whenever
  the tab is focused. It shows the minimap like any editor: "active editor"
  means the active item of the pane shown in the current mode (the active
  history pane in Git History mode), not the last active editor pane.
- Revisions get go to definition (cmd+hover underline, cmd+click, shift for
  the type) and find references from the project's language servers, answered
  for the file as it is now: language servers only know the current project,
  so revisions are never sent to them. When the line is unchanged between the
  revision and the current file, the request goes to the same spot in the
  current file. Otherwise the word is looked up by exact name in the project's
  symbols, preferring the revision's own file when it defines it; references
  then come from that definition, only when the name matches exactly one. A
  name that no longer exists gets no result. Results are current project
  files, so opening one switches to Editor mode. The commit diff does the
  same on its new side (added and unchanged lines); a commit's diff and its
  revision tabs share the current files they keep open. Deleted lines, and the
  old side of a split diff, get none: in any diff, a deleted line's text isn't
  in the buffer, so definitions and references are never asked for there.
- A revision's right-click menu disables, as in any read-only editor, Rename,
  Code Actions, Cut and Paste.
- Focusing a dock panel or an editor pane (opening a file, Open File in Project
  from a commit, File Finder, …) switches back to Editor mode.
- A preview (e.g. Markdown) of a revision keeps its commit's sidebar,
  since the sidebar follows the commit the active tab belongs to.
- The commit details panel (author, refs, email, SHA, message, changed files)
  is a flat, resizable sidebar on the left, like the project panel, with the
  history tabs to its right. Its width is shared by every history tab. It
  shows the active tab's commit: the graph's selected commit or the commit
  view's commit. It has no close button; the graph selects the newest commit
  whenever it has commits and nothing selected, whether they were already
  loaded (graph reopened) or arrive later (unless a specific commit is being
  selected, e.g. "Show in Git History"), so the sidebar is always shown except
  for an empty history. One rule (`select_newest_if_unselected`) serves both.
- Its changed files rows match the git panel's (row height, padding, tree
  indent and guides, icon gap, default-size labels), sharing the git panel's
  constants, so the list reads the same in Editor and Git History mode.
- In a file history graph (View History / File History), the sidebar reveals
  that file in each selected commit's changed files (folders expanded, row
  highlighted, scrolled into view once the files load).

## Commit view

- The diff card has no header of its own; commit details are in the history
  sidebar. The toolbar has the multibuffer diff controls (collapse/expand all
  files, unified/split diff) on the left and the commit's +/- line counts and
  "Show in Git History" on the right. Hunk navigation and
  staging buttons are left out: the diff is read-only.
- "Show in Git History" opens the graph of the commit's own repository, not
  the active one, so it works in multi-root projects.
- Clicking a changed file in the sidebar scrolls the diff to it, bringing the
  commit view's tab and pane to the front and focusing it. In the graph,
  it opens the whole commit view and scrolls to that file; there is no "View
  Commit" button, since clicking a file is the way in.
- Opening from the graph reuses the diff the sidebar already loaded instead of
  reading the commit from git again. The commit view builds files
  concurrently, the clicked file first, and inserts each one as soon as it is
  ready, so it scrolls to the clicked file without waiting for the rest of the
  commit. The scroll puts the file's header at the top and is anchored inside
  that file, so files inserted above it while loading don't move it out of view.
- Opening a commit from blame (gutter click, the blame tooltip's SHA button,
  "Open Git Blame Commit") scrolls to the blamed file the same way.
- The commit message keeps its line breaks (soft breaks render as hard
  breaks), since commit bodies are hard-wrapped.
- While one of the commit's revision tabs, or a preview of one, is active,
  clicking a changed file opens that file's revision instead (like "Open Revision" on its excerpt, so it
  follows the preview-tab settings and lands on the first hunk). The click
  always acts on the visible tab's mode: diff stays diff, revision stays
  revision.

## Changed files context menu

Right-clicking a changed file in the commit details sidebar (graph or commit
view) offers:

- "Open Revision": opens the file's revision tab at that commit. From the
  graph this opens the commit view, then its revision.
- "Open File in Project": opens the file from the project in Editor mode. If
  the path is no longer a file in the project (moved or deleted since the
  commit), a toast says so instead. The commit diff's buffer header menu has
  the same entry (same label, same behaviour), shown only while the file is in
  the project.
- The commit diff's buffer header menu leaves out "Reveal In Project Panel"
  and "Open in Terminal": the excerpt is a revision, not the project file.
- Changed file rows have no hover tooltip, so it doesn't compete with the
  menu.

## Git graph

- A toolbar row sits above the graph (the commit sidebar only describes the
  selected commit, so controls that change what the graph shows live here):
  the branch filter on the left, Search and Refresh icon buttons on the right.
  It and the search row use the pane toolbar metrics (`ui-layout.md`).
- Branch filter (repository graph only, not file history): "All Branches" by
  default; it opens the branch picker in select mode (local and remote
  branches, "Select a branch…") and limits the graph to that branch's history
  (`LogSource::Branch` with the full ref). While filtered the button shows the
  branch name in the accent colour with a clear button next to it. The tab
  stays the same permanent repository graph; search follows the filter. The
  filter is session-only, is dropped when its branch disappears, and is
  cleared when opening the graph at a specific commit (e.g. "Show in Git
  History"), since that commit may not be on the branch.
- Reloads (refs, HEAD, tags or stash changed, filter changed, Refresh) keep
  the selected commit by sha, not by row, since new commits shift the rows;
  the details sidebar stays up meanwhile. When it is gone (e.g. not on the
  filtered branch) the newest commit is selected. This holds whether the
  commits are read from git or taken from the repository's cache (switching
  back to a source loaded before sends no load events), so a reload never
  leaves Git History without a selection or sidebar.
- When an automatic reload or Refresh brings in commits above the previous
  newest one, an "N new commits" pill floats over the top of the list; it
  selects the newest commit and goes away when clicked or when a new commit
  is selected.
- Refresh (`git_graph::Refresh`, `cmd-r` in the graph) is for changes the
  file watcher misses, e.g. a repository on a network or VM-shared mount
  changed from another machine. It rescans the worktrees (like
  `workspace::ReloadProjectFiles`), then drops the cached commits and
  reloads them. The button spins while commits load.
- The search row is hidden until opened with `cmd-f` (or the Search button),
  like buffer search.
  `escape` hides it and clears
  the matches and highlights; the query is kept and searched again on reopen.
  While hidden, the graph itself takes focus.
- Search matches the whole commit message (`git log --grep --fixed-strings`)
  or, for a 7+ character hex query, a commit hash prefix. Every place that
  shows the matched text highlights it: the subject column, the detail panel's
  commit message (all occurrences) and, for hash queries, the SHA column.
  Highlight ranges follow the case-sensitivity toggle.
