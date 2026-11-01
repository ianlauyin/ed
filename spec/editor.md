# Editor

## Behaviour

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
- The minimap runs the full card height. Its thumb's top-right and
  bottom-right corners are concentric with the card (card radius − card
  inset), curving in as the thumb nears the top or bottom, instead of
  shortening the minimap to dodge the corners.
- The current-line highlight follows the card's corners the same way (one
  shared `card_corner_radii` rule): square in the middle of the editor,
  concentric with the card on the first/last visible row, curving in as it
  moves away. A partly scrolled-out row is rounded on its visible part. In a
  split diff, the sides that meet at the divider stay square. The minimap's
  current-line band and markers follow the same rule on their right corners
  only (the minimap's left edge is inside the card). A quad too short to carry
  the corner radius (minimap rows, a partly scrolled-out line) is pulled in
  from the card edge to where the corner's curve crosses it, so nothing pokes
  out of the card.
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
- Smooth scrolling is always on, with no setting. Jumps (page/half-page
  scrolls and autoscroll to the cursor: search, go-to-definition, `G`) move
  the scroll anchor at once; only drawing trails it, easing out over 120ms.
  Everything except painting (selections, events, saved position, LSP
  ranges) sees the destination immediately. Jumps of one row snap, so typing
  or stepping past the viewport edge never lags; jumps longer than a screen
  skip ahead and animate only the last screen. A new jump mid-animation starts
  from the drawn position. Both sides of a split diff animate together. Mouse
  and trackpad scrolling are not animated.
- Cursor animation (neovide-style trail) runs 1.5× faster than the
  vscode-neovide-cursor timings it is adapted from (~83ms for a long move).
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
  advertised, and there are no proto messages for it. Use go-to-definition or
  hover to see a function's parameters.
- Code lenses only appear in the hover popover: never as lines in the editor
  or in the code action menu. Lenses whose range covers the hovered position
  (i.e. hovering a definition's name) show as a clickable row at the top of
  the LSP hover popover, above the signature and docs; they're fetched and
  resolved alongside the hover request. Always on whenever a language server
  provides lenses: there is no `code_lens` setting or `editor::ToggleCodeLens`
  action.
- Inlay hints and LSP semantic highlights are always on whenever a language
  server provides them; stop the server (or set `enable_language_server: false`)
  to get rid of them. Semantic tokens always layer on top of tree-sitter
  highlighting, never replace it. There is no `semantic_tokens` setting, no
  `inlay_hints.enabled` / `inlay_hints.toggle_on_modifiers_press`, no
  `editor::ToggleInlayHints` / `editor::ToggleSemanticHighlights` action or
  keybinding. The per-kind `inlay_hints.show_*`
  filters, background and debounce settings remain.
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
  `cmd-shift-o`): the outline panel is the way to browse a buffer's symbols.
- Read-only editors (Git History revisions and commit diffs, read-only
  buffers) share the editing UI but leave out what can only edit: controls
  follow the editor's capability, not the mode it is shown in, and are hidden
  rather than disabled since they can never apply there. Buffer search offers no
  replace (no toggle, row, or `ToggleReplace`/replace-deploy) on any search
  item that is not editable.
- Diagnostics have one per-editor switch, `editor::ToggleDiagnostics`, which turns underlines,
  inline messages, problem navigation and LSP pulls on or off together. Inline
  messages always show while diagnostics are on: there is no
  `editor::ToggleInlineDiagnostics` action and no `diagnostics.inline.enabled`
  setting (the other `diagnostics.inline` settings remain).
- Run/test code lenses are never requested: gutter runnables cover them.
  rust-analyzer isn't told the client supports `rust-analyzer.runSingle` (so
  it omits "Run" / "Run Test" lenses), and gopls's `test` codelens is left at
  its default (off). Lens commands handled client-side are limited to showing
  locations.
- Text inside highlights (search matches, selections, etc.) is recolored to
  keep `minimum_contrast_for_highlights` against the highlight background.
  The Markdown preview applies the same rule to its search matches and
  selection. Both use the shared `ui::utils::highlight_contrast` helpers, so
  anything else that paints text highlights should go through them too.
- Line endings default to `enforce_lf`: every file is converted to LF on
  format/save. There is no line-ending selector or status bar indicator; to
  keep CRLF for specific files, set `line_ending` (e.g. `detect`) per language
  or project.
- LSP on-type formatting (e.g. after Enter) waits for pending autoindent: the
  request position and the cursors kept in place by the formatting edits are
  both taken after the indentation is inserted, so the server sees the
  indented column.
- There is no notion of private files or secrets: no `private_files` or
  `redact_private_values` settings, no `redactions.scm` queries, and no
  is-private flag on worktree entries or files. Over SSH, `.env`, key and
  certificate files open, copy, rename and jump-to-symbol like any other file.
  Logs and error messages are not scrubbed for secret-looking environment
  variables; logs stay local.
- Project search include/exclude fields take globs, not fuzzy queries like the
  file finder: they are yes/no filters, and a glob's literal prefix limits which
  gitignored directories are scanned. A pattern without glob syntax matches
  that path at any depth (`editor` matches every `editor` directory or file).

## Markdown

- Markdown rendering, the preview and Mermaid diagrams: see `markdown/`.

## Tabular data preview

- CSV-like grid preview and column resizing: see `tabular-data-preview.md`.

## Spell checking

- Spell checking, the bundled dictionaries and word lists: see `spellcheck.md`.
