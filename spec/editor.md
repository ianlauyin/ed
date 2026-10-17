# Editor

## Behaviour

- Occurrence highlights — LSP document highlights (read/write symbol
  occurrences) and selected-text occurrence highlights — only run in
  single-file editors. Multibuffers (project search, find references, etc.)
  already highlight their matches, and stacking occurrence highlights on top
  made results unreadable. The selection itself and bracket highlights still
  show everywhere.
- Hovering a gutter row (after a short delay) shows a bookmark-icon button left
  of the line number. Clicking it copies the worktree-relative `path:line` to
  the clipboard, for referencing code in an agent chat. If the newest selection
  spans multiple lines and covers the clicked row, it copies `path:start-end`
  instead (same format as `editor::CopyFileLocation`). After copying, the
  button turns into a green check ("Copied!") for 2s, staying visible even
  after the mouse leaves, matching the shared copy-button feedback. The button is hidden for
  buffers without a worktree file, on rows with a run indicator, on the left
  side of a split diff, and in editors that hide runnables (previews).
  Singleton editors always reserve gutter space for it.
