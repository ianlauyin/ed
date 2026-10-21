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
  Singleton editors always reserve gutter space for it.
  The gutter right-click menu has a matching "Copy File Location" entry with
  the same row/selection behaviour.
- No vertical scrollbar: the minimap is the vertical scroll widget. The
  markers the scrollbar used to carry are drawn on the minimap, aligned with
  the minimap row of their line (so clicking a marker lands on it): cursor
  lines as a faint full-width band, git hunks as a thin bar on the left edge,
  search/selection/symbol occurrences and diagnostics as two lanes on the
  right edge. Which markers show is still driven by the `scrollbar.*` marker
  settings. In files longer than the minimap, off-screen markers scroll in
  with the minimap. Editors without a minimap have no vertical scroll widget.
- The minimap runs the full card height. Its thumb's bottom-right corner is
  concentric with the card (card radius − card inset), curving in as the
  thumb nears the bottom, instead of shortening the minimap to dodge the
  corner.
- The horizontal scrollbar matches the `ui` scrollbars (project panel): no
  track, a thin rounded thumb inset in a 14px hitbox.
- Text inside highlights (search matches, selections, etc.) is recolored to
  keep `minimum_contrast_for_highlights` against the highlight background.
  The Markdown preview applies the same rule to its search matches and
  selection. Both use the shared `ui::utils::highlight_contrast` helpers, so
  anything else that paints text highlights should go through them too.
- Line endings default to `enforce_lf`: every file is converted to LF on
  format/save. There is no line-ending selector or status bar indicator; to
  keep CRLF for specific files, set `line_ending` (e.g. `detect`) per language
  or project.

## Markdown diagrams

- Mermaid rendering uses Merman 0.9 with default features disabled. Only the
  diagram families supported by the Markdown preview are compiled: flowchart,
  sequence, class, state, ER, Gantt, pie, git graph, mindmap, timeline, quadrant,
  XY chart, and journey. Cytoscape layout remains available for mindmaps.
- Rendering uses deterministic text measurement and raster-safe SVG output,
  with native text fallbacks for HTML labels. Diagram canvases, text, and accents
  follow the editor theme; upstream CSS cannot override the editor's styling.

## Spell checking

- Built-in, in-process English (en_US) spell checker; no language server,
  no network. The Hunspell dictionary (SCOWL en_US-large) and a list of
  programming terms (from codebook) are bundled in the `spell_check` crate,
  checked with `spellbook`.
- What is checked comes from each grammar's `spellcheck.scm` query:
  `@spell` marks text to check, `@nospell` carves text out, across syntax
  layers. Every bundled grammar has one; a new grammar needs one too.
  - Code (Rust, Go, Python, TypeScript/TSX/JavaScript, Bash): comments,
    string contents, and names where they are defined (functions,
    parameters, variables, types, fields, ...). Uses of names, trait impl
    methods, and module specifiers (imports, `require`) are not checked.
  - Markup/data: Markdown paragraph, heading and table text (code spans,
    link targets, autolinks and HTML tags skipped; fenced code is checked
    by its own language's query); HTML text, comments and `alt`/`title`/
    `placeholder`/`label`/`aria-*` values; CSS comments and class/id names;
    JSON/JSONC/YAML values and comments, not keys (schema-defined);
    `.env` comments only.
  - Git Commit (no grammar): every line except `#` comments and the diff
    below the scissors line. Plain Text: only `.txt` files and untitled
    buffers, since languages without support also open as Plain Text.
- Identifiers are split into camelCase/snake_case words; words under 3
  letters, words with digits, non-Latin scripts, URLs, emails, paths and
  shebangs, hex hashes, UUIDs, base64 and backtick code are skipped.
- Only the visible rows are checked, in the background, after edits,
  reparses and scrolls (debounced). Misspellings are wavy underlines in the
  theme's info color; they are not diagnostics and do not appear in the
  diagnostics panel. Full and auto-height editors only.
- Words are only allowed per project: `spellcheck.txt` at a worktree root
  (one word per line, `#` comments, case-insensitive), used only when it
  exists. There is no global word list. It is read through the project's
  buffer, so unsaved edits apply immediately and remote projects work.
- On a misspelled word, code actions offer up to 5 suggestions and
  "Add … to spellcheck.txt", which inserts the word in case-insensitive
  sorted position (comments stay put), skips words already listed, and
  saves the file (creating it if needed).
