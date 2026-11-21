# Editor

## Behaviour

- There is no bracket colorization (rainbow brackets). Bracket queries are
  used only for matching, autoclose and navigation; `brackets.scm` has no
  `rainbow.exclude` markers.
- Occurrence highlights — LSP document highlights (read/write symbol
  occurrences) and selected-text occurrence highlights — only run in
  single-file editors. Multibuffers (project search, find references, etc.)
  already highlight their matches, and stacking occurrence highlights on top
  made results unreadable. The selection itself and bracket highlights still
  show everywhere.
- Only one kind of occurrence highlight shows at a time: if a language server
  for the file supports document highlights, those are used and selected-text
  highlights are off; otherwise selected-text highlights are the fallback.
  Having both on the same page was messy.
- In a single-file editor, both kinds of occurrence highlights are also
  suppressed while buffer search is showing matches, so only the matches are
  highlighted and nothing can be confused with them. They come back once the
  matches are cleared (search dismissed, or the query finds nothing).
- Hovering a gutter row (after a short delay) shows a bookmark-icon button left
  of the line number. Clicking it copies the worktree-relative `path:line` to
  the clipboard. If the newest selection
  spans multiple lines and covers the clicked row, it copies `path:start-end`
  instead (same format as `editor::CopyFileLocation`). After copying, the
  button turns into a green check ("Copied!") for 2s, staying visible even
  after the mouse leaves, matching the shared copy-button feedback. The button is hidden for
  buffers without a worktree file, on rows with a run indicator, on the left
  side of a split diff, and in editors that hide runnables (previews).
  Singleton editors always reserve gutter space for it. The button (like run
  indicators, which share the slot) is centered between the git strip and the
  line numbers, so the gap stays even when numbers fill their column (1000+
  line files).
  The gutter right-click menu has a matching "Copy File Location" entry with
  the same row/selection behaviour.
- No vertical scrollbar: the minimap is the vertical scroll widget. The
  markers the scrollbar used to carry are drawn on the minimap, aligned with
  the minimap row of their line (so clicking a marker lands on it): cursor
  lines as a faint full-width band, git hunks as a thin bar on the left edge,
  search/selection/symbol occurrences and diagnostics as two lanes on the
  right edge. All marker kinds are always on (git hunks only when git diff is
  enabled); there are no editor `scrollbar` settings. In files longer than
  the minimap, off-screen markers scroll in with the minimap. Editors without
  a minimap have no vertical scroll widget.
- Fixed editor behaviour, with no settings: blinking bar cursor with movement
  animation; full-width current-line highlight; rounded selections (editor
  and terminal); selected-text occurrence highlights on; 75ms LSP document
  highlight debounce; hover popover on, sticky, 300ms show and hide delays;
  snippets sorted inline with other completions; 3-line vertical scroll
  margin; full editors scroll one page past the last line. The minimap shows
  only in the active editor (toggleable per editor), up to 80 columns wide,
  with an always-visible borderless thumb.
- The editor has no settings at all (no `EditorSettingsContent` or
  `EditorSettings`). Also fixed:
  - Mouse: alt adds cursors, cmd/ctrl follows links; shift+cmd/ctrl or
    shift+alt starts a columnar selection. Alt scrolls 4x faster. Clicks never
    autoscroll. No drag-and-drop of selected text and no middle-click paste.
    Double-click in a multibuffer selects a word like in a single file.
  - Search: starts with no options set, searches as you type, wraps around,
    no smartcase, and the status bar search button is always shown. Search
    seeds its query from the word under the cursor.
  - Navigation: go to definition falls back to find all references when it
    finds nothing, and centers the target. Results open in a multibuffer
    unless the action asks for the picker (`open_results_in`).
  - LSP: diagnostics down to hints; inline code actions; document colors as
    inlays only; document links on.
  - Completions: kind letter always shown, details left-aligned, no scrollbar.
  - Multibuffers: 5 lines of excerpt context (`EXCERPT_CONTEXT_LINES`); expand
    by 5 lines. Highlighted text keeps an APCA contrast of at least 45.
  - Diffs open split, and fall back to unified below 100 columns. The
    unified/split toggle is a session-wide choice (`GlobalDiffViewStyle`): it
    switches every open diff view and applies to new ones, and is not saved.
- The minimap runs the full card height. The minimap thumb, its markers and the
  current-line highlight are plain square quads; the pane card's corner mask
  (`ui-layout.md`) rounds them where they reach the card's corners.
- Minimap text is drawn as blocks, not glyphs: each run of non-whitespace
  characters is one faded quad in its syntax color, on a one-cell-per-character
  grid (`WindowTextSystem::layout_monospace_line`, `ShapedLine::paint_blocks`).
  Its lines are never shaped by CoreText and it paints far fewer primitives.
  Laid-out minimap rows are kept across frames (`MinimapRowCache`) while the
  display snapshot would produce the same chunks (`same_chunks_as`: buffer
  edit/non-text counts, display map layer versions, highlights by content), so
  scrolling or a cursor blink only lays out rows that come into view. The
  theme is fixed at compile time, so it isn't part of that check.
- The horizontal scrollbar matches the `ui` scrollbars (project panel): no
  track, a thin rounded thumb inset in a 14px hitbox.
  It auto-shows while scrolling or when the buffer has markers (git hunks,
  highlights, diagnostics, off-screen cursors). Panels and the terminal
  default their scrollbars to `auto` instead of inheriting from the editor;
  panels and the terminal have no `scrollbar` setting (always `auto`), and
  the project panel always scrolls horizontally for long file names.
- No sticky scroll: enclosing outline items are never pinned to the top of
  the editor (removed with its `sticky_scroll` setting). Multibuffer sticky
  buffer headers are a separate feature and remain.
- A multibuffer header's "Open File" button (or its addon label, e.g. "Open
  Revision") only shows while the header is hovered, on every header alike,
  with no key binding. Opening from a multibuffer is mouse-only (header
  button/filename, gutter line number, double-click): there is no
  `OpenExcerpts` action, and it always opens one buffer with one cursor at
  the clicked spot, never the multibuffer's selections.
- A multibuffer header's context menu ends with a "Fold All" / "Unfold All"
  group (every file in the multibuffer). Addons can drop its project-tree
  entries (Reveal In Project Panel, Open in Terminal) via
  `shows_project_file_actions`.
- Smooth scrolling is always on, with no setting. Jumps (page/half-page
  scrolls and autoscroll to the cursor: search, go-to-definition, `G`) move
  the scroll anchor at once; only drawing trails it, easing out (cubic) to ~95% in `motion::NAVIGATION`.
  Everything except painting (selections, events, saved position, LSP
  ranges) sees the destination immediately. Jumps of one row snap, so typing
  or stepping past the viewport edge never lags; jumps longer than a screen
  skip ahead and animate only the last screen. A new jump mid-animation starts
  from the drawn position. Both sides of a split diff animate together. Mouse
  and trackpad scrolling are not animated.
- Cursor animation (neovide-style trail) keeps the vscode-neovide-cursor
  timing proportions, scaled so a long move is ~95% there in
  `motion::NAVIGATION` (90ms). Each corner axis is a critically damped
  `gpui::SpringConfig` (`critically_damped(length)`);
  the trail logic (per-corner lengths, snapping) stays in `cursor_motion`.
  A same-row move of one character (up to two em advances, so a full-width
  character counts) snaps, so typing and `h`/`l` draw no animation frames.
- No mouse-wheel zoom: Cmd/Ctrl+scroll scrolls like a plain scroll and never
  changes the buffer font size (removed with its `mouse_wheel_zoom` setting).
- The gutter has no settings: line numbers (with space reserved for 4
  digits), runnables, fold toggles and default-width git diff strips are
  always shown. There is no `gutter` settings block and no
  `editor::ToggleLineNumbers` action or keybinding.
- Gutter line numbers are always absolute. Relative line numbers are removed
  (no `relative_line_numbers` setting, no `editor::ToggleRelativeLineNumbers`).
- No signature help: no popup, actions, keybindings or settings. The LSP
  `textDocument/signatureHelp` request isn't sent, the client capability isn't
  advertised. Use go-to-definition or
  hover to see a function's parameters.
- Code lenses only appear in the hover popover: never as lines in the editor
  or in the code action menu. Lenses whose range covers the hovered position
  (i.e. hovering a definition's name) show as a clickable row at the top of
  the LSP hover popover, above the signature and docs; they're fetched and
  resolved alongside the hover request. Always on whenever a language server
  provides lenses: there is no `code_lens` setting or `editor::ToggleCodeLens`
  action.
- Inlay hints and LSP semantic highlights are always on whenever a language
  server provides them; stop the server to get rid of them. Semantic tokens
  are requested only for languages with semantic token rules (see
  `configuration.md`); other languages never send semantic token requests. Semantic tokens always layer on top of tree-sitter
  highlighting, never replace it. There is no `semantic_tokens` setting, no
  `inlay_hints.enabled` / `inlay_hints.toggle_on_modifiers_press`, no
  `editor::ToggleInlayHints` / `editor::ToggleSemanticHighlights` action or
  keybinding. The per-kind `inlay_hints.show_*`
  filters, background and debounce settings remain.
- Inlay hints are fetched per 50-row chunk and cached without limit until an
  edit or a server refresh invalidates them. Singleton buffers of up to 1000
  rows fetch every chunk (on open and on each invalidation); larger buffers
  and multibuffers fetch only the chunks in the viewport.
- The hover popover's code lens row renders in the UI font, like the outline
  panel; the signature and docs below it use the shared hover markdown style.
- No breadcrumbs anywhere (no breadcrumbs toolbar item, `Item::breadcrumbs`
  API, `toolbar` settings block or `editor::ToggleBreadcrumb`), and no quick
  action bar (selections menu, editor controls, code actions): those stay
  reachable through actions, keybindings and the command palette. Preview
  moved to the tab bar (see ui-layout.md). With no other toolbar item, an editor's card starts right
  at the text. No symbol path anywhere either: multibuffer headers show only
  the file name and its directory, so rendering them never waits on outlines
  or language servers.
- Search has no button anywhere (no tab bar toggle, no toolbar button, no
  close (✕) button on the search row): `cmd-f` (or the menu / action) opens
  or refocuses it and `escape` closes it. Normal files get no extra row:
  the search row appears only while search is open. Multi-buffer views with
  their own controls (unified/split, staging) keep a toolbar row, and the
  search field joins it while open; collapse/expand all files is in the tab
  bar (see ui-layout.md).
- Block rows (buffer headers, split-diff spacers) never get the current-line
  highlight: the split's left side draws no header of its own, so a cursor in
  a folded file would otherwise leave a stray band beside the header. A
  hovered buffer header stays opaque (hover tint flattened onto the header
  background) so the split divider doesn't show through it.
- Diagnostics views (project and current file) have no toolbar: they always
  show errors and warnings (no warnings toggle, `include_warnings` setting or
  `ToggleWarnings`), and update live from language server events, so there is
  no refresh/stop button or `ToggleDiagnosticsRefresh`; restarting a language
  server clears and republishes its diagnostics.
- No outline modal ("Go to Symbol in Editor", `outline::Toggle`,
  `cmd-shift-o`) and no outline panel: nothing lists a buffer's symbols.
  Symbols are reached through project symbols and symbol navigation.
- Read-only editors (Git History revisions and commit diffs, read-only
  buffers) share the editing UI but leave out what can only edit: controls
  follow the editor's capability, not the mode it is shown in, and are hidden
  rather than disabled since they can never apply there. Buffer search offers no
  replace (no toggle, row, or `ToggleReplace`/replace-deploy) on any search
  item that is not editable.
- Read-only is a property of the file, never a user toggle. A worktree
  buffer is `Capability::ReadOnly` when the file isn't writable or is a
  library source (there is no `read_only_files` setting); this is re-derived on
  every worktree entry change, so `chmod` takes effect live (entries carry
  `is_writable`). Git revisions and commit views are read-only too.
  There is no "Make Tab Read-Only/Editable" menu entry, no
  `workspace::ToggleReadOnlyFile` action, and no soft-read-only (`Read`)
  `Capability`.
- A read-only item shows a plain lock icon in place of its leading (file)
  icon, the same on every tab kind and on multibuffer excerpt headers;
  trailing slots stay for things the user can act on.
- Multibuffers filter edits per excerpt by each buffer's capability; a
  capability change counts as a non-text state update, so multibuffers
  resnapshot and stop editing that buffer immediately.
- Diagnostics have one per-editor switch, `editor::ToggleDiagnostics`, which turns underlines,
  inline messages, problem navigation and LSP pulls on or off together. Inline
  messages always show while diagnostics are on: there is no
  `editor::ToggleInlineDiagnostics` action and no `diagnostics.inline.enabled`
  setting (the other `diagnostics.inline` settings remain).
- Run/test code lenses are never requested: gutter runnables cover them.
  rust-analyzer isn't told the client supports `rust-analyzer.runSingle` (so
  it omits "Run" / "Run Test" lenses). Lens commands handled client-side are
  limited to showing locations.
- Text inside highlights (search matches, selections, etc.) is recolored to
  keep `minimum_contrast_for_highlights` against the highlight background.
  The Markdown preview applies the same rule to its search matches and
  selection. Both use the shared `ui::utils::highlight_contrast` helpers, so
  anything else that paints text highlights should go through them too.
- Line endings are always LF: new files start as LF and every file is
  converted to LF on save. There is no line-ending setting, selector or
  status bar indicator.
- LSP on-type formatting (e.g. after Enter) waits for pending autoindent: the
  request position and the cursors kept in place by the formatting edits are
  both taken after the indentation is inserted, so the server sees the
  indented column.
- There is no notion of private files or secrets: no `private_files` or
  `redact_private_values` settings, no `redactions.scm` queries, and no
  is-private flag on worktree entries or files. `.env`, key and
  certificate files open, copy, rename and jump-to-symbol like any other file.
  Logs and error messages are not scrubbed for secret-looking environment
  variables; logs stay local.
- Project search include/exclude fields take globs, not fuzzy queries like the
  file finder: they are yes/no filters, and a glob's literal prefix limits which
  gitignored directories are scanned. A pattern without glob syntax matches
  that path at any depth (`editor` matches every `editor` directory or file).

## Markdown

- Markdown rendering, the preview and Mermaid diagrams: see `markdown/`.

## Spell checking

- Spell checking, the bundled dictionaries and word lists: see `spellcheck.md`.
